//! DSP 性能基准。
//!
//! ## 为什么有这个文件
//!
//! ADR-014 规定：「FFI 的引入门槛 = 实测性能数据表明 Rust 不足」。
//! M0 到 M3 一直没有基准，导致这条门槛**无法执行** —— 只能靠争论。
//! 这个文件把门槛变成可检查的事实。
//!
//! ## 怎么看结果
//!
//! `docs/TEST_PLAN.md` §6 要求记录：机器、CPU、编译 profile、输入规模、
//! 运行次数、均值与分位数。因此 criterion 的 HTML 报告
//! （`target/criterion/report/index.html`）是主要产物。
//!
//! 关注**分位数**而非只看均值：实时分析关心的是最坏情况，
//! 均值被削平后看不出卡顿风险。
//!
//! ## 与实时预算的关系
//!
//! 若采样率为 48 kHz，一次回调的预算约为 `1e6 / 48000 ≈ 20.8 µs`。
//! 下面 `fft_4096` 一项若显著超过这个数量级，就说明
//! 「回调里做 FFT」不可行，必须把分析移到工作线程
//! （这正是 ADR-018 的实时通道设计要解决的问题）。

use std::hint::black_box;

use auralis_dsp::{SpectrumAnalyzer, SpectrumConfig, Stft, StftConfig, WindowFunction};
use criterion::{Criterion, criterion_group, criterion_main};

fn sine(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| {
            let phase = std::f64::consts::TAU * 1_000.0 * i as f64 / 48_000.0;
            (0.5 * phase.sin()) as f32
        })
        .collect()
}

fn bench_fft(c: &mut Criterion) {
    let mut group = c.benchmark_group("fft");
    for &n in &[1024_usize, 4096, 16384] {
        let x = sine(n);
        group.bench_function(format!("fft_{n}"), |b| {
            b.iter(|| {
                let a = SpectrumAnalyzer::new(SpectrumConfig {
                    fft_size: n,
                    hop_size: n,
                    window: WindowFunction::Hann,
                    remove_dc: false,
                })
                .expect("valid config");
                black_box(a.analyze(black_box(&x), 48_000).expect("analyze"))
            })
        });
    }
    group.finish();
}

fn bench_peak_detection(c: &mut Criterion) {
    let n = 4096;
    let a = SpectrumAnalyzer::new(SpectrumConfig {
        fft_size: n,
        hop_size: n,
        window: WindowFunction::Hann,
        remove_dc: false,
    })
    .expect("valid config");
    let frame = a.analyze(&sine(n), 48_000).expect("analyze").remove(0);

    let mut group = c.benchmark_group("peak_detection");
    group.bench_function("detect_peaks_2049_bins", |b| {
        b.iter(|| {
            black_box(auralis_dsp::detect_peaks(
                black_box(&frame.amplitudes),
                black_box(11.718_75),
                8,
            ))
        })
    });
    group.finish();
}

fn bench_stft(c: &mut Criterion) {
    let x = sine(48_000); // 1 秒
    let mut group = c.benchmark_group("stft");

    group.bench_function("stft_2048_512_1s", |b| {
        b.iter(|| {
            let s = Stft::new(StftConfig::default()).expect("valid config");
            black_box(s.compute(black_box(&x), 48_000).expect("compute"))
        })
    });

    // 按区间计算：这是长文件真正会用到的路径
    group.bench_function("stft_range_2048_512_0p1s", |b| {
        b.iter(|| {
            let s = Stft::new(StftConfig::default()).expect("valid config");
            black_box(
                s.compute_range(black_box(&x), 48_000, 0, 4_800)
                    .expect("compute"),
            )
        })
    });

    group.finish();
}

fn bench_pyramid_like_downsampling(c: &mut Criterion) {
    // 频谱降采样到像素宽度：这是每次重绘都要做的
    let n = 16384;
    let a = SpectrumAnalyzer::new(SpectrumConfig {
        fft_size: n,
        hop_size: n,
        window: WindowFunction::Hann,
        remove_dc: false,
    })
    .expect("valid config");
    let frame = a.analyze(&sine(n), 48_000).expect("analyze").remove(0);

    let mut group = c.benchmark_group("downsample");
    for &target in &[800_usize, 2048] {
        group.bench_function(format!("to_{target}_from_8193"), |b| {
            b.iter(|| black_box(frame.downsample_db(black_box(target))))
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_fft,
    bench_peak_detection,
    bench_stft,
    bench_pyramid_like_downsampling
);
criterion_main!(benches);
