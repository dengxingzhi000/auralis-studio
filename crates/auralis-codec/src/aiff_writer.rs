//! 测试用 AIFF 生成器。
//!
//! ## 为什么手写
//!
//! 与 `wav_writer` 同一个理由：`TEST_PLAN.md` §2 要求测试样本可复现。
//! AIFF 是 PCM 容器，结构比 WAV 更简单（IFF 的 `FORM` + 两个块），
//! 按规范写出来，测试就同时验证了「解码器正确」与「我们理解 AIFF 规范」。
//!
//! ## 与 WAV 的三处关键差异（容易写错）
//!
//! 1. **大端**：WAV 是小端，AIFF 全部字段与样本都是 big-endian。
//! 2. **采样率是 80 位 IEEE 754 extended**（10 字节），不是整数。
//! 3. **`SSND` 前面有 8 字节**（`offset` + `blockSize`），
//!    漏掉会让解码器把前 8 字节当成音频。
//!
//! ## 局限（如实记录）
//!
//! 本机没有 ffmpeg / sox 等第三方工具，因此样本由**本文件的 writer** 生成，
//! 而不是「行业工具导出的真实文件」。验证仍有意义：writer 与 symphonia 的
//! AIFF 读取器是两份独立实现，写错的地方不会互相抵消。但它**不是**
//! 「从现实世界拿到的文件」级别的证据，文档里已按此措辞。

use std::io::{BufWriter, Write};
use std::path::Path;

/// `COMM` 块长度：PCM（非 AIFC）固定 18 字节。
const COMM_CHUNK_SIZE: u32 = 18;

/// 把 u32 采样率编码成 80 位 extended float（AIFF 的专属写法）。
///
/// 编码规则：`value = mantissa × 2^(exponent-16383)`，其中 mantissa 的最高位
/// （bit 63）是整数位，隐含为 1。采样率都是正整数，因此符号位恒为 0。
#[must_use]
pub fn sample_rate_extended(rate: u32) -> [u8; 10] {
    assert!(rate > 0, "AIFF 不支持 0 采样率");

    // 有效位数：44100 -> 16 位
    let bits = 32 - rate.leading_zeros();
    // 把整数放到 64 位尾数的最高位，剩下的左移对齐
    let mantissa = u64::from(rate) << (64 - bits);
    // 指数偏置 16383；value 的指数是 bits-1
    let exponent = u16::try_from(bits - 1 + 16_383).expect("exponent fits u16");

    let mut out = [0u8; 10];
    out[0..2].copy_from_slice(&exponent.to_be_bytes());
    out[2..10].copy_from_slice(&mantissa.to_be_bytes());
    out
}

