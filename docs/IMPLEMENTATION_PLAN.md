# 开发实施计划

## 1. 交付策略

按垂直切片推进，每个阶段都应可编译、可运行、可验证。不要先创建所有空 crate，再一次性实现所有功能。

## 2. 里程碑

### M0：工程骨架（约 1 周）

状态：**已完成（2026-10-14）**

- [x] Cargo workspace 和工具链（`rust-toolchain.toml` 固定 1.99.0，`Cargo.lock` 已提交）
- [x] 最小桌面窗口（eframe，当时的技术栈；已在 M0.5 换成 Tauri）
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
| `cargo test --workspace` | 当时 51 项 DSP/core 测试通过（2026-10-23 复核：全仓 281 项，其中 DSP 81 项；前端另 72 项） |
| `cargo build --workspace --release` | 见 CI 流水线 |
| `--self-test` 无窗口 DSP 自检 | 通过 |
| 架构边界检查 | 通过（正反用例均已验证） |

### M0.5：UI 技术栈切换至 Tauri（已完成 2026-10-15，容器部分已被 M4.5 取代）

状态：**已完成**（Tauri 容器已由 M4.5 的 Electron 取代，React UI 决策不变）

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

状态：**已完成（文件路径 + 拖放）；IPC 延迟实测待补**

- [x] 创建 `auralis-codec`（symphonia，显式开启 mp3/aiff feature）
- [x] 创建 `auralis-task`（取消令牌、任务事件、panic 隔离）
- [x] 文件选择器（迁移前为 `tauri-plugin-dialog`，现为 Electron `dialog`）
- [x] 拖放导入（`onDragDropEvent`，与应用内三条入口同一路径）
- [x] 元数据探测（只读文件头，O(1)）
- [x] 分块 PCM 解码（内存与文件长度解耦）
- [x] `probe_file` / `analyze_file` / `cancel_task` 命令
- [x] **可取消**：任务号由前端分配并在发起时传入（见 ADR-018 备注与 `AppState::try_register_token`）
- [x] **WAV 真实样本端到端验证**：写入 → 解码 → DSP → 期望数值
- [ ] **实测 IPC 端到端延迟**（M0.5 欠项；`--self-test` 覆盖不到 IPC，
      必须在 GUI 中点一次并读状态栏）

### 格式支持现状（必须如实呈现）

| 容器 | 编译进产物 | **已用真实样本验证** |
|---|---|---|
| WAV  | 是 | **是**（13 项集成测试） |
| FLAC | 是 | **是**（5 项集成测试，样本在 `crates/auralis-codec/tests/fixtures/`） |
| MP3  | 是（显式开 feature） | 否 |
| AIFF | 是（显式开 feature） | 否 |

`auralis_codec::verified_containers()` 是 UI 的硬约束：
只有 WAV 与 FLAC 会被标记为「已验证」，其余格式在界面上显示「未验证」角标。
依据 `TEST_PLAN.md` §3「不支持的格式必须明确报错，不得崩溃」与
`AGENTS.md`「不宣称未经验证的格式」。

> symphonia 的 default features **不含** mp3 / aiff / aacc —— 这是实测结论。
> 若不显式开启，UI 会出现 MP3 入口但打开失败。

验收：正常、空、截断、损坏文件均有明确行为（不 panic）；取消能真正中止。

### M2：基础 DSP 与波形（约 1–2 周）

状态：**已完成（2026-10-16）**；界面入口于 2026-10-23 开通

- [x] 创建 `auralis-visualization`（多级 min/max 包络金字塔，15 项测试）
- [x] 引入 `rustfft` 6.4.1
- [x] FFT + 单边幅值谱 + 功率谱 + PSD（三者严格分离）
- [x] 相干增益校正（`A = 2|X[k]|/W`；DC 与 Nyquist **不**乘 2）
- [x] 峰值检测 + 抛物线插值（保留未插值 bin 频率供验证）
- [x] **扇贝损失判据**替代原「能量集中度」判据（见下）
- [x] 真实文件频谱接入 `analyze_file`，峰值带「估算」标记
- [x] **oracle 扩展到覆盖 FFT**：137 项（原 101 项）
- [x] **oracle 扩展到覆盖 FFT**（ADR-015 硬性要求）：137 项
- [x] 建立 criterion 基准：**在 M4 完成**（见 M4 与 [BENCHMARKS.md](BENCHMARKS.md)）

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

