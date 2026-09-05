use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::settings::{load_settings, save_settings};

/// 移动事件防抖：拖动中每次 Moved 都置 pending，500ms 后读最新位置落盘。
static GEO_SAVE_PENDING: AtomicBool = AtomicBool::new(false);

/// 锁按钮热点区（逻辑像素，相对窗口内区），由前端 ResizeObserver 上报。
/// None = 尚未上报，回退默认右上角区域。
static LOCK_HOTSPOT: Mutex<Option<Hotspot>> = Mutex::new(None);

/// 穿透轮询代数：每次 sync 递增，旧循环检测到代数不符自行退出（防双循环）。
static LOCK_POLL_GEN: AtomicU64 = AtomicU64::new(0);
/// 上次设置的穿透状态（tauri 无 is_ignore_cursor_events 查询，本地记账去重）。
static LAST_IGNORE: AtomicBool = AtomicBool::new(false);
/// 拉伸状态广播代数：Resized 事件流期间为最新代数，静默 250ms 后归零。
static RESIZE_GEN: AtomicU64 = AtomicU64::new(0);
/// 高度吸附代数：Resized 期间不断刷新，静默 150ms 后执行一次吸附。
static SNAP_GEN: AtomicU64 = AtomicU64::new(0);
/// 轮询周期：光标落在锁按钮热区 → 解除穿透可点击；离开 → 恢复穿透。
/// 50ms 是误点窗口与开销的折中（参考 Mercurial 16ms；我们无需那么快）。
const LOCK_POLL_INTERVAL_MS: u64 = 50;

#[derive(Clone, Copy)]
struct Hotspot {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

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
        .resizable(true) // 仅宽度拉伸（透明窗高度无意义）；undecorated 边缘命中由 tao 内置处理
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

/// 前端上报锁按钮热点区（逻辑像素）。空矩形 = 恢复默认。
pub fn set_lock_hotspot(x: f64, y: f64, w: f64, h: f64) {
    let rect = if w > 0.0 && h > 0.0 {
        Some(Hotspot { x, y, w, h })
    } else {
        None
    };
    if let Ok(mut guard) = LOCK_HOTSPOT.lock() {
        *guard = rect;
    }
}

/// 锁定且可见期间启动轮询：光标进入锁按钮热区时临时解除穿透，
/// 让「解锁」可以直接点在歌词窗上（窗口级穿透无法被 DOM 规避）。
/// 解锁 / 关闭歌词时以 active=false 调用，循环退出并恢复可交互。
pub fn sync_lock_poller(app: &AppHandle, active: bool) {
    let gen = LOCK_POLL_GEN.fetch_add(1, Ordering::Relaxed) + 1;
    if !active {
        LAST_IGNORE.store(false, Ordering::Relaxed);
        if let Some(win) = app.get_webview_window("lyrics") {
            let _ = win.set_ignore_cursor_events(false);
        }
        return;
    }
    LAST_IGNORE.store(true, Ordering::Relaxed); // 锁定初始态 = 整窗穿透
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            if LOCK_POLL_GEN.load(Ordering::Relaxed) != gen {
                return;
            }
            let Some(win) = app.get_webview_window("lyrics") else {
                return; // 歌词窗已关，轮询自然终结
            };
            let locked = {
                let state = app.state::<crate::state::SharedState>();
                let s = state.settings.read().await;
                s.lyrics.locked
            };
            if !locked {
                return; // 已解锁：恢复可交互后退出
            }
            let want_ignore = !cursor_in_hotspot(&app, &win);
            if LAST_IGNORE.swap(want_ignore, Ordering::Relaxed) != want_ignore {
                let _ = win.set_ignore_cursor_events(want_ignore);
            }
            tokio::time::sleep(std::time::Duration::from_millis(LOCK_POLL_INTERVAL_MS)).await;
        }
    });
}

