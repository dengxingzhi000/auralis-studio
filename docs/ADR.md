# Architecture Decision Records（ADR）

## ADR-001：Rust workspace 与多 Crate

状态：Accepted

### 背景
项目同时包含 DSP、实时音频、解码、UI、持久化和报告导出，需保持算法可测试并控制依赖方向。

### 决策
使用 workspace；按稳定职责划分 crate。允许在早期合并尚无独立边界价值的模块。

### 后果
边界更清晰，但需避免过度拆分和循环依赖。

## ADR-002：egui/eframe 作为首版 UI

状态：Proposed

### 背景
项目需要 Rust 原生桌面 UI、快速迭代和自定义绘图。

### 决策
首版使用 egui/eframe；普通二维曲线可使用 egui_plot，高密度声谱图采用纹理绘制。

### 验证
制作包含波形、FFT 和声谱图的垂直切片，测试窗口缩放、高 DPI 和持续刷新性能。

### 备选
若后续需要更复杂的原生桌面体验或大量自定义 GPU 绘制，可重新评估其他框架，但不得在无原型基准前迁移。

## ADR-003：统一 PCM 数据模型

状态：Accepted

### 决策
解码器与实时输入适配层向分析层提供统一 PCM block，携带采样率、声道数和帧偏移。

### 后果
算法可以复用；需要严谨处理交错声道、部分帧、格式转换和时间轴。

## ADR-004：实时采集使用有界缓冲

状态：Accepted

### 决策
设备回调与分析工作线程通过预分配的有界 SPSC 缓冲交接数据。

### 后果
需要定义满载策略、丢帧指标和关闭顺序；不能假设工作线程永远跟得上。

## ADR-005：本地优先

状态：Accepted

### 决策
音频默认只在本机处理。用户主动导出时才生成外部文件；联网功能不属于 V1 核心范围。

### 后果
隐私风险较低，离线可用；自动云同步不在首版范围。

## ADR-006：测量准确性以测试为准

状态：Accepted

### 决策
所有算法声明都需公式、单位、前置条件和可重复测试。未验证的高级指标不得显示伪造数字。

### 后果
功能上线速度可能更慢，但提高结果可信度和可维护性。

## ADR-007：M0 只创建 4 个 crate，其余按里程碑增量创建

状态：Accepted（M0）

### 背景

`TAD.md` §2 列出了 12 个 crate，但同一节也写明：「Crates 应按实际依赖逐步创建。
目录列表不是要求首日为每个 crate 添加空壳或复杂接口。」
`IMPLEMENTATION_PLAN.md` §1 同样要求「不要先创建所有空 crate」。

若 M0 就建 12 个空 crate，会立刻产生一批只有空 `lib.rs` 和未使用依赖声明的 crate，
而 CI 的 `-D warnings` 与 `missing_docs` 会让它们变成噪声；更糟的是它们会让人
误以为模块边界已经建立。

### 决策

M0 只创建 4 个**各自有真实内容**的 crate：

| Crate | M0 内容 |
|---|---|
| `auralis-core` | 领域类型、错误、时间轴、单位、版本标识 |
| `auralis-dsp` | 窗函数 + Peak/RMS/DC/Crest/削波 |
| `auralis-ui` | 设计 Token、五区布局、视图骨架 |
| `apps/auralis-desktop` | eframe 入口、日志、CLI |

`auralis-codec` / `auralis-analysis` / `auralis-audio` / `auralis-visualization` /
`auralis-task` / `auralis-application` / `auralis-project` / `auralis-storage` /
`auralis-report` 在 M1–M5 按实际内容创建。

### 后果

- workspace 从第一行代码起就做到「每个 crate 都有存在理由」，依赖方向立即可验证。
- 合并决策记录在实现计划里，而不是靠提前建空目录来「预留」。

## ADR-008：架构边界由脚本强制检查，不靠纪律

状态：Accepted（M0）

### 背景

`AGENTS.md`「Architecture rules」要求 `auralis-core` 和 `auralis-dsp` 不得依赖
UI、设备 API、文件系统或数据库。这类约束靠自觉必然衰减——某天有人为了方便加了
`std::fs::read_to_string`，code review 很难稳定注意到。

