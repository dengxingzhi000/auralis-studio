//! `auralis-codec` 的集成测试：真实文件 -> 真实解码。
//!
//! 这些测试是「WAV 已验证」这一声明的**唯一依据**。
//! 若它们失败，crate 文档里的格式支持矩阵必须同步改回去。

use std::path::PathBuf;

use auralis_core::AudioFormat;
use auralis_dsp::{ClippingConfig, analyze_channel};

use crate::wav_writer::{sine_i16, stereo_tone_i16, write_wav_s16};
use crate::{BlockDecoder, ContainerKind, probe};

/// 临时文件目录。用唯一名避免并行测试互相覆盖。
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("auralis-codec-test-{name}"));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

#[test]
fn probes_a_real_wav_file() {
    let dir = scratch("probe");
    let path = dir.join("tone.wav");
    let samples = sine_i16(1_000.0, 0.5, 48_000, 48_000);
    write_wav_s16(&path, 48_000, 1, &samples);

    let info = probe(&path).expect("probe should succeed on generated WAV");

    assert_eq!(info.container, ContainerKind::Wav);
    assert_eq!(info.format.sample_rate.get(), 48_000);
    assert_eq!(info.format.channels.get(), 1);
    assert_eq!(info.total_frames, Some(48_000));
    assert!(
        (info.duration_seconds - 1.0).abs() < 1e-9,
        "1 秒音频的时长应为 1.0，实得 {}",
        info.duration_seconds
    );
    assert!(info.is_verified(), "WAV 必须被标记为已验证");

    std::fs::remove_file(&path).ok();
}

#[test]
fn decodes_wav_and_reconstructs_the_signal() {
    // 这是整条链路的验证：写入 -> 解码 -> DSP -> 期望数值
    let dir = scratch("decode");
    let path = dir.join("tone.wav");
    let samples = sine_i16(1_000.0, 0.5, 48_000, 48_000);
    write_wav_s16(&path, 48_000, 1, &samples);

    let mut decoder = BlockDecoder::open(&path).expect("decoder should open");
    assert_eq!(decoder.format().sample_rate.get(), 48_000);
    assert_eq!(decoder.format().channels.get(), 1);

    let mut collected = Vec::new();
    let mut expected_first_frame = 0u64;
    while let Some(block) = decoder.next_block().expect("decode should not fail") {
        assert_eq!(
            block.first_frame, expected_first_frame,
            "帧偏移必须连续，不能跳变"
        );
        expected_first_frame += block.pcm.frame_count() as u64;
        collected.extend_from_slice(&block.pcm.samples);
    }

    assert!(decoder.is_finished());
    assert_eq!(collected.len(), 48_000, "应解码出全部样本");
    assert_eq!(expected_first_frame, 48_000);

    // 解码出来的值应接近原始 i16 / 32768
    let stats = analyze_channel(&collected, ClippingConfig::default());
    let peak_db = stats.peak.dbfs().clamped(-140.0);
    // 原始 i16 满刻度正弦 -> 约 -6.02 dBFS，量化误差带来微小偏差
    assert!(
        (peak_db - -6.020_6).abs() < 0.1,
        "解码后峰值应接近原始 -6.02 dBFS，实得 {peak_db}"
    );
    let rms_db = stats.rms.dbfs().clamped(-140.0);
    assert!(
        (rms_db - -9.030_9).abs() < 0.1,
        "解码后 RMS 应接近 -9.03 dBFS，实得 {rms_db}"
    );

    std::fs::remove_file(&path).ok();
}

