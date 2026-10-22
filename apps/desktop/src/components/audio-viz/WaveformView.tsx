/**
 * WaveformView —— 波形包络（Canvas 2D）。
 *
 * 数据是 Rust 侧算好的 min/max 降采样包络（ADR-018 规则 3）。
 * 前端不接触原始 PCM —— UI_SPEC §5 明确「避免把每个 PCM 样本发给 UI」。
 *
 * ## 视觉约定（2026-10 样式修订）
 *
 * - 零线是唯一强调的水平线；±1 只作弱参考，避免三条线一样重
 * - 时间刻度按**量级自适应**（µs / ms / s），M1 版固定输出
 *   "0:00.000" 这种秒表格式，在毫秒级文件上完全没有可读性
 * - 多声道分道显示，道间有 1px 分隔线
 * - 颜色全部取自 CSS 令牌
 */

import { useEffect, useRef } from "react";

export interface WaveformViewProps {
  /** 包络值，值域 [-1, 1]；null = 尚无数据 */
  envelope: Float32Array | null;
  /** 采样率，用于时间轴刻度 */
  sampleRate: number;
  /** 声道数 */
  channels: number;
  /**
   * 文件总时长，秒。用于时间刻度；null 时回退到「包络数据时长」。
   * 由 Rust 侧给出（FileInfo.durationSeconds），前端不做解码推算。
   */
  durationSeconds?: number | null;
  /**
   * 多声道排布：`lanes` = 每声道一条独立轨道；`overlay` = 叠加在同一条
   * 轨道上（UI_SPEC §4 的「通道叠加」，便于看声道差异与相位问题）。
   */
  layout?: "lanes" | "overlay";
}

const PAD = { left: 50, right: 12, top: 10, bottom: 24 };

