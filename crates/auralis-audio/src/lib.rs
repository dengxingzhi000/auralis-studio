//! # auralis-audio
//!
//! 实时音频采集。**本 crate 的核心约束是「回调里什么都不许慢」**。
//!
//! ## 实时安全铁律（`AGENTS.md`「Real-time safety」）
//!
//! 音频回调中**禁止**：
//! - 阻塞在锁、通道、文件、数据库、网络或 UI 上
//! - 进行无界分配
//! - 同步打印大量内容
//! - 等待分析完成
//!
//! 因此回调里只做三件事：
//! 1. 把输入样本转换/拷进**预分配**的环形缓冲
//! 2. 递增一个原子计数器
//! 3. 返回
//!
//! 全部 DSP 都在工作线程上跑（见 `docs/BENCHMARKS.md`：FFT 4096 需 271 µs，
//! 而 48 kHz 回调预算只有约 20.8 µs，**超 13 倍**，所以放进回调必然掉帧）。
//!
//! ## 为什么用 `crossbeam_channel::ArrayQueue` 而不是自己写
//!
//! 它是无锁的、定长的 SPSC 队列，且已在依赖树里。用它可以避免：
//! - 自己用 `unsafe` 写环形缓冲（违反本项目 `#![forbid(unsafe_code)]`）
//! - 引入 `rtrb` 这个额外依赖
//!
//! ## 溢出策略：丢最新 + 计数
//!
//! 队列满意味着工作线程跟不上。**丢最新**而不是丢最旧，理由：
//! 分析的目的是反映「当前信号是什么」，过期样本价值低于新样本。
//! 两种策略都会造成时间轴断裂，但丢新样本保证显示的时间点
//! 距离「现在」更近。
//!
//! **每次丢弃都被计入 `dropped_frames`**，UI 必须显示这个计数
//! （`AGENTS.md`：「instrument dropped frames」）。静默丢弃是不可接受的。

#![forbid(unsafe_code)]

pub mod capture;
pub mod devices;
pub mod ring;

pub use capture::{BufferRecycler, CallbackBuffer, CaptureSession, start_capture};
pub use devices::{AudioDeviceInfo, CaptureConfig, DeviceError, list_input_devices};
pub use ring::{DropPolicy, RingStats, SampleRing};
