//! 仅供本机脚本调用的软件视图数据接口。
//!
//! 评分、称号和画像的权威实现位于前端 TypeScript，因此本模块只负责 HTTP
//! 入口和请求桥接，不在 Rust 中复制一套容易漂移的评分公式。

use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub const LOCAL_API_ADDRESS: &str = "127.0.0.1:17390";
const LOCAL_API_REQUEST_EVENT: &str = "local-api-request";
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(180);
const HEADER_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_HEADER_LINE_BYTES: usize = 8 * 1024;
const MAX_QUERY_TARGET_BYTES: usize = 4 * 1024;
const MAX_GAME_COUNT: usize = 1000;

type BridgeResult = Result<Value, String>;

#[derive(Clone, Default)]
pub struct LocalApiState {
    inner: Arc<LocalApiInner>,
}

#[derive(Default)]
struct LocalApiInner {
    frontend_ready: AtomicBool,
    request_in_flight: AtomicBool,
    next_request_id: AtomicU64,
    pending: Mutex<HashMap<String, SyncSender<BridgeResult>>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalApiRequest {
    pub request_id: String,
    pub game_name: String,
    pub tag_line: String,
    pub server: String,
    pub mode: String,
    pub count: usize,
}

impl LocalApiState {
    pub fn set_frontend_ready(&self, ready: bool) {
        self.inner.frontend_ready.store(ready, Ordering::Release);
    }

    pub fn complete_request(&self, request_id: &str, data: Option<Value>, error: Option<String>) {
        let sender = self
            .inner
            .pending
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(request_id));

        let Some(sender) = sender else {
            // 请求已超时或软件正在退出时，前端的迟到结果无需再反馈错误。
            return;
        };

        let result = match error.filter(|message| !message.trim().is_empty()) {
            Some(message) => Err(message),
            None => data.ok_or_else(|| "前端没有返回接口数据".to_string()),
        };
        let _ = sender.send(result);
    }

    fn frontend_ready(&self) -> bool {
        self.inner.frontend_ready.load(Ordering::Acquire)
    }

    fn try_begin_request(&self) -> bool {
        self.inner
            .request_in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    fn finish_request(&self) {
        self.inner.request_in_flight.store(false, Ordering::Release);
    }

    fn next_request_id(&self) -> String {
        let sequence = self.inner.next_request_id.fetch_add(1, Ordering::Relaxed) + 1;
        format!("local-api-{sequence}")
    }

    fn dispatch(&self, app: &AppHandle, request: LocalApiRequest) -> Result<Value, LocalApiError> {
        if !self.frontend_ready() {
            return Err(LocalApiError::Unavailable(
                "软件界面尚未初始化，稍后重试".to_string(),
            ));
        }
        if !self.try_begin_request() {
            return Err(LocalApiError::Busy);
        }

        let result = self.dispatch_inner(app, request);
        self.finish_request();
        result
    }

    fn dispatch_inner(
        &self,
        app: &AppHandle,
        request: LocalApiRequest,
    ) -> Result<Value, LocalApiError> {
        let (sender, receiver) = sync_channel::<BridgeResult>(1);
        {
            let mut pending = self
                .inner
                .pending
                .lock()
                .map_err(|_| LocalApiError::Internal("本地接口状态不可用".to_string()))?;
            pending.insert(request.request_id.clone(), sender);
        }

        if let Err(error) = app.emit(LOCAL_API_REQUEST_EVENT, &request) {
            self.remove_pending(&request.request_id);
            return Err(LocalApiError::Internal(format!(
                "无法派发前端请求：{error}"
            )));
        }

        match receiver.recv_timeout(RESPONSE_TIMEOUT) {
            Ok(Ok(payload)) => Ok(payload),
            Ok(Err(message)) => Err(LocalApiError::Upstream(message)),
            Err(RecvTimeoutError::Timeout) => {
                self.remove_pending(&request.request_id);
                Err(LocalApiError::Timeout)
            }
            Err(RecvTimeoutError::Disconnected) => {
                self.remove_pending(&request.request_id);
                Err(LocalApiError::Internal("前端评分桥接已断开".to_string()))
            }
        }
    }

    fn remove_pending(&self, request_id: &str) {
        if let Ok(mut pending) = self.inner.pending.lock() {
            pending.remove(request_id);
        }
    }
}

pub fn start_local_api_server(app: AppHandle, state: LocalApiState) -> Result<(), String> {
    let listener = TcpListener::bind(LOCAL_API_ADDRESS)
        .map_err(|error| format!("本地接口无法监听 {LOCAL_API_ADDRESS}：{error}"))?;

    thread::Builder::new()
        .name("lol-stats-local-api".to_string())
        .spawn(move || {
            for connection in listener.incoming() {
                match connection {
                    Ok(stream) => {
                        let app = app.clone();
                        let state = state.clone();
                        let _ = thread::Builder::new()
                            .name("lol-stats-local-api-client".to_string())
                            .spawn(move || handle_connection(stream, app, state));
                    }
                    Err(error) => {
                        tracing::warn!(target: "lol_stats::local_api", %error, "接收连接失败");
                    }
                }
            }
        })
        .map_err(|error| format!("本地接口线程启动失败：{error}"))?;

    Ok(())
}

fn handle_connection(mut stream: TcpStream, app: AppHandle, state: LocalApiState) {
    let _ = stream.set_read_timeout(Some(HEADER_TIMEOUT));
    let _ = stream.set_write_timeout(Some(HEADER_TIMEOUT));

    let response = read_http_request(&stream).and_then(|request| match request.path.as_str() {
        "/health" => Ok(json!({
            "ok": true,
            "apiVersion": 1,
            "address": LOCAL_API_ADDRESS,
            "ready": state.frontend_ready(),
        })),
        "/v1/player-history" => {
            if request.origin.is_some() {
                return Err(LocalApiError::Forbidden);
            }
            let mut local_request = parse_player_history_request(&request.query)?;
            local_request.request_id = state.next_request_id();
            state.dispatch(&app, local_request)
        }
        _ => Err(LocalApiError::NotFound),
    });

    match response {
        Ok(payload) => write_json(&mut stream, 200, json!({ "ok": true, "data": payload })),
        Err(error) => write_json(
            &mut stream,
            error.status_code(),
            json!({
                "ok": false,
                "error": {
                    "code": error.code(),
                    "message": error.message(),
                }
            }),
        ),
    }
}

struct HttpRequest {
    path: String,
    query: String,
    origin: Option<String>,
}

fn read_http_request(stream: &TcpStream) -> Result<HttpRequest, LocalApiError> {
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .map_err(|error| LocalApiError::Internal(format!("读取连接失败：{error}")))?,
    );
    let mut request_line = String::new();
    read_limited_line(&mut reader, &mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let version = parts.next().unwrap_or_default();

    if method != "GET" {
        return Err(LocalApiError::MethodNotAllowed);
    }
    if !version.starts_with("HTTP/") || target.is_empty() || target.len() > MAX_QUERY_TARGET_BYTES {
        return Err(LocalApiError::BadRequest("请求格式无效".to_string()));
    }

    let mut origin = None;
    loop {
        let mut line = String::new();
        read_limited_line(&mut reader, &mut line)?;
        if line.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("origin") {
                origin = Some(value.trim().to_string());
            }
        }
    }

    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    Ok(HttpRequest {
        path: path.to_string(),
        query: query.to_string(),
        origin,
    })
}

