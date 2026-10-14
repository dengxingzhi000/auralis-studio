# Auralis Studio — 设计文档

日期：2026-10-14
状态：待批准
文档性质：本文件是**实现者的工作依据**。它记录已经做出的决定及其理由，不复述 PRD。

---

## 0. 摘要

用 Rust 从零构建一款 Windows 优先的专业音频分析桌面软件。核心竞争力三条：专业可视化（波形/频谱/声谱图共享统一时间轴与游标）、可信的 DSP 内核（每个指标有明确数学定义与可复现数值测试）、稳定的实时架构（后台计算与 UI 重绘互不拖累）。

本文档在 PRD 之上做了**架构收敛**与**数值约定的补全**，并明确划出「哪些是我（架构师）拍板的、哪些留给实现者、哪些必须由产品方回答」。

---

## 1. 意图与为什么

用户是音频开发者 / 录音人员 / 设备测试人员。他们现在的痛点是：要看一个音频文件的频谱、波形、削波、电平，必须在三四种工具之间来回切换，而且每种工具给出的数字定义各不相同，无法互相印证。

Auralis Studio 的价值不是「能画出频谱」，而是：**同一个时间轴上同时呈现时域与频域证据，且每个数字都能说明它是怎么算出来的**。因此 UI 的专业感和 DSP 的可信度是同一个目标的两面，不是两个独立目标。

**成功的样子**：用户拖入一个 WAV，立刻看到波形包络 + 频谱 + Peak/RMS/DC/Clipping；拖动频率游标，波形上的时间位置和频谱读数同时响应；切换窗函数，频谱分辨率和泄漏的变化肉眼可见且有数值解释；导出的 CSV 里每列都标了单位、窗函数、FFT 长度和算法版本。

---

## 2. 目标 / 非目标 / 反目标

### 2.1 本次交付（M1）的目标
1. `cargo build` / `cargo test` / `cargo clippy` 全绿的可运行 Windows 桌面程序。
2. 五区布局的工作台：左侧资源栏 / 中央分析画布 / 右侧参数检查器 / 底部状态栏 / 顶部命令区。
3. 深色「测量仪器」主题，数值统一等宽字体，颜色定义在设计 token 中。
4. 拖拽或选择音频文件 → 元数据检查 → 解码 → 波形 + 频谱 + 基础指标，全流程带进度条且可取消。
5. `auralis-dsp` 纯数值 crate：窗函数、FFT、单边幅值谱、峰值检测、RMS、Peak、DC Offset、Crest Factor，每个都有确定性测试。
6. CSV / JSON 导出，字段含单位、参数、算法版本。
7. GitHub Actions 流水线：fmt / clippy / test / Windows Release 构建。

### 2.2 非目标（M1 明确不做）
- 实时采集（CPAL 采集在 M3）。M1 的架构必须为它留出接缝，但代码不写。
- 声谱图（STFT 在 M2）。M1 的频谱视图使用与 STFT 相同的数据结构，便于复用。
- SNR / THD / 响度 / 频率响应等高级指标（M4）。
- PDF 报告、插件系统、脚本接口、macOS / Linux。
- 音频编辑 / 修复功能（那是 iZotope 的地盘，不是本项目范围）。

### 2.3 反目标（做到这些算失败）
- **「看起来专业但数字不可信」**：UI 展示了一个精确到小数点后三位的 dB 值，但底层算法定义不清或没有测试覆盖。这比不做更糟。
- **「能启动、能画频谱」就算完成**：M1 结束时不满足本文档 §9 的验收条目即为未完成。
- **过度拆分**：为了「架构漂亮」把一个模块拆成三个 crate，导致每次改动要动四个 `Cargo.toml`。
- **卡顿**：分析进行中拖动面板或缩放频谱掉帧。

---

## 3. 环境与约束

### 3.1 磁盘策略（硬约束）

C 盘剩余 168.9 GB，D 盘剩余 325.6 GB。Rust 生态（尤其 eframe → wgpu）的依赖树与编译产物轻易吃掉数 GB，因此：

