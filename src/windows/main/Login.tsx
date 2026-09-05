import { FormEvent, useState } from "react";
import { login } from "../../commands";

export default function Login({ defaultServer }: { defaultServer: string }) {
  const [server, setServer] = useState(defaultServer || "http://127.0.0.1:3000");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await login(server.trim(), username.trim(), password);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="login-wrap">
      <form className="login-card" onSubmit={onSubmit}>
        <h1>tsmb-desktop</h1>
        <p className="login-sub">TeamSpeak 音乐机器人 · 桌面歌词</p>

        <label>
          服务器
          <input
            value={server}
            onChange={(e) => setServer(e.target.value)}
            placeholder="http://192.168.1.10:3000"
            autoFocus
            spellCheck={false}
          />
        </label>
        <label>
          用户名
          <input
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            autoComplete="username"
            spellCheck={false}
          />
        </label>
        <label>
          密码
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            autoComplete="current-password"
          />
        </label>

        {error && <div className="login-error">{error}</div>}

        <button type="submit" disabled={busy || !username || !password || !server.trim()}>
          {busy ? "登录中…" : "登录"}
        </button>
      </form>
    </div>
  );
}
