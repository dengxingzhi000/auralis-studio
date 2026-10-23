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
  SyntheticRequest,
  TaskInfo,
} from "@/lib/contracts";
import type { DisplayData } from "@/lib/display";
import { describeError, type UiError } from "@/lib/errors";
import { fileNameOf, fmtHz } from "@/lib/format";
import {
  canLoadSpectrogramWindow,
  canStartAnalysis,
  canStartExport,
  exportLabel,
  openFileLabel,
  type OperationState,
} from "@/lib/operations";
import { pushHistory, type HistoryEntry } from "@/lib/task-history";
import { scrollToPane } from "@/lib/scroll";
import { readSession } from "@/lib/session";
import type { ChannelLayout, SpectrumMode } from "@/lib/view";
import { AnalysisCanvas } from "@/components/panels/AnalysisCanvas";
import { InspectorPanel } from "@/components/panels/InspectorPanel";
import { ResourcePanel } from "@/components/panels/ResourcePanel";
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
import { Button, Checkbox } from "@/components/ui/primitives";

/** 面板可见性的持久化键。 */
const VISIBILITY_KEY = "auralis.workspace.visibility.v1";

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

const DISPLAY_KEY = "auralis.display.v1";

/** 正在进行、可取消的文件分析。`null` = 没有在跑的文件任务。 */
interface ActiveTask {
  id: number;
  label: string;
}

