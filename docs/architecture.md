# tsmb-desktop 架构与协议

## 总体

Rust 持有全部状态与网络（鉴权、WS、轮询、锚点插值、歌词缓存、设置持久化）；
两个窗口是纯视图。Rust → 窗口走 Tauri event 广播，窗口 → Rust 走 invoke command。

```
┌─ Rust (src-tauri) ─────────────────────────────────────────────┐
│  http/api  ws  poller  timing  lyrics  settings  lyrics_window │
│  └─ AppState: 锚点[botId] / 歌词缓存 / 设置 / 活跃bot / token   │
└──────────────┬──────────────────────────┬──────────────────────┘
        event 广播                        event 广播 + command
       ┌──────┴──────┐              ┌──────┴──────┐
       │  主窗口      │              │  歌词窗口    │
       │ 登录/bot/设置│              │ 双行渲染     │
       └─────────────┘              └─────────────┘
```

选 Rust 做状态核心而非主窗口前端的原因：主窗口关闭后歌词窗口仍需工作；
HTTP/WS 本来就在 Rust（token 附加）；插值逻辑可单测。

## 目录结构

```
tsmb-desktop/
├─ docs/
├─ src/                          # React 前端，两窗口共用一个 bundle
│  ├─ main.tsx                   # 入口：按 getCurrentWebviewWindow().label 分流
│  ├─ windows/
│  │  ├─ main/
│  │  │  ├─ Login.tsx            # 服务器地址 + 账号密码
│  │  │  ├─ Bots.tsx             # bot 列表与选择
│  │  │  └─ Settings.tsx         # 歌词样式 + 预览 + 偏移调节
│  │  └─ lyrics/
│  │     ├─ LyricsWindow.tsx     # 桌面歌词渲染（订阅 tick/data/settings）
│  │     └─ ControlBar.tsx       # 解锁态悬浮控制条
│  ├─ events.ts                  # listen 封装 + payload 类型
│  ├─ commands.ts                # invoke 封装
│  ├─ types.ts                   # BotStatus / LyricLine 等（对齐后端契约）
│  └─ store.ts                   # zustand：get_state 水合 + 事件订阅更新
└─ src-tauri/src/
   ├─ main.rs                    # 插件注册、状态核心各任务启动
   ├─ state.rs                   # AppState（RwLock/Mutex）、启动水合
   ├─ http.rs                    # reqwest 封装：Bearer 附加、错误映射、401 上报
   ├─ api.rs                     # REST：login/logout/me/bots/elapsed/lyrics
   ├─ ws.rs                      # WS：重连退避、Ping→Pong、消息解析、4001
   ├─ timing.rs                  # 锚点插值（纯逻辑，单测重点）
   ├─ lyrics.rs                  # 歌词获取/LRU 缓存/拉取抖动/代数守卫/行查找
   ├─ poller.rs                  # elapsed 兜底轮询（WS 感知 15s/3s）+ 边界驱动 tick
   ├─ events.rs                  # 事件 payload 定义与广播
   ├─ commands.rs                # invoke 命令
   ├─ lyrics_window.rs           # 歌词窗口创建/销毁/锁定/位置记忆
   └─ settings.rs                # tauri-plugin-store 封装
```

## 核心类型（与后端契约对齐）

后端源头：`teamspeak-music-bot/src/music/provider.ts:62-68`、`src/bot/instance.ts:138-153`。

```ts
interface LyricLine { time: number; text: string; translation?: string; roma?: string } // time: 秒
interface Song { id: string; platform: string; title: string; artist: string; duration: number }
interface BotStatus {
  id: string; name: string; connected: boolean; playing: boolean; paused: boolean;
  currentSong: Song | null; queueSize: number; volume: number; playMode: string;
  elapsed: number; effectiveDuration?: number;
}
```

Rust 侧为 serde 镜像 struct；songKey 统一用 `` `${platform}:${id}` ``（对齐 Web 端 watch 键）。

## 事件协议（Rust → 前端）

