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
pub mod tray;
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
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            commands::login,
            commands::logout,
            commands::select_bot,
            commands::get_state,
            commands::update_lyrics_settings,
            commands::set_lyrics_enabled,
            commands::set_lyrics_locked,
            commands::set_lyrics_lock_hotspot,
            commands::debug_elapsed,
        ])
        .setup(|app| {
            // store 的 save 在目录不存在时静默失败（token/设置全丢）——先建目录
            if let Ok(dir) = app.path().app_data_dir() {
                let _ = std::fs::create_dir_all(dir);
            }
            let loaded = settings::load_settings(app.handle());
            let lyrics_enabled = loaded.lyrics.enabled;
            let state = Arc::new(state::AppState::new(loaded.clone()));
            app.manage(state.clone());

            tauri::async_runtime::spawn(ws::run_ws(app.handle().clone(), state.clone()));
            tauri::async_runtime::spawn(poller::run_poller(app.handle().clone(), state.clone()));
            tauri::async_runtime::spawn(poller::run_ticker(app.handle().clone(), state.clone()));

            if lyrics_enabled {
                lyrics_window::create(app.handle())?;
                if loaded.lyrics.locked {
                    lyrics_window::sync_lock_poller(app.handle(), true);
                }
            }
            tray::init(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            let app = window.app_handle();
            match event {
                // 窗口关闭规则（托盘常驻）：主窗 ✕ = 隐藏到托盘（首次气泡提示）；
                // 歌词窗 ✕ = 记几何 + enabled=false（托盘退出走 is_exiting 跳过）。
                WindowEvent::CloseRequested { api, .. } => match window.label() {
                    "main" => {
                        api.prevent_close();
                        let _ = window.hide();
                        tray::notify_tray_hint_once(app);
                    }
                    "lyrics" => {
                        lyrics_window::save_geometry_now(app);
                        if tray::is_exiting() {
                            return;
                        }
                        // 主线程事件处理器：用 blocking_write（try_write 在
                        // 与防抖落盘任务竞争失败时会静默丢掉 enabled=false，
                        // 窗口关了下次启动却又出现）
                        let state = app.state::<state::SharedState>();
                        let mut s = state.settings.blocking_write();
                        s.lyrics.enabled = false;
                        settings::save_settings(app, &s);
                        if let Some(main) = app.get_webview_window("main") {
                            let _ = main.show();
                            let _ = main.set_focus();
                        }
                    }
                    _ => {}
                },
                // 位置记忆：拖动/拉伸结束后落盘（防抖，含宽度）
                WindowEvent::Moved(_pos) if window.label() == "lyrics" => {
                    lyrics_window::schedule_geometry_save(app);
                }
                // 仅宽度拉伸：广播拉伸态给遮罩；高度吸附延迟到松手后
                // （拉伸中同步 set_size 会与模态循环互搏导致卡死）
                WindowEvent::Resized(_) if window.label() == "lyrics" => {
                    lyrics_window::notify_resizing(app);
                    lyrics_window::schedule_height_snap(app);
                    lyrics_window::schedule_geometry_save(app);
                }
                _ => {}
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
