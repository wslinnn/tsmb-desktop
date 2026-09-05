use std::sync::Arc;

use crate::events;
use crate::state::SharedState;
use crate::types::{song_key, BotStatus, LyricLine};

/// 行查找（纯函数）：返回最后一个 time <= t 的行下标。后端已按 time 升序。
/// 二分 partition_point：lines[n-1].time <= t < lines[n].time。
pub fn find_line(lines: &[LyricLine], t: f64) -> Option<usize> {
    match lines.partition_point(|l| l.time <= t) {
        0 => None,
        n => Some(n - 1),
    }
}

/// 切歌 / 切换活跃 bot 时调用：命中缓存直接发数据，否则发 loading 再拉取。
/// 代数守卫：同一 bot 的下一次请求使上一次 in-flight 的结果作废
/// （快速切歌时旧响应不覆盖新歌）。
pub async fn ensure_lyrics(app: &tauri::AppHandle, state: &SharedState, status: &BotStatus) {
    let Some(song) = status.current_song.as_ref() else {
        events::emit_lyrics_data(app, state, &status.id, "", events::LyricsPhase::None);
        return;
    };
    let key = song_key(song);

    if let Some(cached) = state.lyrics_cache.lock().await.get(&key) {
        events::emit_lyrics_data_lines(app, state, &status.id, &key, cached.clone());
        return;
    }

    let gen = {
        let mut gens = state.lyrics_gen.lock().await;
        let g = gens.get(&status.id).copied().unwrap_or(0) + 1;
        gens.insert(status.id.clone(), g);
        g
    };
    events::emit_lyrics_data(app, state, &status.id, &key, events::LyricsPhase::Loading);

    let fetched = {
        let Some((base, token)) = state.creds().await else { return };
        crate::api::get_lyrics(&state.http, &base, &token, &song.platform, &song.id).await
    };

    // 守卫：期间又切歌了就丢弃
    if state.lyrics_gen.lock().await.get(&status.id).copied() != Some(gen) {
        return;
    }

    match fetched {
        Ok(lines) if !lines.is_empty() => {
            let shared = Arc::new(lines);
            let mut cache = state.lyrics_cache.lock().await;
            // 粗略上限防无界增长（超过 200 首整体清空；歌单极少超过）
            if cache.len() > 200 {
                cache.clear();
            }
            cache.insert(key.clone(), shared.clone());
            drop(cache);
            events::emit_lyrics_data_lines(app, state, &status.id, &key, shared);
        }
        Ok(_) => events::emit_lyrics_data(app, state, &status.id, &key, events::LyricsPhase::None),
        // 网络失败：不缓存（下次切回该曲重试），发 None 让前端显示歌名占位
        Err(_) => events::emit_lyrics_data(app, state, &status.id, &key, events::LyricsPhase::None),
    }
}

/// 供 tick 计算用：取某 songKey 的缓存行。
pub async fn cached_lines(state: &SharedState, key: &str) -> Option<Arc<Vec<LyricLine>>> {
    state.lyrics_cache.lock().await.get(key).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(time: f64) -> LyricLine {
        LyricLine { time, ..Default::default() }
    }

    #[test]
    fn find_line_binary_search_boundaries() {
        let lines = vec![line(1.0), line(2.0), line(4.0)];
        assert_eq!(find_line(&lines, 0.5), None); // 首行之前
        assert_eq!(find_line(&lines, 1.0), Some(0)); // 恰好首行
        assert_eq!(find_line(&lines, 2.9), Some(1)); // 行间
        assert_eq!(find_line(&lines, 3.0), Some(1)); // 空洞归前一行
        assert_eq!(find_line(&lines, 100.0), Some(2)); // 越过尾行
        assert_eq!(find_line(&[], 1.0), None); // 空歌词
    }

    #[test]
    fn offset_applied_by_caller_not_finder() {
        // 偏移在调用侧做减法后传入（见 ticker），行查找本身无偏移概念
        let lines = vec![line(10.0)];
        let t = 10.5 - 0.5; // offset +500ms：歌词延后
        assert_eq!(find_line(&lines, t), Some(0));
        let t = 10.4 - 0.5; // 偏移后还没到
        assert_eq!(find_line(&lines, t), None);
    }
}