/// 光标是否落在锁按钮热区（热区为逻辑坐标，换算到物理桌面坐标比较）。
fn cursor_in_hotspot(app: &AppHandle, win: &tauri::WebviewWindow) -> bool {
    let rect = match LOCK_HOTSPOT.lock().ok().and_then(|g| *g) {
        Some(r) => Some(r),
        None => default_hotspot(win),
    };
    let Some(rect) = rect else { return false };
    let (Ok(scale), Ok(outer), Ok(cursor)) = (
        win.scale_factor(),
        win.outer_position(),
        app.cursor_position(),
    ) else {
        return false;
    };
    let x = outer.x as f64 + rect.x * scale;
    let y = outer.y as f64 + rect.y * scale;
    let w = rect.w * scale;
    let h = rect.h * scale;
    cursor.x >= x && cursor.x < x + w && cursor.y >= y && cursor.y < y + h
}

/// 未上报时的回退热区：右上角 52×52 逻辑像素（与前端按钮默认位置对应）。
fn default_hotspot(win: &tauri::WebviewWindow) -> Option<Hotspot> {
    let scale = win.scale_factor().ok()?;
    let size = win.inner_size().ok()?;
    let w = size.width as f64 / scale;
    Some(Hotspot { x: w - 52.0, y: 8.0, w: 44.0, h: 44.0 })
}

/// 拖动/拉伸后的几何持久化（防抖 500ms 读最新值）：位置 + 宽度（逻辑像素）。
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
                if let (Ok(size), Ok(scale)) = (win.inner_size(), win.scale_factor()) {
                    s.lyrics.geometry.width = size.width as f64 / scale;
                }
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
            if let (Ok(size), Ok(scale)) = (win.inner_size(), win.scale_factor()) {
                s.lyrics.geometry.width = size.width as f64 / scale;
            }
            save_settings(app, &s);
        }
    }
}

/// 拉伸进行中广播：Resized 事件流驱动（模态拉伸循环里 DOM 收不到鼠标
/// 事件，hover 状态不可靠），前端据此与 hover 并集显示遮罩。
/// 静默 250ms 视为拉伸结束，回发 false。
pub fn notify_resizing(app: &AppHandle) {
    let prev = RESIZE_GEN.load(Ordering::Relaxed);
    let gen = prev + 1;
    RESIZE_GEN.store(gen, Ordering::Relaxed);
    if prev == 0 {
        let _ = app.emit_to("lyrics", "lyrics-resizing", true);
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        if RESIZE_GEN.load(Ordering::Relaxed) == gen {
            RESIZE_GEN.store(0, Ordering::Relaxed);
            let _ = app.emit_to("lyrics", "lyrics-resizing", false);
        }
    });
}

/// 仅宽度拉伸：高度吸附回字号推导值。
/// 注意绝不能在 Resized 处理里同步 set_size——那是在模态拉伸循环内部
/// 调 SetWindowPos，会和系统拉伸循环互相打架导致卡死。这里只排队：
/// Resized 事件流静默 150ms（= 用户已松手）后才执行一次吸附；拖动过程
/// 中高度变化是临时的（透明窗高度本来就不可见）。
pub fn schedule_height_snap(app: &AppHandle) {
    let gen = SNAP_GEN.fetch_add(1, Ordering::Relaxed) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        if SNAP_GEN.load(Ordering::Relaxed) != gen {
            return; // 拉伸仍在进行，由更晚的事件负责
        }
        let Some(win) = app.get_webview_window("lyrics") else { return };
        let Ok(scale) = win.scale_factor() else { return };
        let Ok(cur) = win.inner_size() else { return };
        let font_size = {
            let state = app.state::<crate::state::SharedState>();
            let s = state.settings.read().await;
            s.lyrics.font_size
        };
        let want_h = (height_for(font_size) * scale).round() as u32;
        if cur.height == want_h {
            return;
        }
        let w_logical = cur.width as f64 / scale;
        let _ = win.set_size(tauri::LogicalSize::new(w_logical, height_for(font_size)));
    });
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