fn read_limited_line(
    reader: &mut BufReader<TcpStream>,
    line: &mut String,
) -> Result<(), LocalApiError> {
    let bytes = reader
        .read_line(line)
        .map_err(|error| LocalApiError::BadRequest(format!("读取请求失败：{error}")))?;
    if bytes == 0 {
        return Err(LocalApiError::BadRequest("请求内容为空".to_string()));
    }
    if bytes > MAX_HEADER_LINE_BYTES {
        return Err(LocalApiError::BadRequest("请求头过长".to_string()));
    }
    Ok(())
}

fn parse_player_history_request(query: &str) -> Result<LocalApiRequest, LocalApiError> {
    let params = parse_query(query)?;
    let game_name = required_text(&params, "gameName", 64)?;
    let tag_line = required_text(&params, "tagLine", 32)?;
    if game_name.contains('#') || tag_line.contains('#') {
        return Err(LocalApiError::BadRequest(
            "gameName 和 tagLine 需要分开传入，且不能包含 #".to_string(),
        ));
    }

    let server = normalize_server(required_text(&params, "server", 32)?)?;
    let mode = normalize_mode(required_text(&params, "mode", 32)?)?;
    let count = required_text(&params, "count", 8)?
        .parse::<usize>()
        .map_err(|_| LocalApiError::BadRequest("count 必须是数字".to_string()))?;
    if !(1..=MAX_GAME_COUNT).contains(&count) {
        return Err(LocalApiError::BadRequest(format!(
            "count 需要在 1 到 {MAX_GAME_COUNT} 之间"
        )));
    }

    Ok(LocalApiRequest {
        request_id: String::new(),
        game_name,
        tag_line,
        server,
        mode,
        count,
    })
}

fn parse_query(query: &str) -> Result<HashMap<String, String>, LocalApiError> {
    if query.len() > MAX_QUERY_TARGET_BYTES {
        return Err(LocalApiError::BadRequest("查询参数过长".to_string()));
    }

    let mut params = HashMap::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = urlencoding::decode(key)
            .map_err(|_| LocalApiError::BadRequest("查询参数编码无效".to_string()))?
            .into_owned();
        let value = urlencoding::decode(value)
            .map_err(|_| LocalApiError::BadRequest("查询参数编码无效".to_string()))?
            .into_owned();
        params.insert(key, value);
    }
    Ok(params)
}

