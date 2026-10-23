//! FLAC 编码器（**仅测试用**）。
//!
//! ## 为什么需要它
//!
//! FLAC 的「签入样本验证」要求仓库里有一份可复现的 FLAC 文件。
//! 手上只有解码器（symphonia）时，唯一能造出 FLAC 的办法就是带一个编码器。
//! `flacenc` 只作为 dev-dependency 出现，**不进产物**。
//!
//! WAV 不需要这一步：`wav_writer` 是手写的（见同目录）。
//!
//! ## 确定性
//!
//! 同样的输入必然得到同样的字节：编码器配置是固定的常量，
//! 输入由 `sine_i16` 之类的确定性函数生成。测试据此比较
//! 「仓库里的样本」与「现生成的样本」是否等价。

use std::path::Path;

/// 把 16-bit PCM 交错样本写成 FLAC 文件。
///
/// # Errors
/// 编码器配置非法、编码失败或写盘失败。
pub fn write_flac_s16(
    path: &Path,
    sample_rate: u32,
    channels: u16,
    samples: &[i16],
) -> Result<(), String> {
    use flacenc::bitsink::ByteSink;
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;

    // FLAC 的样本是 i32（位深由 STREAMINFO 声明），这里按 16 bit 原样放大。
    let interleaved: Vec<i32> = samples.iter().map(|s| i32::from(*s)).collect();

    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|e| format!("flac encoder config invalid: {e:?}"))?;

    let source = flacenc::source::MemSource::from_samples(
        &interleaved,
        usize::from(channels),
        16,
        sample_rate as usize,
    );

    let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
        .map_err(|e| format!("flac encode failed: {e:?}"))?;

    let mut sink = ByteSink::new();
    stream
        .write(&mut sink)
        .map_err(|e| format!("flac stream write failed: {e:?}"))?;

    std::fs::write(path, sink.as_slice()).map_err(|e| format!("write {}: {e}", path.display()))
}

/// 生成仓库里那份 FLAC 样本（`tests/fixtures/`）。
///
/// 路径相对于 crate 根目录。
///
/// # Errors
/// 写盘失败。
pub fn write_fixture(
    relative: &str,
    sample_rate: u32,
    channels: u16,
    samples: &[i16],
) -> Result<(), String> {
    let path = crate::fixture_path(relative);
    write_flac_s16(&path, sample_rate, channels, samples)
}
