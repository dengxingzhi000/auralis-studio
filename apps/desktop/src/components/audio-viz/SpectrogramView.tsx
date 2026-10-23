/**
 * SpectrogramView — 声谱图（Canvas 2D + ImageData）。
 *
 * ## 为什么用 Canvas 2D 而不是 WebGL
 *
 * ADR-018 初稿建议 WebGL。这里**有意偏离**，理由如下：
 *
 * 1. **数据量本就适中。** 视口内的矩阵被降采样到画布尺寸：
 *    1200×500 ≈ 60 万像素 ≈ 2.4 MB RGBA。一次 `putImageData` 完全够用，
 *    而 WebGL 的 `texImage2D` 在这个量级上未必更快。
 * 2. **可验证性。** 本项目没有图形会话，WebGL shader 编译失败只能靠
 *    人工发现；而 Canvas 2D 路径可以在无头环境下用像素断言验证。
 * 3. **真正的瓶颈不在光栅化。** Rust 侧 STFT + IPC 才是大头，
 *    用 GPU 换不回来。
 *
 * 若将来矩阵规模上到 10⁷ 像素量级（超长文件的全局声谱图），再换 WebGL，
 * 届时数据契约（`Uint8Array` 布局、行列方向、色阶）保持不变即可。
 *
 * ## 坐标约定
 *
 * - 横轴 = 时间，方向向右（与波形一致）
 * - 纵轴 = 频率，**低频在下**（专业音频工具的惯例，与频谱图读数一致）
 * - 颜色 = dB，由下方 `INFERNO_STOPS` 定义的 inferno 色阶
 *
 * ## 视觉约定（2026-10 样式修订）
 *
 * - 时间轴按实际时长标注（µs / ms / s），M1 版标的是 0%/6%/13%… 的百分比，
 *   读不出任何时间信息
 * - 右侧 8px 色阶条 + dB 刻度，说明「亮 = 多少 dB」
 */

import { useEffect, useRef, useState } from "react";

import {
  binRangeForView,
  clampViewRange,
  frequencyTicks,
} from "@/lib/spectrogram-viewport";

import { fitCanvas } from "./canvas-fit";

export interface SpectrogramViewProps {
  /** 行优先矩阵，`matrix[frame][bin]`，值域 dB */
  matrix: Float32Array | null;
  /** 矩阵宽度（时间格数） */
  width: number;
  /** 矩阵高度（频率 bin 数） */
  height: number;
  minDb: number;
  maxDb: number;
  /** 悬停频率游标 Hz */
  cursorHz?: number | null;
  /** 奈奎斯特频率 */
  nyquistHz: number;
  /**
   * 分析覆盖的总时长，秒。用于时间轴；0 或未提供时退化为「相对时间」刻度。
   * 由调用方按 Rust 给的 hop 与帧数换算，组件内不做 DSP 推算。
   */
  durationSeconds?: number;
  /**
   * 矩阵的绝对起点，秒。时间轴标的是 `startSeconds + 偏移`，
   * 否则平移到 3 分钟后画面仍从 0 开始标，读数会错。
   */
  startSeconds?: number;
  /**
   * 平移视口（秒）。滚轮、拖动、双击都会调用它；
   * 由调用方决定请求哪个窗口并对结果做防抖/取消。
   */
  onPan?: (deltaSeconds: number) => void;
  /**
   * 可见频率区间（Hz）；`null` = 全带宽。
   *
   * 与频谱图**共享**同一份状态：两个视图是同一份信号的两个投影，
   * 用户「盯着某一段频率看」时应当一起聚焦。
   */
  viewRange?: [number, number] | null;
  /**
   * 频率视口变化回调。
   *
   * 缩放是**裁剪**：只画区间内的 bin 并铺满绘图区，而不是拉伸整张图 ——
   * 后者会破坏「频率 ↔ bin」的对应关系，读数就是错的。
   */
  onViewRangeChange?: (r: [number, number]) => void;
}