### 决策

新增 `scripts/check-boundaries.ps1`，扫描两处：

1. `crates/auralis-core/Cargo.toml`、`crates/auralis-dsp/Cargo.toml` 的依赖声明；
2. 这两个 crate 下所有 `.rs` 的**代码**（先剥离注释，因为文档里常出现
   「不得使用 `std::fs`」这类说明，否则会误报）。

命中即退出码 1。该脚本同时接入 CI 的 `check` job。

### 验证

已做正反测试：注入 `fn boundary_canary() -> egui::Color32`，以及在
`auralis-dsp/Cargo.toml` 增加 `egui = { workspace = true }`，两次均被捕获并返回 1；
撤销后返回 0。

### 后果

边界从「约定」变成「可执行断言」。代价是脚本本身需要维护，且新增纯 crate 时
需要把名字加进 `$pureCrates`。

## ADR-009：release profile 刻意不使用 `panic = "abort"`

状态：Accepted（M0）

### 背景

`panic = "abort"` 能减小体积、消除 unwinding 开销，是常见的 release 优化。
但 `AGENTS.md` 要求「异常文件不能导致整个应用崩溃」「后台任务失败应可以
定位、取消或重试」。

### 决策

不设置 `panic = "abort"`。原因：工作线程（M3 起）将依赖 `catch_unwind` 做任务级
隔离——一个损坏文件导致的 panic 应只让那个任务失败，不应终止整个进程。
`panic = "abort"` 与该设计直接冲突。

### 后果

release 二进制略大、略慢。若将来确认不需要 `catch_unwind`，可重新评估。

## ADR-010：手写五区布局，不引入 `egui_dock`

状态：Proposed（M0）

### 背景

egui 没有原生 dock 系统，常见做法是引入 `egui_dock`。

### 决策

M0 手写 `TopBottomPanel` / `SidePanel` / `CentralPanel` 组合，只实现
`UI_SPEC.md` §3 要求的固定五区加一个竖向分隔条。

### 理由

- 需求是**固定五区**，不是自由停靠；引入 dock 库解决的是不存在的问题。
- `egui_dock` 自带一套 `TabDefinition` 状态，与 `WorkspaceLayout` 的序列化模型
  构成第二套状态源，两者同步是长期 bug 来源。
- 少一个第三方依赖，少一份许可证审查（`IMPLEMENTATION_PLAN.md` §5 发布门槛）。

### 后果

M0 布局固定。若 M4 之后确实需要自由停靠，重新评估该决策，并接受新增依赖
与 `WorkspaceLayout` 的迁移成本。

## ADR-011：镜像配置属于机器，不进版本库

状态：Accepted（M0）

### 背景

本机直连 `static.rust-lang.org` 约 28 KB/s、`crates.io` 约 80 KB/s，
且本机代理对这两个域名 TLS 握手失败。USTC 镜像实测 860 KB/s / 370 KB/s。

### 决策

- `RUSTUP_DIST_SERVER` / `RUSTUP_UPDATE_ROOT` 写入**用户环境变量**。
- Cargo 的 `[source.crates-io] replace-with` 写入 **`CARGO_HOME\config.toml`**。
- 仓库内 `.cargo/config.toml` 只放与项目相关的设置（alias、终端颜色等）。

### 理由

镜像属于「这台机器处在哪个网络」的事实，不是项目属性。写进仓库会把某个地区的
镜像强加给 GitHub Actions（海外 runner）和其它开发者。

### 后果

镜像配置不随仓库分发，换机器需按 `DEV_ENVIRONMENT.md` §3–§4 重做。
`Cargo.lock` 的校验和仍由 Cargo 强制校验，供应链安全性不受影响。

## ADR-012：`--self-test` CLI：让「二进制能跑」成为可自动化的断言

状态：Accepted（M0）

### 背景

`TEST_PLAN.md` §5 要求「Windows Release 构建可重复」。但 GitHub 的 Windows
runner 没有交互式桌面会话，`eframe::run_native` 无法开窗，因此「Release 产物
确实能启动」无法被 CI 直接验证——只能靠人手工双击。

