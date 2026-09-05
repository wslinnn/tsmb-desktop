import { useEffect } from "react";
import { logout } from "../../commands";
import { hydrate, useStore } from "../../store";
import type { WsState } from "../../types";
import Login from "./Login";
import Bots from "./Bots";
import SettingsPanel from "./Settings";

const WS_LABEL: Record<WsState, string> = {
  connecting: "连接中",
  open: "已连接",
  retrying: "重连中",
  closed: "未连接",
};

export default function MainWindow() {
  const hydrated = useStore((s) => s.hydrated);
  const auth = useStore((s) => s.auth);
  const connection = useStore((s) => s.connection);
  const settings = useStore((s) => s.settings);
  const loggedIn = auth?.state === "logged-in";

  useEffect(() => {
    void hydrate();
  }, []);

  if (!hydrated) {
    return <div className="boot">加载中…</div>;
  }

  if (!loggedIn) {
    return (
      <>
        <Login defaultServer={settings?.server.baseUrl ?? ""} />
        {auth?.reason && <div className="session-expired">{auth.reason}，请重新登录</div>}
      </>
    );
  }

  const ws = connection?.ws ?? "closed";

  return (
    <main className="main-window">
      <header className="topbar">
        <span className="brand">tsmb-desktop</span>
        <span className={`ws-indicator ${ws}`} title={connection?.error ?? ""}>
          <i />
          {WS_LABEL[ws]}
        </span>
        <span className="spacer" />
        <span className="who">{auth.username}</span>
        <button className="ghost" onClick={() => void logout()}>
          登出
        </button>
      </header>

      <div className="content">
        <Bots />
        <SettingsPanel />
      </div>
    </main>
  );
}
