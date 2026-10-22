//! 实时分析工作线程。
//!
//! # 为什么必须有一个独立线程
//!
//! 依据 [docs/BENCHMARKS.md](../../BENCHMARKS.md)：48 kHz 回调预算约 **20.8 µs**，
//! 而 FFT 4096 需 **271 µs**（超 13 倍）。因此：
//!
//! ```text
//! 音频回调（实时）            工作线程（可抢占）        UI 线程
//!   只做拷贝 + 计数      ──►   弹出样本 → FFT ──►   轮询最新快照
//!   ~20 µs 预算内              落后了就丢帧并计数       20–30 Hz
//! ```
//!
//! 工作线程**永不阻塞 UI**：它只往一个「最新快照」槽里写，UI 来了就取，
//! 没更新就沿用上一帧。
//!
//! # 丢帧可观测
//!
//! 两处丢帧都要计数：
//! - 环形缓冲满（生产者跟不上）→ `ring.stats().dropped_frames`
//! - 快照槽被覆盖（UI 跟不上）→ `snapshot_overwrites`
//!
//! 两者都必须显示在 UI 上，否则用户无法判断分析结果是否完整
//! （`AGENTS.md`：「instrument dropped frames」）。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use auralis_dsp::{SpectrumAnalyzer, SpectrumConfig, WindowFunction, analyze_channel};

use crate::capture::{BufferRecycler, CallbackBuffer};
use crate::ring::SampleRing;

/// 工作线程的运行参数。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RealtimeConfig {
    /// FFT 长度。
    pub fft_size: usize,
    /// 频谱降采样目标点数（= 画布像素宽度量级）。
    pub target_bins: usize,
    /// 采集设备的声道数。采集回调给的是**交错**数据，需要它来正确混成单声道。
    pub channels: u16,
}

impl Default for RealtimeConfig {
    /// 2048 点 FFT 足够实时频谱（实测约 130 µs，远低于 20 ms 的刷新周期）。
    fn default() -> Self {
        Self {
            fft_size: 2048,
            target_bins: 1024,
            channels: 2,
        }
    }
}

/// dBFS 转换：保留「静音 = null」而不是把它钳成 -140 dBFS。
///
/// `DSP_SPEC.md` §3 要求零值用明确的 `-inf` 语义，不能用一个假精确的数字代替。
fn dbfs_opt(db: auralis_core::units::Dbfs) -> Option<f32> {
    match db {
        auralis_core::units::Dbfs::NegativeInfinity => None,
        auralis_core::units::Dbfs::Value(v) => Some(v),
    }
}

/// 一次发布是否**覆盖**了未被消费的上一代。
///
/// 不能每发布一帧就记一次覆盖：UI 以 30 Hz 取数，工作线程以更高速率发布，
/// 那样计数会永远等于发布次数，指标变成噪声。只有「上一代还没被取走」
/// 才算真正的覆盖。
const fn is_overwrite(published: u64, taken: u64) -> bool {
    taken < published
}

/// 一帧实时结果。
#[derive(Debug, Clone, Default)]
pub struct RealtimeSnapshot {
    /// 频谱 dB（已降采样）。
    pub spectrum_db: Vec<f32>,
    /// 频率间隔 Hz。
    pub bin_hz: f64,
    /// 采样率。
    pub sample_rate: u32,
    /// 峰值 dBFS；`None` = 静音。
    pub peak_dbfs: Option<f32>,
    /// RMS dBFS；`None` = 静音。
    pub rms_dbfs: Option<f32>,
    /// 环形缓冲丢弃的帧数。
    pub dropped_frames: u64,
    /// 快照被覆盖的次数（UI 跟不上）。
    pub snapshot_overwrites: u64,
    /// 工作线程已处理的帧数。
    pub processed_frames: u64,
}