- ~~criterion 基准未建~~ → **已在 M4 完成**（[BENCHMARKS.md](BENCHMARKS.md)）。
- 声谱图（STFT）属 M3，已完成。
- 削波平顶检测仍返回 0（原因见 M1）。

### M3：频谱 UI（约 1–2 周）

状态：**已完成（2026-10-16）**；声谱图频率缩放与任务队列于 2026-10-23 开通

- [x] STFT（`auralis-dsp::stft`，19 项测试）：2048/512 默认，末尾不足一帧**丢弃不补零**
- [x] `compute_range`：只算指定区间，内存与请求范围成正比
- [x] 帧起点对齐**全局** hop 网格（同一时间窗口从哪里请求结果一致）
- [x] 声谱图矩阵降采样（固定输出网格，纹理需要固定尺寸）
- [x] `SpectrogramView`：Canvas 2D + ImageData，inferno 色阶，**低频在下**
- [x] 频谱缩放（滚轮以光标为中心）/ 拖拽选区 / Shift 平移 / 双击复位
- [x] 峰值标记：实心=实测、空心+警示色=估算
- [x] 频谱与声谱图共享频率游标

#### 相对 ADR-018 的一处有意偏离

ADR-018 初稿建议声谱图用 WebGL。实现时改为 **Canvas 2D + ImageData**，
理由见 ADR-018 的「声谱图渲染：WebGL → Canvas 2D」一节。
数据契约（行列布局、低频在下、色阶）不变，可随时换回。

#### 频谱/声谱图的时间窗

`analyze_file` 的返回值只附带**开头 30 秒**（`SPECTRUM_WINDOW_SECONDS`），
因为 3 小时文件全量 STFT 是数 GB。文件其它位置由 **M4 视口分块**的
`spectrogram_view` 带偏移请求，只解码并计算目标窗口；UI 的按钮/滚轮/拖动
都在调用它，初始视口不需要额外往返。

#### 遗留

- ~~criterion 基准仍未建~~ → **已在 M4 完成**（[BENCHMARKS.md](BENCHMARKS.md)）。
- 实时采集（CPAL 流）：**已在 M4 接通**（`auralis-audio::capture`）。
- 削波平顶检测仍返回 0。

### M4：实时采集骨架 + 性能基准

状态：**部分完成（2026-10-22）** —— 实时内核、CPAL 输入流、实时频谱链路、
视口分块声谱图已接通；高频通道仍未做。界面导航入口于 2026-10-23 开通

- [x] **建立 criterion 基准**（清偿 M2/M3 欠项）：`crates/auralis-dsp/benches/dsp_bench.rs`
- [x] 实测数据写入 [docs/BENCHMARKS.md](BENCHMARKS.md)
- [x] 创建 `auralis-audio`：`SampleRing` 有界无锁 SPSC（`crossbeam-queue::ArrayQueue`）
- [x] 溢出策略明确：**丢最新 + 计数**，每次丢弃都记入 `dropped_frames`
- [x] 实时安全论证 + 测试（容量不超限、丢帧全计数、关闭后拒写、并发不死锁）
- [x] 设备枚举（不依赖真实声卡，CI 可跑）
- [x] **CPAL 输入流接通**（`capture.rs`）：`start_capture` / `CaptureSession` / 缓冲区回收池
- [x] **格式协商**（`resolve_format`）：不再「请求≠默认就报错」，按设备声明的
      f32 区间选择，不可用时回退默认值并**回报真实格式**（否则频率轴会整体偏移）
- [x] **实时分析工作线程**（`realtime.rs`）：滚动窗口 + FFT + 峰值/RMS → 最新快照槽
- [x] **丢帧可观测**：环形缓冲丢弃（`dropped_frames`）与快照覆盖
      （`snapshot_overwrites`）分别计数，前端两者都显示
