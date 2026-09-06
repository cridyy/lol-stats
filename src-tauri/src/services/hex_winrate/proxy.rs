use std::error::Error as _;
use std::fmt::Write as _;
use std::net::IpAddr;
use std::sync::LazyLock;
use std::time::Duration;

use reqwest::{Client, ClientBuilder, Proxy, Response, Url};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

const DEFAULT_PROXY_ADDRESS: &str = "http://127.0.0.1:7890";
const DEFAULT_TIMEOUT_SECONDS: u64 = 20;
pub(super) const MAX_REQUEST_RETRIES: usize = 2;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct AramkitProxySettings {
    pub enabled: bool,
    pub address: String,
    pub wtls_enabled: bool,
    pub timeout_seconds: u64,
}

impl Default for AramkitProxySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            address: DEFAULT_PROXY_ADDRESS.to_string(),
            wtls_enabled: false,
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
        }
    }
}

impl AramkitProxySettings {
    pub(super) fn normalized(&self) -> Result<Self, String> {
        if !(1..=120).contains(&self.timeout_seconds) {
            return Err("超时时间请输入 1 到 120 之间的整数，单位为秒".to_string());
        }
        #[cfg(not(windows))]
        if self.wtls_enabled {
            return Err("WTLS 仅支持 Windows 系统".to_string());
        }
        let address = self.address.trim();
        if !self.enabled {
            return Ok(Self {
                enabled: false,
                address: if address.is_empty() {
                    DEFAULT_PROXY_ADDRESS.to_string()
                } else {
                    address.to_string()
                },
                ..self.clone()
            });
        }

        let url = Url::parse(address)
            .map_err(|_| "代理地址格式不正确，例如 http://127.0.0.1:7890".to_string())?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err("请使用 HTTP/HTTPS 代理地址，填写代理软件的 HTTP 或混合端口".to_string());
        }
        let local_host = url.host_str().is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host == "[::1]"
                || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
        });
        if !local_host {
            return Err(
                "请填写本机代理地址，主机仅支持 localhost、127.0.0.1 或其他回环地址".to_string(),
            );
        }
        if url.port_or_known_default() == Some(0) {
            return Err("代理端口必须在 1 到 65535 之间".to_string());
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err("本地代理地址不能包含账号或密码".to_string());
        }
        if !matches!(url.path(), "" | "/") || url.query().is_some() || url.fragment().is_some() {
            return Err("代理地址只需协议、主机和端口，不要填写路径、查询参数或片段".to_string());
        }

        Ok(Self {
            enabled: true,
            address: url.as_str().trim_end_matches('/').to_string(),
            ..self.clone()
        })
    }

    fn max_retries(&self) -> usize {
        if self.wtls_enabled {
            MAX_REQUEST_RETRIES
        } else {
            0
        }
    }

    fn backend_for_attempt(&self, attempt: usize) -> TlsBackend {
        #[cfg(windows)]
        if self.wtls_enabled && attempt > 0 {
            return TlsBackend::Windows;
        }
        let _ = attempt;
        TlsBackend::Rustls
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TlsBackend {
    Rustls,
    #[cfg(windows)]
    Windows,
}

impl TlsBackend {
    fn label(self) -> &'static str {
        match self {
            Self::Rustls => "Rustls",
            #[cfg(windows)]
            Self::Windows => "WTLS (Schannel)",
        }
    }
}

static SETTINGS: LazyLock<watch::Sender<AramkitProxySettings>> = LazyLock::new(|| {
    let (sender, _) = watch::channel(AramkitProxySettings::default());
    sender
});

pub(crate) fn configure(settings: AramkitProxySettings) -> Result<AramkitProxySettings, String> {
    let settings = settings.normalized()?;
    let changed = SETTINGS.send_if_modified(|current| {
        if *current == settings {
            return false;
        }
        *current = settings.clone();
        true
    });
    if changed {
        tracing::info!(target: "lol_stats::aramkit",
            proxy_enabled = settings.enabled, wtls_enabled = settings.wtls_enabled,
            timeout_seconds = settings.timeout_seconds, "AramKit 请求配置已更新");
    }
    Ok(settings)
}