### 决策

`auralis-studio --self-test` 不开窗，执行真实的 DSP 计算并打印结果后退出，
`--version` 打印版本与算法版本。CI 在 Release 构建后运行这两个命令。

该命令调用的是真实的 `analyze_channel` 与 `WindowFunction::coefficients`，
不是占位 `assert!`，因此它同时验证了「能加载」「能链接」「能执行浮点数值代码」
三件事。

### 后果

CI 多了一条廉价的强断言。代价是多维护一个 CLI 分支。

## ADR-013：静态链接 CRT，使产物成为真正独立的单文件 exe

状态：Accepted（M0 收尾）

### 背景

M0 收尾时用 `dumpbin /dependents` 检查产物，发现
`target/release/auralis-studio.exe`（13.93 MB）存在对
`VCRUNTIME140.dll` 与 `api-ms-win-crt-*.dll` 的**硬导入**
（`dumpbin /imports` 确认，不是延迟加载；`/delayloads` 为空）。

`rustc --print cfg --target x86_64-pc-windows-msvc` 中没有 `crt-static`
特性，说明本机该目标默认**动态**链接 MSVC CRT。

### 影响

`VCRUNTIME140.dll` 来自「Microsoft Visual C++ Redistributable」，不是
Windows 系统自带组件。没有装过该运行库的机器上双击即报
「找不到 VCRUNTIME140.dll」，程序根本不启动——这不符合
`IMPLEMENTATION_PLAN.md` §5「Windows Release 构建可重复」与分发预期。

### 决策

在仓库 `.cargo/config.toml` 中对 `x86_64-pc-windows-msvc` 目标启用
`-C target-feature=+crt-static`，把 CRT 静态链入可执行文件。

### 验证

- `dumpbin /imports` 中 `VCRUNTIME140` / `api-ms-win-crt` 命中数 **4 -> 0**。
- 剩余依赖全部是 Windows 系统 DLL（`kernel32`、`user32`、`gdi32`、
  `opengl32`、`dxgi` 等），任何 Windows 10/11 都自带。
- 把 exe 单独复制到空目录运行 `--self-test`，**exit 0**。
- 改动链接模型后重跑 `cargo test --workspace`：**69 passed / 0 failed**。
- 体积代价实测 **13.93 MB -> 14.11 MB（+0.18 MB）**。

### 后果

- 单文件可直接分发与双击运行，无需安装运行库。
- 代价是体积略增，且不能跨模块边界传递 CRT 私有类型（`FILE*` 等）。
  本项目为纯 Rust，不受影响；若将来引入 C/C++ FFI 需重新评估。
- 该设置只作用于 MSVC 目标，不影响 macOS/Linux 构建。

### 声谱图渲染：WebGL → Canvas 2D（M3 修正）

初稿写「声谱图：WebGL 纹理」。M3 实现时**有意改为 Canvas 2D + ImageData**，理由：

1. **数据量本就适中。** 矩阵已按画布尺寸降采样（M3 上限 1024×512），
   约 52 万像素 ≈ 2 MB RGBA，一次 `putImageData` 足够；WebGL 的
   `texImage2D` 在这个量级未必更快。
2. **可验证性。** 本项目没有图形会话，WebGL shader 编译失败只能靠人工发现；
   Canvas 2D 路径可以在无头环境下做像素级断言。
3. **瓶颈不在光栅化。** Rust 侧 STFT + IPC 才是大头。

若将来矩阵规模上到 10⁷ 像素量级（超长文件全局声谱图），再换 WebGL。
**数据契约（`Uint8Array` 行列布局、行=时间、低频在下、inferno 色阶）保持不变**，
届时只换渲染实现。

## ADR-014：技术栈不设限，按证据引入

状态：Accepted（M0 收尾）

### 背景

项目方提出：「项目不一定需要纯 Rust 语言开发，有更好的实践可以组合开发。」
进一步确认后，其出发点是 **「只是想架构上更开放、不设限」**，
而不是要复用特定 C/C++ 库、也不是对 Rust 性能有具体疑虑。

