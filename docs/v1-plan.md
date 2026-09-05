# tsmb-desktop v1 任务拆解

Tauri v2 桌面端，v1 = 登录/登出/bot 选择 + 桌面歌词（P0 清单见文末）。
架构与协议见 [architecture.md](architecture.md)，后端改动草案见 [backend-token-auth.md](backend-token-auth.md)。

里程碑按依赖排序。M1 在 teamspeak-music-bot 仓库独立进行，可与其余并行。
规模标注：S ≤ 半天，M ≈ 1–2 天，L ≈ 3 天+。

## M0 工程脚手架 [S]

- `npm create tauri-app@latest`（React + TS + Vite），落在本目录
- 依赖：tauri-plugin-store、tauri-plugin-single-instance；Rust 侧 reqwest(rustls)、tokio-tungstenite、serde、thiserror
- 按 architecture.md 建目录骨架；前端入口按 window label 分流（main / lyrics 两个空页面）
- **平台风险前置验证**（全项目最大的技术假设，半天内在本机验证掉）：歌词窗口
  transparent + always_on_top + set_ignore_cursor_events + focusable(false) 四件套
  在目标 Windows/WebView2 上的实际表现（透明是否穿帮、穿透是否生效、点击是否不抢焦点）
- 验收：`cargo check` 通过，`tauri dev` 能同时起主窗口与歌词窗口（空内容）；
  四件套手工验证通过；双开应用时第二实例只聚焦第一实例

## M1 后端 token 鉴权 [M]（teamspeak-music-bot 仓库）

- 照 backend-token-auth.md 实现：client_tokens 表迁移、`POST /api/client/login`、
  `DELETE /api/client/session`、requireAuth Bearer 分支、csrf Bearer 放行、WS upgrade 支持
- 顺带微改动（可拆独立微 PR，建议同做）：`bot.seek()` 后补发一次 `stateChange`
  （见草案 §8）——消除「他端 seek 桌面端最坏 2s 才收敛」的最大同步盲区，Web 面板同样受益
- 测试：token 生命周期、REST/WS Bearer 鉴权、csrf 放行与 cookie 路径回归、限流复用
- 验收：curl 三连全通 —— login 拿 token → Bearer 拉 `/api/me` → Bearer 完成 WS 握手并收到 init

## M2 Rust 状态核心（无 UI）[L]

- `settings.rs`：store 读写 + 默认值合并
- `http.rs` / `api.rs`：reqwest 封装（Bearer 附加、错误映射）+ login/logout/me/bots/elapsed/lyrics
  - 注意：kugou 歌曲 id 含 `|`，拼 `/api/music/lyrics/:id` 必须做路径段 percent-encode
- `ws.rs`：连接管理、重连退避 1s→30s×10（对齐 Web 端）、收到 Ping 回 Pong（服务端 25s ping）、
  init/stateChange/botConnected/botDisconnected/botRemoved 解析、close 4001 = 会话过期
- `timing.rs`：锚点插值（Instant 单调时钟）、暂停冻结、effectiveDuration 钳制、
  songKey（`platform:id`）变化检测
- `lyrics.rs`：歌词获取 + 内存缓存（`(platform,id)` 键）+ 请求代数守卫防旧响应覆盖
- `poller.rs`：活跃 bot 的 elapsed 轮询（playing 2s / paused 15s）
- `events.rs` / `commands.rs`：事件广播 + `get_state`/`login`/`logout`/`select_bot`
- 单测：timing 全场景（播放/暂停/切歌/钳制/漂移重置）、WS 消息解析、歌词行二分查找
- 验收：`cargo test` 绿；调试命令实时打印活跃 bot elapsed，与 Web 面板对拍一致（±100ms）

## M3 主窗口 UI [M]

