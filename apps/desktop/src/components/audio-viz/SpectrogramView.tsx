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

import { useEffect, useRef } from "react";

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
}: SpectrogramViewProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
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

      if (hasMatrix) {
        // 把矩阵渲染到离屏 ImageData，再一次性 drawImage 缩放到绘图区。
        // 直接对主画布逐像素填充会按设备像素率放大 4 倍开销。
        const img = new ImageData(frames, bins);
        const lut = infernoLut();
        const span = Math.max(1e-6, s.maxDb - s.minDb);

        for (let t = 0; t < frames; t++) {
          for (let b = 0; b < bins; b++) {
            const v = matrix[t * bins + b]!;
            const u = Math.min(1, Math.max(0, (v - s.minDb) / span));
            const [r, g, bl] = lut[Math.round(u * 255)]!;
            // 低频在下：频率 bin 翻转成图像行
            const o = ((bins - 1 - b) * frames + t) * 4;
            img.data[o] = r;
            img.data[o + 1] = g;
            img.data[o + 2] = bl;
            img.data[o + 3] = 255;
          }
        }

        const off = document.createElement("canvas");
        off.width = frames;
        off.height = bins;
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

      // 频率刻度（低频在下）
      ctx.fillStyle = token("--axis-label");
      ctx.font = `10px ${monoFont}`;
      ctx.textAlign = "right";
      ctx.textBaseline = "middle";
      for (let i = 0; i <= 4; i++) {
        const hz = (s.nyquistHz * (4 - i)) / 4;
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
      if (s.cursorHz != null && s.cursorHz >= 0 && s.cursorHz <= s.nyquistHz) {
        const y = plotY + plotH * (1 - s.cursorHz / s.nyquistHz);
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
  ]);

  // ---- 视口交互：滚轮 / 拖动 / 双击 ----
  // 用原生监听器而不是 React onWheel：只有原生监听器能以 passive:false
  // 调用 preventDefault，否则滚轮会同时滚动页面。
  useEffect(() => {
    const wrap = wrapRef.current;
    if (!wrap || !onPan) return;

    let dragX: number | null = null;

    const span = () =>
      stateRef.current.durationSeconds > 0
        ? stateRef.current.durationSeconds
        : 0;

    const onWheel = (e: WheelEvent) => {
      const seconds = span();
      if (seconds <= 0) return;
      const delta = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
      if (delta === 0) return;
      e.preventDefault();
      // 一格滚轮 = 半屏；方向与内容移动方向一致（向下/向右 = 更晚）。
      onPan(Math.sign(delta) * seconds * 0.5);
    };

    const onPointerDown = (e: PointerEvent) => {
      if (e.button !== 0) return;
      dragX = e.clientX;
      wrap.setPointerCapture(e.pointerId);
      wrap.style.cursor = "grabbing";
    };

    const onPointerUp = (e: PointerEvent) => {
      wrap.style.cursor = "grab";
      if (dragX == null) return;
      const dx = e.clientX - dragX;
      dragX = null;
      const seconds = span();
      if (seconds <= 0 || Math.abs(dx) < 4) return;
      // 只在绘图区宽度上映射；左侧 50px 是频率刻度。
      const plotW = Math.max(1, wrap.clientWidth - 84);
      onPan((-dx / plotW) * seconds);
    };

    const onDoubleClick = () => {
      const start = stateRef.current.startSeconds;
      if (start > 0) onPan(-start);
    };

    wrap.style.cursor = "grab";
    wrap.addEventListener("wheel", onWheel, { passive: false });
    wrap.addEventListener("pointerdown", onPointerDown);
    wrap.addEventListener("pointerup", onPointerUp);
    wrap.addEventListener("pointercancel", onPointerUp);
    wrap.addEventListener("dblclick", onDoubleClick);
    return () => {
      wrap.removeEventListener("wheel", onWheel);
      wrap.removeEventListener("pointerdown", onPointerDown);
      wrap.removeEventListener("pointerup", onPointerUp);
      wrap.removeEventListener("pointercancel", onPointerUp);
      wrap.removeEventListener("dblclick", onDoubleClick);
      wrap.style.cursor = "";
    };
  }, [onPan]);

  return (
    <div ref={wrapRef} className="viz-canvas">
      <canvas ref={canvasRef} />
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
