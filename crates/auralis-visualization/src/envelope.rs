//! 流式 min/max 包络。
//!
//! ## 为什么不能「先收全文件再建金字塔」
//!
//! 3 小时 48 kHz 立体声是 5.18 亿个 `f32`（约 2 GB）。离线分析是逐块解码的，
//! 不允许为了画一条波形把整段 PCM 留在内存里。
//!
//! 本模块从流式样本直接维护**每声道**的 min/max 桶：
//!
//! - 初始每桶 1 帧；
//! - 桶数超过 `2 × points` 时，把相邻两桶合并（`frames_per_bucket` 翻倍）；
//! - 结束时把桶合并到恰好 `points` 列。
//!
//! 这样内存上界是 `2 × points × 声道数 × 8 字节`（默认 2048 点、立体声约
//! 64 KB），且**整条时间轴**都被覆盖——不会出现「只保留了文件末尾」的抽样。
//!
//! ## 为什么必须保留 min 和 max
//!
//! 只保留 `max` 会丢掉负半波，只保留抽样点会丢掉瞬态。Audacity /
//! Sonic Visualiser 的波形缓存同样按块保存 min/max，道理是一样的：
//! 画一条竖线带需要两个端点，而峰值不能被平均掉。

use crate::pyramid::MinMax;

/// 把交错 PCM 累积成每声道的 min/max 包络。
#[derive(Debug, Clone, PartialEq)]
pub struct StreamingEnvelope {
    channels: usize,
    points: usize,
    max_buckets: usize,
    frames_per_bucket: u64,
    pending_frames: u64,
    buckets: Vec<Vec<MinMax>>,
    pending: Vec<MinMax>,
}

impl StreamingEnvelope {
    /// 新建包络。`channels == 0` 按 1 处理，`points == 0` 按 1 处理。
    #[must_use]
    pub fn new(channels: usize, points: usize) -> Self {
        let channels = channels.max(1);
        let points = points.max(1);
        Self {
            channels,
            points,
            max_buckets: points.saturating_mul(2).max(2),
            frames_per_bucket: 1,
            pending_frames: 0,
            buckets: vec![Vec::new(); channels],
            pending: vec![MinMax::default(); channels],
        }
    }

    /// 声道数。
    #[must_use]
    pub const fn channels(&self) -> usize {
        self.channels
    }

    /// 目标列数（每声道）。
    #[must_use]
    pub const fn points(&self) -> usize {
        self.points
    }

    /// 当前内存里的桶数（所有声道之和），供测试与诊断。
    #[must_use]
    pub fn bucket_count(&self) -> usize {
        self.buckets.iter().map(Vec::len).sum()
    }

    /// 喂入一块**交错**样本（`L0,R0,L1,R1,…`）。
    ///
    /// 非有限样本按 0 处理，与 `auralis-dsp` 的清理策略一致。
    pub fn push_interleaved(&mut self, samples: &[f32]) {
        let channels = self.channels;
        let frames = samples.len() / channels;
        if frames == 0 {
            return;
        }
        for f in 0..frames {
            let base = f * channels;
            for c in 0..channels {
                let raw = samples[base + c];
                let v = if raw.is_finite() { raw } else { 0.0 };
                if self.pending_frames == 0 {
                    self.pending[c] = MinMax { min: v, max: v };
                } else {
                    self.pending[c].min = self.pending[c].min.min(v);
                    self.pending[c].max = self.pending[c].max.max(v);
                }
            }
            self.pending_frames += 1;
            if self.pending_frames == self.frames_per_bucket {
                for c in 0..channels {
                    self.buckets[c].push(self.pending[c]);
                    self.pending[c] = MinMax::default();
                }
                self.pending_frames = 0;
                if self.buckets[0].len() > self.max_buckets {
                    self.halve();
                }
            }
        }
    }

    /// 结束并渲染为 `points` 列（每声道）。
    ///
    /// 返回值按声道分组：`out[ch][column]`。列数恒为 `points`；文件比
    /// `points` 帧还短时相邻列会重复同一个桶（宁可重复，也不伪造细节）。
    #[must_use]
    pub fn finish(mut self) -> Vec<Vec<MinMax>> {
        if self.pending_frames > 0 {
            for c in 0..self.channels {
                self.buckets[c].push(self.pending[c]);
                self.pending[c] = MinMax::default();
            }
            self.pending_frames = 0;
        }
        self.render()
    }

