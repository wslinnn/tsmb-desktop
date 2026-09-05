pub mod api;
pub mod commands;
pub mod events;
pub mod http;
pub mod lyrics;
pub mod lyrics_window;
pub mod poller;
pub mod settings;
pub mod state;
pub mod timing;
pub mod types;
pub mod ws;

use std::sync::Arc;

use tauri::{Manager, WindowEvent};

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
            // store 的 save 在目录不存在时静默失败（token/设置全丢）——先建目录
            if let Ok(dir) = app.path().app_data_dir() {
                let _ = std::fs::create_dir_all(dir);
            }
            let loaded = settings::load_settings(app.handle());
            let lyrics_enabled = loaded.lyrics.enabled;
            let state = Arc::new(state::AppState::new(loaded));
            app.manage(state.clone());

            tauri::async_runtime::spawn(ws::run_ws(app.handle().clone(), state.clone()));
            tauri::async_runtime::spawn(poller::run_poller(app.handle().clone(), state.clone()));
            tauri::async_runtime::spawn(poller::run_ticker(app.handle().clone(), state.clone()));

            if lyrics_enabled {
                lyrics_window::create(app.handle())?;
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            let app = window.app_handle();
            match event {
                // 窗口关闭规则（无托盘闭环）：关主窗时歌词窗在 → 只隐藏主窗；
                // 歌词窗关闭 → 记几何 + enabled=false + 唤回隐藏中的主窗。
                WindowEvent::CloseRequested { api, .. } => match window.label() {
                    "main" => {
                        if app.get_webview_window("lyrics").is_some() {
                            api.prevent_close();
                            let _ = window.hide();
                        }
                    }
                    "lyrics" => {
                        lyrics_window::save_geometry_now(app);
                        let state = app.state::<state::SharedState>();
                        if let Ok(mut s) = state.settings.try_write() {
                            s.lyrics.enabled = false;
                            settings::save_settings(app, &s);
                        }
                        if let Some(main) = app.get_webview_window("main") {
                            let _ = main.show();
                            let _ = main.set_focus();
                        }
                    }
                    _ => {}
                },
                // 位置记忆：拖动结束后落盘（防抖）
                WindowEvent::Moved(pos) if window.label() == "lyrics" => {
                    lyrics_window::schedule_geometry_save(app, pos.x, pos.y);
                }
                _ => {}
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
