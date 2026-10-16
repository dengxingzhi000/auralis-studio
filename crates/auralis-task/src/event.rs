//! 任务事件与状态。
//!
//! 这些类型会被序列化后经 Tauri 送到前端，因此**字段名必须与
//! `apps/desktop/src/lib/contracts.ts` 保持同步**。

use serde::Serialize;

/// 任务唯一标识。
///
/// 自增序号 + 便于日志阅读的前缀。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct TaskId(pub u64);

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "task-{}", self.0)
    }
}

/// 任务终态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskOutcome {
    /// 成功完成。
    Succeeded,
    /// 被用户或调用方取消。**与失败严格区分**——
    /// UI 对二者的提示文案不同，前者不该显示「重试」。
    Cancelled,
    /// 出错。`message` 说明原因。
    Failed,
    /// 工作线程 panic。单独成态，便于日志定位。
    Panicked,
}

/// 任务当前状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus {
    /// 已入队，尚未开始。
    Queued,
    /// 正在处理。
    Running,
    /// 已进入终态。
    Finished(TaskOutcome),
}

/// 进度快照。
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskProgress {
    /// 已处理帧数。
    pub frames_done: u64,
    /// 总帧数。未知时为 `None`（流式格式可能不预知长度）。
    pub frames_total: Option<u64>,
    /// 已解码字节数。
    pub bytes_done: u64,
}

impl TaskProgress {
    /// 进度比例 0.0–1.0。总帧数未知时返回 `None`，UI 应显示不确定态。
    #[must_use]
    pub fn fraction(&self) -> Option<f32> {
        match self.frames_total {
            Some(total) if total > 0 => {
                Some((self.frames_done as f64 / total as f64).clamp(0.0, 1.0) as f32)
            }
            _ => None,
        }
    }
}

/// 任务生命周期事件。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TaskEvent {
    /// 已入队。
    Queued {
        /// 任务 ID。
        id: TaskId,
        /// 文件路径。
        path: String,
    },
    /// 开始处理。
    Started {
        /// 任务 ID。
        id: TaskId,
        /// 采样率。
        sample_rate: u32,
        /// 声道数。
        channels: u32,
    },
    /// 进度更新。**这是可丢弃的事件**：UI 没跟上时直接跳过，不阻塞分析。
    Progress {
        /// 任务 ID。
        id: TaskId,
        /// 进度。
        progress: TaskProgress,
    },
    /// 进入终态。
    Finished {
        /// 任务 ID。
        id: TaskId,
        /// 终态。
        outcome: TaskOutcome,
        /// 失败/取消时的说明。
        message: Option<String>,
    },
}

impl TaskEvent {
    /// 所属任务。
    #[must_use]
    pub const fn id(&self) -> TaskId {
        match self {
            Self::Queued { id, .. }
            | Self::Started { id, .. }
            | Self::Progress { id, .. }
            | Self::Finished { id, .. } => *id,
        }
    }

    /// 是否为终态事件。
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Finished { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fraction_is_none_when_total_unknown() {
        let p = TaskProgress {
            frames_done: 1000,
            frames_total: None,
            bytes_done: 0,
        };
        assert!(p.fraction().is_none(), "总帧数未知时不得编造进度比例");
    }

    #[test]
    fn fraction_is_computed_when_total_known() {
        let p = TaskProgress {
            frames_done: 500,
            frames_total: Some(1000),
            bytes_done: 0,
        };
        assert!((p.fraction().unwrap() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn fraction_clamps_out_of_range() {
        // 解码器偶尔会多解出几帧，不应出现 > 1 的进度条
        let over = TaskProgress {
            frames_done: 1200,
            frames_total: Some(1000),
            bytes_done: 0,
        };
        assert!((over.fraction().unwrap() - 1.0).abs() < 1e-6);

        let zero = TaskProgress {
            frames_done: 0,
            frames_total: Some(0),
            bytes_done: 0,
        };
        assert!(zero.fraction().is_none(), "总帧数为 0 时避免除零");
    }

    #[test]
    fn cancelled_is_distinct_from_failed() {
        // UI 依赖这个区分来决定是否显示「重试」
        let c = TaskOutcome::Cancelled;
        let f = TaskOutcome::Failed;
        assert_ne!(c, f);
    }

    #[test]
    fn event_exposes_id_and_terminality() {
        let e = TaskEvent::Finished {
            id: TaskId(7),
            outcome: TaskOutcome::Succeeded,
            message: None,
        };
        assert_eq!(e.id(), TaskId(7));
        assert!(e.is_terminal());

        let p = TaskEvent::Progress {
            id: TaskId(8),
            progress: TaskProgress {
                frames_done: 0,
                frames_total: None,
                bytes_done: 0,
            },
        };
        assert_eq!(p.id(), TaskId(8));
        assert!(!p.is_terminal());
    }

    #[test]
    fn task_id_renders_readable() {
        assert_eq!(TaskId(3).to_string(), "task-3");
    }
}
