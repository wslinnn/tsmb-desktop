use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::settings::{load_settings, save_settings};

/// 移动事件防抖：拖动中每次 Moved 都置 pending，500ms 后读最新位置落盘。
static GEO_SAVE_PENDING: AtomicBool = AtomicBool::new(false);

/// 歌词窗口高度由字号推导，始终预留双行（1.5em/行）+ 内边距。
pub fn height_for(font_size: f64) -> f64 {
    font_size * 1.5 * 2.0 + 44.0
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window("lyrics").is_some() {
        return Ok(());
    }
    let settings = load_settings(app);
    let w = settings.lyrics.geometry.width;
    let h = height_for(settings.lyrics.font_size);
    let (x, y) = restore_position(app, w, h);

    let win = WebviewWindowBuilder::new(app, "lyrics", WebviewUrl::App("index.html".into()))
        .title("desktop-lyrics")
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .resizable(false)
        .focused(false)
        .focusable(false) // 点击/拖动不抢前台焦点（游戏场景关键）
        .inner_size(w, h)
        .position(x, y)
        .build()?;
    win.set_ignore_cursor_events(settings.lyrics.locked)?;
    Ok(())
}

pub fn close(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("lyrics") {
        let _ = win.close();
    }
}

/// 拖动/创建后的位置持久化（防抖 500ms 读最新值）。
pub fn schedule_geometry_save(app: &AppHandle) {
    if GEO_SAVE_PENDING.swap(true, Ordering::Relaxed) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        GEO_SAVE_PENDING.store(false, Ordering::Relaxed);
        if let Some(win) = app.get_webview_window("lyrics") {
            if let Ok(pos) = win.outer_position() {
                let state = app.state::<crate::state::SharedState>();
                let mut s = state.settings.write().await;
                s.lyrics.geometry.x = Some(pos.x);
                s.lyrics.geometry.y = Some(pos.y);
                save_settings(&app, &s);
            }
        }
    });
}

/// 立即落盘当前几何（窗口关闭前调用）。
pub fn save_geometry_now(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("lyrics") {
        if let Ok(pos) = win.outer_position() {
            let state = app.state::<crate::state::SharedState>();
            let mut s = state.settings.blocking_write();
            s.lyrics.geometry.x = Some(pos.x);
            s.lyrics.geometry.y = Some(pos.y);
            save_settings(app, &s);
        }
    }
}

/// 恢复顺序：settings 里存的物理坐标（且仍落在某显示器内）→ 否则主屏
/// 工作区底部居中。返回逻辑坐标（WebviewWindowBuilder::position 的单位）。
fn restore_position(app: &AppHandle, w: f64, h: f64) -> (f64, f64) {
    let settings = load_settings(app);
    let monitors = app.available_monitors().unwrap_or_default();
    let stored = settings.lyrics.geometry;
    if let (Some(x), Some(y)) = (stored.x, stored.y) {
        if let Some(m) = monitors.iter().find(|m| {
            let mp = m.position();
            let ms = m.size();
            x >= mp.x && x < mp.x + ms.width as i32 && y >= mp.y && y < mp.y + ms.height as i32
        }) {
            let s = m.scale_factor();
            return (x as f64 / s, y as f64 / s);
        }
    }
    // 默认位：主屏底部居中（近似工作区：底部留 56px 任务栏余量）
    let primary = app.primary_monitor().ok().flatten().or_else(|| monitors.into_iter().next());
    if let Some(m) = primary {
        let s = m.scale_factor();
        let mp = m.position();
        let mw = m.size().width as f64 / s;
        let mh = m.size().height as f64 / s;
        return (mp.x as f64 + (mw - w) / 2.0, mp.y as f64 + mh - h - 56.0);
    }
    (100.0, 100.0)
}

/// 字号变化：保持底边不动调高度（对应 Web 端歌词窗习惯）。
pub fn resize_for_font_size(app: &AppHandle, font_size: f64) {
    let Some(win) = app.get_webview_window("lyrics") else { return };
    let Ok(scale) = win.scale_factor() else { return };
    let Ok(size) = win.outer_size() else { return };
    let Ok(pos) = win.outer_position() else { return };

    let new_h_logical = height_for(font_size);
    let new_h_phys = (new_h_logical * scale).round() as i32;
    let bottom = pos.y + size.height as i32;
    let new_w_logical = {
        let s = load_settings(app);
        s.lyrics.geometry.width
    };

    let _ = win.set_size(tauri::LogicalSize::new(new_w_logical, new_h_logical));
    let _ = win.set_position(tauri::PhysicalPosition::new(pos.x, bottom - new_h_phys));
}