const PAD = { left: 50, right: 34, top: 10, bottom: 24 };
/** 色阶条宽度 */
const BAR_W = 8;

export function SpectrogramView({
  matrix,
  width,
  height,
  minDb,
  maxDb,
  cursorHz,
  nyquistHz,
  durationSeconds = 0,
  startSeconds = 0,
  onPan,
  viewRange = null,
  onViewRangeChange,
}: SpectrogramViewProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  /** Shift 拖拽中正在框选的频段（CSS px，相对画布顶部）；null = 没有在拖 */
  const [dragBand, setDragBand] = useState<{
    top: number;
    height: number;
  } | null>(null);
  // 回调放进 ref：父组件每次渲染都会传入新的箭头函数，直接放进 effect
  // 依赖会让监听器反复重建 —— 拖拽进行到一半被换掉，手感很差。
  const onPanRef = useRef(onPan);
  const onViewRangeChangeRef = useRef(onViewRangeChange);
  useEffect(() => {
    onPanRef.current = onPan;
    onViewRangeChangeRef.current = onViewRangeChange;
  }, [onPan, onViewRangeChange]);
  // 色阶在渲染时从 CSS 变量解析，因此用 ref 传给 effect，
  // 避免把 Color 对象放进依赖数组导致每帧重建
  const stateRef = useRef({
    matrix,
    width,
    height,
    minDb,
    maxDb,
    cursorHz,
    nyquistHz,
    durationSeconds,
    startSeconds,
    viewRange,
  });
  stateRef.current = {
    matrix,
    width,
    height,
    minDb,
    maxDb,
    cursorHz,
    nyquistHz,
    durationSeconds,
    startSeconds,
    viewRange,
  };

  useEffect(() => {
    const canvas = canvasRef.current;
    const wrap = wrapRef.current;
    if (!canvas || !wrap) return;

    const draw = () => {
      const dpr = window.devicePixelRatio || 1;
      const cw = wrap.clientWidth;
      const ch = wrap.clientHeight;
      if (cw === 0 || ch === 0) return;

      // 尺寸没变时不重建后备存储（见 canvas-fit.ts）。
      fitCanvas(canvas, cw, ch, dpr);

      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, cw, ch);

      const css = getComputedStyle(document.documentElement);
      const token = (n: string) => css.getPropertyValue(n).trim();
      const monoFont = token("--font-mono");
      const uiFont = token("--font-ui");

      ctx.fillStyle = token("--bg-panel");
      ctx.fillRect(0, 0, cw, ch);

      const plotX = PAD.left;
      const plotY = PAD.top;
      const plotW = cw - PAD.left - PAD.right;
      const plotH = ch - PAD.top - PAD.bottom;
      if (plotW <= 0 || plotH <= 0) return;

      const s = stateRef.current;
      // 数据契约（与 auralis-dsp::Stft::to_matrix 一致）：
      //   matrix[row * bins + col]，row = 时间帧（0 在最左），col = 频率 bin（0 为 DC）
      // 因此步长是**频率格数**，不是时间格数。M1 版按「[频率][时间] 且步长 = width」
      // 读取，把 1024×512 的矩阵读成了错位数据 —— 屏幕上看到的是一列列竖条纹，
      // 而不是 1 kHz 处的一条水平亮线。
      const bins = s.height;
      const frames = s.width;
      const matrix = s.matrix;
      const hasMatrix =
        matrix != null && frames > 0 && bins > 0 && matrix.length >= frames * bins;

      // 频率视口。钳位在组件内部做：视口是**全局共享**的，可能来自
      // 另一个采样率的文件（例如 48 kHz 的文件缩放到 20 kHz，再打开
      // 44.1 kHz 的文件），不钳位就会裁出一片空白。
      const view = clampViewRange(s.viewRange, s.nyquistHz);
      const binView = binRangeForView(view, s.nyquistHz, bins);
      const firstBin = binView.first;
      const lastBin = binView.last;
      const visBins = lastBin - firstBin + 1;

      if (hasMatrix) {
        // 把矩阵渲染到离屏 ImageData，再一次性 drawImage 缩放到绘图区。
        // 直接对主画布逐像素填充会按设备像素率放大 4 倍开销。
        //
        // 缩放是**裁剪**：只画可见 bin 区间，并把它铺满绘图区。
        // 拉伸整张图会破坏「频率 ↔ bin」的对应关系 —— 放大后看到的
        // 仍然是一张被拉长的图，读数是错的。
        const img = new ImageData(frames, visBins);
        const lut = infernoLut();
        const span = Math.max(1e-6, s.maxDb - s.minDb);

        for (let t = 0; t < frames; t++) {
          for (let vb = 0; vb < visBins; vb++) {
            const b = firstBin + vb;
            const v = matrix[t * bins + b]!;
            const u = Math.min(1, Math.max(0, (v - s.minDb) / span));
            const [r, g, bl] = lut[Math.round(u * 255)]!;
            // 低频在下：可见区间内最低的 bin 放在最下面一行
            const o = ((visBins - 1 - vb) * frames + t) * 4;
            img.data[o] = r;
            img.data[o + 1] = g;
            img.data[o + 2] = bl;
            img.data[o + 3] = 255;
          }
        }

        const off = document.createElement("canvas");
        off.width = frames;
        off.height = visBins;
        off.getContext("2d")?.putImageData(img, 0, 0);
        ctx.imageSmoothingEnabled = true;
        ctx.drawImage(off, plotX, plotY, plotW, plotH);

        // ---- 色阶条：说明「亮 = 多少 dB」，否则颜色没有可解释性 ----
        const barX = plotX + plotW + 10;
        const barGrad = ctx.createLinearGradient(0, plotY, 0, plotY + plotH);
        for (let i = 0; i <= 8; i++) {
          const t = 1 - i / 8;
          const [r, g, b] = lut[Math.round(t * 255)]!;
          barGrad.addColorStop(i / 8, `rgb(${r} ${g} ${b})`);
        }
        ctx.fillStyle = barGrad;
        ctx.fillRect(barX, plotY, BAR_W, plotH);
        ctx.strokeStyle = token("--border-subtle");
        ctx.strokeRect(barX + 0.5, plotY + 0.5, BAR_W - 1, plotH - 1);

        ctx.fillStyle = token("--axis-label");
        ctx.font = `10px ${monoFont}`;
        ctx.textAlign = "left";
        ctx.textBaseline = "middle";
        for (let i = 0; i <= 2; i++) {
          const y = plotY + (plotH * i) / 2;
          const v = s.maxDb - ((s.maxDb - s.minDb) * i) / 2;
          ctx.fillText(v.toFixed(0), barX + BAR_W + 3, y);
        }
      } else {
        ctx.textAlign = "center";
        ctx.textBaseline = "alphabetic";
        ctx.font = `13px ${uiFont}`;
        ctx.fillStyle = token("--text-secondary");
        ctx.fillText("尚无声谱图数据", plotX + plotW / 2, plotY + plotH / 2);
        ctx.font = `11px ${uiFont}`;
        ctx.fillStyle = token("--text-disabled");
        ctx.fillText(
          "声谱图来自真实文件的 STFT（M3），合成信号没有时域数据",
          plotX + plotW / 2,
          plotY + plotH / 2 + 18,
        );
      }

      // ---- 坐标轴 ----
      ctx.strokeStyle = token("--axis-line");
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(plotX + 0.5, plotY);
      ctx.lineTo(plotX + 0.5, plotY + plotH + 0.5);
      ctx.lineTo(plotX + plotW, plotY + plotH + 0.5);
      ctx.stroke();

      // 频率刻度（低频在下）。缩放后必须**跟着变** —— 刻度还标全带宽
      // 的话，画面和坐标轴对不上，读数就是错的。
      const ticks = frequencyTicks(view, s.nyquistHz, 4);
      ctx.fillStyle = token("--axis-label");
      ctx.font = `10px ${monoFont}`;
      ctx.textAlign = "right";
      ctx.textBaseline = "middle";
      for (let i = 0; i <= 4; i++) {
        // 低频在下：i = 0 是绘图区顶边，对应最高频率
        const hz = ticks[4 - i]!;
        const y = plotY + (plotH * i) / 4;
        ctx.fillText(fmtHz(hz), plotX - 7, y);
      }

      // 时间刻度：只有真的有声谱图数据时才标时间。
      // 无数据时标一串时间码会让人以为那块区域有内容。
      if (s.matrix && s.width > 0 && s.height > 0) {
        ctx.fillStyle = token("--axis-label");
        ctx.font = `10px ${monoFont}`;
        ctx.textAlign = "center";
        ctx.textBaseline = "top";
        for (let i = 0; i <= 4; i++) {
          const x = plotX + (plotW * i) / 4;
          const label =
            s.durationSeconds > 0
              ? formatTime(s.startSeconds + (s.durationSeconds * i) / 4)
              : `${((i * 25) / 4).toFixed(0)}%`;
          ctx.fillText(label, x, plotY + plotH + 6);
        }
      }

      // ---- 频率游标 ----
      // 游标位置按**可见区间**换算；游标不在可见区间内时不画 ——
      // 在别的频段画一条线只会误导。
      const viewLo = view ? view[0] : 0;
      const viewHi = view ? view[1] : s.nyquistHz;
      if (
        s.cursorHz != null &&
        s.cursorHz >= viewLo &&
        s.cursorHz <= viewHi &&
        viewHi > viewLo
      ) {
        const y = plotY + plotH * (1 - (s.cursorHz - viewLo) / (viewHi - viewLo));
        ctx.strokeStyle = token("--cursor");
        ctx.globalAlpha = 0.55;
        ctx.beginPath();
        ctx.moveTo(plotX, y);
        ctx.lineTo(plotX + plotW, y);
        ctx.stroke();
        ctx.globalAlpha = 1;

        // 游标读数：贴在左侧刻度上，避免遮挡频谱内容
        const text = `${fmtHz(s.cursorHz)} Hz`;
        ctx.font = `10px ${monoFont}`;
        ctx.textAlign = "left";
        ctx.textBaseline = "middle";
        const tw = ctx.measureText(text).width + 8;
        ctx.fillStyle = token("--bg-elevated");
        ctx.strokeStyle = token("--border-subtle");
        ctx.beginPath();
        ctx.roundRect(plotX + 4, y - 8, tw, 16, 3);
        ctx.fill();
        ctx.stroke();
        ctx.fillStyle = token("--text-primary");
        ctx.fillText(text, plotX + 8, y);
      }
    };

    draw();
    const ro = new ResizeObserver(draw);
    ro.observe(wrap);
    return () => ro.disconnect();
  }, [
    matrix,
    width,
    height,
    minDb,
    maxDb,
    cursorHz,
    nyquistHz,
    durationSeconds,
    startSeconds,
    viewRange,
  ]);

  // ---- 视口交互：滚轮 / 拖动 / 双击 ----
  // 用原生监听器而不是 React onWheel：只有原生监听器能以 passive:false
  // 调用 preventDefault，否则滚轮会同时滚动页面。
  //
  // 两个轴各占一套操作，互不抢：
  //   - 时间轴：滚轮平移、拖拽平移、双击回到文件开头
  //   - 频率轴：Shift + 滚轮缩放、Shift + 拖拽框选
  // `Shift` 是唯一不与既有操作冲突的修饰键。
  useEffect(() => {
    const wrap = wrapRef.current;
    if (!wrap) return;

    /** 当前拖拽：记录轴与起点。 */
    let drag: { mode: "time" | "freq"; x: number; y: number } | null = null;

    const span = () =>
      stateRef.current.durationSeconds > 0
        ? stateRef.current.durationSeconds
        : 0;

    /** 屏幕 y -> 频率 Hz（低频在下，故底边是 lo）。 */
    const yToHz = (clientY: number): number => {
      const s = stateRef.current;
      const r = wrap.getBoundingClientRect();
      const plotY = r.top + PAD.top;
      const plotH = r.height - PAD.top - PAD.bottom;
      const view = clampViewRange(s.viewRange, s.nyquistHz);
      const lo = view ? view[0] : 0;
      const hi = view ? view[1] : s.nyquistHz;
      if (plotH <= 0 || hi <= lo) return lo;
      const t = Math.min(1, Math.max(0, (clientY - plotY) / plotH));
      return lo + (1 - t) * (hi - lo);
    };

    const onWheel = (e: WheelEvent) => {
      const delta = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
      if (delta === 0) return;
      e.preventDefault();

      if (e.shiftKey) {
        // 频率缩放：以光标所在频率为中心。
        const s = stateRef.current;
        const nyq = s.nyquistHz;
        const view = clampViewRange(s.viewRange, nyq);
        const [lo0, hi0] = view ?? [0, nyq];
        const focus = yToHz(e.clientY);
        const factor = delta > 0 ? 1.25 : 0.8;
        const next = clampViewRange(
          [focus - (focus - lo0) * factor, focus + (hi0 - focus) * factor],
          nyq,
        );
        // 钳位后无效（区间非法或窄到没有意义）时**不**回调：
        // 回传 null 会被调用方当成「复位」，把用户的缩放清掉。
        if (next) {
          // 立刻把新区间写回本地：连续滚轮时 React 还没来得及重渲染，
          // 下一次事件若仍读 prop 就会用同一个旧区间计算 —— 连滚四格
          // 只生效一格。父组件的 state 会在下一帧用同一个值覆盖它。
          stateRef.current.viewRange = next;
          onViewRangeChangeRef.current?.(next);
        }
        return;
      }

      const seconds = span();
      if (seconds <= 0) return;
      // 一格滚轮 = 半屏；方向与内容移动方向一致（向下/向右 = 更晚）。
      onPanRef.current?.(Math.sign(delta) * seconds * 0.5);
    };

    const onPointerDown = (e: PointerEvent) => {
      if (e.button !== 0) return;
      drag = {
        mode: e.shiftKey ? "freq" : "time",
        x: e.clientX,
        y: e.clientY,
      };
      setDragBand(null);
      wrap.setPointerCapture(e.pointerId);
      wrap.style.cursor = "grabbing";
    };

    /** Shift 拖拽时把正在框选的频段画成半透明带，否则用户看不到选区。 */
    const onPointerMove = (e: PointerEvent) => {
      if (!drag || drag.mode !== "freq") return;
      const r = wrap.getBoundingClientRect();
      const topEdge = PAD.top;
      const bottomEdge = r.height - PAD.bottom;
      const clampY = (clientY: number) =>
        Math.min(bottomEdge, Math.max(topEdge, clientY - r.top));
      const y0 = clampY(drag.y);
      const y1 = clampY(e.clientY);
      setDragBand({ top: Math.min(y0, y1), height: Math.abs(y1 - y0) });
    };

    const onPointerUp = (e: PointerEvent) => {
      wrap.style.cursor = "grab";
      setDragBand(null);
      const d = drag;
      drag = null;
      if (!d) return;

      if (d.mode === "freq") {
        const a = yToHz(d.y);
        const b = yToHz(e.clientY);
        const lo = Math.min(a, b);
        const hi = Math.max(a, b);
        if (hi - lo < 1) return;
        const next = clampViewRange([lo, hi], stateRef.current.nyquistHz);
        if (next) onViewRangeChangeRef.current?.(next);
        return;
      }

      const dx = e.clientX - d.x;
      const seconds = span();
      if (seconds <= 0 || Math.abs(dx) < 4) return;
      // 只在绘图区宽度上映射；左侧 50px 是频率刻度。
      const plotW = Math.max(1, wrap.clientWidth - 84);
      onPanRef.current?.((-dx / plotW) * seconds);
    };

    const onDoubleClick = () => {
      const start = stateRef.current.startSeconds;
      if (start > 0) onPanRef.current?.(-start);
    };

    wrap.style.cursor = "grab";
    wrap.addEventListener("wheel", onWheel, { passive: false });
    wrap.addEventListener("pointerdown", onPointerDown);
    wrap.addEventListener("pointermove", onPointerMove);
    wrap.addEventListener("pointerup", onPointerUp);
    wrap.addEventListener("pointercancel", onPointerUp);
    wrap.addEventListener("dblclick", onDoubleClick);
    return () => {
      wrap.removeEventListener("wheel", onWheel);
      wrap.removeEventListener("pointerdown", onPointerDown);
      wrap.removeEventListener("pointermove", onPointerMove);
      wrap.removeEventListener("pointerup", onPointerUp);
      wrap.removeEventListener("pointercancel", onPointerUp);
      wrap.removeEventListener("dblclick", onDoubleClick);
      wrap.style.cursor = "";
    };
  }, []);

  return (
    <div ref={wrapRef} className="viz-canvas">
      <canvas ref={canvasRef} />
      {dragBand && (
        <div
          className="viz-drag-band"
          style={{
            left: PAD.left,
            right: PAD.right,
            top: dragBand.top,
            height: dragBand.height,
          }}
        />
      )}
    </div>
  );
}

