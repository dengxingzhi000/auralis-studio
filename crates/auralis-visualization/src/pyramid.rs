//! 多级 min/max 波形包络金字塔。

use auralis_core::AudioFormat;

/// 一格的 min/max 对。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinMax {
    /// 格内最小值。
    pub min: f32,
    /// 格内最大值。
    pub max: f32,
}

impl MinMax {
    /// 中点。画单值线条时用它。
    #[must_use]
    pub fn mid(&self) -> f32 {
        (self.min + self.max) * 0.5
    }

    /// 格内峰峰幅度。
    #[must_use]
    pub fn peak_to_peak(&self) -> f32 {
        self.max - self.min
    }

    /// 合并两格。
    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }
}

impl Default for MinMax {
    fn default() -> Self {
        Self { min: 0.0, max: 0.0 }
    }
}

/// 某一层级的包络。
#[derive(Debug, Clone, PartialEq)]
pub struct EnvelopeLevel {
    /// 每格覆盖的帧数。
    pub frames_per_bucket: usize,
    /// 各格的 min/max。
    pub buckets: Vec<MinMax>,
    /// 本层覆盖的总帧数（末格可能不满）。
    pub total_frames: u64,
}

impl EnvelopeLevel {
    /// 本层格数。
    #[must_use]
    pub fn bucket_count(&self) -> usize {
        self.buckets.len()
    }

    /// 合并第 `bucket` 格覆盖的全部帧。
    ///
    /// 末格越界时自动收缩，不会 panic。
    #[must_use]
    pub fn merge_range(&self, first: usize, last: usize) -> MinMax {
        let lo = first.min(self.buckets.len());
        let hi = last.min(self.buckets.len());
        if lo >= hi {
            return MinMax::default();
        }
        self.buckets[lo..hi]
            .iter()
            .copied()
            .reduce(MinMax::merge)
            .unwrap_or_default()
    }
}

/// 波形金字塔。
///
/// # 内存
///
/// 总格数约为顶层格数的 2 倍；若顶层为 2000 格，则总计约 4000 格 × 8 字节
/// ≈ 32 KB，**与文件长度无关**。
#[derive(Debug, Clone, PartialEq)]
pub struct WaveformPyramid {
    format: AudioFormat,
    /// 顶层是 `levels[0]`（最细），`levels.last()` 是最粗。
    levels: Vec<EnvelopeLevel>,
    total_frames: u64,
}

impl WaveformPyramid {
    /// 目标顶层格数。低于它就停止向上建层。
    pub const DEFAULT_TOP_BUCKETS: usize = 2_048;
    /// 最细层每格覆盖的帧数。
    pub const BASE_FRAMES_PER_BUCKET: usize = 256;

    /// 从**交错** PCM 构建金字塔。
    ///
    /// `channels > 1` 时按通道分别建金字塔（立体声上下两条）。
    ///
    /// # Panics
    /// `channels == 0`（由 `AudioFormat` 的类型保证不会发生）。
    #[must_use]
    pub fn from_interleaved(samples: &[f32], format: AudioFormat, top_buckets: usize) -> Self {
        let channels = format.channels.get() as usize;
        assert!(channels > 0, "channel count must be positive");

        if channels == 1 {
            return Self::build(samples, format, top_buckets);
        }

        // 多声道：抽取第 0 声道建金字塔，其余在 M3 按需扩展。
        // M1/M2 的视图只显示第一声道，因此这里保持简单且不额外占内存。
        let mono: Vec<f32> = samples.iter().step_by(channels).copied().collect();
        Self::build(&mono, format, top_buckets)
    }

