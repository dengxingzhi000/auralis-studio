/**
 * WaveformView —— 波形包络（Canvas 2D）。
 *
 * 数据是 Rust 侧算好的 min/max 降采样包络（ADR-018 规则 3）。
 * 前端不接触原始 PCM ——UI_SPEC §5 明确「避免把每个 PCM 样本发给 UI」。
 */

import { useEffect, useRef } from "react";

export interface WaveformViewProps {
  /** 包络值，值域 [-1, 1]；null = 尚无数据 */
  envelope: Float32Array | null;
  /** 采样率，用于时间轴刻度 */
  sampleRate: number;
  /** 声道数 */
  channels: number;
}

const PAD = { left: 46, right: 10, top: 8, bottom: 20 };

export function WaveformView({
  envelope,
  sampleRate,
  channels,
}: WaveformViewProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const stateRef = useRef({ envelope, sampleRate, channels });
  stateRef.current = { envelope, sampleRate, channels };

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

      // ---- 幅值刻度：+1 / 0 / -1 ----
      ctx.strokeStyle = token("--grid-major");
      ctx.fillStyle = token("--axis-label");
      ctx.lineWidth = 1;
      ctx.font = "10px " + token("--font-mono");
      ctx.textAlign = "right";
      ctx.textBaseline = "middle";
      for (const v of [1, 0, -1]) {
        const y = plotY + ((1 - v) / 2) * plotH;
        ctx.beginPath();
        ctx.moveTo(plotX, Math.round(y) + 0.5);
        ctx.lineTo(plotX + plotW, Math.round(y) + 0.5);
        ctx.stroke();
        ctx.fillText(v > 0 ? `+${v}` : v.toFixed(0), plotX - 6, y);
      }

      // ---- 坐标轴 ----
      ctx.strokeStyle = token("--axis-line");
      ctx.beginPath();
      ctx.moveTo(plotX + 0.5, plotY);
      ctx.lineTo(plotX + 0.5, plotY + plotH + 0.5);
      ctx.lineTo(plotX + plotW, plotY + plotH + 0.5);
      ctx.stroke();

      const s = stateRef.current;

      if (s.envelope && s.envelope.length > 1) {
        // 多声道纵向堆叠；单声道占满全高
        const chCount = Math.max(1, s.channels);
        const laneH = plotH / chCount;

        for (let ch = 0; ch < chCount; ch++) {
          const baseY = plotY + laneH * ch + laneH / 2;
          ctx.strokeStyle = token("--axis-line");
          ctx.globalAlpha = 0.4;
          ctx.beginPath();
          ctx.moveTo(plotX, Math.round(baseY) + 0.5);
          ctx.lineTo(plotX + plotW, Math.round(baseY) + 0.5);
          ctx.stroke();
          ctx.globalAlpha = 1;

          ctx.beginPath();
          for (let i = 0; i < s.envelope.length; i++) {
            const v = s.envelope[i]!;
            const t = i / (s.envelope.length - 1);
            const x = plotX + t * plotW;
            const y = baseY - v * (laneH / 2) * 0.92;
            if (i === 0) ctx.moveTo(x, y);
            else ctx.lineTo(x, y);
          }
          ctx.strokeStyle = token(`--channel-${ch % 6}`);
          ctx.lineWidth = 1;
          ctx.stroke();
        }

        // ---- 时间刻度 ----
        // 每个包络点对应一个分析帧，帧长 = envelope.length / sampleRate 秒
        const durationSec = s.envelope.length / s.sampleRate;
        ctx.fillStyle = token("--axis-label");
        ctx.textAlign = "center";
        ctx.textBaseline = "top";
        const ticks = 8;
        for (let i = 0; i <= ticks; i++) {
          const x = plotX + (plotW * i) / ticks;
          const sec = (durationSec * i) / ticks;
          ctx.fillText(
            `0:${sec.toFixed(3).padStart(6, "0")}`,
            x,
            plotY + plotH + 4,
          );
        }
      } else {
        ctx.fillStyle = token("--text-disabled");
        ctx.font = "13px " + token("--font-ui");
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.fillText("尚无波形数据", plotX + plotW / 2, plotY + plotH / 2);
      }
    };

    draw();
    const ro = new ResizeObserver(draw);
    ro.observe(wrap);
    return () => ro.disconnect();
  }, [envelope, sampleRate, channels]);

  return (
    <div ref={wrapRef} className="viz-canvas">
      <canvas ref={canvasRef} />
    </div>
  );
}