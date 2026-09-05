# 轻量化计划与测试矩阵（v1.1）

目标：把"轻量"作为硬性设计目标——CPU / 内存 / 磁盘 / 网络全部对标预算。
核心原则（第一性原理）：桌面歌词的信息速率极低（一行/约 5 秒，暂停时为零），
架构上必须**按需唤醒、无变化不发射、不可见不渲染**。

状态标记：[ ] 待做 / [x] 完成 / [~] 部分完成。执行时逐项回填实测数据。

## 〇、基线实测（2026-09-05，优化前，release 构建 12572b0）

环境：Win11 26200，16 逻辑核。release exe（含 review1/2 修复）。假后端 dev-server。
采样脚本：进程树（tsmb-desktop + 7×msedgewebview2）CPU/工作集/IO 写计数，2s 间隔 × 60s。
CPU 换算：整系统百分比 ×16 = 单核占用。

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

### T1 tick 边界驱动（核心）[ ]
- 现状：ticker 100ms 定时永久跑，10Hz 发事件（绝大多数 unchanged），驱动
  WebView2 10次/s 重渲染——99% 唤醒无信息量。
- 目标：算出下一行边界时刻 → `sleep_until min(边界, 1s 兜底)`；仅
  `(lineIndex, songKey, playing)` 变化时发射；播放中 1Hz 心跳（P1 卡拉OK
  anchor 预留，如 P1 不做可砍为纯边界驱动）；暂停/无锚点发终态后停车；
  WS 事件 / 设置变更（offset/fontSize）立即唤醒。
- 验收：行间期歌词面 CPU≈0；行切换时刻误差 <100ms；暂停零唤醒。

### T2 停车矩阵 [ ]
登出 / 歌词禁用 / 无活跃 bot / 暂停 四状态下，ws/poller/ticker 三任务
各自的唤醒率必须趋零。现状：登出态 ticker 已门控但仍 10Hz 空醒。
含 F1 修复：登出时关闭歌词窗并清 enabled（当前残留冻结帧，见基线 F1）。

### T3 不可见不渲染 [ ]
主窗隐藏（歌词-only 模式）时挂起其 WebView（WebView2 TrySuspend，评估
Tauri v2 API 暴露方式），唤回时 resume；歌词窗销毁后 ticker 停车。

### T4 网络自适应（对 VPS 友好）[ ]
WS open → elapsed 轮询降为 15s 纯兜底；WS 断开 → 3s 快速自愈；暂停 → 15s。
轮询从主通道降级为兜底（M1 后 seek 也走 WS 广播）。

### T5 歌词拉取礼貌化 [ ]
切歌拉取加 0–1.5s 随机抖动（多客户端天然错峰）；歌词缓存从"200 首全清"
改为 LRU 20 首。

### T6 体积 [ ]
release profile：`strip=true, lto=true, codegen-units=1, opt-level="s",
panic="abort"`；NSIS LZMA。现状 exe 14MB → 目标 <8MB；安装包 <4MB。
用 cargo-bloat 归因。

### T7 前端双入口分包（可选）[ ]
main/lyrics 各自 chunk，互不加载对方代码（百 KB 级收益）。

### T8 磁盘 [ ]
debug 日志（TEMP/tsmb-debug.log，仅 debug 构建）加 1MB 上限；
测量 WebView2 用户数据目录（EBWebView）体积。

### T9 安全与工程化配套 [ ]
- S1 CSP：`csp: null` → 收紧为本地资源白名单（脚手架欠账，真实加固）
- S2 分窗 capabilities 最小权限：歌词窗不给自己用不到的 store/opener 权限
- L2 tauri-plugin-log 替换手写 debug_log（分级/轮转/release 剔除）
- E3 proptest：interpolate / find_line 属性测试（单调、不越界）
- E4 clippy `-D warnings` 清零并入检查

## 三、测试矩阵（执行时逐项回填证据）

