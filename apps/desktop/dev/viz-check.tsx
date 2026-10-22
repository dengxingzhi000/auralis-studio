/**
 * 渲染自检页 —— **仅开发用**，不进入生产构建。
 *
 * ## 为什么需要它
 *
 * 换容器（WebView2 → Electron 自带 Chromium）时，必须能回答一个问题：
 * 「同样的数据、同样的三块画布，在新容器里还会不会花屏？」
 *
 * 主界面依赖 Rust 后端，换容器后 IPC 还没接上时画布是空的 —— 空画布
 * 不经过声谱图的 ImageData / drawImage 路径，测不出问题。所以这里用
 * **确定性合成数据**把三块画布喂满，作为两个容器的 A/B 对照页：
 *
 * ```text
 * WebView2: http://127.0.0.1:5173/viz-check.html
 * Electron: electron . --dev  (同上 URL)
 * ```
 *
 * 数据用固定种子的 PRNG 生成：两次运行逐像素一致，出问题时能确认是
 * 渲染差异而不是数据差异。
 */

import { useMemo } from "react";
import { createRoot } from "react-dom/client";

import { SpectrumView } from "@/components/audio-viz/SpectrumView";import { SpectrogramView } from "@/components/audio-viz/SpectrogramView";
import { WaveformView } from "@/components/audio-viz/WaveformView";

import "@/styles/tokens.css";
import "@/styles/globals.css";
import "@/styles/layout.css";

const SAMPLE_RATE = 48_000;
const NYQUIST = SAMPLE_RATE / 2;
const MIN_DB = -140;
const MAX_DB = 0;

/** 确定性 PRNG（mulberry32）：同一种子 → 同一张图。 */
function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** 在 base 之上叠加一个 dB 域的窄峰（洛伦兹形状）。 */
function withPeak(
  base: number,
  hz: number,
  centerHz: number,
  peakDb: number,
  halfWidthHz: number,
): number {
  const d = (hz - centerHz) / halfWidthHz;
  return Math.max(base, peakDb - 60 * d * d);
}

/** 频谱曲线：1/f 底噪 + 60 Hz 低频隆起 + 1 kHz / 3 kHz 两个窄峰。 */
function makeSpectrum(): { db: Float32Array; stepHz: number } {
  const bins = 1024;
  const stepHz = NYQUIST / bins;
  const rnd = mulberry32(0x5eed);
  const db = new Float32Array(bins);

  for (let i = 0; i < bins; i++) {
    const hz = i * stepHz;
    let v = -104 - 6 * Math.log10(1 + hz / 200) + (rnd() - 0.5) * 3;
    v = withPeak(v, hz, 60, -34, 120);
    v = withPeak(v, hz, 1_000, -16, 90);
    v = withPeak(v, hz, 3_000, -34, 180);
    // 20 kHz 以上模拟抗混叠滚降
    if (hz > 20_000) v -= (hz - 20_000) / 250;
    db[i] = Math.max(MIN_DB, Math.min(MAX_DB, v));
  }

  return { db, stepHz };
}

/**
 * 声谱图矩阵：行优先 `matrix[frame * bins + bin]`，
 * 四条随时间起伏的水平谱线 + 1/f 底噪。
 */
function makeSpectrogram(): {
  matrix: Float32Array;
  width: number;
  height: number;
} {
  const frames = 512;
  const bins = 256;
  const binHz = NYQUIST / bins;
  const rnd = mulberry32(0xb0a7);
  const matrix = new Float32Array(frames * bins);

  const bands = [
    { hz: 500, db: -26, wobble: 0.0 },
    { hz: 1_000, db: -12, wobble: 1.7 },
    { hz: 1_500, db: -30, wobble: 3.1 },
    { hz: 3_000, db: -42, wobble: 0.9 },
  ];

  for (let t = 0; t < frames; t++) {
    const u = t / (frames - 1);
    for (let b = 0; b < bins; b++) {
      const hz = b * binHz;
      let v = -106 - 4 * Math.log10(1 + hz / 400) + (rnd() - 0.5) * 6;

      for (const band of bands) {
        const halfWidth = Math.max(binHz * 1.5, band.hz * 0.07);
        const mod = 0.72 + 0.28 * Math.sin(u * Math.PI * 8 + band.wobble);
        v = withPeak(v, hz, band.hz, band.db + 10 * (mod - 1), halfWidth);
      }

      matrix[t * bins + b] = Math.max(MIN_DB, Math.min(MAX_DB, v));
    }
  }

  return { matrix, width: frames, height: bins };
}

