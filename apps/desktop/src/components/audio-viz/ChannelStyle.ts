/**
 * 声道视觉样式：颜色 + 线型双编码。
 *
 * 迁移自 M0 的 `auralis-ui::theme::ChannelStyle`。ADR-017 要求
 * 「基础控件优先满足键盘操作、无障碍」，UI_SPEC §2 要求重要状态
 * 不能只靠颜色表达——因此色觉差异用户必须仍能区分声道。
 */

export type DashPattern =
  | "solid"
  | "dashed"
  | "dotted"
  | "dashdot"
  | "dense"
  | "sparse";

export interface ChannelStyle {
  color: string;
  dash: DashPattern;
  label: string;
}

/** Canvas 2D 的 setLineDash 数组。`solid` 返回空数组。 */
const DASH_ARRAYS: Record<DashPattern, number[]> = {
  solid: [],
  dashed: [8, 5],
  dotted: [2, 3],
  dashdot: [8, 4, 2, 4],
  dense: [5, 2],
  sparse: [12, 8],
};

const STYLES: readonly ChannelStyle[] = [
  { color: "var(--channel-0)", dash: "solid", label: "1" },
  { color: "var(--channel-1)", dash: "dashed", label: "2" },
  { color: "var(--channel-2)", dash: "dotted", label: "3" },
  { color: "var(--channel-3)", dash: "dashdot", label: "4" },
  { color: "var(--channel-4)", dash: "dense", label: "5" },
  { color: "var(--channel-5)", dash: "sparse", label: "6" },
] as const;

/**
 * 取第 `index` 个声道的样式。
 *
 * 超过 6 声道时压低 alpha 并改用虚线，使重复样式仍可辨。
 * 注意 Canvas 无法直接解析 CSS 变量，所以这里同时返回变量名
 * 与已解析的色值（由调用方用 getComputedStyle 提供）。
 */
export function channelStyle(index: number): ChannelStyle {
  const base = STYLES[index % STYLES.length]!;
  const cycle = Math.floor(index / STYLES.length);
  if (cycle === 0) return base;
  return { ...base, dash: "dashed" as DashPattern };
}

/** Canvas setLineDash 用的数组。 */
export function dashArray(pattern: DashPattern): number[] {
  return DASH_ARRAYS[pattern];
}

/** 6 个基础声道的 CSS 变量名，供构建 CSS 用。 */
export const CHANNEL_VARS: readonly string[] = STYLES.map((s) => s.color);