/// 写一个 16-bit PCM AIFF 文件。
///
/// 样本按**交错**顺序给出（与 `wav_writer` 相同的约定）。
///
/// # Panics
/// 声道数为 0、样本数不是声道数整数倍，或写盘失败 —— 只在测试中使用。
pub fn write_aiff_s16(path: &Path, sample_rate: u32, channels: u16, samples: &[i16]) {
    assert!(channels > 0, "AIFF 声道数必须大于 0");
    assert!(
        samples.len().is_multiple_of(usize::from(channels)),
        "样本数 {} 不是声道数 {} 的整数倍",
        samples.len(),
        channels
    );

    const BITS_PER_SAMPLE: u16 = 16;
    let bytes_per_sample: u32 = 2;
    let sample_frames =
        u32::try_from(samples.len() / usize::from(channels)).expect("frames fit u32");
    let data_bytes = u32::try_from(samples.len())
        .expect("samples fit u32")
        .checked_mul(bytes_per_sample)
        .expect("data size fits u32");

    // SSND 的 8 字节是 offset + blockSize，不是音频
    let ssnd_size = 8u32.checked_add(data_bytes).expect("ssnd size fits u32");
    // FORM 的长度是「文件总长 - 8」（不含 FORM 自身 8 字节 + 长度字段）
    let form_size = 4  // "AIFF"
        + (8 + COMM_CHUNK_SIZE)  // "COMM" + 长度 + 载荷
        + (8 + ssnd_size); // "SSND" + 长度 + 载荷

    let file = std::fs::File::create(path).expect("create aiff");
    let mut w = BufWriter::new(file);

    // --- FORM ---
    w.write_all(b"FORM").expect("write");
    w.write_all(&form_size.to_be_bytes()).expect("write");
    w.write_all(b"AIFF").expect("write");

    // --- COMM ---
    w.write_all(b"COMM").expect("write");
    w.write_all(&COMM_CHUNK_SIZE.to_be_bytes()).expect("write");
    w.write_all(&channels.to_be_bytes()).expect("write");
    w.write_all(&sample_frames.to_be_bytes()).expect("write");
    w.write_all(&BITS_PER_SAMPLE.to_be_bytes()).expect("write");
    w.write_all(&sample_rate_extended(sample_rate))
        .expect("write");

    // --- SSND ---
    w.write_all(b"SSND").expect("write");
    w.write_all(&ssnd_size.to_be_bytes()).expect("write");
    w.write_all(&0u32.to_be_bytes()).expect("write"); // offset
    w.write_all(&0u32.to_be_bytes()).expect("write"); // blockSize
    for s in samples {
        w.write_all(&s.to_be_bytes()).expect("write");
    }

    w.flush().expect("flush");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_rate_extended_matches_the_spec() {
        // 44100 = 0xAC44 -> 指数 15+16383=0x400E，尾数 0xAC44_0000_0000_0000。
        // 这组字节是按 AIFF 规范手算的期望值，不是从本实现反推的。
        assert_eq!(
            sample_rate_extended(44_100),
            [0x40, 0x0E, 0xAC, 0x44, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
        );
        // 48000 = 0xBB80 -> 0x400E_BB80_0000_0000_0000
        assert_eq!(
            sample_rate_extended(48_000),
            [0x40, 0x0E, 0xBB, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
        );
        // 8000 = 0x1F40（13 位）-> 指数 12+16383=0x400B，
        // 尾数是 0x1F40 << (63-12) = 0xFA00_0000_0000_0000。
        assert_eq!(
            sample_rate_extended(8_000),
            [0x40, 0x0B, 0xFA, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn generated_aiff_has_correct_header() {
        let dir = std::env::temp_dir().join("auralis-aiff-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("probe.aiff");

        let samples = crate::wav_writer::sine_i16(1_000.0, 0.5, 48_000, 4_800);
        write_aiff_s16(&path, 48_000, 1, &samples);

        let bytes = std::fs::read(&path).unwrap();
        let u16at = |o: usize| u16::from_be_bytes([bytes[o], bytes[o + 1]]);
        let u32at =
            |o: usize| u32::from_be_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);

        assert_eq!(&bytes[0..4], b"FORM");
        assert_eq!(&bytes[8..12], b"AIFF");
        assert_eq!(u32at(4) as usize, bytes.len() - 8, "FORM 长度 = 文件长 - 8");

        assert_eq!(&bytes[12..16], b"COMM");
        assert_eq!(u32at(16), 18, "PCM 的 COMM 载荷固定 18 字节");
        assert_eq!(u16at(20), 1, "声道数");
        assert_eq!(u32at(22), 4_800, "numSampleFrames 是帧数不是样本数");
        assert_eq!(u16at(26), 16, "sampleSize 是每样本位数");
        assert_eq!(
            &bytes[28..38],
            &sample_rate_extended(48_000),
            "采样率 80 位 extended"
        );

        assert_eq!(&bytes[38..42], b"SSND");
        assert_eq!(u32at(42), 8 + 4_800 * 2, "SSND 长度 = 8 + 数据字节");
        assert_eq!(u32at(46), 0, "offset 必须为 0，否则解码器会跳过头部");
        assert_eq!(u32at(50), 0, "blockSize");
        assert_eq!(bytes.len(), 54 + 4_800 * 2, "总长度");

        // 样本是大端：第一个样本与生成器给出的值一致
        let first = i16::from_be_bytes([bytes[54], bytes[55]]);
        assert_eq!(first, samples[0]);

        std::fs::remove_file(&path).ok();
    }
}