- [x] 实时快照走**二进制**（小端 f32 字节流），符合 ADR-018 规则 2
- [x] 前端 `RealtimePanel`：设备选择、开始/停止、丢帧/覆盖计数、实时频谱
- [x] 视口分块声谱图：`spectrogram_view(path, startSeconds, windowSeconds)`；
      解码器支持精确 seek，Rust 只算请求窗口；UI 支持按钮/滚轮/拖动平移
- [x] 实时高频通道决策（WebSocket / SAB+Worklet / 原生插件）——
      **决定不引入**，保留 30 Hz stdio 轮询；依据见
      [ADR-020](ADR.md#adr-020实时高频通道与-asio-评估)（2026-10-22）
- [x] ASIO 评估——**暂不启用**：`cpal/asio` 需要单独下载的 Steinberg SDK，
      当前 WASAPI 实测丢帧为 0，详见 ADR-020
- [x] **真实声卡采集已在一台机器上验证**（2026-10-22）：
      Realtek WASAPI 默认输入 / 2ch / 48 kHz / 6.03 s，
      处理 287,040 帧（0.991 × 实时）、丢帧 0、快照 RPC p95 3.10 ms；
      可复现命令 `node scripts/check-capture.mjs --seconds 6`。
      热插拔、设备切换与 8 小时连续运行仍未验证

#### 实时链路的实测依据

`realtime.rs` 的工作线程设计直接来自 [BENCHMARKS.md](BENCHMARKS.md)：
FFT 4096 需 271 µs，而回调预算只有约 20.8 µs。因此回调只做「取缓冲 → 拷贝 →
入环 → 计数」，FFT 全部在工作线程。

工作线程**只保留最新的 `fft_size` 个单声道样本**。环形缓冲积压时一次
`pop_chunk` 可能取到上万样本，而一次刷新只画一条曲线；保留整段既没人看，
又会让内存与缓冲深度成正比。

多声道数据按各声道**平均**混成单声道。完全反相的立体声（L=+1, R=−1）
混音后是数字静音，界面显示「—」而不是一个假读数——有测试锁定这个行为。

#### 基准给出的关键结论

48 kHz 回调预算 ≈ **20.8 µs**，实测中位数：

| 操作 | 中位数 | 预算倍数 |
|---|---|---|
| FFT 4096 | **271 µs** | **13.0 倍** |
| FFT 1024 | 77 µs | 3.7 倍 |
| 峰值检测 | 65 µs | 3.1 倍 |
| STFT 1 秒 | 3.47 ms | — |
| STFT 区间 0.1 秒 | 264 µs | — |

**这从猜测变成了数据：FFT 绝对不能放进音频回调。**
即使最小的 1024 点 FFT 也超预算 3.7 倍。
ADR-018「回调只做拷贝、全部 DSP 在工作线程」的架构由实测确认。

另外，设计文档 §7.5 曾写「FFT 4096 < 100 µs」——**实测 271 µs，未达标**。
应以 [BENCHMARKS.md](BENCHMARKS.md) 为准，目前**不宣称**任何 FFT 性能指标。

#### 环形缓冲为什么用 `crossbeam-queue` 而不是自己写

- 本项目 `#![forbid(unsafe_code)]`，自己写环形缓冲必须引入 `unsafe`
- `rtrb` 会多一个依赖，而 `ArrayQueue` 无锁、定长、SPSC，且已在依赖树里

### M4.5：桌面容器迁移至 Electron（已完成 2026-10-22）

状态：**已完成**（代码、构建与端到端验证；安装包属 M6）

迁移的触发因素不是「想换技术栈」，而是一个可复现的显示故障：Tauri/WebView2
下窗口最大化**必定**触发屏幕级花屏与任务栏抖动。排查数据（窗口矩形稳定、
刷新率恒定 240 Hz、无 TDR、CDP 截图内容正确、唯一同刻事件是画布后备存储
重建风暴）与最小 Electron 壳的对照实验，完整记录在
[ADR-019](ADR.md#adr-019桌面容器从-tauri-2-迁移到-electron)。

- [x] 画布重建修复（`canvas-fit.ts`）：只在设备像素尺寸真变化时写
      `canvas.width/height`；42 次 draw 只有 12 次写入
- [x] Electron 45 主进程 + preload 窄接口（sandbox / contextIsolation /
      无 Node 集成）
- [x] Rust 命令层从 `src-tauri` 迁到 `crates/auralis-engine`，不再依赖 Tauri
- [x] stdio JSON-Lines RPC：快命令内联、控制泳道 1 worker + 队列 2、慢命令 2 worker + 有界队列 8、
      stdin EOF 先排空已接受任务再退出
- [x] 二进制字段 base64 过线，主进程还原为 `Uint8Array`
- [x] 对话框走 Electron `dialog`；拖放走 DOM + `webUtils.getPathForFile`
- [x] 一条命令启动开发：`npm run electron:dev`
- [x] **IPC 端到端延迟实测**（2026-10-23，走 preload 桥接 `window.auralis.invoke`，
      非人工点击状态栏）：快命令 `app_info` ≈1 ms、控制泳道 `stop_realtime` ≈1–2 ms、
      `input_devices` ≈94 ms（**debug 引擎**构建；release 构建更快）。
      文件分析路径的延迟另计：900 秒 WAV 经 Electron 全链路 14.6 s（debug 构建），
      同一文件直接走引擎 release 构建约 1.2 s——差异来自构建 profile，不是 IPC 开销。
      release 构建下的端到端数字仍需补测。
- [x] 真实 WAV 走通引擎 RPC 的 probe / analyze / CSV+JSON 导出
- [x] Electron 渲染进程 → preload → 引擎的 `probe_file` / `analyze_file`
      验证；二进制字段到达渲染层为 `Uint8Array`
- [x] 真窗口渲染验证（三块画布均有内容）

#### 迁移**没有**解决/验证的事

- 真实声卡采集仍未验证（M4 既有欠项，与容器无关）。
- Electron 只在这台机器、这个 GPU/驱动组合上验证过，不构成通用保证。
- 安装包、签名、自动更新尚未开始（M6）。
- 拖放路径只做了类型检查，尚未用真实文件拖放验证。

### M5：导出与可靠性（约 1 周）

状态：**已完成（2026-10-22）** —— 导出、布局持久化与进程级会话恢复已落地；
`auralis-app` 用例层仍刻意推迟。偏好设置对话框与全局搜索于 2026-10-23 开通

- [x] 创建 `auralis-storage`（导出渲染 + 原子写盘；11 项测试）
- [x] CSV/JSON（含算法版本、单位、参数；`#` 元数据行 / JSON `units` 块）
- [x] 扩展名与格式不符时**拒绝导出**，不留「后缀是 .json 的 CSV」
- [x] 原子写盘（同目录 `.part` + `rename`，失败清理临时文件）
- [x] 工作区布局持久化（面板尺寸与可见性，`localStorage`，键带版本号）
- [x] **任务级 panic 隔离**：`isolate_analysis` 把分析中的 panic 变成一条
      `Err`（含文件名与 panic 文本），界面显示错误条而不是进程消失
- [ ] `auralis-app`（用例层）—— **刻意推迟**：目前命令层本身就是用例，
      提前抽一层只会多一层转发，等出现「批量分析 / 会话恢复」这类真实
      编排需求再建，符合 AGENTS「不预建空 crate」
- [x] **会话/崩溃恢复（进程级）**：
  - Electron 主进程维护 `auralis-running.lock`：启动写入、`will-quit` 删除；
    文件仍在 = 上次未正常退出
  - 渲染层把上次成功打开的文件路径写入 `auralis.session.v1`
  - 启动时自动重新分析上次文件；异常退出时状态栏提示「上次会话未正常结束」
  - 实测：强杀 → 重启恢复 + 异常提示；正常关闭 → 重启恢复 + 普通提示
  - 恢复的是「文件 + 分析」，不是崩溃瞬间的未返回值；分析中被强杀时，
    重启会重新跑一遍（结果可复现，代价是一次解码）
- 验收：结果可追溯 —— CSV/JSON 均已覆盖（算法版本 / 规格修订 / 参数 / 单位）
- 已知限制：运行标记写入失败时只失去崩溃检测，不影响启动；恢复路径若
  文件已不存在，会按普通打开错误呈现

#### 为什么导出要重新分析一次

前端手里的频谱/包络是**已降采样到像素宽度**的视图数据，且传回数百 KB~数 MB
才能导出。导出必须能独立复现，又要和屏幕上的数字同源，所以 Rust 侧重跑
同一条 `run_analysis` 路径，格式化也留在 Rust（前端不碰数值，ADR-018 规则 1）。
代价是一次解码，换来「导出内容 = 显示内容 + 可追溯」。

#### 数值约定（导出）

- 非有限值（静音 = `-inf`）在 JSON 里是 `null`、CSV 里是**空字段**，
  绝不写成 `0`；两者都用 `non_finite_values=<n>` 报出计数。
- 波形包络的来源如实写成「第一个声道的按桶最大值」——不是所有声道的 min/max。

### M6：发布候选（约 1–2 周）

状态：未开始

- [ ] 安装包（Electron 打包方案待定；必须同时携带 Chromium、
      `auralis-engine.exe` 与许可证清单）
- [ ] 用户手册
- [ ] 依赖许可证清单（含 npm 侧）
- [ ] 长时间运行稳定性（崩溃恢复已在 M5 完成）
- 验收：发布检查表通过

上述周期是有经验开发者的初始规划，不构成承诺。首次使用 Rust、桌面 GUI 或 DSP 时应增加学习缓冲。

## 2.6 界面入口全部开通（2026-10-23）

状态：**已完成**

M0 以来界面上有 8 个入口一直是「禁用 + 标注里程碑」的状态。它们的**计算
能力大多早已存在**（`task_queue` / `cancel_task` / `app_info` 命令已实现；
实时采集面板早已可用；搜索面板 `SearchPalette.tsx` 早已在树内但未接线），
缺的是界面表达。本轮**没有改动任何 Rust crate**。

- [x] **声谱图频率缩放**：与频谱共享频率视口。缩放是**裁剪**（只画可见的
      bin 并铺满绘图区）而不是拉伸整张图 —— 后者会破坏「频率 ↔ bin」的
      对应关系。矩阵是 512 个 bin（不是 1024），`binHz = 奈奎斯特 / bin 数`。
- [x] **修正采样率错标**：声谱图画的是文件的矩阵，频率轴此前却用了实时
      采集设备的采样率 —— 实时开着时频率刻度整体错位，而画面看不出异常。
- [x] **「电平 / 指标」视图**：中央画布新增卡片（电平 / 信号指标 / 数据质量），
      数值全部来自 Rust 已算好的结果。
- [x] **后台任务队列**：左侧栏面板，运行中任务可取消；「最近完成」由前端
      记录（Rust 侧任务结束即注销，只暴露运行中的任务）。
- [x] **实时输入导航入口**与「采集中」状态标记。
- [x] **偏好设置**：统一了散落的多处偏好（单一存储键 + 旧键迁移），
      新增包络点数与「启动时恢复上次文件」。
- [x] **全局搜索**：文件 / 峰值 / 操作三组命令面板。
- [x] **帮助**与**关于**对话框。
- [x] **前端单元测试基座**：新增 vitest（仅 devDependency），CI 前端 job
      增加 `npm run test`。新增的可测逻辑（偏好校验与迁移、视口换算、
      任务历史、搜索匹配）抽成无 DOM 依赖的纯函数。

设计与取舍见 [设计文档](superpowers/specs/2026-10-22-enable-pending-ui-features-design.md)，
实施步骤见 [实施计划](superpowers/plans/2026-10-23-enable-pending-ui-features.md)。

## 3. 第一条垂直切片

1. 创建 workspace 和 desktop app。
2. 显示主窗口（M0 用 egui，M0.5 起为 Web UI；M4.5 起容器为 Electron）。
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
