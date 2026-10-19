/**
 * SpectrumView — FFT 频谱曲线（Canvas 2D）。
 *
 * 数据由 Rust 算好后传入：`db[]` 已经是**按画布宽度降采样**的 dB 值
 * （ADR-018 规则 3）。前端只负责把点画出来，**不做任何数值计算**
 * —— 不含 dB 换算、不含 FFT、不含重采样。
 *
 * ## 为什么用 Canvas 而不是 SVG/DOM
 *
 * 4096 个点若建成 SVG 节点，每帧的 DOM 操作会直接拖垮主线程。
 * Canvas 是一次批量 path 描边。
 *
 * ## 缩放 / 平移
 *
 * 拖拽选区 = 设置新的频率视口；双击 = 复位到全带宽。
 * 平移通过鼠标中键或按住 Shift 拖拽完成。
 *
 * ## 视觉约定（2026-10 样式修订）
 *
 * 所有颜色从 CSS 令牌读取，画布里**不写死色值**。曲线下方的渐变填充、
 * 网格与坐标轴的三级明度差、零线强调，都与 tokens.css 的
 * --grid-minor / --grid-major / --axis-line / --trace-fill-* 对应。
 */

import { useCallback, useEffect, useRef } from "react";

export interface PeakInfo {
  frequencyHz: number;
  amplitudeDbfs: number;
  /** true when the reading is unreliable (large scalloping loss) */
  estimated: boolean;
}

export interface SpectrumViewProps {
  /** dB values, uniform bins; -Infinity means silence */
  db: Float32Array | null;
  /** frequency step, Hz */
  stepHz: number;
  /** sample rate, for the Nyquist limit */
  sampleRate: number;
  minDb: number;
  maxDb: number;
  /** hover frequency cursor, Hz */
  cursorHz: number | null;
  /** detected peaks to mark on the curve */
  peaks?: PeakInfo[];
  /** visible frequency range, Hz. Omit for full band. */
  viewRange?: [number, number] | null;
  /** called with a new range after a drag-select */
  onViewRangeChange?: (r: [number, number]) => void;
  /** called on hover with the frequency under the cursor, or null on leave */
  onHoverHz?: (hz: number | null) => void;
}

const PAD = { left: 46, right: 12, top: 10, bottom: 24 };

/** 每格 dB 数与分格数：140 dB / 8 ≈ 17.5 dB 一格，与 M0 版一致 */
const DB_STEPS = 8;
const HZ_STEPS = 8;

