<div align="center">

**English** · [简体中文](./README.zh-CN.md)

# Auralis Studio

### 一个 Windows 优先的桌面音频分析仪——每个数字都追得到来源。

**Rust DSP 内核，React 展示层，以及一条硬规矩：不报告任何没人测过的数字。**

[![CI](https://img.shields.io/github/actions/workflow/status/dengxingzhi000/auralis-studio/ci.yml?branch=master&style=flat-square&logo=github&label=ci)](https://github.com/dengxingzhi000/auralis-studio/actions/workflows/ci.yml)
[![Stars](https://img.shields.io/github/stars/dengxingzhi000/auralis-studio?color=ffcb47&labelColor=black&style=flat-square&logo=github)](https://github.com/dengxingzhi000/auralis-studio/stargazers)
[![Forks](https://img.shields.io/github/forks/dengxingzhi000/auralis-studio?color=8ae8ff&labelColor=black&style=flat-square&logo=github)](https://github.com/dengxingzhi000/auralis-studio/network/members)
[![Issues](https://img.shields.io/github/issues/dengxingzhi000/auralis-studio?color=ff80eb&labelColor=black&style=flat-square&logo=github)](https://github.com/dengxingzhi000/auralis-studio/issues)
[![Rust](https://img.shields.io/badge/Rust-1.99-007396?logo=rust&logoColor=white&style=flat-square)](https://www.rust-lang.org/)
[![Electron](https://img.shields.io/badge/Electron-45-47848F?logo=electron&logoColor=white&style=flat-square)](https://www.electronjs.org/)
[![React](https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=black&style=flat-square)](https://react.dev/)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue?style=flat-square)](#许可证)

**[快速开始](#快速开始) · [它能做什么](#它能做什么) · [技术栈](#技术栈) · [架构](#架构) · [验证体系](#验证体系) · [常见问题](#常见问题) · [文档](#文档)**

```powershell
git clone https://github.com/dengxingzhi000/auralis-studio
cargo build -p auralis-engine
cd auralis-studio\apps\desktop && npm install && npm run electron:dev
```

</div>

> [!IMPORTANT]
> **Auralis Studio 不是校准过的测量仪器，本版本也绝不假装是。**
> 没有 SPL、没有 LUFS、没有 SNR、没有 THD、没有 THD+N——这些算法根本没实现。
> 信任任何数字之前请先读[已知限制](#已知限制)，那份清单很短，而且写得很诚实。

> [!NOTE]
> **仅支持 Windows。** macOS 与 Linux 未适配。内核理论上可移植，
> 但外壳、CPAL 采集通路与静态 CRT 构建都不是。

---

桌面上的音频工具大多是一个套着黑盒的界面。频谱看起来不对时，你无法判断是窗函数错了、
归一化错了、文件解错了，还是根本没人验证过。

Auralis Studio 反过来做。界面上每一个数字都来自一个有确定性单测的 Rust 函数，
而测试套件会用一份**独立的 numpy 实现**（`scripts/oracle.py`）交叉验证该函数——
那份实现与 Rust 侧不共享任何代码。基准没达标时，结果记在
[docs/BENCHMARKS.md](./docs/BENCHMARKS.md)里，而不是悄悄删掉。

整个架构围绕一条约束：**音频回调只做「拷贝样本 + 计数」，别的什么都不做。**
这不是保守，是测出来的。参考机器上 4096 点 FFT 需要 271 µs，
而 48 kHz 下一个回调的预算只有 20.8 µs。回调里放任何更重的东西，按构造就会掉帧。

于是一个对自己的说法格外严谨的分析仪。

## 快速开始

> [!NOTE]
> **版本是钉死的。** `rust-toolchain.toml` 固定 Rust 1.99.0。
> CI 用 `--locked` 构建，`Cargo.lock` 已入库，必须保持入库。

### 环境要求

| 工具 | 版本 | 用途 |
|---|---|---|
| **Rust** | 1.99.0 | 由 `rust-toolchain.toml` 固定 |
| **MSVC 生成工具** | — | 启用 `+crt-static`，产物是真正独立的单文件 exe（ADR-013） |
| **Node.js** | 22.12+ 或 24 | Electron 45 要求。CI 还会用 Node 20 做渲染层构建（仅构建；分支保护保留了这个检查名） |
| **Python** | 3.12 | 仅用于 numpy oracle 交叉验证 |

本机安装方式、镜像配置与路径约定见
[docs/DEV_ENVIRONMENT.md](./docs/DEV_ENVIRONMENT.md)。

### 1. 只跑前端

```powershell
cd apps\desktop
npm install
npm run dev          # Vite dev server
```

### 2. 完整应用

```powershell
cargo build -p auralis-engine          # Rust 引擎是独立进程
cd apps\desktop
npm run electron:dev                  # 同时拉起 Vite 与 Electron
```

`electron:dev` 会等 Vite 就绪后再启动 Electron。窗口加载
`http://127.0.0.1:5173`，通过 stdio 与引擎通信，详见
[ADR-019](./docs/ADR.md#adr-019桌面容器从-tauri-2-迁移到-electron)。

### 3. 生产构建

```powershell
cargo build -p auralis-engine --release
cd apps\desktop
npm run electron:start                # vite build + esbuild 主进程/preload + electron .
```

**还没有安装包**（M6）。引擎必须存在于
`target\debug\auralis-engine.exe`（或由 `AURALIS_ENGINE` 指定）；
打包后预期读取 `<resources>\engine\auralis-engine.exe`。

> [!WARNING]
> **Release 产物是 GUI 子系统程序。** PowerShell 直接调用不会可靠等待，
> `$LASTEXITCODE` 可能为空。脚本中需要退出码时请用
> `Start-Process -Wait -PassThru`——`scripts/check-oracle.ps1` 已按此写法。

### CLI（无需窗口，CI 跑的就是它）

```powershell
.\target\release\auralis-engine.exe --version
.\target\release\auralis-engine.exe --self-test
.\target\release\auralis-engine.exe --dump-reference out.json
```

`--dump-reference` 导出参考向量，numpy oracle 就是拿它来比对的。

## 它能做什么

| 图标 | 领域 | 现状 |
|---|---|---|
| 📂 | **导入** | WAV 元数据检查、可取消的分块 PCM 解码。FLAC / MP3 / AIFF 能解码但**未验证**，见[下表](#容器支持实测结论不是推测) |
| 📈 | **时域** | Peak、RMS、DC Offset、Crest、满刻度溢出计数、左右声道独立 |
| 📊 | **频域** | 带显式幅值校正的 FFT、峰值检测与标记、缩放平移、游标读数 |
| 🌈 | **声谱图** | 窗长与跳步可配的 STFT，基于纹理的 Canvas 渲染 |
| 🔴 | **实时** | CPAL 采集 → 有界无锁环形缓冲 → 缓冲区回收 + 丢帧计数 |
| 🧮 | **DSP 验证** | Criterion 基准 + 与独立 numpy 实现的交叉验证 |
| 🧱 | **边界强制** | CI 会在 DSP crate 引入 I/O、或前端出现音频运算时失败 |
| 🪟 | **仪器化 UI** | 深色测量仪器设计系统、自研 Canvas 2D 渲染、Radix headless 原语 |

## 技术栈

| 层 | 选择 |
|---|---|
| **桌面外壳** | Electron 45（自带 Chromium，运行时版本随发行固定） |
| **UI** | React 19 + TypeScript 7 + Vite 8（渲染进程沙箱 + `contextIsolation`） |
| **设计系统** | CSS Variables 令牌 + Radix Primitives（headless）+ 自研组件 |
| **可视化** | 自研 Canvas 2D（波形 / 频谱），纹理化 Canvas（声谱图） |
| **DSP** | Rust：`auralis-dsp`，只依赖 `rustfft` 6.4 |
| **编解码** | `symphonia` 0.6，`mp3` / `aiff` feature 显式声明 |
| **并发** | 无锁 SPSC 环（`crossbeam-queue`）、`catch_unwind` 任务隔离 |
| **IPC** | Rust 引擎子进程 + stdio JSON-Lines；视图数据预降采样后以 base64 → `Uint8Array` 传输 |
| **验证** | Rust 单测 + numpy oracle + CI |
| **基准** | Criterion 0.8 |

**不引入** Ant Design / MUI（ADR-017）。这个应用里的面板是仪器，不是表单。

## 架构

```
   ┌──────────────────────────────────────────────────────┐
   │  React 19 · TypeScript · Vite（沙箱渲染进程）        │
   │  ── 只展示算好的缓冲，不做任何 DSP 计算。──           │
   │                                                      │
   │  Workspace · Spectrum · Spectrogram · Inspector      │
   └───────────────────────┬──────────────────────────────┘
                           │  preload: window.auralis.invoke
                           │  结构化克隆（字符串、Uint8Array）
   ┌───────────────────────▼──────────────────────────────┐
   │  Electron 主进程                                     │
   │  窗口 · 对话框 · 引擎子进程 · 生命周期               │
   └───────────────────────┬──────────────────────────────┘
                           │  stdio JSON-Lines RPC
                 标量     │  base64 字段 → Uint8Array
   ┌───────────────────────▼──────────────────────────────┐
   │  auralis-engine（独立进程）                          │
   │  快命令内联 · 慢命令 2 worker · 队列 8               │
   └───────┬───────────────────────────┬──────────────────┘
           │                           │
   ┌───────▼────────┐         ┌────────▼─────────────────┐
   │  音频线程      │         │  任务引擎                 │
   │  ┌───────────┐ │         │  auralis-task            │
   │  │ CPAL      │ │         │  取消令牌 · 进度          │
   │  │ 回调      │ │         │  队列 · catch_unwind     │
   │  └─────┬─────┘ │         └────────┬─────────────────┘
   │        │ 回收缓冲                 │ 拉取数据块
   │        ▼                          ▼
   │  ┌──────────────────────────────────┐
   │  │  SPSC 环形缓冲（有界、无锁）      │  ◀── 丢帧是计数出来的，不是藏起来的
   │  └───────────────┬──────────────────┘
   │                  │
   │  ┌───────────────▼──────────────────┐
   │  │  auralis-codec   auralis-dsp     │
   │  │  解码             RMS·Peak·DC     │
   │  │  symphonia        FFT·STFT·峰值   │
   │  └───────────────┬──────────────────┘
   │                  │
   │  ┌───────────────▼──────────────────┐
   │  │  auralis-core（纯净）            │  无 UI · 无设备 API
   │  │  类型 · 单位 · 错误 · 时间轴     │  无文件系统 · 无数据库
   │  └──────────────────────────────────┘
   └──────────────────────────────────────┘
```

**整张图只编码一条规矩：React 不参与音频计算。**
dB 换算、FFT、降采样、包络构建全部在 Rust 侧完成后才跨越 IPC 边界。
`scripts/check-boundaries.ps1` 会在这条规矩失效、或 `auralis-core` /
`auralis-dsp` 依赖上出现 `std::fs`、`cpal`、`symphonia` 之类的东西时让构建失败。

### 缓冲区回收——为什么采集代码长这样

CPAL 回调在 48 kHz 下的预算大约是 20.8 µs。每个回调都新建一个 `Vec<f32>`，
就等于永远每回调一次分配，而这正是工程契约明令禁止的。所以回调从一个固定
64 格的池子里取一个缓冲，`clear()` 掉，重新填满（容量已够，不会重新分配），
推进环形缓冲。消费端用完再把缓冲还回池子。

```rust
// 回调里做的全部事情
let mut buf = recycle.pop().unwrap_or_else(Vec::with_capacity);
buf.clear();
buf.extend_from_slice(input);
let _ = ring.push(buf);   // 溢出时返回 Err(buf) —— 复用它，别丢掉
```

前几次回调会分配，用于填满池子。之后进入**零分配**稳态，
而池子容量固定，因此不会无限增长。

这也正是 `SampleRing::push` 返回 `Result<(), T>` 而不是 `bool` 的原因：
溢出时缓冲必须回到调用方手里，否则回收无从谈起。

## 容器支持——实测结论，不是推测

能编译的解码器不等于可用的格式。只有当真实样本走完「导入 → DSP」全链路
且结果被核对过，格式才会被标记为已验证。

| 容器 | 编译进产物 | 已用真实样本验证 |
|---|---|---|
| **WAV** | 是 | **是**——13 项集成测试覆盖「写入 → 解码 → DSP」 |
| FLAC | 是 | 否 |
| MP3 | 是（`mp3` feature 显式声明） | 否 |
| AIFF | 是（`aiff` feature 显式声明） | 否 |

`verified_containers()` 是 UI 的硬约束：不在列表里的格式在界面上带
**「未验证」**角标，而不是当作已支持来呈现。
依据 `docs/TEST_PLAN.md` §3 与 [AGENTS.md](./AGENTS.md)「不宣称未经验证的格式」。

> [!NOTE]
> symphonia 0.6.1 的 default features **不含** mp3 / aiff——实测结论，不是推测。

## 验证体系

这是本项目比较特别的部分，所以放在文档前面。
在信任任何数字之前，值得先读完这一节。

### 与 numpy 交叉验证

`scripts/oracle.py` 是用 numpy 写的第二套 DSP 实现，与 Rust 侧不共享任何代码。
两者一致，说明的是「Rust 实现是对的」，而不只是「它自洽」。

```powershell
cargo build --workspace --release --locked
./scripts/check-oracle.ps1
```

CI 还会断言十一个具名 DSP 测试仍然存在，避免这些性质被悄悄删掉：

| 测试 | 钉住了什么 |
|---|---|
| `dc_offset_is_the_mean` / `dc_offset_is_signed` | DC Offset 是均值，且符号保留 |
| `silence_produces_zeroes_not_nan` | 静音不产生 NaN |
| `non_finite_samples_are_sanitized_and_counted` | NaN / Inf 被清理并计数 |
| `over_full_scale_is_counted_not_clipped` | 超满刻度被计数，而不是被截断 |
| `flat_topped_waveform_is_detected_as_suspicious` | 平顶被标记 |
| `clean_sine_is_not_reported_as_clipping` | ……而干净的正弦不会被误报 |
| `channel_isolation_l_and_r_are_independent` | 左右声道互不串扰 |
| `peak_of_sine_tracks_amplitude` | 峰值幅度正确 |
| `rms_of_sine_is_amplitude_over_sqrt_two` | RMS 归一化正确 |
| `crest_factor_of_full_scale_sine_is_about_3_01_db` | Crest ≈ 3.01 dB |

### 基准数据

Criterion，release profile，48 kHz。完整方法与注意事项见
[docs/BENCHMARKS.md](./docs/BENCHMARKS.md)。

| 基准 | 中位数 |
|---|---|
| `fft/fft_1024` | 77.2 µs |
| `fft/fft_4096` | 270.7 µs |
| `fft/fft_16384` | 1.054 ms |
| `peak_detection/detect_peaks_2049_bins` | 65.4 µs |
| `stft/stft_2048_512_1s` | 3.470 ms |
| `stft/stft_range_2048_512_0p1s` | 263.8 µs |
| `downsample/to_800_from_8193` | 19.8 µs |

对照 48 kHz 下 20.8 µs 的回调预算，架构就成了测量结论而非主观判断：

| 操作 | 中位数 | 预算倍数 |
|---|---|---|
| FFT 4096 | 271 µs | **13.0×** |
| FFT 1024 | 77 µs | 3.7× |
| 峰值检测 | 65 µs | 3.1× |

> [!WARNING]
> **FFT 没达到原始目标，而且记录里写明了。** 设计文档写的是
> 「4096 点 FFT（Release，单次）< 100 µs」。实测 271 µs，超了 **2.7 倍**。
> 那个目标是在没有任何测量的情况下写下的估算值，基准数据才是事实。
> **本项目不宣称 FFT 达到任何性能指标。**

### 完整检查集

```powershell
./scripts/check-boundaries.ps1   # 架构边界
./scripts/check-oracle.ps1       # numpy 交叉验证 DSP
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace           # 237 个测试
cargo test -p auralis-engine ipc -- --nocapture   # IPC 载荷体积
cd apps\desktop && npm run typecheck
cd apps\desktop && npm run electron:typecheck
cd apps\desktop && npm run electron:build
```

## 里程碑进度

| 里程碑 | 内容 | 状态 |
|---|---|---|
| **M0** | Rust workspace、工具链、深色主题、DSP 内核骨架、CI | ✅ 已完成 |
| **M0.5** | UI 栈切换至 Tauri 2 + React、IPC 通路验证 | ✅ 已完成（容器后由 Electron 取代） |
| **M1** | WAV 导入、元数据、分块 PCM、取消 | ✅ 已完成 |
| **M2** | FFT、幅值校正、峰值检测、多级波形包络 | ✅ 已完成 |
| **M3** | 频谱交互：游标、缩放平移、参数检查器、峰值标记 | ✅ 已完成 |
| **M4** | 实时采集（CPAL）+ 实时声谱图 | 🚧 进行中——CPAL 流、分析线程、UI 实时频谱、带 offset 的视口分块声谱图已接通；高频通道未做。**尚未在真实声卡上运行过。** |
| **M4.5** | 桌面容器 Tauri 2 → Electron 45；Rust 命令层抽为 `auralis-engine` 独立进程（ADR-019） | ✅ 已完成（2026-10-22） |
| M5 | 导出、配置持久化、异常恢复 | ✅ 已完成——CSV/JSON 导出、布局持久化、进程级会话恢复（含异常退出提示）；`auralis-app` 用例层刻意推迟 |
| M6 | 安装包、用户手册、许可证清单 | ⬜ 未开始 |

## 项目结构

```
auralis-studio/
├── crates/
│   ├── auralis-core/            # 共享词汇表：类型、单位、错误、时间轴——纯净
│   ├── auralis-dsp/             # RMS、Peak、DC、FFT、STFT、峰值检测——纯净
│   ├── auralis-codec/           # 容器与 PCM 解码（symphonia）
│   ├── auralis-audio/           # SPSC 环、设备枚举、CPAL 采集
│   ├── auralis-task/            # 离线任务调度、进度、取消
│   ├── auralis-visualization/   # 波形金字塔、声谱帧
│   └── auralis-engine/          # 命令层 + stdio RPC 端点（无 UI、无窗口）
├── apps/desktop/
│   ├── src/                     # React 19 前端——只负责展示
│   │   ├── components/
│   │   │   ├── audio-viz/       # WaveformView · SpectrumView · SpectrogramView
│   │   │   └── ui/              # primitives.tsx · icons.tsx
│   │   ├── layouts/             # Workspace
│   │   ├── lib/                 # contracts.ts · ipc.ts
│   │   └── styles/              # tokens.css · layout.css · globals.css
│   ├── electron/                # 主进程、preload 桥、引擎宿主
│   ├── scripts/                 # 开发启动器 + esbuild 配置
│   └── dev/                     # 仅开发用的渲染自检页（不进生产构建）
├── scripts/
│   ├── check-boundaries.ps1     # 架构被破坏时失败
│   ├── check-oracle.ps1         # 驱动 numpy 交叉验证
│   ├── oracle.py                # 独立的 numpy DSP 实现
│   └── activate-dev.ps1         # 本地工具链环境
├── docs/
│   ├── PRD.md  TAD.md  DSP_SPEC.md  UI_SPEC.md
│   ├── TEST_PLAN.md  IMPLEMENTATION_PLAN.md  ADR.md  BENCHMARKS.md
│   └── screenshots/
└── AGENTS.md                    # 本项目被约束的工程契约
```

## 工程原则

代码就是围绕这几条建的。它们来自 [AGENTS.md](./AGENTS.md)，
其中能被机械执行的部分由 `check-boundaries.ps1` 与 CI 强制。

1. **React 不参与音频计算。** dB 换算、FFT、降采样全部在 Rust 侧完成后才传输。
2. **DSP crate 保持纯净。** 无 UI、无设备 API、无文件系统、无数据库——
   数据进，数据出。
3. **实时音频回调不阻塞、不进行无界分配、不等待 UI。** 它拷贝样本、累加计数。
   丢帧被计数记录，绝不静默吞掉。
4. **所有核心算法均有确定性数值测试**，并由**独立的 numpy 实现交叉验证**。
5. **每个指标都写明单位、参考电平、归一化方式与适用条件**，并标注算法版本。
6. **不宣称未经验证的格式、性能或测量准确度**——包括对本项目自身架构的判断。

## 已知限制

信任任何数字之前请读完这一节。清单刻意保持简短。

- **没有校准测量。** 无 SPL、无 LUFS、无 SNR、无 THD、无 THD+N——
  这些算法尚未实现，本版本不作任何准确度声明。
- **只有 WAV 被验证过。** FLAC / MP3 / AIFF 能解码，但没有任何真实样本走完端到端验证。
  界面上会标注「未验证」。
- **实时采集从未在真实声卡上运行过。** CPAL 流、分析线程、丢帧/覆盖计数、
  UI 实时频谱都已实现并有单元测试，但实际跑到的只有「设备不存在」错误路径
  与纯逻辑测试。所有关于真实采集行为的说法（延迟、丢帧率、设备热插拔）
  都**未经验证**。
- **实时分析会丢弃中间样本。** 环形缓冲积压时，工作线程每次刷新只保留最新的
  `fft_size` 个样本，而不是对每个 hop 都出频谱。显示因此保持最新，
  代价是并非每个 hop 都有结果；UI 错过的帧计入 `snapshot_overwrites`。
- **FFT 超原设计目标 2.7 倍。** 271 µs vs 目标 < 100 µs，见
  [docs/BENCHMARKS.md](./docs/BENCHMARKS.md)。
- **削波检测在命令层返回 0。** 完整的平顶检测需要逐样本轨迹，
  与「分块、内存有界」冲突，将在 M2 随波形金字塔一起实现。
  **返回 0 是刻意的，猜测不是。**
- **拖放导入已实现但只做过类型检查。** 路径由 preload 的
  `webUtils.getPathForFile` 解析；尚未在本机用真实文件拖放验证。
- **IPC 端到端延迟只测过一次，也未在负载下测。** 合成信号一帧
  （含 4096 点 FFT）在开发机约 11 ms；持续实时吞吐未测。
- **Electron 迁移只在一台机器上验证过。** 它消除了本机的最大化花屏，
  但不保证所有 GPU/驱动组合。见
  [ADR-019](./docs/ADR.md#adr-019桌面容器从-tauri-2-迁移到-electron)。
- **会话恢复是尽力而为。** 它只重新打开上次成功探测的文件并重跑分析，
  不恢复崩溃瞬间的未返回结果；运行标记写不了时应用仍能启动，
  但无法判断上次是否异常退出。
- **界面目前只有中文**，尚无 i18n 层。
- macOS / Linux 未适配。

## 常见问题

<details>
<summary><strong>为什么 FFT 放在工作线程而不是音频回调里？</strong></summary>

因为 4096 点 FFT 需要 271 µs，而 48 kHz 下回调预算只有 20.8 µs——超了 13 倍。
这不是「差一点」，再仔细的编码也填不平 13 倍的差距。测量见
[docs/BENCHMARKS.md](./docs/BENCHMARKS.md)。

因此回调只做它负担得起的两件事：把样本拷进缓冲，累加计数。

</details>

<details>
<summary><strong>为什么 <code>SampleRing::push</code> 返回 <code>Result&lt;(), T&gt;</code> 而不是 bool？</strong></summary>

因为溢出时调用方需要把缓冲拿回来。`push` 过去返回 `bool` 并把元素丢掉，
这意味着环形缓冲一满就会销毁回收好的缓冲，逼得下一个回调重新分配——
恰好是回收机制要防止的失败。

返回 `Err(item)` 把缓冲交还回去以便复用。`DropOldest` 在回填失败时同样如此。

</details>

<details>
<summary><strong>为什么项目不宣称测量准确度？</strong></summary>

因为算法没实现。校准 SPL 需要已知参考电平与明确的测量带宽；
LUFS 需要完整的 ITU-R BS.1770 门控与 K 计权链路；THD 需要把失真分量
从基波中分离出来。这些在代码库里都不存在。

`AGENTS.md` 要求「在算法与前提条件实现并验证之前，不宣称 SPL、LUFS、SNR、
THD 或 THD+N」。硬要宣称，只会让这份 README 里其他所有数字都变得不可信。

</details>

<details>
<summary><strong>FLAC、MP3、AIFF 都能编译进来，为什么没标为已支持？</strong></summary>

因为 symphonia 的 default features 不含 mp3 / aiff（实测，非推测），
所以在 workspace manifest 里显式声明了。这让它们**能解码**，
但不代表**已验证**。

只有当真实样本走完「导入 → DSP」且结果被核对时，格式才算已验证。
目前 WAV 通过了。界面对此如实标注，而不是假装。

</details>

<details>
<summary><strong>为什么用独立的 numpy 实现，而不是多写些 Rust 测试？</strong></summary>

因为 Rust 测试只能确认代码做了代码做的事。oracle 是照着 DSP 规范写的，
不是照着 Rust 源码写的，所以两者一致时，证据是「实现符合规范」，
而不仅是「实现自洽」。

参考向量由二进制自身导出（`--dump-reference out.json`），
这保证两份实现面对的是同一批输入。

</details>

<details>
<summary><strong>为什么桌面外壳从 Tauri 换成了 Electron？</strong></summary>

因为在目标机器上，窗口最大化会**必现**屏幕级花屏；而用最小 Electron 壳
加载同一个页面时不会。完整排查、排除驱动与刷新率的测量、以及
「证明了什么、没证明什么」都写在
[ADR-019](./docs/ADR.md#adr-019桌面容器从-tauri-2-迁移到-electron)。

Electron 并不是「到处都不花屏」的保证。它把机器相关的 WebView2 运行时
从变量里去掉，代价是安装包更大、Rust 引擎变成独立进程。

</details>

<details>
<summary><strong>应用只支持 Windows，为什么 CI 还要跑 Linux？</strong></summary>

内核是可移植的，crate 也是跨平台的，所以矩阵任务会在 `ubuntu-latest` 上
也构建一遍。这能在到达 Windows 之前先发现纯 crate 里的平台相关问题。

迁移到 Electron 之后，Rust 侧在 Linux 上只需要 ALSA 开发头文件
（`libasound2-dev`），不再需要 Tauri 的 GTK/WebKit 头文件。

不产出 Linux 产物——artifact 任务只跑 Windows。

</details>

## 文档

想理解代码背后的理由，从这里开始：

| 文档 | 内容 |
|---|---|
| [用户手册](./docs/USER_MANUAL.md) | 安装、导入、视图操作、实时输入、指标、导出、快捷键与故障排查 |
| [产品需求文档 PRD](./docs/PRD.md) | 用户、版本范围、功能与非功能需求、产品边界 |
| [技术架构设计 TAD](./docs/TAD.md) | 分层、依赖方向、线程模型、错误分类 |
| [DSP 算法规范](./docs/DSP_SPEC.md) | RMS / Peak / DC Offset / FFT / STFT / 削波的定义与误差目标 |
| [UI/UX 设计规范](./docs/UI_SPEC.md) | 设计 Token、布局、视图组件、渲染性能约束 |
| [测试与验收计划](./docs/TEST_PLAN.md) | 测试分层、合成信号集、验收目标、CI 命令 |
| [开发实施计划](./docs/IMPLEMENTATION_PLAN.md) | M0–M6 里程碑与垂直切片 |
| [性能基准记录](./docs/BENCHMARKS.md) | Criterion 结果、方法，以及它们对架构意味着什么 |
| [架构决策记录 ADR](./docs/ADR.md) | ADR-001 … ADR-020，包括被推翻的那几条 |
| [第三方许可证清单](./THIRD_PARTY_LICENSES.md) | 生成式清单，含 SPDX 表达式与许可证全文 |
| [开发环境设置](./docs/DEV_ENVIRONMENT.md) | 本机工具链、镜像与路径约定 |
| [工程契约](././AGENTS.md) | 约束本项目的那些规则 |

安全问题请走 [SECURITY.md](./SECURITY.md)，不要开 issue。

## Star 历史

<a href="https://www.star-history.com/?repos=dengxingzhi000%2Fauralis-studio&amp;type=date">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/chart?repos=dengxingzhi000/auralis-studio&amp;type=date&amp;theme=dark&amp;legend=top-left" />
    <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/chart?repos=dengxingzhi000/auralis-studio&amp;type=date" />
    <img alt="Auralis Studio GitHub star 历史" src="https://api.star-history.com/chart?repos=dengxingzhi000/auralis-studio&amp;type=date" />
  </picture>
</a>

## 参与贡献

请先读 [CONTRIBUTING.md](./CONTRIBUTING.md)——里面讲了构建方式、
分支约定，以及 CI 具体会检查什么。

一句话版本：本项目在「不宣称没测过的东西」上守一条硬线。
如实报告哪些验证了、哪些没验证的 PR 是受欢迎的；悄悄夸大的不是。

1. Fork 仓库
2. 创建分支（`git checkout -b feature/…`）
3. 推送前跑完整检查集——命令见
   [CONTRIBUTING.md](./CONTRIBUTING.md#the-checks-ci-runs)
4. 开 PR，并填写模板里的「How it was verified」

参与行为受[行为准则](./CODE_OF_CONDUCT.md)约束。

## 许可证

双许可：[MIT](./LICENSE-MIT) **或** [Apache-2.0](./LICENSE-APACHE)，由你选。
这与 workspace manifest 里的 `license` 字段一致。

---

<div align="center">

**为需要数字正确的人而建。**

</div>