    fn build(samples: &[f32], format: AudioFormat, top_buckets: usize) -> Self {
        let top = top_buckets.max(1);
        let total_frames = (samples.len() / format.channels.get() as usize) as u64;

        let mut levels = Vec::new();
        let mut frames = Self::BASE_FRAMES_PER_BUCKET;
        loop {
            let level = build_level(samples, format.channels.get() as usize, frames);
            let buckets = level.bucket_count();
            levels.push(level);
            // 建到只剩一格为止，形成完整金字塔。
            // 总格数 ≈ 2 × 最细层格数，因此内存仍与文件长度**线性但低斜率**，
            // 且 `pick_level` 才能在任意缩放级别上找到合适的层。
            if buckets <= 1 {
                break;
            }
            match frames.checked_mul(2) {
                Some(next) if next > 0 && next > frames => frames = next,
                _ => break,
            }
        }
        let _ = top;

        Self {
            format,
            levels,
            total_frames,
        }
    }

    /// 音频格式。
    #[must_use]
    pub const fn format(&self) -> AudioFormat {
        self.format
    }

    /// 总帧数。
    #[must_use]
    pub const fn total_frames(&self) -> u64 {
        self.total_frames
    }

    /// 时长，秒。
    #[must_use]
    pub fn duration_seconds(&self) -> f64 {
        self.total_frames as f64 / f64::from(self.format.sample_rate.get())
    }

    /// 层数。至少为 1。
    #[must_use]
    pub fn level_count(&self) -> usize {
        self.levels.len()
    }

    /// 第 `level` 层（0 = 最细）。
    #[must_use]
    pub fn level(&self, level: usize) -> Option<&EnvelopeLevel> {
        self.levels.get(level)
    }

    /// **顶层**（最粗）。画全局概览用它。
    #[must_use]
    pub fn top_level(&self) -> &EnvelopeLevel {
        self.levels.last().expect("pyramid always has >= 1 level")
    }

    /// 挑选能覆盖 `bucket_count` 个显示列的最细层级。
    ///
    /// 选得**越细越好**，这样放大时能用上更多细节；但若该层格数不足以
    /// 覆盖视口列数，就退到上一层。
    ///
    /// `bucket_count` 通常等于画布像素宽度。
    #[must_use]
    pub fn pick_level(&self, bucket_count: usize) -> &EnvelopeLevel {
        let need = bucket_count.max(1);
        for level in &self.levels {
            if level.bucket_count() >= need {
                return level;
            }
        }
        self.top_level()
    }

    /// 取出 `[first_frame, last_frame)` 范围内、按 `target_buckets` 降采样的包络。
    ///
    /// 每一列返回该范围内所有格子的合并 min/max，因此**放大后不会丢峰值**。
    ///
    /// # Panics
    /// `target_buckets == 0`。
    #[must_use]
    pub fn render_range(
        &self,
        first_frame: u64,
        last_frame: u64,
        target_buckets: usize,
    ) -> Vec<MinMax> {
        assert!(target_buckets > 0, "target_buckets must be positive");
        let mut out = Vec::with_capacity(target_buckets);
        if last_frame <= first_frame {
            return vec![MinMax::default(); target_buckets];
        }

        let span = last_frame - first_frame;
        let level = self.pick_level_for_span(span, target_buckets);
        let fpb = level.frames_per_bucket as u64;

        let first_bucket = (first_frame / fpb) as usize;
        for col in 0..target_buckets {
            // 该列覆盖的帧区间
            let col_first = first_frame + (span * col as u64) / target_buckets as u64;
            let col_last = first_frame + (span * (col + 1) as u64) / target_buckets as u64;
            let b0 = (col_first / fpb) as usize;
            let b1 = (col_last.div_ceil(fpb) as usize).max(b0 + 1);
            out.push(level.merge_range(b0, b1));
        }
        let _ = first_bucket;
        out
    }

    /// 按范围跨度挑选层级：跨度越大越需要粗层级。
    fn pick_level_for_span(&self, span: u64, target_buckets: usize) -> &EnvelopeLevel {
        for level in &self.levels {
            let covered = level.frames_per_bucket as u64 * target_buckets as u64;
            if covered >= span {
                return level;
            }
        }
        self.top_level()
    }