    fn render(&self) -> Vec<Vec<MinMax>> {
        let points = self.points;
        let mut out = Vec::with_capacity(self.channels);
        for ch in 0..self.channels {
            let src = &self.buckets[ch];
            if src.is_empty() {
                out.push(vec![MinMax::default(); points]);
                continue;
            }
            let b = src.len();
            let mut col = Vec::with_capacity(points);
            for i in 0..points {
                let lo = i * b / points;
                let hi = (((i + 1) * b / points).max(lo + 1)).min(b);
                col.push(
                    src[lo..hi]
                        .iter()
                        .copied()
                        .reduce(MinMax::merge)
                        .unwrap_or_default(),
                );
            }
            out.push(col);
        }
        out
    }

    fn halve(&mut self) {
        for ch in 0..self.channels {
            let src = std::mem::take(&mut self.buckets[ch]);
            let mut dst = Vec::with_capacity(src.len() / 2 + 1);
            let mut i = 0;
            while i + 1 < src.len() {
                dst.push(src[i].merge(src[i + 1]));
                i += 2;
            }
            if i < src.len() {
                dst.push(src[i]);
            }
            self.buckets[ch] = dst;
        }
        self.frames_per_bucket = self.frames_per_bucket.saturating_mul(2).max(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_both_polarity_and_transients_over_a_long_timeline() {
        let frames = 1_000_000usize;
        let mut env = StreamingEnvelope::new(1, 512);
        let mut block = vec![0.0_f32; 4096];
        for start in (0..frames).step_by(block.len()) {
            let n = block.len().min(frames - start);
            for (i, slot) in block[..n].iter_mut().enumerate() {
                let idx = start + i;
                *slot = if idx == 123_456 {
                    1.0
                } else if idx == 0 {
                    -0.75
                } else {
                    0.1
                };
            }
            env.push_interleaved(&block[..n]);
        }
        let out = env.finish();
        let col = &out[0];
        assert_eq!(col.len(), 512);
        let global_min = col.iter().map(|m| m.min).fold(f32::INFINITY, f32::min);
        let global_max = col.iter().map(|m| m.max).fold(f32::NEG_INFINITY, f32::max);
        assert!((global_min - (-0.75)).abs() < 1e-6, "首样本的负峰不能丢");
        assert!((global_max - 1.0).abs() < 1e-6, "中段瞬态不能丢");
        // 第一列必须覆盖文件开头，最后一列必须覆盖文件结尾。
        assert!((col[0].min - (-0.75)).abs() < 1e-6);
        assert!(col[511].max > 0.0);
    }

    #[test]
    fn memory_stays_bounded_regardless_of_length() {
        let mut env = StreamingEnvelope::new(2, 1024);
        for _ in 0..200 {
            let block = vec![0.25_f32; 4096 * 2];
            env.push_interleaved(&block);
            assert!(
                env.bucket_count() <= 2 * (2 * 1024 + 1),
                "桶数必须与文件长度无关，实得 {}",
                env.bucket_count()
            );
        }
        let out = env.finish();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].len(), 1024);
        assert_eq!(out[1].len(), 1024);
    }

    #[test]
    fn channels_are_kept_separate() {
        let mut env = StreamingEnvelope::new(2, 8);
        let mut interleaved = Vec::new();
        for i in 0..64 {
            interleaved.push(1.0_f32);
            interleaved.push(if i < 32 { -1.0 } else { 0.5 });
        }
        env.push_interleaved(&interleaved);
        let out = env.finish();
        assert!(out[0].iter().all(|m| m.min == 1.0 && m.max == 1.0));
        assert_eq!(out[1][0].min, -1.0);
        assert_eq!(out[1][7].max, 0.5);
    }

    #[test]
    fn short_input_still_renders_requested_columns() {
        let mut env = StreamingEnvelope::new(1, 64);
        env.push_interleaved(&[0.5, -0.5, 0.25]);
        let out = env.finish();
        assert_eq!(out[0].len(), 64);
        let min = out[0].iter().map(|m| m.min).fold(f32::INFINITY, f32::min);
        let max = out[0]
            .iter()
            .map(|m| m.max)
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!(min, -0.5);
        assert_eq!(max, 0.5);
    }

    #[test]
    fn non_finite_samples_are_sanitized() {
        let mut env = StreamingEnvelope::new(1, 2);
        env.push_interleaved(&[f32::NAN, f32::INFINITY, 0.5]);
        let out = env.finish();
        assert!(
            out[0]
                .iter()
                .all(|m| m.min.is_finite() && m.max.is_finite())
        );
        assert_eq!(out[0][1].max, 0.5);
    }
}