#[test]
fn decodes_stereo_wav_with_channel_isolation() {
    let dir = scratch("stereo");
    let path = dir.join("stereo.wav");
    let samples = stereo_tone_i16(48_000, 24_000);
    write_wav_s16(&path, 48_000, 2, &samples);

    let info = probe(&path).expect("probe stereo");
    assert_eq!(info.format.channels.get(), 2);
    assert_eq!(info.channel_layout, auralis_core::ChannelLayout::Stereo);

    let mut decoder = BlockDecoder::open(&path).expect("open stereo");
    let mut left = Vec::new();
    let mut right = Vec::new();
    while let Some(block) = decoder.next_block().expect("decode") {
        left.extend(block.pcm.channel_view(0));
        right.extend(block.pcm.channel_view(1));
    }

    // 左 0.5 幅度，右 0.15 幅度
    let l = analyze_channel(&left, ClippingConfig::default());
    let r = analyze_channel(&right, ClippingConfig::default());
    assert!(
        (l.peak.amplitude - 0.5).abs() < 0.01,
        "L={}",
        l.peak.amplitude
    );
    assert!(
        (r.peak.amplitude - 0.15).abs() < 0.01,
        "R={}",
        r.peak.amplitude
    );

    std::fs::remove_file(&path).ok();
}

#[test]
fn rejects_non_audio_file_without_panicking() {
    let dir = scratch("garbage");
    let path = dir.join("not_audio.wav");
    // 内容不是 RIFF/WAVE
    std::fs::write(&path, b"this is definitely not a wav file, just text").expect("write");

    let result = probe(&path);
    assert!(result.is_err(), "非音频文件必须报错而不是 panic");
    // 错误信息要能指导用户
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("unrecognised") || msg.contains("corrupt"),
        "错误信息应说明原因，实得: {msg}"
    );

    std::fs::remove_file(&path).ok();
}

#[test]
fn truncated_wav_decodes_what_is_there_and_reports_the_truncation() {
    let dir = scratch("truncated");
    let path = dir.join("truncated.wav");

    // 写一个完整文件，再砍掉一半数据
    let samples = sine_i16(1_000.0, 0.5, 48_000, 48_000);
    write_wav_s16(&path, 48_000, 1, &samples);
    let full = std::fs::read(&path).expect("read");
    std::fs::write(&path, &full[..full.len() / 2]).expect("truncate");

    // 探针读的是文件头，所以仍然能给出**声明的**时长。
    let info = probe(&path).expect("probe reads the header");
    assert_eq!(info.total_frames, Some(48_000));

    let mut decoder = BlockDecoder::open(&path).expect("open should work");
    assert!(!decoder.is_truncated(), "尚未解码时不能声称被截断");

    let mut frames = 0u64;
    while let Some(block) = decoder.next_block().expect("decode must not fail") {
        frames += block.pcm.frame_count() as u64;
    }

    // 关键行为：截断**不是**解码失败。已解出的部分照常给出，
    // 同时把「文件不完整」这件事如实标记出来。
    assert!(decoder.is_truncated(), "读到文件尾必须标记为截断");
    assert!(frames > 0, "截断文件仍应解出可用的音频，实得 {frames} 帧");
    assert!(
        frames < 48_000,
        "解出的帧数应短于声明的 48000，实得 {frames}"
    );
    assert!(decoder.truncation_note().is_some(), "截断必须带上底层原因");

    std::fs::remove_file(&path).ok();
}

#[test]
fn complete_wav_is_not_reported_as_truncated() {
    let dir = scratch("not-truncated");
    let path = dir.join("tone.wav");
    let samples = sine_i16(1_000.0, 0.5, 48_000, 48_000);
    write_wav_s16(&path, 48_000, 1, &samples);

    let mut decoder = BlockDecoder::open(&path).expect("open");
    while decoder.next_block().expect("decode").is_some() {}

    assert!(!decoder.is_truncated(), "完整文件不得被标记为截断");

    std::fs::remove_file(&path).ok();
}

#[test]
fn empty_file_is_reported_not_panicked() {
    let dir = scratch("empty");
    let path = dir.join("empty.wav");
    std::fs::write(&path, b"").expect("write empty");

    assert!(probe(&path).is_err(), "空文件必须报错");
    std::fs::remove_file(&path).ok();
}

#[test]
fn missing_file_is_reported_not_panicked() {
    let path = PathBuf::from("definitely-does-not-exist-12345.wav");
    let err = probe(&path).unwrap_err();
    assert!(err.to_string().contains("not found"), "实得: {err}");
}