/// 工作线程句柄。
pub struct RealtimeWorker {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
    snapshot: Arc<std::sync::Mutex<RealtimeSnapshot>>,
    /// 已发布的快照代数。工作线程写。
    published: Arc<AtomicU64>,
    /// UI 最后取走的代数。`latest()` 写。
    taken: Arc<AtomicU64>,
    /// 累计「上一帧没被取走就又被覆盖」的次数。
    overwrites: Arc<AtomicU64>,
}

impl RealtimeWorker {
    /// 启动工作线程。
    ///
    /// `ring` 由音频回调写入，本线程消费；两者都是无锁 SPSC。
    ///
    /// # Errors
    /// 线程无法创建。
    pub fn spawn(
        ring: Arc<SampleRing<CallbackBuffer>>,
        recycler: BufferRecycler,
        sample_rate: u32,
        config: RealtimeConfig,
    ) -> Result<Self, String> {
        let analyzer = SpectrumAnalyzer::new(SpectrumConfig {
            fft_size: config.fft_size,
            hop_size: config.fft_size / 2,
            window: WindowFunction::Hann,
            remove_dc: false,
        })
        .map_err(|e| e.to_string())?;

        let stop = Arc::new(AtomicBool::new(false));
        let snapshot = Arc::new(std::sync::Mutex::new(RealtimeSnapshot::default()));
        let published = Arc::new(AtomicU64::new(0));
        let taken = Arc::new(AtomicU64::new(0));
        let overwrites = Arc::new(AtomicU64::new(0));

        let stop_c = stop.clone();
        let snapshot_c = snapshot.clone();
        let published_c = published.clone();
        let taken_c = taken.clone();
        let overwrites_c = overwrites.clone();

        let handle = std::thread::Builder::new()
            .name("auralis-realtime".to_string())
            .spawn(move || {
                let mut chunk: Vec<CallbackBuffer> = Vec::with_capacity(64);
                // 滚动窗口：**只保留最新的 fft_size 个单声道样本**。
                //
                // 不保留整段 chunk：环形缓冲积压时一次可能取到上万样本，
                // 而一次刷新只画一条曲线，攒下来的中间样本既没人看，
                // 又会把内存推到与缓冲深度成正比。
                let mut tail: Vec<f32> = Vec::with_capacity(config.fft_size * 2);
                let channels = (config.channels as usize).max(1);
                let mut processed: u64 = 0;

                while !stop_c.load(Ordering::Relaxed) {
                    if !ring.pop_chunk(&mut chunk) {
                        // 没有数据：睡一小会儿，别空转烧 CPU
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        continue;
                    }

                    // 采集回调给的是**交错**多声道数据，这里混成单声道。
                    // 取各声道平均而非只取通道 0：立体声左右内容不同时，
                    // 只取一个声道会漏掉一半信息。
                    let consumed: u64 = chunk.iter().map(|b| (b.len() / channels) as u64).sum();
                    processed += consumed;

                    for buf in &chunk {
                        if channels == 1 {
                            tail.extend_from_slice(buf);
                        } else {
                            let frames = buf.len() / channels;
                            for f in 0..frames {
                                let base = f * channels;
                                let mut acc = 0.0_f32;
                                for c in 0..channels {
                                    acc += buf[base + c];
                                }
                                tail.push(acc / channels as f32);
                            }
                        }
                    }

                    // 丢掉过旧的样本，保持窗口正好是 fft_size
                    if tail.len() > config.fft_size {
                        let excess = tail.len() - config.fft_size;
                        tail.drain(0..excess);
                    }

                    if tail.len() >= config.fft_size {
                        let frame = tail.as_slice();
                        let (db, step) = match analyzer.analyze(frame, sample_rate) {
                            Ok(frames) => match frames.last() {
                                Some(f) => f.downsample_db(config.target_bins),
                                None => (Vec::new(), 1.0),
                            },
                            Err(_) => (Vec::new(), 1.0),
                        };

                        let stats = analyze_channel(frame, auralis_dsp::ClippingConfig::default());
                        let next = RealtimeSnapshot {
                            spectrum_db: db,
                            bin_hz: step,
                            sample_rate,
                            peak_dbfs: dbfs_opt(stats.peak.dbfs()),
                            rms_dbfs: dbfs_opt(stats.rms.dbfs()),
                            dropped_frames: ring.stats().dropped_frames,
                            snapshot_overwrites: overwrites_c.load(Ordering::Relaxed),
                            processed_frames: processed,
                        };

                        {
                            let mut guard = match snapshot_c.lock() {
                                Ok(g) => g,
                                Err(poisoned) => poisoned.into_inner(),
                            };
                            *guard = next;
                        }

                        // 覆盖 = 上一代还没被 UI 取走就被这一代顶掉。
                        // 单纯「写过一次」不算覆盖，否则计数永远等于发布数。
                        let prev = published_c.load(Ordering::Relaxed);
                        if is_overwrite(prev, taken_c.load(Ordering::Relaxed)) {
                            overwrites_c.fetch_add(1, Ordering::Relaxed);
                        }
                        published_c.store(prev + 1, Ordering::Relaxed);
                    }

                    // 缓冲归池，供回调复用。drain 保留 chunk 自身容量，
                    // 避免每轮重新分配那 64 个指针位。
                    recycler.recycle_drain(&mut chunk);
                }
            })
            .map_err(|e| e.to_string())?;

        Ok(Self {
            stop,
            handle: Some(handle),
            snapshot,
            published,
            taken,
            overwrites,
        })
    }