### 关键区分

**「保持开放」与「预先引入 FFI」是两件相反的事。**

| 做法 | 得到 | 付出 |
|---|---|---|
| 搭 FFI 骨架（bindgen/cbindgen） | 形式上的开放 | 破坏 DSP 可脱离 UI 测试；与 ADR-013 的 `crt-static` 冲突；当前零用途 |
| 预先把 DSP 换成 C/C++ 实现 | 确定性（性能上） | 未测量即承诺，违反 `AGENTS.md`「不宣称未经验证的性能」 |
| 记录决策 + 建测量基线 | 未来决策有证据 | 接近零 |

### 决策

1. **不锁定语言**。Rust 是**当前**最优选择，不是永久约束。
2. **不预先引入 C/C++ FFI**。引入门槛 = 出现**实测**性能数据表明 Rust 不足。
   在此之前，任何 FFI 提案需要先提交 benchmark 报告。
3. **保留可插拔接缝**，使未来的替换是局部改动：
   - `auralis-dsp` 仅依赖 `auralis-core`，FFT 实现可整体替换而不触及 UI 与解码层；
   - 架构边界由 `scripts/check-boundaries.ps1` 强制，但该脚本的
     `$pureCrates` 列表是**策略**而非法律——新增 crate 时可经 ADR 讨论后调整；
   - `crt-static`（ADR-013）是 `.cargo/config.toml` 中的一行，可随时撤销。
4. **混合工具链是允许且推荐的**，只要不进产品运行时：
   - **Python（numpy/scipy）作为独立参考实现与黄金数据集**（见 ADR-015）；
   - 打包工具（WiX / NSIS）、CI 脚本（PowerShell / YAML）。

### 后果

- 「为什么不用 C++」这个问题有了明确答案：**先测，再决定**，而不是永久禁止。
- M2 引入 FFT 时**必须同时建立 criterion 基准**，否则这条 ADR 无法执行。
- 若将来确有 FFI 需求，需重新评估 ADR-013（静态 CRT 与 C++ 跨模块边界不兼容），
  并在本 ADR 追加后续条目说明触发证据。

### 备选方案

- **现在就引入 C++ DSP 层**：性能上确定收益，但需要先把 M2 的 FFT 与
  STFT 实现出来才能测，在 M0 阶段无法提供任何依据。拒绝。
- **完全不提及语言选型**：会导致后续反复讨论同一问题。拒绝。

## ADR-015：用 numpy 独立参考实现交叉验证 DSP

状态：Accepted（M0 收尾）

### 背景

`docs/TEST_PLAN.md` §8 要求关键算法具有「参考实现或黄金数据集」。
M0 完成后这一条仍是空缺：只有 `auralis-dsp` 自己的单元测试。

单元测试的固有局限：它只能证明「实现与作者的预期一致」，
无法排除「实现和预期**同时**错了」。例如把 0.5 FS 正弦的 RMS
期望值误写成 −3.01 dBFS（实际是波峰因数），实现与测试会一起错，
测试照样全绿 —— M0 期间确实发生过这个错误。

### 决策

引入 `scripts/oracle.py`，用 **numpy 独立重算**同一批标准信号，
与产品代码的输出逐项对照。

数据流：

```text
auralis-studio --dump-reference <path>   产品代码算出结果 -> JSON
                    |
                    v
scripts/oracle.py --rust-json <path>     numpy 独立重算 -> 逐项对照
                    |
                    v
        一致: exit 0      不一致: exit 1
```

`scripts/check-oracle.ps1` 封装定位 Python / exe / 临时文件，
CI 直接调用它。

### 覆盖范围（M0 当前）

- 7 组标准信号：1 kHz/0.5 FS、1 kHz/1.0 FS、1 kHz/1.5 FS（超量程）、
  全零、纯直流 0.1、含 NaN/Inf、显式削波平顶
- 每组的 Peak / RMS / DC / Crest / dBFS，以及
  超量程计数、削波片段数与长度、非有限值清理计数（这些是**整数/布尔**，
  要求完全相等，不容误差）
