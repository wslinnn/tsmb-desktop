import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { hydrate, useStore } from "../../store";
import { setLyricsLockHotspot, setLyricsLocked } from "../../commands";
import { lineStyles } from "../../lyricsStyle";
import type { CSSProperties } from "react";
import type { BotStatus, LyricsDataEvent, Settings, TickEvent } from "../../types";
import ControlBar from "./ControlBar";

/** 桌面歌词窗口：消费 lyrics-data（歌词行）+ lyrics-tick（行边界驱动，仅
 *  行切换/切歌/暂停恢复时到达）。双行规则：有翻译 → 「原文+翻译」；
 *  无翻译 → 「当前行+下一行」；无歌词/首行前/加载中 → 歌名占位。
 *  锁定态整窗穿透，但 Rust 光标轮询在锁按钮热区内临时解除穿透，
 *  使按钮可直接点击解锁；解锁态 hover 显示遮罩，宽度可边缘拉伸。 */

/** 行切换交叉淡入：返回上一段文本（150ms 后清除），文本未变时为 null。
 *  用 useLayoutEffect 保证旧行层在绘制前就位（真交叉淡入，无一帧空档）。 */
function usePrevText(text: string): string | null {
  const prevRef = useRef(text);
  const [prev, setPrev] = useState<string | null>(null);
  useLayoutEffect(() => {
    if (prevRef.current === text) return;
    setPrev(prevRef.current);
    prevRef.current = text;
    const t = setTimeout(() => setPrev(null), 160);
    return () => clearTimeout(t);
  }, [text]);
  return prev;
}

/** 单行歌词：新行淡入 + 旧行淡出（grid 同格叠放）；超宽行来回跑马灯。
 *  跑马灯结构必须是「外层裁剪 + 内层平移」——transform 加在裁剪元素
 *  自身上是刚性移动，被裁掉的内容永远进不来。 */
function LyricLine({ text, prevText, style }: { text: string; prevText: string | null; style: CSSProperties }) {
  const ref = useRef<HTMLDivElement>(null);
  const [marqueeW, setMarqueeW] = useState(0);

  // 文本变化后测量溢出宽度，超宽才启用跑马灯（替代截断省略号）
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    setMarqueeW(Math.max(0, el.scrollWidth - el.clientWidth));
  }, [text]);

  const clip: CSSProperties = {
    gridArea: "1/1",
    maxWidth: "100%",
    overflow: "hidden",
  };
  const inner: CSSProperties = {
    whiteSpace: "nowrap",
    width: marqueeW > 4 ? "max-content" : "100%",
    textAlign: marqueeW > 4 ? "left" : "center",
  };
  const marquee =
    marqueeW > 4
      ? {
          "--mw": `${marqueeW}px`,
          animation: `lyric-marquee ${Math.max(5, marqueeW / 40)}s ease-in-out 180ms infinite alternate`,
        }
      : {};
  return (
    <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr)", width: "96%", margin: "0 auto" }}>
      <div key={text} ref={ref} data-tauri-drag-region style={{ ...clip, ...style, animation: "lyric-in 150ms ease-out" }}>
        <div data-tauri-drag-region style={{ ...inner, ...marquee }}>{text}</div>
      </div>
      {prevText != null && (
        <div data-tauri-drag-region style={{ ...clip, ...style, animation: "lyric-out 150ms ease-out forwards" }}>
          <div data-tauri-drag-region style={inner}>{prevText}</div>
        </div>
      )}
    </div>
  );
}

/** 歌词正文：从共享状态派生当前/下一行文本，带交叉淡入与跑马灯。
 *  独立成组件是为了让 usePrevText 等 hook 在 settings 就绪后稳定调用。 */
