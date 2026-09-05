use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use crate::state::{SharedState, WsPhase};
use crate::types::BotStatus;

/// R2 半开连接判定：入站帧静默超过该秒数视为连接已死。服务端心跳 25s，
/// 留 2 倍以上余量；健康连接任一帧（含 ping）都会重置计时，不会误杀。
const WS_MAX_SILENCE_SECS: u64 = 60;

/// WS 服务端消息（纯解析，可单测）。favoritesChanged 等忽略。
#[derive(Clone, Debug, PartialEq)]
pub enum WsEvent {
    Init { bots: Vec<BotStatus> },
    StateChange { bot_id: String, status: BotStatus },
    BotConnected { bot_id: String, status: BotStatus },
    BotDisconnected { bot_id: String, status: BotStatus },
    BotRemoved { bot_id: String },
    Other,
}

pub fn parse_ws_message(text: &str) -> Option<WsEvent> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    Some(match v.get("type")?.as_str()? {
        "init" => WsEvent::Init {
            bots: serde_json::from_value(v.get("bots").cloned().unwrap_or_default()).ok()?,
        },
        "stateChange" => WsEvent::StateChange {
            bot_id: v.get("botId")?.as_str()?.to_string(),
            status: serde_json::from_value(v.get("status")?.clone()).ok()?,
        },
        "botConnected" => WsEvent::BotConnected {
            bot_id: v.get("botId")?.as_str()?.to_string(),
            status: serde_json::from_value(v.get("status")?.clone()).ok()?,
        },
        "botDisconnected" => WsEvent::BotDisconnected {
            bot_id: v.get("botId")?.as_str()?.to_string(),
            status: serde_json::from_value(v.get("status")?.clone()).ok()?,
        },
        "botRemoved" => WsEvent::BotRemoved {
            bot_id: v.get("botId")?.as_str()?.to_string(),
        },
        _ => WsEvent::Other,
    })
}

/// WS 主循环：等登录 → 连接（无限重连，退避 1s 起步、30s 封顶）。
/// 与 Web 端的差异（10 次上限）是有意的：桌面端常驻，bot 服务器重启后
/// 必须能自愈；4001（会话过期）仍然立即停止并登出。
pub async fn run_ws(app: tauri::AppHandle, state: SharedState) {
    let mut auth_rx = state.auth_rev.subscribe();
    loop {
        // 等待已登录
        loop {
            if state.is_logged_in().await {
                break;
            }
            if auth_rx.changed().await.is_err() {
                return;
            }
        }
        let mut attempt: u32 = 0;
        loop {
            if !state.is_logged_in().await {
                break;
            }
            let Some((base, token)) = state.creds().await else { break };
            eprintln!("[ws] connecting to {base}");
            set_conn(&app, &state, WsPhase::Connecting, None).await;
            match connect_and_stream(&app, &state, &base, &token).await {
                StreamOutcome::SessionExpired => {
                    crate::commands::debug_log("ws outcome: SessionExpired");
                    // 主动登出时服务器也会以 4001 关闭 socket——那不是"会话过期"，
                    // 不能覆盖已登出状态（reason 会被改成误导性的"登录已过期"）
                    if state.is_logged_in().await {
                        crate::commands::force_logout(&app, &state, "登录已过期").await;
                    }
                    break;
                }
                StreamOutcome::LoggedOut => break,
                StreamOutcome::Interrupted(e) => {
                    set_conn(&app, &state, WsPhase::Retrying, Some(e)).await;
                }
            }
            let delay = std::cmp::min(30u64, 1u64 << attempt.min(5)) * 1000;
            attempt += 1;
            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
        }
        set_conn(&app, &state, WsPhase::Closed, None).await;
        let _ = auth_rx.changed().await; // 等下一次登录态变更
    }
}

enum StreamOutcome {
    /// 服务端以 4001 关闭（或握手即拒）：会话过期。
    SessionExpired,
    /// 本地登出打断。
    LoggedOut,
    /// 连接失败或中断（含错误描述）。
    Interrupted(String),
}