- 4 种窗函数的相干增益、ENBW(bin)、系数和、前 8 个系数
- 专项：静音的 dBFS 必须是 `null`（对应 Rust 的真 `-inf`），
  而不是被钳位或替换过的「假精确数字」

共 101 项对照。

### 容差选择

- dB 类：绝对容差 **0.01 dB**。不用相对容差，因为低电平处误差会被放大。
- 线性幅值：绝对 **1e-5 FS**。
- 计数 / 布尔：**完全相等**。

### 验证

已做负例测试，确认它不是「永远通过」的橡皮图章：

| 注入的错误 | 结果 |
|---|---|
| RMS dB 偏 0.5 dB | **exit 1**，精确报出差值与容差 |
| 静音的 `-inf` 换成 `-140.0` | **exit 1** |
| Hann ENBW 改成 1.6 | **exit 1** |

正常情况下 101 项全部一致，**exit 0**。

### 与 ADR-014 的关系

这是「技术栈不设限」的第一个实际落地点：
**产品代码保持纯 Rust，验证层用 Python。**

Python 不进入 exe、不参与运行时、不影响发布体积。

### 后果

- DSP 结果有了一个**独立于作者意图**的证据来源。
- M2 引入 FFT / STFT 后，oracle 必须同步扩展 —— 否则新算法就没有交叉验证。
  该要求已写入 `IMPLEMENTATION_PLAN.md` 的 M2 验收项。
- CI 依赖 Python 环境。Windows runner 自带 Python + numpy，无需额外安装步骤。

### 备选方案

- **只加更多 Rust 单元测试**：拒绝。单侧断言无法发现「预期本身错了」。
- **固定黄金数据集（二进制文件）**：拒绝作主方案——它由本实现生成，
  同样可能带着同样的错误。用独立实现更可信。
  黄金数据集仍可在 M2 后作为补充，用于捕捉跨平台/跨编译器差异。

## ADR-016：UI 技术栈从 egui/eframe 切换为 Tauri 2 + React

状态：Accepted（2026-10-15，M0 收尾后、M1 开始前）

### 背景

M0 用 egui/eframe 实现了五区工作台骨架。ADR-010 当时记录：
「M0 布局固定。若 M4 之后确实需要自由停靠，重新评估该决策，
并接受新增依赖与 `WorkspaceLayout` 的迁移成本。」

项目方随后确定目标 UI 能力为：可停靠布局、复杂工作区交互、
快捷键体系、密集参数面板、原生级无障碍。逐条对照 egui：

| 目标能力 | egui 现状 | 判定 |
|---|---|---|
| 可停靠布局 | 无内置，需手写；ADR-010 已预判 | **不足** |
| 无障碍 / 屏幕阅读器 | 明显弱于浏览器原生语义 | **不足** |
| 复杂文本排版 / 富文本 | 受限 | **不足** |
| 键盘导航、焦点管理 | 可做但成本高 | **不足** |
| 深色密集仪表美学 | 可做（已实现） | 满足 |
| 自绘 Canvas/WebGL 图表 | egui Painter + 纹理 | 满足 |

触发条件命中，ADR-010 预留的重评估条款生效。

### 决策

桌面 UI 改为 **Tauri 2 + React 19 + TypeScript + Vite**。
Rust 音频内核**完全保留**，并作为唯一音频计算来源。

### 依赖边界（强约束）

React **不得**参与音频计算。所有 DSP 在 Rust 侧完成，
前端只消费结果。这条在旧架构下由 `check-boundaries.ps1` 强制，
切换后改为由 `packages/contracts` 的类型边界 + CI 中的架构检查保证。

### 保留与废弃

| 资产 | 结果 |
|---|---|
| `auralis-core`（1401 行含 dsp） | 完整保留，已被 numpy oracle 交叉验证 |
| `scripts/oracle.py` / `check-oracle.ps1` | 完整保留 |
| `scripts/check-boundaries.ps1` | 保留（规则调整） |
| CI / ADR / 文档 | 完整保留 |
| `auralis-ui`（1716 行 egui） | **废弃** |
| `apps/auralis-desktop`（eframe 入口） | 替换为 `apps/desktop`（Tauri） |

