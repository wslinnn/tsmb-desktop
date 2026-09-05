import { useRef } from "react";
import { useStore } from "../../store";
import { updateLyricsSettings } from "../../commands";
import { outlineShadow } from "../../lyricsStyle";
import type { LyricsSettings } from "../../types";

const FONT_PRESETS = [
  "Microsoft YaHei",
  "微软雅黑",
  "SimHei",
  "SimSun",
  "KaiTi",
  "DengXian",
  "Segoe UI",
  "HarmonyOS Sans SC",
];

/** 设置面板：歌词样式全 P0 项 + 实时预览 + 偏移调节。
 *  乐观更新：本地先改 store，再 invoke（失败下次 settings-changed 会纠回）。 */
export default function SettingsPanel() {
  const settings = useStore((s) => s.settings);
  const setSettings = useStore((s) => s.setSettings);
  // 滑杆拖动时 onChange 每像素都触发：本地即时生效，后端按字段防抖落盘，
  // 否则一次拖动写几十次 settings.json（fontSize 还会连带几十次窗口 resize）
  const pending = useRef<Record<string, ReturnType<typeof setTimeout>>>({});
  if (!settings) return null;
  const ly = settings.lyrics;

  const patch = (p: Partial<LyricsSettings>) => {
    setSettings({ ...settings, lyrics: { ...ly, ...p } });
    const key = Object.keys(p)[0];
    clearTimeout(pending.current[key]);
    pending.current[key] = setTimeout(() => void updateLyricsSettings(p), 250);
  };

  return (
    <section className="panel">
      <h2>桌面歌词</h2>

      <div className="form-grid">
        <label className="span2">
          <input
            type="checkbox"
            checked={ly.enabled}
            onChange={(e) => patch({ enabled: e.target.checked })}
          />
          启用桌面歌词
        </label>
        <label>
          <input
            type="checkbox"
            checked={ly.locked}
            onChange={(e) => patch({ locked: e.target.checked })}
          />
          锁定（鼠标穿透）
        </label>

        <label>
          字体
          <select value={ly.fontFamily} onChange={(e) => patch({ fontFamily: e.target.value })}>
            {[...new Set([ly.fontFamily, ...FONT_PRESETS])].map((f) => (
              <option key={f} value={f}>
                {f}
              </option>
            ))}
          </select>
        </label>
        <label>
          字号 <em>{ly.fontSize}px</em>
          <input
            type="range"
            min={16}
            max={48}
            step={1}
            value={ly.fontSize}
            onChange={(e) => patch({ fontSize: Number(e.target.value) })}
          />
        </label>
        <label>
          <input
            type="checkbox"
            checked={ly.bold}
            onChange={(e) => patch({ bold: e.target.checked })}
          />
          加粗
        </label>
        <label>
          透明度 <em>{Math.round(ly.opacity * 100)}%</em>
          <input
            type="range"
            min={0.3}
            max={1}
            step={0.05}
            value={ly.opacity}
            onChange={(e) => patch({ opacity: Number(e.target.value) })}
          />
        </label>
        <label>
          描边宽度 <em>{ly.outlineWidth}px</em>
          <input
            type="range"
            min={0}
            max={4}
            step={0.5}
            value={ly.outlineWidth}
            onChange={(e) => patch({ outlineWidth: Number(e.target.value) })}
          />
        </label>
        <label>
          原文字色
          <input
            type="color"
            value={ly.baseColor}
            onChange={(e) => patch({ baseColor: e.target.value })}
          />
        </label>
        <label>
          高亮色
          <input
            type="color"
            value={ly.highlightColor}
            onChange={(e) => patch({ highlightColor: e.target.value })}
          />
        </label>
        <label>
          描边色
          <input
            type="color"
            value={ly.outlineColor}
            onChange={(e) => patch({ outlineColor: e.target.value })}
          />
        </label>
        <label>
          <input
            type="checkbox"
            checked={ly.showTranslation}
            onChange={(e) => patch({ showTranslation: e.target.checked })}
          />
          显示翻译行
        </label>
      </div>

      <div className="offset-row">
        <span>歌词偏移</span>
        <button onClick={() => patch({ offsetMs: ly.offsetMs - 500 })}>◀ 提前 0.5s</button>
        <em className="offset-value">
          {(ly.offsetMs / 1000).toFixed(1)}s
        </em>
        <button onClick={() => patch({ offsetMs: ly.offsetMs + 500 })}>延后 0.5s ▶</button>
        <button className="ghost" onClick={() => patch({ offsetMs: 0 })} disabled={ly.offsetMs === 0}>
          重置
        </button>
      </div>
      <p className="muted small">
        偏移只影响歌词显示，不改变播放进度（正值 = 歌词延后显示）。
      </p>

      <LyricsPreview ly={ly} />
    </section>
  );
}

/** 与歌词窗口同规则的静态预览：当前行（高亮色）+ 翻译/下一行（原色）。 */
function LyricsPreview({ ly }: { ly: LyricsSettings }) {
  const shadow = outlineShadow(ly.outlineColor, ly.outlineWidth);
  const common: React.CSSProperties = {
    fontFamily: `${ly.fontFamily}, sans-serif`,
    fontWeight: ly.bold ? 700 : 400,
    opacity: ly.opacity,
    textShadow: shadow,
  };
  return (
    <div className="lyrics-preview">
      <div style={{ ...common, fontSize: ly.fontSize, color: ly.highlightColor }}>
        预览：当前歌词行
      </div>
      <div style={{ ...common, fontSize: ly.fontSize * 0.72, color: ly.baseColor }}>
        {ly.showTranslation ? "translation line / 下一句歌词" : "下一句歌词"}
      </div>
    </div>
  );
}