| 事件 | 目标 | 频率 | payload |
|---|---|---|---|
| `auth-state` | 全部 | 变更时 | `{state:"logged-out"\|"logged-in", username?, server?, reason?}` |
| `connection-state` | 全部 | 变更时 | `{ws:"connecting"\|"open"\|"retrying"\|"closed", error?}` |
| `bots-updated` | 全部 | 变更时 | `{bots: BotStatus[]}`（全量，规模小） |
| `active-bot` | 全部 | 变更时 | `{botId: string \| null}` |
| `lyrics-data` | 全部 | 切歌/切换活跃 bot | `{botId, songKey, state:"loading"\|"ok"\|"none", lines?: LyricLine[]}` |
| `lyrics-tick` | lyrics 窗口 | 行边界驱动（见下） | 见下 |
| `settings-changed` | 全部 | 变更时 | `{settings: LyricSettings}`（全量） |

`lyrics-tick`：

```json
{
  "botId": "bot-1", "songKey": "netease:20391", "playing": true,
  "elapsed": 83.42,            // 渲染时间（未应用偏移的原始插值值）
  "lineIndex": 12, "nextIndex": 13,   // 已按 offset 修正后的行查找结果
  "offsetMs": -500,
  "anchor": { "elapsed": 83.30, "ageMs": 120 }  // 锚点真值 + 距同步的时长
}
```

- 发射时机（T1 边界驱动）：仅 `(lineIndex, songKey, playing)` 变化时才发射；
  播放中最多 1s 一次心跳兜锚点漂移；暂停/登出/歌词禁用/无锚点时停车
  （无定时器，纯事件唤醒：WS 事件 / 轮询锚点刷新 / 设置变更 / 登录态变更）
- 发射前写入 `last_tick` 快照：晚加载或重建的歌词窗从 `get_state` 补水
  （暂停态停车后不再有周期 tick，这是唯一来源）
- 前端只消费 `lineIndex` / `nextIndex` / `songKey`；`elapsed` / `anchor`
  为 P1 行内渐变预留（当前无消费者）：`t = anchor.elapsed + (anchor.ageMs
  + 事件到达以来耗时)/1000`，协议一次定好不再动

## 命令（前端 → Rust）

| 命令 | 参数 | 说明 |
|---|---|---|
| `get_state` | — | `{auth, connection, bots, activeBotId, settings, lyrics, tick}`，窗口挂载时水合；后两项为最近一次快照，补齐晚加载/重建窗口 |
| `login` | `{server, username, password}` | 存 token，随后拉 bots + 开 WS |
| `logout` | — | 调后端撤销 + 清本地 + 关歌词窗（清 enabled）+ 广播 auth-state |
| `select_bot` | `{botId}` | 切换活跃 bot（影响轮询与歌词窗口） |
| `set_lyrics_enabled` | `{enabled}` | 创建/销毁歌词窗口 |
| `set_lyrics_locked` | `{locked}` | `set_ignore_cursor_events` + 持久化 |
| `update_lyrics_settings` | `{patch}` | 合并持久化 + 广播 settings-changed + 唤醒 tick（重开窗口/offset 变化依赖此唤醒） |

## 设置 schema（tauri-plugin-store）

```json
{
  "server": { "baseUrl": "http://192.168.1.10:3000" },
  "auth": { "token": "…", "username": "…" },
  "activeBotId": null,
  "lyrics": {
    "enabled": true, "locked": true,
    "fontFamily": "Microsoft YaHei", "fontSize": 28, "bold": true,
    "baseColor": "#FFFFFF", "highlightColor": "#00D2FF",
    "opacity": 0.9, "outlineColor": "#000000", "outlineWidth": 2,
    "offsetMs": 0, "showTranslation": true,
    "geometry": { "x": null, "y": null, "width": 900, "monitor": null }
  }
}
```

## 关键流程

1. **启动**：load settings → 有 token：乐观置 logged-in，直接起 ws/poller/ticker
  （不调 /api/me 验证；WS 4001 / REST 401 会强制登出纠正）；无 token：主窗口出登录页
2. **登录**：`POST /api/client/login` → 存 token → 流程同上
3. **切歌**：WS `stateChange` → songKey 变化 → 重置锚点 + 唤醒 tick；歌词侧
   LRU 缓存（20 首）命中直接发 `ok`，否则 `loading` → fetch（0–1.5s 随机抖动
   错峰 + 代数守卫，旧响应丢弃）→ `ok`/`none`