export function SpectrumView({
  db,
  stepHz,
  sampleRate,
  minDb,
  maxDb,
  cursorHz,
  peaks = [],
  viewRange,
  onViewRangeChange,
  onHoverHz,
}: SpectrumViewProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ x0: number; x1: number } | null>(null);

  const stateRef = useRef({ db, stepHz, sampleRate, minDb, maxDb, cursorHz, peaks, viewRange });
  stateRef.current = { db, stepHz, sampleRate, minDb, maxDb, cursorHz, peaks, viewRange };

  /** 像素 x -> 频率 Hz（需要绘图区几何，故从 DOM 读） */
  const xToHz = useCallback((clientX: number, el: HTMLElement) => {
    const s = stateRef.current;
    const r = el.getBoundingClientRect();
    const plotX = r.left + PAD.left;
    const plotW = r.width - PAD.left - PAD.right;
    const nyq = s.sampleRate / 2;
    const [lo, hi] = s.viewRange ?? [0, nyq];
    const t = Math.min(1, Math.max(0, (clientX - plotX) / Math.max(1, plotW)));
    return lo + t * (hi - lo);
  }, []);

  const onWheel = useCallback(
    (e: React.WheelEvent<HTMLDivElement>) => {
      const el = e.currentTarget;
      const s = stateRef.current;
      const nyq = s.sampleRate / 2;
      const [lo0, hi0] = s.viewRange ?? [0, nyq];
      const focus = xToHz(e.clientX, el);
      // ctrl+wheel 或普通 wheel 都做缩放：以光标位置为中心
      const factor = e.deltaY > 0 ? 1.25 : 0.8;
      let lo = focus - (focus - lo0) * factor;
      let hi = focus + (hi0 - focus) * factor;
      if (lo < 0) lo = 0;
      if (hi > nyq) hi = nyq;
      if (hi - lo < 1) return;
      onViewRangeChange?.([lo, hi]);
    },
    [xToHz, onViewRangeChange],
  );

  useEffect(() => {
    const canvas = canvasRef.current;
    const wrap = wrapRef.current;
    if (!canvas || !wrap) return;

    const draw = () => {
      const dpr = window.devicePixelRatio || 1;
      const w = wrap.clientWidth;
      const h = wrap.clientHeight;
      if (w === 0 || h === 0) return;

      canvas.width = Math.floor(w * dpr);
      canvas.height = Math.floor(h * dpr);
      canvas.style.width = `${w}px`;
      canvas.style.height = `${h}px`;

      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, h);

      const css = getComputedStyle(document.documentElement);
      const token = (n: string) => css.getPropertyValue(n).trim();
      const monoFont = token("--font-mono");
      const uiFont = token("--font-ui");

      const plotX = PAD.left;
      const plotY = PAD.top;
      const plotW = w - PAD.left - PAD.right;
      const plotH = h - PAD.top - PAD.bottom;
      if (plotW <= 0 || plotH <= 0) return;

      const s = stateRef.current;
      const nyq = s.sampleRate / 2;
      const [loHz, hiHz] = s.viewRange ?? [0, nyq];
      const spanHz = Math.max(1e-6, hiHz - loHz);
      const hzToX = (hz: number) => plotX + ((hz - loHz) / spanHz) * plotW;
      const dbToY = (db: number) => {
        const span = Math.max(1e-6, s.maxDb - s.minDb);
        const c = Math.min(s.maxDb, Math.max(s.minDb, db));
        return plotY + (1 - (c - s.minDb) / span) * plotH;
      };

      // 画布底：与卡片表面同色，用令牌而不是硬编码
      ctx.fillStyle = token("--bg-panel");
      ctx.fillRect(0, 0, w, h);

      // ---- 水平网格 + dB 刻度 ----
      // 网格三级明度：整数格用 major，半格用 minor，坐标轴单独一条
      ctx.lineWidth = 1;
      ctx.font = `10px ${monoFont}`;
      ctx.textAlign = "right";
      ctx.textBaseline = "middle";
      for (let i = 0; i <= DB_STEPS; i++) {
        const y = Math.round(plotY + (plotH * i) / DB_STEPS) + 0.5;
        ctx.strokeStyle = token("--grid-major");
        ctx.beginPath();
        ctx.moveTo(plotX, y);
        ctx.lineTo(plotX + plotW, y);
        ctx.stroke();

        const v = s.maxDb - ((s.maxDb - s.minDb) * i) / DB_STEPS;
        ctx.fillStyle = token("--axis-label");
        ctx.fillText(v.toFixed(0), plotX - 7, y);
      }

      // ---- 垂直网格 + 频率刻度 ----
      for (let i = 0; i <= HZ_STEPS; i++) {
        const hz = loHz + (spanHz * i) / HZ_STEPS;
        const x = Math.round(hzToX(hz)) + 0.5;
        ctx.strokeStyle = token("--grid-minor");
        ctx.beginPath();
        ctx.moveTo(x, plotY);
        ctx.lineTo(x, plotY + plotH);
        ctx.stroke();

        ctx.fillStyle = token("--axis-label");
        ctx.textAlign = "center";
        ctx.textBaseline = "top";
        ctx.fillText(formatHz(hz), x, plotY + plotH + 6);
      }

      // ---- 坐标轴：左 + 下，1px 高对比 ----
      ctx.strokeStyle = token("--axis-line");
      ctx.beginPath();
      ctx.moveTo(plotX + 0.5, plotY);
      ctx.lineTo(plotX + 0.5, plotY + plotH + 0.5);
      ctx.lineTo(plotX + plotW, plotY + plotH + 0.5);
      ctx.stroke();

      // ---- 曲线 ----
      if (s.db && s.db.length > 1) {
        // 可见区间内的点，先收集再画：填充、描边、峰标记共用同一组坐标
        const pts: { x: number; y: number }[] = [];
        for (let i = 0; i < s.db.length; i++) {
          const hz = i * s.stepHz;
          if (hz < loHz || hz > hiHz) continue;
          const v = s.db[i]!;
          pts.push({
            x: hzToX(hz),
            y: Number.isFinite(v) ? dbToY(v) : dbToY(s.minDb),
          });
        }

        if (pts.length > 1) {
          const first = pts[0]!;
          const last = pts[pts.length - 1]!;

          // 曲线下方渐变填充：给曲线体积感，但不喧宾夺主
          const grad = ctx.createLinearGradient(0, plotY, 0, plotY + plotH);
          grad.addColorStop(0, token("--trace-fill-top"));
          grad.addColorStop(1, token("--trace-fill-bottom"));
          ctx.beginPath();
          ctx.moveTo(first.x, plotY + plotH);
          for (const p of pts) ctx.lineTo(p.x, p.y);
          ctx.lineTo(last.x, plotY + plotH);
          ctx.closePath();
          ctx.fillStyle = grad;
          ctx.fill();

          // 曲线本体：外发光一层 + 实线一层，1.4px 在深色底上偏细
          ctx.lineJoin = "round";
          ctx.beginPath();
          ctx.moveTo(first.x, first.y);
          for (let i = 1; i < pts.length; i++) ctx.lineTo(pts[i]!.x, pts[i]!.y);

          ctx.strokeStyle = token("--accent-primary");
          ctx.globalAlpha = 0.22;
          ctx.lineWidth = 3;
          ctx.stroke();
          ctx.globalAlpha = 1;
          ctx.lineWidth = 1.5;
          ctx.stroke();
        }

        // ---- 峰值标记 ----
        ctx.font = `10px ${monoFont}`;
        for (const p of s.peaks) {
          if (p.frequencyHz < loHz || p.frequencyHz > hiHz) continue;
          const x = hzToX(p.frequencyHz);
          const y = dbToY(p.amplitudeDbfs);
          const color = p.estimated
            ? token("--signal-warning")
            : token("--accent-primary");

          // 竖线指向坐标轴，弱化到不干扰曲线
          ctx.globalAlpha = 0.3;
          ctx.strokeStyle = color;
          ctx.lineWidth = 1;
          ctx.beginPath();
          ctx.moveTo(x, y);
          ctx.lineTo(x, plotY + plotH);
          ctx.stroke();
          ctx.globalAlpha = 1;

          // 估算读数用空心 + 警示色，实测用实心
          ctx.beginPath();
          ctx.arc(x, y, 3.5, 0, Math.PI * 2);
          ctx.fillStyle = p.estimated ? token("--bg-panel") : color;
          ctx.fill();
          ctx.strokeStyle = color;
          ctx.lineWidth = 1.5;
          ctx.stroke();

          // 峰值读数：贴在标记上方，避免与曲线重叠
          const label = `${formatHz(p.frequencyHz)} Hz`;
          ctx.textAlign = "center";
          ctx.textBaseline = "bottom";
          ctx.fillStyle = token("--text-secondary");
          ctx.fillText(label, x, Math.max(plotY + 9, y - 6));
          ctx.textAlign = "left";
        }
      } else {
        drawEmpty(ctx, plotX, plotY, plotW, plotH, uiFont, token);
      }

      // ---- 游标 ----
      if (s.cursorHz != null && s.cursorHz >= loHz && s.cursorHz <= hiHz) {
        const x = Math.round(hzToX(s.cursorHz)) + 0.5;
        ctx.strokeStyle = token("--cursor");
        ctx.globalAlpha = 0.5;
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(x, plotY);
        ctx.lineTo(x, plotY + plotH);
        ctx.stroke();
        ctx.globalAlpha = 1;

        // 光标读数气泡：只显示数字与单位，单位用次级色
        const text = `${formatHz(s.cursorHz)} Hz`;
        ctx.font = `10px ${monoFont}`;
        const tw = ctx.measureText(text).width + 10;
        const bx = Math.min(Math.max(x - tw / 2, plotX), plotX + plotW - tw);
        ctx.fillStyle = token("--bg-elevated");
        ctx.strokeStyle = token("--border-subtle");
        ctx.beginPath();
        ctx.roundRect(bx, plotY + 2, tw, 16, 3);
        ctx.fill();
        ctx.stroke();
        ctx.fillStyle = token("--text-primary");
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.fillText(text, bx + tw / 2, plotY + 10);
      }

      // ---- drag selection band ----
      if (dragRef.current) {
        const { x0, x1 } = dragRef.current;
        ctx.fillStyle = token("--selection-fill");
        ctx.fillRect(Math.min(x0, x1), plotY, Math.abs(x1 - x0), plotH);
      }
    };

    draw();
    const ro = new ResizeObserver(draw);
    ro.observe(wrap);
    return () => ro.disconnect();
  }, [db, stepHz, sampleRate, minDb, maxDb, cursorHz, peaks, viewRange]);

  const onDown = useCallback((e: React.MouseEvent<HTMLDivElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    dragRef.current = { x0: e.clientX - r.left, x1: e.clientX - r.left };
  }, []);

  const onUp = useCallback(
    () => {
      const d = dragRef.current;
      dragRef.current = null;
      const wrap = wrapRef.current;
      if (!d || !wrap || !onViewRangeChange) return;
      const dx = Math.abs(d.x1 - d.x0);
      // 几乎没拖动 = 单击复位
      if (dx < 4) {
        onViewRangeChange([0, stateRef.current.sampleRate / 2]);
        return;
      }
      const r = wrap.getBoundingClientRect();
      const a = xToHz(r.left + Math.min(d.x0, d.x1), wrap);
      const b = xToHz(r.left + Math.max(d.x0, d.x1), wrap);
      if (b - a >= 1) onViewRangeChange([a, b]);
    },
    [xToHz, onViewRangeChange],
  );

  const onMove = useCallback(
    (e: React.MouseEvent<HTMLDivElement>) => {
      const r = e.currentTarget.getBoundingClientRect();
      const nyq = stateRef.current.sampleRate / 2;
      const [lo, hi] = stateRef.current.viewRange ?? [0, nyq];

      if (dragRef.current && e.shiftKey) {
        // Shift 拖拽 = 平移视口
        const plotW = Math.max(1, r.width - PAD.left - PAD.right);
        const dx = (dragRef.current.x1 - dragRef.current.x0) / plotW;
        const span = hi - lo;
        let nlo = lo + dx * span;
        let nhi = hi + dx * span;
        if (nlo < 0) {
          nlo = 0;
          nhi = span;
        }
        if (nhi > nyq) {
          nhi = nyq;
          nlo = Math.max(0, nyq - span);
        }
        onViewRangeChange?.([nlo, nhi]);
        dragRef.current = { x0: e.clientX - r.left, x1: e.clientX - r.left };
        return;
      }

      if (dragRef.current) {
        dragRef.current.x1 = e.clientX - r.left;
        return;
      }

      // 悬停游标
      const plotW = r.width - PAD.left - PAD.right;
      const t = (e.clientX - r.left - PAD.left) / plotW;
      if (t >= 0 && t <= 1) onHoverHz?.(lo + t * (hi - lo));
    },
    [onHoverHz, onViewRangeChange],
  );

  return (
    <div
      ref={wrapRef}
      className="viz-canvas"
      onWheel={onWheel}
      onMouseDown={onDown}
      onMouseMove={onMove}
      onMouseUp={onUp}
      onMouseLeave={() => {
        dragRef.current = null;
        onHoverHz?.(null);
      }}
      onDoubleClick={() => {
        onViewRangeChange?.([0, stateRef.current.sampleRate / 2]);
      }}
    >
      <canvas ref={canvasRef} />
    </div>
  );
}