| 内容 | 位置 | 手段 |
|---|---|---|
| `CARGO_HOME`（注册表源码、git 依赖缓存） | `D:\DevTools\cargo` | 用户环境变量 |
| `RUSTUP_HOME`（工具链本体） | `D:\DevTools\rustup` | 用户环境变量 |
| `target/`（编译产物） | 仓库根 `D:\ProgramProject\sonic-scope\target` | 仓库天然在 D 盘 |
| 源码 / 文档 | `D:\ProgramProject\sonic-scope` | 已确定 |
| 应用配置 `%APPDATA%\AuralisStudio\` | C 盘 | 仅数 KB，可接受 |

**实现者必须**：用 `setx` 写入用户级 `CARGO_HOME` / `RUSTUP_HOME` 后再安装；安装完成后用 `$env:CARGO_HOME` 验证 `cargo` 实际解析路径；并在文档中记录这一配置，避免用户在另一台机器上踩坑。

`CARGO_BUILD_JOBS` 建议设为 CPU 逻辑核数，避免并行编译时内存打满。

### 3.2 仓库状态

当前目录**不是 git 仓库**。第一步必须 `git init`，并 `.gitignore` 掉 `target/`、`*.pdb`、`.idea/`。`Cargo.lock` 必须提交（这是二进制产品，不是库）。

### 3.3 技术选型（已确定，不再讨论）

| 项 | 选择 | 说明 |
|---|---|---|
| 语言 | Rust Stable | `rust-toolchain.toml` 在安装后锁定到具体版本号 |
| UI | egui / eframe | 纯 Rust，与 DSP 同语言同类型 |
| 2D 图表 | egui_plot | 仅用于频谱曲线（点数 ≤ 2048，够用） |
| 高密度绘制 | egui `Painter` + `ColorImage` 纹理 | 波形包络、声谱图；**不用 egui_plot** |
| FFT | rustfft | 用 `FftPlanner` 缓存 plan，不在热路径重建 |
| 解码 | symphonia | |
| 实时 I/O | cpal（M3 引入） | |
| 错误 | thiserror | |
| 日志 | tracing | |
| 测试 | 内置 + proptest + criterion | |

### 3.4 依赖版本策略

**不手写版本号**。流程：
1. workspace 根 `Cargo.toml` 只写 `[workspace.dependencies]`，**不带 version**；
2. 各 crate 用 `dep = { workspace = true }` 引用；
3. 用 `cargo add <crate> --package <pkg>` 让 Cargo 解析当前兼容的最新版本；
4. `cargo check --workspace` 验证；
5. 提交 `Cargo.lock`，此后 CI 用 `--locked` 构建。

**格式支持必须实测，不能靠库名承诺**：symphonia 的 WAV / FLAC / MP3 / AIFF / PCM / Vorbis 是纯 Rust 原生解码；AAC / Opus 等可能需要额外 feature 或外部解码器。M1 验收时必须对每种**声称支持**的格式都有真实样本测试，测不出来的格式就不要在 UI 里列出。

---

## 4. 架构

### 4.1 Crate 划分：13 → 9

PRD 列了 13 个 crate。我把它收敛为 9 个，理由是**只保留真正有价值的接缝**，其余合并。PRD 自己也在风险表里写了「多 Crate 过度拆分 → 开发效率下降」，并且注明「目录较完整，但可以分阶段创建」。

```
auralis-studio/
├── Cargo.toml                 # workspace
├── rust-toolchain.toml
├── .cargo/config.toml         # 强制 --locked? 不，CI 用
├── .gitignore
├── apps/
│   └── auralis-desktop/       # eframe 二进制，唯一同时认识 egui + 全部 crate 的地方
├── crates/
│   ├── auralis-core/          # 共享类型、错误、时间轴、结果模型、算法版本常量
│   ├── auralis-dsp/           # 纯数值：窗、FFT、谱、指标。无 I/O、无 UI、无 OS
│   ├── auralis-analysis/      # Analyzer trait、配置校验、离线分析管线
│   ├── auralis-codec/         # symphonia 解码 → PcmBlock 流 + 元数据
│   ├── auralis-audio/         # cpal + SPSC ring buffer（M3；M1 留空壳接口）
│   ├── auralis-viz/           # 波形多级包络、频谱显示降采样、色阶 LUT
│   ├── auralis-task/          # 任务队列、取消、进度
│   ├── auralis-app/           # 用例编排 + 工作区状态 + 持久化 + 导出
│   └── auralis-ui/            # egui：主题、布局、视图组件、交互状态
├── tests/                     # 集成测试 + fixtures + golden
├── benches/
└── docs/
```

**PRD 中被合并的 crate，以及将来拆分的位置**：

| PRD crate | 合并到 | 将来拆分的触发条件 |
|---|---|---|
| `auralis-application` + `auralis-project` + `auralis-storage` + `auralis-report` | `auralis-app` | 当 `auralis-app` 超过 ~1500 行，或 UI 需要在没有应用层的条件下做单元测试 |
| `auralis-visualization` | `auralis-viz` | 保持独立（它是纯计算，值得单独测） |
| `auralis-core` + `auralis-visualization` 中的显示降采样 | — | 不合并：`auralis-core` 是所有 crate 的公共词汇表，必须是最小的 |

### 4.2 依赖方向（单向，禁止反向）

```
auralis-desktop  (binary)
      │
      ├──▶ auralis-ui ──┬──▶ auralis-app ──┬──▶ auralis-task
      │                 │                  ├──▶ auralis-codec
      │                 │                  └──▶ auralis-analysis ──▶ auralis-dsp ──▶ auralis-core
      │                 └──▶ auralis-viz ────────────────────────────▶ auralis-core
      │                 └──▶ auralis-audio ───▶ auralis-core
      └──▶ auralis-audio
