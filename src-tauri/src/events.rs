use std::sync::Arc;

use serde::Serialize;
use tauri::Emitter;

use crate::state::{AuthSnapshot, ConnSnapshot, SharedState};
use crate::types::LyricLine;

/// Rust → 前端事件（payload 定义，见 docs/architecture.md 事件协议表）。
/// 全部全量快照：前端「先 listen 再 get_state」的乱序到达是幂等的。

#[derive(Clone, Copy, Serialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum LyricsPhase {
    Loading,
    Ok,
    None,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsDataEvent {
    pub bot_id: String,
    pub song_key: String,
    pub state: LyricsPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<Arc<Vec<LyricLine>>>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TickEvent {
    pub bot_id: String,
    pub song_key: String,
    pub playing: bool,
    /// 渲染时间（未应用偏移的原始插值值）
    pub elapsed: f64,
    /// 已按 offset 修正后的行查找结果
    pub line_index: Option<usize>,
    pub next_index: Option<usize>,
    pub offset_ms: i64,
    /// P1 行内渐变本地插值预留：t = anchor.elapsed + (ageMs + 到达以来耗时)/1000
    pub anchor: AnchorEvent,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorEvent {
    pub elapsed: f64,
    pub age_ms: f64,
}

pub fn emit_auth(app: &tauri::AppHandle, snap: &AuthSnapshot) {
    let _ = app.emit("auth-state", snap);
}

pub async fn emit_conn(app: &tauri::AppHandle, state: &SharedState) {
    let snap: ConnSnapshot = state.conn.read().await.clone();
    let _ = app.emit("connection-state", &snap);
}

pub async fn emit_bots(app: &tauri::AppHandle, state: &SharedState) {
    let bots = state.bots.read().await.clone();
    let _ = app.emit("bots-updated", serde_json::json!({ "bots": bots }));
}

pub fn emit_active_bot(app: &tauri::AppHandle, bot_id: Option<&str>) {
    let _ = app.emit("active-bot", serde_json::json!({ "botId": bot_id }));
}

/// 发射 lyrics-data 前先写入 state.last_lyrics（晚加载的窗口从 get_state 补水）。
pub fn emit_lyrics_data(
    app: &tauri::AppHandle,
    state: &SharedState,
    bot_id: &str,
    song_key: &str,
    phase: LyricsPhase,
) {
    let ev = LyricsDataEvent {
        bot_id: bot_id.to_string(),
        song_key: song_key.to_string(),
        state: phase,
        lines: None,
    };
    if let Ok(mut slot) = state.last_lyrics.try_lock() {
        *slot = Some(ev.clone());
    }
    let _ = app.emit("lyrics-data", &ev);
}

pub fn emit_lyrics_data_lines(
    app: &tauri::AppHandle,
    state: &SharedState,
    bot_id: &str,
    song_key: &str,
    lines: Arc<Vec<LyricLine>>,
) {
    let ev = LyricsDataEvent {
        bot_id: bot_id.to_string(),
        song_key: song_key.to_string(),
        state: LyricsPhase::Ok,
        lines: Some(lines),
    };
    if let Ok(mut slot) = state.last_lyrics.try_lock() {
        *slot = Some(ev.clone());
    }
    let _ = app.emit("lyrics-data", &ev);
}

pub fn emit_settings(app: &tauri::AppHandle, settings: &crate::settings::Settings) {
    let _ = app.emit("settings-changed", serde_json::json!({ "settings": settings }));
}

pub fn emit_tick(app: &tauri::AppHandle, tick: TickEvent) {
    // 窗口不存在时 emit_to 无害（自动丢弃）
    let _ = app.emit_to("lyrics", "lyrics-tick", tick);
}
