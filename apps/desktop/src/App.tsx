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
  fetchTaskQueue,
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
  TaskInfo,
} from "@/lib/contracts";
import type { DisplayData } from "@/lib/display";
import { pushHistory, type HistoryEntry } from "@/lib/task-history";
import { LevelMeter } from "@/components/audio-viz/LevelMeter";
import { MetricsPanel } from "@/components/audio-viz/MetricsPanel";
import { RealtimePanel } from "@/components/audio-viz/RealtimePanel";
import { SpectrumView } from "@/components/audio-viz/SpectrumView";
import { SpectrogramView } from "@/components/audio-viz/SpectrogramView";
import { TaskQueuePanel } from "@/components/audio-viz/TaskQueuePanel";
import { WaveformView } from "@/components/audio-viz/WaveformView";
import { readStoredJson, writeStoredJson } from "@/lib/persist";
import {
  clearPrefs,
  clearSession,
  DB_RANGES,
  readPrefs,
  resetLayout,
  SESSION_KEY,
  writePrefs,
  type Prefs,
} from "@/lib/prefs";
import { AboutDialog } from "@/components/ui/AboutDialog";
import { HelpDialog } from "@/components/ui/HelpDialog";
import { PreferencesDialog } from "@/components/ui/PreferencesDialog";
import { SearchPalette, type PaletteItem } from "@/components/ui/SearchPalette";
import {
  Button,
  Checkbox,
  Collapsible,
  DataRow,
  ErrorBar,
  FormField,
  IconButton,
  MetricRow,
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

/**
 * 把某张卡片滚进视野。
 *
 * 中央画布是可纵向滚动的（窗口变矮时图表不会被压成条带），
 * 侧栏的导航项需要能把对应的卡片带进来。
 *
 * 用**瞬时**滚动而不是平滑滚动：平滑滚动是一次动画，页面不可见时
 * （窗口最小化、被完全遮挡）没有帧来推进它，滚动就停在原地 ——
 * 导航项会变成「点了没反应」。瞬时滚动没有这个依赖。
 *
 * 立刻滚一次，下一帧再滚一次：刚打开文件时卡片可能还没布局完，
 * 只滚一次会滚不到位。
 */
function scrollToPane(id: string): void {
  const scroll = () => {
    document
      .getElementById(id)
      ?.scrollIntoView({ behavior: "instant", block: "nearest" });
  };
  scroll();
  window.requestAnimationFrame(scroll);
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

/** 频谱显示模式：填充（面积）或仅曲线。 */
type SpectrumMode = "area" | "line";

/** 多声道波形的排布：分道 / 叠加。 */
type ChannelLayout = "lanes" | "overlay";

const DISPLAY_KEY = "auralis.display.v1";

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
  const spectrumRef = useRef<HTMLDivElement>(null);
  /** 任务号由前端分配：见 AppState::try_register_token 的说明 */
  const taskSeq = useRef(0);
  const [activeTask, setActiveTask] = useState<ActiveTask | null>(null);
  /** 上一个成功 probe 的路径，供「重试」使用 */
  const lastPathRef = useRef<string | null>(null);

  // ---- 统一偏好（持久化）----
  //
  // 一份真值源：顶栏复选框、检查器的「显示」分区、偏好设置对话框
  // 全部读写这同一份状态。改一处三处一致 —— 「改了没反应」几乎总是
  // 因为两处各写各的。
  //
  // 初值来自 `readPrefs()`：它会先读新键，再从旧的
  // `auralis.display.v1` / `auralis.workspace.visibility.v1` 迁移，
  // 所以已有的用户设置不会因为换了存储位置而丢失。
  const initialPrefs = useRef(readPrefs()).current;
  const [spectrumMode, setSpectrumMode] = useState<SpectrumMode>(
    initialPrefs.spectrumMode,
  );
  const [dbRangeIdx, setDbRangeIdx] = useState(initialPrefs.dbRange);
  const [channelLayout, setChannelLayout] = useState<ChannelLayout>(
    initialPrefs.channelLayout,
  );
  // 面板可见性也持久化：一上来就隐藏资源栏的人，不希望每次启动都重新勾一遍
  const [showLeft, setShowLeft] = useState(initialPrefs.showLeft);
  const [showRight, setShowRight] = useState(initialPrefs.showRight);
  /** 文件分析的波形包络点数 */
  const [envelopePoints, setEnvelopePoints] = useState(
    initialPrefs.envelopePoints,
  );
  /** 启动时是否自动恢复上次打开的文件 */
  const [restoreSession, setRestoreSession] = useState(
    initialPrefs.restoreSession,
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

  // ---- 后台任务队列（M3）----
  /** 运行中的任务；来自 Rust 的任务注册表 */
  const [tasks, setTasks] = useState<TaskInfo[]>([]);
  /** 最近完成的文件分析；Rust 侧任务结束即注销，结局只有发起方知道 */
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  /** 实时采集是否正在运行（用于导航项上的状态标记） */
  const [liveRunning, setLiveRunning] = useState(false);
  /** 全局搜索面板是否打开 */
  const [searchOpen, setSearchOpen] = useState(false);
  /** 打开中的模态对话框；`null` = 都没开 */
  const [dialog, setDialog] = useState<null | "prefs" | "help" | "about">(null);
  /** 布局重置计数：递增会重建 Workspace，让它重新读一遍面板宽度 */
  const [layoutEpoch, setLayoutEpoch] = useState(0);

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

  // 统一偏好存储键。状态是真值源，这个 effect 只是把它落盘。
  useEffect(() => {
    writePrefs({
      spectrumMode,
      dbRange: dbRangeIdx,
      channelLayout,
      showLeft,
      showRight,
      envelopePoints,
      restoreSession,
    });
  }, [
    spectrumMode,
    dbRangeIdx,
    channelLayout,
    showLeft,
    showRight,
    envelopePoints,
    restoreSession,
  ]);

  /** 把一份偏好应用到全部状态。 */
  const applyPrefs = useCallback((p: Prefs) => {
    setSpectrumMode(p.spectrumMode);
    setDbRangeIdx(p.dbRange);
    setChannelLayout(p.channelLayout);
    setShowLeft(p.showLeft);
    setShowRight(p.showRight);
    setEnvelopePoints(p.envelopePoints);
    setRestoreSession(p.restoreSession);
  }, []);

  /** 当前偏好的快照，供偏好设置对话框渲染。 */
  const prefs = useMemo<Prefs>(
    () => ({
      spectrumMode,
      dbRange: dbRangeIdx,
      channelLayout,
      showLeft,
      showRight,
      envelopePoints,
      restoreSession,
    }),
    [
      spectrumMode,
      dbRangeIdx,
      channelLayout,
      showLeft,
      showRight,
      envelopePoints,
      restoreSession,
    ],
  );

  /**
   * 重置某一组本地设置。
   *
   * 删除对应的存储键，并把运行中的界面同步回默认值 —— 只删键不刷新
   * 界面的话，用户会看到「按钮按了没反应」。
   */
  const onResetPrefs = useCallback(
    (kind: "prefs" | "layout" | "session") => {
      if (kind === "prefs") {
        clearPrefs();
        applyPrefs(readPrefs());
        setNotice("显示与分析偏好已重置");
      } else if (kind === "layout") {
        resetLayout();
        // 侧栏宽度由 Workspace 组件自持；重建它才会重新读一次存储。
        setLayoutEpoch((n) => n + 1);
        applyPrefs({ ...readPrefs(), showLeft: true, showRight: true });
        setNotice("面板布局已重置");
      } else {
        clearSession();
        setNotice("会话记录已清除，下次启动不再自动恢复文件");
      }
    },
    [applyPrefs],
  );

  useEffect(() => {
    fetchAppInfo()
      .then(setInfo)
      .catch((e) => setError(describeError(e, "无法连接 Rust 后端")));
  }, []);

  /**
   * 轮询后台任务队列。
   *
   * `task_queue` 是**快命令**：在 RPC 读取线程里就地执行，只读一张
   * 哈希表，不会排在耗时的文件分析后面。500 ms 一次，开销可忽略。
   *
   * 失败时静默忽略：拿不到队列不代表分析失败，不该因此弹错误条。
   */
  useEffect(() => {
    let alive = true;
    const tick = () => {
      fetchTaskQueue()
        .then((t) => {
          if (alive) setTasks(t);
        })
        .catch(() => undefined);
    };
    tick();
    const id = window.setInterval(tick, 500);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, []);

  /** 取下一个任务号。毫秒时间戳 ×1000 + 序号，避开后端 0..N 的分配区间。 */
  const nextTaskId = (): number => {
    taskSeq.current += 1;
    return Date.now() * 1000 + taskSeq.current;
  };

  /**
   * 把一次文件分析的结局记进「最近完成」。
   *
   * Rust 侧的任务注册表只暴露**运行中**的任务（结束即注销），所以
   * 「完成 / 已取消 / 失败」这个结局只有发起方知道 —— 这里就是那个
   * 发起方。
   *
   * 只收文件分析：声谱图视口请求在自动平移时会连发，进历史会把
   * 用户真正关心的分析挤掉。
   */
  const recordAnalysis = useCallback(
    (
      taskId: number,
      label: string,
      outcome: "succeeded" | "cancelled" | "failed",
      elapsedMs: number,
    ) => {
      setHistory((h) =>
        pushHistory(h, {
          id: `analyze:${taskId}`,
          kind: "analyze",
          label,
          outcome,
          elapsedMs,
          endedAt: Date.now(),
        }),
      );
    },
    [],
  );

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
      const a = await analyzeFile(fi.path, envelopePoints, id);

      // 取消不是失败：保留上一次的数据，只在状态栏说明
      if (a.outcome === "cancelled") {
        recordAnalysis(id, fi.fileName, "cancelled", a.elapsedMs);
        setNotice(`已取消分析：${fi.fileName}`);
        return;
      }

      recordAnalysis(
        id,
        fi.fileName,
        // Rust 的 outcome 还有 "panicked"（任务级 panic 隔离），
        // 对用户来说同样是「没成功」。
        a.outcome === "succeeded" ? "succeeded" : "failed",
        a.__elapsedMeasuredMs,
      );

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
        spectrogramStartSeconds: a.spectrogramStartSeconds,
        peakPerChannel: a.peakDbfsPerChannel,
        overFullScaleSamples: a.overFullScaleSamples,
        clippingRegions: a.clippingRegions,
        sanitizedSamples: a.sanitizedSamples,
        truncated: a.truncated,
        declaredDurationSeconds: a.declaredDurationSeconds,
        decodedDurationSeconds: a.decodedDurationSeconds,
      });
      setSpectroStart(a.spectrogramStartSeconds);
      // 新文件的频率轴可能与旧的视口不匹配（比如 48 kHz 文件缩到
      // 20 kHz，再打开 44.1 kHz 文件）—— 保留视口会裁出一片空白。
      setViewRange(null);
      // 新文件的结果替换了旧矩阵；旧的视口请求已经无意义。
      if (spectroTaskRef.current != null) {
        void cancelTask(spectroTaskRef.current).catch(() => undefined);
        spectroTaskRef.current = null;
        setSpectroLoading(false);
      }

      // 截断的提示优先级最高：恢复会话的提示不能把它盖掉，
      // 否则用户看到「已恢复上次打开的文件」，却不知道文件其实不完整。
      if (a.truncated) {
        setNotice(
          `文件不完整：只分析了前 ${a.decodedDurationSeconds.toFixed(2)} 秒` +
            `（文件头声明 ${a.declaredDurationSeconds.toFixed(2)} 秒）`,
        );
      } else if (restore === "crashed") {
        setNotice(`上次会话未正常结束，已恢复：${fi.fileName}`);
      } else if (restore === "clean") {
        setNotice(`已恢复上次打开的文件：${fi.fileName}`);
      }
    } catch (e) {
      recordAnalysis(id, fileNameOf(path), "failed", 0);
      setError(describeError(e, "打开文件失败"));
    } finally {
      setActiveTask(null);
      setBusy(false);
    }
  }, [recordAnalysis, envelopePoints]);

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

    // 用户可以关掉「启动时恢复上次打开的文件」。
    if (!restoreSession) return;

    const recovered = params.get("recovered") === "1";
    void analyzePath(session.path, recovered ? "crashed" : "clean");
  }, [analyzePath, restoreSession]);

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
   * 取消任务队列里的一个任务。
   *
   * 「已请求取消」不等于「已取消」：真正的结局要等那个任务的 promise
   * 结算。所以这里只说明请求已发出，状态由「最近完成」里的记录给出。
   */
  const onCancelQueueTask = useCallback(async (taskId: number) => {
    setError(null);
    try {
      const found = await cancelTask(taskId);
      setNotice(found ? "已请求取消任务" : "任务已经结束");
    } catch (e) {
      setError(describeError(e, "取消失败"));
    }
  }, []);

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
      const out = await exportAnalysis(
        selected.path,
        destination,
        format,
        envelopePoints,
      );
      setNotice(
        `已导出 ${out.format.toUpperCase()} · ${(out.bytesWritten / 1024).toFixed(1)} KiB → ${out.destination}`,
      );
    } catch (e) {
      setError(describeError(e, "导出失败"));
    } finally {
      setBusy(false);
    }
  }, [selected, envelopePoints]);

  /**
   * 把频率视口对准某个检测到的峰值。
   *
   * 半宽取 `max(250 Hz, 6 × 声谱图 bin 宽)`：用绝对 Hz 而不是「几个
   * bin」，因为 bin 宽随采样率与 bin 数变化。6 个 bin 保证放大后声谱图
   * 里仍有若干行可看 —— 只剩一两行的话，那条带子说明不了任何问题。
   *
   * 这里只做**坐标换算**（Hz ↔ 视口），不做任何音频计算。
   */
  const focusPeak = useCallback(
    (p: PeakInfo) => {
      const nyq = (display?.sampleRate ?? req.sampleRate) / 2;
      const binHz = nyq / Math.max(1, display?.spectrogramBins ?? 512);
      const half = Math.max(250, 6 * binHz);
      const center = p.binFrequencyHz > 0 ? p.binFrequencyHz : p.frequencyHz;
      const lo = Math.max(0, center - half);
      const hi = Math.min(nyq, center + half);
      if (hi - lo < 1) return;
      setViewRange([lo, hi]);
      setCursorHz(p.frequencyHz);
      scrollToPane("pane-spectrum");
    },
    [display, req.sampleRate],
  );

  // ---- 键盘：Ctrl/Cmd+O 打开，Esc 取消，Ctrl/Cmd+R 重试，Ctrl/Cmd+K 搜索 ----
  //
  // 有模态打开时键盘归模态：否则在对话框里按 Esc 会同时「关闭对话框」
  // 和「取消正在进行的分析」；在搜索面板里按 Ctrl+O 会在面板上再弹一个
  // 文件选择器。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      const key = e.key.toLowerCase();

      if (mod && (key === "k" || key === "f")) {
        e.preventDefault();
        if (dialog === null) setSearchOpen(true);
        return;
      }

      if (dialog !== null || searchOpen) return;

      if (mod && key === "o") {
        e.preventDefault();
        void onOpenFile();
      } else if (mod && key === ",") {
        e.preventDefault();
        setDialog("prefs");
      } else if (e.key === "F1") {
        e.preventDefault();
        setDialog("help");
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
  }, [
    activeTask,
    dialog,
    onCancelTask,
    onOpenFile,
    onRetry,
    searchOpen,
  ]);

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
        spectrogramStartSeconds: 0,
        peakPerChannel: [],
        // 合成信号是纯正弦，超量程/削波/清理计数都为 0：
        // 写 0 而不是省略，界面才能一致性地区分「没有」与「未知」
        overFullScaleSamples: r.metrics.overFullScaleSamples,
        clippingRegions: r.metrics.clippingRegions,
        sanitizedSamples: r.metrics.sanitizedSamples,
        // 合成信号没有文件头，不存在「声明时长」这回事。
        truncated: false,
        declaredDurationSeconds: 0,
        decodedDurationSeconds: 0,
      });
      setSpectroStart(0);
      setViewRange(null);
    } catch (e) {
      setError(describeError(e, "合成信号分析失败"));
    } finally {
      setBusy(false);
    }
  }, [req]);

  /**
   * 搜索面板的条目。
   *
   * 三组：文件（重新分析）、峰值（把频率视口对准它）、操作。
   * 全部是**已算好的东西**或纯界面动作，不触发新的音频计算。
   */
  const paletteItems = useMemo<PaletteItem[]>(() => {
    const items: PaletteItem[] = [];

    for (const f of files) {
      items.push({
        id: `file:${f.path}`,
        group: "文件",
        label: f.fileName,
        hint: `${f.containerName} · ${f.sampleRate.toLocaleString("en-US")} Hz`,
        keywords: f.path,
        run: () => void analyzePath(f.path),
      });
    }

    for (const [i, p] of (display?.peaks ?? []).entries()) {
      items.push({
        id: `peak:${i}`,
        group: "峰值",
        label: `${fmtHz(p.frequencyHz)} Hz`,
        hint: `${p.amplitudeDbfs.toFixed(2)} dBFS${p.estimated ? "（估算）" : ""}`,
        keywords: "peak 峰值 频率 frequency",
        run: () => focusPeak(p),
      });
    }

    items.push(
      {
        id: "action:open",
        group: "操作",
        label: "打开文件…",
        hint: "Ctrl+O",
        keywords: "open 打开 文件",
        run: () => void onOpenFile(),
      },
      {
        id: "action:export",
        group: "操作",
        label: "导出分析结果…",
        hint: "CSV / JSON",
        keywords: "export 导出 保存",
        run: () => void onExport(),
      },
      {
        id: "action:synthetic",
        group: "操作",
        label: "生成并分析合成信号",
        keywords: "synthetic 合成 正弦",
        run: () => void run(),
      },
      {
        id: "action:reset-zoom",
        group: "操作",
        label: "复位频率缩放",
        hint: "全带宽",
        keywords: "reset zoom 复位 缩放",
        run: () => setViewRange(null),
      },
      {
        id: "action:toggle-left",
        group: "操作",
        label: "显示 / 隐藏资源栏",
        keywords: "toggle 资源栏 侧栏 panel",
        run: () => setShowLeft((v) => !v),
      },
      {
        id: "action:toggle-right",
        group: "操作",
        label: "显示 / 隐藏检查器",
        keywords: "toggle 检查器 侧栏 panel",
        run: () => setShowRight((v) => !v),
      },
    );

    return items;
  }, [
    files,
    display?.peaks,
    focusPeak,
    onOpenFile,
    onExport,
    run,
  ]);

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
      if (!selected || !display || durationSeconds <= 0 || busy) return;

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
        <Button onClick={run} disabled={busy}>
          合成信号
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
              ? "导出 CSV / JSON（含算法版本与参数）"
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
        key={layoutEpoch}
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
            liveRunning={liveRunning}
            onLiveStatusChange={setLiveRunning}
            onSearch={() => setSearchOpen(true)}
            onOpenPrefs={() => setDialog("prefs")}
            onOpenHelp={() => setDialog("help")}
            onOpenAbout={() => setDialog("about")}
            tasks={tasks}
            history={history}
            onCancelTask={onCancelQueueTask}
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
            <section className="pane" id="pane-spectrum" style={{ flex: "1.35" }}>
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

            <section className="pane" id="pane-spectrogram" style={{ flex: 1 }}>
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
                {viewRange && (
                  <>
                    <span className="pane-meta num" title="当前频率视口">
                      {fmtHz(viewRange[0])}–{fmtHz(viewRange[1])} Hz
                    </span>
                    <Button
                      size="tiny"
                      onClick={() => setViewRange(null)}
                      title="恢复全带宽频率视口"
                    >
                      频率复位
                    </Button>
                  </>
                )}
                {selected && durationSeconds > 0 && (
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
                  // 声谱图画的是**文件**的矩阵，频率轴必须用文件自己的
                  // 采样率。实时采集开着时 `sampleRate` 会切成设备的采样率，
                  // 那样频率刻度会整体错位 —— 而画面看上去完全正常。
                  nyquistHz={(display?.sampleRate ?? sampleRate) / 2}
                  durationSeconds={spectrogramSeconds}
                  startSeconds={spectroStart}
                  onPan={panSpectrogram}
                  viewRange={viewRange}
                  onViewRangeChange={setViewRange}
                />
              </div>
              <div className="pane-note">
                滚轮 / 拖拽平移时间
                <span className="sep">·</span>
                Shift + 滚轮缩放频率 · Shift + 拖拽框选频段
                <span className="sep">·</span>
                双击回到文件开头
              </div>
            </section>

            <div className="splitter splitter-h" />

            <section className="pane" id="pane-waveform" style={{ flex: 1 }}>
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
                  layout={channelLayout}
                />
              </div>
            </section>

            <div className="splitter splitter-h" />

            <section className="pane pane-metrics" id="pane-metrics">
              <div className="pane-head">
                <span className="pane-title">电平 / 指标</span>
                <span className="pane-meta">
                  {liveDb ? "实时采集" : display ? "已分析" : "等待数据"}
                  {display && (
                    <>
                      <span className="sep">·</span>
                      <span className="num">
                        {display.elapsedMs.toFixed(1)} ms
                      </span>
                    </>
                  )}
                </span>
              </div>
              <div className="pane-body pane-scroll">
                <MetricsPanel
                  display={display}
                  live={
                    liveDb
                      ? { peakDbfs: livePeak, rmsDbfs: liveRms }
                      : null
                  }
                  minDb={minDb}
                  maxDb={maxDb}
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

      {/* ---- 全局搜索 ---- */}
      <SearchPalette
        open={searchOpen}
        onClose={() => setSearchOpen(false)}
        items={paletteItems}
      />

      {/* ---- 对话框 ---- */}
      <PreferencesDialog
        open={dialog === "prefs"}
        onClose={() => setDialog(null)}
        prefs={prefs}
        onChange={applyPrefs}
        onReset={onResetPrefs}
      />
      <HelpDialog open={dialog === "help"} onClose={() => setDialog(null)} />
      <AboutDialog
        open={dialog === "about"}
        onClose={() => setDialog(null)}
        info={info}
      />
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
  tasks,
  history,
  onCancelTask: onCancelQueueTask,
  liveRunning,
  onLiveStatusChange,
  onSearch,
  onOpenPrefs,
  onOpenHelp,
  onOpenAbout,
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
  /** 运行中的后台任务 */
  tasks: TaskInfo[];
  /** 最近完成的文件分析 */
  history: HistoryEntry[];
  /** 取消一个运行中的任务 */
  onCancelTask: (taskId: number) => void;
  /** 实时采集是否在运行 */
  liveRunning: boolean;
  /** 实时采集状态变化回调 */
  onLiveStatusChange: (running: boolean) => void;
  /** 打开全局搜索 */
  onSearch: () => void;
  /** 打开偏好设置 */
  onOpenPrefs: () => void;
  /** 打开帮助 */
  onOpenHelp: () => void;
  /** 打开关于 */
  onOpenAbout: () => void;
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
                  仅 WAV 已用真实样本验证，其余容器会在名字旁标注。
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
          <NavItem
            icon={<IconWaveform />}
            active
            onClick={() => scrollToPane("pane-waveform")}
          >
            波形
          </NavItem>
          <NavItem
            icon={<IconSpectrum />}
            active
            onClick={() => scrollToPane("pane-spectrum")}
          >
            频谱
          </NavItem>
          <NavItem
            icon={<IconSpectrogram />}
            onClick={() => scrollToPane("pane-spectrogram")}
            title="声谱图：Shift + 滚轮缩放频率，Shift + 拖拽框选频段"
          >
            声谱图缩放
          </NavItem>
          <NavItem
            icon={<IconGauge />}
            onClick={() => scrollToPane("pane-metrics")}
            title="电平、信号指标与数据质量"
          >
            电平表 / 指标
          </NavItem>
          <NavItem
            icon={<IconTasks />}
            onClick={() => scrollToPane("task-queue-panel")}
            title="运行中的任务与最近完成的分析"
            tag={tasks.length > 0 ? String(tasks.length) : undefined}
          >
            后台任务队列
          </NavItem>
          <NavItem
            icon={<IconDevice />}
            onClick={() => scrollToPane("realtime-panel")}
            title="枚举输入设备并启动实时采集"
            tag={liveRunning ? "采集中" : undefined}
          >
            实时输入
          </NavItem>
        </NavGroup>

        {/* M4：实时采集。放在导航之外，因为它是一个真正的控制面板，
            不是切换视图的入口。 */}
        <div className="nav-divider" />
        <RealtimePanel
          onSnapshot={onLiveSnapshot}
          onStatusChange={onLiveStatusChange}
        />
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

        {/* M3：后台任务队列。与实时面板一样是控制面板，不是视图入口。 */}
        <div className="nav-divider" />
        <div id="task-queue-panel">
          <TaskQueuePanel
            running={tasks}
            history={history}
            onCancel={onCancelQueueTask}
          />
        </div>

        <NavGroup label="工程">
          <NavItem
            icon={<IconSettings />}
            onClick={onOpenPrefs}
            title="显示、布局与分析参数（Ctrl+,）"
          >
            偏好设置
          </NavItem>
        </NavGroup>
      </div>

      <div className="sidebar-foot">
        <ul className="nav">
          <NavItem
            icon={<IconSearch />}
            onClick={onSearch}
            title="搜索文件、峰值或操作（Ctrl+K）"
          >
            搜索
          </NavItem>
          <NavItem
            icon={<IconHelp />}
            onClick={onOpenHelp}
            title="快捷键、界面导览与能力边界（F1）"
          >
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
          <IconButton label="关于与版本信息" onClick={onOpenAbout}>
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
  const set = <K extends keyof SyntheticRequest>(    k: K,
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

/**
 * 取路径的文件名部分；没有分隔符时原样返回。
 *
 * 只用于显示标签，不是解析路径 —— 真正的路径处理由 Rust 侧负责。
 */
function fileNameOf(path: string): string {
  const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return cut >= 0 ? path.slice(cut + 1) : path;
}

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
