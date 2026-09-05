use serde_json::Value;
use tauri::{AppHandle, Manager, State};

use crate::events;
use crate::settings::{normalize_base_url, save_settings};
use crate::state::{AuthPhase, AuthSnapshot, SharedState, WsPhase};

/// 调试日志：Windows GUI 子进程的 stderr 不进 tauri dev 输出，落文件兜底。
#[cfg(debug_assertions)]
pub fn debug_log(msg: &str) {
    use std::io::Write;
    let path = std::env::temp_dir().join("tsmb-debug.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} {msg}", chrono_like_now());
    }
}
#[cfg(debug_assertions)]
fn chrono_like_now() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("[{ms}]")
}
#[cfg(not(debug_assertions))]
pub fn debug_log(_msg: &str) {}

#[tauri::command]
pub async fn login(
    app: AppHandle,
    state: State<'_, SharedState>,
    server: String,
    username: String,
    password: String,
) -> Result<(), String> {
    let base = normalize_base_url(&server);
    if base.is_empty() {
        return Err("服务器地址不能为空".into());
    }
    let resp = crate::api::login(&state.http, &base, &username, &password)
        .await
        .map_err(|e| e.to_string())?;

    {
        let mut s = state.settings.write().await;
        s.server.base_url = base.clone();
        s.auth.token = Some(resp.token);
        s.auth.username = Some(resp.username.clone());
        save_settings(&app, &s);
    }
    *state.auth.write().await = AuthSnapshot {
        state: AuthPhase::LoggedIn,
        username: Some(resp.username),
        server: Some(base),
        reason: None,
    };
    let _ = state.auth_rev.send_modify(|n| *n += 1);
    events::emit_auth(&app, &state.auth.read().await.clone());

    // 不等 WS：立即拉一次 bot 列表（全量替换，别把上一账号的残留带给新用户）
    if let Some((b, t)) = state.creds().await {
        if let Ok(bots) = crate::api::get_bots(&state.http, &b, &t).await {
            state.replace_bots(bots).await;
            events::emit_bots(&app, &state).await;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn logout(app: AppHandle, state: State<'_, SharedState>) -> Result<(), String> {
    // best-effort 撤销远端 token
    if let Some((base, token)) = state.creds().await {
        crate::api::logout(&state.http, &base, &token).await;
    }
    force_logout(&app, &state, "已登出").await;
    Ok(())
}

/// 401（REST）/ 4001（WS）统一走这里：清凭证、广播、打断 ws 循环。
/// 同时清空会话期内存状态（bots/锚点/歌词），不留跨账号残留。
pub async fn force_logout(app: &AppHandle, state: &SharedState, reason: &str) {
    debug_log(&format!("force_logout: {reason}"));
    {
        let mut s = state.settings.write().await;
        s.auth.token = None;
        s.auth.username = None;
        save_settings(app, &s);
    }
    *state.auth.write().await = AuthSnapshot {
        state: AuthPhase::LoggedOut,
        username: None,
        server: None,
        reason: Some(reason.to_string()),
    };
    *state.conn.write().await = crate::state::ConnSnapshot::new(WsPhase::Closed, None);
    state.bots.write().await.clear();
    state.timings.lock().await.clear();
    state.lyrics_gen.lock().await.clear();
    *state.last_lyrics.lock().await = None;
    let _ = state.auth_rev.send_modify(|n| *n += 1);
    events::emit_auth(app, &state.auth.read().await.clone());
    events::emit_conn(app, state).await;
}

#[tauri::command]
pub async fn select_bot(
    app: AppHandle,
    state: State<'_, SharedState>,
    bot_id: String,
) -> Result<(), String> {
    {
        let mut s = state.settings.write().await;
        s.active_bot_id = Some(bot_id.clone());
        save_settings(&app, &s);
    }
    events::emit_active_bot(&app, Some(&bot_id));
    // 歌词窗口立即拿到新活跃 bot 的歌词（缓存或拉取）。
    // 先克隆出守卫再 await：bots.read() 跨 await 会让写锁饿死整个拉取时长。
    let status = state.bots.read().await.iter().find(|b| b.id == bot_id).cloned();
    if let Some(status) = status {
        crate::lyrics::ensure_lyrics(&app, &state, &status).await;
    }
    Ok(())
}

/// 窗口挂载时水合：全量快照（前端先 listen 再调本命令，乱序幂等）。
#[tauri::command]
pub async fn get_state(state: State<'_, SharedState>) -> Result<Value, String> {
    let auth = state.auth.read().await.clone();
    let connection = state.conn.read().await.clone();
    let bots = state.bots.read().await.clone();
    let settings = state.settings.read().await.clone();
    let last_lyrics = state.last_lyrics.lock().await.clone();
    Ok(serde_json::json!({
        "auth": auth,
        "connection": connection,
        "bots": bots,
        "activeBotId": settings.active_bot_id,
        "settings": settings,
        "lyrics": last_lyrics,
    }))
}

/// 歌词设置字段级合并（patch -> lyrics 子对象）+ 持久化 + 广播。
/// 附带窗口副作用：enabled 建窗/销窗、locked 应用穿透、fontSize 保持底边调高。
#[tauri::command]
pub async fn update_lyrics_settings(
    app: AppHandle,
    state: State<'_, SharedState>,
    patch: Value,
) -> Result<(), String> {
    let merged = {
        let mut s = state.settings.write().await;
        let mut lyrics = serde_json::to_value(&s.lyrics).map_err(|e| e.to_string())?;
        if let (Some(dst), Some(src)) = (lyrics.as_object_mut(), patch.as_object()) {
            for (k, v) in src {
                dst.insert(k.clone(), v.clone());
            }
        }
        s.lyrics = serde_json::from_value(lyrics).map_err(|e| e.to_string())?;
        save_settings(&app, &s);
        s.clone()
    };

    // enabled 变化 → 建窗/销窗
    if patch.get("enabled").and_then(Value::as_bool).is_some() {
        if merged.lyrics.enabled {
            crate::lyrics_window::create(&app).map_err(|e| e.to_string())?;
        } else {
            crate::lyrics_window::close(&app);
        }
    }
    // locked 变化 → 应用穿透
    if let Some(locked) = patch.get("locked").and_then(Value::as_bool) {
        if let Some(win) = app.get_webview_window("lyrics") {
            win.set_ignore_cursor_events(locked).map_err(|e| e.to_string())?;
        }
    }
    // fontSize 变化 → 保持底边调高度
    if patch.get("fontSize").and_then(Value::as_f64).is_some() {
        crate::lyrics_window::resize_for_font_size(&app, merged.lyrics.font_size);
    }

    events::emit_settings(&app, &merged);
    Ok(())
}

#[tauri::command]
pub async fn set_lyrics_enabled(
    app: AppHandle,
    state: State<'_, SharedState>,
    enabled: bool,
) -> Result<(), String> {
    update_lyrics_settings(app, state, serde_json::json!({ "enabled": enabled })).await
}

/// 锁定 = 鼠标穿透（副作用由 update_lyrics_settings 统一处理）。
#[tauri::command]
pub async fn set_lyrics_locked(
    app: AppHandle,
    state: State<'_, SharedState>,
    locked: bool,
) -> Result<(), String> {
    update_lyrics_settings(app, state, serde_json::json!({ "locked": locked })).await
}

/// M2 验收调试：活跃 bot 的实时插值（与 Web 面板对拍用）。
#[tauri::command]
pub async fn debug_elapsed(state: State<'_, SharedState>) -> Result<Value, String> {
    let Some(bot_id) = state.active_bot_id().await else {
        return Ok(serde_json::json!({ "error": "no active bot" }));
    };
    let timings = state.timings.lock().await;
    match timings.get(&bot_id) {
        Some(a) => Ok(serde_json::json!({
            "botId": bot_id,
            "renderElapsed": a.render_elapsed(),
            "serverElapsed": a.server_elapsed,
            "ageMs": a.age_ms(),
            "playing": a.playing,
            "songKey": a.song_key,
            "maxDuration": a.max_duration,
        })),
        None => Ok(serde_json::json!({ "error": "no anchor", "botId": bot_id })),
    }
}