async fn connect_and_stream(
    app: &tauri::AppHandle,
    state: &SharedState,
    base: &str,
    token: &str,
) -> StreamOutcome {
    let url = crate::settings::ws_url(base);
    // IntoClientRequest 补全 WS 握手必需头（key/version/upgrade），再追加
    // Authorization（走 header 而非 ?token=，避免进访问日志）
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::http::HeaderValue;
    let mut req = match (&url).into_client_request() {
        Ok(r) => r,
        Err(e) => return StreamOutcome::Interrupted(e.to_string()),
    };
    if let Ok(v) = HeaderValue::from_str(&format!("Bearer {token}")) {
        req.headers_mut().insert("Authorization", v);
    }

    let stream = match tokio_tungstenite::connect_async_tls_with_config(req, None, false, None).await
    {
        Ok((s, _)) => s,
        Err(e) => {
            let msg = e.to_string();
            crate::commands::debug_log(&format!("ws handshake err: {msg}"));
            // 握手完成但被 4001 关闭 → 会话过期（tungstenite 报 Protocol/Response 错误）
            if msg.contains("4001") {
                return StreamOutcome::SessionExpired;
            }
            return StreamOutcome::Interrupted(msg);
        }
    };
    crate::commands::debug_log("ws open");
    set_conn(app, state, WsPhase::Open, None).await;
    let (mut sink, mut stream) = stream.split();

    loop {
        // R2：每次读帧带 60s 超时——任何入站帧（含 ping/pong/数据）都会重置；
        // 静默超限即判定半开连接（服务器断电无 Close 帧的场景），走退避重连。
        let next = tokio::time::timeout(
            std::time::Duration::from_secs(WS_MAX_SILENCE_SECS),
            stream.next(),
        )
        .await;
        let msg = match next {
            Err(_) => {
                let m = format!("{WS_MAX_SILENCE_SECS} 秒未收到服务端任何帧，判定连接已死");
                crate::commands::debug_log(&format!("ws half-open: {m}"));
                return StreamOutcome::Interrupted(m);
            }
            Ok(None) => return StreamOutcome::Interrupted("连接中断".into()),
            Ok(Some(Err(e))) => return StreamOutcome::Interrupted(e.to_string()),
            Ok(Some(Ok(m))) => m,
        };
        match msg {
            Message::Text(text) => {
                if let Some(event) = parse_ws_message(&text) {
                    apply_ws_event(app, state, event).await;
                }
            }
            Message::Ping(payload) => {
                // 服务端 25s ping：必须回 Pong，否则两轮后连接被 terminate
                let _ = sink.send(Message::Pong(payload)).await;
            }
            Message::Close(frame) => {
                let code = frame.as_ref().map(|f| u16::from(f.code)).unwrap_or(0);
                crate::commands::debug_log(&format!("ws close code={code}"));
                return match code {
                    4001 => StreamOutcome::SessionExpired,
                    _ => StreamOutcome::Interrupted("连接被服务端关闭".into()),
                };
            }
            _ => {}
        }
        if !state.is_logged_in().await {
            return StreamOutcome::LoggedOut;
        }
    }
}

async fn apply_ws_event(app: &tauri::AppHandle, state: &SharedState, event: WsEvent) {
    match event {
        // init 是全量快照：整体替换（增量合并会让快照里消失的 bot 永久残留，
        // 换账号登录后就是跨用户的数据泄漏）
        WsEvent::Init { bots } => {
            state.replace_bots(bots).await;
            crate::events::emit_bots(app, state).await;
            maybe_fetch_active_lyrics(app, state);
            state.wake_tick();
        }
        WsEvent::StateChange { status, .. } | WsEvent::BotConnected { status, .. }
        | WsEvent::BotDisconnected { status, .. } => {
            let (key_changed, _) = state.update_bot(status).await;
            crate::events::emit_bots(app, state).await;
            state.wake_tick();
            if key_changed {
                maybe_fetch_active_lyrics(app, state);
            }
        }
        WsEvent::BotRemoved { bot_id } => {
            state.remove_bot(&bot_id).await;
            crate::events::emit_bots(app, state).await;
            state.wake_tick();
        }
        WsEvent::Other => {}
    }
}

/// 活跃 bot 的歌词兜底拉取（切歌检测的触发点在 update_bot 的 key_changed）。
/// spawn 而非原地 await：歌词网络请求不得阻塞 WS 读循环（Pong 饥饿会被
/// 服务端 25s 心跳判死）。
fn maybe_fetch_active_lyrics(app: &tauri::AppHandle, state: &SharedState) {
    let app = app.clone();
    let state = state.clone();
    tauri::async_runtime::spawn(async move {
        let Some(active) = state.active_bot_id().await else { return };
        // 先克隆再出守卫：bots.read() 不得跨 await
        let status = state.bots.read().await.iter().find(|b| b.id == active).cloned();
        if let Some(status) = status {
            crate::lyrics::ensure_lyrics(&app, &state, &status).await;
        }
    });
}

/// 更新连接状态并广播（前端顶栏指示灯依赖这条事件流，不能只改状态不发射）。
async fn set_conn(
    app: &tauri::AppHandle,
    state: &SharedState,
    phase: WsPhase,
    error: Option<String>,
) {
    *state.conn.write().await = crate::state::ConnSnapshot::new(phase, error);
    crate::events::emit_conn(app, state).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_init_and_state_change() {
        let e = parse_ws_message(
            r#"{"type":"init","bots":[{"id":"b1","name":"x","elapsed":3.5,"playing":true}]}"#,
        )
        .unwrap();
        match e {
            WsEvent::Init { bots } => {
                assert_eq!(bots.len(), 1);
                assert_eq!(bots[0].id, "b1");
                assert!((bots[0].elapsed - 3.5).abs() < 1e-9);
            }
            _ => panic!("wrong variant"),
        }

        let e = parse_ws_message(
            r#"{"type":"stateChange","botId":"b2","status":{"id":"b2","elapsed":10,"currentSong":{"id":"s1","platform":"netease","duration":180}},"queueUnchanged":true}"#,
        )
        .unwrap();
        match e {
            WsEvent::StateChange { bot_id, status } => {
                assert_eq!(bot_id, "b2");
                assert_eq!(status.current_song.unwrap().platform, "netease");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parses_removal_and_ignores_unknown() {
        assert_eq!(
            parse_ws_message(r#"{"type":"botRemoved","botId":"b1"}"#).unwrap(),
            WsEvent::BotRemoved { bot_id: "b1".into() }
        );
        assert_eq!(parse_ws_message(r#"{"type":"favoritesChanged"}"#).unwrap(), WsEvent::Other);
        assert!(parse_ws_message("not json").is_none());
        assert!(parse_ws_message(r#"{"type":42}"#).is_none());
    }
}
