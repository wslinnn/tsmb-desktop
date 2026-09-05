# tsmb-desktop

TeamSpeak 音乐机器人（[teamspeak-music-bot](../teamspeak-music-bot)）的桌面端伴侣应用：
登录服务器、选择机器人，在桌面上悬浮显示实时滚动的歌词。

Tauri 2 + React 19 + TypeScript，状态核心在 Rust（WS/轮询/锚点插值/歌词缓存），
两个窗口（主窗口 + 桌面歌词）为纯视图。设计与协议见 [docs/](docs/)。

## 功能（v1）

- 登录 / 登出（`/api/client` Bearer token，60 天有效；改密码即全部失效）
- 机器人列表与选择，实时显示播放状态与进度
- 桌面歌词：置顶 / 透明 / 无边框 / 不进任务栏 / 不抢焦点
  - 双行显示：有翻译 →「原文 + 翻译」；无翻译 →「当前行 + 下一行」
  - 样式：字体 / 字号 / 加粗 / 原文字色 / 高亮色 / 透明度 / 描边
  - 歌词偏移 ±0.5s 步进（只影响显示，不影响播放）
  - 锁定（鼠标穿透，游戏时可用）/ 解锁（悬浮控制条：锁定、关闭、字号、偏移、打开设置）
  - 拖动 + 位置记忆；字号变化保持底边不动调高度
- 全部设置持久化；他端 seek 后歌词即时收敛（后端 seek 补发 stateChange；
  WS 断开时播放中 3s 轮询兜底）

## 开发

```bash
npm install
npm run tauri dev        # 需要一个后端：见下
```

联调可先用 teamspeak-music-bot 仓库的假后端（无需真实 TeamSpeak 服务器）：

```bash
# teamspeak-music-bot 仓库
npx tsx scripts/dev-server.mjs        # http://127.0.0.1:3999（alice / pw-alice-123）
```

测试：

```bash
cd src-tauri
cargo test                                             # 单测
TSMB_DEV_SERVER=http://127.0.0.1:3999 cargo test --test integration   # 集成（需假后端在跑）
```

## 打包

```bash
npm run tauri build     # 产出 NSIS 安装包 src-tauri/target/release/bundle/nsis/
```

## 注意事项

- **公网部署必须 https**：v1 的 Bearer token 走 `Authorization` 头，明文 http 下
  局域网内可见。家用局域网 http 可接受。
- **全屏独占游戏**中桌面歌词不可见（无边框窗口被独占模式覆盖，各音乐软件通病）；
  无边框/窗口化全屏正常。
- **反向代理**需放行 `/ws` 升级连接及其 `Authorization` 头（如 nginx
  `proxy_set_header Authorization $http_authorization;` 且不剥离自定义头）。
- Windows：需要 WebView2 Runtime（Win11 自带）；Rust 工具链为 MSVC。
- 改密码会使所有桌面端 token 失效（需重新登录）；每个账号最多 10 台设备，
  超出自动挤掉最旧的。

## 已知取舍（v1）

- 关闭主窗口时若桌面歌词开启，主窗口只是隐藏（托盘为 P1）
- 登出会关闭桌面歌词并清除启用开关（换取登出后的干净桌面），重新登录后需手动重新开启
- token 存本地 settings.json（P1 迁 Windows 凭据管理器）
- 游客登录、行内卡拉OK渐变、平滑追赶为 P1
