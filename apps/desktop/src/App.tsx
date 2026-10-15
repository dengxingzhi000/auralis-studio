/**
 * App —— Auralis Studio 主界面。
 *
 * M1 的范围：**打通 IPC 通路并压测**，不做任何真实音频分析。
 * 界面上会用合成信号驱动频谱与波形，目的是验证
 * 「Rust 计算 -> 二进制 IPC -> Canvas 渲染」整条链路，
 * 这是 ADR-018 要求的、在写实时模块之前必须完成的验证。
 *
 * 未实现的入口（导入文件、实时采集、导出）一律禁用并标注目标里程碑，
 * 避免做成「看起来能用但其实没功能」的样子。
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { Workspace } from "@/layouts/Workspace";
import { analyzeSynthetic, fetchAppInfo } from "@/lib/ipc";
import type { AppInfo, SyntheticRequest } from "@/lib/contracts";
import { SpectrumView } from "@/components/audio-viz/SpectrumView";
import { WaveformView } from "@/components/audio-viz/WaveformView";

const MIN_DB = -140;
const MAX_DB = 0;

const DEFAULTS: SyntheticRequest = {
  frequencyHz: 1000,
  amplitude: 0.5,
  sampleRate: 48000,
  fftSize: 4096,
  targetBins: 1200,
  envelopePoints: 2000,
};

export default function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [req, setReq] = useState<SyntheticRequest>(DEFAULTS);
  const [result, setResult] = useState<{
    db: Float32Array;
    startHz: number;
    stepHz: number;
    env: Float32Array;
    peakDbfs: number | null;
    rmsDbfs: number | null;
    crestDb: number | null;
    computeMicros: number;
    roundTripMicros: number;
    bytes: number;
  } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [cursorHz, setCursorHz] = useState<number | null>(null);
  const [showLeft, setShowLeft] = useState(true);
  const [showRight, setShowRight] = useState(true);
  const spectrumRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    fetchAppInfo()
      .then(setInfo)
      .catch((e) => setError(`无法连接 Rust 后端: ${String(e)}`));
  }, []);

  // 画布宽度变化时，把 targetBins 跟着调 —— ADR-018 规则 3：
  // 频谱点数应等于像素宽度，不多传。
  useEffect(() => {
    const el = spectrumRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      const w = el.clientWidth - 56; // 减去坐标轴留白
      if (w > 32) {
        setReq((r) => (Math.abs(r.targetBins - w) > 16 ? { ...r, targetBins: Math.floor(w) } : r));
      }
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const run = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await analyzeSynthetic(req);
      setResult({
        db: r.spectrumDb,
        startHz: r.spectrumStartHz,
        stepHz: r.spectrumStepHz,
        env: r.waveformEnvelope,
        peakDbfs: r.metrics.peakDbfs,
        rmsDbfs: r.metrics.rmsDbfs,
        crestDb: r.metrics.crestDb,
        computeMicros: r.computeMicros,
        roundTripMicros: r.roundTripMicros,
        bytes: r.spectrumDb.byteLength + r.waveformEnvelope.byteLength,
      });
    } catch (e) {
      setError(String(e));
      setResult(null);
    } finally {
      setBusy(false);
    }
  }, [req]);

  return (
    <div className="app-shell">
      {/* ---- 顶部命令栏 ---- */}
      <header className="top-bar">
        <span className="brand">
          AURALIS <span className="brand-dim">STUDIO</span>
        </span>
        <span className="toolbar-sep" />
        <button className="btn" onClick={run} disabled={busy}>
          {busy ? "分析中..." : "运行合成分析"}
        </button>
        <button className="btn" disabled title="M1 接入 WAV 解码">
          导入文件
        </button>
        <button className="btn" disabled title="M4 接入 CPAL 采集">
          实时采集
        </button>
        <button className="btn" disabled title="M5 接入 CSV/JSON 导出">
          导出
        </button>

        <div className="spacer" />

        <label className="toggle">
          <input
            type="checkbox"
            checked={showLeft}
            onChange={(e) => setShowLeft(e.target.checked)}
          />
          资源栏
        </label>
        <label className="toggle">
          <input
            type="checkbox"
            checked={showRight}
            onChange={(e) => setShowRight(e.target.checked)}
          />
          检查器
        </label>

        <span className="toolbar-sep" />
        <span className="num muted">
          {info ? `v${info.version} · ${info.algorithmVersion}` : "连接中..."}
        </span>
      </header>

      {/* ---- 主体 ---- */}
      <Workspace
        leftVisible={showLeft}
        rightVisible={showRight}
        left={<ResourcePanel />}
        center={
          <div className="center-stack">
            <section className="pane" style={{ flex: "1.45" }}>
              <div className="pane-head">
                <strong>频谱</strong>
                <span className="muted">
                  {req.fftSize} · Hann · {req.targetBins} bins ·{" "}
                  {(req.sampleRate / 2 / 1).toFixed(0)} Hz 奈奎斯特
                </span>
              </div>
              <div
                ref={spectrumRef}
                className="pane-body"
                onMouseMove={(e) => {
                  const r = e.currentTarget.getBoundingClientRect();
                  const t = (e.clientX - r.left - 46) / Math.max(1, r.width - 56);
                  setCursorHz(
                    t >= 0 && t <= 1 ? t * (req.sampleRate / 2) : null,
                  );
                }}
                onMouseLeave={() => setCursorHz(null)}
              >
                <SpectrumView
                  db={result?.db ?? null}
                  startHz={result?.startHz ?? 0}
                  stepHz={result?.stepHz ?? 1}
                  sampleRate={req.sampleRate}
                  minDb={MIN_DB}
                  maxDb={MAX_DB}
                  cursorHz={cursorHz}
                />
              </div>
            </section>

            <div className="splitter splitter-h" />

            <section className="pane" style={{ flex: 1 }}>
              <div className="pane-head">
                <strong>波形</strong>
                <span className="muted">
                  min/max 包络 · {result ? result.env.length : 0} 点
                </span>
              </div>
              <div className="pane-body">
                <WaveformView
                  envelope={result?.env ?? null}
                  sampleRate={req.sampleRate}
                  channels={1}
                />
              </div>
            </section>
          </div>
        }
        right={<InspectorPanel req={req} setReq={setReq} result={result} />}
      />

      {/* ---- 状态栏 ---- */}
      <footer className="status-bar">
        <span className="num">{req.sampleRate.toLocaleString("en-US")} Hz</span>
        <span className="dot">·</span>
        <span>设备 未连接</span>
        <span className="dot">·</span>
        <span>IPC {result ? `${(result.roundTripMicros / 1000).toFixed(2)} ms` : "—"}</span>
        <span className="dot">·</span>
        <span>计算 {result ? `${(result.computeMicros / 1000).toFixed(2)} ms` : "—"}</span>
        <span className="dot">·</span>
        <span className="num">{result ? `${result.bytes} B` : "—"}</span>
        <div className="spacer" />
        {error && <span className="err">{error}</span>}
        <span className="warn" title="M1 前的 IPC 冒烟测试">合成信号 · 非真实文件</span>
      </footer>
    </div>
  );
}