export default function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [req, setReq] = useState<SyntheticRequest>(DEFAULTS);
  /**
   * 分析（文件或合成信号）与导出各自独立的忙碌位。
   *
   * 共用一个 `busy` 时，导出期间「打开文件」会谎称「分析中…」，
   * 拖放/搜索入口也会被「已有分析在进行中」拒绝。两者的闸门策略
   * 集中在 `lib/operations.ts`，这里只保存事实。
   */
  const [analyzing, setAnalyzing] = useState(false);
  const [exporting, setExporting] = useState(false);
  /** 一次渲染内的操作快照，供按钮文案与 disabled 使用。 */
  const ops: OperationState = { analyzing, exporting };
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
  /**
   * 是否已有一次分析（文件或合成信号）在跑。
   *
   * 侧栏按钮/列表会用 `analyzing` 禁用，但拖放与搜索面板是**第二、第三条
   * 入口**，它们不看渲染状态。没有这个同步闸门时，第二次分析会与第一次
   * 并行，两个 promise 各自 setDisplay，先结束的那个还会把后一个的
   * activeTask 清掉 —— 最终屏幕上可能是「高亮的文件 A + 显示的频谱 B」。
   *
   * 导出**不**写这个 ref：导出不修改分析状态，也不应阻止新分析。
   */
  const analyzingRef = useRef(false);
  /** 导出的同步闸门，防止保存对话框返回前重复触发。 */
  const exportingRef = useRef(false);

  // ---- 统一偏好（持久化）----
  //
  // 一份真值源：顶栏复选框、检查器的「显示」分区、偏好设置对话框
  // 全部读写这同一份状态。改一处三处一致 —— 「改了没反应」几乎总是
  // 因为两处各写各的。
  //
  // 初值来自 `readPrefs()`：它会先读新键，再从旧的
  // `auralis.display.v1` / `auralis.workspace.visibility.v1` 迁移，
  // 所以已有的用户设置不会因为换了存储位置而丢失。
  // `useState` 的惰性初始化只跑一次；写成 `useRef(readPrefs()).current`
  // 会在每次渲染都读一次 localStorage 并解析 JSON。
  const [initialPrefs] = useState(readPrefs);
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
  const [livePeaks, setLivePeaks] = useState<(number | null)[]>([]);
  const [liveRmsPerChannel, setLiveRmsPerChannel] = useState<
    (number | null)[]
  >([]);
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
    // 同一时刻只允许一个队列请求在路上：后端卡住时，500 ms 的定时器
    // 会不断叠加请求，占满 RPC 通道。
    let inFlight = false;
    const tick = () => {
      if (inFlight) return;
      inFlight = true;
      fetchTaskQueue()
        .then((t) => {
          if (alive) setTasks(t);
        })
        .catch(() => undefined)
        .finally(() => {
          inFlight = false;
        });
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
    // 拖放、搜索面板、侧栏三条入口可能同时到达。只让第一次通过，
    // 其余给出可见提示，而不是静默丢弃。只看分析自身：导出不阻止分析。
    if (!canStartAnalysis({ analyzing: analyzingRef.current, exporting: false })) {
      setNotice("已有分析在进行中，请等待完成或先取消");
      return;
    }
    analyzingRef.current = true;
    setError(null);
    setNotice(null);
    setAnalyzing(true);
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
        source: "file",
        db: a.spectrumBins > 0 ? a.spectrumDb : null,
        startHz: 0,
        stepHz: a.spectrumStepHz,
        env: a.waveformEnvelope,
        envMin: a.waveformMin,
        envPoints: a.waveformPoints,
        sampleRate: a.info.sampleRate,
        durationSeconds: a.decodedDurationSeconds,
        peakDbfs: a.peakDbfs,
        rmsDbfs: a.rmsDbfs,
        crestDb: a.crestDb,
        dc: a.dc,
        outcome: a.outcome,
        elapsedMs: a.__elapsedMeasuredMs,
        bytes:
          a.waveformEnvelope.byteLength +
          a.waveformMin.byteLength +
          a.spectrumDb.byteLength,
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
      recordAnalysis(id, fileNameOf(path), "failed", 0);
      setError(describeError(e, "打开文件失败"));
    } finally {
      analyzingRef.current = false;
      setActiveTask(null);
      setAnalyzing(false);
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
    // 分析进行中不导出（导出对象可能与屏幕内容对不上）；导出自身不可重入。
    if (
      !canStartExport({
        analyzing: analyzingRef.current,
        exporting: exportingRef.current,
      })
    ) {
      setNotice(
        analyzingRef.current ? "分析进行中，完成后再导出" : "正在导出，请等待完成",
      );
      return;
    }
    exportingRef.current = true;
    setExporting(true);
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
      exportingRef.current = false;
      setExporting(false);
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
    // 合成信号也是「分析」：它与文件分析都会替换 display，不能并行。
    if (!canStartAnalysis({ analyzing: analyzingRef.current, exporting: false })) {
      setNotice("已有分析在进行中，请等待完成或先取消");
      return;
    }
    analyzingRef.current = true;
    setAnalyzing(true);
    setError(null);
    try {
      const r = await analyzeSynthetic(req);
      setDisplay({
        source: "synthetic",
        db: r.spectrumDb,
        startHz: r.spectrumStartHz,
        stepHz: r.spectrumStepHz,
        env: r.waveformEnvelope,
        envMin: r.waveformMin,
        envPoints: r.waveformPoints,
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
        // 合成信号没有文件头，不存在「声明时长」这回事。
        truncated: false,
        declaredDurationSeconds: 0,
        decodedDurationSeconds: 0,
      });
      setSpectroStart(0);
      // 合成信号替换了画布数据：文件留下的频率视口可能超出合成信号的
      // 奈奎斯特，进入前先复位，避免画出一片空白。
      setViewRange(null);
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
      analyzingRef.current = false;
      setAnalyzing(false);
    }
  }, [req, selected]);

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
      {
        id: "action:prefs",
        group: "操作",
        label: "偏好设置…",
        hint: "Ctrl+,",
        keywords: "preferences 偏好 设置 settings",
        run: () => setDialog("prefs"),
      },
      {
        id: "action:help",
        group: "操作",
        label: "帮助与快捷键",
        hint: "F1",
        keywords: "help 帮助 快捷键 shortcut",
        run: () => setDialog("help"),
      },
      {
        id: "action:about",
        group: "操作",
        label: "关于 Auralis Studio",
        keywords: "about 关于 版本 version",
        run: () => setDialog("about"),
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
        // 导出不修改矩阵，只有分析会替换 display。
        !canLoadSpectrogramWindow({ analyzing, exporting })
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
    [selected, display, durationSeconds, spectrogramWindow, analyzing, exporting],
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
        <Button onClick={onOpenFile} disabled={!canStartAnalysis(ops)} title="Ctrl+O">
          {openFileLabel(ops)}
        </Button>
        <Button
          onClick={run}
          disabled={!canStartAnalysis(ops)}
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
          disabled={!canStartExport(ops) || !selected || display == null}
          title={
            selected
              ? `导出 ${selected.fileName} 的分析结果（CSV / JSON，含算法版本与参数）`
              : "先打开一个音频文件"
          }
        >
          {exportLabel(ops)}
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
            analyzing={analyzing}
            version={version}
            livePeak={livePeak}
            liveRms={liveRms}
            onLiveSnapshot={(snap) => {
              setLiveDb(snap ? snap.spectrumDb : null);
              setLiveStepHz(snap ? snap.binHz : 1);
              setLiveSampleRate(snap ? snap.sampleRate : null);
              setLivePeak(snap ? snap.peakDbfs : null);
              setLiveRms(snap ? snap.rmsDbfs : null);
              setLivePeaks(snap ? snap.peakDbfsPerChannel : []);
              setLiveRmsPerChannel(snap ? snap.rmsDbfsPerChannel : []);
            }}
            liveRunning={liveRunning}
            onLiveStatusChange={(running) => {
              setLiveRunning(running);
              // 文件与实时输入的奈奎斯特不同。进入采集时清掉文件留下的
              // 频率视口，否则 30–40 kHz 的文件缩放会映射到 44.1 kHz
              // 实时数据的绘图区之外，只剩一块空白。
              if (running) setViewRange(null);
            }}
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
          <AnalysisCanvas
            error={error}
            canRetry={lastPathRef.current != null}
            onRetry={onRetry}
            onDismissError={() => setError(null)}
            spectrumRef={spectrumRef}
            liveDb={liveDb}
            liveStepHz={liveStepHz}
            livePeak={livePeak}
            liveRms={liveRms}
            livePeakPerChannel={livePeaks}
            liveRmsPerChannel={liveRmsPerChannel}
            display={display}
            selected={selected}
            sampleRate={sampleRate}
            minDb={minDb}
            maxDb={maxDb}
            cursorHz={cursorHz}
            onHoverHz={setCursorHz}
            viewRange={viewRange}
            onViewRangeChange={setViewRange}
            spectrumMode={spectrumMode}
            channelLayout={channelLayout}
            spectroStart={spectroStart}
            spectrogramSeconds={spectrogramSeconds}
            spectrogramWindow={spectrogramWindow}
            durationSeconds={durationSeconds}
            showSpectroPaging={showSpectroPaging}
            canPage={canLoadSpectrogramWindow(ops)}
            spectroLoading={spectroLoading}
            onLoadSpectrogramWindow={(start) =>
              void loadSpectrogramWindow(start)
            }
            onPanSpectrogram={panSpectrogram}
          />
        }
        right={
          <InspectorPanel
            req={req}
            setReq={setReq}
            display={display}
            selected={selected}
            analyzing={analyzing}
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
