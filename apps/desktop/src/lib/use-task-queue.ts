/**
 * 后台任务队列（React 状态层）。
 *
 * 两样东西：
 *
 * - `tasks`：**运行中**的任务，来自 Rust 的任务注册表；
 * - `history`：最近完成的分析，**只在前端记**（理由见 `task-history.ts`）。
 *
 * ## 轮询为什么是 500 ms
 *
 * `task_queue` 是**快命令**：在 RPC 读取线程里就地执行，只读一张哈希表，
 * 不会排在耗时的文件分析后面。500 ms 一次，开销可忽略。
 *
 * 失败时静默忽略：拿不到队列不代表分析失败，不该因此弹错误条。
 */

import { useCallback, useEffect, useState } from "react";

import { fetchTaskQueue } from "./ipc";
import type { TaskInfo } from "./contracts";
import { pushHistory, type HistoryEntry, type TaskOutcome } from "./task-history";

/** 任务队列的状态与操作。 */
export interface TaskQueueController {
  /** 运行中的任务；来自 Rust 的任务注册表。 */
  tasks: TaskInfo[];
  /** 最近完成的分析；Rust 侧任务结束即注销，结局只有发起方知道。 */
  history: HistoryEntry[];
  /**
   * 把一次文件分析的结局记进「最近完成」。
   *
   * 只收文件分析：声谱图视口请求在自动平移时会连发，进历史会把用户
   * 真正关心的分析挤掉。
   */
  record: (
    taskId: number,
    label: string,
    outcome: TaskOutcome,
    elapsedMs: number,
  ) => void;
}

/** 后台任务队列。 */
export function useTaskQueue(): TaskQueueController {
  const [tasks, setTasks] = useState<TaskInfo[]>([]);
  const [history, setHistory] = useState<HistoryEntry[]>([]);

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

  const record = useCallback(
    (
      taskId: number,
      label: string,
      outcome: TaskOutcome,
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

  return { tasks, history, record };
}