    /// 取最新快照。永不阻塞（锁竞争极短且不涉及 IO）。
    #[must_use]
    pub fn latest(&self) -> RealtimeSnapshot {
        let value = match self.snapshot.lock() {
            Ok(g) => g.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        // 标记「当前这一代已被消费」，供覆盖计数判断。
        self.taken
            .store(self.published.load(Ordering::Relaxed), Ordering::Relaxed);
        value
    }

    /// 快照被覆盖的累计次数（UI 跟不上工作线程的速率）。
    #[must_use]
    pub fn overwrites(&self) -> u64 {
        self.overwrites.load(Ordering::Relaxed)
    }

    /// 停止工作线程并回收缓冲。
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for RealtimeWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ring::DropPolicy;

    fn ring_of(n: usize) -> Arc<SampleRing<CallbackBuffer>> {
        Arc::new(SampleRing::new(n, 256, DropPolicy::DropNewest))
    }

    fn recycler() -> BufferRecycler {
        // 独立回收池，测试里不与真实回调共享
        BufferRecycler::for_tests(64)
    }

    #[test]
    fn worker_produces_a_snapshot_from_pushed_samples() {
        let ring = ring_of(16);
        let cfg = RealtimeConfig {
            fft_size: 512,
            target_bins: 64,
            channels: 1,
        };
        let mut w = RealtimeWorker::spawn(ring.clone(), recycler(), 48_000, cfg).expect("spawn");

        // 推入足够多的样本
        let data = vec![0.5_f32; 512];
        for _ in 0..8 {
            let _ = ring.push(data.clone());
        }

        // 等工作线程处理
        let mut snap = w.latest();
        for _ in 0..100 {
            if snap.processed_frames > 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            snap = w.latest();
        }

        assert!(snap.processed_frames > 0, "工作线程应处理了样本");
        assert_eq!(snap.spectrum_db.len(), 64, "频谱应降采样到目标点数");
        assert!(snap.spectrum_db.iter().all(|v| v.is_finite()));
        assert_eq!(snap.sample_rate, 48_000);
        w.stop();
    }

    #[test]
    fn worker_reports_dropped_frames() {
        let ring = ring_of(2); // 故意很小，触发丢弃
        let cfg = RealtimeConfig {
            fft_size: 512,
            target_bins: 32,
            channels: 1,
        };
        let mut w = RealtimeWorker::spawn(ring.clone(), recycler(), 48_000, cfg).expect("spawn");

        let data = vec![0.5_f32; 512];
        for _ in 0..200 {
            let _ = ring.push(data.clone());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));

        let snap = w.latest();
        // 丢帧必须可观测
        assert!(
            snap.dropped_frames > 0 || ring.stats().dropped_frames > 0,
            "环形缓冲丢弃必须被计数"
        );
        w.stop();
    }

