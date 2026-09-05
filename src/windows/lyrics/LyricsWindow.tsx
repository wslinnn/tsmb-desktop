import { useEffect } from "react";
import { hydrate, useStore } from "../../store";
import { lineStyles } from "../../lyricsStyle";
import ControlBar from "./ControlBar";

/** 桌面歌词窗口：消费 lyrics-data（歌词行）+ lyrics-tick（行边界驱动，仅
 *  行切换/切歌/暂停恢复时到达）。双行规则：有翻译 → 「原文+翻译」；
 *  无翻译 → 「当前行+下一行」；无歌词/首行前/加载中 → 歌名占位。
 *  锁定态整窗穿透（Rust set_ignore_cursor_events）。 */
export default function LyricsWindow() {
  useEffect(() => {
    void hydrate();
  }, []);

  const settings = useStore((s) => s.settings);
  const lyrics = useStore((s) => s.lyrics);
  const tick = useStore((s) => s.tick);
  const bots = useStore((s) => s.bots);
  const activeBotId = useStore((s) => s.activeBotId);
  const hovered = useStore((s) => s.lyricsHovered);
  const setHovered = useStore((s) => s.setLyricsHovered);

  if (!settings) return null;
  const ly = settings.lyrics;

  // tick 与歌词数据是两条事件流，songKey 对齐才可用（切歌瞬间的竞态防护）
  const linesReady =
    lyrics?.state === "ok" && lyrics.lines && tick && lyrics.songKey === tick.songKey;

  const bot = bots.find((b) => b.id === activeBotId) ?? null;
  const song = bot?.currentSong ?? null;

  let main: string;
  let sub = "";
  if (linesReady && tick.lineIndex != null) {
    const cur = lyrics!.lines![tick.lineIndex];
    main = cur.text;
    if (cur.translation && ly.showTranslation) {
      sub = cur.translation;
    } else if (tick.nextIndex != null) {
      sub = lyrics!.lines![tick.nextIndex].text;
    }
  } else if (linesReady) {
    // 歌词已就绪但还在第一行之前（前奏）：显示歌名 + 首行预览，
    // 不能写"暂无歌词"——歌词明明存在
    main = song ? `${song.title} — ${song.artist}` : "tsmb-desktop 桌面歌词";
    sub = lyrics!.lines![0].text;
  } else if (lyrics?.state === "loading") {
    main = song ? `${song.title} — ${song.artist}` : "歌词加载中…";
    sub = "歌词加载中…";
  } else {
    // 无歌词 / 未播放
    main = song ? song.title : "tsmb-desktop 桌面歌词";
    sub = song ? "暂无歌词" : "等待播放…";
  }

  const { main: ms, sub: ss } = lineStyles(ly);
  const locked = ly.locked;

  return (
    <div
      data-tauri-drag-region
      onMouseEnter={() => !locked && setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        width: "100vw",
        height: "100vh",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: 6,
        userSelect: "none",
        opacity: ly.opacity,
        cursor: locked ? "default" : "move",
        overflow: "hidden",
      }}
    >
      {hovered && !locked && <ControlBar />}
      <div
        data-tauri-drag-region
        style={{
          ...ms,
          lineHeight: 1.5,
          maxWidth: "96%",
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
      >
        {main}
      </div>
      {sub && (
        <div
          data-tauri-drag-region
          style={{
            ...ss,
            lineHeight: 1.4,
            maxWidth: "96%",
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {sub}
        </div>
      )}
    </div>
  );
}
