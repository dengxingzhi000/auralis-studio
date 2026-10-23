/**
 * App —— Auralis Studio 主界面。
 *
 * 责任划分：Rust 做全部音频计算，前端只做展示与任务状态（ADR-018 规则 1）。
 * 文件分析是**可取消**的长任务：任务号由前端分配，运行中可用 Esc 或状态栏
 * 的「取消」按钮中止；取消不算失败，也不会清掉已有结果。
 *
 * 未实现的分析视图一律禁用并标注，避免做成「看起来能用但其实没功能」
 * 的样子。
 *
 * ## 布局（2026-10 样式修订）
 *
 *   ┌ 顶栏：品牌 + 全局动作 ─────────────────────────────┐
 *   ├ 侧栏 ┬ 中央画布（卡片堆叠） ┬ 检查器 ──────────────┤
 *   └ 状态栏：设备 / 采样率 / 端到端 / 载荷 ─────────────┘
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { Workspace } from "@/layouts/Workspace";
import {
  analyzeFile,
  analyzeSynthetic,
  cancelTask,
  exportAnalysis,
  fetchAppInfo,
  fetchSpectrogramView,
  isAudioPath,
  pickAudioFile,
  pickExportDestination,
  probeFile,
} from "@/lib/ipc";
import type {
  AppInfo,
  FileInfo,
  PeakInfo,
  RealtimeSnapshot,
  SyntheticRequest,
} from "@/lib/contracts";
import { LevelMeter } from "@/components/audio-viz/LevelMeter";
import { RealtimePanel } from "@/components/audio-viz/RealtimePanel";
import { SpectrumView } from "@/components/audio-viz/SpectrumView";
import { SpectrogramView } from "@/components/audio-viz/SpectrogramView";
import { WaveformView } from "@/components/audio-viz/WaveformView";
import {
  asBoolean,
  asNumberInRange,
  readStoredJson,
  writeStoredJson,
} from "@/lib/persist";
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

/**
 * 稳定的空峰值数组。
 *
 * `liveDb ? [] : (display?.peaks ?? [])` 每次渲染都会产生新引用，而
 * SpectrumView 的绘制 effect 依赖 `peaks` —— 任意一次重渲染都会重绘频谱
 * 并重建后备存储。实时态下峰值本来就该留空（频率位置对不上），
 * 用模块级常量固定引用。
 */
const NO_PEAKS: PeakInfo[] = [];

/** 面板可见性的持久化键。 */
const VISIBILITY_KEY = "auralis.workspace.visibility.v1";

/** 校验存下来的可见性；任何字段损坏都整条丢弃，回到默认值。 */
function readVisibility(v: unknown): { left: boolean; right: boolean } | null {
  if (typeof v !== "object" || v === null) return null;
  const rec = v as Record<string, unknown>;
  const left = asBoolean(rec.left);
  const right = asBoolean(rec.right);
  return left != null && right != null ? { left, right } : null;
}

const DEFAULTS: SyntheticRequest = {
  frequencyHz: 1000,
  amplitude: 0.5,
  sampleRate: 48000,
  fftSize: 4096,
  targetBins: 1200,
  envelopePoints: 2000,
};

/** 声谱图一屏的默认时长（秒），与 Rust 侧 `SPECTRUM_WINDOW_SECONDS` 一致。 */
const SPECTROGRAM_WINDOW_SECONDS = 30;

/** 当前显示的数据来自哪里。 */
type DisplaySource = "file" | "synthetic";

/** 真实文件分析结果的显示形状（供 Canvas 组件消费）。 */
interface DisplayData {
  /**
   * 这份数据的来源。
   *
   * 必须记着：合成信号分析会**替换**画布上的数据，但文件元数据还在。
   * 不带来源就分不清「现在画的是文件的还是合成信号的」——
   * 波形面板顶着文件名画合成信号、检查器报着文件的时长，
   * 看起来就像一个挂着文件名的默认页。
   */
  source: DisplaySource;
  db: Float32Array | null;
  startHz: number;
  stepHz: number;
  env: Float32Array | null;
  sampleRate: number;
  /**
   * 这份数据实际覆盖的时长，秒。
   *
   * 文件取 Rust 的解码时长（截断文件短于声明值）；合成信号取
   * `sampleCount / sampleRate`。别用包络点数推算——包络是降采样后的
   * 显示点，和信号时长没有固定比例。
   */
  durationSeconds: number;
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
  /** 声谱图矩阵的绝对起点（秒） */
  spectrogramStartSeconds: number;
  /** 每声道峰值 dBFS；空数组表示未提供（合成信号没有声道概念） */
  peakPerChannel: (number | null)[];
  /** 超过满刻度的样本数（|x| > 1）。> 0 时电平表显示削波警示 */
  overFullScaleSamples: number;
  /** 削波平顶片段数（启发式，不是模拟削波的证据） */
  clippingRegions: number;
  /** 被清理的非有限样本数（NaN / Inf） */
  sanitizedSamples: number;
}

/** 频谱显示模式：填充（面积）或仅曲线。 */
type SpectrumMode = "area" | "line";

/** 多声道波形的排布：分道 / 叠加。 */
type ChannelLayout = "lanes" | "overlay";

