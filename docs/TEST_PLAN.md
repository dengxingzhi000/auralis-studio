# 测试与验收计划

## 1. 测试层级

1. 单元测试：DSP 公式、窗函数、频率映射、错误边界。
2. 集成测试：解码到分析、任务取消、结果导出、缓冲溢出。
3. UI/桌面测试：文件导入、布局调整、设备错误、窗口缩放。
4. 性能测试：FFT、STFT、波形包络、纹理更新、内存上限。
5. 发布测试：Windows Release、安装/卸载、依赖和许可证审查。

## 2. 合成信号集

至少包含：
- 全零信号；
- 常量 DC 信号；
- 相干采样正弦波；
- 非整数周期正弦波；
- 多音信号；
- 近 Nyquist 信号；
- 左右声道不同频率/幅值；
- 超量程浮点信号；
- 含 NaN/Inf 的无效输入；
- 极短输入和不完整多声道帧。

测试样本、生成脚本和预期值应纳入仓库，避免依赖外部临时文件。

## 3. 文件解码测试

每种宣称支持的容器/编码组合至少有：
- 正常文件；
- 空文件或截断文件；截断文件必须解出可用部分并被标记为截断，
  而不是整体失败（ADR-021）；
- 损坏头信息；
- 多声道文件；
- 非 44.1/48 kHz 采样率文件；
- 长文件或大文件；
- 元数据缺失的文件。

不支持的格式必须明确报错，不得崩溃。

### 3.1 已验证容器登记表（2026-11-03）

「已验证」的判定标准见 `crates/auralis-codec/src/lib.rs`：仓库内签入（或由
生成器产出的）样本必须走完「解码 → DSP」，且数值与预期一致。
UI 依据 `auralis_codec::verified_containers()` 决定是否显示「未验证」角标。

| 容器 | 样本 | 生成方式 | 覆盖的测试 |
|---|---|---|---|
| WAV | 测试运行时生成（`wav_writer`） | 手写 writer（44 字节 RIFF 头） | 9 项集成测试（其中核心链路 5 项：探针、单声道重建、立体声隔离、截断、完整文件），另有 1 项 writer 头字段单测 |
| FLAC | `tone-1k-mono.flac`、`tone-stereo.flac` | dev-dependency `flacenc` 0.4 | 4 项：探针元数据、单声道重建、立体声通道隔离、样本与生成器一致 |
| AIFF | `tone-1k-mono.aiff`、`tone-stereo.aiff` | 本 crate 手写 writer（大端 IFF、80 位 extended 采样率、`SSND` 8 字节头） | 4 项集成测试（探针元数据、单声道重建、立体声通道隔离、样本与生成器逐字节一致）+ 2 项 writer 单测（80 位采样率编码、头字段） |
| MP3 | 无 | —（本机没有 MP3 编码器） | **未验证** |
| OGG / Vorbis | 无 | — | **未验证** |

样本位于 `crates/auralis-codec/tests/fixtures/`，生成器是 `#[ignore]` 的测试：

```text
cargo test -p auralis-codec -- --ignored regenerate_flac_fixtures
cargo test -p auralis-codec -- --ignored regenerate_aiff_fixtures
```

三种无损格式使用同一信号（1 kHz、0.5 幅度、整周期、44.1 kHz），
因此预期值相同：峰值 −6.02 dBFS、RMS −9.03 dBFS。
立体声样本左 0.5 / 右 0.15 幅度（1 kHz / 1.5 kHz），用于抓左右声道写反。

**证据强度的区别必须保留**：WAV / AIFF 样本由本仓库手写 writer 生成，
FLAC 由第三方编码器生成；**没有一份是现实世界采集的文件**。
「已验证」证明的是解码器与独立的编码侧实现一致，不等于
「任意来源的同格式文件都保证可用」。MP3 缺样本的原因是本机没有
`ffmpeg` / `lame` / `sox`，造不出可复现样本，因此不作宣称。

## 4. 实时音频测试

- 无输入设备；
- 设备启动失败；
- 支持格式协商失败；
- 设备拔出/切换；
- Ring Buffer 满；
- 工作线程短暂落后；
- 连续采集至少 8 小时的稳定性目标（**未验收**：目前只有一台设备的 6.03 s 采集）；
- UI 高频操作时采集仍保持可观测的稳定状态。

物理设备测试应在具有真实 Windows 音频设备的专用环境运行。普通 CI 不应假设一定有音频设备。

## 5. 初始验收目标

- 所有核心 DSP 算法有测试。
- 关键算法具有参考值、容差和边界测试。
- 核心模块行覆盖率目标 ≥ 80%，但覆盖率不能代替数值验证。
- 常规交互目标 60 FPS；实际以约定测试机和视图复杂度验收。
- 实时显示目标 20–60 次/秒，和音频设备采样回调分离。
- 实时连续运行 8 小时无崩溃、无持续内存增长（**未验收**：只有 10 分钟离线 soak，
  见 `docs/BENCHMARKS.md`；实时采集侧没有长稳记录）。
- 取消任务后停止后续处理并释放资源。
- Windows Release 构建可重复。

## 6. 基准记录

每份基准记录：
- CPU、内存、OS 版本；
- Rust 工具链、编译 profile；
- crate 版本和 Cargo.lock；
- 输入样本率、通道数、样本数；
- FFT/STFT 参数；
- 运行次数、均值和分位数；
- 预期结果和实际结果。

## 7. CI 命令

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo build --workspace --release`

在 workspace 完成初始化后，将以上命令接入 Windows GitHub Actions。根据实际 target 与依赖情况调整矩阵。
