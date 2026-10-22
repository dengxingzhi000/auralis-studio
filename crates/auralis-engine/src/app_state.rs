//! Tauri 全局状态。
//!
//! 目前只持有任务取消令牌的注册表（`auralis-task` 的 `TaskEngine`
//! 是同步执行的，没有后台线程，所以不需要 `Arc<Mutex<Engine>>`）。
//!
//! M4 引入实时采集后会扩展这里，但**不会**在这里放任何音频数据
//! ——音频缓冲属于 `auralis-audio` 的有界通道，不进 UI 状态。

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use auralis_task::CancelToken;
use serde::Serialize;

/// 一条正在运行的任务（供任务队列视图）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInfoDto {
    /// 调用方分配的 task id。
    pub task_id: u64,
    /// 任务类型：`analyze` / `spectrogram` 等。
    pub kind: String,
    /// 人类可读的标签（文件名、视口范围）。
    pub label: String,
    /// 已运行时长，毫秒。
    pub elapsed_ms: u64,
}

/// 注册表中的一条任务记录。
struct TaskEntry {
    token: CancelToken,
    kind: String,
    label: String,
    started: Instant,
}

/// 桌面应用共享状态。
#[derive(Default)]
pub struct AppState {
    next_task: AtomicU64,
    tokens: Mutex<HashMap<u64, TaskEntry>>,
}

impl AppState {
    /// 新建空状态。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 分配下一个任务 ID。
    pub fn next_task_id(&self) -> u64 {
        self.next_task.fetch_add(1, Ordering::Relaxed)
    }

    /// 注册取消令牌（无标签，兼容旧调用点与测试）。
    pub fn register_token(&self, id: u64, token: CancelToken) {
        self.register_task(id, token, "task", "");
    }

    /// 注册带类型与标签的取消令牌。
    pub fn register_task(
        &self,
        id: u64,
        token: CancelToken,
        kind: impl Into<String>,
        label: impl Into<String>,
    ) {
        if let Ok(mut m) = self.tokens.lock() {
            m.insert(
                id,
                TaskEntry {
                    token,
                    kind: kind.into(),
                    label: label.into(),
                    started: Instant::now(),
                },
            );
        }
    }

    /// 用**调用方提供的** ID 注册令牌；ID 已被占用时返回 `false`。
    ///
    /// 为什么允许前端指定 ID：`analyze_file` 只能在**返回时**告诉前端任务号，
    /// 而取消要发生在运行期间。由前端先分配 ID 再发起分析，才可能在中途取消；
    /// 否则 `cancel_task` 永远只能取消一个已经结束的任务，等于没有取消能力。
    /// 重复 ID 会被拒绝，避免两个任务共用一个取消令牌。
    pub fn try_register_token(&self, id: u64, token: CancelToken) -> bool {
        self.try_register_task(id, token, "task", "")
    }

    /// 用调用方提供的 ID 注册带标签的任务；ID 已被占用时返回 `false`。
    pub fn try_register_task(
        &self,
        id: u64,
        token: CancelToken,
        kind: impl Into<String>,
        label: impl Into<String>,
    ) -> bool {
        match self.tokens.lock() {
            Ok(mut m) => match m.entry(id) {
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(TaskEntry {
                        token,
                        kind: kind.into(),
                        label: label.into(),
                        started: Instant::now(),
                    });
                    true
                }
                std::collections::hash_map::Entry::Occupied(_) => false,
            },
            Err(_) => false,
        }
    }

    /// 当前正在运行的任务快照，按开始顺序排列。
    #[must_use]
    pub fn tasks(&self) -> Vec<TaskInfoDto> {
        let Ok(m) = self.tokens.lock() else {
            return Vec::new();
        };
        let mut tasks: Vec<TaskInfoDto> = m
            .iter()
            .map(|(&task_id, entry)| TaskInfoDto {
                task_id,
                kind: entry.kind.clone(),
                label: entry.label.clone(),
                elapsed_ms: u64::try_from(entry.started.elapsed().as_millis()).unwrap_or(u64::MAX),
            })
            .collect();
        tasks.sort_by_key(|t| t.task_id);
        tasks
    }

    /// 注销令牌。任务结束时必须调用，否则注册表会无限增长。
    pub fn unregister_token(&self, id: u64) {
        if let Ok(mut m) = self.tokens.lock() {
            m.remove(&id);
        }
    }

    /// 取消指定任务。返回是否找到了该任务。
    pub fn cancel(&self, id: u64) -> bool {
        match self.tokens.lock() {
            Ok(m) => match m.get(&id) {
                Some(t) => {
                    t.token.cancel();
                    true
                }
                None => false,
            },
            Err(_) => false,
        }
    }

    /// 当前注册的任务数（供测试与状态栏使用）。
    #[must_use]
    pub fn registered_count(&self) -> usize {
        self.tokens.lock().map_or(0, |m| m.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_ids_are_unique_and_monotonic() {
        let s = AppState::new();
        let a = s.next_task_id();
        let b = s.next_task_id();
        assert!(b > a, "任务 ID 必须单调递增");
    }

    #[test]
    fn cancel_only_affects_registered_task() {
        let s = AppState::new();
        let id = s.next_task_id();
        let t = CancelToken::new();
        s.register_token(id, t.clone());

        assert!(!s.cancel(id + 999), "未注册的任务不可取消");
        assert!(!t.is_cancelled());

        assert!(s.cancel(id));
        assert!(t.is_cancelled(), "已注册的任务应被取消");
    }

    #[test]
    fn unregister_prevents_later_cancellation() {
        let s = AppState::new();
        let id = s.next_task_id();
        let t = CancelToken::new();
        s.register_token(id, t.clone());
        assert_eq!(s.registered_count(), 1);

        s.unregister_token(id);
        assert_eq!(s.registered_count(), 0, "注销后注册表不应增长");
        assert!(!s.cancel(id));
        assert!(!t.is_cancelled());
    }

    #[test]
    fn task_registry_exposes_kind_and_label() {
        let s = AppState::new();
        let token = CancelToken::new();
        assert!(s.try_register_task(42, token.clone(), "analyze", "tone.wav"));

        let tasks = s.tasks();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].task_id, 42);
        assert_eq!(tasks[0].kind, "analyze");
        assert_eq!(tasks[0].label, "tone.wav");
        assert!(s.cancel(42));
        assert!(token.is_cancelled());

        s.unregister_token(42);
        assert!(s.tasks().is_empty(), "注销后任务不应留在队列快照里");
    }

    #[test]
    fn caller_supplied_ids_must_be_unique() {
        let s = AppState::new();
        let t1 = CancelToken::new();
        assert!(s.try_register_token(7, t1.clone()), "首次注册应成功");
        let t2 = CancelToken::new();
        assert!(
            !s.try_register_token(7, t2.clone()),
            "重复 ID 必须被拒绝，否则两个任务共用令牌"
        );
        // 被拒绝的那次不能覆盖已注册的令牌
        assert!(s.cancel(7));
        assert!(t1.is_cancelled());
        assert!(!t2.is_cancelled());
    }
}