/** dB 量程预设。 */
const DB_RANGES = [
  { label: "全量程 −140…0", min: MIN_DB, max: MAX_DB },
  { label: "常用 −100…0", min: -100, max: MAX_DB },
  { label: "细节 −80…−10", min: -80, max: -10 },
  { label: "底噪 −140…−40", min: MIN_DB, max: -40 },
] as const;

const DISPLAY_KEY = "auralis.display.v1";
/** 上次打开的文件；M5 会话恢复只存路径，不存分析结果。 */
const SESSION_KEY = "auralis.session.v1";

/** 显示偏好。字段缺一个就整条丢弃，回到默认值（不猜用户意图）。 */
interface DisplayPrefs {
  mode: SpectrumMode;
  dbRange: number;
  channelLayout: ChannelLayout;
}

function readDisplayPrefs(v: unknown): DisplayPrefs | null {
  if (typeof v !== "object" || v === null) return null;
  const rec = v as Record<string, unknown>;
  const mode = rec.mode === "line" || rec.mode === "area" ? rec.mode : null;
  const dbRange = asNumberInRange(rec.dbRange, 0, DB_RANGES.length - 1);
  const channelLayout =
    rec.channelLayout === "lanes" || rec.channelLayout === "overlay"
      ? rec.channelLayout
      : null;
  return mode != null && dbRange != null && channelLayout != null
    ? { mode, dbRange, channelLayout }
    : null;
}

/** 正在进行、可取消的文件分析。`null` = 没有在跑的文件任务。 */
interface ActiveTask {
  id: number;
  label: string;
}

/** 会话恢复记录：上一次成功打开的文件路径。 */
interface SessionRecord {
  path: string;
}

function readSession(value: unknown): SessionRecord | null {
  if (typeof value !== "object" || value === null) return null;
  const rec = value as Record<string, unknown>;
  return typeof rec.path === "string" && rec.path.length > 0
    ? { path: rec.path }
    : null;
}

