pub mod api;
pub mod commands;
pub mod events;
pub mod http;
pub mod lyrics;
pub mod poller;
pub mod settings;
pub mod state;
pub mod timing;
pub mod types;
pub mod ws;

use std::sync::Arc;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // single-instance 必须最先注册：双开时只聚焦已有实例的窗口
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::login,
            commands::logout,
            commands::select_bot,
            commands::get_state,
            commands::update_lyrics_settings,
            commands::set_lyrics_enabled,
            commands::set_lyrics_locked,
            commands::debug_elapsed,
        ])
        .setup(|app| {
            let loaded = settings::load_settings(app.handle());
            let lyrics_enabled = loaded.lyrics.enabled;
            let lyrics_locked = loaded.lyrics.locked;
            let state = Arc::new(state::AppState::new(loaded));
            app.manage(state.clone());

            tauri::async_runtime::spawn(ws::run_ws(app.handle().clone(), state.clone()));
            tauri::async_runtime::spawn(poller::run_poller(app.handle().clone(), state.clone()));
            tauri::async_runtime::spawn(poller::run_ticker(app.handle().clone(), state.clone()));

            if lyrics_enabled {
                create_lyrics_window(app.handle(), lyrics_locked)?;
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // 窗口关闭规则（无托盘闭环）：关主窗时歌词窗在 → 只隐藏主窗；
            // 歌词窗关闭 → 唤回隐藏中的主窗。两个窗口都没了应用自然退出。
            if let WindowEvent::CloseRequested { api, .. } = event {
                match window.label() {
                    "main" => {
                        if window.app_handle().get_webview_window("lyrics").is_some() {
                            api.prevent_close();
                            let _ = window.hide();
                        }
                    }
                    "lyrics" => {
                        if let Some(main) = window.app_handle().get_webview_window("main") {
                            let _ = main.show();
                            let _ = main.set_focus();
                        }
                    }
                    _ => {}
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn create_lyrics_window(app: &AppHandle, locked: bool) -> tauri::Result<()> {
    if app.get_webview_window("lyrics").is_some() {
        return Ok(());
    }
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
        .inner_size(900.0, 110.0)
        .build()?;
    win.set_ignore_cursor_events(locked)?;
    Ok(())
}
