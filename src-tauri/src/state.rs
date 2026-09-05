use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::{Mutex, RwLock};

use crate::settings::Settings;
use crate::timing::TimingAnchor;
use crate::types::{song_key, BotStatus, LyricLine};

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum AuthPhase {
    LoggedIn,
    LoggedOut,
}

#[derive(Clone, Debug, Serialize)]
pub struct AuthSnapshot {
    pub state: AuthPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum WsPhase {
    Connecting,
    Open,
    Retrying,
    Closed,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConnSnapshot {
    pub ws: WsPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ConnSnapshot {
    pub fn new(ws: WsPhase, error: Option<String>) -> Self {
        Self { ws, error }
    }
}

/// 进程级共享状态。字段全部小、更新快，读写锁粒度按字段拆分；
/// 锚点/歌词缓存按 botId / songKey 分键（对齐 Web 端 timings 结构）。
pub struct AppState {
    pub http: reqwest::Client,
    pub settings: RwLock<Settings>,
    pub auth: RwLock<AuthSnapshot>,
    pub conn: RwLock<ConnSnapshot>,
    pub bots: RwLock<Vec<BotStatus>>,
    pub timings: Mutex<HashMap<String, TimingAnchor>>,
    pub lyrics_cache: Mutex<HashMap<String, Arc<Vec<LyricLine>>>>,
    /// 每 bot 的歌词请求代数：切歌后旧响应作废。
    pub lyrics_gen: Mutex<HashMap<String, u64>>,
    /// 最近一次歌词数据快照：歌词窗口加载晚于广播时会错过 lyrics-data，
    /// get_state 里补发（水合契约的一部分）。
    pub last_lyrics: Mutex<Option<crate::events::LyricsDataEvent>>,
    /// 登录态版本号：login/logout/watch 驱动 ws 任务重连。
    pub auth_rev: tokio::sync::watch::Sender<u64>,
}

pub type SharedState = Arc<AppState>;

impl AppState {
    pub fn new(settings: Settings) -> Self {
        let (auth_rev, _) = tokio::sync::watch::channel(0);
        let has_token = settings.auth.token.as_deref().map_or(false, |t| !t.is_empty());
        // 乐观登录：有 token 即 LoggedIn，由 WS 4001 / REST 401 纠正
        //（服务器暂时不可达不应导致桌面端登出）。
        let auth = AuthSnapshot {
            state: if has_token { AuthPhase::LoggedIn } else { AuthPhase::LoggedOut },
            username: settings.auth.username.clone(),
            server: if settings.server.base_url.is_empty() {
                None
            } else {
                Some(settings.server.base_url.clone())
            },
            reason: None,
        };
        Self {
            http: reqwest::Client::new(),
            settings: RwLock::new(settings),
            auth: RwLock::new(auth),
            conn: RwLock::new(ConnSnapshot { ws: WsPhase::Closed, error: None }),
            bots: RwLock::new(Vec::new()),
            timings: Mutex::new(HashMap::new()),
            lyrics_cache: Mutex::new(HashMap::new()),
            lyrics_gen: Mutex::new(HashMap::new()),
            last_lyrics: Mutex::new(None),
            auth_rev,
        }
    }

    pub async fn is_logged_in(&self) -> bool {
        self.auth.read().await.state == AuthPhase::LoggedIn
    }

    pub async fn creds(&self) -> Option<(String, String)> {
        let s = self.settings.read().await;
        let token = s.auth.token.clone()?;
        let base = s.server.base_url.clone();
        (!token.is_empty() && !base.is_empty()).then_some((base, token))
    }

    pub async fn active_bot_id(&self) -> Option<String> {
        self.settings.read().await.active_bot_id.clone()
    }

    /// 更新单个 bot 状态；返回 (songKey 是否变化, 新 songKey)。
    /// songKey 变化是切歌信号，驱动歌词重取。
    pub async fn update_bot(&self, status: BotStatus) -> (bool, Option<String>) {
        let new_key = status.current_song.as_ref().map(song_key);
        let mut timings = self.timings.lock().await;
        let key_changed = timings
            .get(&status.id)
            .and_then(|a| a.song_key.clone())
            != new_key;
        timings.insert(status.id.clone(), TimingAnchor::from_status(&status));
        drop(timings);

        let mut bots = self.bots.write().await;
        match bots.iter_mut().find(|b| b.id == status.id) {
            Some(b) => *b = status,
            None => bots.push(status),
        }
        (key_changed, new_key)
    }

    /// bot 被移除时清理锚点。
    pub async fn remove_bot(&self, bot_id: &str) {
        self.timings.lock().await.remove(bot_id);
        self.lyrics_gen.lock().await.remove(bot_id);
        self.bots.write().await.retain(|b| b.id != bot_id);
    }
}