/**
 * 空状态：主提示 + 次级原因。
 *
 * 只写「尚无数据」是没用的——用户不知道是没打开文件、还是后端出错。
 * 这里明确说出数据来源，与 M1 的实际能力一致。
 */
function drawEmpty(
  ctx: CanvasRenderingContext2D,
  plotX: number,
  plotY: number,
  plotW: number,
  plotH: number,
  uiFont: string,
  token: (n: string) => string,
) {
  const cx = plotX + plotW / 2;
  const cy = plotY + plotH / 2;
  ctx.textAlign = "center";
  ctx.textBaseline = "alphabetic";
  ctx.font = `13px ${uiFont}`;
  ctx.fillStyle = token("--text-secondary");
  ctx.fillText("尚无频谱数据", cx, cy);
  ctx.font = `11px ${uiFont}`;
  ctx.fillStyle = token("--text-disabled");
  ctx.fillText("打开音频文件，或点击检查器里的「生成并分析」", cx, cy + 18);
}

/** 频率刻度格式化：12000 -> "12k" */
function formatHz(hz: number): string {
  if (hz >= 1000) {
    const k = hz / 1000;
    return Number.isInteger(k) ? `${k}k` : `${k.toFixed(1)}k`;
  }
  if (hz >= 100) return hz.toFixed(0);
  return hz.toFixed(1);
}
