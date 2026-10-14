# 开发实施计划

## 1. 交付策略

按垂直切片推进，每个阶段都应可编译、可运行、可验证。不要先创建所有空 crate，再一次性实现所有功能。

## 2. 里程碑

### M0：工程骨架（约 1 周）

状态：**已完成（2026-10-14）**

- [x] Cargo workspace 和工具链（`rust-toolchain.toml` 固定 1.99.0，`Cargo.lock` 已提交）
- [x] 最小桌面窗口（`eframe`，可运行、深色、可调整面板比例）
- [x] 主题 Token（`auralis-ui::theme`，含 inferno 256 级色阶与颜色+线型双编码）
- [x] 五区布局骨架（顶部命令栏 / 资源栏 / 中央画布 / 检查器 / 状态栏）
- [x] 日志（`tracing` + `tracing-subscriber`，`RUST_LOG` 可调）
- [x] 错误处理（`auralis-core` 类型化错误：格式 / 配置 / 输入不足 / 非有限值）
- [x] DSP 内核骨架：4 种周期型窗函数、Peak / RMS / DC Offset / Crest Factor、超量程统计、削波平顶检测
- [x] CI（fmt / clippy / test / Windows Release 构建 / `--self-test` 产物自检）
- [x] 架构边界脚本（`scripts/check-boundaries.ps1`，含正反测试）

验收：Windows 可编译启动 —— 已达成（详见下方「M0 验收记录」）。

**M0 刻意未做**：音频解码、FFT、任务调度、实时采集。
界面上的对应控件是**禁用且标注了目标里程碑**的，不做成看似可用但无功能的样子。

#### M0 验收记录

| 项 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | 见 CI 流水线 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 见 CI 流水线 |
| `cargo test --workspace` | 51 项 DSP/core 测试通过 |
| `cargo build --workspace --release` | 见 CI 流水线 |
| `--self-test` 无窗口 DSP 自检 | 通过 |
| 架构边界检查 | 通过（正反用例均已验证） |

### M1：文件与 PCM（约 1–2 周）

状态：未开始

- [ ] 创建 `auralis-codec`（symphonia）
- [ ] 创建 `auralis-task`（任务队列、进度、取消）
- [ ] WAV 首个可靠路径
- [ ] 元数据、分块 PCM
- [ ] 任务进度和取消
- 验收：正常、空、损坏文件均有明确行为

### M2：基础 DSP 与波形（约 1–2 周）

状态：未开始

- [ ] 创建 `auralis-visualization`（多级 min/max 包络）
- [ ] 创建 `auralis-analysis`（分析管线、参数校验）
- [ ] Peak、RMS、DC Offset（与 M0 内核对接，补齐导出）
- [ ] FFT、窗函数、幅值校正（按
      [M0 设计文档](superpowers/specs/2026-10-14-auralis-studio-design.md) §5.3 的归一化约定）
- [ ] 峰值检测与抛物线插值
- [ ] 多级波形包络
- 验收：合成信号测试通过（含设计文档 §5.7 的 T1–T15）

### M3：频谱 UI（约 1–2 周）

状态：未开始

- [ ] 引入 `egui_plot`（已验证可解析：0.37.0）
- [ ] 深色仪器风格图表（坐标轴、网格、刻度）
- [ ] 频率/幅值坐标、游标、缩放
- [ ] 参数检查器接入真实参数、峰值标记
- 验收：视图数据映射与 DSP 输出一致

### M4：实时输入与声谱图（约 2 周）

状态：未开始

- [ ] 创建 `auralis-audio`（cpal + `rtrb` 有界 SPSC）
- [ ] CPAL 设备枚举和输入
- [ ] 实时 Peak/RMS/FFT
- [ ] STFT 与纹理渲染（inferno 色阶）
- [ ] 任务线程 `catch_unwind` 隔离（见 ADR-009）
- 验收：溢出可观测、UI 不阻塞回调

### M5：导出与可靠性（约 1 周）

状态：未开始

- [ ] 创建 `auralis-app`（用例编排 + 持久化 + 导出）
- [ ] CSV/JSON（含算法版本、单位、参数）
- [ ] 配置和历史记录
- [ ] 异常恢复、长文件性能
- 验收：结果可追溯

### M6：发布候选（约 1–2 周）

状态：未开始

- [ ] Windows 安装包
- [ ] 用户手册
- [ ] 依赖许可证清单
- [ ] 回归、性能与长时间运行
- 验收：发布检查表通过

上述周期是有经验开发者的初始规划，不构成承诺。首次使用 Rust、桌面 GUI 或 DSP 时应增加学习缓冲。

## 3. 第一条垂直切片

1. 创建 workspace 和 desktop app。
2. 使用 egui/eframe 显示主窗口。
3. 实现统一深色主题与五区域布局。
4. 导入一个受支持 WAV 文件并读取元数据。
5. 解码为 PCM block。
6. 计算 Peak/RMS。
7. 生成波形概览。
8. 实现最小 FFT 频谱。
9. 添加合成正弦波测试。
10. 在 Windows Release 构建中验证。

## 4. AI 编程协作流程

每个任务都按以下步骤：
1. 读取相关文档和已有代码；
2. 给出简短计划；
3. 修改最小范围的文件；
4. 运行 fmt、Clippy、tests 和 build；
5. 修复回归；
6. 更新文档；
7. 汇报实际执行结果和未解决问题。

## 5. 发布门槛

- 无未解决的高严重度崩溃问题；
- DSP 关键结果通过确定性测试；
- 设备异常和缓冲溢出有明确状态；
- 长文件不导致无界内存增长；
- UI 操作不被离线分析阻塞；
- 许可证和打包流程完成审查；
- 用户手册与实际功能一致。
