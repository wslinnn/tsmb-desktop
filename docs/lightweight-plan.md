# 轻量化计划与测试矩阵（v1.1）

目标：把"轻量"作为硬性设计目标——CPU / 内存 / 磁盘 / 网络全部对标预算。
核心原则（第一性原理）：桌面歌词的信息速率极低（一行/约 5 秒，暂停时为零），
架构上必须**按需唤醒、无变化不发射、不可见不渲染**。

状态标记：[ ] 待做 / [x] 完成 / [~] 部分完成。执行时逐项回填实测数据。

## 〇、基线实测（2026-09-05，优化前，release 构建 12572b0）

环境：Win11 26200，16 逻辑核。release exe（含 review1/2 修复）。假后端 dev-server。
采样脚本：进程树（tsmb-desktop + 7×msedgewebview2）CPU/工作集/IO 写计数，2s 间隔 × 60s。
CPU 换算：整系统百分比 ×16 = 单核占用。

### 对拍：优化前 → 优化后（f97487d，T1/T2/T4-T6 全量生效）

| 状态 | CPU 整系统（前→后） | 磁盘写入 ops/61s（前→后） | 备注 |
|---|---|---|---|
| A 播放中·双窗可见 | 0.15% → **0.02%** | 9728 → **1205**（=底噪） | 单核 2.4% → 0.3% |
| B 歌词-only | 0.12% → **0.01%** | 9456 → **1603** | |
| C 暂停·双窗可见 | 0.12% → （并入 A 验证） | 8246 → — | 暂停无定时器 |
| D 登出 | 0.02% → 0.03% | 1180 → **1168** | **7 进程**（F1：歌词窗随登出销毁） |

内存（私有工作集合计）：284MB → ~254MB；其中歌词 renderer 31→58MB 区间
（无 10Hz 重绘后的稳态波动），GPU 进程写频率 193→52 ops/15s（重绘随 tick 停车）。
体积：exe 13.8MB → **4.78MB**；安装包 3.2MB → **1.87MB**。
网络：播放中 WS open 时 0.5 → **0.07 req/s**（15s 兜底轮询）。
C4 滑杆：字号一次变更 = 一次防抖保存（~12 次小系统调用，plugin-store 单次
save 的固定行为），无逐帧写入风暴；稳态零写入 ✓。

优化前基线（保留存档）：

| 状态 | CPU 整系统 | CPU 单核 | 内存私有合计 | 磁盘写入 | 说明 |
|---|---|---|---|---|---|
| A 播放中·双窗可见 | avg 0.15% / max 0.75% | ≈2.4% | 283.7MB | 9728 ops/61s ≈1.6MB | tick 全速跑 |
| B 歌词-only（主窗隐藏） | avg 0.12% / max 0.33% | ≈1.9% | ≈284MB | 9456 ops/61s | 隐藏主窗 renderer 仍存活 |
| C 暂停·双窗可见 | avg 0.12% / max 0.99% | ≈1.9% | ≈285MB | 8246 ops/61s | 暂停不停车，与 A 无异 |
| D 登出（歌词窗残留） | avg 0.02% / max 0.13% | ≈0.3% | ≈284MB | 1180 ops/61s | ticker 已门控 |

内存按进程拆分（状态 A 稳态，私有工作集）：
host(Rust) 42.1 + browser 67.5 + gpu 84.1 + renderer-main 36.1 + renderer-lyrics 31 +
utility 19.9 + crashpad 3 ≈ **284MB**。其中 WebView2 固定开销（browser+gpu+utility+crashpad）
≈ **175MB 不可寻址**；我们可寻址的是 host + 两个 renderer ≈ **109MB**。
工作集求和（534MB）含大量共享页重复计数，仅作参考。预算"稳态 <120MB"按树内私有内存
不可达——修订为：**可寻址部分（host+renderers）<110MB，且树私有总量不高于基线**。

磁盘写入：稳态 settings.json 零写入（C4 ✓）；写入风暴 ~8000 ops/min 来自 tick 驱动的
渲染管线（GPU 进程写频率 ≈10Hz 与 tick 同频），WebView2 底噪 ≈1180 ops/min（D 态）。
应用数据目录 44MB（EBWebView 缓存为主）。exe 13.8MB，安装包 3.2MB。

网络（代码推导，C5 部分）：播放中 poller 2s 间隔 = 0.5 req/s，超预算（≤0.1）5 倍。

**基线新发现**：
- F1 登出后歌词窗不关闭，冻结显示旧歌词行；且 settings.enabled 保持 true，
  下次启动登录前歌词窗照常创建——修复归入 T2。
