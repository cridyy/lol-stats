use thiserror::Error;

/// 应用内部统一错误类型。
///
/// Tauri 命令会把它转换成字符串返回给前端；后端模块之间仍然保留结构化错误，
/// 方便后续把错误映射为更细的 UI 状态。
#[derive(Debug, Error)]
pub enum AppError {
    #[error("没有发现正在运行的 LeagueClientUx.exe")]
    ClientNotFound,

    #[error("发现了客户端进程，但没有读到可用的 LCU 连接参数")]
    ClientAuthNotFound,

    #[error("客户端接口请求失败：{0}")]
    LcuUnavailable(String),

    #[error("输入格式不正确：{0}")]
    InvalidInput(String),

    #[error("Riot Client 接口不可用：{0}")]
    RiotClientUnavailable(String),

    #[error("深战绩服务暂时不可用：{0}")]
    SgpUnavailable(String),

    #[error("没有找到玩家：{0}")]
    PlayerNotFound(String),

    #[error("当前阶段没有可读取的对局")]
    LiveGameUnavailable,

    #[error("已取消加载")]
    Cancelled,

    #[error("网络请求失败：{0}")]
    Http(#[from] reqwest::Error),

    #[error("Windows API 调用失败：{0}")]
    Windows(String),
}

pub type AppResult<T> = Result<T, AppError>;
