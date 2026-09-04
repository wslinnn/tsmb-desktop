import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export default function MainWindow() {
  const [locked, setLocked] = useState(true);

  const toggle = async () => {
    const next = !locked;
    await invoke("toggle_lyrics_lock", { locked: next });
    setLocked(next);
  };

  return (
    <main style={{ padding: 24, fontFamily: "system-ui, sans-serif" }}>
      <h1>tsmb-desktop</h1>
      <p>主窗口占位（M3 实装：登录 / bot 选择 / 歌词设置）</p>
      <button onClick={toggle}>{locked ? "解锁歌词窗口" : "锁定歌词窗口"}</button>
    </main>
  );
}
