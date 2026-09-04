export default function LyricsWindow() {
  return (
    <div
      data-tauri-drag-region
      style={{
        width: "100vw",
        height: "100vh",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: 8,
        userSelect: "none",
      }}
    >
      <div
        data-tauri-drag-region
        style={{
          fontSize: 28,
          fontWeight: 700,
          color: "#ffffff",
          textShadow: "0 1px 2px rgba(0,0,0,.9), 0 0 6px rgba(0,0,0,.8)",
        }}
      >
        桌面歌词窗口 · M0 四件套验证
      </div>
      <div
        data-tauri-drag-region
        style={{ fontSize: 15, color: "rgba(255,255,255,.75)", textShadow: "0 1px 2px rgba(0,0,0,.9)" }}
      >
        解锁后可拖动（drag-region） · 默认锁定穿透
      </div>
    </div>
  );
}