### C 客户端性能
- [x] C1 基线：播放中进程树（tsmb-desktop + msedgewebview2*）CPU%/工作集，
      60s 采样；优化后对拍（见「〇、基线实测」表）
- [x] C2 空闲/登出态 CPU 与唤醒率（D 态 avg 0.02%；ticker 10Hz 空醒由代码
      结构推导，优化后以 CPU 对拍验证）
- [ ] C3 内存老化：假后端循环切歌 ≥1h，工作集无单调增长；歌词缓存 LRU 生效
- [~] C4 磁盘：稳态 60s settings.json mtime 不变（✓ 已证）；滑杆拖动一次写
      次数 ≤3（待测）
- [~] C5 网络速率：播放中 0.5 req/s（代码推导，超预算 5 倍，T4 后复测）

### S 服务端（VPS 多用户）
- [ ] S1 歌词惊群：N 客户端同 bot 切歌，假后端 getLyrics 计数器实测上游
      调用量（预判：无服务端缓存时 = N 次/歌，需 T5 抖动缓解 + 建议后端缓存）
- [ ] S2 轮询负载：N=10/50/100 模拟客户端（WS+轮询脚本），VPS CPU/事件循环延迟
- [ ] S3 广播扇出：stateChange ~30次/歌 × N 连接成本
- [ ] S4 单 token 多连接：验证服务端无每 token 连接上限（预判缺失，记录）

### A 越权与安全
- [ ] A1 受限 member：REST /api/bot、/api/player/:id/elapsed、WS init 三处
      范围一致性（player 路由是否校验 per-user bot 可见性——预判 Web 端既有缺口）
- [ ] A2 权限收窄时效：member WS botScope 握手盖章、无 live re-scope
      （仅 guest 有）——量化暴露窗口，记录
- [ ] A3 token 五条失效路径：过期/DELETE/改密码/管理员重置/删除用户（已有
      e2e+单测，回归即可）
- [ ] A4 CORS 缺席论证：Bearer 放行 csrf 的安全性依赖"跨站无法附带自定义头"
- [ ] A5 token 不入日志：服务端日志 / 客户端 debug_log / debug_elapsed 输出
- [ ] A6 注入面：deviceName 入库、服务器地址拼 URL、歌名渲染——无 XSS/注入

### R 可靠性
- [ ] R1 三任务状态机穷举：登出→登录快速交替、服务器时好时坏，无 busy-loop
- [ ] R2 半开连接（服务器断电无 Close 帧）：客户端缺死连接检测（预判缺失，
      指示灯说谎）——补"无帧 60s 判死"逻辑
- [ ] R3 异常路径：超时/连接重置/DNS 失败/非 JSON 响应（反代错误页）降级
- [ ] R4 数据边界：lineIndex 越界、空歌词、负 elapsed、超大 duration
- [ ] R5 生命周期：挂起/恢复、改系统时间、快速切歌×10、活跃 bot 被删、
      显示器拔除位置恢复

## 四、已知取舍（记录在案，不处理）

- poller 对同一 bot 双写（原地改 + update_bot 替换）——冗余但无害
- dev-server 已改文件库（scripts/dev-bot.db），重启 token 保持有效
- 全屏独占游戏歌词不可见——平台通病，README 已注明

## 五、技术栈优势盘点（结论）

已用足：Rust 状态核心（主窗关闭业务仍在）、serde 契约、单调时钟插值、
透明穿透窗口五件套、tokio watch、single-instance、Arc 零拷贝缓存。
本轮补齐（轻量化/安全）：T1-T8、S1/S2、L2、E3/E4。
P1 路线图：托盘（v2 内置 API）、autostart、global-shortcut、updater、
tauri-specta 生成 TS 绑定（消灭手写双份契约）、IPC Channel（卡拉OK 60fps
前置）、歌词 LRU。
P2 记录：歌词面 Rust Direct2D 原生渲染（脱离 WebView2 的轻量化终局）、
真逐字歌词（需后端保留 KRC 逐字标签）。
