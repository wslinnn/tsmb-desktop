import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ActiveBotEvent,
  AuthSnapshot,
  BotsUpdatedEvent,
  ConnSnapshot,
  LyricsDataEvent,
  SettingsChangedEvent,
  TickEvent,
} from "./types";

/** 订阅全部 Rust→前端事件，返回清理函数。窗口挂载时先调本函数再 get_state
 *  （事件全量幂等，乱序到达不丢状态）。 */
export function subscribeAll(handlers: {
  onAuth: (snap: AuthSnapshot) => void;
  onConn: (snap: ConnSnapshot) => void;
  onBots: (e: BotsUpdatedEvent) => void;
  onActiveBot: (e: ActiveBotEvent) => void;
  onSettings: (e: SettingsChangedEvent) => void;
  onLyricsData: (e: LyricsDataEvent) => void;
  onTick: (e: TickEvent) => void;
}): Promise<UnlistenFn> {
  const subs: Promise<UnlistenFn>[] = [
    listen<AuthSnapshot>("auth-state", (e) => handlers.onAuth(e.payload)),
    listen<ConnSnapshot>("connection-state", (e) => handlers.onConn(e.payload)),
    listen<BotsUpdatedEvent>("bots-updated", (e) => handlers.onBots(e.payload)),
    listen<ActiveBotEvent>("active-bot", (e) => handlers.onActiveBot(e.payload)),
    listen<SettingsChangedEvent>("settings-changed", (e) => handlers.onSettings(e.payload)),
    listen<LyricsDataEvent>("lyrics-data", (e) => handlers.onLyricsData(e.payload)),
    listen<TickEvent>("lyrics-tick", (e) => handlers.onTick(e.payload)),
  ];
  return Promise.all(subs).then((fns) => () => fns.forEach((fn) => fn()));
}