export default function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [req, setReq] = useState<SyntheticRequest>(DEFAULTS);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<UiError | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [cursorHz, setCursorHz] = useState<number | null>(null);
  const [viewRange, setViewRange] = useState<[number, number] | null>(null);
  // 面板可见性也持久化：一上来就隐藏资源栏的人，不希望每次启动都重新勾一遍
  const [showLeft, setShowLeft] = useState(
    () => readStoredJson(VISIBILITY_KEY, readVisibility)?.left ?? true,
  );
  const [showRight, setShowRight] = useState(
    () => readStoredJson(VISIBILITY_KEY, readVisibility)?.right ?? true,
  );
  const spectrumRef = useRef<HTMLDivElement>(null);
  /** 任务号由前端分配：见 AppState::try_register_token 的说明 */
  const taskSeq = useRef(0);
  const [activeTask, setActiveTask] = useState<ActiveTask | null>(null);
  /** 上一个成功 probe 的路径，供「重试」使用 */
  const lastPathRef = useRef<string | null>(null);

  // ---- 显示偏好（持久化）----
  const storedDisplay = useRef(readStoredJson(DISPLAY_KEY, readDisplayPrefs))
    .current;
  const [spectrumMode, setSpectrumMode] = useState<SpectrumMode>(
    storedDisplay?.mode ?? "area",
  );
  const [dbRangeIdx, setDbRangeIdx] = useState(storedDisplay?.dbRange ?? 0);
  const [channelLayout, setChannelLayout] = useState<ChannelLayout>(
    storedDisplay?.channelLayout ?? "lanes",
  );

  // ---- 真实文件 ----
  const [files, setFiles] = useState<FileInfo[]>([]);
  const [selected, setSelected] = useState<FileInfo | null>(null);
  const [display, setDisplay] = useState<DisplayData | null>(null);
  /** 声谱图视口当前起点（秒）；与 display 里的矩阵起点保持一致 */
  const [spectroStart, setSpectroStart] = useState(0);
  const [spectroLoading, setSpectroLoading] = useState(false);
  /** 正在跑的视口请求任务号；用于丢弃过期响应并取消旧请求 */
  const spectroTaskRef = useRef<number | null>(null);

  // ---- 实时采集（M4）----
  const [livePeak, setLivePeak] = useState<number | null>(null);
  const [liveRms, setLiveRms] = useState<number | null>(null);
  const [liveDb, setLiveDb] = useState<Float32Array | null>(null);
  const [liveStepHz, setLiveStepHz] = useState(1);
  const [liveSampleRate, setLiveSampleRate] = useState<number | null>(null);

  useEffect(() => {
    writeStoredJson(VISIBILITY_KEY, { left: showLeft, right: showRight });
  }, [showLeft, showRight]);

  useEffect(() => {
    writeStoredJson(DISPLAY_KEY, {
      mode: spectrumMode,
      dbRange: dbRangeIdx,
      channelLayout,
    });
  }, [spectrumMode, dbRangeIdx, channelLayout]);

  useEffect(() => {
    fetchAppInfo()
      .then(setInfo)
      .catch((e) => setError(describeError(e, "无法连接 Rust 后端")));
  }, []);

  /** 取下一个任务号。毫秒时间戳 ×1000 + 序号，避开后端 0..N 的分配区间。 */
  const nextTaskId = (): number => {
    taskSeq.current += 1;
    return Date.now() * 1000 + taskSeq.current;
  };

  /**
   * 分析一个已知路径。文件选择器与拖放都走这里，保证两条入口行为一致。
   */
  const analyzePath = useCallback(async (path: string, restore?: "clean" | "crashed") => {
    setError(null);
    setNotice(null);
    setBusy(true);
    const id = nextTaskId();
    try {
      const fi = await probeFile(path);
      lastPathRef.current = fi.path;
      writeStoredJson(SESSION_KEY, { path: fi.path });
      setFiles((prev) => [fi, ...prev.filter((f) => f.path !== fi.path)]);
      setSelected(fi);

      setActiveTask({ id, label: fi.fileName });
      const a = await analyzeFile(fi.path, 2048, id);

      // 取消不是失败：保留上一次的数据，只在状态栏说明
      if (a.outcome === "cancelled") {
        setNotice(`已取消分析：${fi.fileName}`);
        return;
      }

      setDisplay({
        source: "file",
        db: a.spectrumBins > 0 ? a.spectrumDb : null,
        startHz: 0,
        stepHz: a.spectrumStepHz,
        env: a.waveformEnvelope,
        sampleRate: a.info.sampleRate,
        durationSeconds: a.decodedDurationSeconds,
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
        spectrogramStartSeconds: a.spectrogramStartSeconds,
        peakPerChannel: a.peakDbfsPerChannel,
        overFullScaleSamples: a.overFullScaleSamples,
        clippingRegions: a.clippingRegions,
        sanitizedSamples: a.sanitizedSamples,
      });
      setSpectroStart(a.spectrogramStartSeconds);
      // 新文件的结果替换了旧矩阵；旧的视口请求已经无意义。
      if (spectroTaskRef.current != null) {
        void cancelTask(spectroTaskRef.current).catch(() => undefined);
        spectroTaskRef.current = null;
        setSpectroLoading(false);
      }

      // 截断的提示优先级最高：恢复会话的提示不能把它盖掉，
      // 否则用户看到「已恢复上次打开的文件」，却不知道文件其实不完整。
      if (a.truncated) {
        // 「缺多少」比「只分析到哪」更有用：末尾少 0.01 秒和少 30 秒
        // 是两种完全不同的处境，用户据此判断要不要重新获取文件。
        const missing = Math.max(
          0,
          a.declaredDurationSeconds - a.decodedDurationSeconds,
        );
        setNotice(
          `文件不完整：已分析 ${a.decodedDurationSeconds.toFixed(2)} 秒` +
            `（声明 ${a.declaredDurationSeconds.toFixed(2)} 秒，缺 ${missing.toFixed(2)} 秒）`,
        );
      } else if (restore === "crashed") {
        setNotice(`上次会话未正常结束，已恢复：${fi.fileName}`);
      } else if (restore === "clean") {
        setNotice(`已恢复上次打开的文件：${fi.fileName}`);
      }
    } catch (e) {
      setError(describeError(e, "打开文件失败"));
    } finally {
      setActiveTask(null);
      setBusy(false);
    }
  }, []);

  /**
   * 启动入口：
   *
   * - `?open=<绝对路径>`（开发/自动化）：直接分析指定文件；
   * - 否则恢复上次成功打开的文件（M5 会话恢复）。
   *
   * Electron 主进程在上次异常退出时会给 URL 加 `recovered=1`，
   * 界面据此区分「正常恢复」与「上次会话异常结束」。
   */
  const startupRestoreStarted = useRef(false);
  useEffect(() => {
    if (startupRestoreStarted.current) return;
    startupRestoreStarted.current = true;

    const params = new URLSearchParams(window.location.search);
    const requested = params.get("open");
    if (requested && isAudioPath(requested)) {
      void analyzePath(requested);
      return;
    }

    const session = readStoredJson(SESSION_KEY, readSession);
    if (!session || !isAudioPath(session.path)) return;

    const recovered = params.get("recovered") === "1";
    void analyzePath(session.path, recovered ? "crashed" : "clean");
  }, [analyzePath]);

  const onOpenFile = useCallback(async () => {
    setError(null);
    try {
      const path = await pickAudioFile();
      // 用户取消选择不是错误
      if (!path) return;
      await analyzePath(path);
    } catch (e) {
      setError(describeError(e, "打开文件失败"));
    }
  }, [analyzePath]);

  /** 取消当前文件分析。任务号是前端给的，所以运行中也能取消。 */
  const onCancelTask = useCallback(async () => {
    const task = activeTask;
    if (!task) return;
    try {
      const found = await cancelTask(task.id);
      setNotice(found ? `正在取消：${task.label}` : "任务已经结束");
    } catch (e) {
      setError(describeError(e, "取消失败"));
    }
  }, [activeTask]);

  const onRetry = useCallback(() => {
    const path = lastPathRef.current;
    if (path) void analyzePath(path);
  }, [analyzePath]);

  /**
   * 导出当前文件的分析结果。
   *
   * 格式由保存对话框里选的扩展名决定（Rust 侧会校验两者一致），
   * 这样只有一个按钮、没有额外的格式开关。用户取消保存不是错误。
   */
  const onExport = useCallback(async () => {
    if (!selected) return;
    setError(null);
    setNotice(null);
    try {
      const base = selected.fileName.replace(/\.[^.]+$/, "");
      const picked = await pickExportDestination(`${base}.csv`);
      if (!picked) return;
      // 用户可能把扩展名删掉，补一个默认值而不是让 Rust 端报错
      const destination = /\.(csv|json)$/i.test(picked) ? picked : `${picked}.csv`;
      const format = destination.toLowerCase().endsWith(".json")
        ? ("json" as const)
        : ("csv" as const);

      setBusy(true);
      const out = await exportAnalysis(selected.path, destination, format);
      setNotice(
        `已导出 ${out.format.toUpperCase()} · ${(out.bytesWritten / 1024).toFixed(1)} KiB → ${out.destination}`,
      );
    } catch (e) {
      setError(describeError(e, "导出失败"));
    } finally {
      setBusy(false);
    }
  }, [selected]);

  // ---- 键盘：Ctrl/Cmd+O 打开，Esc 取消，Ctrl/Cmd+R 重试 ----
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      const key = e.key.toLowerCase();
      if (mod && key === "o") {
        e.preventDefault();
        void onOpenFile();
      } else if (e.key === "Escape" && activeTask) {
        e.preventDefault();
        void onCancelTask();
      } else if (mod && key === "r" && lastPathRef.current) {
        e.preventDefault();
        onRetry();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [activeTask, onCancelTask, onOpenFile, onRetry]);

  // ---- 拖放：桌面音频应用的基本预期 ----
  //
  // Electron 不再提供 `onDragDropEvent`，走标准 DOM 拖放。Chromium 不把
  // 磁盘路径交给页面，路径由 preload 的 `getPathForFile` 解析。
  useEffect(() => {
    const onDragOver = (e: DragEvent) => {
      // 不 preventDefault 的话浏览器会拒绝 drop 事件。
      if (e.dataTransfer?.types.includes("Files")) e.preventDefault();
    };

    const onDrop = (e: DragEvent) => {
      const files = Array.from(e.dataTransfer?.files ?? []);
      if (files.length === 0) return;
      e.preventDefault();

      const api = window.auralis;
      if (!api) {
        setError({ title: "拖放仅在桌面容器中可用", detail: null });
        return;
      }

      const paths = files.map((file) => api.getPathForFile(file));
      const path = paths.find(isAudioPath);
      if (path) void analyzePath(path);
      else setError({ title: "拖入的文件不是受支持的音频格式", detail: null });
    };

    window.addEventListener("dragover", onDragOver);
    window.addEventListener("drop", onDrop);
    return () => {
      window.removeEventListener("dragover", onDragOver);
      window.removeEventListener("drop", onDrop);
    };
  }, [analyzePath]);

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
        source: "synthetic",
        db: r.spectrumDb,
        startHz: r.spectrumStartHz,
        stepHz: r.spectrumStepHz,
        env: r.waveformEnvelope,
        sampleRate: r.metrics.sampleRate,
        durationSeconds: r.metrics.sampleCount / r.metrics.sampleRate,
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
        spectrogramStartSeconds: 0,
        peakPerChannel: [],
        // 合成信号是纯正弦，超量程/削波/清理计数都为 0：
        // 写 0 而不是省略，界面才能一致性地区分「没有」与「未知」
        overFullScaleSamples: r.metrics.overFullScaleSamples,
        clippingRegions: r.metrics.clippingRegions,
        sanitizedSamples: r.metrics.sanitizedSamples,
      });
      setSpectroStart(0);
      // 合成信号会**替换**当前显示的结果。开着文件时，用户很容易把
      // 「分析合成信号」当成「分析这个文件」——明说一次，别让人以为
      // 自己的文件被分析成了那声 1 kHz 正弦。
      if (selected) {
        setNotice(
          `已切换到合成信号分析：${selected.fileName} 的显示已被替换 · 点左侧文件名可恢复`,
        );
      }
    } catch (e) {
      setError(describeError(e, "合成信号分析失败"));
    } finally {
      setBusy(false);
    }
  }, [req, selected]);

  /**
   * 当前用于画频率轴的采样率。
   *
   * 实时采集时**必须**用采集设备的采样率：沿用一个已打开文件的 44.1 kHz
   * 会把 48 kHz 的实时数据画到错误的频率位置上，而界面看不出任何异常。
   */
  const sampleRate = liveDb
    ? (liveSampleRate ?? req.sampleRate)
    : (display?.sampleRate ?? req.sampleRate);

  /** 当前 dB 量程。索引越界时回落到全量程，不让坏偏好把坐标轴画反。 */
  const dbRange = DB_RANGES[dbRangeIdx] ?? DB_RANGES[0];
  const minDb = dbRange.min;
  const maxDb = dbRange.max;

  /**
   * 画布横轴覆盖的时长，秒。只用于画刻度，不参与任何 DSP 判断。
   *
   * 文件：优先用 `FileInfo.durationSeconds`（头里声明的时长）；
   * 合成信号：用 Rust 报的 `sampleCount / sampleRate` —— 合成信号是
   * **一个 FFT 帧**，不是「包络点数 / 采样率」。算错就会把 85 ms 的波形
   * 拉到文件的 204 秒坐标轴上，画出来是一条贴底的直线。
   */
  const durationSeconds = useMemo(() => {
    if (display?.source === "synthetic") return display.durationSeconds;
    if (selected?.durationSeconds != null) return selected.durationSeconds;
    return display?.durationSeconds ?? 0;
  }, [selected, display]);

  /**
   * 声谱图矩阵自身覆盖的时长。
   *
   * 矩阵列数 × 每列 hop 才是这张图的时间跨度；用文件总时长去标它会在大文件
   * （只分析了开头一段）上错得离谱，所以这里单独算。
   */
  const spectrogramSeconds = useMemo(() => {
    if (!display || display.spectrogramFrames <= 0) return 0;
    return (display.spectrogramFrames * display.spectrogramHopMs) / 1000;
  }, [display]);

  /** 一屏的请求时长：文件比 30 秒短时不请求不存在的部分。 */
  const spectrogramWindow = useMemo(
    () =>
      durationSeconds > 0
        ? Math.min(SPECTROGRAM_WINDOW_SECONDS, durationSeconds)
        : SPECTROGRAM_WINDOW_SECONDS,
    [durationSeconds],
  );

  /**
   * 请求声谱图的一个时间窗口。
   *
   * 只重算窗口，不重跑整文件分析。滚动/拖动时旧请求会被取消，
   * 过期响应按任务号丢弃——否则慢请求返回后会把视口拉回旧位置。
   */
  const loadSpectrogramWindow = useCallback(
    async (targetStart: number) => {
      // 文件分析进行中时拒绝新视口请求：此时 display 还是旧文件的矩阵，
      // 结果会被随后的分析结果覆盖，徒增一次解码。
      // 合成信号的 display 里没有声谱图；往里塞文件的矩阵会造出
      // 「频谱是合成信号、声谱图是文件」的混合状态。
      if (
        !selected ||
        !display ||
        display.source === "synthetic" ||
        durationSeconds <= 0 ||
        busy
      )
        return;

      const maxStart = Math.max(0, durationSeconds - spectrogramWindow);
      const start = Math.min(Math.max(0, targetStart), maxStart);

      const previous = spectroTaskRef.current;
      if (previous != null) void cancelTask(previous).catch(() => undefined);

      const id = nextTaskId();
      spectroTaskRef.current = id;
      setSpectroLoading(true);

      try {
        const view = await fetchSpectrogramView(
          selected.path,
          start,
          spectrogramWindow,
          id,
        );
        if (spectroTaskRef.current !== id) return; // 过期响应
        if (view.outcome === "cancelled") return;

        setDisplay((prev) =>
          prev
            ? {
                ...prev,
                spectrogram:
                  view.frames > 0 && view.bins > 0
                    ? view.spectrogramDb
                    : null,
                spectrogramFrames: view.frames,
                spectrogramBins: view.bins,
                spectrogramHopMs: view.hopMs,
                spectrogramStartSeconds: view.startSeconds,
              }
            : prev,
        );
        setSpectroStart(view.startSeconds);
      } catch (e) {
        if (spectroTaskRef.current === id) {
          setError(describeError(e, "声谱图视口加载失败"));
        }
      } finally {
        if (spectroTaskRef.current === id) {
          spectroTaskRef.current = null;
          setSpectroLoading(false);
        }
      }
    },
    [selected, display, durationSeconds, spectrogramWindow, busy],
  );

  const panSpectrogram = useCallback(
    (deltaSeconds: number) => {
      void loadSpectrogramWindow(spectroStart + deltaSeconds);
    },
    [loadSpectrogramWindow, spectroStart],
  );

  const version = useMemo(
    () => (info ? `v${info.version}` : "连接中…"),
    [info],
  );

  /**
   * 声谱图翻页按钮只在「画的是文件」时出现。
   *
   * 合成信号的 display 里没有声谱图矩阵；此时翻页会把文件的矩阵塞进来，
   * 变成「频谱是合成信号、声谱图是文件」的混合状态。
   */
  const showSpectroPaging =
    selected != null &&
    display?.source !== "synthetic" &&
    durationSeconds > 0;

  return (
    <div className="app-shell">
      {/* ---- 顶部命令栏 ---- */}
      <header className="top-bar">
        <span className="brand">
          AURALIS <span className="brand-dim">STUDIO</span>
        </span>
        <span className="toolbar-sep" />
        <Button onClick={onOpenFile} disabled={busy} title="Ctrl+O">
          {busy ? "分析中…" : "打开文件…"}
        </Button>
        <Button
          onClick={run}
          disabled={busy}
          title={
            selected
              ? `用合成信号自检 IPC 与渲染（会顶替 ${selected.fileName} 的显示，不修改文件）`
              : "用合成信号自检 IPC 与渲染"
          }
        >
          合成信号自检
        </Button>
        {activeTask && (
          <Button
            onClick={onCancelTask}
            title="取消当前分析（Esc）"
            className="danger"
          >
            取消
          </Button>
        )}
        <Button
          onClick={() => {
            setShowLeft(true);
            window.requestAnimationFrame(() => {
              document
                .getElementById("realtime-panel")
                ?.scrollIntoView({ block: "center" });
            });
          }}
          title="在左侧资源栏中启动实时输入"
        >
          实时采集
        </Button>
        <Button
          onClick={onExport}
          disabled={busy || !selected || display == null}
          title={
            selected
              ? `导出 ${selected.fileName} 的分析结果（CSV / JSON，含算法版本与参数）`
              : "先打开一个音频文件"
          }
        >
          导出…
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
            onSelect={analyzePath}
            busy={busy}
            version={version}
            livePeak={livePeak}
            liveRms={liveRms}
            onLiveSnapshot={(snap) => {
              setLiveDb(snap ? snap.spectrumDb : null);
              setLiveStepHz(snap ? snap.binHz : 1);
              setLiveSampleRate(snap ? snap.sampleRate : null);
              setLivePeak(snap ? snap.peakDbfs : null);
              setLiveRms(snap ? snap.rmsDbfs : null);
            }}
          />
        }
        center={
          <div className="center-stack">
            {error && (
              <div style={{ marginBottom: 8 }}>
                <ErrorBar
                  title={error.title}
                  detail={error.detail}
                  action={
                    <>
                      {lastPathRef.current && (
                        <Button size="tiny" onClick={onRetry} title="Ctrl+R">
                          重试
                        </Button>
                      )}
                      <Button size="tiny" onClick={() => setError(null)}>
                        关闭
                      </Button>
                    </>
                  }
                />
              </div>
            )}
            <section className="pane" style={{ flex: "1.35" }}>
              <div className="pane-head">
                <span className="pane-title">
                  频谱
                  {liveDb && <span className="rt-badge">实时</span>}
                </span>
                <span className="pane-meta">
                  {liveDb
                    ? `${liveDb.length} bins`
                    : display?.db
                      ? `${display.db.length} bins`
                      : "等待数据"}
                  <span className="sep">·</span>
                  {liveDb
                    ? `${liveStepHz.toFixed(1)} Hz/bin`
                    : display
                      ? `${display.fftSize} 点 FFT`
                      : "FFT 4096"}
                  <span className="sep">·</span>
                  {fmtHzInt(sampleRate / 2)} Hz 奈奎斯特
                  {/* 画的是合成信号时必须标出来：否则用户会以为这是文件的频谱。 */}
                  {!liveDb && display?.source === "synthetic" && (
                    <>
                      <span className="sep">·</span>
                      <span className="est-tag">合成信号</span>
                    </>
                  )}
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
                {/* 光标读数只在悬停时出现：常驻一个 "—" 只是噪声 */}
                {cursorHz != null && (
                  <span className="pane-meta num" title="光标频率">
                    {fmtHz(cursorHz)} Hz
                  </span>
                )}
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
                {/* 实时数据上不能叠文件分析出的峰值标记：频率位置对不上，
                    会让人以为那是当前信号的峰值。 */}
                <SpectrumView
                  db={liveDb ?? display?.db ?? null}
                  stepHz={liveDb ? liveStepHz : (display?.stepHz ?? 1)}
                  sampleRate={sampleRate}
                  minDb={minDb}
                  maxDb={maxDb}
                  cursorHz={cursorHz}
                  peaks={liveDb ? NO_PEAKS : display?.peaks}
                  viewRange={viewRange}
                  onViewRangeChange={setViewRange}
                  onHoverHz={setCursorHz}
                  mode={spectrumMode}
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
                    : display?.source === "synthetic"
                      ? "合成信号不产生声谱图"
                      : "等待数据"}
                  {display && display.spectrogramHopMs > 0 && (
                    <>
                      <span className="sep">·</span>
                      <span className="num">
                        {display.spectrogramHopMs.toFixed(2)} ms/列
                      </span>
                    </>
                  )}
                  {display && display.spectrogramFrames > 0 && (
                    <>
                      <span className="sep">·</span>
                      <span className="num" title="矩阵覆盖的绝对时间范围">
                        {fmtClock(spectroStart)}–
                        {fmtClock(spectroStart + spectrogramSeconds)}
                      </span>
                    </>
                  )}
                </span>
                <span className="spacer" />
                {showSpectroPaging && (
                  <>
                    <Button
                      size="tiny"
                      onClick={() =>
                        void loadSpectrogramWindow(
                          spectroStart - spectrogramSeconds * 0.8,
                        )
                      }
                      disabled={busy || spectroLoading || spectroStart <= 0}
                      title="向前一屏"
                    >
                      ◀
                    </Button>
                    <Button
                      size="tiny"
                      onClick={() => void loadSpectrogramWindow(0)}
                      disabled={busy || spectroLoading || spectroStart <= 0}
                      title="回到文件开头"
                    >
                      开头
                    </Button>
                    <Button
                      size="tiny"
                      onClick={() =>
                        void loadSpectrogramWindow(
                          spectroStart + spectrogramSeconds * 0.8,
                        )
                      }
                      disabled={
                        busy ||
                        spectroLoading ||
                        spectroStart >=
                          Math.max(0, durationSeconds - spectrogramWindow) -
                            1e-6
                      }
                      title="向后一屏"
                    >
                      ▶
                    </Button>
                  </>
                )}
                {spectroLoading && <span className="pane-meta">载入…</span>}
              </div>
              <div className="pane-body">
                <SpectrogramView
                  matrix={display?.spectrogram ?? null}
                  width={display?.spectrogramFrames ?? 0}
                  height={display?.spectrogramBins ?? 0}
                  minDb={minDb}
                  maxDb={maxDb}
                  cursorHz={cursorHz}
                  nyquistHz={sampleRate / 2}
                  durationSeconds={spectrogramSeconds}
                  startSeconds={spectroStart}
                  onPan={panSpectrogram}
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
                  {/* 标题跟着**显示数据的来源**走：合成信号不能顶着文件名，
                      否则看起来就是一个挂着文件名的默认页。 */}
                  {display?.source === "synthetic" ? (
                    <>
                      <span className="sep">·</span>
                      <span className="est-tag">合成信号</span>
                    </>
                  ) : (
                    selected && (
                      <>
                        <span className="sep">·</span>
                        {selected.fileName}
                      </>
                    )
                  )}
                </span>
              </div>
              <div className="pane-body">
                {/* 合成信号是单声道：别拿已打开文件的声道数去切它的包络。 */}
                <WaveformView
                  envelope={display?.env ?? null}
                  sampleRate={sampleRate}
                  channels={
                    display?.source === "synthetic"
                      ? 1
                      : (selected?.channels ?? 1)
                  }
                  durationSeconds={durationSeconds}
                  layout={channelLayout}
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
            minDb={minDb}
            maxDb={maxDb}
            spectrumMode={spectrumMode}
            onSpectrumModeChange={setSpectrumMode}
            dbRangeIdx={dbRangeIdx}
            onDbRangeChange={setDbRangeIdx}
            channelLayout={channelLayout}
            onChannelLayoutChange={setChannelLayout}
          />
        }
      />

      {/* ---- 状态栏 ---- */}
      <footer className="status-bar">
        {activeTask ? (
          <>
            <span className="state warn">分析中</span>
            <span className="num" style={{ maxWidth: "28ch" }}>
              {activeTask.label}
            </span>
            <Button size="tiny" onClick={onCancelTask} title="Esc">
              取消
            </Button>
          </>
        ) : (
          <>
            <span
              className={liveDb ? "state ok" : "state"}
              title="在左侧「实时输入」中启动采集"
            >
              {liveDb ? "实时采集中" : "实时输入未启动"}
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
          </>
        )}
        <div className="spacer" />
        {error ? (
          <span className="err" title={`${error.title}\n${error.detail ?? ""}`}>
            {error.title}
          </span>
        ) : notice ? (
          <span className="sub">{notice}</span>
        ) : (
          display?.outcome === "succeeded" && (
            <span className="warn" title="未经验证的容器会在文件列表里标注">
              未验证格式会标注
            </span>
          )
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
  onSelect,
  busy,
  version,
  livePeak,
  liveRms,
  onLiveSnapshot,
}: {
  files: FileInfo[];
  selected: FileInfo | null;
  onPick: () => void;
  /** 点击历史文件重新分析，不必再过一遍文件对话框 */
  onSelect: (path: string) => void;
  busy: boolean;
  version: string;
  /** 实时 Peak dBFS；null = 未采集或静音 */
  livePeak: number | null;
  /** 实时 RMS dBFS；null = 未采集或静音 */
  liveRms: number | null;
  /** 实时快照回调（频谱 / 电平） */
  onLiveSnapshot: (snap: RealtimeSnapshot | null) => void;
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
                  还没有文件。可打开 WAV / FLAC / MP3 / AIFF / OGG；
                  WAV / FLAC 已用真实样本验证，其余容器会在名字旁标注。
                </span>
              </div>
            </li>
          ) : (
            files.map((f) => (
              <li key={f.path}>
                <button
                  type="button"
                  className={
                    "file-item" + (selected?.path === f.path ? " on" : "")
                  }
                  title={`${f.path}\n点击重新分析`}
                  disabled={busy}
                  onClick={() => onSelect(f.path)}
                >
                  <span className="file-name">{f.fileName}</span>
                  <span className="file-meta num">
                    <span className="meta-text">
                      {f.containerName} ·{" "}
                      {f.sampleRate.toLocaleString("en-US")} Hz ·{" "}
                      {f.channels === 1 ? "单声道" : `${f.channels} ch`}
                    </span>
                    {!f.verified && (
                      <span className="file-warn" title={f.verificationNote}>
                        未验证
                      </span>
                    )}
                  </span>
                </button>
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

        {/* M4：实时采集。放在导航之外，因为它是一个真正的控制面板，
            不是切换视图的入口。 */}
        <div className="nav-divider" />
        <RealtimePanel onSnapshot={onLiveSnapshot} />
        {livePeak !== null && (
          <LevelMeter
            peakDbfs={livePeak}
            rmsDbfs={liveRms}
            peakPerChannel={[]}
            minDb={MIN_DB}
            maxDb={MAX_DB}
            overFullScale={0}
            clippingRegions={0}
          />
        )}

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
  minDb,
  maxDb,
  spectrumMode,
  onSpectrumModeChange,
  dbRangeIdx,
  onDbRangeChange,
  channelLayout,
  onChannelLayoutChange,
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
  /** 当前 dB 量程（与中央画布一致） */
  minDb: number;
  maxDb: number;
  spectrumMode: SpectrumMode;
  onSpectrumModeChange: (m: SpectrumMode) => void;
  dbRangeIdx: number;
  onDbRangeChange: (idx: number) => void;
  channelLayout: ChannelLayout;
  onChannelLayoutChange: (l: ChannelLayout) => void;
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
        电平
      </div>
      <div className="readout">
        <LevelMeter
          peakDbfs={display?.peakDbfs ?? null}
          rmsDbfs={display?.rmsDbfs ?? null}
          peakPerChannel={display?.peakPerChannel ?? []}
          minDb={minDb}
          maxDb={maxDb}
          overFullScale={display?.overFullScaleSamples ?? 0}
          clippingRegions={display?.clippingRegions ?? 0}
        />
      </div>

      <div className="label" style={{ marginBottom: 5, marginTop: 12 }}>
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
        {/* 多声道文件的逐声道峰值：单声道时与上面的 Peak 完全重复，不显示 */}
        {display && display.peakPerChannel.length > 1 && (
          <div style={{ marginTop: 4, paddingTop: 4, borderTop: "1px solid var(--grid-minor)" }}>
            {display.peakPerChannel.map((p, i) => (
              <DataRow
                key={i}
                label={`CH${i + 1} Peak`}
                value={p == null ? "—（静音）" : `${p.toFixed(2)} dBFS`}
              />
            ))}
          </div>
        )}
        {/* 数据质量：这些计数为 0 也要显示，否则「有没有检查过」无法区分 */}
        <DataRow
          label="超量程样本"
          value={(display?.overFullScaleSamples ?? 0).toLocaleString("en-US")}
        />
        <DataRow
          label="平顶片段"
          value={(display?.clippingRegions ?? 0).toLocaleString("en-US")}
          title="启发式计数，不是模拟削波的证据"
        />
        <DataRow
          label="清理样本"
          value={(display?.sanitizedSamples ?? 0).toLocaleString("en-US")}
          title="NaN / Inf 被替换为 0 的数量"
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
        <Collapsible title="合成信号自检">
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
              {busy ? "计算中…" : "分析合成信号"}
            </Button>
          </div>
          <p className="note">
            合成 1 kHz 正弦，用于验证 IPC 通路与渲染；不读取也不修改任何文件。
            {selected
              ? "运行后会顶替画布上当前文件的结果，点左侧文件名可重新分析恢复。"
              : ""}
          </p>
        </Collapsible>

        {selected && (
          <Collapsible
            title="文件"
            meta={
              [
                // 合成信号分析会把画布换掉：此时这一节讲的是**已打开的文件**，
                // 不是屏幕上画的东西，必须点名，否则就是在标签上撒谎。
                display?.source === "synthetic" ? "未显示" : null,
                selected.verified ? null : "未验证",
              ]
                .filter(Boolean)
                .join(" · ") || undefined
            }
            defaultOpen
          >
            {display?.source === "synthetic" && (
              <p className="note warn-note">
                下列是已打开文件的属性；画布上显示的是合成信号，不是这份文件。
              </p>
            )}
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
          <FormField label="频谱形态">
            <Select
              value={spectrumMode}
              onValueChange={(v) =>
                onSpectrumModeChange(v === "line" ? "line" : "area")
              }
            >
              <option value="area">填充（面积）</option>
              <option value="line">曲线（声线）</option>
            </Select>
          </FormField>
          <FormField label="dB 量程">
            <Select
              value={String(dbRangeIdx)}
              onValueChange={(v) => onDbRangeChange(Number(v))}
            >
              {DB_RANGES.map((r, i) => (
                <option key={r.label} value={i}>
                  {r.label}
                </option>
              ))}
            </Select>
          </FormField>
          <FormField label="声道显示">
            <Select
              value={channelLayout}
              onValueChange={(v) =>
                onChannelLayoutChange(v === "overlay" ? "overlay" : "lanes")
              }
            >
              <option value="lanes">分道</option>
              <option value="overlay">叠加（对比）</option>
            </Select>
          </FormField>
          <DataRow label="当前量程" value={`${minDb} … ${maxDb} dBFS`} />
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
            真实文件与实时输入共用同一条计算链路。
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
  // 上游可能给出 NaN / Infinity（例如零点附近插值失败），
  // 这类值直接交给 toFixed 会抛错并让整个渲染树崩掉。
  if (!Number.isFinite(hz)) return "—";
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

/** 绝对时间：`m:ss.s`，用于声谱图视口范围。 */
function fmtClock(sec: number): string {
  if (!Number.isFinite(sec) || sec < 0) return "—";
  const m = Math.floor(sec / 60);
  const s = sec - m * 60;
  return `${m}:${s.toFixed(1).padStart(4, "0")}`;
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
  // 截断与损坏要分开说：截断通常意味着文件没下完/拷贝不完整，重新获取即可；
  // 混在一句话里，用户会以为是格式不支持而白折腾。
  if (raw.includes("truncated")) {
    return { title: `${action}：文件不完整（已截断）`, detail: raw };
  }
  if (raw.includes("corrupt")) {
    return { title: `${action}：文件已损坏`, detail: raw };
  }
  return { title: `${action}（详见悬停提示）`, detail: raw };
}
