use tauri::AppHandle;

use crate::events::{AnchorEvent, TickEvent};
use crate::lyrics::{cached_lines, find_line};
use crate::state::SharedState;

/// elapsed 轮询（自愈通道）：活跃 bot 播放中 2s、暂停/停止 15s、未就绪 5s 空转。
/// 兜住 WS 覆盖不到的三类场景：他端 seek（M1 后已补 stateChange，这里是兜底）、
/// WS 断线期间、本地时钟累计漂移。
pub async fn run_poller(app: AppHandle, state: SharedState) {
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(poll_interval_ms(&state).await)).await;

        let Some(bot_id) = state.active_bot_id().await else { continue };
        let Some((base, token)) = state.creds().await else { continue };

        match crate::api::get_elapsed(&state.http, &base, &token, &bot_id).await {
            Ok(info) => {
                // 合并进 bots 快照 + 重置锚点（硬同步）
                let mut bots = state.bots.write().await;
                if let Some(b) = bots.iter_mut().find(|b| b.id == bot_id) {
                    b.elapsed = info.elapsed;
                    b.playing = info.playing;
                    b.paused = info.paused;
                    if info.effective_duration.is_some() {
                        b.effective_duration = info.effective_duration;
                    }
                    let status = b.clone();
                    drop(bots);
                    let (key_changed, _) = state.update_bot(status).await;
                    crate::events::emit_bots(&app, &state).await;
                    if key_changed {
                        let status2 = state
                            .bots
                            .read()
                            .await
                            .iter()
                            .find(|b| b.id == bot_id)
                            .cloned();
                        if let Some(s) = status2 {
                            crate::lyrics::ensure_lyrics(&app, &state, &s).await;
                        }
                    }
                }
            }
            Err(crate::http::AppError::Unauthorized) => {
                crate::commands::force_logout(&app, &state, "登录已过期").await;
            }
            Err(_) => {} // 网络抖动：下轮再试
        }
    }
}

async fn poll_interval_ms(state: &SharedState) -> u64 {
    let Some(bot_id) = state.active_bot_id().await else { return 5000 };
    let playing = state
        .bots
        .read()
        .await
        .iter()
        .find(|b| b.id == bot_id)
        .map(|b| b.is_progressing())
        .unwrap_or(false);
    if playing { 2000 } else { 15000 }
}

/// 10Hz tick：计算活跃 bot 的插值时间 + 行查找，广播给歌词窗口。
pub async fn run_ticker(app: AppHandle, state: SharedState) {
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let Some(bot_id) = state.active_bot_id().await else { continue };
        let Some(anchor) = state.timings.lock().await.get(&bot_id).cloned() else { continue };

        let offset_ms = state.settings.read().await.lyrics.offset_ms;
        let elapsed = anchor.render_elapsed();
        let t_eff = elapsed - offset_ms as f64 / 1000.0;

        let (line_index, next_index) = match &anchor.song_key {
            Some(key) => match cached_lines(&state, key).await {
                Some(lines) if !lines.is_empty() => {
                    let idx = find_line(&lines, t_eff);
                    (idx, idx.and_then(|i| (i + 1 < lines.len()).then_some(i + 1)))
                }
                _ => (None, None),
            },
            None => (None, None),
        };

        crate::events::emit_tick(
            &app,
            TickEvent {
                bot_id,
                song_key: anchor.song_key.clone().unwrap_or_default(),
                playing: anchor.playing,
                elapsed,
                line_index,
                next_index,
                offset_ms,
                anchor: AnchorEvent {
                    elapsed: anchor.server_elapsed,
                    age_ms: anchor.age_ms(),
                },
            },
        );
    }
}
