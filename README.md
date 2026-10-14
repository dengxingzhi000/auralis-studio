# Auralis Studio

Auralis Studio 是一款 Windows 优先的专业桌面音频分析软件，采用 Rust workspace、多 Crate 分层、
可测试 DSP 内核和深色专业仪器风格 UI。

**当前状态：M0 工程骨架。** 可以编译、可以启动、可以跑 DSP 自检。
文件导入、频谱计算、声谱图、实时采集均**尚未实现** —— 界面上对应按钮是灰的，
中央画布显示「尚未加载音频」而不是假数据。详见下方「里程碑」。

## 文档入口

| 文档 | 内容 |
|---|---|
| [产品需求文档](docs/PRD.md) | 用户、版本范围、功能与非功能需求、产品边界 |
| [技术架构设计](docs/TAD.md) | 分层、依赖方向、线程模型、错误分类 |
| [DSP 算法规范](docs/DSP_SPEC.md) | RMS / Peak / DC Offset / FFT / STFT / 削波的定义与误差目标 |
| [UI/UX 设计规范](docs/UI_SPEC.md) | 设计 Token、五区布局、视图组件、图表性能约束 |
| [测试与验收计划](docs/TEST_PLAN.md) | 测试分层、合成信号集、验收目标、CI 命令 |
| [开发实施计划](docs/IMPLEMENTATION_PLAN.md) | M0–M6 里程碑与垂直切片 |
| [架构决策记录](docs/ADR.md) | ADR-001 … ADR-007 |
| [M0 设计文档](docs/superpowers/specs/2026-10-14-auralis-studio-design.md) | DSP 数值约定的完整推导与测试清单 |
| [开发环境设置](docs/DEV_ENVIRONMENT.md) | 本机工具链、镜像与路径约定 |

## 里程碑进度

| 里程碑 | 内容 | 状态 |
|---|---|---|
| **M0** | workspace、工具链、深色主题、五区布局、日志、CI、DSP 内核骨架 | ✅ 已完成 |
| M1 | WAV 导入、元数据、分块 PCM、任务进度与取消 | ⬜ 未开始 |
| M2 | 基础 DSP 与波形：FFT、幅值校正、多级波形包络 | ⬜ 未开始 |
| M3 | 频谱 UI：坐标轴、游标、缩放、峰值标记、参数检查器 | ⬜ 未开始 |
| M4 | 实时输入与声谱图：CPAL、有界 SPSC 缓冲、STFT、纹理渲染 | ⬜ 未开始 |
| M5 | 导出与可靠性：CSV/JSON、配置持久化、异常恢复 | ⬜ 未开始 |
| M6 | 发布候选：安装包、用户手册、许可证清单 | ⬜ 未开始 |

M0 已实现的、**可验证**的能力：

- `auralis-core`：交错 PCM 数据模型、帧基时间轴、dBFS 换算（保留真实 `-inf`）、类型化错误、算法版本标识。
- `auralis-dsp`：4 种周期型窗函数（含相干增益与 ENBW）、Peak / RMS / DC Offset / Crest Factor、超量程统计、削波平顶启发式检测。全部带确定性单元测试。
- `auralis-ui`：设计 Token（含 inferno 色阶）、颜色+线型双编码声道样式、五区工作台骨架、频谱/波形空画布、状态栏帧率读数。
- `apps/auralis-desktop`：eframe 入口、`tracing` 日志、`--version` / `--self-test`。

## 构建与运行

需要 Rust `1.99.0`（由 `rust-toolchain.toml` 固定）与 MSVC 链接器。
本机的具体安装方式见 [docs/DEV_ENVIRONMENT.md](docs/DEV_ENVIRONMENT.md)。

```powershell
cargo run --release -p auralis-desktop              # 启动图形界面
cargo run --release -p auralis-desktop -- --version # 版本信息
cargo run --release -p auralis-desktop -- --self-test  # 无窗口 DSP 自检
```

### 本地质量检查（与 CI 完全一致）

```powershell
./scripts/check-boundaries.ps1   # 架构边界
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

## 工程原则

1. DSP 与 UI、文件 I/O、操作系统 API 解耦，并由 `scripts/check-boundaries.ps1` 强制。
2. 实时音频回调不阻塞、不进行无界分配、不等待 UI。
3. 所有核心算法均有确定性数值测试，测试信号由代码生成。
4. 分块处理大型音频文件，控制内存上限。
5. 明确每个指标的单位、归一化方式、适用条件与算法版本。
6. **不宣称未通过验证的格式、性能或测量准确度。**

## 当前已知的限制

- 只有 WAV 相关能力完全未实现；解码器尚未接入，因此**当前不支持任何文件格式**。
- 不提供校准级 SPL 测量；界面中不会出现 `dB SPL` 字样，`dBFS` 指数字满刻度。
- SNR、THD、THD+N、响度（LUFS）**未实现**，界面上不显示占位数值。
- Release 构建未做跨机器性能基准，60 FPS 目标尚无实测数据。
- macOS / Linux 未验证（CI 会跑检查，但 GUI 与音频路径未适配）。