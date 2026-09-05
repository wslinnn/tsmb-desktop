import { invoke } from "@tauri-apps/api/core";
import type { Settings } from "./types";

export function login(server: string, username: string, password: string): Promise<void> {
  return invoke("login", { server, username, password });
}

export function logout(): Promise<void> {
  return invoke("logout");
}

export function selectBot(botId: string): Promise<void> {
  return invoke("select_bot", { botId });
}

export function getState(): Promise<{
  auth: unknown;
  connection: unknown;
  bots: unknown;
  activeBotId: string | null;
  settings: Settings;
  lyrics: import("./types").LyricsDataEvent | null;
}> {
  return invoke("get_state");
}

export function updateLyricsSettings(patch: Partial<Settings["lyrics"]>): Promise<void> {
  return invoke("update_lyrics_settings", { patch });
}

export function setLyricsEnabled(enabled: boolean): Promise<void> {
  return invoke("set_lyrics_enabled", { enabled });
}

export function setLyricsLocked(locked: boolean): Promise<void> {
  return invoke("set_lyrics_locked", { locked });
}

export function debugElapsed(): Promise<Record<string, unknown>> {
  return invoke("debug_elapsed");
}
