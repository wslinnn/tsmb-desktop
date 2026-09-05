import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import {
  setLyricsEnabled,
  setLyricsLocked,
  updateLyricsSettings,
} from "../../commands";
import { useStore } from "../../store";

const btn: React.CSSProperties = {
  padding: "3px 10px",
  fontSize: 12,
  borderRadius: 6,
  border: "1px solid rgba(255,255,255,.25)",
  background: "rgba(20,22,26,.85)",
  color: "#fff",
  cursor: "pointer",
};

/** 解锁态悬浮控制条（hover 出现）：锁定 / 关闭 / 字号± / 偏移±0.5s / 打开设置。 */
export default function ControlBar() {
  const settings = useStore((s) => s.settings);
  if (!settings) return null;
  const ly = settings.lyrics;

  const patch = (p: Parameters<typeof updateLyricsSettings>[0]) =>
    void updateLyricsSettings(p);

  const openMain = async () => {
    const w = await WebviewWindow.getByLabel("main");
    if (w) {
      await w.show();
      await w.setFocus();
    }
  };

  return (
    <div
      style={{
        position: "absolute",
        top: 4,
        right: 8,
        display: "flex",
        gap: 6,
        zIndex: 10,
      }}
    >
      <button style={btn} title="锁定歌词（鼠标穿透）" onClick={() => setLyricsLocked(true)}>
        锁定
      </button>
      <button style={btn} title="字号减小" onClick={() => patch({ fontSize: Math.max(16, ly.fontSize - 2) })}>
        A−
      </button>
      <button style={btn} title="字号增大" onClick={() => patch({ fontSize: Math.min(48, ly.fontSize + 2) })}>
        A+
      </button>
      <button style={btn} title="歌词提前 0.5s" onClick={() => patch({ offsetMs: ly.offsetMs - 500 })}>
        ◀0.5s
      </button>
      <button style={btn} title="歌词延后 0.5s" onClick={() => patch({ offsetMs: ly.offsetMs + 500 })}>
        0.5s▶
      </button>
      <button style={btn} title="打开主窗口设置" onClick={() => void openMain()}>
        设置
      </button>
      <button
        style={{ ...btn, color: "#ff9a9a" }}
        title="关闭桌面歌词"
        onClick={() => setLyricsEnabled(false)}
      >
        ✕
      </button>
    </div>
  );
}