```

**硬性禁止**（用 CI 里的依赖检查脚本兜底）：
- `auralis-dsp` 不得依赖 `egui` / `eframe` / `cpal` / `symphonia` / `std::fs` / `std::net`。
- `auralis-core` 不得依赖除 `serde` / `thiserror` / `num-traits` 之外的任何 crate。
- 任何 crate 不得依赖 `auralis-ui`（除了 `auralis-desktop`）。
- OS API（Windows 句柄、路径、注册表）只允许出现在 `auralis-audio` 和 `auralis-app` 的存储子模块。

### 4.3 线程模型（M1）

| 线程 | 拥有 | 禁止 |
|---|---|---|
| UI 线程（eframe） | 全部 egui 状态、绘制、用户输入 | 执行解码或 FFT；等待任务完成 |
| 任务工作线程（N 个，M1 取 1） | 解码 + 分析 | 触碰 egui 上下文 |
| 音频回调（M3） | 仅从 CPAL 缓冲区复制 f32 到 ring buffer | 分配内存、加锁、日志、等待 |

任务工作线程与 UI 之间用 `crossbeam-channel`（有界）通信，消息是 `Arc` 包裹的不可变快照。UI 每帧 `try_iter()` 抽干消息，只保留最后一个进度/结果快照。

**M1 的分析必须可取消**：解码循环每块检查 `CancelToken`（`Arc<AtomicBool>`），一旦置位立即停止解码、释放缓冲、发出 `TaskOutcome::Cancelled`（**不是** `Failed`）。

### 4.4 实时线程的纪律（M3 起生效，现在就要按此设计数据结构）

- 采集回调里**只有**一次 `Vec::push` 级别的拷贝（或直接写入预分配 ring buffer），没有 `Vec` 分配、没有锁、没有 `format!`、没有 `tracing`。
- ring buffer 容量固定。溢出策略：**丢最新 + 原子计数器 +1**，UI 显示告警。理由：丢最新的实现最简单且不破坏内存顺序；丢掉的数据量等于 UI 消费速度不足，计数器让用户能看见。
- 工作线程的 FFT scratch buffer、窗数组、STFT 环形帧缓冲全部在初始化时预分配，热路径零分配。
- UI 不需要每来一块音频就重绘。M3 的目标刷新率是 20–60 Hz 可配置，与音频回调解耦。

---

## 5. DSP 数值约定

**这是本项目最重要的部分。** PRD 反复强调「不能把幅值谱、功率谱、功率谱密度混为一谈」，下面把每一处都定死。

### 5.1 内部表示与时间轴

- 解码后统一为**交错 `f32` PCM**，满刻度 = ±1.0（即 `i16` 的 ±32768 映射到 ±1.0）。
- 保留源采样率。**绘图不得触发重采样。**
- 时间一律以 `u64` 帧号表达；秒数只在渲染时现算 `frame as f64 / sample_rate as f64`。禁止累加浮点秒数。
- 样本索引 `samples[frame_index * channels + channel]`。
- 解码器必须保证 `samples.len() % channels == 0`；否则丢弃尾部不完整帧并记录一次 warning（不要 panic）。
- 遇到 NaN / ±Inf 样本：替换为 0 并在结果里带上 `sanitized_samples: u64` 计数，让用户知道数据被改过。

### 5.2 窗函数

使用**周期型（periodic, DFT-even）**定义，`n = 0..N`，`N = fft_size`：

| 名称 | 定义 | 相干增益 CG = Σw/N | ENBW（bin） | ENBW（Hz） |
|---|---|---|---|---|
| Rectangular | `1` | 1.0 | 1.000 | `fs/N` |
| Hann | `0.5 - 0.5·cos(2πn/N)` | 0.5 | 1.500 | `1.5·fs/N` |
| Hamming | `0.54 - 0.46·cos(2πn/N)` | 0.54 | 1.3628 | `1.3628·fs/N` |
| Blackman | `0.42 - 0.5cos(2πn/N) + 0.08cos(4πn/N)` | 0.42 | 1.7269 | `1.7269·fs/N` |

ENBW 的通用公式（代码里用它，不要硬编码常数）：
```
W  = Σ_{n=0}^{N-1} w[n]
W2 = Σ_{n=0}^{N-1} w[n]²
ENBW_bins = N · W2 / W²
ENBW_Hz   = fs · W2 / W²
```

> 为什么是周期型：对称型（`n/(N-1)`）在做非整数周期信号分析时会在频谱两端产生额外的镜像旁瓣。对称型只用于滤波器设计，不用于谱分析。

### 5.3 归一化：三种谱的严格区分

设 `x[n]` 为实数输入，`w[n]` 为 N 点窗，`y[n] = x[n]·w[n]`，`X[k] = FFT(y)`（`k = 0..N-1`），`W = Σ w[n]`。

**单边**取 `k = 0 .. N/2`。

#### (a) 幅值谱 Amplitude Spectrum — 单位：FS
```
A[k] = |X[k]| / W          ,  k = 0 或 k = N/2
A[k] = 2·|X[k]| / W        ,  0 < k < N/2
```
含义：`A[k]` 是「在该 bin 频率上产生同样 `|X[k]|` 的正弦波的幅值」。
**校验**：满刻度（`A = 1.0`）的相干正弦波，其主峰读数必须是 `1.0` → `0 dBFS`。

> DC 和 Nyquist bin 不能乘 2，因为它们没有镜像伙伴。这是 PRD 里特别点名过的错误。

#### (b) 功率谱 Power Spectrum — 单位：FS²
```
P[k] = A[k]²  ,  k = 0 或 N/2
P[k] = A[k]² / 2 ,  0 < k < N/2
```
含义：单个 bin 携带的功率。

> **必须在文档和 UI 里说明**：`Σ P[k] ≠ 信号总功率`，因为窗函数的能量被摊到 `ENBW_bins` 个 bin 上。这个"不等"不是 bug，是窗函数的定义。

#### (c) 功率谱密度 PSD — 单位：FS²/Hz
```
S[k] = P[k] / ENBW_Hz
```
**校验（Parseval，可写成测试）**：对白噪声（方差 σ²），
```
Σ_{k=0}^{N/2} S[k] · (fs/N)  ≈  σ²
```
这个等式成立是 PSD 归一化正确的充分证据。必须写成单元测试，四种窗各测一次。

### 5.4 dB 约定

| 量 | 换算 | 参考值 | 零值处理 |
|---|---|---|---|
| 幅值 → dBFS | `20·log10(A / 1.0)` | 满刻度正弦 = 0 dBFS | `A == 0` → 返回 `-inf`，**在结果里存 `-inf`，在渲染时钳到 `min_db`** |
| 功率 → dB | `10·log10(P / 1.0)` | 1.0 FS² = 0 dB | 同上 |
| PSD → dBFS/Hz | `10·log10(S / 1.0)` | 1.0 FS²/Hz = 0 dBFS/Hz | 同上 |

- `min_db` 默认 **−140 dB**，`max_db` 默认 **0 dB**。UI 的 y 轴范围与颜色映射共用这两个值。
- **绝不**在 DSP 层把 `-inf` 换成一个大数字。「显示下限」是渲染层的决定，不是算法的决定。指标结果里应当能看到真实的 `-inf`。
- 单位后缀必须显式标注：`dBFS` / `dB` / `dBFS/Hz`。FS 是满刻度数字信号，**不是声压级 SPL**。UI 里不得出现 `dB SPL` 字样——本软件没有麦克风校准，不具备测量 SPL 的能力。

### 5.5 峰值检测与频率插值

1. 在 `k ∈ [1, N/2]` 上用局部极大判定：`mag[k] > mag[k-1] && mag[k] >= mag[k+1]`。
2. 频率插值：对 `(k-1, k, k+1)` 三点的**对数幅值**做抛物线拟合，
   `δ = 0.5·(y₋ − y₊) / (y₋ − 2y₀ + y₊)`，`f = (k + δ)·fs/N`。
   分母为 0（非局部极大）时 `δ = 0`。`δ` 必须被夹在 `[-0.5, 0.5]`。
3. 峰值必须通过 `min_db` 阈值，且与相邻已选峰值的距离 ≥ `min_peak_separation_hz`（默认 `3·fs/N`）。
4. 默认返回按幅值降序的前 **20** 个峰；UI 显示前 8 个并标序号（参考图里的做法）。
5. 每个峰返回 `{ frequency_hz, amplitude_dbfs, bin_index, bin_offset }`，**同时保留未插值的 bin 频率**，让用户能验证插值没有跑偏。

### 5.6 指标定义（M1 实现的）

| 指标 | 定义 | 单位 | 备注 |
|---|---|---|---|
| Peak | `max_n \|x_n\|`，逐通道 + 聚合 | FS / dBFS | 同时报告 `over_full_scale: bool`（是否存在 \|x\| > 1.0）与 `sample_index` |
| RMS | `sqrt( (1/N) Σ x_n² )`，逐通道 + 聚合 | FS / dBFS | 窗口 = 整个分析区间，**在 UI 上明示** |
| DC Offset | `(1/N) Σ x_n`，逐通道 | FS / dBFS | 频谱入口默认 **不做** 去直流；是否去直流是用户选项（`SpectrumConfig::remove_dc`），且去直流后的结果必须与原始 DC 测量值分别展示 |
| Crest Factor | `20·log10(peak / rms)` | dB | `rms == 0` → `-inf`，并给 `undefined: true` |
| Clipping | 见下 | 次 / ms | 两类，见下 |

**削波必须分两个独立指标，不能合并**：
1. `over_full_scale_count`：`\|x_n\| > 1.0` 的样本数。这是数字域事实，无争议。
2. `flat_top_regions`：连续 `k` 个样本满足 `\|x_n\| ≥ clip_threshold`（默认 0.999）的**片段**数量、总时长、最长片段位置。这是**可疑特征**，不等于模拟域削波。

> PRD 明确要求：「仅凭某个样本达到 0 dBFS，不能断定原始模拟信号一定发生了削波。」UI 上这两项必须是两个独立行，且第二行标注「启发式」。

所有指标结果携带：
```rust
struct MetricValue {
    value: Result<f64, MetricUnavailable>,  // Err = 无法计算
    unit: &'static str,
    algorithm_version: &'static str,          // 来自 auralis-core::ALGORITHM_VERSION
    params_digest: String,                    // 影响结果的参数的哈希，便于复现
    notes: Vec<String>,                       // 如「窗口 = 全文件」「估算值」
}
```

### 5.7 测试信号与容差（M1 必须全部通过）

`auralis-dsp/tests/` 中的确定性测试。**测试信号由代码生成，不使用外部样本文件**（避免版权与体积问题），生成器放在 `tests/common/mod.rs`。

| # | 输入 | 断言 | 容差 |
|---|---|---|---|
| T1 | 1 kHz、A=0.5 的相干正弦，fs=48k, N=4096 | 主峰频率 | ≤ 0.1 bin（插值后） |
| T2 | 同上 | 主峰幅值 dBFS | ≤ 0.2 dB |
| T3 | 同上，窗 = Rect/Hann/Hamming/Blackman 各跑一遍 | 幅值一致（验证相干增益校正正确） | ≤ 0.2 dB |
| T4 | 100 Hz + 1 kHz 双音（相干，对 N=4096@48k） | 两个峰都存在，频率正确 | 各 ≤ 0.1 bin；幅值 ≤ 0.2 dB |
| T5 | 全零 | 无 NaN / 无 Inf（除 dB 换算的 -inf）；Peak=0, RMS=0 | 精确 |
| T6 | 直流 c=0.1 | DC Offset = 0.1 ± 1e-6；频谱 bin0 的 A = 0.1 ± 0.2 dB；bin1 之后 ≈ −inf | — |
| T7 | f = 0.45·fs/2（接近 Nyquist） | 主峰落在正确 bin 侧，幅值正确 | ≤ 0.5 dB（边界 bin 能量会向两侧分） |
| T8 | 非相干正弦（如 f=1234.5 Hz @ N=4096） | 频率误差 | ≤ 1 bin；幅值误差 ≤ 1.5 dB（泄漏，非相干下放宽） |
| T9 | 双声道，L=A·sin、R=0.3A·sin | 通道隔离：R 的幅值是 L 的 0.3 | ≤ 0.2 dB |
| T10 | 硬削波（`clamp(sin, ±0.999)`） | `over_full_scale_count == 0`；`flat_top_regions > 0` | 精确 |
| T11 | 超量程（`1.5·sin`） | `over_full_scale_count > 0`，且等于 \|x\|>1 的样本数 | 精确 |
| T12 | 白噪声（固定 seed 的 LCG，不用 rand） | PSD 的 Parseval：`Σ S[k]·Δf ≈ σ²` | 相对误差 ≤ 5% |
| T13 | 能量守恒：同一信号，`Σ A[k]²` 在加窗前后 | 记录差异作为已知偏差，不做硬断言，但要在注释里说明 | — |
| T14 | `fft_size = 1`、`hop_size = 0`、`hop_size > fft_size`、`min_db > max_db`、采样率 0 | 全部返回 `Err(ConfigError)` 而不是 panic | — |
| T15 | `fft_size` 非 2 的幂 | 明确策略：**允许**，因为 float FFT 支持任意长度；但 UI 下拉只提供 2 的幂，并给用户一个「自定义」输入框 | — |

> T8 的容差故意放宽。这是诚实的做法：非相干信号在有限长度下无法精确分离能量，任何声称「非相干正弦幅值也能到 0.2 dB」的说法都是错的。**在 UI 上，当信号非相干（主峰能量分散到多个 bin，谱峰度 < 0.9）时，峰值读数旁显示「估算」标记。**

### 5.8 `ALGORITHM_VERSION`

在 `auralis-core` 定义常量：

```rust
pub const ALGORITHM_VERSION: &str = "auralis-dsp/0.1.0+spec.2026-10-14";
```

任何改变上述数值约定的改动都必须提升它。UI 在每个指标面板脚注、每个导出文件头部都写它。

---

## 6. 数据流

### 6.1 离线分析（M1 完整实现）

```
用户操作
  ↓