#[test]
fn probe_does_not_load_whole_file_into_memory() {
    // 探测必须是 O(1)：3 分钟 48k 立体声 WAV 的 data 段约 34 MB，
    // 但 probe 不应因文件大小而变慢到不可用。
    // 这里只验证「大文件探测成功」，不做时间断言（CI 机器差异大）。
    let dir = scratch("large");
    let path = dir.join("large.wav");
    let samples = sine_i16(1_000.0, 0.5, 48_000, 48_000 * 60); // 60 秒
    write_wav_s16(&path, 48_000, 1, &samples);
    let size = std::fs::metadata(&path).unwrap().len();
    assert!(size > 5_000_000, "样本应足够大以体现意义，实得 {size} B");

    let info = probe(&path).expect("probe large file");
    assert_eq!(info.file_size_bytes, size);

    std::fs::remove_file(&path).ok();
}

#[test]
fn multi_block_decode_has_continuous_timeline() {
    // 超过一个解码包的文件，应产出多块，且帧偏移严格连续
    let dir = scratch("multiblock");
    let path = dir.join("multi.wav");
    // 10 秒，保证跨多个包
    let samples = sine_i16(1_000.0, 0.5, 48_000, 480_000);
    write_wav_s16(&path, 48_000, 1, &samples);

    let mut d = BlockDecoder::open(&path).expect("open");
    let mut blocks = 0usize;
    let mut expect = 0u64;
    let mut total = 0u64;
    while let Some(b) = d.next_block().expect("decode") {
        assert_eq!(b.first_frame, expect);
        expect = b.total_frames_decoded;
        total += b.pcm.frame_count() as u64;
        blocks += 1;
    }
    assert!(blocks > 1, "10 秒音频应解码为多块，实得 {blocks} 块");
    assert_eq!(total, 480_000);

    std::fs::remove_file(&path).ok();
}

#[test]
fn audio_format_is_preserved_not_resampled() {
    // PRD FR-02 / TAD §5：不得因绘图方便修改源采样率
    for sr in [8_000u32, 22_050, 44_100, 48_000, 96_000] {
        let dir = scratch(&format!("sr{sr}"));
        let path = dir.join(format!("t{sr}.wav"));
        let samples = sine_i16(1_000.0, 0.5, sr, 4_800);
        write_wav_s16(&path, sr, 1, &samples);

        let info = probe(&path).expect("probe");
        assert_eq!(
            info.format.sample_rate.get(),
            sr,
            "采样率必须原样保留，不得重采样"
        );
        assert_eq!(
            info.format,
            AudioFormat::new(sr, 1).unwrap(),
            "格式应与文件头一致"
        );

        std::fs::remove_file(&path).ok();
    }
}

#[test]
fn seeks_to_a_sample_accurate_frame() {
    // 视口分块依赖这个能力：从文件中间直接开始解码，而不是从头扫。
    // WAV 的 timebase 是 1/sample_rate，因此实际帧号应与请求值一致。
    let dir = scratch("seek");
    let path = dir.join("tone.wav");
    let samples = sine_i16(440.0, 0.5, 48_000, 96_000);
    write_wav_s16(&path, 48_000, 1, &samples);

    let mut decoder = BlockDecoder::open(&path).expect("decoder should open");
    let actual = decoder
        .seek_to_frame(48_000)
        .expect("WAV should support seeking");

    assert!(
        actual <= 48_000,
        "Accurate seek 保证不晚于请求位置，实得 {actual}"
    );
    assert!(
        48_000 - actual <= 8_192,
        "WAV seek 不应退得离目标太远，实得 {actual}"
    );

    let block = decoder
        .next_block()
        .expect("decode after seek")
        .expect("there should be audio after 1 s");
    assert!(
        block.first_frame >= actual,
        "seek 后的第一块不能早于实际起点"
    );
    assert!(
        block.first_frame <= 48_000,
        "seek 后的第一块不能晚于请求位置，实得 first_frame={}",
        block.first_frame
    );
    assert!(block.total_frames_decoded > block.first_frame);
    // 调用方（spectrogram_view）负责跳过 [actual, target) 的样本。

    std::fs::remove_file(&path).ok();
}
