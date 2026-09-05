use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use crate::events;
use crate::state::SharedState;
use crate::types::{song_key, BotStatus, LyricLine};

/// 歌词缓存容量（LRU）：反复切回热门歌命中缓存；上限防无界增长。
/// 20 首 × 数 KB/首，内存代价可忽略。
pub const LYRICS_CACHE_CAP: usize = 20;

/// 歌词行 LRU 缓存：map 存数据，order 记访问序（尾部 = 最近使用）。
/// 容量小，O(n) 触碰即可，不值得引依赖。
pub struct LyricsCache {
    map: HashMap<String, Arc<Vec<LyricLine>>>,
    order: VecDeque<String>,
}

impl Default for LyricsCache {
    fn default() -> Self {
        Self::new()
    }
}

impl LyricsCache {
    pub fn new() -> Self {
        Self { map: HashMap::new(), order: VecDeque::new() }
    }

    pub fn get(&mut self, key: &str) -> Option<Arc<Vec<LyricLine>>> {
        let hit = self.map.get(key)?.clone();
        self.touch(key.to_string());
        Some(hit)
    }

    pub fn insert(&mut self, key: String, lines: Arc<Vec<LyricLine>>) {
        if self.map.len() >= LYRICS_CACHE_CAP && !self.map.contains_key(&key) {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            }
        }
        self.map.insert(key.clone(), lines);
        self.touch(key);
    }

    fn touch(&mut self, key: String) {
        self.order.retain(|k| *k != key);
        self.order.push_back(key);
    }
}

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
        events::emit_lyrics_data_lines(app, state, &status.id, &key, cached);
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

    // 多客户端对同一 bot 同歌切歌会同时打到后端/上游：0–1.5s 随机抖动错峰
    //（礼貌性代价：未命中缓存的首次拉取晚 ≤1.5s；时钟纳秒做抖动源，零依赖）
    let jitter_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::from(d.subsec_nanos()) % 1500)
        .unwrap_or(0);
    tokio::time::sleep(std::time::Duration::from_millis(jitter_ms)).await;

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
            state.lyrics_cache.lock().await.insert(key.to_string(), shared.clone());
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
    use proptest::prelude::*;

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

    fn put(cache: &mut LyricsCache, key: &str) {
        cache.insert(key.to_string(), Arc::new(vec![line(1.0)]));
    }

    #[test]
    fn lyrics_cache_lru_eviction_and_touch() {
        let mut c = LyricsCache::new();
        for i in 0..LYRICS_CACHE_CAP {
            put(&mut c, &format!("k{i}"));
        }
        // 访问 k0 使其变为最近使用，再插入新条目：被逐出的是 k1
        assert!(c.get("k0").is_some());
        put(&mut c, "new");
        assert!(c.get("k0").is_some(), "最近使用的 k0 不应被逐出");
        assert!(c.get("k1").is_none(), "最久未用的 k1 应被逐出");
        assert!(c.get("new").is_some());
        // 容量不增长
        assert_eq!(c.map.len(), LYRICS_CACHE_CAP);
    }

    #[test]
    fn lyrics_cache_reinsert_updates_order() {
        let mut c = LyricsCache::new();
        for i in 0..LYRICS_CACHE_CAP {
            put(&mut c, &format!("k{i}"));
        }
        // 重复插入已存在的 key：不触发逐出，只更新序
        put(&mut c, "k0");
        assert_eq!(c.map.len(), LYRICS_CACHE_CAP);
        // 再挤进来一个新条目：被逐出的是复活前的 k1（而非 k0）
        put(&mut c, "new");
        assert!(c.get("k0").is_some(), "复活的 k0 不应被逐出");
        assert!(c.get("k1").is_none());
    }

    proptest! {
        // E3 属性测试：行查找对任意非降时间轴与任意 t 不越界、语义正确
        #[test]
        fn prop_find_line_correct_and_in_bounds(
            times in proptest::collection::vec(0.0f64..600.0, 0..40),
            t in -10.0f64..700.0,
        ) {
            let mut sorted = times;
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let lines: Vec<LyricLine> = sorted.iter().map(|&time| line(time)).collect();
            match find_line(&lines, t) {
                None => {
                    // 空轴或 t 在首行之前
                    prop_assert!(lines.is_empty() || lines[0].time > t);
                }
                Some(i) => {
                    prop_assert!(i < lines.len(), "下标不越界");
                    prop_assert!(lines[i].time <= t, "当前行 time <= t");
                    prop_assert!(
                        i + 1 >= lines.len() || lines[i + 1].time > t,
                        "下一行 time > t（归前一行语义）"
                    );
                }
            }
        }

        #[test]
        fn prop_find_line_nan_query_is_none(t in -5.0f64..5.0) {
            let lines = vec![line(1.0), line(2.0)];
            prop_assert_eq!(find_line(&lines, f64::NAN), None);
            let expected = if t < 1.0 {
                None
            } else if t < 2.0 {
                Some(0)
            } else {
                Some(1)
            };
            prop_assert_eq!(find_line(&lines, t), expected);
        }
    }
}
