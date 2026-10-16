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

### M0.5：UI 技术栈切换至 Tauri（已完成 2026-10-15）

状态：**已完成**

- [x] `auralis-ui`（egui，1716 行）废弃，保留 git tag `m0-egui-baseline`
- [x] `apps/desktop` Tauri 2 + React 19 + TS + Vite 骨架
- [x] 设计令牌迁移到 CSS Variables；声道颜色+线型双编码保留
- [x] 五区工作区 + 可拖拽分隔条
- [x] 自研 `SpectrumView` / `WaveformView`（Canvas 2D）
- [x] 二进制 IPC 通路打通：`analyze_synthetic_signal` Command
- [x] **IPC 载荷实测**（ADR-018 修正依据）：二进制 12.5 KB/帧 vs JSON 34.8 KB/帧
- [x] `check-boundaries.ps1` 扩展为同时守护前端（禁止 React 做音频运算）
- [x] CI 改为双栈：Rust job + 前端 job + DSP oracle job

验收：合成信号可跑通「Rust 计算 → 二进制 IPC → Canvas 渲染」全链路。

> **未做**：未在真实图形会话中打开过窗口；IPC **端到端延迟未测**（只有载荷体积）。
> 下一步 M1 接入真实文件时应补这个数据。

### M1：文件与 PCM（约 1–2 周）

状态：未开始

状态：**已完成（文件路径）；IPC 延迟实测待补**

- [x] 创建 `auralis-codec`（symphonia，显式开启 mp3/aiff feature）
- [x] 创建 `auralis-task`（取消令牌、任务事件、panic 隔离）
- [x] 文件选择器（`tauri-plugin-dialog`）；拖放待补
- [x] 元数据探测（只读文件头，O(1)）
- [x] 分块 PCM 解码（内存与文件长度解耦）
- [x] `probe_file` / `analyze_file` / `cancel_task` 命令
- [x] **WAV 真实样本端到端验证**：写入 → 解码 → DSP → 期望数值
- [ ] **实测 IPC 端到端延迟**（M0.5 欠项；`--self-test` 覆盖不到 IPC，
      必须在 GUI 中点一次并读状态栏）
- [ ] 拖放导入

### 格式支持现状（必须如实呈现）

| 容器 | 编译进产物 | **已用真实样本验证** |
|---|---|---|
| WAV  | 是 | **是**（13 项集成测试） |
| FLAC | 是 | 否 |
| MP3  | 是（显式开 feature） | 否 |
| AIFF | 是（显式开 feature） | 否 |

`auralis_codec::verified_containers()` 是 UI 的硬约束：
只有 WAV 会被标记为「已验证」，其余格式在界面上显示「未验证」角标。
依据 `TEST_PLAN.md` §3「不支持的格式必须明确报错，不得崩溃」与
`AGENTS.md`「不宣称未经验证的格式」。

> symphonia 的 default features **不含** mp3 / aiff / aacc —— 这是实测结论。
> 若不显式开启，UI 会出现 MP3 入口但打开失败。

验收：正常、空、截断、损坏文件均有明确行为（不 panic）；取消能真正中止。

### M2：基础 DSP 与波形（约 1–2 周）

状态：**已完成（2026-10-16）**

- [x] 创建 `auralis-visualization`（多级 min/max 包络金字塔，15 项测试）
- [x] 引入 `rustfft` 6.4.1
- [x] FFT + 单边幅值谱 + 功率谱 + PSD（三者严格分离）
- [x] 相干增益校正（`A = 2|X[k]|/W`；DC 与 Nyquist **不**乘 2）
- [x] 峰值检测 + 抛物线插值（保留未插值 bin 频率供验证）
- [x] **扇贝损失判据**替代原「能量集中度」判据（见下）
- [x] 真实文件频谱接入 `analyze_file`，峰值带「估算」标记
- [x] **oracle 扩展到覆盖 FFT**：137 项（原 101 项）
- [ ] 建立 criterion 基准（ADR-014 要求）—— **仍缺**，见「遗留」
- [x] **扩展 `scripts/oracle.py` 覆盖 FFT**（ADR-015 硬性要求）：137 项

验收：设计文档 §5.7 的 T1–T15 全通过，oracle 对 FFT 全部一致。

#### 关于「估算」判据的一次修正

原设计文档写「谱峰度 < 0.9 时标注估算」。实测发现该判据**几乎永不触发**：

| 信号 | 主瓣能量占比（bin ±1） |
|---|---|
| 相干正弦 | 1.000 |
| 偏离 0.37 bin | 0.991 |
| 偏离 0.5 bin | 0.980 |

纯正弦无论相干与否，能量都高度集中在主瓣内，用它判断插值可靠性没有意义。
改用**扇贝损失**（bin 读数与插值读数之差）后判据才可用：偏离 0.5 bin 时约 1.4 dB。

另外确认了一个数值事实：**恰好半个 bin 偏移时抛物线插值是病态的**。
主瓣严格对称，三点抛物线接近水平，delta 解析式极敏感，
实测插值结果（-9.19 dBFS）反而比 bin 读数（-7.44 dBFS）更差。
这不是 bug，而是该方法的固有极限；`Peak::is_estimated()` 把这类读数标为「估算」。

#### 遗留

- **criterion 基准未建**。ADR-014 要求「没有基准就无法判断 Rust 是否够快」，
  本次仍未做基准，因此**不能**声称 FFT 性能达标。
- 声谱图（STFT + WebGL）属 M3/M4，未实现。
- 削波平顶检测仍返回 0（原因见 M1）。

### M3：频谱 UI（约 1–2 周）

状态：未开始

- [ ] 频率游标、缩放、平移、峰值标记
- [ ] 参数检查器接入真实参数
- [ ] 声谱图：WebGL 纹理（整块矩阵一次传输，见 ADR-018）
- 验收：视图数据映射与 DSP 输出一致

### M4：实时输入与声谱图（约 2 周）

状态：未开始

- [ ] 创建 `auralis-audio`（cpal + `rtrb` 有界 SPSC）
- [ ] **决策实时高频通道**（ADR-018 留待实测：WebSocket / SAB+Worklet / 原生插件）
- [ ] 实时电平表与频谱
- [ ] 任务线程 `catch_unwind` 隔离（ADR-009）
- 验收：溢出可观测、UI 不阻塞回调

### M5：导出与可靠性（约 1 周）

状态：未开始

- [ ] 创建 `auralis-app` / `auralis-storage`（用例、持久化、导出）
- [ ] CSV/JSON（含算法版本、单位、参数）
- [ ] 工作区布局持久化（`Workspace` 面板尺寸）
- 验收：结果可追溯

### M6：发布候选（约 1–2 周）

状态：未开始

- [ ] NSIS/MSI 安装包（`tauri build` 已配置）
- [ ] 用户手册
- [ ] 依赖许可证清单（含 npm 侧）
- [ ] 长时间运行与崩溃恢复
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
