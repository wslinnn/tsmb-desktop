import type { CSSProperties } from "react";
import type { LyricsSettings } from "./types";

/** 多向 text-shadow 模拟描边（设置页预览与歌词窗口共用同一实现）。 */
export function outlineShadow(color: string, width: number): string {
  if (width <= 0) return "none";
  const dirs = [
    [0, -1], [0, 1], [-1, 0], [1, 0],
    [-0.7, -0.7], [0.7, -0.7], [-0.7, 0.7], [0.7, 0.7],
  ];
  return dirs
    .map(([x, y]) => `${(x * width).toFixed(1)}px ${(y * width).toFixed(1)}px 1px ${color}`)
    .join(", ");
}

/** 当前行（高亮色）与次行（原色）的公共样式。 */
export function lineStyles(ly: LyricsSettings): { main: CSSProperties; sub: CSSProperties } {
  const shadow = outlineShadow(ly.outlineColor, ly.outlineWidth);
  return {
    main: {
      fontFamily: `${ly.fontFamily}, sans-serif`,
      fontWeight: ly.bold ? 700 : 400,
      fontSize: ly.fontSize,
      color: ly.highlightColor,
      textShadow: shadow,
    },
    sub: {
      fontFamily: `${ly.fontFamily}, sans-serif`,
      fontWeight: ly.bold ? 700 : 400,
      fontSize: ly.fontSize * 0.72,
      color: ly.baseColor,
      textShadow: shadow,
    },
  };
}