auralis-app::OpenFile
  ├─ 校验路径存在、可读
  ├─ auralis-codec::probe(path) → AudioFileInfo { sample_rate, channels, duration, codec, bit_depth }
  │     （不解码整文件；用 symphonia 的 format probe + metadata）
  └─ 提交任务到 auralis-task
        ↓
工作线程：AnalysisJob
  ├─ 循环：codec.next_block() → PcmBlock { first_frame, samples }
  │     每块检查 CancelToken
  ├─ auralis-viz：累加多级波形包络（min/max/RMS，每级 2× 递降）
  ├─ auralis-dsp：流式累计 Peak / RMS / DC / Clipping 统计量
  ├─ auralis-dsp：按 hop 滑窗做 FFT，产出频谱帧
  │     （M1：产出「整段平均频谱」+ 最新帧；M2：产出 STFT 矩阵）
  ├─ 通过 channel 发送进度（节流到 10 Hz）与阶段性可视化数据
  └─ 完成 → 汇总 AnalysisReport
        ↓
auralis-app：把 report 存入 workspace 状态，必要时写盘
        ↓
UI 每帧抽干消息，更新对应面板
```

**关键决策**：
- **解码永不整文件载入内存**。`symphonia` 的 `Decoder` 以 `Box<dyn Iterator>` 逐包取出，每次转成一块 `PcmBlock`（默认 65536 帧）。
- 波形包络必须**多级**：level 0 = 每 256 样本一格，level n = 每 256·2ⁿ 样本一格，一直建到顶层格子数 < 4096。这样无论文件是 3 秒还是 3 小时，视口内只需取 ≤4096 个格子，与文件长度解耦。
- 进度节流 10 Hz。UI 重绘由 egui 的 `request_repaint_after` 驱动，不由任务进度驱动。
- 任务失败必须**只影响该任务**。任何一个文件解码失败不能影响其他任务或整个应用。

### 6.2 实时采集（M3）

```
CPAL input callback  ──►  rtrb::Producer<f32>  ──►  工作线程
   （仅 memcpy + 溢出计数）      （有界 SPSC）      │
                                                   ├─► 帧组装（凑够 fft_size）
                                                   ├─► FFT / RMS / Peak
                                                   └─► Arc<Mutex<Snapshot>> 覆盖写
                                                            ↓
                                                      UI 抽帧读取
