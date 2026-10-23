/**
 * 「最近完成」的历史记录（纯函数）。
 *
 * ## 为什么历史在前端记
 *
 * Rust 侧的任务注册表只持有**运行中**的任务：任务结束就注销了。
 * 所以「完成 / 已取消 / 失败」这个结局，只有发起方知道 —— 前端自己
 * 持有那个 promise，Rust 把结果写在返回值里。
 *
 * 与其让 Rust 保留一份已完成的任务表（要加清理策略、要处理进程重启），
 * 不如让前端记自己发起的那些。后者天然只包含「用户关心的」。
 *
 * ## 只收录文件分析
 *
 * 声谱图视口请求（`spectrogram_view`）在自动平移时会连发，进历史
 * 会把列表刷满，用户真正关心的文件分析反而被挤掉。所以历史只收
 * `analyze` 类任务。
 */

/** 任务的结束状态。 */
export type TaskOutcome = "succeeded" | "cancelled" | "failed";

/** 一条已完成的任务。 */
export interface HistoryEntry {
  /** 唯一标识。同一个 id 再次出现表示覆盖（而不是追加一条）。 */
  id: string;
  /** 任务类型：`analyze` / `spectrogram`。 */
  kind: string;
  /** 人类可读的标签（文件名）。 */
  label: string;
  /** 结束状态。 */
  outcome: TaskOutcome;
  /** 耗时，毫秒。 */
  elapsedMs: number;
  /** 结束时刻（`Date.now()`）。 */
  endedAt: number;
}

/** 历史记录默认上限。 */
export const HISTORY_LIMIT = 8;

/**
 * 把一条记录并入历史。
 *
 * 最新在前；同 id 覆盖旧记录（一个任务不可能既完成又失败）；
 * 超出上限时截掉最旧的。**不修改**传入的数组。
 */
export function pushHistory(
  list: HistoryEntry[],
  entry: HistoryEntry,
  limit: number = HISTORY_LIMIT,
): HistoryEntry[] {
  const next = [entry, ...list.filter((e) => e.id !== entry.id)];
  return next.slice(0, Math.max(0, limit));
}
