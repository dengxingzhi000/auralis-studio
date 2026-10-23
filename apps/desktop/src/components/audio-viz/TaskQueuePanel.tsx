/**
 * TaskQueuePanel —— 后台任务队列（左侧资源栏）。
 *
 * ## 数据来自两个地方
 *
 * - **运行中**：Rust 侧的任务注册表（`task_queue` 命令）。它是**快命令**，
 *   在 RPC 读取线程里就地执行，不会排在耗时的文件分析后面。
 * - **最近完成**：前端在本次会话内记的（`lib/task-history.ts`）。Rust 侧
 *   任务结束就注销，只暴露运行中的任务。
 *
 * ## 为什么面板默认折叠
 *
 * 侧栏宽度有限，而且绝大多数时候没有任务在跑。一个永远占着半屏的
 * 空列表是干扰。有任务时自动展开，没任务时折叠。
 */

import type { TaskInfo } from "@/lib/contracts";
import type { HistoryEntry, TaskOutcome } from "@/lib/task-history";

export interface TaskQueuePanelProps {
  /** 运行中的任务（来自 `task_queue`） */
  running: TaskInfo[];
  /** 最近完成的任务（本次会话内） */
  history: HistoryEntry[];
  /** 请求取消一个运行中的任务 */
  onCancel: (taskId: number) => void;
}

/** 任务类型 → 界面上的短标签。 */
const KIND_LABELS: Record<string, string> = {
  analyze: "分析",
  spectrogram: "声谱图",
  task: "任务",
};

/** 结束状态 → 徽标文字与样式类。 */
const OUTCOMES: Record<TaskOutcome, { label: string; cls: string }> = {
  succeeded: { label: "完成", cls: "ok" },
  cancelled: { label: "已取消", cls: "warn" },
  failed: { label: "失败", cls: "bad" },
};

/** 毫秒 → 人类可读的耗时。 */
function fmtElapsed(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return "—";
  if (ms < 1000) return `${Math.round(ms)} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)} s`;
  const m = Math.floor(ms / 60_000);
  const s = Math.round((ms % 60_000) / 1000);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

export function TaskQueuePanel({
  running,
  history,
  onCancel,
}: TaskQueuePanelProps) {
  const empty = running.length === 0 && history.length === 0;

  return (
    <div className="task-queue">
      <div className="rt-head">
        <span className="label">后台任务</span>
        {running.length > 0 && (
          <span className="rt-live-dot" title="有任务正在运行" />
        )}
      </div>

      {empty && <div className="muted rt-note">没有正在运行的任务。</div>}

      {running.length > 0 && (
        <>
          <div className="task-section-label">运行中</div>
          <ul className="task-list">
            {running.map((t) => (
              <li className="task-row" key={t.taskId}>
                <span className="task-kind">{KIND_LABELS[t.kind] ?? t.kind}</span>
                <span className="task-label" title={t.label}>
                  {t.label || "（未命名）"}
                </span>
                <span className="task-elapsed num">{fmtElapsed(t.elapsedMs)}</span>
                <button
                  type="button"
                  className="btn tiny danger"
                  onClick={() => onCancel(t.taskId)}
                  title="取消这个任务"
                >
                  取消
                </button>
              </li>
            ))}
          </ul>
        </>
      )}

      {history.length > 0 && (
        <>
          <div className="task-section-label">最近完成</div>
          <ul className="task-list">
            {history.map((h) => {
              const outcome = OUTCOMES[h.outcome];
              return (
                <li className="task-row" key={h.id}>
                  <span className="task-kind">{KIND_LABELS[h.kind] ?? h.kind}</span>
                  <span className="task-label" title={h.label}>
                    {h.label}
                  </span>
                  <span className={`task-outcome ${outcome.cls}`}>
                    {outcome.label}
                  </span>
                  <span className="task-elapsed num">{fmtElapsed(h.elapsedMs)}</span>
                </li>
              );
            })}
          </ul>
        </>
      )}
    </div>
  );
}
