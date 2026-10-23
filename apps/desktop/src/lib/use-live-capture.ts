/**
 * 实时采集的显示状态（React 状态层）。
 *
 * 依据 `docs/BENCHMARKS.md`：48 kHz 回调预算约 20.8 µs，FFT 4096 需 271 µs，
 * 所以链路是：
 *
 * ```text
 * 音频回调 -> 环形缓冲 -> 工作线程(FFT) -> 最新快照槽 -> UI 每 ~33 ms 拉一次
 * ```
 *
 * 本模块**只存快照**，不做任何计算：频谱是 Rust 侧已经降采样好的
 * `Float32Array`，这里原样透传（ADR-018 规则 1）。
 *
 * 采集的启停本身在 `RealtimePanel` 里（它要跟设备列表放在一起），
 * 本模块只接收它报回来的快照与状态。
 */

import { useCallback, useState } from "react";

import type { RealtimeSnapshot } from "./contracts";

/** 实时采集的显示状态。 */
export interface LiveCaptureController {
  /** 峰值 dBFS；`null` = 静音。 */
  peak: number | null;
  /** RMS dBFS；`null` = 静音。 */
  rms: number | null;
  /** 每声道峰值 dBFS。 */
  peaks: (number | null)[];
  /** 每声道 RMS dBFS。 */
  rmsPerChannel: (number | null)[];
  /** 实时频谱（dB，已按像素宽度降采样）。 */
  db: Float32Array | null;
  /** 频率间隔 Hz。 */
  stepHz: number;
  /** 采集设备的采样率；画频率轴必须用它，不能沿用文件的。 */
  sampleRate: number | null;
  /** 采集是否正在运行（用于导航项上的状态标记）。 */
  running: boolean;
  /**
   * 灌入一份实时快照。
   *
   * `null` 表示采集已停止或还没有数据：清空读数，但**不动** `running`
   * —— 采集中但输入静音时快照也是空的，那样「采集中」标记会一闪一闪。
   */
  applySnapshot: (snapshot: RealtimeSnapshot | null) => void;
  /** 采集启停状态变化。 */
  applyStatus: (running: boolean) => void;
}

/** 实时采集的显示状态。 */
export function useLiveCapture(): LiveCaptureController {
  const [peak, setPeak] = useState<number | null>(null);
  const [rms, setRms] = useState<number | null>(null);
  const [peaks, setPeaks] = useState<(number | null)[]>([]);
  const [rmsPerChannel, setRmsPerChannel] = useState<(number | null)[]>([]);
  const [db, setDb] = useState<Float32Array | null>(null);
  const [stepHz, setStepHz] = useState(1);
  const [sampleRate, setSampleRate] = useState<number | null>(null);
  const [running, setRunning] = useState(false);

  const applySnapshot = useCallback((snapshot: RealtimeSnapshot | null) => {
    setDb(snapshot ? snapshot.spectrumDb : null);
    setStepHz(snapshot ? snapshot.binHz : 1);
    setSampleRate(snapshot ? snapshot.sampleRate : null);
    setPeak(snapshot ? snapshot.peakDbfs : null);
    setRms(snapshot ? snapshot.rmsDbfs : null);
    setPeaks(snapshot ? snapshot.peakDbfsPerChannel : []);
    setRmsPerChannel(snapshot ? snapshot.rmsDbfsPerChannel : []);
  }, []);

  const applyStatus = useCallback((value: boolean) => {
    setRunning(value);
  }, []);

  return {
    peak,
    rms,
    peaks,
    rmsPerChannel,
    db,
    stepHz,
    sampleRate,
    running,
    applySnapshot,
    applyStatus,
  };
}
