use tauri::AppHandle;

use crate::lyrics::find_line;
use crate::state::SharedState;

/// elapsed 轮询（自愈通道）：活跃 bot 播放中 2s、暂停/停止 15s。
/// 兜住 WS 覆盖不到的三类场景：他端 seek（M1 后已补 stateChange，这里是兜底）、
/// WS 断线期间、本地时钟累计漂移。
/// 停车矩阵：登出（等 auth_rev）/ 无活跃 bot（等 tick_wake）时零周期唤醒。
pub async fn run_poller(app: AppHandle, state: SharedState) {
    let mut auth_rx = state.auth_rev.subscribe();
    loop {
        if !state.is_logged_in().await {
            if auth_rx.changed().await.is_err() {
                return;
            }
            continue;
        }
        let Some(bot_id) = state.active_bot_id().await else {
            // select_bot / WS init（带来 bot 列表）都会 wake_tick
            state.tick_wake.notified().await;
            continue;
        };
        let Some((base, token)) = state.creds().await else {
            // 已登录但服务器地址为空（半配置）：慢速空转即可
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            continue;
        };
        tokio::time::sleep(std::time::Duration::from_millis(poll_interval_ms(&state).await)).await;

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
                    // 锚点刷新后 tick 需要重算（seek / 漂移纠正可能移动行边界）
                    state.wake_tick();
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
    // WS 开着：stateChange 是主通道，轮询降为纯兜底（他端 seek / 本地漂移），
    // 15s 足够——这是对 VPS 的主要减负（基线：播放中曾固定 2s = 0.5 req/s）
    if state.conn.read().await.ws == crate::state::WsPhase::Open {
        return 15000;
    }
    // WS 断开：轮询是唯一数据通道；播放中 3s 快速自愈，其余 15s
    let Some(bot_id) = state.active_bot_id().await else { return 15000 };
    let playing = state
        .bots
        .read()
        .await
        .iter()
        .find(|b| b.id == bot_id)
        .map(|b| b.is_progressing())
        .unwrap_or(false);
    if playing { 3000 } else { 15000 }
}

/// 边界驱动 tick：算出下一行边界的到达时刻 → sleep_until min(边界, 1s 心跳)；
/// 仅 (行索引, songKey, playing) 变化时才发射 lyrics-tick（绝大多数唤醒无
/// 信息量，不发射 → 不唤醒 WebView 渲染）。暂停 / 登出 / 无锚点停车：
/// 播放中才有定时器，其余纯事件唤醒（tick_wake / auth_rev）。
pub async fn run_ticker(app: AppHandle, state: SharedState) {
    let mut auth_rx = state.auth_rev.subscribe();
    // 已发射签名 (lineIndex, songKey, playing)：相同内容不重复发射
    let mut last: Option<(Option<usize>, String, bool)> = None;
    loop {
        // 登出后锚点是残留快照，继续 tick 会让歌词永远冻在最后一句
        if !state.is_logged_in().await {
            if auth_rx.changed().await.is_err() {
                return;
            }
            last = None; // 重新登录后强制重发一次
            continue;
        }
        // 歌词窗未开启（enabled=false 即窗口不存在）：发射只是空投，停车等
        // 设置变更（update_lyrics_settings 会 wake_tick）；重开时强制重发
        if !state.settings.read().await.lyrics.enabled {
            last = None;
            state.tick_wake.notified().await;
            continue;
        }

        let bot_id = state.active_bot_id().await;
        let offset_ms = state.settings.read().await.lyrics.offset_ms;
        let anchor = match bot_id.as_ref() {
            Some(id) => state.timings.lock().await.get(id).cloned(),
            None => None,
        };

        // 无锚点 / 无歌：发一次占位 tick（前端落到歌名/等待页），然后等事件唤醒
        let Some(a) = anchor.filter(|a| a.song_key.is_some()) else {
            let sig = (None, String::new(), false);
            if last.as_ref() != Some(&sig) {
                crate::events::emit_tick(
                    &app,
                    &state,
                    crate::events::TickEvent {
                        bot_id: bot_id.unwrap_or_default(),
                        song_key: String::new(),
                        playing: false,
                        elapsed: 0.0,
                        line_index: None,
                        next_index: None,
                        offset_ms,
                        anchor: crate::events::AnchorEvent { elapsed: 0.0, age_ms: 0.0 },
                    },
                );
                last = Some(sig);
            }
            state.tick_wake.notified().await;
            continue;
        };

        let elapsed = a.render_elapsed();
        let t_eff = elapsed - offset_ms as f64 / 1000.0;
        // 行索引与下一行边界共用一次缓存读取（get 会触碰 LRU 序）
        let lines = match a.song_key.as_deref() {
            Some(k) => state.lyrics_cache.lock().await.get(k),
            None => None,
        };
        let (line_index, next_index, boundary_ms) = match lines.as_deref() {
            Some(ls) if !ls.is_empty() => {
                let idx = find_line(ls, t_eff);
                let boundary = idx.and_then(|i| boundary_delay_ms(ls, i, t_eff));
                (idx, idx.and_then(|i| (i + 1 < ls.len()).then_some(i + 1)), boundary)
            }
            _ => (None, None, None),
        };

        let sig = (line_index, a.song_key.clone().unwrap_or_default(), a.playing);
        if last.as_ref() != Some(&sig) {
            crate::events::emit_tick(
                &app,
                &state,
                crate::events::TickEvent {
                    bot_id: bot_id.unwrap_or_default(),
                    song_key: sig.1.clone(),
                    playing: a.playing,
                    elapsed,
                    line_index,
                    next_index,
                    offset_ms,
                    anchor: crate::events::AnchorEvent {
                        elapsed: a.server_elapsed,
                        age_ms: a.age_ms(),
                    },
                },
            );
            last = Some(sig);
        }

        // 播放中：等到 min(下一行边界, 1s 心跳)；暂停：无定时器，纯事件唤醒
        let deadline = tick_deadline(boundary_ms, a.playing);
        tokio::select! {
            _ = async {
                match deadline {
                    Some(d) => tokio::time::sleep_until(d).await,
                    None => std::future::pending::<()>().await,
                }
            } => {}
            _ = state.tick_wake.notified() => {}
        }
    }
}

/// 下一行边界距现在的毫秒数（纯函数，可测）。t_eff 已越过边界时返回 0
///（立即触发重算）。
fn boundary_delay_ms(lines: &[crate::types::LyricLine], cur: usize, t_eff: f64) -> Option<u64> {
    lines
        .get(cur + 1)
        .map(|l| ((l.time - t_eff).max(0.0) * 1000.0) as u64)
}

/// tick 定时器：播放中 = min(下一行边界, 1s 心跳兜底)（心跳兜锚点漂移与
/// 外部状态突变）；暂停 / 未播放返回 None（无周期唤醒，纯事件驱动）。
fn tick_deadline(
    boundary_ms: Option<u64>,
    playing: bool,
) -> Option<tokio::time::Instant> {
    if !playing {
        return None;
    }
    let now = tokio::time::Instant::now();
    let heartbeat = now + std::time::Duration::from_secs(1);
    Some(match boundary_ms {
        Some(ms) => (now + std::time::Duration::from_millis(ms)).min(heartbeat),
        None => heartbeat,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::LyricLine;

    fn line(t: f64) -> LyricLine {
        LyricLine { time: t, text: "x".into(), translation: None, roma: None }
    }

    #[test]
    fn boundary_delay_positive_and_clamped() {
        let lines = [line(10.0), line(20.0), line(30.0)];
        // cur=0 的下一边界是 lines[1]=20
        assert_eq!(boundary_delay_ms(&lines, 0, 12.0), Some(8000));
        assert_eq!(boundary_delay_ms(&lines, 0, 19.0), Some(1000));
        // 已越过（seek 后落在下一行区间内）：0，立即重算
        assert_eq!(boundary_delay_ms(&lines, 0, 25.0), Some(0));
        // 最后一行没有下一边界
        assert_eq!(boundary_delay_ms(&lines, 2, 29.0), None);
        // 越界行下标也不 panic
        assert_eq!(boundary_delay_ms(&lines, 5, 1.0), None);
    }

    #[test]
    fn deadline_park_when_not_playing() {
        assert!(tick_deadline(Some(100), false).is_none());
        assert!(tick_deadline(None, false).is_none());
    }

    #[test]
    fn deadline_capped_by_heartbeat() {
        let d = tick_deadline(Some(3_600_000), true).unwrap();
        let cap = std::time::Duration::from_secs(1);
        // 远处边界被 1s 心跳截断
        assert!(d < tokio::time::Instant::now() + cap + std::time::Duration::from_millis(50));
        let d2 = tick_deadline(Some(200), true).unwrap();
        // 近处边界优先于心跳
        assert!(d2 < tokio::time::Instant::now() + std::time::Duration::from_millis(400));
    }
}
