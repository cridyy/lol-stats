use std::path::PathBuf;

use super::error::{AppError, AppResult};
use super::lcu::LcuClient;
use super::models::ClientAuth;

/// 返回当前登录客户端实际使用的游戏内配置文件。
///
/// 国服的 LCU 安装目录与游戏目录分离，因此需要按 Akari 的路径规则转到
/// `Game/Config/PersistedSettings.json`；直营服则直接位于安装目录的 Config 下。
async fn persisted_settings_path(lcu: &LcuClient, auth: &ClientAuth) -> AppResult<PathBuf> {
    let install_dir = PathBuf::from(lcu.install_dir().await?);
    let config_dir = if auth.region.eq_ignore_ascii_case("TENCENT") {
        install_dir.join("..").join("Game").join("Config")
    } else {
        install_dir.join("Config")
    };
    let settings_path = config_dir.join("PersistedSettings.json");

    if !settings_path.is_file() {
        return Err(AppError::LcuUnavailable(format!(
            "没有找到游戏设置文件：{}",
            settings_path.display()
        )));
    }

    Ok(settings_path)
}

pub async fn game_settings_locked(lcu: &LcuClient, auth: &ClientAuth) -> AppResult<bool> {
    let path = persisted_settings_path(lcu, auth).await?;
    Ok(std::fs::metadata(path)?.permissions().readonly())
}

pub async fn set_game_settings_locked(
    lcu: &LcuClient,
    auth: &ClientAuth,
    locked: bool,
) -> AppResult<bool> {
    let path = persisted_settings_path(lcu, auth).await?;
    let mut permissions = std::fs::metadata(&path)?.permissions();
    permissions.set_readonly(locked);
    std::fs::set_permissions(&path, permissions)?;
    Ok(std::fs::metadata(path)?.permissions().readonly())
}
