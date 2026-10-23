/**
 * 显示层的数据形状。
 *
 * `App.tsx` 里状态太多，直接把这份接口留在那儿会让「谁在消费它」变得
 * 不清楚 —— 现在有 `MetricsPanel` 也要用它。单独放一个模块，
 * 消费方 import 它，而不是 import App。
 *
 * 这里**只有形状，没有行为**：所有数值都是 Rust 侧算好的，
 * 前端只负责显示（ADR-018 规则 1）。
 */

import type { PeakInfo } from "@/lib/contracts";

/** 真实文件分析结果的显示形状（供 Canvas 组件与指标视图消费）。 */
export interface DisplayData {
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
  /**
   * 数据流提前结束（文件被截断）。
   *
   * 截断**不是失败**：已解出的部分依然有效，分析覆盖的是文件里实际
   * 存在的那一段。界面上必须说出来，否则用户会以为分析覆盖了整个文件。
   */
  truncated: boolean;
  /** 文件头声明的时长，秒 */
  declaredDurationSeconds: number;
  /** 实际解码出来的时长，秒 */
  decodedDurationSeconds: number;
}
