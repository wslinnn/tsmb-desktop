use std::time::{Duration, Instant};

use crate::types::{song_key, BotStatus};

/// 每个 bot 一份的播放锚点（镜像 Web 端 TimingState，但用单调时钟）：
/// 最近一次服务端报告的 elapsed + 收到报告的本地时刻。
/// 显示时间 = server_elapsed + 距同步的单调时长（播放中），暂停冻结。
#[derive(Clone, Debug)]
pub struct TimingAnchor {
    pub server_elapsed: f64,
    pub sync_instant: Instant,
    pub playing: bool,
    pub song_key: Option<String>,
    /// effectiveDuration ?? song.duration（>0 才生效），防止插值越过片段末尾。
    pub max_duration: Option<f64>,
}

impl TimingAnchor {
    pub fn from_status(status: &BotStatus) -> Self {
        Self {
            server_elapsed: status.elapsed,
            sync_instant: Instant::now(),
            playing: status.is_progressing(),
            song_key: status.current_song.as_ref().map(song_key),
            max_duration: max_duration(status),
        }
    }

    pub fn render_elapsed(&self) -> f64 {
        interpolate(
            self.server_elapsed,
            self.sync_instant.elapsed(),
            self.playing,
            self.max_duration,
        )
    }

    /// 距锚点同步过了多久（供 lyrics-tick 的 anchor 字段）。
    pub fn age_ms(&self) -> f64 {
        self.sync_instant.elapsed().as_secs_f64() * 1000.0
    }
}

/// 纯函数插值（可测）：播放中推进 + 钳制；暂停冻结在 server_elapsed。
pub fn interpolate(
    server_elapsed: f64,
    since_sync: Duration,
    playing: bool,
    max_duration: Option<f64>,
) -> f64 {
    if !playing {
        return clamp(server_elapsed, max_duration);
    }
    clamp(server_elapsed + since_sync.as_secs_f64(), max_duration)
}

fn clamp(v: f64, max: Option<f64>) -> f64 {
    match max {
        Some(m) if m > 0.0 && v > m => m,
        _ => v,
    }
}

fn max_duration(status: &BotStatus) -> Option<f64> {
    status
        .effective_duration
        .or_else(|| status.current_song.as_ref().map(|s| s.duration))
        .filter(|d| *d > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paused_freezes_at_server_elapsed() {
        assert_eq!(interpolate(42.0, Duration::from_secs(10), false, None), 42.0);
        assert_eq!(interpolate(42.0, Duration::from_secs(0), false, Some(50.0)), 42.0);
    }

    #[test]
    fn playing_advances_by_wall_time() {
        let v = interpolate(10.0, Duration::from_millis(1500), true, None);
        assert!((v - 11.5).abs() < 1e-9, "{v}");
    }

    #[test]
    fn clamps_to_max_duration() {
        assert_eq!(interpolate(99.0, Duration::from_secs(5), true, Some(100.0)), 100.0);
        // 已超锚点值本身也钳（试听片段末尾的服务端真值）
        assert_eq!(interpolate(105.0, Duration::from_secs(0), true, Some(100.0)), 100.0);
        // 无效 max（0/负）不钳制
        assert_eq!(interpolate(105.0, Duration::from_secs(0), true, Some(0.0)), 105.0);
        assert_eq!(interpolate(105.0, Duration::from_secs(0), true, None), 105.0);
    }

    #[test]
    fn anchor_from_status_maps_fields() {
        let status = serde_json::from_value(serde_json::json!({
            "id": "b1", "name": "x", "playing": true, "paused": false,
            "elapsed": 12.5,
            "currentSong": { "id": "abc|1|2", "platform": "kugou", "duration": 200.0 },
            "effectiveDuration": 30.0
        }))
        .unwrap();
        let a = TimingAnchor::from_status(&status);
        assert_eq!(a.song_key.as_deref(), Some("kugou:abc|1|2"));
        assert_eq!(a.max_duration, Some(30.0)); // effectiveDuration 优先
        assert!(a.playing);
        assert!((a.render_elapsed() - 12.5).abs() < 0.5); // 刚建锚点 ≈ 真值
    }

    #[test]
    fn paused_status_means_frozen_anchor() {
        // 后端语义：暂停时 playing 仍为 true、elapsed 冻结 —— 必须看 paused
        let status = serde_json::from_value(serde_json::json!({
            "id": "b1", "playing": true, "paused": true, "elapsed": 7.0
        }))
        .unwrap();
        assert!(!TimingAnchor::from_status(&status).playing);
    }
}
