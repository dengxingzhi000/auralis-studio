/**
 * Tauri IPC 封装。
 *
 * 薄薄一层，只负责：
 *   1. 用 camelCase 调 Rust 侧命令
 *   2. 把二进制字段还原成 Float32Array
 *
 * 刻意不引入 TanStack Query 之类的数据层——本项目的"异步任务"是
 * 本地音频分析，不是 Web 请求。套一层 HTTP 语义的缓存层反而
 * 会误导状态管理（技术选型文档里也明确提到这一点）。
 */

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

import {
  decodeF32,
  type AppInfo,
  type FileAnalysis,
  type FileInfo,
  type SyntheticAnalysis,
  type SyntheticRequest,
} from "./contracts";

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
      spectrumDb: Uint8Array;
      waveformEnvelope: Uint8Array;
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

/** File extensions offered in the open dialog. */
const AUDIO_EXTENSIONS = ["wav", "flac", "mp3", "aiff", "aif", "ogg", "wav"];

/**
 * Show the native file picker.
 *
 * Returns null when the user cancels - cancellation is not an error,
 * so callers must not surface it as one.
 */
export async function pickAudioFile(): Promise<string | null> {
  const picked = await open({
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
): Promise<TimedFileAnalysis> {
  const t0 = performance.now();
  const raw = await invoke<
    Omit<FileAnalysis, "waveformEnvelope" | "spectrumDb"> & {
      waveformEnvelope: Uint8Array;
      spectrumDb: Uint8Array;
    }
  >("analyze_file", {
    path,
    envelopePoints: envelopePoints ?? 2048,
  });
  return {
    ...raw,
    waveformEnvelope: decodeF32(raw.waveformEnvelope),
    spectrumDb: decodeF32(raw.spectrumDb),
    __elapsedMeasuredMs: performance.now() - t0,
  };
}

/** Cancel a running task. Returns false when the task id is unknown. */
export async function cancelTask(taskId: number): Promise<boolean> {
  return invoke<boolean>("cancel_task", { taskId });
}