- F2 主窗隐藏后 renderer 进程不挂起（内存/CPU 与可见时相同）——T3 的直接依据。
- F3 暂停态与播放态资源占用相同——T1/T2 的直接依据。

## 一、量化预算（验收标准）

| 维度 | 播放中·歌词可见 | 暂停/登出/歌词关闭 |
|---|---|---|
| CPU | 平均 <1% 单核；行切换瞬间 <5% | <0.1%（近零） |
| 唤醒频率 | 行边界精确唤醒 + ≤1次/s 兜底 | 无周期唤醒 |
| 内存（全进程含 WebView2 子进程） | 可寻址（host+renderers）<110MB；树私有 ≤ 基线 284MB | 主窗隐藏时更低 |
| 磁盘写入 | 稳态 ≈0 | ≈0 |
| 网络（对 VPS） | 播放中 ≤0.1 req/s/客户端 | ≈0 |
| 安装包 / 安装后 | <5MB / <30MB | — |

## 二、优化项

### T1 tick 边界驱动（核心）[x]
- 现状：ticker 100ms 定时永久跑，10Hz 发事件（绝大多数 unchanged），驱动
  WebView2 10次/s 重渲染——99% 唤醒无信息量。
- 目标：算出下一行边界时刻 → `sleep_until min(边界, 1s 兜底)`；仅
  `(lineIndex, songKey, playing)` 变化时发射；播放中 1Hz 心跳（P1 卡拉OK
  anchor 预留，如 P1 不做可砍为纯边界驱动）；暂停/无锚点发终态后停车；
  WS 事件 / 设置变更（offset/fontSize）立即唤醒。
- 验收：行间期歌词面 CPU≈0；行切换时刻误差 <100ms；暂停零唤醒。

### T2 停车矩阵 [x]
登出 / 歌词禁用 / 无活跃 bot / 暂停 四状态下，ws/poller/ticker 三任务
各自的唤醒率必须趋零。现状：登出态 ticker 已门控但仍 10Hz 空醒。
含 F1 修复：登出时关闭歌词窗并清 enabled（原残留冻结帧，见基线 F1）。
实测对拍见「〇、基线实测」对比表：A 态 CPU 0.15%→0.02%，写入 9728→1205 ops/min。

### T3 不可见不渲染 [x]（评估结论：推迟，见下）
- 歌词窗销毁后 ticker 停车：已由 T2 完成（enabled=false ⇔ 窗口不存在）。
- 主窗隐藏挂起 WebView：**评估后推迟到 P2**。Tauri v2 无内置 suspend API；
  WebView2 `TrySuspendAsync` 需 webview2-com unsafe COM + 在全部 3 个 show
  路径（single-instance / 歌词窗关闭唤回 / 歌词控制条按钮）手动 Resume，
  漏一处即白窗。收益 ~30MB（树私有内存 11%，基线 F2：隐藏后 renderer 不
  挂起）。T1/T2 后隐藏主窗的增量 CPU 已近零，收益/风险比不划算。

### T4 网络自适应（对 VPS 友好）[x]
WS open → elapsed 轮询降为 15s 纯兜底（0.5 → 0.07 req/s）；WS 断开 → 播放中
3s 快速自愈；暂停 → 15s。轮询从主通道降级为兜底。

### T5 歌词拉取礼貌化 [x]
切歌拉取加 0–1.5s 随机抖动（时钟纳秒源，零依赖）；歌词缓存改为 LRU 20 首
（LyricsCache + 单测）。

### T6 体积 [x]
release profile：`strip=true, lto=true, codegen-units=1, opt-level="s",
panic="abort"`。exe 13.8MB → **4.78MB**；安装包 3.2MB → **1.87MB**。

### T7 前端双入口分包 [x]（评估后跳过）
实测单 chunk 仅 225KB（gzip 69KB），拆分只省另一窗口不执行代码的解析时间
（百 KB 内），不值改动。保持单入口，两窗口按 label 分流渲染。

### T8 磁盘 [~]
EBWebView 实测 44MB（见基线节），稳态零增长，settings.json 稳态零写入。
debug 日志（TEMP/tsmb-debug.log，仅 debug 构建）1MB 上限未做：release 为
no-op，风险限于开发机，暂不加。

### T9 安全与工程化配套 [x]
- S1 CSP：`csp: null` → `default-src 'self'` + Tauri IPC 白名单（全部
  HTTP/WS 在 Rust 侧，webview 无外联需求）