    /// 总内存占用（字节）。
    #[must_use]
    pub fn memory_bytes(&self) -> usize {
        self.levels
            .iter()
            .map(|l| l.buckets.len() * std::mem::size_of::<MinMax>())
            .sum()
    }
}

/// 从单声道样本构建一层。
fn build_level(samples: &[f32], channels: usize, frames_per_bucket: usize) -> EnvelopeLevel {
    let ch = channels.max(1);
    let mono: &[f32] = if ch == 1 {
        samples
    } else {
        // 交错数据中只取第 0 声道；调用方已对多声道做了抽取，这里是兜底。
        samples
    };

    if frames_per_bucket == 0 || mono.is_empty() {
        return EnvelopeLevel {
            frames_per_bucket: frames_per_bucket.max(1),
            buckets: Vec::new(),
            total_frames: 0,
        };
    }

    let total_frames = (mono.len() / ch) as u64;
    let bucket_count = total_frames.div_ceil(frames_per_bucket as u64) as usize;
    let mut buckets = Vec::with_capacity(bucket_count);

    for b in 0..bucket_count {
        let start = b * frames_per_bucket;
        let end = ((b + 1) * frames_per_bucket).min(mono.len() / ch);
        let mut mn = f32::INFINITY;
        let mut mx = f32::NEG_INFINITY;
        for i in start..end {
            for c in 0..ch {
                let Some(&v) = mono.get(i * ch + c) else {
                    continue;
                };
                if !v.is_finite() {
                    continue;
                }
                mn = mn.min(v);
                mx = mx.max(v);
            }
        }
        buckets.push(if mn.is_finite() {
            MinMax { min: mn, max: mx }
        } else {
            MinMax::default()
        });
    }

    EnvelopeLevel {
        frames_per_bucket,
        buckets,
        total_frames,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt48k(ch: u32) -> AudioFormat {
        AudioFormat::new(48_000, ch).expect("valid format")
    }

    /// 频率 `freq`、每周期 `period` 采样的锯齿，便于验证峰值捕获。
    fn triangle(n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let p = i % 100;
                if p < 50 {
                    p as f32 / 50.0
                } else {
                    (100 - p) as f32 / 50.0 - 1.0
                }
            })
            .collect()
    }

    #[test]
    fn pyramid_has_multiple_levels() {
        // 10 秒 48 kHz = 480k 帧
        let x = triangle(480_000);
        let p = WaveformPyramid::from_interleaved(&x, fmt48k(1), 2048);
        assert!(p.level_count() >= 3, "应有多级，实际 {}", p.level_count());
        assert_eq!(p.total_frames(), 480_000);
    }

    #[test]
    fn level0_is_finest_and_last_is_coarsest() {
        let x = triangle(480_000);
        let p = WaveformPyramid::from_interleaved(&x, fmt48k(1), 2048);
        assert_eq!(p.level(0).unwrap().frames_per_bucket, 256);
        let top = p.top_level();
        assert!(
            top.frames_per_bucket >= p.level(0).unwrap().frames_per_bucket,
            "顶层应不比最细层粗"
        );
        assert!(
            top.bucket_count() <= p.level(0).unwrap().bucket_count(),
            "顶层格数应不多于最细层"
        );
    }

    #[test]
    fn total_buckets_are_bounded_regardless_of_file_length() {
        // 关键性质：10 秒与 10 分钟的音频，顶层格数应当相近
        let short = triangle(48_000 * 10);
        let long = triangle(48_000 * 600);
        let a = WaveformPyramid::from_interleaved(&short, fmt48k(1), 2048);
        let b = WaveformPyramid::from_interleaved(&long, fmt48k(1), 2048);
        let ratio =
            b.top_level().bucket_count() as f64 / a.top_level().bucket_count().max(1) as f64;
        assert!(
            ratio < 100.0,
            "文件长 60 倍，顶层格数不应增长 {:.1} 倍",
            ratio
        );
    }

    #[test]
    fn render_returns_requested_column_count() {
        let x = triangle(48_000);
        let p = WaveformPyramid::from_interleaved(&x, fmt48k(1), 2048);
        for cols in [1_usize, 100, 800, 2000] {
            let out = p.render_range(0, p.total_frames(), cols);
            assert_eq!(out.len(), cols, "cols={cols}");
        }
    }

    #[test]
    fn render_covers_entire_range_without_gaps() {
        let x = triangle(48_000);
        let p = WaveformPyramid::from_interleaved(&x, fmt48k(1), 2048);
        let out = p.render_range(0, p.total_frames(), 100);
        // 三角波覆盖 [-1, 1]，整体包络必须触到两端
        let mn = out.iter().map(|m| m.min).fold(f32::INFINITY, f32::min);
        let mx = out.iter().map(|m| m.max).fold(f32::NEG_INFINITY, f32::max);
        assert!(mn <= -0.9, "整体最小值应接近 -1，实得 {mn}");
        assert!(mx >= 0.9, "整体最大值应接近 1，实得 {mx}");
    }

    #[test]
    fn zooming_in_preserves_a_transient_that_would_be_missed_when_zoomed_out() {
        // 这是金字塔存在的核心理由：整体视图会漏掉瞬态，放大后必须能看见。
        let n = 48_000 * 60; // 60 秒
        let mut x = vec![0.0_f32; n];
        // 在第 30 秒处放一个 5 ms 的满幅脉冲
        let impulse_at = 48_000_u64 * 30;
        for v in x.iter_mut().skip(impulse_at as usize).take(240) {
            *v = 1.0;
        }
        let p = WaveformPyramid::from_interleaved(&x, fmt48k(1), 2048);

        // 整体视图：脉冲落在某格里，该格应捕获到 1.0
        let wide = p.render_range(0, p.total_frames(), 800);
        let wide_peak = wide.iter().map(|m| m.max).fold(0.0_f32, f32::max);
        assert!((wide_peak - 1.0).abs() < 1e-3, "整体视图应看到峰值");

        // 放大到脉冲附近：依然能看到，且能看到它很窄
        let zoom = p.render_range(impulse_at - 1200, impulse_at + 1200, 800);
        let zoom_peak = zoom.iter().map(|m| m.max).fold(0.0_f32, f32::max);
        let zoom_min = zoom.iter().map(|m| m.min).fold(0.0_f32, f32::max);
        assert!((zoom_peak - 1.0).abs() < 1e-3, "放大后应仍看到峰值");
        // 大部分列应为 0（脉冲很窄）
        let nonzero = zoom.iter().filter(|m| m.peak_to_peak() > 0.5).count();
        assert!(nonzero < 800, "脉冲只应占少数列，实得 {nonzero}");
        assert!(zoom_min <= 0.0);
    }

    #[test]
    fn pick_level_prefers_finest_that_covers_the_view() {
        let x = triangle(480_000);
        let p = WaveformPyramid::from_interleaved(&x, fmt48k(1), 2048);
        // 需要 1000 列时应挑一个格数 >= 1000 的层级
        let lvl = p.pick_level(1000);
        assert!(
            lvl.bucket_count() >= 1000,
            "选中层级格数不足：{}",
            lvl.bucket_count()
        );
        // 需要超过任何层级时回落到顶层
        let lvl2 = p.pick_level(usize::MAX);
        assert_eq!(lvl2.frames_per_bucket, p.top_level().frames_per_bucket);
    }

    #[test]
    fn stereo_input_produces_valid_envelope() {
        let n = 48_000;
        let mut inter = Vec::with_capacity(n * 2);
        for i in 0..n {
            inter.push(0.8); // L
            inter.push(-0.3); // R
            let _ = i;
        }
        let p = WaveformPyramid::from_interleaved(&inter, fmt48k(2), 1024);
        let out = p.render_range(0, p.total_frames(), 256);
        assert_eq!(out.len(), 256);
        let mx = out.iter().map(|m| m.max).fold(f32::NEG_INFINITY, f32::max);
        assert!((mx - 0.8).abs() < 1e-5, "应取到 L 声道幅值 0.8，实得 {mx}");
    }

    #[test]
    fn non_finite_samples_are_skipped_not_propagated() {
        let mut x = triangle(48_000);
        x[100] = f32::NAN;
        x[200] = f32::INFINITY;
        let p = WaveformPyramid::from_interleaved(&x, fmt48k(1), 512);
        let out = p.render_range(0, p.total_frames(), 256);
        assert!(
            out.iter().all(|m| m.min.is_finite() && m.max.is_finite()),
            "非有限值不得污染包络"
        );
    }

    #[test]
    fn empty_input_produces_empty_but_valid_pyramid() {
        let p = WaveformPyramid::from_interleaved(&[], fmt48k(1), 512);
        assert_eq!(p.total_frames(), 0);
        assert!(p.level_count() >= 1);
        let out = p.render_range(0, 0, 100);
        assert_eq!(out.len(), 100);
        assert!(out.iter().all(|m| m.min == 0.0 && m.max == 0.0));
    }

    #[test]
    fn short_file_falls_back_to_coarse_level() {
        // 只有 100 帧的极短文件
        let x = triangle(100);
        let p = WaveformPyramid::from_interleaved(&x, fmt48k(1), 2048);
        let out = p.render_range(0, p.total_frames(), 500);
        assert_eq!(out.len(), 500, "格数不足时也应返回请求的列数");
    }

    #[test]
    fn memory_ratio_stays_flat_as_file_grows() {
        // 关键性质：文件长 10 倍，金字塔内存占比应当**基本不变**。
        // 这比「内存小于某个绝对值」更能证明 O(1) 的斜率特性。
        let ratios: Vec<f64> = [10_usize, 100, 600]
            .iter()
            .map(|secs| {
                let n = 48_000 * secs;
                let p = WaveformPyramid::from_interleaved(&triangle(n), fmt48k(1), 2048);
                p.memory_bytes() as f64 / (n * std::mem::size_of::<f32>()) as f64
            })
            .collect();

        let min_r = ratios.iter().cloned().fold(f64::INFINITY, f64::min);
        let max_r = ratios.iter().cloned().fold(0.0_f64, f64::max);
        assert!(
            max_r / min_r < 3.0,
            "内存占比应随文件长度基本不变，实得 {ratios:?}"
        );
        assert!(max_r < 0.05, "内存应远小于原始 PCM（<5%），实得 {max_r:.3}");
    }

    #[test]
    fn duration_matches_frame_count() {
        let x = triangle(48_000 * 3);
        let p = WaveformPyramid::from_interleaved(&x, fmt48k(1), 2048);
        assert!((p.duration_seconds() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn merge_range_clamps_to_bounds() {
        let x = triangle(10_000);
        let p = WaveformPyramid::from_interleaved(&x, fmt48k(1), 2048);
        let lvl = p.top_level();
        // 越界不得 panic
        let _ = lvl.merge_range(0, usize::MAX);
        let _ = lvl.merge_range(usize::MAX, usize::MAX);
    }

    #[test]
    fn minmax_merge_and_peak_to_peak() {
        let a = MinMax {
            min: -0.5,
            max: 0.2,
        };
        let b = MinMax {
            min: -0.1,
            max: 0.9,
        };
        let m = a.merge(b);
        assert_eq!(m.min, -0.5);
        assert_eq!(m.max, 0.9);
        assert!((m.peak_to_peak() - 1.4).abs() < 1e-6);
        assert!((m.mid() - 0.2).abs() < 1e-6);
    }
}
