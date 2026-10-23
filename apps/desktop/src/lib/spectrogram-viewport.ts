/**
 * 声谱图频率视口的坐标换算（纯函数）。
 *
 * ## 为什么要有这一层
 *
 * 声谱图的矩阵是 **512 个 bin**（Rust 侧 `SPECTROGRAM_BINS = 512`），
 * 覆盖 0…奈奎斯特。频率缩放要做两件事：
 *
 * 1. 把用户选的频率区间 `[lo, hi]` 换算成**要画哪几个 bin**（裁剪，不是拉伸）；
 * 2. 把 bin 下标换算回频率，好给坐标轴打刻度。
 *
 * 这两件事如果写在 Canvas 绘制代码里，会和渲染逻辑缠在一起，且无法单独
 * 验证 —— 画错了不报错，只是频率对不上。所以拆成纯函数。
 *
 * 这里**不做任何音频计算**：矩阵里的 dB 值是 Rust 算好的，
 * 本模块只决定「显示哪几行、刻度标多少 Hz」。
 */

/** 频率区间 `[lo, hi]`，单位 Hz。 */
export type HzRange = [number, number];

/** 可见的 bin 下标区间，闭区间。 */
export interface BinRange {
  first: number;
  last: number;
}

/**
 * 把频率区间钳到 `[0, nyquistHz]`，并判定它是否还是一个**有效的缩放**。
 *
 * 返回 `null` 表示「没有缩放」——它同时承担三个含义：
 * 视口未设置、区间非法（非有限、上下界颠倒）、区间窄到没有意义（< 1 Hz）。
 *
 * 把「非法」和「未设置」归成同一个返回值是有意的：调用方只需要知道
 * 「现在是不是全带宽视图」，不需要区分原因；真正的原因在界面上有
 * 别的出口（复位按钮）。
 */
export function clampViewRange(
  range: HzRange | null,
  nyquistHz: number,
): HzRange | null {
  if (range === null) return null;
  // 非有限或非正的奈奎斯特频率无法定义坐标轴：此时任何区间都不可信。
  // 不做这个检查的话 `Math.min(NaN, hi)` 会把 NaN 混进返回值，
  // 下游画出来的是一条空白带，而界面看不出异常。
  if (!Number.isFinite(nyquistHz) || nyquistHz <= 0) return null;

  const [lo, hi] = range;
  if (!Number.isFinite(lo) || !Number.isFinite(hi)) return null;
  if (lo >= hi) return null;
  // 窄于 1 Hz 的区间在 44.1 kHz / 512 bin 下连一个 bin 都不到，
  // 放大它没有意义，只会让画面变成一条色带。
  if (hi - lo < 1) return null;

  const clampedLo = Math.max(0, lo);
  const clampedHi = Math.min(nyquistHz, hi);
  if (clampedHi - clampedLo < 1) return null;

  return [clampedLo, clampedHi];
}

/**
 * 把频率区间换算成要绘制的 bin 下标区间。
 *
 * 缩放是**裁剪**：只画区间内的 bin，并把它们铺满绘图区。
 * 如果改成缩放整张图，放大后看到的仍是一张被拉长的图 —— 频率位置
 * 与 bin 的对应关系会被破坏，读数就是错的。
 *
 * `bins <= 0` 时返回 `{ first: 0, last: 0 }`：宁可画一个 bin，
 * 也不要返回 `last = -1` 那种会让下游循环下标越界的区间。
 */
export function binRangeForView(
  range: HzRange | null,
  nyquistHz: number,
  bins: number,
): BinRange {
  if (bins <= 0) return { first: 0, last: 0 };

  if (range === null) return { first: 0, last: bins - 1 };

  const binHz = nyquistHz / bins;
  if (!Number.isFinite(binHz) || binHz <= 0) {
    return { first: 0, last: bins - 1 };
  }

  const first = Math.min(
    bins - 1,
    Math.max(0, Math.floor(range[0] / binHz)),
  );
  // ceil 再减 1：让**包含** hiHz 的那个 bin 也画出来，
  // 否则区间右端正好落在 bin 边界时会少画一格。
  const last = Math.min(
    bins - 1,
    Math.max(first, Math.ceil(range[1] / binHz) - 1),
  );

  return { first, last };
}

/**
 * 生成频率刻度。
 *
 * 返回 `count + 1` 个等距频率，从低到高。调用方按「低频在下」的画法
 * 反序取用（声谱图纵轴是低频在下，频谱图纵轴是频率从下到上）。
 */
export function frequencyTicks(
  range: HzRange | null,
  nyquistHz: number,
  count: number,
): number[] {
  const lo = range === null ? 0 : range[0];
  const hi = range === null ? nyquistHz : range[1];
  // `count = 0` 时除法会产生 NaN —— 返回单个刻度（区间起点）而不是
  // 一个画不出坐标的坏值。负数/非整数同理向下取整。
  const steps = Number.isFinite(count) ? Math.floor(count) : 0;
  if (steps <= 0) return [lo];
  const ticks: number[] = [];
  for (let i = 0; i <= steps; i++) {
    ticks.push(lo + ((hi - lo) * i) / steps);
  }
  return ticks;
}