pub(super) fn subscribe() -> watch::Receiver<AramkitProxySettings> {
    SETTINGS.subscribe()
}

pub(super) fn client() -> Result<AramkitClient, String> {
    let settings = SETTINGS.borrow().clone();
    AramkitClient::new(settings)
}

pub(super) fn client_for_settings(settings: &AramkitProxySettings) -> Result<Client, String> {
    client_for_backend(settings, TlsBackend::Rustls)
}

pub(super) fn client_for_backend(
    settings: &AramkitProxySettings,
    backend: TlsBackend,
) -> Result<Client, String> {
    let builder = client_builder_for_settings(settings)?.http1_only();
    let builder = match backend {
        TlsBackend::Rustls => builder.use_rustls_tls(),
        #[cfg(windows)]
        TlsBackend::Windows => builder.use_native_tls(),
    };
    builder
        .build()
        .map_err(|error| format_request_error("创建 AramKit 请求失败", &error))
}

pub(super) fn client_builder_for_settings(
    settings: &AramkitProxySettings,
) -> Result<ClientBuilder, String> {
    let settings = settings.normalized()?;
    let timeout = Duration::from_secs(settings.timeout_seconds);
    // 编译诊断用的 native-tls 后，reqwest 的默认后端会改变，业务请求必须显式固定。
    let mut builder = Client::builder()
        .use_rustls_tls()
        .retry(reqwest::retry::never())
        .timeout(timeout)
        .connect_timeout(timeout)
        .user_agent("lol-stats hex-winrate-overlay");
    if settings.enabled {
        let proxy = Proxy::all(&settings.address)
            .map_err(|error| format_request_error("无法创建本地代理，请检查代理地址", &error))?;
        // 显式代理只作用于这个客户端；关闭时保留原来的系统/环境代理行为。
        builder = builder.no_proxy().proxy(proxy);
    }
    Ok(builder)
}

pub(super) struct AramkitClient {
    settings: AramkitProxySettings,
    rustls: Client,
    #[cfg(windows)]
    wtls: tokio::sync::OnceCell<Client>,
}

impl AramkitClient {
    fn new(settings: AramkitProxySettings) -> Result<Self, String> {
        let settings = settings.normalized()?;
        let rustls = client_for_settings(&settings)?;
        Ok(Self {
            settings,
            rustls,
            #[cfg(windows)]
            wtls: tokio::sync::OnceCell::new(),
        })
    }

    async fn transport(&self, backend: TlsBackend) -> Result<&Client, String> {
        match backend {
            TlsBackend::Rustls => Ok(&self.rustls),
            #[cfg(windows)]
            TlsBackend::Windows => {
                self.wtls
                    .get_or_try_init(|| async {
                        client_for_backend(&self.settings, TlsBackend::Windows)
                    })
                    .await
            }
        }
    }

