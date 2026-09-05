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

    if fetch_store_emit(app, state, status, &key).await {
        spawn_retry(app, state, status);
    }
}

/// 拉取 + 代数守卫 + 入缓存 + 发结果。返回 true = 网络失败（调用方可安排重试）。
async fn fetch_store_emit(
    app: &tauri::AppHandle,
    state: &SharedState,
    status: &BotStatus,
    key: &str,
) -> bool {
    let gen = {
        let mut gens = state.lyrics_gen.lock().await;
        let g = gens.get(&status.id).copied().unwrap_or(0) + 1;
        gens.insert(status.id.clone(), g);
        g
    };
    events::emit_lyrics_data(app, state, &status.id, key, events::LyricsPhase::Loading);

    let song = status.current_song.as_ref().expect("caller ensures current_song");
    let fetched = {
        let Some((base, token)) = state.creds().await else { return false };
        crate::api::get_lyrics(&state.http, &base, &token, &song.platform, &song.id).await
    };

    // 守卫：期间又切歌了就丢弃
    if state.lyrics_gen.lock().await.get(&status.id).copied() != Some(gen) {
        return false;
    }

    match fetched {
        Ok(lines) if !lines.is_empty() => {
            let shared = Arc::new(lines);
            let mut cache = state.lyrics_cache.lock().await;
            // 粗略上限防无界增长（超过 200 首整体清空；歌单极少超过）
            if cache.len() > 200 {
                cache.clear();
            }
            cache.insert(key.to_string(), shared.clone());
            drop(cache);
            events::emit_lyrics_data_lines(app, state, &status.id, key, shared);
            false
        }
        Ok(_) => {
            events::emit_lyrics_data(app, state, &status.id, key, events::LyricsPhase::None);
            false
        }
        // 网络失败：不缓存（下次切回该曲重试），发 None 让前端显示歌名占位
        Err(_) => {
            events::emit_lyrics_data(app, state, &status.id, key, events::LyricsPhase::None);
            true
        }
    }
}

/// 网络失败后的延时重试：30s 后若仍是这首且仍是活跃 bot，重拉一次。
/// 注意独立于 ensure_lyrics 调用，避免 async 递归导致的 Send 循环。
fn spawn_retry(app: &tauri::AppHandle, state: &SharedState, status: &BotStatus) {
    let app2 = app.clone();
    let state2 = state.clone();
    let bot2 = status.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        let still_active = state2.active_bot_id().await.as_deref() == Some(bot2.id.as_str());
        // 先克隆再判断，读守卫不得跨 await（非 Send + 潜在死锁）
        let cur = state2
            .bots
            .read()
            .await
            .iter()
            .find(|b| b.id == bot2.id)
            .cloned();
        let still_same_song = cur
            .as_ref()
            .and_then(|b| b.current_song.as_ref().map(song_key))
            == bot2.current_song.as_ref().map(song_key);
        if still_active && still_same_song {
            if let (Some(cur), Some(song)) = (cur, bot2.current_song.as_ref()) {
                let key = song_key(song);
                fetch_store_emit(&app2, &state2, &cur, &key).await;
            }
        }
    });
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
