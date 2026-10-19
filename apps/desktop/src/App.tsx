/**
 * App —— Auralis Studio 主界面。
 *
 * M1 的范围：**打通 IPC 通路并压测**，不做任何真实音频分析。
 * 界面上用合成信号驱动频谱与波形，验证
 * 「Rust 计算 -> 二进制 IPC -> Canvas 渲染」整条链路（ADR-018）。
 *
 * 未实现的入口（实时采集、导出、其他分析视图）一律禁用并标注目标里程碑，
 * 避免做成「看起来能用但其实没功能」的样子。
 *
 * ## 布局（2026-10 样式修订）
 *
 *   ┌ 顶栏：品牌 + 全局动作 ─────────────────────────────┐
 *   ├ 侧栏 ┬ 中央画布（卡片堆叠） ┬ 检查器 ──────────────┤
 *   └ 状态栏：设备 / 采样率 / 端到端 / 载荷 ─────────────┘
 *
 * 侧栏是**导航式**的：文件列表 + 未实现的视图入口（禁用并标里程碑），
 * 与参考界面一致；不再把「实时输入 / 分析任务」当纯文本堆在底部。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

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
import { SpectrogramView } from "@/components/audio-viz/SpectrogramView";
import { WaveformView } from "@/components/audio-viz/WaveformView";
import {
  Button,
  Checkbox,
  Collapsible,
  DataRow,
  ErrorBar,
  FormField,
  IconButton,
  NavGroup,
  NavItem,
  Select,
} from "@/components/ui/primitives";
import {
  IconDevice,
  IconFilePlus,
  IconGauge,
  IconHelp,
  IconMore,
  IconSearch,
  IconSettings,
  IconSpectrogram,
  IconSpectrum,
  IconTasks,
  IconWaveform,
} from "@/components/ui/icons";

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
  spectrogram: Float32Array | null;
  spectrogramFrames: number;
  spectrogramBins: number;
  spectrogramHopMs: number;
}

export default function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [req, setReq] = useState<SyntheticRequest>(DEFAULTS);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<UiError | null>(null);
  const [cursorHz, setCursorHz] = useState<number | null>(null);
  const [viewRange, setViewRange] = useState<[number, number] | null>(null);
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
      .catch((e) => setError(describeError(e, "无法连接 Rust 后端")));
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
        bytes: a.waveformEnvelope.byteLength + a.spectrumDb.byteLength,
        peaks: a.peaks,
        fftSize: a.fftSize,
        window: a.window,
        spectrogram:
          a.spectrogramFrames > 0 && a.spectrogramBins > 0
            ? a.spectrogramDb
            : null,
        spectrogramFrames: a.spectrogramFrames,
        spectrogramBins: a.spectrogramBins,
        spectrogramHopMs: a.spectrogramHopMs,
      });
    } catch (e) {
      setError(describeError(e, "打开文件失败"));
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
        setReq((r) =>
          Math.abs(r.targetBins - w) > 16
            ? { ...r, targetBins: Math.floor(w) }
            : r,
        );
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
        spectrogram: null,
        spectrogramFrames: 0,
        spectrogramBins: 0,
        spectrogramHopMs: 0,
      });
    } catch (e) {
      setError(describeError(e, "合成信号分析失败"));
    } finally {
      setBusy(false);
    }
  }, [req]);

  const sampleRate = display?.sampleRate ?? req.sampleRate;

  /**
   * 分析覆盖的时长，秒。
   *
   * 优先用 Rust 的 `FileInfo.durationSeconds`（唯一权威来源）；
   * 该字段缺失时才回退到「包络点数 / 采样率」——只用于画刻度，
   * 不参与任何 DSP 判断。
   */
  const durationSeconds = useMemo(() => {
    if (selected?.durationSeconds != null) return selected.durationSeconds;
    const n = display?.env?.length ?? 0;
    return n > 1 ? n / sampleRate : 0;
  }, [selected, display, sampleRate]);

  const version = useMemo(
    () => (info ? `v${info.version}` : "连接中…"),
    [info],
  );

  return (
    <div className="app-shell">
      {/* ---- 顶部命令栏 ---- */}
      <header className="top-bar">
        <span className="brand">
          AURALIS <span className="brand-dim">STUDIO</span>
        </span>
        <span className="toolbar-sep" />
        <Button onClick={onOpenFile} disabled={busy}>
          {busy ? "分析中…" : "打开文件…"}
        </Button>
        <Button onClick={run} disabled={busy}>
          合成信号
        </Button>
        <Button disabled title="M4 接入 CPAL 采集">
          实时采集
        </Button>
        <Button disabled title="M5 接入 CSV/JSON 导出">
          导出
        </Button>

        <div className="spacer" />

        <Checkbox
          label="资源栏"
          checked={showLeft}
          onChange={(e) => setShowLeft(e.target.checked)}
        />
        <Checkbox
          label="检查器"
          checked={showRight}
          onChange={(e) => setShowRight(e.target.checked)}
        />
      </header>

      {/* ---- 主体 ---- */}
      <Workspace
        leftVisible={showLeft}
        rightVisible={showRight}
        left={
          <ResourcePanel
            files={files}
            selected={selected}
            onPick={onOpenFile}
            busy={busy}
            version={version}
          />
        }
        center={
          <div className="center-stack">
            <section className="pane" style={{ flex: "1.35" }}>
              <div className="pane-head">
                <span className="pane-title">频谱</span>
                <span className="pane-meta">
                  {display?.db
                    ? `${display.db.length} bins`
                    : "等待数据"}
                  <span className="sep">·</span>
                  {display ? `${display.fftSize} 点 FFT` : "FFT 4096"}
                  <span className="sep">·</span>
                  {fmtHzInt(sampleRate / 2)} Hz 奈奎斯特
                  {viewRange && (
                    <>
                      <span className="sep">·</span>
                      <span className="num">
                        {fmtHz(viewRange[0])}–{fmtHz(viewRange[1])} Hz
                      </span>
                    </>
                  )}
                </span>
                <span className="spacer" />
                <span className="pane-meta num">
                  {cursorHz != null ? `${fmtHz(cursorHz)} Hz` : "—"}
                </span>
                {viewRange && (
                  <Button
                    size="tiny"
                    onClick={() => setViewRange(null)}
                    title="回到全带宽"
                  >
                    复位缩放
                  </Button>
                )}
              </div>
              <div className="pane-body" ref={spectrumRef}>
                <SpectrumView
                  db={display?.db ?? null}
                  stepHz={display?.stepHz ?? 1}
                  sampleRate={sampleRate}
                  minDb={MIN_DB}
                  maxDb={MAX_DB}
                  cursorHz={cursorHz}
                  peaks={display?.peaks ?? []}
                  viewRange={viewRange}
                  onViewRangeChange={setViewRange}
                  onHoverHz={setCursorHz}
                />
              </div>
              <div className="pane-note">
                拖拽框选缩放 · 滚轮以光标为中心缩放 · 双击复位
                <span className="sep">·</span>
                dBFS，未做校准
              </div>
            </section>

            <div className="splitter splitter-h" />

            <section className="pane" style={{ flex: 1 }}>
              <div className="pane-head">
                <span className="pane-title">声谱图</span>
                <span className="pane-meta">
                  STFT 2048/512
                  <span className="sep">·</span>
                  {display && display.spectrogramFrames > 0
                    ? `${display.spectrogramFrames} × ${display.spectrogramBins}`
                    : "等待数据"}
                  {display && display.spectrogramHopMs > 0 && (
                    <>
                      <span className="sep">·</span>
                      <span className="num">
                        {display.spectrogramHopMs.toFixed(2)} ms/列
                      </span>
                    </>
                  )}
                </span>
              </div>
              <div className="pane-body">
                <SpectrogramView
                  matrix={display?.spectrogram ?? null}
                  width={display?.spectrogramFrames ?? 0}
                  height={display?.spectrogramBins ?? 0}
                  minDb={MIN_DB}
                  maxDb={MAX_DB}
                  cursorHz={cursorHz}
                  nyquistHz={sampleRate / 2}
                  durationSeconds={durationSeconds}
                />
              </div>
            </section>

            <div className="splitter splitter-h" />

            <section className="pane" style={{ flex: 1 }}>
              <div className="pane-head">
                <span className="pane-title">波形</span>
                <span className="pane-meta">
                  min/max 包络
                  <span className="sep">·</span>
                  {display?.env?.length ?? 0} 点
                  {selected && (
                    <>
                      <span className="sep">·</span>
                      {selected.fileName}
                    </>
                  )}
                </span>
              </div>
              <div className="pane-body">
                <WaveformView
                  envelope={display?.env ?? null}
                  sampleRate={sampleRate}
                  channels={selected?.channels ?? 1}
                  durationSeconds={durationSeconds}
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
            busy={busy}
            onRun={run}
            info={info}
            viewRange={viewRange}
            onResetViewRange={() => setViewRange(null)}
          />
        }
      />

      {/* ---- 状态栏 ---- */}
      <footer className="status-bar">
        <span className="state" title="M4 接入 WASAPI 采集">
          设备未连接
        </span>
        <span className="dot">·</span>
        <span className="status-chip">
          <span>采样率</span>
          <span className="v num">
            {sampleRate.toLocaleString("en-US")} Hz
          </span>
        </span>
        <span className="status-chip">
          <span>端到端</span>
          <span className="v num">
            {display ? `${display.elapsedMs.toFixed(1)} ms` : "—"}
          </span>
        </span>
        <span className="status-chip">
          <span>载荷</span>
          <span className="v num">
            {display ? `${(display.bytes / 1024).toFixed(1)} KiB` : "—"}
          </span>
        </span>
        <div className="spacer" />
        {error && (
          <span className="err" title={`${error.title}\n${error.detail ?? ""}`}>
            {error.title}
          </span>
        )}
        {!error && display?.outcome === "succeeded" && (
          <span className="warn" title="M1 只接 WAV；未验证格式会标注">
            未验证格式会标注
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
  busy,
  version,
}: {
  files: FileInfo[];
  selected: FileInfo | null;
  onPick: () => void;
  busy: boolean;
  version: string;
}) {
  return (
    <>
      <div className="panel-body">
        <button className="nav-cta" onClick={onPick} disabled={busy}>
          <IconFilePlus size={15} />
          {busy ? "分析中…" : "打开音频文件…"}
        </button>

        <NavGroup label="文件">
          {files.length === 0 ? (
            <li>
              <div className="empty" style={{ padding: "10px 4px" }}>
                <span className="empty-hint">
                  还没有文件。M1 只接 WAV / FLAC / MP3 / AIFF / OGG，
                  未经验证的容器会在名字旁标注。
                </span>
              </div>
            </li>
          ) : (
            files.map((f) => (
              <li key={f.path}>
                <div
                  className={
                    "file-item" + (selected?.path === f.path ? " on" : "")
                  }
                  title={f.path}
                >
                  <span className="file-name">{f.fileName}</span>
                  <span className="file-meta num">
                    {f.containerName} · {f.sampleRate.toLocaleString("en-US")}{" "}
                    Hz · {f.channels === 1 ? "单声道" : `${f.channels} ch`}
                    {!f.verified && (
                      <span className="file-warn" title={f.verificationNote}>
                        未验证
                      </span>
                    )}
                  </span>
                </div>
              </li>
            ))
          )}
        </NavGroup>

        <NavGroup label="分析视图">
          <NavItem icon={<IconWaveform />} active>
            波形
          </NavItem>
          <NavItem icon={<IconSpectrum />} active>
            频谱
          </NavItem>
          <NavItem icon={<IconSpectrogram />} disabled tag="M3">
            声谱图缩放
          </NavItem>
          <NavItem icon={<IconGauge />} disabled tag="M2">
            电平表 / 指标
          </NavItem>
          <NavItem icon={<IconTasks />} disabled tag="M3">
            后台任务队列
          </NavItem>
          <NavItem icon={<IconDevice />} disabled tag="M4">
            实时输入
          </NavItem>
        </NavGroup>

        <NavGroup label="工程">
          <NavItem icon={<IconSettings />} disabled tag="M5">
            偏好设置
          </NavItem>
        </NavGroup>
      </div>

      <div className="sidebar-foot">
        <ul className="nav">
          <NavItem icon={<IconSearch />} disabled tag="M5">
            搜索
          </NavItem>
          <NavItem icon={<IconHelp />} disabled>
            帮助
          </NavItem>
        </ul>
        <div className="sidebar-user">
          <span className="avatar">AS</span>
          <span className="who">
            <b>Auralis Studio</b>
            <span className="num">{version} · 本地分析</span>
          </span>
          <span className="spacer" />
          <IconButton label="关于与版本信息" disabled>
            <IconMore size={15} />
          </IconButton>
        </div>
      </div>
    </>
  );
}

/* -------------------------------------------------------------- 检查器 */

function InspectorPanel({
  req,
  setReq,
  display,
  selected,
  busy,
  onRun,
  info,
  viewRange,
  onResetViewRange,
}: {
  req: SyntheticRequest;
  setReq: React.Dispatch<React.SetStateAction<SyntheticRequest>>;
  display: DisplayData | null;
  selected: FileInfo | null;
  busy: boolean;
  onRun: () => void;
  info: AppInfo | null;
  viewRange: [number, number] | null;
  onResetViewRange: () => void;
}) {
  const set = <K extends keyof SyntheticRequest>(
    k: K,
    v: SyntheticRequest[K],
  ) => setReq((r) => ({ ...r, [k]: v }));

  return (
    <div className="panel-body">
      {display?.outcome === "failed" && (
        <div style={{ marginBottom: 8 }}>
          <ErrorBar title="分析未成功完成" />
        </div>
      )}

      <div className="label" style={{ marginBottom: 5 }}>
        信号指标
      </div>
      <div className="readout">
        <MetricRow label="Peak" value={display?.peakDbfs} unit="dBFS" />
        <MetricRow label="RMS" value={display?.rmsDbfs} unit="dBFS" />
        <MetricRow label="Crest" value={display?.crestDb} unit="dB" />
        <DataRow
          label="DC Offset"
          value={display ? display.dc.toExponential(2) : "—"}
        />
        <DataRow
          label="包络点数"
          value={
            display ? display.env?.length.toLocaleString("en-US") ?? "—" : "—"
          }
        />
      </div>

      {display && display.peaks.length > 0 && (
        <div style={{ marginTop: 12 }}>
          <div className="label" style={{ marginBottom: 5 }}>
            峰值
          </div>
          <div className="readout">
            {display.peaks.slice(0, 8).map((p, i) => (
              <div className="metric-row" key={i}>
                <span className="muted num">{i + 1}</span>
                <span className="num">
                  {fmtHz(p.frequencyHz)}
                  <span className="muted"> Hz · </span>
                  {p.amplitudeDbfs.toFixed(2)}
                  <span className="muted"> dBFS</span>
                  {p.estimated && (
                    <span
                      className="est-tag"
                      title={`扇贝损失 ${p.scallopingDb.toFixed(2)} dB，读数不可靠`}
                    >
                      估算
                    </span>
                  )}
                </span>
              </div>
            ))}
          </div>
          <div className="note">
            FFT {display.fftSize} · {display.window} ·{" "}
            <span className="num">{display.stepHz.toFixed(3)}</span> Hz/bin
          </div>
        </div>
      )}

      <div style={{ marginTop: 12 }}>
        <div className="label" style={{ marginBottom: 2 }}>
          参数
        </div>
        <Collapsible title="合成信号" defaultOpen>
          <FormField label="频率" unit="Hz">
            <input
              type="number"
              className="num"
              value={req.frequencyHz}
              step={50}
              onChange={(e) => {
                const v = Number(e.target.value);
                if (Number.isFinite(v)) set("frequencyHz", v);
              }}
            />
          </FormField>
          <FormField label="幅值" unit="FS">
            <input
              type="number"
              className="num"
              value={req.amplitude}
              step={0.1}
              onChange={(e) => {
                const v = Number(e.target.value);
                if (Number.isFinite(v)) set("amplitude", v);
              }}
            />
          </FormField>
          <FftSizeField
            value={req.fftSize}
            onChange={(v) => set("fftSize", v)}
          />
          <div style={{ marginTop: 8 }}>
            <Button variant="primary" wide onClick={onRun} disabled={busy}>
              {busy ? "计算中…" : "生成并分析"}
            </Button>
          </div>
          <p className="note">合成 1 kHz 正弦，用于验证 IPC 通路与渲染。</p>
        </Collapsible>

        {selected && (
          <Collapsible
            title="文件"
            meta={selected.verified ? undefined : "未验证"}
            defaultOpen
          >
            <DataRow label="容器" value={selected.containerName} />
            <DataRow label="编码" value={selected.codec} />
            <DataRow
              label="采样率"
              value={`${selected.sampleRate.toLocaleString("en-US")} Hz`}
            />
            <DataRow label="声道" value={String(selected.channels)} />
            <DataRow
              label="时长"
              value={
                selected.durationSeconds == null
                  ? "—"
                  : `${selected.durationSeconds.toFixed(3)} s`
              }
            />
            <DataRow
              label="文件大小"
              value={`${(selected.fileSizeBytes / 1024).toFixed(1)} KiB`}
            />
            {!selected.verified && (
              <p className="note warn-note">{selected.verificationNote}</p>
            )}
          </Collapsible>
        )}

        <Collapsible title="显示" defaultOpen>
          <DataRow label="dB 范围" value={`${MIN_DB} … ${MAX_DB}`} />
          <DataRow label="视口" value={fmtRange(viewRange)} />
          <DataRow label="频谱点数" value={String(req.targetBins)} />
          <div style={{ marginTop: 8 }}>
            <Button
              size="tiny"
              wide
              onClick={onResetViewRange}
              disabled={!viewRange}
            >
              复位缩放
            </Button>
          </div>
        </Collapsible>

        <Collapsible title="关于">
          <DataRow label="版本" value={info?.version ?? "—"} />
          <DataRow label="算法" value={info?.algorithmVersion ?? "—"} mono />
          <DataRow label="DSP 规范" value={info?.dspSpecRevision ?? "—"} />
          <p className="note">
            dB 换算、FFT、降采样全部在 Rust 侧完成，前端只画结果（ADR-018）。
            频谱来自合成信号；真实文件的频谱在 M2 接入。
          </p>
        </Collapsible>
      </div>
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
    <FormField label="FFT 点数">
      <Select
        className="num"
        value={value}
        onValueChange={(v) => onChange(Number(v))}
      >
        {FFT_SIZES.map((n) => (
          <option key={n} value={n}>
            {n}
          </option>
        ))}
      </Select>
    </FormField>
  );
}

/* ---------------------------------------------------------------- 工具 */

/** 频率显示：12000 -> "12.0k" */
function fmtHz(hz: number): string {
  return hz >= 1000 ? `${(hz / 1000).toFixed(2)}k` : hz.toFixed(0);
}

function fmtHzInt(hz: number): string {
  return Math.round(hz).toLocaleString("en-US");
}

/**
 * 缩放范围摘要：`null` 表示全带宽。
 *
 * 只格式化显示值，不做任何计算（ADR-018 规则 1）。
 */
function fmtRange(r: [number, number] | null): string {
  if (!r) return "全带宽";
  return `${fmtHz(r[0])} – ${fmtHz(r[1])} Hz`;
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
    <DataRow
      label={label}
      value={
        <>
          {/* null 表示真 -inf / 未定义，不是被钳过的数字 */}
          {value == null ? "—" : value.toFixed(4)}
          {value != null && <span className="muted"> {unit}</span>}
        </>
      }
    />
  );
}

/* -------------------------------------------------------------- 错误文案 */

interface UiError {
  title: string;
  detail: string | null;
}

/**
 * 把未知异常翻译成「能看懂的一句话 + 可复制的原始文本」。
 *
 * M1 阶段直接把 `String(e)` 铺在状态栏，结果是满屏 `TypeError: ...`：
 * 既看不出是什么操作失败，也无法定位。这里做最小翻译，未知错误原样保留
 * detail，不编造原因。
 */
function describeError(e: unknown, action: string): UiError {
  const raw = e instanceof Error ? `${e.name}: ${e.message}` : String(e);
  if (raw.includes("is not a multiple of 4")) {
    return { title: `${action}：后端返回的二进制载荷长度非法`, detail: raw };
  }
  if (raw.includes("not found") || raw.includes("No such file")) {
    return { title: `${action}：文件不存在或已被移动`, detail: raw };
  }
  if (raw.includes("Unsupported") || raw.includes("unsupported")) {
    return { title: `${action}：该格式尚未验证支持`, detail: raw };
  }
  return { title: `${action}（详见悬停提示）`, detail: raw };
}
