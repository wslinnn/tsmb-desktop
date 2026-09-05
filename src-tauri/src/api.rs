use crate::http::{check, encode_path_segment, AppResult};
use crate::types::{BotStatus, ElapsedInfo, LoginResp, LyricLine};

/// REST 封装。client/base/token 由调用方（AppState）提供。

pub async fn login(
    client: &reqwest::Client,
    base: &str,
    username: &str,
    password: &str,
) -> AppResult<LoginResp> {
    let resp = client
        .post(format!("{base}/api/client/login"))
        .json(&serde_json::json!({ "username": username, "password": password }))
        .send()
        .await?;
    check(resp).await?.json().await.map_err(Into::into)
}

/// 登出是 best-effort：即使网络失败也继续本地清理。
pub async fn logout(client: &reqwest::Client, base: &str, token: &str) {
    let _ = client
        .delete(format!("{base}/api/client/session"))
        .bearer_auth(token)
        .send()
        .await;
}

pub async fn get_bots(
    client: &reqwest::Client,
    base: &str,
    token: &str,
) -> AppResult<Vec<BotStatus>> {
    let resp = client
        .get(format!("{base}/api/bot"))
        .bearer_auth(token)
        .send()
        .await?;
    let v: serde_json::Value = check(resp).await?.json().await?;
    // 响应形如 { "bots": [...] }
    Ok(serde_json::from_value(v.get("bots").cloned().unwrap_or(serde_json::Value::Null))
        .unwrap_or_default())
}

pub async fn get_elapsed(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    bot_id: &str,
) -> AppResult<ElapsedInfo> {
    let resp = client
        .get(format!("{base}/api/player/{}/elapsed", encode_path_segment(bot_id)))
        .bearer_auth(token)
        .send()
        .await?;
    check(resp).await?.json().await.map_err(Into::into)
}

pub async fn get_lyrics(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    platform: &str,
    song_id: &str,
) -> AppResult<Vec<LyricLine>> {
    let resp = client
        .get(format!(
            "{base}/api/music/lyrics/{}?platform={}",
            encode_path_segment(song_id),
            encode_path_segment(platform)
        ))
        .bearer_auth(token)
        .send()
        .await?;
    let v: serde_json::Value = check(resp).await?.json().await?;
    Ok(serde_json::from_value(v.get("lyrics").cloned().unwrap_or(serde_json::Value::Null))
        .unwrap_or_default())
}
