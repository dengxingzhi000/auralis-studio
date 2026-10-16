# DSP 性能基准记录

数据用途：**把「Rust 够不够快」从争论变成可检查的事实**（ADR-014）。
没有这份记录，ADR-014 的 FFI 门槛就无法执行。

## 运行环境（`docs/TEST_PLAN.md` §6 要求记录）

| 项 | 值 |
|---|---|
| CPU | 32 逻辑核（型号未记录，下次补） |
| OS | Windows 11 / Windows SDK 10.0.22621 |
| 工具链 | rustc 1.99.0 (b940084d7 2026-09-28) |
| profile | `cargo bench`（criterion，等效 release 优化） |
| crate 版本 | auralis-dsp 0.1.0，rustfft 6.4.1 |
| 采样率 | 48 000 Hz，输入为 1 kHz 满刻度一半正弦 |
| 运行方式 | `cargo bench -p auralis-dsp`（100 样本，warm-up 1 s / measure 2 s） |

## 测量结果

criterion 报告格式 `[lower bound  point estimate  upper bound]`：

| 基准 | 下界 | **中位数** | 上界 |
|---|---|---|---|
| `fft/fft_1024` | 62.6 µs | **77.2 µs** | 98.8 µs |
| `fft/fft_4096` | 253.5 µs | **270.7 µs** | 290.9 µs |
| `fft/fft_16384` | 1.031 ms | **1.054 ms** | 1.078 ms |
| `peak_detection/detect_peaks_2049_bins` | 46.7 µs | **65.4 µs** | 96.1 µs |
| `stft/stft_2048_512_1s` | 3.418 ms | **3.470 ms** | 3.526 ms |
| `stft/stft_range_2048_512_0p1s` | 258.7 µs | **263.8 µs** | 269.8 µs |
| `downsample/to_800_from_8193` | 18.5 µs | **19.8 µs** | 21.5 µs |
| `downsample/to_2048_from_8193` | 31.2 µs | **32.8 µs** | 34.8 µs |

复现命令：

```powershell
cargo bench -p auralis-dsp
# HTML 报告：target/criterion/report/index.html
```

## 结论一：FFT 绝对不能放进音频回调

48 kHz 下每个回调周期的预算约 `1e6 / 48000 ≈ 20.8 µs`。

| 操作 | 中位数 | 预算倍数 |
|---|---|---|
| FFT 4096 | 271 µs | **13.0 倍** |
| FFT 1024 | 77 µs | 3.7 倍 |
| 峰值检测（2049 bins） | 65 µs | 3.1 倍 |

即使最小的 1024 点 FFT 也超预算 3.7 倍。

**这确认了 ADR-018 的架构**：音频回调只做「拷贝样本 + 计数」，
所有 FFT / 峰值检测 / STFT 都在工作线程上跑。
这不是「保守起见」，是**测量结论**。

> 唯一的例外是频谱降采样到 800 点（19.8 µs）勉强在预算内，
> 但它依赖上游已有频谱数据，回调里并不存在这样的数据。
> 不应据此认为回调可以承担任何 DSP。

## 结论二：一个未能达标的既有目标

`docs/superpowers/specs/2026-10-14-auralis-studio-design.md` §7.5 曾写
「FFT 4096 点（Release，单次）< 100 µs」。

**实测 271 µs，未达标（2.7 倍）。**

该目标是在没有任何测量的情况下写下的估算值。现在有了实测，
应以本文件为准。需要更低延迟时应考虑：预分配 plan（已做）、
实数 FFT（`realfft`，M2 已验证可解析 6.4 同系）、或 SIMD（`wide` / `core::arch`）。
**目前不宣称 FFT 达到任何既定性能指标。**

## 结论三：离线分析吞吐是够的

`stft_2048_512_1s` = 3.47 ms/秒音频 ⇒ 实时率约 **290 倍**。
`stft_range` 0.1 秒 = 264 µs ⇒ **视口分块重算也足够快**
（M4 的分块方案在性能上成立）。

## 尚缺

- 未记录 CPU 具体型号（下次补），跨机器对比会受影响。
- 未测并行/多线程收益；M1 的任务引擎目前是单线程串行。
- 未测内存曲线（长时间运行的增长趋势）。