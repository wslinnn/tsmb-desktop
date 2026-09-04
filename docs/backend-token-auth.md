# 后端 PR 草案：/api/client/* token 鉴权（teamspeak-music-bot）

## 目标 / 非目标

**目标**：为非浏览器客户端（tsmb-desktop）提供一条显式凭证通道：`Authorization: Bearer`。
**非目标**：改动 Web 面板的 cookie 会话体系；多设备管理 UI；refresh token 轮换。

原则：纯增量。cookie 路径零行为变化，现有测试全部保持绿。

## 1. 数据迁移：client_tokens 表

```sql
CREATE TABLE client_tokens (
  token_hash   TEXT PRIMARY KEY,      -- sha256(token) 十六进制
  user_id      INTEGER NOT NULL,
  device_name  TEXT NOT NULL DEFAULT 'desktop',
  created_at   INTEGER NOT NULL,      -- Date.now() 毫秒，与 sessions 表同口径
  last_used_at INTEGER,
  expires_at   INTEGER NOT NULL
);
CREATE INDEX idx_client_tokens_user ON client_tokens(user_id);
```

token 明文只在创建响应里出现一次，库中只存哈希（256-bit 随机数，无需时序攻击防护）。
TTL 常量 60 天。`last_used_at` 节流更新（≥1h 才写一次，避免每请求写库）。

## 2. 端点：src/web/api/client.ts（新文件）

### POST /api/client/login

```jsonc
// 请求 { "username": "u", "password": "p", "deviceName": "desktop" }
// 响应 { "token": "…base64url…", "expiresAt": 1760000000000 }
```

- 校验逻辑**复用** `session.ts` 登录的 bcrypt 比对 + timing equalizer + 限流（5 次/分钟/IP+用户名）
  ——若当前逻辑内联在 session 路由里，先抽成共享函数再两处调用，不改行为
- token 生成：`crypto.randomBytes(32).toString("base64url")`；落库存 sha256

### DELETE /api/client/session（需 Bearer）

删除当前 token，返回 204。同时关闭用该 token 鉴权的 WS 连接（见 §5）。

### 挂载位置（关键）

login 本身没有 cookie 语义（凭证在请求体里），CSRF 不适用；且登录前不可能持有 token。
所以 **client 路由要在全局 `csrfOriginCheck` / `createRequireAuth` 之前挂载**，
自带独立的限流中间件。游客入口：P1 与 `/api/client/guest`（直接发 token）一起做，
v1 不做桌面端游客登录。

## 3. requireAuth.ts：新增 Bearer 分支

在现有 cookie 查询之前插入（示意）：

```ts
const authHeader = req.get("authorization");
if (authHeader?.startsWith("Bearer ")) {
  const row = tokens.findByHash(sha256hex(authHeader.slice(7)));
  if (!row || row.expiresAt < Date.now()) {
    return res.status(401).json({ error: "invalid token" });
  }
  maybeTouchLastUsed(row);            // 节流写
  req.authUser = { id: row.userId, isGuest: false };   // 字段名对齐现有 session 中间件
  return next();
}
// ……以下现有 cookie 路径原样保留……
```

注意：**无效 Bearer 直接 401，不回落 cookie**——显式凭证失败应当大声失败。

## 4. csrf.ts：Bearer 放行

`csrfOriginCheck` 顶部（SAFE_METHODS 判断之后）：

```ts
if (req.get("authorization")?.startsWith("Bearer ")) {
  next(); return;
}
```

依据：CSRF 防的是浏览器环境凭证（cookie）被跨站自动携带；显式 Authorization header
不存在这个攻击面。cookie 路径行为不变（现有 `csrf.test.ts` 全量回归）。

## 5. WS upgrade（server.ts 的 upgrade 处理内）

现有流程是解析 Cookie → 校验会话 → 在 socket 上打 `userId/isGuest/botScope` 标记。
在 Cookie 解析之前加一步：

```ts
const authHeader = req.headers.get("authorization") ?? null;
const tokenRow = authHeader?.startsWith("Bearer ")
  ? tokens.findByHash(sha256hex(authHeader.slice(7))) : null;
// 有效 → 打 userId 标记（并记 tokenId，供 DELETE /api/client/session 时定向关闭）
// 无效/过期 → 走现有 4001 关闭路径
// 无 Bearer → 落回现有 cookie 校验
```

Rust 客户端（tokio-tungstenite）握手可带自定义 header，不用 `?token=` 查询参数，
避免 token 进访问日志。

登出联动：`DELETE /api/client/session` 时按 socket 上记录的 tokenId 关闭对应 WS
（对齐现有 `onSessionsRevoked` 以 4001 关连接的语义）。

## 6. 测试清单（对齐现有 websocket-auth.test.ts / csrf.test.ts 风格）

- login：成功 / 密码错 / 限流触发；响应含 token 与 expiresAt
- token：过期后 REST 401；DELETE /api/client/session 后 REST 401
- requireAuth：Bearer 通过（`GET /api/me`）；无效 Bearer 401 且不回落 cookie
- csrf：Bearer 请求无 Origin 直接放行；无 Bearer 的写请求仍要求 Origin==Host（回归）
- WS：Bearer 握手成功并收到 init；无效 token 被拒；DELETE session 后 WS 被 4001 关闭
- cookie 路径全量回归（session 登录登出、WS cookie 鉴权）

## 7. 安全备注

- 桌面端与 bot 通常同局域网，v1 允许 http；README 注明公网部署必须 https
- token 泄露的撤销手段：改密码时**同时清除该用户的 client_tokens（连带 sessions 现有行为）**，
  外加 60 天硬过期；v1 不做设备列表管理 UI

## 8. 顺带微改动（建议同 PR 或独立微 PR）：seek 补发 stateChange

现状：`bot.seek()` 不 emit `stateChange`（`instance.ts` 的 seek 实现处无 emit），
他端 seek 时所有 WS 客户端无感知，只能靠轮询收敛（Web 3s / 桌面 2s）。

改法：seek 完成（ffmpeg 重启发起后，elapsed 已更新为新 seekOffset）补一次
`emit("stateChange")`。收益：全体客户端的 seek 收敛从秒级降到 <100ms，Web 面板的
「乐观锚点 + 500ms 校准」复杂度也可以借机简化。风险极低：seek 是离散 POST，
不会造成广播风暴。
