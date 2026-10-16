//! 测试用 WAV 生成器。
//!
//! ## 为什么自己生成而不是带样本文件
//!
//! `TEST_PLAN.md` §2 要求「测试样本、生成脚本和预期值应纳入仓库」。
//! WAV 是**已知格式**：44 字节头 + PCM 数据。我们按规范生成，
//! 因此测试同时验证了「解码器正确」与「我们理解 WAV 规范」两件事。
//!
//! 若用外部样本文件，测试只能验证前者，而且样本的来源/许可会成为问题。
//!
//! 用途：为集成测试提供**真实可解码**的 WAV 字节流，
//! 从而让 `auralis-codec` 的格式支持从「未验证」变成「已验证」。

use std::io::{BufWriter, Write};
use std::path::Path;

/// 写一个 16-bit PCM WAV 文件。
///
/// # Panics
/// 写入失败时 panic —— 只在测试中使用。
pub fn write_wav_s16(path: &Path, sample_rate: u32, channels: u16, samples: &[i16]) {
    // WAV fmt chunk 有两个**不同**的量，混淆会写出一个「看起来对」但无法解码的文件：
    //   bits_per_sample  = 16  (每个样本多少**位**)
    //   bytes_per_sample = 2   (每个样本多少**字节**)
    // block_align = channels * bytes_per_sample
    const BITS_PER_SAMPLE: u16 = 16;
    let bytes_per_sample: u16 = 2;
    let block_align = channels * bytes_per_sample;
    let byte_rate = sample_rate * u32::from(block_align);
    let data_bytes =
        u32::try_from(samples.len() * usize::from(bytes_per_sample)).expect("wav data fits u32");

    let file = std::fs::File::create(path).expect("create wav");
    let mut w = BufWriter::new(file);

    // --- RIFF 头 ---
    w.write_all(b"RIFF").expect("write");
    w.write_all(&(36 + data_bytes).to_le_bytes())
        .expect("write");
    w.write_all(b"WAVE").expect("write");

    // --- fmt 子块 ---
    w.write_all(b"fmt ").expect("write");
    w.write_all(&16u32.to_le_bytes()).expect("write"); // 子块大小 (PCM)
    w.write_all(&1u16.to_le_bytes()).expect("write"); // 格式 = PCM
    w.write_all(&channels.to_le_bytes()).expect("write");
    w.write_all(&sample_rate.to_le_bytes()).expect("write");
    w.write_all(&byte_rate.to_le_bytes()).expect("write");
    w.write_all(&block_align.to_le_bytes()).expect("write");
    w.write_all(&BITS_PER_SAMPLE.to_le_bytes()).expect("write");

    // --- data 子块 ---
    w.write_all(b"data").expect("write");
    w.write_all(&data_bytes.to_le_bytes()).expect("write");
    for s in samples {
        w.write_all(&s.to_le_bytes()).expect("write");
    }
    // data_bytes 由每个样本 2 字节算出，与上面的 bytes_per_sample 保持一致

    w.flush().expect("flush");
}

/// 生成一个整周期相干正弦的 i16 样本序列。
#[must_use]
pub fn sine_i16(freq_hz: f64, amplitude: f32, sample_rate: u32, frames: usize) -> Vec<i16> {
    (0..frames)
        .map(|i| {
            let phase = std::f64::consts::TAU * freq_hz * i as f64 / f64::from(sample_rate);
            let v = f64::from(amplitude) * phase.sin() * f64::from(i16::MAX);
            v.round().clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
        })
        .collect()
}

/// 立体声版本：左声道 X Hz，右声道 0.3 倍幅值的 1.5 倍频率。
#[must_use]
pub fn stereo_tone_i16(sample_rate: u32, frames: usize) -> Vec<i16> {
    let left = sine_i16(1_000.0, 0.5, sample_rate, frames);
    let right = sine_i16(1_500.0, 0.15, sample_rate, frames);
    left.into_iter()
        .zip(right)
        .flat_map(|(l, r)| [l, r])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_wav_has_correct_header() {
        let dir = std::env::temp_dir().join("auralis-wav-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("probe.wav");

        let samples = sine_i16(1_000.0, 0.5, 48_000, 4_800);
        write_wav_s16(&path, 48_000, 1, &samples);

        let bytes = std::fs::read(&path).unwrap();
        let u16at = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]);
        let u32at =
            |o: usize| u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);

        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(u32at(16), 16, "fmt 子块应为 16 字节");
        assert_eq!(u16at(20), 1, "audio format 应为 PCM(1)");
        assert_eq!(u16at(22), 1, "声道数");
        assert_eq!(u32at(24), 48_000, "采样率");
        assert_eq!(u32at(28), 96_000, "byte rate = 48000 * 2 * 1");
        assert_eq!(u16at(32), 2, "block align = channels * bytes_per_sample");
        // 这一项曾经写错：误用了 bytes_per_sample(2) 而非 bits_per_sample(16)，
        // 导致 symphonia 报 "bits per sample must be 8, 16, 24 or 32"。
        assert_eq!(
            u16at(34),
            16,
            "bits per sample 必须是 16，不能写成每样本字节数"
        );
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(
            u32at(40),
            u32::try_from(samples.len() * 2).unwrap(),
            "data 子块长度"
        );
        assert_eq!(bytes.len(), 44 + samples.len() * 2, "总长度");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn stereo_generator_interleaves_channels() {
        let frames = 100;
        let stereo = stereo_tone_i16(48_000, frames);
        assert_eq!(stereo.len(), frames * 2, "立体声每帧 2 个样本");
    }
}