fn required_text(
    params: &HashMap<String, String>,
    key: &str,
    max_chars: usize,
) -> Result<String, LocalApiError> {
    let value = params
        .get(key)
        .map(String::as_str)
        .unwrap_or_default()
        .trim();
    if value.is_empty() {
        return Err(LocalApiError::BadRequest(format!("缺少参数 {key}")));
    }
    if value.chars().count() > max_chars || value.chars().any(char::is_control) {
        return Err(LocalApiError::BadRequest(format!("参数 {key} 格式无效")));
    }
    Ok(value.to_string())
}

fn normalize_server(raw: String) -> Result<String, LocalApiError> {
    let server = raw.to_ascii_uppercase();
    let suffix = server.strip_prefix("TENCENT_").unwrap_or_default();
    if suffix.is_empty()
        || !suffix
            .chars()
            .all(|character| character.is_ascii_uppercase() || character.is_ascii_digit())
    {
        return Err(LocalApiError::BadRequest(
            "server 需要是具体国服区服，例如 TENCENT_HN1".to_string(),
        ));
    }
    Ok(server)
}

fn normalize_mode(raw: String) -> Result<String, LocalApiError> {
    let normalized = raw.trim().to_ascii_lowercase();
    let mode = match normalized.as_str() {
        "hex-aram" | "hexaram" | "2400" | "海克斯大乱斗" => "hex-aram",
        "aram" | "450" | "大乱斗" => "aram",
        "ranked" | "排位" => "ranked",
        "normal" | "匹配" => "normal",
        "arena" | "斗魂竞技场" => "arena",
        "all" | "全部" => "all",
        _ => {
            return Err(LocalApiError::BadRequest(
                "mode 支持 hex-aram、aram、ranked、normal、arena 或 all".to_string(),
            ))
        }
    };
    Ok(mode.to_string())
}

fn write_json(stream: &mut TcpStream, status: u16, payload: Value) {
    let body = match serde_json::to_vec(&payload) {
        Ok(value) => value,
        Err(_) => "{\"ok\":false,\"error\":{\"code\":\"serialization_failed\",\"message\":\"响应序列化失败\"}}"
            .as_bytes()
            .to_vec(),
    };
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Internal Server Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}

#[derive(Debug)]
enum LocalApiError {
    BadRequest(String),
    MethodNotAllowed,
    Forbidden,
    NotFound,
    Unavailable(String),
    Busy,
    Timeout,
    Upstream(String),
    Internal(String),
}

impl LocalApiError {
    fn status_code(&self) -> u16 {
        match self {
            Self::BadRequest(_) => 400,
            Self::MethodNotAllowed => 405,
            Self::Forbidden => 403,
            Self::NotFound => 404,
            Self::Unavailable(_) => 503,
            Self::Busy => 409,
            Self::Timeout => 504,
            Self::Upstream(_) => 502,
            Self::Internal(_) => 500,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "invalid_request",
            Self::MethodNotAllowed => "method_not_allowed",
            Self::Forbidden => "browser_request_rejected",
            Self::NotFound => "not_found",
            Self::Unavailable(_) => "frontend_not_ready",
            Self::Busy => "request_in_progress",
            Self::Timeout => "request_timeout",
            Self::Upstream(_) => "query_failed",
            Self::Internal(_) => "internal_error",
        }
    }

    fn message(&self) -> String {
        match self {
            Self::BadRequest(message)
            | Self::Unavailable(message)
            | Self::Upstream(message)
            | Self::Internal(message) => message.clone(),
            Self::MethodNotAllowed => "仅支持 GET 请求".to_string(),
            Self::Forbidden => "网页跨域请求已拒绝，请通过本地脚本调用".to_string(),
            Self::NotFound => "接口不存在".to_string(),
            Self::Busy => "已有接口请求正在执行，请等待完成后再试".to_string(),
            Self::Timeout => "等待软件计算结果超时".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_aram_request() {
        let request = parse_player_history_request(
            "gameName=%E6%B5%8B%E8%AF%95&tagLine=12345&server=tencent_hn1&mode=2400&count=50",
        )
        .expect("请求应可解析");

        assert_eq!(request.game_name, "测试");
        assert_eq!(request.server, "TENCENT_HN1");
        assert_eq!(request.mode, "hex-aram");
        assert_eq!(request.count, 50);
    }

    #[test]
    fn reject_invalid_mode_and_count() {
        let invalid_mode = parse_player_history_request(
            "gameName=a&tagLine=1&server=TENCENT_HN1&mode=unknown&count=1",
        );
        assert!(matches!(invalid_mode, Err(LocalApiError::BadRequest(_))));

        let invalid_count = parse_player_history_request(
            "gameName=a&tagLine=1&server=TENCENT_HN1&mode=all&count=1001",
        );
        assert!(matches!(invalid_count, Err(LocalApiError::BadRequest(_))));
    }
}
