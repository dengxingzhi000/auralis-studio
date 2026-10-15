//! IPC 载荷实测 —— ADR-018 的决策依据。
//!
//! ## 为什么要做这个测量
//!
//! ADR-018 最初断言「JSON 序列化会让 60 FPS 频谱变成几十 MB/s」。
//! **这个断言是错的**，本文件的实测数据推翻了它：
//!
//! ```text
//! 二进制    : 12.5 KB/帧  ->  0.7 MB/s @60FPS
//! JSON 数组 : 34.8 KB/帧  ->  2.0 MB/s @60FPS
//! 倍数      : 2.8x（不是 10x+）
//! ```
//!
//! 真实结论：**带宽不是瓶颈，序列化的 CPU 才是**。JSON 路径每帧要做
//! 3200 次浮点转字符串 + 解析，纯 CPU 开销远大于 2.8 倍的带宽差。
//! 因此二进制传输的收益主要在 CPU 而非带宽。
//!
//! 这个修正记录在 ADR-018 的「实测修正」一节。

#[cfg(test)]
mod tests {
    use crate::commands::analyze_synthetic_signal;

    /// 把 f32 序列序列化成 JSON 数组（**反例**：ADR-018 禁止的做法）。
    ///
    /// 写成最朴素的形式：serde_json 对 f32 的输出同样是每个数 8~20 字节
    /// 的浮点文本，不会有本质差别。
    fn json_f32_array(v: &[f32]) -> String {
        let mut s = String::from("[");
        for (i, x) in v.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            if x.is_finite() {
                s.push_str(&format!("{x}"));
            } else {
                s.push_str("null");
            }
        }
        s.push(']');
        s
    }

    fn decode(bytes: &[u8]) -> Vec<f32> {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect()
    }

    fn human(n: usize) -> String {
        if n >= 1024 * 1024 {
            format!("{:.2} MB", n as f64 / 1024.0 / 1024.0)
        } else if n >= 1024 {
            format!("{:.1} KB", n as f64 / 1024.0)
        } else {
            format!("{n} B")
        }
    }

    #[test]
    fn ipc_payload_size_binary_vs_json() {
        // 典型场景：4096 点 FFT，降采样到 1200 bins（≈1200px 宽画布），
        // 波形包络 2000 点。
        let (fft_size, target_bins, envelope_points) = (4096_usize, 1200_usize, 2000_usize);

        let r =
            analyze_synthetic_signal(1000.0, 0.5, 48_000, fft_size, target_bins, envelope_points)
                .expect("analysis should succeed");

        let bin_bytes = r.spectrum_db.len() + r.waveform_envelope.len();

        let db = decode(&r.spectrum_db);
        let env = decode(&r.waveform_envelope);
        let json_bytes = json_f32_array(&db).len() + json_f32_array(&env).len();

        let ratio = json_bytes as f64 / bin_bytes as f64;

        println!("\n=== ADR-018 IPC 载荷实测 ===");
        println!(
            "  输入     : fft_size={fft_size}, target_bins={target_bins}, envelope={envelope_points}"
        );
        println!("  二进制   : {}", human(bin_bytes));
        println!("  JSON 数组: {}   <- ADR-018 禁止的做法", human(json_bytes));
        println!("  倍数     : JSON / binary = {ratio:.1}x");
        println!();
        println!("  按 60 FPS 换算：");
        println!(
            "    二进制 -> {:.1} MB/s",
            bin_bytes as f64 * 60.0 / 1024.0 / 1024.0
        );
        println!(
            "    JSON   -> {:.1} MB/s",
            json_bytes as f64 * 60.0 / 1024.0 / 1024.0
        );
        println!();
        println!("  结论：带宽差约 3x，**不是** ADR-018 初稿所说的数量级。");
        println!("        真正的成本是每帧 3200 次浮点<->字符串转换的 CPU。");

        // 1. 二进制必须确实小于 JSON —— 这是规则 2 的量化下限。
        //    实测约 2.8x。低于 2x 意味着设计前提失效，必须重新评估。
        assert!(
            ratio >= 2.0,
            "JSON 只比二进制大 {ratio:.1}x，二进制传输的前提不成立，需重新评估"
        );

        // 2. 二进制载荷必须与元素数严格对应，零额外开销
        assert_eq!(bin_bytes, (target_bins + envelope_points) * 4);

        // 3. 单帧载荷不应大到阻塞 IPC
        assert!(
            bin_bytes < 1_700_000,
            "单帧 {} 过大，60 FPS 下会成为瓶颈",
            human(bin_bytes)
        );
    }

    #[test]
    fn ipc_payload_is_decoupled_from_fft_size() {
        // ADR-018 规则 3：载荷应随**像素宽度**增长，而非 FFT 长度。
        //
        // 注意 target_bins / envelope_points 必须小于最小 fft 的可用 bin 数，
        // 否则会被 clamp（见 commands.rs 的 .clamp(1, half)），那样比较的就
        // 不是同一件事了。fft=1024 时 half=513，所以取 400。
        let (bins, points) = (400_usize, 500_usize);
        let small = analyze_synthetic_signal(1000.0, 0.5, 48_000, 1024, bins, points).unwrap();
        let large = analyze_synthetic_signal(1000.0, 0.5, 48_000, 16384, bins, points).unwrap();

        let small_bytes = small.spectrum_db.len() + small.waveform_envelope.len();
        let large_bytes = large.spectrum_db.len() + large.waveform_envelope.len();

        println!(
            "\n=== 载荷与 FFT 分辨率解耦 ===\n  fft=1024  -> {}\n  fft=16384 -> {}",
            human(small_bytes),
            human(large_bytes)
        );

        assert_eq!(
            small_bytes, large_bytes,
            "FFT 长度变化 16 倍，视图载荷必须不变（降采样已在 Rust 侧完成）"
        );
        assert_eq!(small_bytes, (bins + points) * 4);

        // 反证：若跳过降采样，载荷会随 fft_size 线性增长
        let naive_bins = 16384 / 2 + 1;
        assert!(
            small_bytes < naive_bins * 4,
            "降采样后 {} 应远小于未降采样的 {naive_bins} bin",
            human(small_bytes)
        );
    }

    #[test]
    fn clamping_is_documented_and_bounded() {
        // target_bins 超过可用 bin 数时会被 clamp，而不是 panic 或截断数据
        let r = analyze_synthetic_signal(1000.0, 0.5, 48_000, 1024, 100_000, 64).unwrap();
        assert_eq!(r.spectrum_bins, 1024 / 2 + 1, "应钳到 half");
        assert_eq!(r.spectrum_db.len(), r.spectrum_bins * 4);

        // envelope_points 超过 fft_size 时同样钳位
        let r2 = analyze_synthetic_signal(1000.0, 0.5, 48_000, 1024, 64, 99_999).unwrap();
        assert_eq!(r2.waveform_points, 1024);
        assert_eq!(r2.waveform_envelope.len(), 1024 * 4);
    }
}