function LyricsText({ settings, lyrics, tick, bots, activeBotId }: {
  settings: Settings;
  lyrics: LyricsDataEvent | null;
  tick: TickEvent | null;
  bots: BotStatus[];
  activeBotId: string | null;
}) {
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
  const prevMain = usePrevText(main);
  const prevSub = usePrevText(sub);

  return (
    <>
      <LyricLine text={main} prevText={prevMain} style={ms} />
      {sub && <LyricLine text={sub} prevText={prevSub} style={{ ...ss, lineHeight: 1.4 }} />}
    </>
  );
}

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
  const lockRef = useRef<HTMLButtonElement>(null);

  const lockedNow = settings ? settings.lyrics.locked : false;
  const [size, setSize] = useState({ w: window.innerWidth, h: window.innerHeight });
  // 拉伸进行中（Rust Resized 事件流广播）：模态拉伸循环里 DOM 收不到
  // 鼠标事件，hover 不可靠，遮罩显示条件取两者并集
  const [resizing, setResizing] = useState(false);

  useEffect(() => {
    const un = listen<boolean>("lyrics-resizing", (e) => setResizing(e.payload));
    return () => {
      void un.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    const onResize = () => setSize({ w: window.innerWidth, h: window.innerHeight });
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  // 锁定态：把锁按钮热区报给 Rust（光标轮询据此动态解除窗口穿透）。
  // 窗口/按钮尺寸或位置变化都重报；未上报时 Rust 回退右上角默认区。
  useEffect(() => {
    if (!lockedNow) return;
    const report = () => {
      const el = lockRef.current;
      if (!el) return;
      const r = el.getBoundingClientRect();
      void setLyricsLockHotspot(r.left, r.top, r.width, r.height);
    };
    report();
    const ro = new ResizeObserver(report);
    if (lockRef.current) ro.observe(lockRef.current);
    window.addEventListener("resize", report);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", report);
    };
  }, [lockedNow]);

  if (!settings) return null;
  const ly = settings.lyrics;
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
        position: "relative",
      }}
    >
      {locked && (
        <button
          ref={lockRef}
          title="点击解锁歌词"
          onClick={() => {
            void setLyricsLocked(false).then(() => setHovered(true)); // 光标就在窗上，直接显示控制条
          }}
          style={{
            position: "absolute",
            top: 8,
            right: 8,
            width: 36,
            height: 36,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            padding: 0,
            border: "1px solid rgba(255,255,255,0.35)",
            borderRadius: 8,
            background: "rgba(0,0,0,0.4)",
            color: "#fff",
            cursor: "pointer",
            opacity: 0.75,
            zIndex: 10,
          }}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" aria-hidden>
            {/* 开锁图标：锁体 + 开着的锁环 */}
            <rect x="4" y="10" width="16" height="11" rx="2.5" fill="currentColor" />
            <path
              d="M8 10V7a4 4 0 0 1 7.7-1.5"
              stroke="currentColor"
              strokeWidth="2.4"
              strokeLinecap="round"
              fill="none"
            />
          </svg>
        </button>
      )}
      {(hovered || resizing) && !locked && (
        <>
          {/* 半透明遮罩：标出歌词区域边界 + 实时尺寸（拉伸拖拽中随 resize 刷新） */}
          <div
            style={{
              position: "absolute",
              inset: 0,
              background: "rgba(80,160,255,0.10)",
              border: "1px dashed rgba(255,255,255,0.55)",
              pointerEvents: "none",
            }}
          />
          <div
            style={{
              position: "absolute",
              top: 8,
              left: 10,
              fontSize: 12,
              lineHeight: "18px",
              color: "rgba(255,255,255,0.9)",
              background: "rgba(0,0,0,0.45)",
              padding: "2px 8px",
              borderRadius: 6,
              pointerEvents: "none",
            }}
          >
            {Math.round(size.w)}×{Math.round(size.h)}
          </div>
          <ControlBar />
        </>
      )}
      <LyricsText settings={settings} lyrics={lyrics} tick={tick} bots={bots} activeBotId={activeBotId} />
    </div>
  );
}
