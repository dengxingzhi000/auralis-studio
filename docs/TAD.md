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
│   └── auralis-desktop/
├── crates/
│   ├── auralis-core/
│   ├── auralis-dsp/
│   ├── auralis-analysis/
│   ├── auralis-audio/
│   ├── auralis-codec/
│   ├── auralis-visualization/
│   ├── auralis-application/
│   ├── auralis-task/
│   ├── auralis-project/
│   ├── auralis-storage/
│   ├── auralis-report/
│   └── auralis-ui/
├── tests/fixtures/
├── benchmarks/
├── docs/
└── packaging/windows/
```

Crates 应按实际依赖逐步创建。目录列表不是要求首日为每个 crate 添加空壳或复杂接口。

## 3. 依赖方向

- `auralis-core`：共享领域类型和错误，不依赖 UI/OS/数据库。
- `auralis-dsp` → `auralis-core`：纯数值算法。
- `auralis-codec` → `auralis-core`：解码、探测和 PCM 转换。
- `auralis-audio` → `auralis-core`：设备输入与平台适配。
- `auralis-analysis` → `auralis-core`、`auralis-dsp`：分析流程。
- `auralis-visualization` → `auralis-core`：视图数据准备，不负责音频采集。
- `auralis-task`：离线任务调度、取消、进度。
- `auralis-application`：编排用例。
- `auralis-storage`、`auralis-report`：持久化与导出。
- `auralis-ui`：依赖应用接口和视图数据。
- `apps/auralis-desktop`：组装应用。

避免循环依赖。若两个 crate 需要共享类型，将类型下沉到 core 或重新审视边界。

## 4. 依赖选型基线

- Rust Stable；edition 2024（先确认所用 Rust 工具链支持）。
- `eframe`/`egui`：桌面 UI。
- `egui_plot`：基础曲线与坐标交互；高密度声谱图采用纹理渲染。
- `cpal`：设备枚举与实时音频 I/O。
- `symphonia`：文件探测与解码，按需启用 codec features。
- `rustfft`：FFT；评估 `realfft` 是否有实际收益。
- `serde`/`serde_json`：参数和结果序列化。
- `thiserror`：库错误。
- `tracing`：结构化诊断。
- `crossbeam-channel`：普通后台任务通信。
- `rtrb` 或经验证的 SPSC Ring Buffer：实时线程与工作线程交接。
- `proptest`：算法性质测试。
- `criterion`：性能基准。
- `rusqlite`：仅在确实需要查询型历史记录时引入。

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
UI → Application → bounded task queue → decoder blocks → DSP → immutable result/view snapshot → UI。

实时：
device callback → preallocated bounded SPSC buffer → analysis worker → latest snapshot → UI。

回调不阻塞、不访问文件/数据库、不等待 UI、不执行复杂分配。UI 不应消费每一个音频样本。

## 6. 数据流与内存

- 文件解码采用块处理。
- 波形使用多级 min/max 包络。
- 声谱图采用分块 STFT 与纹理增量更新。
- 图表数据按视口和像素宽度降采样。
- 所有队列、缓存和历史结果设置上限或淘汰策略。
- 取消任务后尽快停止生产新数据并释放资源。

## 7. 错误处理

使用类型化错误区分：
- 不支持格式；
- 文件损坏或截断；
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
