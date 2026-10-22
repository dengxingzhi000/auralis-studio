/**
 * 前后端共享的类型契约。
 *
 * 这是 ADR-018 规则 1「React 不得参与音频计算」的**类型层面保障**：
 * 前端拿到的已经是 Rust 算好的视图数据，这里只描述它的形状，
 * 不提供任何做计算的函数。
 *
 * 修改本文件时注意：Rust 侧对应 `crates/auralis-engine/src/commands.rs` 与
 * `file_commands.rs` 的 `#[derive(Serialize)]` 结构，两边必须同步。
 */

/** 应用元信息。 */
export interface AppInfo {
  version: string;
  algorithmVersion: string;
  dspSpecRevision: string;
}

/** 标量指标。dB 类字段为 null 表示真 `-inf`（静音），不是被钳过的数字。 */
export interface SyntheticMetrics {
  sampleRate: number;
  sampleCount: number;
  /** 峰值 dBFS；null = 静音 */
  peakDbfs: number | null;
  /** RMS dBFS；null = 静音 */
  rmsDbfs: number | null;
  /** 直流偏移，线性，保留符号 */
  dc: number;
  /** 波峰因数 dB；null = rms 为 0 时未定义 */
  crestDb: number | null;
  overFullScaleSamples: number;
  clippingRegions: number;
  sanitizedSamples: number;
}

/**
 * 合成信号分析结果。
 *
 * `spectrumDb` 与 `waveformEnvelope` 在 Rust 侧序列化为**小端 f32 字节流**，
 * 在 JS 侧还原为 Float32Array。
 *
 * 这是 ADR-018 规则 2「二进制优先」的落点：线路上的 base64 字符串由
 * Electron 主进程还原为 `Uint8Array`，渲染进程不接触 JSON 数字数组。
 * 如果这里改成 number[]，4096 个点就是几千个 JSON 数字，
 * 这正是本架构明令禁止的。
 */
export interface SyntheticAnalysis {
  metrics: SyntheticMetrics;
  /** Float32Array，dB 值，长度 = spectrumBins */
  spectrumDb: Float32Array;
  spectrumStartHz: number;
  spectrumStepHz: number;
  spectrumBins: number;
  /** Float32Array，值域 [-1, 1]，长度 = waveformPoints */
  waveformEnvelope: Float32Array;
  waveformPoints: number;
  /** Rust 侧分析 + 编码耗时，微秒 */
  computeMicros: number;
}

/** 分析请求参数。字段名与 Rust 侧 Command 签名一一对应。 */
export interface SyntheticRequest {
  frequencyHz: number;
  amplitude: number;
  sampleRate: number;
  fftSize: number;
  /** 频谱降采样目标点数，通常 = 画布像素宽度（ADR-018 规则 3） */
  targetBins: number;
  /** 波形包络目标点数 */
  envelopePoints: number;
}

/**
 * Rust 侧 `Vec<u8>` 到达 JS 后的形态。
 *
 * Electron 主进程把 base64 解码成 `Uint8Array` 后经结构化克隆传递，
 * 正常路径总是 `Uint8Array`。保留 `ArrayLike<number>` 是为了兼容
 * 手工构造的测试数据与旧容器；归一化逻辑不能假设 `.buffer` 一定存在。
 */
type RawBytes = Uint8Array | ArrayLike<number>;

/**
 * 把 Rust 返回的小端字节流还原为 Float32Array。
 *
 * `byteLength` 可能大于 `length`（视图带偏移），因此一律按
 * `byteOffset`/`byteLength` 取窗口，而不是直接拿 `length` 算。
 *
 * 字节数不是 4 的倍数时抛错 —— 这类错误必须炸出来，静默截断会让
 * 频谱曲线整体错位，且从画面上看不出来。
 */
export function decodeF32(bytes: RawBytes): Float32Array {
  const byteOffset =
    bytes instanceof Uint8Array ? bytes.byteOffset : 0;
  const byteLength =
    bytes instanceof Uint8Array ? bytes.byteLength : bytes.length;

  if (byteLength % 4 !== 0) {
    throw new Error(
      `binary payload length ${byteLength} is not a multiple of 4`,
    );
  }

  // 只在必要处拷贝：已经是 4 字节对齐的 Uint8Array 可直接建视图（零拷贝），
  // 普通数组则必须先物化成 Uint8Array 才能取到 ArrayBuffer。
  const aligned =
    bytes instanceof Uint8Array && byteOffset % 4 === 0
      ? bytes
      : new Uint8Array(bytes);

  return new Float32Array(
    aligned.buffer.slice(
      aligned.byteOffset,
      aligned.byteOffset + aligned.byteLength,
    ),
  );
}

/* ============================================================
 * M1: real files
 * Mirrors crates/auralis-engine/src/file_commands.rs on the Rust side.
 * ============================================================ */