```

- 「最新快照」用 `Arc<Mutex<Snapshot>>` 覆盖写，而不是有界 channel 排队。理由：UI 关心的是**当前**状态，排队只会让显示滞后。
- 快照里带 `dropped_frames: u64`、`underruns: u64`、`buffer_fill_ratio: f32`，UI 在状态栏显示。

---

## 7. UI 架构

### 7.1 布局

egui 没有原生 dock 系统。**决定：手写布局**，不引入 `egui_dock`。

```
┌────────────────────────────────────────────────────────────────┐
│ TopBar:  [导入] [开始采集] [分析配置] [工作区▾] [导出]          │  28pt
├────────────┬───────────────────────────────────┬───────────────┤
│            │                                   │               │
│  SidePanel │         CentralPanel               │   SidePanel   │
│    LEFT    │   ┌─────────────┬─────────────┐    │     RIGHT     │
│            │   │  Spectrum   │  Metrics    │    │               │
│  · 文件     │   │  (频谱)      │  (指标表)    │    │  · FFT 点数    │
│  · 实时设备 │   ├─────────────┴─────────────┤    │  · 窗函数      │
│  · 任务队列 │   │  Waveform（波形概览+局部）   │    │  · 频率范围    │
│  · 工作区   │   └───────────────────────────┘    │  · 动态范围    │
│            │                                   │  · 声道        │
│   200–320pt │        可拖拽分隔条               │   240–320pt    │
├────────────┴───────────────────────────────────┴───────────────┤
│ StatusBar: fs=48kHz · 设备 · 12.4ms · 分析延迟 · 任务 1/3 · ⚠ 2 警告 │ 24pt
└────────────────────────────────────────────────────────────────┘
```

- 中央区域默认上下分栏（上：频谱，下：波形），比例可拖拽并持久化。
- 左右侧栏可折叠（`CollapsingHeader` 不够，要能完全收起）。
- **布局是数据**：存进 workspace JSON，键为面板名 → `{ visible, size_ratio, collapsed }`。

### 7.2 设计 Token

在 `auralis-ui::theme::tokens` 里定义，所有组件**只能**通过 token 取色，禁止硬编码颜色字面量。

```rust
// 基础
BG_APP       = #101318
BG_PANEL     = #181D25
BG_ELEVATED  = #202733
BORDER_SUBTLE= #303946
BORDER_FOCUS = #57A6FF
TEXT_PRIMARY = #E7EBF1
TEXT_SECONDARY = #98A3B3
TEXT_DISABLED  = #5B6472
ACCENT_PRIMARY = #57A6FF