export function WaveformView({
  envelope,
  sampleRate,
  channels,
  durationSeconds = null,
  layout = "lanes",
}: WaveformViewProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const stateRef = useRef({
    envelope,
    sampleRate,
    channels,
    durationSeconds,
    layout,
  });
  stateRef.current = { envelope, sampleRate, channels, durationSeconds, layout };

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
      const token = (name: string) => css.getPropertyValue(name).trim();
      const monoFont = token("--font-mono");
      const uiFont = token("--font-ui");

      ctx.fillStyle = token("--bg-panel");
      ctx.fillRect(0, 0, w, h);

      const plotW = w - PAD.left - PAD.right;
      const plotH = h - PAD.top - PAD.bottom;
      if (plotW <= 0 || plotH <= 0) return;

      const plotX = PAD.left;
      const plotY = PAD.top;
      const s = stateRef.current;

      // ---- 声道分隔线 + 零线 ----
      const chCount = Math.max(1, s.channels);
      const overlay = s.layout === "overlay" && chCount > 1;
      // 叠加模式只有一条轨道；分道模式每声道一条
      const lanes = overlay ? 1 : chCount;
      const laneH = plotH / lanes;
      const laneTop = (ch: number) => plotY + laneH * (overlay ? 0 : ch);
      const laneCenter = (ch: number) => laneTop(ch) + laneH / 2;

      ctx.lineWidth = 1;
      ctx.font = `10px ${monoFont}`;
      ctx.textAlign = "right";
      ctx.textBaseline = "middle";

      for (let ch = 0; ch < lanes; ch++) {
        const top = Math.round(plotY + laneH * ch) + 0.5;
        const baseY = Math.round(plotY + laneH * ch + laneH / 2) + 0.5;

        // 道间分隔线（第一条不画，它就是坐标轴顶部）
        if (ch > 0) {
          ctx.strokeStyle = token("--grid-minor");
          ctx.beginPath();
          ctx.moveTo(plotX, top);
          ctx.lineTo(plotX + plotW, top);
          ctx.stroke();
        }

        // 零线：唯一的强调水平线
        ctx.strokeStyle = token("--grid-major");
        ctx.beginPath();
        ctx.moveTo(plotX, baseY);
        ctx.lineTo(plotX + plotW, baseY);
        ctx.stroke();

        // 幅值刻度：单轨时标注；分道时用声道名
        if (lanes === 1) {
          ctx.fillStyle = token("--axis-label");
          for (const v of [1, -1]) {
            const y = Math.round(plotY + ((1 - v) / 2) * plotH) + 0.5;
            ctx.strokeStyle = token("--grid-minor");
            ctx.beginPath();
            ctx.moveTo(plotX, y);
            ctx.lineTo(plotX + plotW, y);
            ctx.stroke();
            ctx.textAlign = "right";
            ctx.fillStyle = token("--axis-label");
            ctx.fillText(v > 0 ? "+1" : "-1", plotX - 7, y);
          }
          ctx.fillStyle = token("--axis-label");
          ctx.fillText("0", plotX - 7, baseY);
        } else {
          ctx.fillStyle = token("--axis-label");
          ctx.fillText(`CH${ch + 1}`, plotX - 7, baseY);
        }
      }

      // ---- 坐标轴 ----
      ctx.strokeStyle = token("--axis-line");
      ctx.beginPath();
      ctx.moveTo(plotX + 0.5, plotY);
      ctx.lineTo(plotX + 0.5, plotY + plotH + 0.5);
      ctx.lineTo(plotX + plotW, plotY + plotH + 0.5);
      ctx.stroke();

      if (s.envelope && s.envelope.length > 1) {
        const n = s.envelope.length;

        // ---- 包络 ----
        // 采样点与像素是两种密度，必须分开处理：
        //   · 每像素 ≥4 个样本 -> 按列聚合 min/max，画出竖线带
        //   · 否则（合成信号这种短片段）-> 直接连线，避免出现摩尔纹
        // M1 版只做逐点连线：2000 点在宽画布上看着还行，但在噪声上会糊成
        // 一片，而且丢掉极值；反过来一味按列聚合又会让低频信号变成梳齿。
        for (let ch = 0; ch < chCount; ch++) {
          const baseY = laneCenter(ch);
          const half = (laneH / 2) * 0.92;
          const color = token(`--channel-${ch % 6}`);
          const samplesPerCol = n / Math.max(1, plotW);

          if (samplesPerCol >= 4) {
            // 密集：逐列 min/max 竖线带
            ctx.strokeStyle = color;
            // 叠加模式下两条波形会互相覆盖，降低不透明度才看得出重叠
            ctx.globalAlpha = overlay ? 0.72 : 0.9;
            ctx.lineWidth = 1;
            ctx.beginPath();
            const cols = Math.max(1, Math.floor(plotW));
            for (let c = 0; c < cols; c++) {
              const i0 = Math.min(n - 1, Math.floor((c * n) / cols));
              const i1 = Math.min(n, Math.max(i0 + 1, Math.floor(((c + 1) * n) / cols)));
              let lo = Infinity;
              let hi = -Infinity;
              for (let i = i0; i < i1; i++) {
                const v = s.envelope[i]!;
                if (v < lo) lo = v;
                if (v > hi) hi = v;
              }
              if (!Number.isFinite(lo) || !Number.isFinite(hi)) continue;
              const x = Math.round(plotX + c) + 0.5;
              ctx.moveTo(x, baseY - Math.min(1, hi) * half);
              ctx.lineTo(x, baseY - Math.max(-1, lo) * half);
            }
            ctx.stroke();
            ctx.globalAlpha = 1;
          } else {
            // 稀疏：逐点连线。单声道时额外加一层渐变填充，
            // 与频谱曲线的视觉语言保持一致。
            ctx.beginPath();
            for (let i = 0; i < n; i++) {
              const v = s.envelope[i]!;
              const x = plotX + (i / (n - 1)) * plotW;
              const y = baseY - Math.max(-1, Math.min(1, v)) * half;
              if (i === 0) ctx.moveTo(x, y);
              else ctx.lineTo(x, y);
            }

            // 只有「单声道且不叠加」时才填充：叠加两条曲线各填一次会把
            // 下层完全盖住，反而看不出差异。
            if (lanes === 1 && chCount === 1) {
              const grad = ctx.createLinearGradient(0, baseY - half, 0, baseY + half);
              grad.addColorStop(0, token("--trace-fill-top"));
              grad.addColorStop(0.5, token("--trace-fill-bottom"));
              grad.addColorStop(1, token("--trace-fill-top"));
              ctx.save();
              ctx.lineTo(plotX + plotW, baseY);
              ctx.lineTo(plotX, baseY);
              ctx.closePath();
              ctx.fillStyle = grad;
              ctx.fill();
              ctx.restore();

              // 重画描边：fill 之后路径已被 closePath 污染
              ctx.beginPath();
              for (let i = 0; i < n; i++) {
                const v = s.envelope[i]!;
                const x = plotX + (i / (n - 1)) * plotW;
                const y = baseY - Math.max(-1, Math.min(1, v)) * half;
                if (i === 0) ctx.moveTo(x, y);
                else ctx.lineTo(x, y);
              }
            }

            ctx.strokeStyle = color;
            ctx.lineWidth = 1.2;
            ctx.lineJoin = "round";
            ctx.stroke();
          }
        }

        // ---- 时间刻度 ----
        // 总时长优先用 Rust 给的 durationSeconds；没给才回退到
        // 「包络点数 / 采样率」（这是显示回退，不是数值计算）。
        const totalSec = s.durationSeconds ?? n / s.sampleRate;
        ctx.fillStyle = token("--axis-label");
        ctx.font = `10px ${monoFont}`;
        ctx.textAlign = "center";
        ctx.textBaseline = "top";
        const ticks = 8;
        for (let i = 0; i <= ticks; i++) {
          const x = plotX + (plotW * i) / ticks;
          const sec = (totalSec * i) / ticks;
          ctx.fillText(formatTime(sec), x, plotY + plotH + 6);
        }
      } else {
        ctx.textAlign = "center";
        ctx.textBaseline = "alphabetic";
        ctx.font = `13px ${uiFont}`;
        ctx.fillStyle = token("--text-secondary");
        ctx.fillText("尚无波形数据", plotX + plotW / 2, plotY + plotH / 2);
        ctx.font = `11px ${uiFont}`;
        ctx.fillStyle = token("--text-disabled");
        ctx.fillText(
          "打开音频文件，或点击检查器里的「生成并分析」",
          plotX + plotW / 2,
          plotY + plotH / 2 + 18,
        );
      }
    };

    draw();
    const ro = new ResizeObserver(draw);
    ro.observe(wrap);
    return () => ro.disconnect();
  }, [envelope, sampleRate, channels, durationSeconds, layout]);

  return (
    <div ref={wrapRef} className="viz-canvas">
      <canvas ref={canvasRef} />
    </div>
  );
}

/**
 * 时间刻度格式化：按量级选单位。
 *
 * 3.2e-5 s -> "32.0 µs"    0.0042 s -> "4.2 ms"    12.5 s -> "12.5 s"
 * M1 版固定 "0:00.000"（秒），毫秒级文件上每个刻度都是 0:00.000，
 * 等于没有刻度。
 */
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
