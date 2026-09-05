//! 系统托盘常驻：主窗关闭后应用仍有踪迹（痛点：否则退出只能任务管理器）。
//! 左键单击切换主窗显隐；右键菜单：显示设置 / 锁定解锁 / 开关歌词 / 退出。
//! 退出语义：不改任何设置（enabled 保留），下次启动按设置恢复。

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::settings::load_settings;

static EXITING: AtomicBool = AtomicBool::new(false);
static HINT_SHOWN: AtomicBool = AtomicBool::new(false);

pub fn is_exiting() -> bool {
    EXITING.load(Ordering::Relaxed)
}

const SHOW_SETTINGS: &str = "show-settings";
const TOGGLE_LOCKED: &str = "toggle-locked";
const TOGGLE_ENABLED: &str = "toggle-enabled";
const QUIT: &str = "quit";

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let tray = TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().expect("bundle icon").clone())
        .tooltip("tsmb-desktop")
        .menu(&build_menu(app)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu_event(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_main(tray.app_handle());
            }
        })
        .build(app)?;
    app.manage(tray);
    Ok(())
}

/// 按当前设置重建托盘菜单（设置变化时由 update_lyrics_settings 调用）。
/// 锁定项在歌词关闭时置灰；文案随状态切换。
pub fn sync_menu(app: &AppHandle) {
    if let Some(tray) = app.try_state::<TrayIcon>() {
        match build_menu(app) {
            Ok(menu) => {
                let _ = tray.set_menu(Some(menu));
            }
            Err(e) => eprintln!("[tray] rebuild menu failed: {e}"),
        }
    }
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let ly = &load_settings(app).lyrics;
    let show = MenuItem::with_id(app, SHOW_SETTINGS, "显示设置", true, None::<&str>)?;
    let toggle_locked = MenuItem::with_id(
        app,
        TOGGLE_LOCKED,
        if ly.locked { "解锁歌词" } else { "锁定歌词" },
        ly.enabled,
        None::<&str>,
    )?;
    let toggle_enabled = MenuItem::with_id(
        app,
        TOGGLE_ENABLED,
        if ly.enabled { "关闭桌面歌词" } else { "开启桌面歌词" },
        true,
        None::<&str>,
    )?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, QUIT, "退出", true, None::<&str>)?;
    Menu::with_items(app, &[&show, &toggle_locked, &toggle_enabled, &sep, &quit])
}

fn on_menu_event(app: &AppHandle, id: &str) {
    match id {
        SHOW_SETTINGS => show_main(app),
        TOGGLE_LOCKED | TOGGLE_ENABLED => {
            let ly = &load_settings(app).lyrics;
            let patch = if id == TOGGLE_LOCKED {
                serde_json::json!({ "locked": !ly.locked })
            } else {
                serde_json::json!({ "enabled": !ly.enabled })
            };
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = app.state::<crate::state::SharedState>();
                let _ = crate::commands::update_lyrics_settings(app.clone(), state, patch).await;
            });
        }
        QUIT => quit(app),
        _ => {}
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

/// 左键切换：可见且有焦点 → 隐藏；否则唤回并聚焦（从别的应用切来时不误隐藏）。
fn toggle_main(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        if win.is_visible().unwrap_or(false) && win.is_focused().unwrap_or(false) {
            let _ = win.hide();
        } else {
            show_main(app);
        }
    }
}

fn quit(app: &AppHandle) {
    EXITING.store(true, Ordering::Relaxed);
    crate::lyrics_window::save_geometry_now(app);
    if let Some(tray) = app.try_state::<TrayIcon>() {
        let _ = tray.set_visible(false);
    }
    app.exit(0);
}

/// 主窗首次隐藏到托盘时提示一次（每进程一次；通知失败静默——托盘仍在）。
pub fn notify_tray_hint_once(app: &AppHandle) {
    if HINT_SHOWN.swap(true, Ordering::Relaxed) {
        return;
    }
    use tauri_plugin_notification::NotificationExt;
    let result = app
        .notification()
        .builder()
        .title("tsmb-desktop")
        .body("已隐藏到托盘，右下角图标可唤回或退出")
        .show();
    if let Err(e) = result {
        eprintln!("[tray] notification failed: {e}");
    }
}