// 信号状态
SIGNAL_POSITIVE = #54D6A0
SIGNAL_WARNING  = #F2B75B
SIGNAL_CRITICAL = #FF657A

// 声道（颜色 + 线型双编码，兼顾色觉差异）
CH0 = { #57A6FF, LineStyle::Solid    }   // 蓝
CH1 = { #F2B75B, LineStyle::Dashed  }   // 琥珀
CH2 = { #54D6A0, LineStyle::Dotted  }   // 绿
CH3 = { #C792EA, LineStyle::DashDot }   // 紫
CH4 = { #FF8FA3, LineStyle::Dense   }   // 粉
CH5 = { #7FD1D9, LineStyle::Sparse  }   // 青

// 图表
GRID_MAJOR  = #262D38
GRID_MINOR  = #1D232C
AXIS_LINE   = #3A4353
AXIS_LABEL  = #98A3B3
CURSOR      = #FFFFFF
CLIP_BAND   = #FF657A @ 12% alpha
```

**字体规则**：所有数值（频率、dB、采样率、百分比）必须使用等宽字体。实现方式：在 `FontDefinitions` 中把 `epaint::FontFamily::Monospace` 映射到一个等宽字体（如 `JetBrains Mono` / `Cascadia Mono`，从系统字体目录加载，失败则回退到内置字体）。**频率轴刻度、dB 刻度、指标表、数值输入框** 全部走 Monospace。这是 PRD 明确点名的要求，也是「专业仪器感」最便宜的来源。

**声谱图色阶**：实现 **inferno** 作为 256 级 LUT（感知均匀、色觉安全）。存为 `const INFERNO_LUT: [[u8;3]; 256]`，在 `auralis-viz`。UI 侧栏附一个 256×18 的色阶条（可点击设置 min/max dB）。

### 7.3 视图组件

PRD 明确要求不要做成巨型 `AudioChart`。M1 实现这些，每个是独立的 `impl` + 一个绘制适配器：

| 组件 | 绘制方式 | 数据来源 |
|---|---|---|
| `WaveformView` | `Painter::vline` 批量画 min/max 包络柱 | `auralis-viz::WaveformPyramid` |
| `SpectrumView` | egui_plot `Line`（点数 ≤ 2048，先按屏幕像素降采样） | `auralis-dsp` 的 SpectrumFrame |
| `MetricsView` | egui 表格，等宽字体，右对齐数值，状态色来自阈值 | `AnalysisReport` |
| `FileListView` | egui 列表 + 拖放 | workspace 状态 |
| `TaskListView` | egui 列表 + `ProgressBar` + 取消按钮 | `auralis-task` |
| `InspectorView` | egui 控件（ComboBox / DragValue） | `SpectrumConfig` 等 |
| `StatusBar` | egui 顶栏/底栏布局 | `AppStatus` |
| `SpectrogramView` | **M2**：`ColorImage` 纹理 + `Image` 绘制 | STFT 矩阵 |

**波形必须用 `Painter` 不用 egui_plot**：一个全屏宽的波形有上万个包络柱，egui_plot 的 `PlotPoint` 开销会直接掉帧。声谱图的每个时频单元绝不能变成 `PlotPoint`。

### 7.4 统一时间游标（核心竞争力，不可省）

```
AppState {
    cursor_frame: u64,      // 全局时间位置，单位：帧
    selection: Option<Selection { start_frame, end_frame }>,
}
```

- 波形视图、频谱视图（仅在有 spectrogram 时）、声谱图视图**读同一个** `cursor_frame`。
- 在任一视图里移动鼠标 → 写 `cursor_frame` → 其它视图在下一帧读到并画出游标线。
- `Selection` 用于「只分析这一段」。
- 游标位置的频率/幅值读数显示在频谱视图右上角，用 Monospace。

### 7.5 性能预算（M1 验收时用 `tracing` 打点实测）

| 场景 | 目标 |
|---|---|
| 空载 UI | 稳定 60 FPS |
| 分析中拖动面板 / 缩放频谱 | 不低于 45 FPS |
| 单帧 UI 绘制 | < 8 ms |
| FFT 4096 点（Release，单次） | < 100 µs |
| 3 分钟立体声文件的波形包络构建 | < 2 s |
| 内存 | 与文件时长**线性但低斜率**（包络 ≈ 文件大小的 1/2000） |

> 达不到就如实报告达不到，不要在文档里写「目标 60 FPS」然后不测。

---

## 8. 交付切片

PRD 的周计划（10–12 周）是**规划估算，不是承诺**。实际按下面的切片执行，每个切片独立可运行、可测试。

### M1 — 骨架与离线分析垂直切片（本次）
工作区骨架 → 主题与布局 → 文件打开 → 解码流 → DSP 内核 → 波形视图 → 频谱视图 → 指标表 → 参数检查器 → 任务进度/取消 → 导出 → CI。

**完成定义**：§9 的 M1 验收清单全部通过。

### M2 — 频谱交互深化 + 声谱图
STFT（帧长 256–8192、hop 1/4–1/8）、色阶与动态范围、inferno 色阶条、声谱图纹理与缩放、游标读数、多峰标记交互、参数持久化。

### M3 — 实时采集
CPAL 设备枚举与格式协商、rtrb ring buffer、工作线程快照、实时电平表与频谱、溢出/掉帧告警、设备断开恢复。

### M4 — 高级指标与打磨
SNR、THD/THD+N、A/C/Z 计权响度（明确标注非 BS.1770 除非真的实现了 K 计权）、频段能量、批量任务与结果对比、PDF 报告、性能回归、长时间运行验收。

---

## 9. 验收

### M1 验收清单（全部为「是/否」，不允许「基本可以」）

1. `cargo fmt --all -- --check` 通过。
2. `cargo clippy --workspace --all-targets -- -D warnings` 通过。
3. `cargo test --workspace` 通过，且 §5.7 的 T1–T15 全部存在并通过。
4. `cargo build --release --locked` 在 Windows 上成功，产出可双击运行的 exe。
5. 程序启动后能看到五区布局，主题为深色，数值是等宽字体。
6. 拖入 / 选择一个 WAV，能看到：文件名、采样率、声道数、时长、编码；波形包络；频谱曲线；Peak / RMS / DC Offset / Crest Factor 四个指标及单位。
7. 分析过程中进度条推进，界面可继续拖动面板。
8. 点取消，任务状态变为「已取消」而非「失败」，且内存回落。
9. 换 4 种窗函数，频谱形态可见变化，幅值读数符合 §5.7 T3。
10. 导出的 CSV 有表头，含单位、窗函数、FFT 长度、采样率、`ALGORITHM_VERSION`。
11. 每种**声称支持**的解码格式都有真实样本测试；测不通的格式不在 UI 列表里。
12. `core` 和 `dsp` crate 的源码里 grep 不到 `egui`、`cpal`、`symphonia`、`std::fs`。

---

## 10. 风险与对策

| 风险 | 触发条件 | 对策 |
|---|---|---|
| wgpu 首次编译极慢 / 吃满 D 盘 | 首次 `cargo build` | 预期内。D 盘 325 GB 充足。冷编译预计 5–15 分钟。 |
| symphonia 版本 API 变动 | 写 codec 时 | 先 `cargo add` 拿当前版本，再读其文档；不要凭记忆写 API。 |
| egui 版本 API 变动（`Shape`/`epaint` 频繁改） | 写 theme 时 | 同上。所有颜色访问集中在一个文件里，改动面收敛。 |
| 频谱图误用 egui_plot 导致卡帧 | 实现 SpectrumView 时 | 硬性规则：> 1024 点必须先按屏幕像素降采样。 |
| 「非相干信号幅值不准」被用户当 bug | 实际使用 | 谱峰度 < 0.9 时显示「估算」标记（§5.7）。这是设计，不是缺陷。 |
| 长文件内存增长 | 3 小时以上文件 | 包络金字塔 + 分块解码 + 有界缓存（§6.1）。 |
| 用户误把 dBFS 当 dB SPL | 任何时候 | UI 全程 `dBFS` 字样，文案明确「数字满刻度，非声压级」。 |

---

## 11. 我（架构师）拍板的决定

以下是我替你做的判断。如果哪一条你不同意，现在说；我不会再问第二遍，但这些都可以推翻。

1. **13 个 crate 收敛为 9 个**，四个合并进 `auralis-app`。
2. **不用 egui_dock**，手写五区布局 + 可拖拽分隔条。
3. **周期型窗函数**，非对称型。
4. **单边幅值谱用相干增益归一化**，满刻度正弦读数为 1.0 → 0 dBFS。
5. **DC / Nyquist bin 不乘 2**。
6. **PSD 定义为 `P[k] / ENBW_Hz`**，并用 Parseval 写测试证明。
7. **`min_db` 默认 −140**，DSP 层保留 `-inf`，渲染层才钳位。
8. **频率轴默认对数**（参考图 SpectrumScope/Inspect 都是对数），线性作为选项。
9. **声谱图色阶用 inferno**（感知均匀 + 色觉安全）。
10. **削波拆成「超量程计数」和「平顶片段」两个独立指标**，后者标注启发式。
11. **实时溢出策略：丢最新 + 计数器**，不实现丢最旧。
12. **UI 快照用 `Arc<Mutex<>>` 覆盖写**，不用有界队列排队。
13. **测试信号全部代码生成**，不引入外部音频样本（解码格式测试除外，那部分需要真实样本）。
14. **M1 不写实时采集代码**，但 `auralis-audio` 先建带接口的空 crate，避免 M3 时改动依赖图。
15. **不引入 `egui_dock`、不引入插件系统、不引入数据库**（`rusqlite` 推迟到 M4 且需先确认历史记录是否真的需要查询能力）。

---

## 12. 需要你回答的（不回答我就按括号里的默认值走）

**只有 3 个问题会改变实现，且都有合理默认值。**

1. **默认频率轴是线性还是对数？** 参考图（SignalScope、FFT Analyzer）用的都是对数，PRM 说「支持线性与对数」但没说默认。
   → 我默认**对数**（20 Hz – 20 kHz），因为专业音频工具的事实标准是这样。反对请说。

2. **单条曲线还是左右声道默认同时显示？** 参考图里 FFT Analyzer 默认 4 条彩色曲线（Ch1/Ch2/Out1/Out2）。
   → 我默认**立体声文件默认叠加显示两条**（实线 + 虚线），单声道只显示一条。反对请说。

3. **M1 完成后怎么验收？** 我需要一个能跑的 exe。
   → 我默认构建 `release` 版 exe 放到 `target/release/`，你双击运行并按 §9 的 12 条清单逐条确认。**我不会自己声称验收通过。**
   → 另需一个测试音频文件：任意 WAV 即可（哪怕手机录的 10 秒语音）。你没有的话我可以用代码生成一个标准测试信号（1 kHz + 100 Hz 双音，48 kHz，立体声），这对验证 FFT 反而更可靠。

---

## 13. 明确留给实现者的

- `auralis-core` 内部类型定义的字段细节（PRD §8.1 的 `PcmBlock` / `AudioFormat` / `AudioTime` 已经是很好的起点）。
- `auralis-task` 的并发策略细节（工作线程数量、优先级、任务队列的数据结构）。
- `auralis-viz` 包络金字塔的具体级数与内存上限常量。
- UI 组件的内部布局细节，只要符合 §7 的分区与 token 约束。
- 所有未在 §5 明确的数值细节 —— **但任何新数值约定都必须写进 `auralis-dsp` 的模块文档并提升 `ALGORITHM_VERSION`**。