    async fn get_bytes(&self, url: &str, context: &str) -> Result<Vec<u8>, String> {
        let max_retries = self.settings.max_retries();
        let mut failures = Vec::new();
        for attempt in 0..=max_retries {
            let backend = self.settings.backend_for_attempt(attempt);
            let attempt_number = attempt + 1;
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
            }
            let result = match self.transport(backend).await {
                Ok(client) => read_bytes(client, url, context).await,
                Err(message) => Err(RequestFailure {
                    message,
                    retryable: false,
                }),
            };
            match result {
                Ok(bytes) => {
                    if attempt > 0 {
                        tracing::info!(target: "lol_stats::aramkit",
                            tls = backend.label(), attempt = attempt_number,
                            timeout_seconds = self.settings.timeout_seconds,
                            "AramKit 兼容请求成功：{context}");
                    }
                    return Ok(bytes);
                }
                Err(error) => {
                    let retry = error.retryable && attempt < max_retries;
                    tracing::warn!(target: "lol_stats::aramkit",
                        tls = backend.label(), attempt = attempt_number,
                        timeout_seconds = self.settings.timeout_seconds, retry,
                        error = %error.message, "AramKit 请求失败：{context}");
                    failures.push(format!(
                        "第 {attempt_number} 次 / {} / 超时 {} 秒：\n{}",
                        backend.label(),
                        self.settings.timeout_seconds,
                        error.message
                    ));
                    if !retry {
                        break;
                    }
                }
            }
        }
        Err(failures.join("\n\n"))
    }

    pub(super) async fn get_text(&self, url: &str, context: &str) -> Result<String, String> {
        let bytes = self.get_bytes(url, context).await?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    pub(super) async fn get_json<T: DeserializeOwned>(
        &self,
        url: &str,
        context: &str,
    ) -> Result<T, String> {
        let bytes = self.get_bytes(url, context).await?;
        serde_json::from_slice(&bytes)
            .map_err(|error| format!("{context}，解析 JSON 失败：{error}"))
    }
}

struct RequestFailure {
    message: String,
    retryable: bool,
}

pub(super) fn retryable_request_error(error: &reqwest::Error) -> bool {
    error.is_timeout()
        || error.is_connect()
        || error.is_request()
        || error.is_body()
        || error.is_decode()
}

pub(super) fn retryable_response(response: &Response) -> bool {
    matches!(response.status().as_u16(), 408 | 500 | 502 | 503 | 504)
        && !response
            .headers()
            .contains_key(reqwest::header::RETRY_AFTER)
}

async fn read_bytes(client: &Client, url: &str, context: &str) -> Result<Vec<u8>, RequestFailure> {
    let request_error = |error: reqwest::Error| RequestFailure {
        retryable: retryable_request_error(&error),
        message: format_request_error(&format!("{context}失败"), &error),
    };
    let response = client.get(url).send().await.map_err(request_error)?;
    if !response.status().is_success() {
        return Err(RequestFailure {
            retryable: retryable_response(&response),
            message: format!("{context}返回 HTTP {}", response.status()),
        });
    }
    // 必须读完整个正文才算成功，HTTP 200 后的超时/断流也参与 TLS 兜底。
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(request_error)
}

