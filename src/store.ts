import { create } from "zustand";
import { subscribeAll } from "./events";
import { getState } from "./commands";
import type {
  AuthSnapshot,
  BotStatus,
  ConnSnapshot,
  LyricsDataEvent,
  Settings,
  TickEvent,
} from "./types";

interface AppStore {
  hydrated: boolean;
  auth: AuthSnapshot | null;
  connection: ConnSnapshot | null;
  bots: BotStatus[];
  activeBotId: string | null;
  settings: Settings | null;
  lyrics: LyricsDataEvent | null;
  tick: TickEvent | null;
  /** 歌词窗口 hover 态（仅解锁态有鼠标事件） */
  lyricsHovered: boolean;

  setAuth: (a: AuthSnapshot) => void;
  setConn: (c: ConnSnapshot) => void;
  setBots: (bots: BotStatus[]) => void;
  setActiveBot: (id: string | null) => void;
  setSettings: (s: Settings) => void;
  setLyrics: (l: LyricsDataEvent) => void;
  setTick: (t: TickEvent) => void;
  setLyricsHovered: (h: boolean) => void;
}

export const useStore = create<AppStore>((set) => ({
  hydrated: false,
  auth: null,
  connection: null,
  bots: [],
  activeBotId: null,
  settings: null,
  lyrics: null,
  tick: null,
  lyricsHovered: false,

  setAuth: (auth) => set({ auth }),
  setConn: (connection) => set({ connection }),
  setBots: (bots) => set({ bots }),
  setActiveBot: (activeBotId) => set({ activeBotId }),
  setSettings: (settings) => set({ settings }),
  setLyrics: (lyrics) => set({ lyrics }),
  setTick: (tick) => set({ tick }),
  setLyricsHovered: (lyricsHovered) => set({ lyricsHovered }),
}));

let subscribed = false;

/** 窗口挂载时调用一次：先订阅事件、再拉全量快照（乱序到达幂等，约定见
 *  docs/architecture.md「水合顺序」）。React StrictMode 双挂载下只订一次。 */
export async function hydrate(): Promise<void> {
  if (!subscribed) {
    subscribed = true;
    const s = useStore.getState();
    await subscribeAll({
      onAuth: s.setAuth,
      onConn: s.setConn,
      onBots: (e) => s.setBots(e.bots),
      onActiveBot: (e) => s.setActiveBot(e.botId),
      onSettings: (e) => s.setSettings(e.settings),
      onLyricsData: (l) => s.setLyrics(l),
      onTick: (t) => s.setTick(t),
    });
  }
  const snap = await getState();
  const s = useStore.getState();
  s.setAuth(snap.auth as AuthSnapshot);
  s.setConn(snap.connection as ConnSnapshot);
  s.setBots(snap.bots as BotStatus[]);
  s.setActiveBot(snap.activeBotId);
  s.setSettings(snap.settings);
  if (snap.lyrics) s.setLyrics(snap.lyrics);
  if (snap.tick) s.setTick(snap.tick as TickEvent);
  useStore.setState({ hydrated: true });
}
