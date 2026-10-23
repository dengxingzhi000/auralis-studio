/**
 * 引擎 IPC 封装。
 *
 * 薄薄一层，只负责：
 *   1. 用 camelCase 调 Rust 侧命令
 *   2. 把二进制字段还原成 Float32Array
 *
 * 容器由 `./bridge` 提供（当前是 Electron preload）：这里**不再**直接
 * import 任何容器 SDK，命令名与参数与迁移前保持一致。
 *
 * 刻意不引入 TanStack Query 之类的数据层——本项目的"异步任务"是
 * 本地音频分析，不是 Web 请求。套一层 HTTP 语义的缓存层反而
 * 会误导状态管理（技术选型文档里也明确提到这一点）。
 */

import { bridge } from "./bridge";

import {
  decodeF32,
  type AppInfo,
  type CaptureStatus,
  type DeviceInfo,
  type FileAnalysis,
  type FileInfo,
  type RealtimeSnapshot,
  type SpectrogramViewData,
  type SyntheticAnalysis,
  type SyntheticRequest,
  type TaskInfo,
} from "./contracts";

/**
 * Rust `Vec<u8>` 在 WebView2 上实测到达的是普通数字数组。
 * 这里如实描述，避免用 `Uint8Array` 做类型断言而掩盖运行时差异
 * （那个断言正是「图表全空」这个 bug 的成因）。
 */
type RawBytes = number[] | Uint8Array;

/**
 * 调用 Rust 引擎命令。
 *
 * 桥接对象在模块加载时不取：浏览器（无 preload）里打开 dev server 时，
 * `window.auralis` 不存在，只有在真正调用时才应该报错。
 */
function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return bridge().invoke<T>(cmd, args);
}

/** 取应用与算法版本。 */
export async function fetchAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>("app_info");
}

/**
 * 分析合成信号，并做一次 IPC 往返压测。
 *
 * 返回的 `roundTripMicros` 是**端到端**耗时（前端发出到收到），
 * 与 Rust 侧的 `computeMicros`（纯计算 + 编码）不同。
 * 两者之差即 IPC 开销，是 ADR-018 的关键观测指标。
 */
export interface TimedAnalysis extends SyntheticAnalysis {
  /** 前端观察到的完整往返耗时，微秒 */
  roundTripMicros: number;
}

export async function analyzeSynthetic(
  req: SyntheticRequest,
): Promise<TimedAnalysis> {
  const t0 = performance.now();
  const raw = await invoke<
    Omit<SyntheticAnalysis, "spectrumDb" | "waveformEnvelope"> & {
      spectrumDb: RawBytes;
      waveformEnvelope: RawBytes;
    }
  >("analyze_synthetic_signal", {
    frequencyHz: req.frequencyHz,
    amplitude: req.amplitude,
    sampleRate: req.sampleRate,
    fftSize: req.fftSize,
    targetBins: req.targetBins,
    envelopePoints: req.envelopePoints,
  });
  const t1 = performance.now();

  return {
    ...raw,
    spectrumDb: decodeF32(raw.spectrumDb),
    waveformEnvelope: decodeF32(raw.waveformEnvelope),
    roundTripMicros: (t1 - t0) * 1000,
  };
}

/* ============================================================
 * M1: real files
 * ============================================================ */

/**
 * Extensions offered in the open dialog and accepted on drag-and-drop.
 *
 * This is a *filter*, not a promise: the Rust side still reports per-container
 * verification status, and the UI labels unverified containers. Duplicates are
 * harmless but confusing, so the list is deduplicated in `isAudioPath`.
 */
export const AUDIO_EXTENSIONS = ["wav", "flac", "mp3", "aiff", "aif", "ogg"];

/** True when `path` looks like an audio file this build claims to open. */
export function isAudioPath(path: string): boolean {
  const dot = path.lastIndexOf(".");
  if (dot < 0) return false;
  const ext = path.slice(dot + 1).toLowerCase();
  return AUDIO_EXTENSIONS.includes(ext);
}

/**
 * Show the native file picker.
 *
 * Returns null when the user cancels - cancellation is not an error,
 * so callers must not surface it as one.
 */
export async function pickAudioFile(): Promise<string | null> {
  const picked = await bridge().openFile({
    multiple: false,
    directory: false,
    filters: [{ name: "Audio", extensions: AUDIO_EXTENSIONS }],
  });
  if (typeof picked === "string") return picked;
  if (Array.isArray(picked)) return picked[0] ?? null;
  return null;
}

/** Probe file metadata. Reads the header only, does not decode. */
export async function probeFile(path: string): Promise<FileInfo> {
  return invoke<FileInfo>("probe_file", { path });
}

/**
 * Analyse a real file: probe + block decode + metrics + waveform envelope.
 *
 * `taskId` is allocated by the caller and passed through to Rust so the same
 * id can be handed to `cancelTask` while the analysis is still running —
 * without it, a task id is only known after the work is already over.
 *
 * Returns the analysis with the measured end-to-end round-trip attached,
 * which is the number M0.5 left unmeasured.
 */