/* ---------------------------------------------------------------- 侧栏 */

function ResourcePanel() {
  return (
    <div className="panel-body">
      <Section title="文件" hint="空 — M1 接入 WAV 解码" />
      <Section title="实时输入" hint="空 — M4 接入 WASAPI 采集" />
      <Section title="分析任务" hint="空 — M1 接入任务调度" />
      <Section title="工作区" hint="面板布局持久化将在 M5 出现" />
    </div>
  );
}

function Section({ title, hint }: { title: string; hint: string }) {
  return (
    <div className="section">
      <div className="label">{title}</div>
      <div className="muted">{hint}</div>
    </div>
  );
}

function InspectorPanel({
  req,
  setReq,
  result,
}: {
  req: SyntheticRequest;
  setReq: React.Dispatch<React.SetStateAction<SyntheticRequest>>;
  result: {
    peakDbfs: number | null;
    rmsDbfs: number | null;
    crestDb: number | null;
  } | null;
}) {
  const set = <K extends keyof SyntheticRequest>(
    k: K,
    v: SyntheticRequest[K],
  ) => setReq((r) => ({ ...r, [k]: v }));

  return (
    <div className="panel-body">
      <div className="label">参数</div>

      <NumberField
        label="频率"
        unit="Hz"
        value={req.frequencyHz}
        step={50}
        onChange={(v) => set("frequencyHz", v)}
      />
      <NumberField
        label="幅值"
        unit="FS"
        value={req.amplitude}
        step={0.1}
        onChange={(v) => set("amplitude", v)}
      />
      <FftSizeField
        value={req.fftSize}
        onChange={(v) => set("fftSize", v)}
      />
      <NumberField
        label="采样率"
        unit="Hz"
        value={req.sampleRate}
        step={8000}
        onChange={(v) => set("sampleRate", v)}
      />

      <div className="label" style={{ marginTop: 12 }}>
        指标
      </div>
      <MetricRow label="Peak" value={result?.peakDbfs} unit="dBFS" />
      <MetricRow label="RMS" value={result?.rmsDbfs} unit="dBFS" />
      <MetricRow label="Crest" value={result?.crestDb} unit="dB" />

      <div className="label" style={{ marginTop: 12 }}>
        说明
      </div>
      <p className="note">
        本页用<b>合成信号</b>验证 IPC 通路，非真实文件分析。
        dB 换算、FFT、降采样全部在 Rust 侧完成，前端只画结果
        （ADR-018）。
      </p>
    </div>
  );
}