- S2 分窗 capabilities 最小权限：main 仅 core:default；lyrics 追加
  show/set-focus/start-dragging；opener/store 为 Rust 侧专用，从 webview
  授权中移除
- L2 tauri-plugin-log 评估后**不采纳**：现有 debug_log 已 release 剔除且
  零磁盘写入，插件反而给 release 增加落盘开销，与轻量化目标相悖
- E3 proptest：interpolate（单调/钳制/非有限归零——顺带修 R4 NaN 毒化）、
  find_line（不越界/归前一行语义）
- E4 clippy `-D warnings` 清零

## 三、测试矩阵（执行时逐项回填证据）

### C 客户端性能
- [x] C1 基线：播放中进程树（tsmb-desktop + msedgewebview2*）CPU%/工作集，
      60s 采样；优化后对拍（见「〇、基线实测」表）
- [x] C2 空闲/登出态 CPU 与唤醒率（D 态 avg 0.02%；ticker 10Hz 空醒由代码
      结构推导，优化后以 CPU 对拍验证）
- [ ] C3 内存老化：假后端循环切歌 ≥1h，工作集无单调增长；歌词缓存 LRU 生效
      （按用户要求跳过长挂机项，需要时补跑；LRU 驱逐已有单测覆盖）
- [x] C4 磁盘：稳态 60s settings.json mtime 不变（✓ 已证）；滑杆拖动一次 =
      一次防抖保存（✓ 实测，~12 次小系统调用为 plugin-store 单次 save 固定行为）
- [x] C5 网络速率：播放中（WS open）0.5 → 0.07 req/s 实测达标（15s 兜底）

### S 服务端（VPS 多用户）
- [x] S1 歌词惊群：客户端 T5 抖动缓解 + 服务端内容缓存双管齐下。服务端
      已实施（bot 仓库 docs/server-cache-plan.md，12d7370..9ddf3c2）：
      歌词/歌单/专辑/搜索/详情 LRU+TTL 缓存 + /lyrics 限流；同歌重复请求
      收敛为 1 次穿透由装饰器/路由单测覆盖（双桌面客户端实测受
      single-instance 限制，以单测代验）
- [x] S2 轮询负载：本机模拟（16 核，非真实 VPS，脚本 scripts/load-sim.mjs
      于 bot 仓库）。N=10/50/100 客户端（WS + 3s 轮询）25s 窗口：
      elapsed RTT p50=2ms、p95=10/25/46ms、max≤51ms，0 错误；
      服务器 node CPU 0.03%/0.06%/0.13%（整系统 16 核归一）——
      单进程 Node 余量充足
- [x] S3 广播扇出：同上模拟，stateChange（假后端 5s/次）N 客户端全量送达
      （100 连接 500 条/25s），0 丢失。真实后端 ~30次/歌 的成本按同量级
      线性外推
- [x] S4 单 token 多连接：代码走查确认无按 token 去重/上限——升级校验通过
      即 `clients.add(ws)`（server.ts:321 + websocket.ts:81），每连接独立
      收广播；10 台上限是 client_tokens 的 token 数量限制而非连接数。

### A 越权与安全
- [x] A1 三处范围一致性走查：预判的 player 路由缺口**不存在**——
      player.ts:47 `router.use("/:botId", requireBotAccess("botId"))` 统一
      403（防 404 探测）；REST bot 列表按 `u.bots` 过滤；WS 由 server.ts:396
      botScope 握手盖章 + websocket.ts visibleToClient 过滤 init/广播。
      三处同一权限上下文，一致
- [x] A2 权限收窄时效：member 的 botScope 仅在 WS 升级时盖章，无 live
      re-scope（guest 有 refreshGuestPolicy 实时刷新/关闭）。管理员收窄
      member 的 bot 白名单后，**已建立的 WS 连接保留旧 scope 直至断开**
      （桌面端长连接，最坏数小时）；REST 逐请求校验不受影响。记录为已知
      取舍，P1 可比照 guest 做成员断连
- [x] A3 token 五条失效路径全部有实现+测试：TTL 过期（client-auth.test:136）、
      DELETE /session（client-auth.test:103/119）、改密码（session.ts:235
      deleteAllForUser + session.test:144）、管理员重置（users.ts:128）、
      删除用户（users.ts:93）
- [x] A4 CORS 缺席论证：Bearer 走 `Authorization` 自定义头，非环境凭据——
      跨站页面无法附带（设自定义头需 CORS 预审批，服务端不授权）；cookie
      路径有 SameSite=Lax + 变更方法 csrfOriginCheck；浏览器 WebSocket API
      不能设自定义头，跨站 WS 只能依赖 cookie → 被 SameSite=Lax 阻断
