# 技术架构设计（TAD）

## 1. 架构目标

- Rust workspace、多 Crate。
- DSP 与 UI、音频设备、文件系统解耦。
- 离线分析与实时采集使用不同调度策略，共享可复用 DSP。
- 统一 PCM、时间轴、配置、错误和结果模型。
- 避免首版引入微服务、动态插件 ABI 或过度抽象。

## 2. 建议目录

```text
auralis-studio/
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── AGENTS.md
├── apps/
│   └── desktop/                  # Electron 主进程 + preload + React 19 渲染层
│       ├── electron/             # 主进程、preload 窄接口、引擎宿主
│       ├── src/                  # React/TS 渲染层（只展示，不做音频计算）
│       └── scripts/              # 开发启动、esbuild、打包
├── crates/
│   ├── auralis-core/             # 共享类型、单位、错误——纯净
│   ├── auralis-dsp/              # RMS/Peak/DC/FFT/STFT/峰值/削波——纯净
│   ├── auralis-codec/            # 容器探测与 PCM 解码（symphonia）
│   ├── auralis-audio/            # SPSC 环、设备枚举、CPAL 采集
│   ├── auralis-task/             # 离线任务调度、进度、取消
│   ├── auralis-storage/          # CSV/JSON 导出与原子写盘
│   ├── auralis-visualization/    # 波形包络、声谱矩阵
│   └── auralis-engine/           # 命令层 + stdio JSON-Lines RPC（无 UI、无窗口）
├── scripts/                      # 边界检查、oracle、采集/soak、许可证
├── docs/
└── .github/workflows/
```

Crates 按实际依赖逐步创建。目录列表不是要求首日为每个 crate 添加空壳或复杂接口；
当前没有单独的 `auralis-app`、`auralis-report` 或 `auralis-project`，等出现真实编排
需求再建（AGENTS.md「不预建空 crate」）。

## 3. 依赖方向

- `auralis-core`：共享领域类型和错误，不依赖 UI/OS/数据库。
- `auralis-dsp` → `auralis-core`：纯数值算法。
- `auralis-codec` → `auralis-core`：解码、探测和 PCM 转换（`auralis-dsp` 只作为
  dev-dependency 用于生成测试样本）。
- `auralis-audio` → `auralis-core`、`auralis-dsp`：设备输入、环形缓冲与实时分析。
- `auralis-visualization` → `auralis-core`：视图数据准备，不负责音频采集。
- `auralis-task`：离线任务调度、取消、进度（不依赖其它内部 crate）。
- `auralis-storage`：导出与原子写盘（不依赖其它内部 crate）。
- `auralis-engine` → 以上全部：命令层与 stdio RPC，负责编排、进程生命周期与
  跨进程协议；文件读写由 `auralis-codec` / `auralis-storage` 承担。
- `apps/desktop`：Electron 容器 + React 渲染层，通过 stdio JSON-Lines 调
  `auralis-engine`；渲染层不直接依赖任何 Rust crate，也不做音频计算。

避免循环依赖。若两个 crate 需要共享类型，将类型下沉到 core 或重新审视边界。

## 4. 依赖选型基线

| 领域 | 选型 |
|---|---|
| 桌面容器 | Electron 45：主进程 + sandboxed preload + 独立渲染进程（ADR-019） |
| UI | React 19 + TypeScript 7 + Vite 8；Canvas 2D 自绘图表 |
| Rust | Stable 1.99.0（`rust-toolchain.toml` 固定），edition 2024 |
| 设备 I/O | `cpal` 0.18（默认 WASAPI；ASIO 未启用，见 ADR-020） |
| 解码 | `symphonia` 0.6，显式启用 `mp3` / `aiff` features |
| FFT | `rustfft` 6.4 |
| 并发 | `crossbeam-channel` 0.5；`crossbeam-queue` 0.3 的 `ArrayQueue`（SPSC 环） |
| 序列化 | `serde` / `serde_json` |
| 错误 | `thiserror` |
| 诊断 | `tracing` / `tracing-subscriber` |
| 基准 | `criterion` 0.8 |
| 测试样本 | WAV/AIFF 手写 writer；FLAC 由 dev-dependency `flacenc` 0.4 生成 |

版本必须在初始化时通过 Cargo 解析、编译和测试验证。提交 Cargo.lock；不要在每次构建时浮动升级依赖。

## 5. 关键接口建议

### PCM

- sample rate：正整数 Hz。
- channels：正整数。
- frame offset：以帧为单位。
- samples：明确交错布局。
- 每块必须保证样本数能被声道数整除。
- 不因 UI 显示而修改源采样率。

### 分析器

使用强类型配置与结果。可定义 `Analyzer` trait，但不要为了每个算法引入动态注册系统。

分析结果至少包含：
- analyzer ID 与算法版本；
- 输入元数据；
- 参数；
- 数据和单位；
- 时间范围；
- 诊断/警告；
- 是否完整或被取消。

### 线程模型

离线：
renderer → preload → Electron main → engine stdio RPC → bounded task queue →
decoder blocks → DSP → immutable result/view snapshot → renderer。

实时：
device callback → preallocated bounded SPSC buffer → analysis worker → latest snapshot → UI。

回调不阻塞、不访问文件/数据库、不等待 UI、不执行复杂分配。UI 不应消费每一个音频样本。

## 6. 数据流与内存

- 文件解码采用块处理。
- 波形使用多级 min/max 包络。
- 声谱图采用分块 STFT，在 Rust 侧降采样到固定网格后以 Canvas 2D `ImageData`
  绘制（WebGL 路径保留为可替换方案，见 ADR-018）。
- 图表数据按视口和像素宽度降采样。
- 所有队列、缓存和历史结果设置上限或淘汰策略。
- 取消任务后尽快停止生产新数据并释放资源。

## 7. 错误处理

使用类型化错误区分：
- 不支持格式；
- 文件损坏（容器头坏掉、编码不认识）；
- 文件截断（数据短于声明长度）——**这一类不是错误**：照常分析已解出的部分，
  并在结果里标记截断与声明/实际时长（ADR-021）；
- 参数无效；
- 输入长度不足；
- 设备不可用；
- 设备格式协商失败；
- 实时缓冲溢出；
- 任务取消；
- 导出/持久化失败。

用户可见错误应说明可采取的操作；日志包含诊断上下文，但避免记录不必要的音频内容。

## 8. 架构决策

详见 `ADR.md`。重大变更应记录背景、选项、决策、后果和验证方法。
