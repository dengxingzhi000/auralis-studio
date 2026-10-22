/**
 * fitCanvas — 让画布后备存储匹配布局尺寸，**仅在尺寸真的变化时**重建。
 *
 * ## 为什么必须判断「尺寸是否变化」
 *
 * 给 `canvas.width` / `height` 赋值（即使赋的是同一个数值）都会让浏览器
 * 销毁现有后备存储并分配一张新的。在 Chromium 里这等于换掉一块合成表面，
 * DWM 必须重新规划该窗口的合成平面。
 *
 * 2026-10 的实测（2560×1600@240Hz、Intel iGPU、MPO+PSR2 开启）：
 * 窗口最大化的一次过渡里，SpectrumView 连续重建了 8 次后备存储，
 * 同一毫秒内出现 3 次掉帧（最大 39 ms，240 Hz 下≈丢 9 帧）。
 * 每帧都重建会把「一次 resize」放大成「一串合成表面替换」，
 * 表现为整屏抖动、任务栏区域闪烁。尺寸没变时必须什么都不写。
 *
 * ## 返回值
 *
 * `true` 表示后备存储被重建 —— 此时 2D 上下文状态（变换、样式）已被重置，
 * 调用方必须重新 `setTransform`。`false` 表示位图原样保留，调用方直接重绘即可。
 *
 * @param canvas     目标画布
 * @param cssWidth   布局宽度（CSS px）
 * @param cssHeight  布局高度（CSS px）
 * @param dpr        设备像素比，默认取 `window.devicePixelRatio`
 */
export function fitCanvas(
  canvas: HTMLCanvasElement,
  cssWidth: number,
  cssHeight: number,
  dpr: number = window.devicePixelRatio || 1,
): boolean {
  const pxWidth = Math.max(1, Math.floor(cssWidth * dpr));
  const pxHeight = Math.max(1, Math.floor(cssHeight * dpr));

  // 内联尺寸只在变化时写：避免每帧产生一次样式失效。
  const cssW = `${cssWidth}px`;
  const cssH = `${cssHeight}px`;
  if (canvas.style.width !== cssW) canvas.style.width = cssW;
  if (canvas.style.height !== cssH) canvas.style.height = cssH;

  if (canvas.width === pxWidth && canvas.height === pxHeight) return false;

  canvas.width = pxWidth;
  canvas.height = pxHeight;
  return true;
}