function fmtHz(hz: number): string {
  if (hz >= 1000) return `${(hz / 1000).toFixed(1)}k`;
  return hz.toFixed(0);
}

/** 与 WaveformView 同一套时间格式：按量级选单位，避免满屏 "0:00.000"。 */
function formatTime(sec: number): string {
  if (sec === 0) return "0";
  const abs = Math.abs(sec);
  if (abs < 0.001) return `${(sec * 1e6).toFixed(0)} µs`;
  if (abs < 1) return `${(sec * 1e3).toFixed(abs < 0.01 ? 1 : 0)} ms`;
  if (abs < 60) return `${sec.toFixed(abs < 10 ? 2 : 1)} s`;
  const m = Math.floor(sec / 60);
  const s = sec - m * 60;
  return `${m}:${s.toFixed(1).padStart(4, "0")}`;
}

/**
 * inferno 色阶的 RGB 查表。
 *
 * 与 Rust 侧 `auralis-ui` 早期版本同源（256 级）。
 * 这里用同样的 17 个控制点插值，保证两端口径一致。
 */
let LUT_CACHE: [number, number, number][] | null = null;

function infernoLut(): [number, number, number][] {
  if (LUT_CACHE) return LUT_CACHE;
  const STOPS: [number, number, number][] = [
    [0.001, 0.0, 0.014],
    [0.044, 0.033, 0.141],
    [0.142, 0.058, 0.276],
    [0.258, 0.039, 0.406],
    [0.367, 0.075, 0.432],
    [0.472, 0.11, 0.428],
    [0.578, 0.148, 0.404],
    [0.682, 0.195, 0.359],
    [0.78, 0.254, 0.3],
    [0.865, 0.332, 0.229],
    [0.929, 0.437, 0.142],
    [0.968, 0.553, 0.08],
    [0.988, 0.673, 0.056],
    [0.988, 0.789, 0.146],
    [0.976, 0.878, 0.31],
    [0.949, 0.96, 0.585],
    [0.988, 0.998, 0.645],
  ];
  const lut: [number, number, number][] = [];
  const n = STOPS.length;
  for (let i = 0; i < 256; i++) {
    const t = (i / 255) * (n - 1);
    const lo = Math.floor(t);
    const hi = Math.min(lo + 1, n - 1);
    const f = t - lo;
    const a = STOPS[lo]!;
    const b = STOPS[hi]!;
    lut.push([
      Math.round((a[0] + (b[0] - a[0]) * f) * 255),
      Math.round((a[1] + (b[1] - a[1]) * f) * 255),
      Math.round((a[2] + (b[2] - a[2]) * f) * 255),
    ]);
  }
  LUT_CACHE = lut;
  return lut;
}