    #[test]
    fn worker_stops_promptly() {
        let ring = ring_of(8);
        let cfg = RealtimeConfig::default();
        let mut w = RealtimeWorker::spawn(ring, recycler(), 48_000, cfg).expect("spawn");
        let t0 = std::time::Instant::now();
        w.stop();
        assert!(
            t0.elapsed() < std::time::Duration::from_secs(2),
            "停止应迅速完成，实得 {:?}",
            t0.elapsed()
        );
    }

    #[test]
    fn worker_on_empty_ring_does_not_spin_or_panic() {
        let ring = ring_of(4);
        let cfg = RealtimeConfig::default();
        let mut w = RealtimeWorker::spawn(ring, recycler(), 48_000, cfg).expect("spawn");
        std::thread::sleep(std::time::Duration::from_millis(60));
        let snap = w.latest();
        assert_eq!(snap.processed_frames, 0, "无数据时不应产生帧");
        w.stop();
    }

    #[test]
    fn overwrite_is_only_counted_when_the_previous_frame_was_not_taken() {
        // 初始：发布 0、已取 0 —— 没有任何东西被覆盖
        assert!(!is_overwrite(0, 0));
        // UI 每帧都取：发布数始终等于已取数
        assert!(!is_overwrite(5, 5));
        // UI 落后一代：上一代（发布 5）还没被取（已取 4）
        assert!(is_overwrite(5, 4));
        // UI 落后多代也只算「这一次覆盖了上一代」，不是算差值
        assert!(is_overwrite(9, 2));
    }

    #[test]
    fn worker_counts_consumed_samples_not_window_length() {
        // 推入的量就是 stats 里 processed_frames 的下限：
        // 统计的是**从环形缓冲取走的样本数**，不是每帧固定加 fft_size。
        let ring = ring_of(64);
        let cfg = RealtimeConfig {
            fft_size: 256,
            target_bins: 16,
            channels: 1,
        };
        let mut w = RealtimeWorker::spawn(ring.clone(), recycler(), 48_000, cfg).expect("spawn");

        let total: u64 = 4 * 1024;
        for _ in 0..4 {
            let _ = ring.push(vec![0.1_f32; 1024]);
        }

        let mut snap = w.latest();
        for _ in 0..200 {
            if snap.processed_frames >= total {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            snap = w.latest();
        }

        assert_eq!(
            snap.processed_frames, total,
            "已处理样本数必须等于实际取走的样本数"
        );
        w.stop();
    }

    #[test]
    fn stereo_input_is_downmixed_to_mono_before_analysis() {
        // 左 +1.0 / 右 −1.0 的完全反相立体声混成单声道后应当抵消为静音。
        // 这条断言能抓住「只取通道 0」或「交错数据当成单声道」两类错误。
        let ring = ring_of(16);
        let cfg = RealtimeConfig {
            fft_size: 128,
            target_bins: 8,
            channels: 2,
        };
        let mut w = RealtimeWorker::spawn(ring.clone(), recycler(), 48_000, cfg).expect("spawn");

        // 256 个交错样本 = 128 帧立体声
        let mut interleaved = Vec::with_capacity(256);
        for _ in 0..128 {
            interleaved.push(1.0_f32);
            interleaved.push(-1.0_f32);
        }
        let _ = ring.push(interleaved);

        let mut snap = w.latest();
        for _ in 0..200 {
            if snap.processed_frames > 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            snap = w.latest();
        }

        assert_eq!(snap.processed_frames, 128, "交错数据应按帧计数");
        assert_eq!(
            snap.peak_dbfs, None,
            "完全反相的立体声混音后是数字静音，应为 null 而不是一个假读数"
        );
        w.stop();
    }
}
