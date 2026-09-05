import { selectBot } from "../../commands";
import { useStore } from "../../store";
import { formatSec, type BotStatus } from "../../types";

function statusOf(b: BotStatus): { text: string; cls: string } {
  if (!b.connected) return { text: "离线", cls: "off" };
  if (b.paused) return { text: "暂停", cls: "paused" };
  if (b.playing) return { text: "播放中", cls: "playing" };
  return { text: "空闲", cls: "idle" };
}

export default function Bots() {
  const bots = useStore((s) => s.bots);
  const activeBotId = useStore((s) => s.activeBotId);

  if (bots.length === 0) {
    return (
      <section className="panel">
        <h2>机器人</h2>
        <p className="muted">暂无可见的机器人（等待连接或联系管理员开放权限）</p>
      </section>
    );
  }

  return (
    <section className="panel">
      <h2>机器人</h2>
      <div className="bot-list">
        {bots.map((b) => {
          const st = statusOf(b);
          const active = b.id === activeBotId;
          const dur = b.effectiveDuration ?? b.currentSong?.duration;
          return (
            <button
              key={b.id}
              className={`bot-card ${active ? "active" : ""}`}
              onClick={() => selectBot(b.id)}
            >
              <div className="bot-head">
                <span className="bot-name">{b.name || b.id}</span>
                <span className={`bot-status ${st.cls}`}>
                  <i />
                  {st.text}
                </span>
              </div>
              <div className="bot-song">
                {b.currentSong
                  ? `${b.currentSong.title} — ${b.currentSong.artist}`
                  : "未在播放"}
              </div>
              {b.currentSong && (
                <div className="bot-progress">
                  {formatSec(b.elapsed)} / {formatSec(dur ?? 0)}
                </div>
              )}
            </button>
          );
        })}
      </div>
    </section>
  );
}
