use serde::{Deserialize, Serialize};

/// 与后端契约对齐的镜像类型（teamspeak-music-bot src/bot/instance.ts、
/// src/music/provider.ts）。serde 全部 camelCase + 宽松默认值：后端加字段
/// 不破坏反序列化。

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Song {
    pub id: String,
    pub platform: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub duration: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BotStatus {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub connected: bool,
    #[serde(default)]
    pub playing: bool,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub current_song: Option<Song>,
    #[serde(default)]
    pub queue_size: i64,
    #[serde(default)]
    pub volume: i64,
    #[serde(default)]
    pub play_mode: String,
    #[serde(default)]
    pub elapsed: f64,
    #[serde(default)]
    pub effective_duration: Option<f64>,
}

impl BotStatus {
    /// 是否在推进播放（暂停时后端 playing 仍可能为 true，elapsed 冻结）。
    pub fn is_progressing(&self) -> bool {
        self.playing && !self.paused
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LyricLine {
    pub time: f64,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roma: Option<String>,
}

/// GET /api/player/:botId/elapsed 的响应。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElapsedInfo {
    #[serde(default)]
    pub elapsed: f64,
    #[serde(default)]
    pub playing: bool,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub effective_duration: Option<f64>,
}

/// POST /api/client/login 的响应。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginResp {
    pub token: String,
    #[serde(default)]
    pub expires_at: f64,
    #[serde(default)]
    pub username: String,
}

/// 歌曲唯一键（对齐 Web 端 watch 键 `${platform}:${id}`）。
pub fn song_key(song: &Song) -> String {
    format!("{}:{}", song.platform, song.id)
}
