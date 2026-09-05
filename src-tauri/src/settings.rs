use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

/// 全部持久化设置（tauri-plugin-store, settings.json）。缺省值合并：
/// 旧版本配置文件缺字段时回落 Default，不会因加字段而失效。
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    #[serde(default)]
    pub server: ServerSettings,
    #[serde(default)]
    pub auth: AuthSettings,
    #[serde(default)]
    pub active_bot_id: Option<String>,
    #[serde(default)]
    pub lyrics: LyricsSettings,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[derive(Default)]
pub struct ServerSettings {
    pub base_url: String,
}


#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[derive(Default)]
pub struct AuthSettings {
    pub token: Option<String>,
    pub username: Option<String>,
}


#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LyricsSettings {
    pub enabled: bool,
    pub locked: bool,
    pub font_family: String,
    pub font_size: f64,
    pub bold: bool,
    pub base_color: String,
    pub highlight_color: String,
    pub opacity: f64,
    pub outline_color: String,
    pub outline_width: f64,
    /// 歌词显示偏移（毫秒，正 = 歌词延后）。只影响行查找，不影响锚点。
    pub offset_ms: i64,
    pub show_translation: bool,
    pub geometry: WindowGeometry,
}

impl Default for LyricsSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            locked: true,
            font_family: "Microsoft YaHei".into(),
            font_size: 28.0,
            bold: true,
            base_color: "#FFFFFF".into(),
            highlight_color: "#00D2FF".into(),
            opacity: 0.9,
            outline_color: "#000000".into(),
            outline_width: 2.0,
            offset_ms: 0,
            show_translation: true,
            geometry: WindowGeometry::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WindowGeometry {
    /// 物理像素；None = 使用默认位置（主屏工作区底部居中）。
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: f64,
    pub monitor: Option<String>,
}

impl Default for WindowGeometry {
    fn default() -> Self {
        Self { x: None, y: None, width: 900.0, monitor: None }
    }
}

const STORE_FILE: &str = "settings.json";

pub fn load_settings(app: &AppHandle) -> Settings {
    let Ok(store) = app.store(STORE_FILE) else {
        eprintln!("[settings] store open failed");
        return Settings::default();
    };
    match store.get("settings") {
        Some(v) => serde_json::from_value(v).unwrap_or_default(),
        None => Settings::default(),
    }
}

pub fn save_settings(app: &AppHandle, settings: &Settings) {
    if let Ok(store) = app.store(STORE_FILE) {
        if let Ok(v) = serde_json::to_value(settings) {
            store.set("settings", v);
            if let Err(e) = store.save() {
                eprintln!("[settings] save failed: {e}");
            }
        }
    } else {
        eprintln!("[settings] store open failed (save)");
    }
}

/// 规范化服务器地址：去空白、去尾斜杠、无 scheme 时补 http://。
pub fn normalize_base_url(input: &str) -> String {
    let s = input.trim().trim_end_matches('/');
    if s.is_empty() {
        return String::new();
    }
    if s.starts_with("http://") || s.starts_with("https://") {
        s.to_string()
    } else {
        format!("http://{s}")
    }
}

/// http(s)://host:port -> ws(s)://host:port/ws
pub fn ws_url(base: &str) -> String {
    let s = base
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    format!("{s}/ws")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_merge_into_partial_json() {
        // 旧配置文件只有少量字段时回落默认值
        let v: serde_json::Value = serde_json::json!({ "lyrics": { "fontSize": 36.0 } });
        let s: Settings = serde_json::from_value(v).unwrap();
        assert_eq!(s.lyrics.font_size, 36.0);
        assert_eq!(s.lyrics.font_family, "Microsoft YaHei");
        assert!(s.lyrics.locked);
        assert_eq!(s.active_bot_id, None);
    }

    #[test]
    fn base_url_normalization() {
        assert_eq!(normalize_base_url(" 192.168.1.10:3000/ "), "http://192.168.1.10:3000");
        assert_eq!(normalize_base_url("https://a.b/"), "https://a.b");
        assert_eq!(normalize_base_url(""), "");
        assert_eq!(ws_url("http://h:3000"), "ws://h:3000/ws");
        assert_eq!(ws_url("https://h"), "wss://h/ws");
    }
}