- 登录页：服务器地址 + 用户名密码（游客登录移到 P1，见「已知取舍」）
- bot 列表页：选择、在线/播放状态（订阅 `bots-updated`）
- 设置页：歌词样式全部 P0 项 + 实时预览区 + offset 调节（±0.5s、重置、显示当前值）
- 连接状态指示（`connection-state`）；会话过期（`auth-state`）自动回登录页
- 窗口关闭规则（无托盘下的完整闭环）：关主窗时歌词启用 → 隐藏主窗（应用存活）；
  歌词未启用 → 退出应用。歌词窗口关闭/控制条 X → 销毁歌词窗，若主窗处于隐藏则重新显示
- 验收：冷启动 → 登录 → 选 bot → 实时看到播放状态；上述关闭规则三条路径行为正确

## M4 歌词窗口 [L]

- `lyrics_window.rs`：运行时创建
  `WebviewWindowBuilder`（transparent / decorations:false / always_on_top / skip_taskbar /
  shadow:false / resizable:false / focused:false / focusable:false）、关闭即销毁、重开重建、
  位置记忆（PhysicalPosition + 显示器指纹，跨重启恢复，越界夹回工作区）
- 渲染：双行规则（有翻译「原文+翻译」，无翻译「当前行+下一行」）、当前行高亮色、
  暂停冻结、切歌换词、无歌词占位（「歌名 - 歌手」/「暂无歌词」）
- 悬浮控制条（仅解锁态 hover 显示）：锁定、关闭、字号 ±、偏移 ±0.5s、打开设置（显示主窗）
- 锁定：`set_ignore_cursor_events(true)`；解锁入口 = 控制条按钮 + 设置页开关
- 样式应用：字体/字号/粗细/原色/高亮色/透明度/描边色与宽度
- 拖动：`data-tauri-drag-region`（注意：文本子节点也要带该属性）
- 验收：过一遍 P0 清单

## M5 打磨与打包 [S/M]

- 错误与边界：服务器不可达提示、WS 断线指示、歌词加载失败重试
- README 注意事项：公网部署需 https；全屏独占模式下歌词不可见（通病）；反向代理需放行
  WS upgrade 的 Authorization 头
- 应用图标、NSIS 打包、干净 Windows 环境冒烟
- 验收：安装包在无开发环境的 Windows 上跑通全流程

## P0 验收清单（M4/M5 用）

实测记录（2026-09-05，对 dev-server + computer-use/Win32 验证）：

- [x] 登录 / 登出 / 会话过期处理（REST 401 与 WS 4001 都回登录页；e2e 测试覆盖）
- [x] bot 列表、选择、在线状态显示；活跃 bot 记忆跨重启
- [x] 歌词窗口：置顶 / 透明 / 无边框 / 不进任务栏（Win32 ex-style 0x8040118 五位全中）
- [x] 锁定（鼠标穿透）/ 解锁（控制条出现、可拖动）
- [x] 样式：字号、字体、粗细、原色、高亮色、透明度、描边 —— 即改即见
- [x] 偏移：±0.5s 步进、重置、持久化、仅影响歌词不影响锚点
- [x] 双行显示；暂停冻结；切歌 <1s 换词；他端 seek 收敛（后端补发
  stateChange，桌面端靠 WS 即时；集成测试验证轮询兜底）；无歌词占位
- [x] 全部设置持久化（token/样式/偏移/位置/活跃 bot；修复 store 目录缺失）
- [x] 位置记忆跨重启（拖动落盘物理坐标，重启后精确恢复）
- [ ] 干净 Windows 环境安装包冒烟（M5 收尾）

## 已知取舍（v1 记录在案）

- 游客登录移到 P1：guest 会话是 cookie 形态，支持它意味着为一条路径把 cookie 语义
  重新引入 Rust 客户端；P1 与后端 `/api/client/guest`（直接发 token）一起做
- token 明文存 tauri-plugin-store（P1 迁 Windows 凭据管理器 / keyring crate）
- 字体选择 = 预设列表 + 手动输入（P1 评估 `queryLocalFonts`）
- 行内卡拉OK 渐变、平滑追赶（P 控制器）、锁定态悬浮锚点、托盘 = P1