4. **他端 seek**：后端 seek 补发 stateChange → WS 即时收敛（唤醒 tick 重算锚点）；
   WS 断开时 poller 播放中 3s 兜底硬同步
5. **锁定**：`set_lyrics_locked` → `set_ignore_cursor_events(true)`；解锁态 hover 显示控制条
6. **窗口关闭规则**：关主窗时歌词启用 → 隐藏主窗（应用存活）；否则退出应用。
   歌词窗关闭 → 销毁；主窗隐藏中则重新显示主窗。控制条「打开设置」= 显示主窗
   （前端直接 `WebviewWindow.getByLabel("main").show()`，无需新命令）。
   登出（主动或 401/4001 强制）→ 关闭歌词窗并清 enabled，回到干净桌面

## 实现要点（踩坑清单）

- **WS 保活**：tokio-tungstenite 收到 `Message::Ping` 必须回 `Pong`（服务端 25s ping）；
  close code 4001 = 会话过期 → 清 token 回登录页
- **路径编码**：kugou id 形如 `hash|albumAudio_id|album_id`，`/api/music/lyrics/:id`
  路径段必须 percent-encode
- **拖动**：`data-tauri-drag-region` 放在容器上，文本子节点也要带，否则点到文字拖不动
- **位置记忆**：存 `PhysicalPosition`（物理像素）+ 所在显示器指纹；恢复时若显示器不存在
  或越界，夹回主屏工作区底部居中（默认位）
- **窗口样式**：transparent 在 Windows/WebView2 可用；`shadow(false)` 避免默认投影穿帮；
  创建后单独调 `set_ignore_cursor_events`（按 locked 状态）
- **tick 任务（T1 重写为边界驱动）**：算出下一行边界时刻 →
  `sleep_until min(边界, 1s 心跳)`；仅 `(lineIndex, songKey, playing)` 变化才
  `emit_to("lyrics", ...)`；播放中才有定时器，暂停/登出/歌词禁用/无锚点纯事件
  停车（`state.tick_wake` Notify + `auth_rev` watch）；窗口不存在时 emit 无害
  （自动丢弃）
- **轮询任务**：WS open → 15s 纯兜底；WS 断开 → 播放中 3s、其余 15s；登出
  停车等 auth_rev，无活跃 bot 停车等 tick_wake（登录/切 bot 唤醒）
- **插值**：`Instant` 单调时钟；`playing==false` 冻结；非有限 elapsed 归零
  （NaN 会永久毒化行查找）；`elapsed` 钳制到
  `effectiveDuration ?? currentSong.duration`；歌词行查找
  `t_eff = elapsed - offsetMs/1000` 后二分取最后一个 `time <= t_eff`
- **reqwest**：rustls 特性；不启用 cookie store（手动 Bearer，无 cookie 语义）
- **前端路由**：不用 vue-router/react-router，`getCurrentWebviewWindow().label` 直接分流
- **水合顺序**：窗口挂载先 `listen` 全部事件再 `invoke(get_state)`；事件均为全量快照，
  乱序到达幂等覆盖，不丢事件也不怕重复
- **后端 WS tick 兼容**：若后端将来实施 state-sync-analysis.md 的周期 tick 广播，
  poller 可直接移除，锚点更新多一个来源即可，架构无需改动

## 歌词窗口创建（M4 参考）

```rust
tauri::WebviewWindowBuilder::new(app, "lyrics", WebviewUrl::App("index.html".into()))
    .title("desktop-lyrics")
    .transparent(true).decorations(false).always_on_top(true)
    .skip_taskbar(true).shadow(false).resizable(false).focused(false)
    .focusable(false)   // 点击/拖动不抢焦点（游戏场景关键；实现时确认 tauri 版本支持该 API）
    .inner_size(w, h).position(x, y)
    .build()?;
// 之后按 locked 状态调 set_ignore_cursor_events
```

宽度默认取主屏工作区 55%（clamp 600–1400），高度由字号推导并预留双行，
字号变化时保持底边不动调高度（参考 Mercurial-Player 的做法）。