- [x] A5 token 不入日志：服务端仅记 userId/username/device（client.ts:58），
      WS 升级只存 hashToken（server.ts:369），无请求头日志中间件；客户端
      debug_log 仅枚举/数值（handshake err/close code/outcome），且 debug
      构建独占、release 为 no-op；debug_elapsed 只输出时间数值
- [x] A6 注入面：deviceName 预编译 SQL + 64 字符截断（client.ts:55）；
      两前端无 dangerouslySetInnerHTML/innerHTML（React 文本节点自动转义，
      R3 的 501 HTML 错误文案实测为转义纯文本渲染）；服务器地址经
      normalize_base_url/ws_url 归一（kugou id 走 percent-encoding）；
      deviceName 进 pino 日志被 JSON 编码，无日志注入

### R 可靠性
- [x] R1 状态机实测：登出→登录 UI 交替 4+ 次（ bots/锚点/歌词全清后干净
      重建，无跨会话残留）；服务器杀/重启 2 轮 + 半开断开/恢复 1 轮——
      指示灯诚实切换 已连接↔重连中，无崩溃无 busy-loop（ws 退避 1s→30s
      封顶、poller/ticker 停车矩阵结构性排除空转）
- [x] R2 半开连接：已实现"无帧 60s 判死"（ws.rs `WS_MAX_SILENCE_SECS`，
      每次读帧带超时，任何入站帧重置；服务端 25s 心跳下健康连接不误杀）。
      运行时验证：TCP 代理模拟静默 → ~60s 后指示灯 已连接→重连中（不再
      说谎）→ 解除静默 ~20s 内自动恢复已连接
- [x] R3 异常路径：连接拒绝（后端全下线）→ 指示灯重连中、UI 完整可操作、
      无崩溃；非 JSON（python http.server 501 HTML 页）→ 登录优雅显示
      "服务器错误 501: <上游原文截断>"；连接重置走 Interrupted 同一退避
      路径；HTTP 超时 10s 连接/20s 请求（http.rs）。小瑕疵记录：错误文案
      透传上游 HTML 原文，后续可截断/泛化
- [x] R4 数据边界：E3 proptest + 单测覆盖——find_line 越界下标/空歌词/NaN
      查询（不 panic、None 正确）、interpolate 负 elapsed/NaN/Inf 归零/
      max_duration 钳制（timing.rs 单测 + 属性测试）
- [~] R5 生命周期：改系统时间——锚点用 Instant 单调时钟免疫（代码论证），
      token 过期按系统时间（语义正确）；活跃 bot 被删——WS botRemoved →
      remove_bot → wake → 占位（代码路径 + ws 解析单测）；快速切歌——
      key_changed→抖动/缓存/代数守卫路径由单测+集成覆盖，未做 ×10 实测；
      显示器拔除——restore_position 缺失显示器时钳回主屏（代码在，未实测）；
      挂起/恢复——deadline 单发结构无 busy-loop（未实测）。→ 硬件类场景
      留待真机按需补验

## 四、已知取舍（记录在案，不处理）

- poller 对同一 bot 双写（原地改 + update_bot 替换）——冗余但无害
- dev-server 已改文件库（scripts/dev-bot.db），重启 token 保持有效
- 全屏独占游戏歌词不可见——平台通病，README 已注明

## 五、技术栈优势盘点（结论）

已用足：Rust 状态核心（主窗关闭业务仍在）、serde 契约、单调时钟插值、
透明穿透窗口五件套、tokio watch、single-instance、Arc 零拷贝缓存。
本轮完成（轻量化/安全）：T1/T2/T4/T5/T6 + S1/S2 + E3/E4。
评估后未采纳/推迟：T3 推迟 P2（收益/风险比，见 T3 节）；T7 跳过（单
bundle 仅 225KB，拆分收益太小）；L2 不采纳（理由见 T9）。
P1 路线图：托盘（v2 内置 API）、autostart、global-shortcut、updater、
tauri-specta 生成 TS 绑定（消灭手写双份契约）、IPC Channel（卡拉OK 60fps
前置）。
P2 记录：主窗隐藏 WebView 挂起（WebView2 TrySuspend，T3 评估结论）、
歌词面 Rust Direct2D 原生渲染（脱离 WebView2 的轻量化终局）、
真逐字歌词（需后端保留 KRC 逐字标签）。