设计 Token 的**取值**（`bg.app` 等 10 个色值）迁移到 CSS Variables，
概念保留、实现重写。

### 后果

- **收益**：可停靠布局、无障碍、快捷键、文本排版达到目标水平。
- **代价**：新增 Node.js/npm 工具链；CI 需要 Node job；
  `auralis-ui` 1716 行作废；主题体系需重写一遍。
- **新增风险**：Rust ↔ WebView 的 IPC 成为新的架构约束，见 ADR-018。

### 备选方案

- **继续 egui 并手写完整 dock**：可行但要自建焦点管理、
  键盘导航、无障碍树，成本高于切换且结果不确定。
- **Electron**：渲染能力更强，但包体积与内存占用显著高于
  Tauri 的系统 WebView2。对本项目（60 FPS Canvas 图表）无必要。
- **Qt/PySide**：需引入 Python 运行时，与 ADR-014 的
  「产品代码单一语言」冲突。
## ADR-017：前端架构 —— shadcn 源码所有权模式，不引入完整组件库

状态：Accepted（2026-10-15）

### 背景

React 生态提供了大量组件库。但本项目要的是**密集测量仪器界面**，
不是通用后台。Aurence 类软件的视觉语言（大面积深色、极细分隔线、
紧凑参数、等宽数字）与 Ant Design / MUI 的默认审美**冲突**，
强行覆写主题的成本高于自建。

### 决策

1. **不引入** Ant Design、MUI 等完整组件库（`package.json` 中禁止）。
2. 采用 **shadcn/ui 的源码所有权模式**：把需要的组件源码复制进仓库，
   按 Auralis 设计规范改造。项目拥有全部源码与 API。
3. **Radix Primitives 按需引入**，解决 Dialog / Popover / DropdownMenu /
   Tooltip / ContextMenu 的焦点管理与键盘导航。
4. **CSS Variables 是设计令牌的唯一来源**。Tailwind CSS 仅作构建期工具，
   不引入运行时 CSS 框架。
5. 音频可视化组件（Waveform / Spectrum / Spectrogram / LevelMeter /
   TimeRuler）**完全自研**，独立于基础 UI 层，不依赖表格或图表库实现。

### 为什么保留 Radix 的运行时依赖

Radix 提供真实的 React 交互逻辑：焦点陷阱、键盘导航、ARIA 属性、
浮层定位。这些边界情况极易写错，自己重写是重复劳动。
真正要避免的是**庞大的运行时 CSS 框架**与**通用组件库黑盒**，
Radix 两者都不是——它是 headless 的，只管行为不管视觉。

### 组件分层与建设顺序

| 层 | 内容 | 时机 |
|---|---|---|
| L1 基础控件 | Button / IconButton / Input / Checkbox / Switch / Select / Slider | 第一批 |
| L2 交互容器 | Dialog / Popover / Tooltip / ContextMenu / Resizable | 第二批 |
| L3 业务组件 | Dock / DataTable / FileList / CommandPalette / StatusBar / TaskQueue | 第三批 |

**不提前建设通用设计系统。** 先只做当下用得到的。

### 目录策略

`ui` 与 `audio-viz` **先放在 `apps/desktop/src/` 内部**，
等组件 API 稳定后再拆成独立 workspace package。理由：KISS——
过早拆包会引入不必要的构建配置与版本协调成本。

### 后果

- 视觉完全可控，无第三方主题冲突。
- 需要自己维护组件源码，升级需手工合并。
- 前端无障碍质量取决于 Radix 使用是否正确，需在 review 中关注。
## ADR-018：Rust ↔ WebView 的 IPC 设计约束

状态：Accepted（2026-10-15，M1 开始前必须遵守）

### 背景

ADR-016 切换到 Tauri 后，Rust 与前端之间新增了一条此前不存在的
架构约束：数据必须跨 WebView 边界传递。