/** 波形包络：两段渐弱的音块 + 少量噪声，值域 [-1, 1]。 */
function makeEnvelope(): Float32Array {
  const n = 4_000;
  const rnd = mulberry32(0xe11);
  const out = new Float32Array(n);

  for (let i = 0; i < n; i++) {
    const u = i / (n - 1);
    const burst = Math.max(0, Math.sin(u * Math.PI * 3)) ** 0.6;
    const carrier =
      Math.sin(u * 1_400) * 0.62 + Math.sin(u * 3_700) * 0.28 + Math.sin(u * 190) * 0.1;
    const v = carrier * burst + (rnd() - 0.5) * 0.05;
    out[i] = Math.max(-1, Math.min(1, v));
  }

  return out;
}

/** 单块画布外框：定高、relative，供 `.viz-canvas` 的 absolute 定位使用。 */
function Pane({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section
      style={{
        position: "relative",
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
        background: "var(--bg-panel)",
        border: "1px solid var(--border-subtle)",
        borderRadius: 6,
        overflow: "hidden",
      }}
    >
      <div
        style={{
          flex: "0 0 auto",
          padding: "4px 8px",
          font: "11px var(--font-ui)",
          color: "var(--text-secondary)",
          borderBottom: "1px solid var(--border-subtle)",
        }}
      >
        {title}
      </div>
      <div style={{ position: "relative", flex: 1, minHeight: 0 }}>{children}</div>
    </section>
  );
}

function VizCheck() {
  const spectrum = useMemo(makeSpectrum, []);
  const spectrogram = useMemo(makeSpectrogram, []);
  const envelope = useMemo(makeEnvelope, []);

  const container = window.auralis?.container;
  const runtime = container
    ? `Electron ${container.versions.electron} · Chromium ${container.versions.chrome}`
    : "浏览器（无 preload）";

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        display: "flex",
        flexDirection: "column",
        gap: 6,
        padding: 6,
        background: "var(--bg-app)",
      }}
    >
      <div
        style={{
          flex: "0 0 auto",
          font: "11px var(--font-mono)",
          color: "var(--text-secondary)",
        }}
      >
        渲染自检 · 确定性合成数据 · {runtime}
      </div>

      <div style={{ flex: "1 1 0", minHeight: 0, display: "flex", gap: 6 }}>
        <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
          <Pane title="频谱 · 1 kHz / 3 kHz">
            <SpectrumView
              db={spectrum.db}
              stepHz={spectrum.stepHz}
              sampleRate={SAMPLE_RATE}
              minDb={MIN_DB}
              maxDb={MAX_DB}
              cursorHz={1_000}
              mode="area"
            />
          </Pane>
        </div>
        <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
          <Pane title="波形包络">
            <WaveformView
              envelope={envelope}
              sampleRate={SAMPLE_RATE}
              channels={1}
              durationSeconds={1.6}
              layout="lanes"
            />
          </Pane>
        </div>
      </div>

      <div style={{ flex: "1 1 0", minHeight: 0, display: "flex" }}>
        <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
          <Pane title="声谱图 · STFT 2048/512">
            <SpectrogramView
              matrix={spectrogram.matrix}
              width={spectrogram.width}
              height={spectrogram.height}
              minDb={MIN_DB}
              maxDb={MAX_DB}
              cursorHz={1_000}
              nyquistHz={NYQUIST}
              durationSeconds={1.6}
            />
          </Pane>
        </div>
      </div>
    </div>
  );
}

const container = document.getElementById("root");
if (!container) throw new Error("#root not found in viz-check.html");

createRoot(container).render(<VizCheck />);