/**
 * FFT 点数选择器。
 *
 * 用固定候选而非自由输入 + log2 取整：
 *   - DSP_SPEC §5 只定义 2 的幂长度，自由输入会被静默取整，用户不知情
 *   - 避免在前端做任何数值运算（ADR-018：前端只做展示）
 */
const FFT_SIZES = [1024, 2048, 4096, 8192, 16384] as const;

function FftSizeField({
  value,
  onChange,
}: {
  value: number;
  onChange: (v: number) => void;
}) {
  return (
    <label className="field">
      <span className="field-label">FFT 点数</span>
      <span className="field-input">
        <select
          className="num"
          value={value}
          onChange={(e) => onChange(Number(e.target.value))}
        >
          {FFT_SIZES.map((n) => (
            <option key={n} value={n}>
              {n}
            </option>
          ))}
        </select>
      </span>
    </label>
  );
}

function NumberField({
  label,
  value,
  step,
  unit,
  onChange,
}: {
  label: string;
  value: number;
  step: number;
  unit?: string;
  onChange: (v: number) => void;
}) {
  return (
    <label className="field">
      <span className="field-label">{label}</span>
      <span className="field-input">
        <input
          type="number"
          className="num"
          value={value}
          step={step}
          onChange={(e) => {
            const v = Number(e.target.value);
            if (Number.isFinite(v)) onChange(v);
          }}
        />
        {unit && <span className="field-unit muted">{unit}</span>}
      </span>
    </label>
  );
}

function MetricRow({
  label,
  value,
  unit,
}: {
  label: string;
  value: number | null | undefined;
  unit: string;
}) {
  return (
    <div className="metric-row">
      <span className="muted">{label}</span>
      <span className="num">
        {/* null 表示真 -inf / 未定义，不是被钳过的数字 */}
        {value == null ? "—" : value.toFixed(4)}
        {value != null && <span className="muted"> {unit}</span>}
      </span>
    </div>
  );
}