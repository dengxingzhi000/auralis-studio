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

const PAD = { left: 46, right: 10, top: 8, bottom: 22 };

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

      const css = getComputedStyle(document.documentElement);
      const token = (n: string) => css.getPropertyValue(n).trim();

      ctx.fillStyle = token("--bg-panel");
      ctx.fillRect(0, 0, w, h);

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

      // ---- 水平网格 + dB 刻度 ----
      ctx.strokeStyle = token("--grid-major");
      ctx.fillStyle = token("--axis-label");
      ctx.lineWidth = 1;
      ctx.font = "10px " + token("--font-mono");
      ctx.textAlign = "right";
      ctx.textBaseline = "middle";
      for (let i = 0; i <= 8; i++) {
        const y = plotY + (plotH * i) / 8;
        ctx.beginPath();
        ctx.moveTo(plotX, Math.round(y) + 0.5);
        ctx.lineTo(plotX + plotW, Math.round(y) + 0.5);
        ctx.stroke();
        const v = s.maxDb - ((s.maxDb - s.minDb) * i) / 8;
        ctx.fillText(v.toFixed(0), plotX - 6, y);
      }

      // ---- 垂直网格 + 频率刻度 ----
      ctx.strokeStyle = token("--grid-minor");
      ctx.textAlign = "center";
      ctx.textBaseline = "top";
      for (let i = 0; i <= 8; i++) {
        const hz = loHz + (spanHz * i) / 8;
        const x = hzToX(hz);
        ctx.beginPath();
        ctx.moveTo(Math.round(x) + 0.5, plotY);
        ctx.lineTo(Math.round(x) + 0.5, plotY + plotH);
        ctx.stroke();
        ctx.fillText(formatHz(hz), x, plotY + plotH + 4);
      }

      // ---- axes ----
      ctx.strokeStyle = token("--axis-line");
      ctx.beginPath();
      ctx.moveTo(plotX + 0.5, plotY);
      ctx.lineTo(plotX + 0.5, plotY + plotH + 0.5);
      ctx.lineTo(plotX + plotW, plotY + plotH + 0.5);
      ctx.stroke();

      // ---- curve ----
      if (s.db && s.db.length > 1) {
        const tracePath = (color: string, width: number) => {
          ctx.beginPath();
          for (let i = 0; i < s.db!.length; i++) {
            const hz = i * s.stepHz;
            if (hz < loHz || hz > hiHz) continue;
            const v = s.db![i]!;
            const x = hzToX(hz);
            const y = Number.isFinite(v) ? dbToY(v) : dbToY(s.minDb);
            if (i === 0) ctx.moveTo(x, y);
            else ctx.lineTo(x, y);
          }
          ctx.strokeStyle = color;
          ctx.lineWidth = width;
          ctx.stroke();
        };

        // fill under the curve first
        ctx.beginPath();
        let started = false;
        for (let i = 0; i < s.db.length; i++) {
          const hz = i * s.stepHz;
          if (hz < loHz || hz > hiHz) continue;
          const v = s.db[i]!;
          const x = hzToX(hz);
          const y = Number.isFinite(v) ? dbToY(v) : dbToY(s.minDb);
          if (!started) {
            ctx.moveTo(x, plotY + plotH);
            ctx.lineTo(x, y);
            started = true;
          } else ctx.lineTo(x, y);
        }
        if (started) {
          const lastHz = (s.db.length - 1) * s.stepHz;
          ctx.lineTo(hzToX(Math.min(lastHz, hiHz)), plotY + plotH);
          ctx.closePath();
          ctx.fillStyle = "rgba(87, 166, 255, 0.16)";
          ctx.fill();
        }

        tracePath(token("--accent-primary"), 1.4);

        // ---- peak markers ----
        for (const p of s.peaks) {
          if (p.frequencyHz < loHz || p.frequencyHz > hiHz) continue;
          const x = hzToX(p.frequencyHz);
          const y = dbToY(p.amplitudeDbfs);
          // 估算读数用空心 + 警示色，实测用实心
          ctx.beginPath();
          ctx.arc(x, y, 3, 0, Math.PI * 2);
          ctx.strokeStyle = p.estimated ? token("--signal-warning") : token("--accent-primary");
          ctx.fillStyle = p.estimated ? token("--bg-panel") : token("--accent-primary");
          ctx.lineWidth = 1.5;
          ctx.fill();
          ctx.stroke();
          // 竖线指向坐标轴
          ctx.globalAlpha = 0.35;
          ctx.beginPath();
          ctx.moveTo(x, y);
          ctx.lineTo(x, plotY + plotH);
          ctx.strokeStyle = p.estimated
            ? token("--signal-warning")
            : token("--accent-primary");
          ctx.stroke();
          ctx.globalAlpha = 1;
        }
      } else {
        ctx.fillStyle = token("--text-disabled");
        ctx.font = "13px " + token("--font-ui");
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.fillText("尚无频谱数据", plotX + plotW / 2, plotY + plotH / 2);
      }

      // ---- cursor ----
      if (s.cursorHz != null && s.cursorHz >= loHz && s.cursorHz <= hiHz) {
        const x = hzToX(s.cursorHz);
        ctx.strokeStyle = token("--cursor");
        ctx.globalAlpha = 0.55;
        ctx.beginPath();
        ctx.moveTo(x, plotY);
        ctx.lineTo(x, plotY + plotH);
        ctx.stroke();
        ctx.globalAlpha = 1;
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

/** 频率刻度格式化：12000 -> "12k" */
function formatHz(hz: number): string {
  if (hz >= 1000) {
    const k = hz / 1000;
    return Number.isInteger(k) ? `${k}k` : `${k.toFixed(1)}k`;
  }
  if (hz >= 100) return hz.toFixed(0);
  return hz.toFixed(1);
}