pub(super) fn format_request_error(context: &str, error: &reqwest::Error) -> String {
    let mut message = format!("{context}：{error}");
    let mut cause = error.source();
    let mut depth = 1;
    // Display 只显示请求的外层描述，连接、代理隧道、TLS 和系统错误位于 source 链中。
    while let Some(source) = cause {
        let _ = write!(message, "\n原因 {depth}：{source}");
        cause = source.source();
        depth += 1;
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Instant;

    fn enabled_settings(address: &str) -> AramkitProxySettings {
        AramkitProxySettings {
            enabled: true,
            address: address.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn proxy_is_disabled_by_default() {
        let settings: AramkitProxySettings = serde_json::from_str("{}").unwrap();
        assert!(!settings.enabled);
        assert_eq!(settings.address, DEFAULT_PROXY_ADDRESS);
        assert!(!settings.wtls_enabled);
        assert_eq!(settings.timeout_seconds, 20);
    }

    #[test]
    fn old_proxy_settings_gain_wtls_defaults_without_losing_the_port() {
        let settings: AramkitProxySettings =
            serde_json::from_str(r#"{"enabled":true,"address":"http://127.0.0.1:7897"}"#).unwrap();
        let settings = settings.normalized().unwrap();
        assert_eq!(settings.address, "http://127.0.0.1:7897");
        assert!(!settings.wtls_enabled);
        assert_eq!(settings.timeout_seconds, 20);
        assert_eq!(settings.max_retries(), 0);
    }

    #[test]
    fn timeout_has_a_finite_valid_range() {
        for seconds in [0, 121, u64::MAX] {
            let settings = AramkitProxySettings {
                timeout_seconds: seconds,
                ..Default::default()
            };
            assert!(settings.normalized().is_err());
        }
        for seconds in [1, 20, 45, 120] {
            let settings = AramkitProxySettings {
                timeout_seconds: seconds,
                ..Default::default()
            };
            assert_eq!(settings.normalized().unwrap().timeout_seconds, seconds);
        }
    }

    #[cfg(windows)]
    #[test]
    fn wtls_is_only_used_after_the_first_rustls_attempt() {
        let settings = AramkitProxySettings {
            wtls_enabled: true,
            ..Default::default()
        };
        assert_eq!(settings.max_retries(), 2);
        assert_eq!(settings.backend_for_attempt(0), TlsBackend::Rustls);
        assert_eq!(settings.backend_for_attempt(1), TlsBackend::Windows);
        assert_eq!(settings.backend_for_attempt(2), TlsBackend::Windows);
    }

    #[test]
    fn normalizes_local_http_proxy_addresses() {
        for address in [
            "http://127.0.0.1:7890",
            "http://localhost:10809",
            "https://127.0.0.2:8080",
            "http://[::1]:7890",
        ] {
            assert_eq!(
                enabled_settings(&format!(" {address}/ "))
                    .normalized()
                    .unwrap()
                    .address,
                address
            );
        }
    }

    #[test]
    fn rejects_non_local_or_invalid_proxy_addresses() {
        for address in [
            "",
            "127.0.0.1:7890",
            "http://example.com:7890",
            "http://192.168.1.1:7890",
            "http://localhost.example.com:7890",
            "socks5://127.0.0.1:7890",
            "http://127.0.0.1:0",
            "http://127.0.0.1:65536",
            "http://127.0.0.1:7890/path",
            "http://127.0.0.1:7890?query=1",
            "http://127.0.0.1:7890#fragment",
            "http://user:password@127.0.0.1:7890",
        ] {
            assert!(enabled_settings(address).normalized().is_err(), "{address}");
        }
    }

    #[test]
    fn disabling_proxy_is_allowed_even_when_the_address_is_invalid() {
        let settings = AramkitProxySettings {
            enabled: false,
            address: " invalid ".to_string(),
            ..Default::default()
        }
        .normalized()
        .unwrap();
        assert!(!settings.enabled);
        assert_eq!(settings.address, "invalid");
    }

    fn mock_proxy(response: &'static str) -> (String, thread::JoinHandle<Vec<String>>) {
        mock_proxy_responses(vec![response])
    }

    fn mock_proxy_responses(
        responses: Vec<&'static str>,
    ) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let task = thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "proxy was not contacted");
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    socket.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                    assert!(request.len() < 8192);
                }
                socket.write_all(response.as_bytes()).unwrap();
                requests.push(String::from_utf8(request).unwrap());
            }
            requests
        });
        (address, task)
    }

    #[tokio::test]
    async fn http_request_uses_the_explicit_local_proxy() {
        let (address, task) =
            mock_proxy("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK");
        let result = client_for_settings(&enabled_settings(&address))
            .unwrap()
            .get("http://aramkit.invalid/proxy-test")
            .timeout(Duration::from_secs(3))
            .send()
            .await;
        let request = task.join().unwrap();
        assert!(request[0].starts_with("GET http://aramkit.invalid/proxy-test HTTP/1.1\r\n"));
        assert_eq!(result.unwrap().text().await.unwrap(), "OK");
    }

    #[tokio::test]
    async fn https_request_uses_connect_without_direct_fallback() {
        let (address, task) = mock_proxy(
            "HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        let result = client_for_settings(&enabled_settings(&address))
            .unwrap()
            .get("https://aramkit.invalid/proxy-test")
            .timeout(Duration::from_secs(3))
            .send()
            .await;
        let request = task.join().unwrap();
        assert!(request[0].starts_with("CONNECT aramkit.invalid:443 HTTP/1.1\r\n"));
        let error = result.unwrap_err();
        let message = format_request_error("读取 AramKit 数据版本失败", &error);
        assert!(message.contains("https://aramkit.invalid/proxy-test"));
        let mut cause = std::error::Error::source(&error);
        let mut depth = 1;
        while let Some(source) = cause {
            assert!(message.contains(&format!("原因 {depth}：{source}")));
            cause = source.source();
            depth += 1;
        }
        assert!(depth > 2, "应输出代理隧道的底层错误：{message}");
        println!("{message}");
    }

    #[tokio::test]
    async fn proxy_connection_failure_includes_the_underlying_error() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);

        let error = client_for_settings(&enabled_settings(&address))
            .unwrap()
            .get("https://aramkit.invalid/proxy-test")
            .timeout(Duration::from_secs(3))
            .send()
            .await
            .unwrap_err();
        assert!(error.is_connect());
        let message = format_request_error("读取 AramKit 数据版本失败", &error);
        assert!(message.contains("原因 1："));
        assert!(message.contains("原因 2："));
        let mut root = std::error::Error::source(&error).unwrap();
        while let Some(source) = root.source() {
            root = source;
        }
        assert!(message.contains(&root.to_string()));
        println!("{message}");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn wtls_retries_include_a_failure_after_http_200() {
        let (address, task) = mock_proxy_responses(vec![
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nshort",
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK",
        ]);
        let client = AramkitClient::new(AramkitProxySettings {
            wtls_enabled: true,
            ..enabled_settings(&address)
        })
        .unwrap();
        let result = client
            .get_text("http://aramkit.invalid/data", "测试正文重试")
            .await;
        assert_eq!(task.join().unwrap().len(), 3);
        assert_eq!(result.unwrap(), "OK");
        assert!(client.wtls.get().is_some());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn wtls_stops_after_two_retries() {
        let (address, task) = mock_proxy_responses(vec![
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"; 3
        ]);
        let client = AramkitClient::new(AramkitProxySettings {
            wtls_enabled: true,
            ..enabled_settings(&address)
        })
        .unwrap();
        let result = client
            .get_text("http://aramkit.invalid/data", "测试重试上限")
            .await;
        assert_eq!(task.join().unwrap().len(), 3);
        let error = result.unwrap_err();
        assert!(error.contains("第 3 次 / WTLS"));
        assert!(!error.contains("第 4 次"));
    }

    #[tokio::test]
    async fn disabled_wtls_keeps_one_request_per_chain() {
        let (address, task) = mock_proxy(
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        let client = AramkitClient::new(enabled_settings(&address)).unwrap();
        let result = client
            .get_text("http://aramkit.invalid/data", "测试关闭兜底")
            .await;
        assert_eq!(task.join().unwrap().len(), 1);
        assert!(!result.unwrap_err().contains("第 2 次"));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_successful_rustls_request_does_not_initialize_wtls() {
        let (address, task) =
            mock_proxy("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK");
        let client = AramkitClient::new(AramkitProxySettings {
            wtls_enabled: true,
            ..enabled_settings(&address)
        })
        .unwrap();
        let result = client
            .get_text("http://aramkit.invalid/data", "测试首选成功")
            .await;
        assert_eq!(task.join().unwrap().len(), 1);
        assert_eq!(result.unwrap(), "OK");
        assert!(client.wtls.get().is_none());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn non_transient_statuses_do_not_repeat_the_request() {
        for response in [
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 503 Service Unavailable\r\nRetry-After: 60\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ] {
            let (address, task) = mock_proxy(response);
            let client = AramkitClient::new(AramkitProxySettings {
                wtls_enabled: true, ..enabled_settings(&address)
            }).unwrap();
            let result = client.get_text("http://aramkit.invalid/data", "测试不可重试状态").await;
            assert_eq!(task.join().unwrap().len(), 1);
            assert!(result.is_err());
            assert!(client.wtls.get().is_none());
        }
    }
}
