use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("网络错误: {0}")]
    Network(#[from] reqwest::Error),
    #[error("登录状态已过期")]
    Unauthorized,
    #[error("服务器错误 {status}: {body}")]
    Server { status: u16, body: String },
}

pub type AppResult<T> = Result<T, AppError>;

/// 检查响应状态：401 -> Unauthorized（显式凭证失败要大声失败），
/// 其他非 2xx -> Server。调用方拿 .json() 时 reqwest::Error 自动归 Network。
pub async fn check(resp: reqwest::Response) -> AppResult<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    if status.as_u16() == 401 {
        return Err(AppError::Unauthorized);
    }
    let body = resp.text().await.unwrap_or_default();
    Err(AppError::Server { status: status.as_u16(), body: body.chars().take(200).collect() })
}

/// 路径段 percent-encode：kugou 歌曲 id 形如 `hash|albumAudio_id|album_id`，
/// `|` 必须编码后才能拼进 /api/music/lyrics/:id。
pub fn encode_path_segment(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}
