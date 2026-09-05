# tsmb-desktop

TeamSpeak 音乐机器人（[teamspeak-music-bot](https://github.com/wslinnn/teamspeak-music-bot)）的桌面端伴侣应用：
登录服务器、选择机器人，在桌面上悬浮显示实时滚动的歌词。

Tauri 2 + React 19 + TypeScript，状态核心在 Rust（WS 连接、边界驱动轮询、锚点插值、
内容缓存、托盘与穿透控制），两个窗口（主窗口 + 桌面歌词）为纯视图。
设计与协议见 [docs/](docs/)。

## 功能

- 登录 / 登出（`/api/client` Bearer token，60 天有效；改密码即全部失效）
- 机器人列表与选择，实时显示播放状态与进度
- 系统托盘常驻：主窗 ✕ = 隐藏到托盘（首次气泡提示），托盘左键单击唤回/隐藏主窗，
  右键菜单：显示设置 / 锁定解锁歌词（歌词关闭时置灰）/ 开关桌面歌词 / 退出
  （退出不改任何设置，下次启动按设置恢复）
- 桌面歌词：置顶 / 透明 / 无边框 / 不进任务栏 / 不抢焦点
  - 双行显示：有翻译 →「原文 + 翻译」；无翻译 →「当前行 + 下一行」
  - 样式：字体 / 字号 / 加粗 / 原文字色 / 高亮色 / 透明度 / 描边
  - 歌词偏移 ±0.5s 步进（只影响显示，不影响播放）
  - 锁定（鼠标穿透，游戏时可用）：歌词窗右上角常驻小锁按钮，鼠标悬停其上即可
    直接点击解锁，无需去设置窗口；托盘与设置窗解锁等效
  - 解锁态：悬浮显示半透明遮罩（标出区域边界与实时尺寸）+ 控制条；
    左右边缘可拉伸宽度（高度始终由字号推导），拖动 + 位置与宽度记忆
  - 行切换交叉淡入；超宽行自动来回跑马灯（替代截断省略）
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
npx tsx scripts/dev-server.mjs            # http://127.0.0.1:3999（alice / pw-alice-123）
PAUSED=1 npx tsx scripts/dev-server.mjs   # 启动即处于暂停态（调试暂停 / 恢复路径）
```

假后端数据落在 `scripts/dev-bot.db`（`DEV_DB=<路径>` 可换），登录 token 重启后依然有效；删掉该文件即重置。

测试：

```bash
cd src-tauri
cargo test                                             # 单测
TSMB_DEV_SERVER=http://127.0.0.1:3999 cargo test --test integration   # 集成（需假后端在跑）
```

## 打包与发版

```bash
npm run tauri build     # NSIS 安装包：src-tauri/target/release/bundle/nsis/
                        # 免安装绿色版：src-tauri/target/release/tsmb-desktop.exe
```

推送 `v*` 形式的 tag（如 `v0.2.0`）会触发 [release workflow](.github/workflows/release.yml)：
校验 tag 与 `src-tauri/tauri.conf.json` 的版本一致后自动构建，并把安装包 + 绿色版 exe
发布到 GitHub Release——发布说明取自 `.github/releases/<tag>.md`，缺省自动生成。
push / PR 由 [ci.yml](.github/workflows/ci.yml) 跑前端构建（tsc + vite）与 `cargo test`。

## 注意事项

- **服务器版本要求**：需要 teamspeak-music-bot **v2.2.0+**（`/api/client` Bearer
  token 鉴权通道自该版本引入；seek 补发 stateChange 也在同版本）。
- **公网部署必须 https**：Bearer token 走 `Authorization` 头，明文 http 下局域网内
  可见。家用局域网 http 可接受。
- **全屏独占游戏**中桌面歌词不可见（无边框窗口被独占模式覆盖，各音乐软件通病）；
  无边框/窗口化全屏正常。
- **反向代理**需放行 `/ws` 升级连接及其 `Authorization` 头（如 nginx
  `proxy_set_header Authorization $http_authorization;` 且不剥离自定义头）。
- Windows：需要 WebView2 Runtime（Win11 自带）；Rust 工具链为 MSVC。
- 改密码会使所有桌面端 token 失效（需重新登录）；每个账号最多 10 台设备，
  超出自动挤掉最旧的。

## 已知取舍

- 登出会关闭桌面歌词并清除启用开关（换取登出后的干净桌面），重新登录后需手动重新开启
- 托盘图标默认折叠在任务栏溢出区，常显需在「任务栏设置 → 其他系统托盘图标」开启
- token 存本地 settings.json（P1 迁 Windows 凭据管理器）
- 游客登录、行内卡拉OK渐变、平滑追赶为 P1

## License

[MIT](LICENSE) © TSMusicBot Contributors —— 与 teamspeak-music-bot 主项目一致。
