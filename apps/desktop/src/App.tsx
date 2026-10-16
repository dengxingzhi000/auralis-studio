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
import {
  analyzeFile,
  analyzeSynthetic,
  fetchAppInfo,
  pickAudioFile,
  probeFile,
} from "@/lib/ipc";
import type {
  AppInfo,
  FileInfo,
  PeakInfo,
  SyntheticRequest,
} from "@/lib/contracts";
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

/** 真实文件分析结果的显示形状（供 Canvas 组件消费）。 */
interface DisplayData {
  db: Float32Array | null;
  startHz: number;
  stepHz: number;
  env: Float32Array | null;
  sampleRate: number;
  peakDbfs: number | null;
  rmsDbfs: number | null;
  crestDb: number | null;
  dc: number;
  outcome: string;
  elapsedMs: number;
  bytes: number;
  peaks: PeakInfo[];
  fftSize: number;
  window: string;
}

export default function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [req, setReq] = useState<SyntheticRequest>(DEFAULTS);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [cursorHz, setCursorHz] = useState<number | null>(null);
  const [showLeft, setShowLeft] = useState(true);
  const [showRight, setShowRight] = useState(true);
  const spectrumRef = useRef<HTMLDivElement>(null);

  // ---- 真实文件 ----
  const [files, setFiles] = useState<FileInfo[]>([]);
  const [selected, setSelected] = useState<FileInfo | null>(null);
  const [display, setDisplay] = useState<DisplayData | null>(null);

  useEffect(() => {
    fetchAppInfo()
      .then(setInfo)
      .catch((e) => setError(`无法连接 Rust 后端: ${String(e)}`));
  }, []);

  const onOpenFile = useCallback(async () => {
    setError(null);
    try {
      const path = await pickAudioFile();
      // 用户取消选择不是错误
      if (!path) return;
      setBusy(true);
      const fi = await probeFile(path);
      setFiles((prev) => {
        const rest = prev.filter((f) => f.path !== fi.path);
        return [fi, ...rest];
      });
      setSelected(fi);

      const a = await analyzeFile(fi.path, 2048);
      setDisplay({
        db: a.spectrumBins > 0 ? a.spectrumDb : null,
        startHz: 0,
        stepHz: a.spectrumStepHz,
        env: a.waveformEnvelope,
        sampleRate: a.info.sampleRate,
        peakDbfs: a.peakDbfs,
        rmsDbfs: a.rmsDbfs,
        crestDb: a.crestDb,
        dc: a.dc,
        outcome: a.outcome,
        elapsedMs: a.__elapsedMeasuredMs,
        bytes:
          a.waveformEnvelope.byteLength + a.spectrumDb.byteLength,
        peaks: a.peaks,
        fftSize: a.fftSize,
        window: a.window,
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
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
      setDisplay({
        db: r.spectrumDb,
        startHz: r.spectrumStartHz,
        stepHz: r.spectrumStepHz,
        env: r.waveformEnvelope,
        sampleRate: req.sampleRate,
        peakDbfs: r.metrics.peakDbfs,
        rmsDbfs: r.metrics.rmsDbfs,
        crestDb: r.metrics.crestDb,
        dc: 0,
        outcome: "succeeded",
        elapsedMs: r.roundTripMicros / 1000,
        bytes: r.spectrumDb.byteLength + r.waveformEnvelope.byteLength,
        peaks: [],
        fftSize: req.fftSize,
        window: "hann",
      });
    } catch (e) {
      setError(String(e));
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
        <button className="btn" onClick={onOpenFile} disabled={busy}>
          {busy ? "分析中..." : "打开文件..."}
        </button>
        <button className="btn" onClick={run} disabled={busy}>
          合成信号
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
        left={
          <ResourcePanel files={files} selected={selected} onPick={onOpenFile} />
        }
        center={
          <div className="center-stack">
            <section className="pane" style={{ flex: "1.45" }}>
              <div className="pane-head">
                <strong>频谱</strong>
                <span className="muted">
                  {display?.db ? `${display.db.length} bins` : "FFT amplitude spectrum"} ·{" "}
                  {(display?.sampleRate ?? req.sampleRate) / 2} Hz 奈奎斯特
                </span>
              </div>
              <div
                ref={spectrumRef}
                className="pane-body"
                onMouseMove={(e) => {
                  const r = e.currentTarget.getBoundingClientRect();
                  const t = (e.clientX - r.left - 46) / Math.max(1, r.width - 56);
                  const nyq = (display?.sampleRate ?? req.sampleRate) / 2;
                  setCursorHz(t >= 0 && t <= 1 ? t * nyq : null);
                }}
                onMouseLeave={() => setCursorHz(null)}
              >
                <SpectrumView
                  db={display?.db ?? null}
                  startHz={display?.startHz ?? 0}
                  stepHz={display?.stepHz ?? 1}
                  sampleRate={display?.sampleRate ?? req.sampleRate}
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
                  min/max 包络 · {display?.env?.length ?? 0} 点
                  {selected && ` · ${selected.fileName}`}
                </span>
              </div>
              <div className="pane-body">
                <WaveformView
                  envelope={display?.env ?? null}
                  sampleRate={display?.sampleRate ?? req.sampleRate}
                  channels={selected?.channels ?? 1}
                />
              </div>
            </section>
          </div>
        }
        right={
          <InspectorPanel
            req={req}
            setReq={setReq}
            display={display}
            selected={selected}
          />
        }
      />

      {/* ---- 状态栏 ---- */}
      <footer className="status-bar">
        <span className="num">
          {(display?.sampleRate ?? req.sampleRate).toLocaleString("en-US")} Hz
        </span>
        <span className="dot">·</span>
        <span>设备 未连接</span>
        <span className="dot">·</span>
        <span>
          端到端{" "}
          {display ? `${display.elapsedMs.toFixed(1)} ms` : "—"}
        </span>
        <span className="dot">·</span>
        <span className="num">{display ? `${display.bytes} B` : "—"}</span>
        <div className="spacer" />
        {error && <span className="err">{error}</span>}
        {display?.outcome === "succeeded" && (
          <span className="warn" title="M1 只接 WAV；未验证格式会标注">
            合成信号 · 未验证格式会标注
          </span>
        )}
      </footer>
    </div>
  );
}

/* ---------------------------------------------------------------- 侧栏 */

function ResourcePanel({
  files,
  selected,
  onPick,
}: {
  files: FileInfo[];
  selected: FileInfo | null;
  onPick: () => void;
}) {
  return (
    <div className="panel-body">
      <div className="label">文件</div>
      {files.length === 0 ? (
        <button className="btn wide" onClick={onPick}>
          打开音频文件...
        </button>
      ) : (
        <ul className="file-list">
          {files.map((f) => (
            <li
              key={f.path}
              className={
                "file-item" + (selected?.path === f.path ? " on" : "")
              }
              title={f.path}
            >
              <span className="file-name">{f.fileName}</span>
              <span className="file-meta num">
                {f.containerName} · {f.sampleRate.toLocaleString("en-US")} Hz ·{" "}
                {f.channels === 1 ? "单声道" : `${f.channels} ch`}
              </span>
              {!f.verified && (
                <span className="file-warn" title={f.verificationNote}>
                  未验证
                </span>
              )}
            </li>
          ))}
        </ul>
      )}

      <div className="section" style={{ marginTop: 12 }}>
        <div className="label">实时输入</div>
        <div className="muted">空 — M4 接入 WASAPI 采集</div>
      </div>
      <div className="section">
        <div className="label">分析任务</div>
        <div className="muted">M1 同步执行，尚无后台队列 UI</div>
      </div>
    </div>
  );
}

function InspectorPanel({
  req,
  setReq,
  display,
  selected,
}: {
  req: SyntheticRequest;
  setReq: React.Dispatch<React.SetStateAction<SyntheticRequest>>;
  display: DisplayData | null;
  selected: FileInfo | null;
}) {
  const set = <K extends keyof SyntheticRequest>(
    k: K,
    v: SyntheticRequest[K],
  ) => setReq((r) => ({ ...r, [k]: v }));

  return (
    <div className="panel-body">
      <div className="label">信号指标</div>
      <MetricRow label="Peak" value={display?.peakDbfs} unit="dBFS" />
      <MetricRow label="RMS" value={display?.rmsDbfs} unit="dBFS" />
      <MetricRow label="Crest" value={display?.crestDb} unit="dB" />
      <div className="metric-row">
        <span className="muted">DC Offset</span>
        <span className="num">
          {display ? display.dc.toExponential(2) : "—"}
        </span>
      </div>
      {display && (
        <div className="metric-row">
          <span className="muted">解码帧数</span>
          <span className="num">
            {Math.round(
              (display.env?.length ?? 0),
            ).toLocaleString("en-US")}
          </span>
        </div>
      )}

      {display && display.peaks.length > 0 && (
        <>
          <div className="label" style={{ marginTop: 12 }}>
            峰值
          </div>
          {display.peaks.slice(0, 8).map((p, i) => (
            <div className="metric-row" key={i}>
              <span className="muted num">{i + 1}</span>
              <span className="num">
                {fmtHz(p.frequencyHz)}
                <span className="muted"> Hz · </span>
                {p.amplitudeDbfs.toFixed(2)}
                <span className="muted"> dBFS</span>
                {p.estimated && (
                  <span className="est-tag" title={`扇贝损失 ${p.scallopingDb.toFixed(2)} dB，读数不可靠`}>
                    估算
                  </span>
                )}
              </span>
            </div>
          ))}
          <div className="note">
            FFT {display.fftSize} · {display.window} ·{" "}
            <span className="num">{display.stepHz.toFixed(3)}</span> Hz/bin
          </div>
        </>
      )}

      {selected && (
        <>
          <div className="label" style={{ marginTop: 12 }}>
            文件
          </div>
          <div className="kv">
            <span className="muted">容器</span>
            <span className="num">{selected.containerName}</span>
          </div>
          <div className="kv">
            <span className="muted">编码</span>
            <span className="num">{selected.codec}</span>
          </div>
          <div className="kv">
            <span className="muted">采样率</span>
            <span className="num">
              {selected.sampleRate.toLocaleString("en-US")} Hz
            </span>
          </div>
          <div className="kv">
            <span className="muted">声道</span>
            <span className="num">{selected.channels}</span>
          </div>
          {!selected.verified && (
            <p className="note warn-note">{selected.verificationNote}</p>
          )}
        </>
      )}

      <div className="label" style={{ marginTop: 12 }}>
        合成信号参数
      </div>
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

      <p className="note">
        dB 换算、FFT、降采样全部在 Rust 侧完成，前端只画结果
        （ADR-018）。频谱来自合成信号；真实文件的频谱在 M2 接入。
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

/** 频率显示：12000 -> "12.0k" */
function fmtHz(hz: number): string {
  return hz >= 1000 ? `${(hz / 1000).toFixed(2)}k` : hz.toFixed(0);
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