Tauri 的 `Command` 走 **JSON 序列化**。`Channel` 虽然用于流式，
但**同样是序列化的**。若把「60 FPS × 4096 点频谱」直接塞进 Command，
等于每秒序列化几十 MB JSON —— 这不是优化问题，是架构选型问题。

本 ADR 在**写实时模块之前**锁定策略，避免 M4 推倒重写。

### 通信分层决策

| 数据类型 | 方式 | 频率 | 理由 |
|---|---|---|---|
| 元数据、配置、指标标量 | Command (JSON) | 按需 | 体积小，无需优化 |
| 任务开始 / 结束 / 异常 | Event | 按需 | 天然的事件语义 |
| 离线分析进度 | Event（节流 10 Hz） | 低 | 进度是标量 |
| 波形包络（多级） | Command 返回 `Uint8Array` | 每次分析一次 | **二进制，不走 JSON** |
| 频谱快照（离线） | Channel + 二进制，节流 20–30 Hz | 中 | 降采样到像素宽度后再传 |
| **声谱图整块矩阵** | 一次传输整块 + 前端 WebGL | 每次分析一次 | **逐帧传必然重写** |
| 实时电平 / 频谱 | M4 决策，本 ADR 不预设 | — | 见下 |

### 三条硬规则

1. **前端不得参与音频计算。** 降采样、dB 换算、峰值检测全部在 Rust 侧
   完成后才传输。传输的是「已经算好的视图数据」，不是原始采样。
   这样传输量与文件长度**解耦**。

2. **二进制优先。** 波形与频谱一律以 `Float32Array` / `Uint8Array`
   传输，禁止序列化为 JSON 数组。

3. **频率与数据量反比。** 实时快照必须**先按显示像素宽度降采样**，
   再决定发送频率。像素宽度 800 的频谱就发 800 个点，不发 4096。

### 尚未决策：M4 实时高频通道

实时采集需要比 20–30 Hz 更高的刷新率时，候选路径：

- 本地 WebSocket 推快照（需管理重连与端口）
- SharedArrayBuffer + AudioWorklet（需 WebView2 COOP/COEP 头）
- 自定义原生插件

**该项必须由 M4 的实测数据决定**，不在 M1 预设。
但 M1–M3 的离线路径必须按上表设计，确保届时只需替换实时通道，
不需改动上层渲染。

### 实测修正（M1 开工后补）

本 ADR 初稿断言「JSON 序列化等于每秒几十 MB」。**该断言被实测推翻。**

`apps/desktop/src-tauri/src/ipc_bench.rs` 用同一份真实数据测得：

```text
输入     : fft_size=4096, target_bins=1200, envelope=2000
二进制   : 12.5 KB/帧
JSON 数组: 34.8 KB/帧
倍数     : 2.8x（不是数量级差异）

按 60 FPS 换算：
  二进制 -> 0.7 MB/s
  JSON   -> 2.0 MB/s
```

**修正后的结论**：

1. **带宽不是瓶颈。** 2.0 MB/s 对本地 IPC 完全可承受。
   初稿的「几十 MB/s」是把未降采样的 4096 点直接乘 60 FPS 估的，
   但规则 3 要求的降采样已经把量级砍掉了两个数量级。
2. **真正的成本是 CPU**：JSON 路径每帧要做 3200 次
   浮点转字符串 + 解析。这才是二进制传输的主要收益来源。
3. **规则 2（二进制优先）依然成立**，理由改为「省 CPU」而非「省带宽」。
4. 载荷与 FFT 长度已解耦：fft 1024 与 16384 在相同像素宽度下
   载荷完全相同（3.5 KB），验证规则 3 落地正确。

> 这条修正本身是 ADR-014「按证据引入」的第一次实际应用：
> 原判断被数据推翻，就改判断，而不是找理由维护原结论。

### 后果

- M1 的波形传输接口一开始就设计为二进制 + 降采样，避免 M2 返工。
- 需要一个 IPC 压测工具，在真实数据量下验证吞吐（见实施步骤）。
- 若日后实测证明 Tauri IPC 成为瓶颈，迁移到 WebSocket / SAB 时
  只影响传输层，渲染层与 DSP 层不变。