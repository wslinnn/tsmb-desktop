// 与 Rust 侧 serde 输出对齐（camelCase）。Rust 是契约源头，前端只做镜像。

export interface Song {
  id: string;
  platform: string;
  title: string;
  artist: string;
  duration: number;
}

export interface BotStatus {
  id: string;
  name: string;
  connected: boolean;
  playing: boolean;
  paused: boolean;
  currentSong: Song | null;
  queueSize: number;
  volume: number;
  playMode: string;
  elapsed: number;
  effectiveDuration?: number | null;
}

export interface LyricLine {
  time: number;
  text: string;
  translation?: string | null;
  roma?: string | null;
}

export interface WindowGeometry {
  x: number | null;
  y: number | null;
  width: number;
  monitor: string | null;
}

export interface LyricsSettings {
  enabled: boolean;
  locked: boolean;
  fontFamily: string;
  fontSize: number;
  bold: boolean;
  baseColor: string;
  highlightColor: string;
  opacity: number;
  outlineColor: string;
  outlineWidth: number;
  offsetMs: number;
  showTranslation: boolean;
  geometry: WindowGeometry;
}

export interface Settings {
  server: { baseUrl: string };
  auth: { token: string | null; username: string | null };
  activeBotId: string | null;
  lyrics: LyricsSettings;
}

export type AuthState = "logged-in" | "logged-out";

export interface AuthSnapshot {
  state: AuthState;
  username?: string | null;
  server?: string | null;
  reason?: string | null;
}

export type WsState = "connecting" | "open" | "retrying" | "closed";

export interface ConnSnapshot {
  ws: WsState;
  error?: string | null;
}

// —— 事件 payload ——

export interface BotsUpdatedEvent {
  bots: BotStatus[];
}

export interface ActiveBotEvent {
  botId: string | null;
}

export interface SettingsChangedEvent {
  settings: Settings;
}

export type LyricsPhase = "loading" | "ok" | "none";

export interface LyricsDataEvent {
  botId: string;
  songKey: string;
  state: LyricsPhase;
  lines?: LyricLine[] | null;
}

export interface TickEvent {
  botId: string;
  songKey: string;
  playing: boolean;
  elapsed: number;
  lineIndex: number | null;
  nextIndex: number | null;
  offsetMs: number;
  anchor: { elapsed: number; ageMs: number };
}

export function formatSec(sec: number): string {
  if (!Number.isFinite(sec) || sec < 0) return "0:00";
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${String(s).padStart(2, "0")}`;
}
