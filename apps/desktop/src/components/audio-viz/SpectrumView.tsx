/**
 * SpectrumView —— FFT 频谱曲线（Canvas 2D）。
 *
 * 数据由 Rust 算好后传入：`db[]` 已经是**降采样到目标像素宽度**的
 * dB 值（ADR-018 规则 3）。前端只负责把点画出来，**不做任何数值计算**
 * ——不含 dB 换算、不含 FFT、不含重采样。
 *
 * 为什么用 Canvas 而不是 SVG/DOM：4096 个点若建成 SVG 节点，
 * 每帧的 DOM 操作会直接拖垮主线程。Canvas 是一次批量 path 描边。
 */

import { useEffect, useRef } from "react";

export interface SpectrumViewProps {
  /** dB 值，长度 = bins；-Infinity 表示静音段 */
  db: Float32Array | null;
  /** 频率轴起点 Hz（等间隔） */
  startHz: number;
  /** 频率轴间隔 Hz */
  stepHz: number;
  /** 采样率，用于绘制奈奎斯特标注 */
  sampleRate: number;
  /** dB 显示下限 */
  minDb: number;
  /** dB 显示上限 */
  maxDb: number;
  /** 鼠标悬停的频率游标 Hz；null 表示不显示 */
  cursorHz: number | null;
}

const PAD = { left: 46, right: 10, top: 8, bottom: 20 };

export function SpectrumView({
  db,
  startHz,
  stepHz,
  sampleRate,
  minDb,
  maxDb,
  cursorHz,
}: SpectrumViewProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const stateRef = useRef({ db, startHz, stepHz, sampleRate, minDb, maxDb, cursorHz });
  stateRef.current = { db, startHz, stepHz, sampleRate, minDb, maxDb, cursorHz };

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
      const token = (name: string) => css.getPropertyValue(name).trim();

      ctx.fillStyle = token("--bg-panel");
      ctx.fillRect(0, 0, w, h);

      const plotW = w - PAD.left - PAD.right;
      const plotH = h - PAD.top - PAD.bottom;
      if (plotW <= 0 || plotH <= 0) return;

      const plotX = PAD.left;
      const plotY = PAD.top;

      // ---- 水平网格 + dB 刻度 ----
      ctx.strokeStyle = token("--grid-major");
      ctx.fillStyle = token("--axis-label");
      ctx.lineWidth = 1;
      ctx.font = "10px " + token("--font-mono");
      ctx.textAlign = "right";
      ctx.textBaseline = "middle";

      const rows = 8;
      for (let i = 0; i <= rows; i++) {
        const y = plotY + (plotH * i) / rows;
        ctx.beginPath();
        ctx.moveTo(plotX, Math.round(y) + 0.5);
        ctx.lineTo(plotX + plotW, Math.round(y) + 0.5);
        ctx.stroke();

        // 上方是 maxDb，下方是 minDb
        const value = maxDb - ((maxDb - minDb) * i) / rows;
        ctx.fillText(value.toFixed(0), plotX - 6, y);
      }

      // ---- 垂直网格 + 频率刻度 ----
      const nyquist = sampleRate / 2;
      const cols = 8;
      ctx.strokeStyle = token("--grid-minor");
      ctx.textAlign = "center";
      ctx.textBaseline = "top";
      for (let i = 0; i <= cols; i++) {
        const x = plotX + (plotW * i) / cols;
        ctx.beginPath();
        ctx.moveTo(Math.round(x) + 0.5, plotY);
        ctx.lineTo(Math.round(x) + 0.5, plotY + plotH);
        ctx.stroke();
        const hz = (nyquist * i) / cols;
        ctx.fillText(formatHz(hz), x, plotY + plotH + 4);
      }

      // ---- 坐标轴线 ----
      ctx.strokeStyle = token("--axis-line");
      ctx.beginPath();
      ctx.moveTo(plotX + 0.5, plotY);
      ctx.lineTo(plotX + 0.5, plotY + plotH + 0.5);
      ctx.lineTo(plotX + plotW, plotY + plotH + 0.5);
      ctx.stroke();

      // ---- 频谱曲线 ----
      const s = stateRef.current;
      if (s.db && s.db.length > 1) {
        const span = Math.max(1e-6, maxDb - minDb);
        // 填充
        ctx.beginPath();
        for (let i = 0; i < s.db.length; i++) {
          const v = s.db[i]!;
          const clamped = Number.isFinite(v) ? Math.max(minDb, Math.min(maxDb, v)) : minDb;
          const t = i / (s.db.length - 1);
          const x = plotX + t * plotW;
          const y = plotY + (1 - (clamped - minDb) / span) * plotH;
          if (i === 0) ctx.moveTo(x, y);
          else ctx.lineTo(x, y);
        }
        // 回到基线闭合，做面积填充
        ctx.lineTo(plotX + plotW, plotY + plotH);
        ctx.lineTo(plotX, plotY + plotH);
        ctx.closePath();
        ctx.fillStyle = "rgba(87, 166, 255, 0.16)";
        ctx.fill();

        // 描边
        ctx.beginPath();
        for (let i = 0; i < s.db.length; i++) {
          const v = s.db[i]!;
          const clamped = Number.isFinite(v) ? Math.max(minDb, Math.min(maxDb, v)) : minDb;
          const t = i / (s.db.length - 1);
          const x = plotX + t * plotW;
          const y = plotY + (1 - (clamped - minDb) / span) * plotH;
          if (i === 0) ctx.moveTo(x, y);
          else ctx.lineTo(x, y);
        }
        ctx.strokeStyle = token("--accent-primary");
        ctx.lineWidth = 1.4;
        ctx.stroke();
      } else {
        ctx.fillStyle = token("--text-disabled");
        ctx.font = "13px " + token("--font-ui");
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.fillText("尚无频谱数据", plotX + plotW / 2, plotY + plotH / 2);
      }

      // ---- 频率游标 ----
      if (s.cursorHz != null && s.cursorHz >= 0 && s.cursorHz <= nyquist) {
        const x = plotX + (s.cursorHz / nyquist) * plotW;
        ctx.strokeStyle = token("--cursor");
        ctx.globalAlpha = 0.55;
        ctx.beginPath();
        ctx.moveTo(x, plotY);
        ctx.lineTo(x, plotY + plotH);
        ctx.stroke();
        ctx.globalAlpha = 1;
      }
    };

    draw();
    const ro = new ResizeObserver(draw);
    ro.observe(wrap);
    return () => ro.disconnect();
  }, [db, startHz, stepHz, sampleRate, minDb, maxDb, cursorHz]);

  return (
    <div ref={wrapRef} className="viz-canvas">
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
  return hz.toFixed(0);
}