export interface TimedFileAnalysis extends FileAnalysis {
  /** measured client-observed round trip, milliseconds */
  __elapsedMeasuredMs: number;
}

export async function analyzeFile(
  path: string,
  envelopePoints?: number,
  taskId?: number,
): Promise<TimedFileAnalysis> {
  const t0 = performance.now();
  const raw = await invoke<
    Omit<FileAnalysis, "waveformEnvelope" | "spectrumDb" | "spectrogramDb"> & {
      waveformEnvelope: RawBytes;
      spectrumDb: RawBytes;
      spectrogramDb: RawBytes;
    }
  >("analyze_file", {
    path,
    envelopePoints: envelopePoints ?? 2048,
    ...(taskId == null ? {} : { taskId }),
  });
  return {
    ...raw,
    waveformEnvelope: decodeF32(raw.waveformEnvelope),
    spectrumDb: decodeF32(raw.spectrumDb),
    spectrogramDb: decodeF32(raw.spectrogramDb),
    __elapsedMeasuredMs: performance.now() - t0,
  };
}

/**
 * 取一个时间窗口的声谱图（M4 视口分块）。
 *
 * Rust 侧只解码并计算请求的窗口，内存与窗口长度成正比，与文件长度无关。
 * 返回的 `startSeconds` 可能因 hop 网格对齐而略早于请求值，时间轴应使用它。
 */
export async function fetchSpectrogramView(
  path: string,
  startSeconds: number,
  windowSeconds: number,
  taskId?: number,
): Promise<SpectrogramViewData> {
  const raw = await invoke<
    Omit<SpectrogramViewData, "spectrogramDb"> & { spectrogramDb: RawBytes }
  >("spectrogram_view", {
    path,
    startSeconds,
    windowSeconds,
    ...(taskId == null ? {} : { taskId }),
  });
  return { ...raw, spectrogramDb: decodeF32(raw.spectrogramDb) };
}

/** Cancel a running task. Returns false when the task id is unknown. */
export async function cancelTask(taskId: number): Promise<boolean> {
  return invoke<boolean>("cancel_task", { taskId });
}

/**
 * 当前正在运行的后台任务。
 *
 * 这是快速命令：只读取任务注册表，不会排在耗时的文件分析后面。
 * 已结束的任务不在列表里，历史由前端会话记录补充。
 */
export async function fetchTaskQueue(): Promise<TaskInfo[]> {
  return invoke<TaskInfo[]>("task_queue");
}

/* ============================================================
 * M4: realtime capture
 * ============================================================ */

/** List input devices. Throws when the host has no audio backend. */
export async function listInputDevices(): Promise<DeviceInfo[]> {
  return invoke<DeviceInfo[]>("input_devices");
}

/** Start realtime capture from a device. */
export async function startRealtime(
  deviceId: string,
  sampleRate?: number,
  channels?: number,
): Promise<CaptureStatus> {
  return invoke<CaptureStatus>("start_realtime", {
    deviceId,
    sampleRate,
    channels,
  });
}

/** Stop realtime capture. */
export async function stopRealtime(): Promise<CaptureStatus> {
  return invoke<CaptureStatus>("stop_realtime");
}

/** Current capture status (dropped frames, overwrites, ...). */
export async function captureStatus(): Promise<CaptureStatus> {
  return invoke<CaptureStatus>("capture_status_cmd");
}

/** Latest realtime analysis snapshot. */
export async function realtimeSnapshot(): Promise<RealtimeSnapshot> {
  const raw = await invoke<
    Omit<RealtimeSnapshot, "spectrumDb"> & { spectrumDb: RawBytes }
  >("realtime_snapshot");
  return { ...raw, spectrumDb: decodeF32(raw.spectrumDb) };
}

/* ============================================================
 * M5: export
 * ============================================================ */

/** Result of writing an export file. */
export interface ExportOutcome {
  destination: string;
  /** Normalised format: "csv" or "json". */
  format: string;
  bytesWritten: number;
  elapsedMs: number;
}

/**
 * Ask the user where to save an export.
 *
 * Returns null when cancelled — cancellation is not an error. The format is
 * derived from the chosen extension by the Rust side, so a mismatch between
 * `.json` and "csv" is rejected there instead of silently producing a file
 * whose contents contradict its name.
 */
export async function pickExportDestination(
  defaultName: string,
): Promise<string | null> {
  const picked = await bridge().saveFile({
    defaultPath: defaultName,
    filters: [
      { name: "CSV（长格式）", extensions: ["csv"] },
      { name: "JSON（完整结构）", extensions: ["json"] },
    ],
  });
  return picked ?? null;
}

/**
 * Export an analysed file as CSV or JSON.
 *
 * Rust re-runs the analysis rather than accepting the decimated view data from
 * the UI: the export must be reproducible on its own and must match the same
 * code path that produced what is on screen.
 */
export async function exportAnalysis(
  source: string,
  destination: string,
  format: "csv" | "json",
  envelopePoints?: number,
): Promise<ExportOutcome> {
  return invoke<ExportOutcome>("export_analysis", {
    source,
    destination,
    format,
    envelopePoints: envelopePoints ?? 2048,
  });
}