/** File metadata (probed from the header only). */
export interface FileInfo {
  path: string;
  fileName: string;
  fileSizeBytes: number;
  /** container id: wav / flac / mp3 / aiff / ogg / ... */
  container: string;
  containerName: string;
  codec: string;
  sampleRate: number;
  channels: number;
  /** duration in seconds; null when unknown - never faked as 0 */
  durationSeconds: number | null;
  /**
   * Whether this container has been verified against a real sample.
   * The UI **must** use this to decide what it promises to support;
   * an unverified container may only be reported as "decodable, unverified".
   */
  verified: boolean;
  verificationNote: string;
}

/** Terminal state of an analysis. */
export type AnalysisOutcome = "succeeded" | "cancelled" | "failed" | "panicked";

/** Result of analysing a real file. */
export interface FileAnalysis {
  info: FileInfo;
  /** peak dBFS; null = true silence (-inf) */
  peakDbfs: number | null;
  /** RMS dBFS; null = silence */
  rmsDbfs: number | null;
  /** DC offset, linear, sign preserved */
  dc: number;
  /** crest factor dB; null = undefined when RMS is 0 */
  crestDb: number | null;
  /** per-channel peak dBFS, length = channel count */
  peakDbfsPerChannel: (number | null)[];
  /** over-full-scale sample count (|x| > 1.0) */
  overFullScaleSamples: number;
  /** flat-top region count (heuristic, NOT evidence of analogue clipping) */
  clippingRegions: number;
  sanitizedSamples: number;
  framesDecoded: number;
  elapsedMs: number;
  /** Float32Array in [-1, 1] */
  waveformEnvelope: Float32Array;
  waveformPoints: number;
  /** Float32Array of dB values, already downsampled to canvas width (ADR-018) */
  spectrumDb: Float32Array;
  spectrumBins: number;
  spectrumStepHz: number;
  peaks: PeakInfo[];
  /** STFT dB matrix, row-major [frame][bin] (M3) */
  spectrogramDb: Float32Array;
  spectrogramFrames: number;
  spectrogramBins: number;
  /** milliseconds per spectrogram column, for the time axis */
  spectrogramHopMs: number;
  /** absolute start of the spectrogram matrix, seconds from the file start */
  spectrogramStartSeconds: number;
  /** FFT size used, so results are reproducible (DSP_SPEC 9) */
  fftSize: number;
  window: string;
  /**
   * Task id the caller supplied when starting the analysis.
   *
   * Same value that `cancelTask()` accepts, so the UI can cancel a run that
   * has not returned yet.
   */
  taskId: number;
  outcome: AnalysisOutcome;
}

/** A detected spectral peak. */
export interface PeakInfo {
  frequencyHz: number;
  /** bin reading, dBFS */
  amplitudeDbfs: number;
  /** parabolic-interpolated amplitude, dBFS */
  interpolatedDbfs: number;
  /** un-interpolated bin frequency, so the interpolation can be checked */
  binFrequencyHz: number;
  binOffset: number;
  /**
   * Scalloping loss in dB: how far the bin reading sits from the
   * interpolated one. Large values mean the reading is unreliable.
   */
  scallopingDb: number;
  /** true when the reading should be shown as an estimate, not a precise number */
  estimated: boolean;
}

/* ============================================================
 * M4: viewport-chunked spectrogram
 * ============================================================ */

/**
 * 声谱图视口。只包含一个时间窗口的 STFT 矩阵；平移视口时重新请求即可，
 * 不需要重跑整文件分析。
 */
export interface SpectrogramViewData {
  /** Float32Array, row-major [frame][bin] */
  spectrogramDb: Float32Array;
  frames: number;
  bins: number;
  /** effective milliseconds per column (already includes downsampling) */
  hopMs: number;
  /** absolute start, seconds from the file start */
  startSeconds: number;
  /** matrix span, seconds */
  durationSeconds: number;
  sampleRate: number;
  fftSize: number;
  window: string;
  outcome: string;
  elapsedMs: number;
  taskId: number;
}

/* ============================================================
 * M4: realtime capture
 * ============================================================ */

/** An input device reported by the OS. */
export interface DeviceInfo {
  id: string;
  name: string;
  /** empty when the driver does not report its rates */
  supportedSampleRates: number[];
  inputChannels: number;
}

/** Capture status. */
export interface CaptureStatus {
  running: boolean;
  sampleRate: number;
  channels: number;
  /** frames dropped because the analyser could not keep up */
  droppedFrames: number;
  /** snapshots overwritten because the UI could not keep up */
  snapshotOverwrites: number;
  processedFrames: number;
}

/** Latest realtime analysis snapshot. */
export interface RealtimeSnapshot {
  /** dB values, already downsampled to the target width (little-endian f32 bytes on the wire) */
  spectrumDb: Float32Array;
  binHz: number;
  /** capture sample rate, Hz — the frequency axis needs it */
  sampleRate: number;
  /** null = silence (true -inf, not a clamped number) */
  peakDbfs: number | null;
  rmsDbfs: number | null;
  processedFrames: number;
  droppedFrames: number;
  snapshotOverwrites: number;
}