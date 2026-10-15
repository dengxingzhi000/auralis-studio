# Auralis Studio

Auralis Studio 是一款 Windows 优先的专业桌面音频分析软件。

**架构：Tauri 2 + React 19 (TypeScript) 前端，Rust 音频内核。**
React 只负责展示，**所有 DSP 在 Rust 侧完成**（ADR-016 / ADR-018）。

**当前状态：M0 完成 + IPC 通路已验证。** 可以编译、可以运行 CLI 自检、
前端能通过合成信号跑通「Rust 计算 → 二进制 IPC → Canvas 渲染」全链路。
**尚不支持任何真实音频文件**——「导入文件」按钮是灰的并标注 M1。

## 文档入口

| 文档 | 内容 |
|---|---|
| [产品需求文档](docs/PRD.md) | 用户、版本范围、功能与非功能需求、产品边界 |
| [技术架构设计](docs/TAD.md) | 分层、依赖方向、线程模型、错误分类 |
| [DSP 算法规范](docs/DSP_SPEC.md) | RMS / Peak / DC Offset / FFT / STFT / 削波的定义与误差目标 |
| [UI/UX 设计规范](docs/UI_SPEC.md) | 设计 Token、布局、视图组件、图表性能约束 |
| [测试与验收计划](docs/TEST_PLAN.md) | 测试分层、合成信号集、验收目标、CI 命令 |
| [开发实施计划](docs/IMPLEMENTATION_PLAN.md) | M0–M6 里程碑与垂直切片 |
| [架构决策记录](docs/ADR.md) | ADR-001 … ADR-018 |
| [开发环境设置](docs/DEV_ENVIRONMENT.md) | 本机工具链、镜像与路径约定 |

## 技术栈

| 层 | 选择 |
|---|---|
| 桌面外壳 | Tauri 2（系统 WebView2，不捆绑 Chromium） |
| UI | React 19 + TypeScript + Vite |
| 设计系统 | CSS Variables 令牌 + Radix Primitives（headless）+ 自研组件 |
| 音频可视化 | 自研 Canvas 2D（波形 / 频谱），WebGL 预留给声谱图 |
| 音频内核 | Rust：`auralis-core` / `auralis-dsp`，独立 workspace crate |
| 通信 | Tauri Commands（标量）+ 二进制批量（视图数据） |
| 验证 | Rust 单测 + numpy 独立参考实现 + CI |

**不引入** Ant Design / MUI 等完整组件库（ADR-017）。

## 里程碑进度

| 里程碑 | 内容 | 状态 |
|---|---|---|
| **M0** | workspace、工具链、深色主题、DSP 内核骨架、CI | 已完成 |
| **M0.5** | UI 栈切换至 Tauri/React、IPC 通路验证 | 已完成 |
| M1 | WAV 导入、元数据、分块 PCM、任务进度与取消 | 未开始 |
| M2 | FFT、幅值校正、峰值检测、多级波形包络 | 未开始 |
| M3 | 频谱交互：游标、缩放、参数检查器、峰值标记 | 未开始 |
| M4 | 实时采集（CPAL）+ STFT 声谱图 | 未开始 |
| M5 | 导出、配置持久化、异常恢复 | 未开始 |
| M6 | 安装包、用户手册、许可证清单 | 未开始 |

## 构建与运行

需要 Rust `1.99.0`（`rust-toolchain.toml` 固定）、MSVC 链接器、Node.js 20+。
本机安装方式见 [docs/DEV_ENVIRONMENT.md](docs/DEV_ENVIRONMENT.md)。

```powershell
# 前端
cd apps/desktop
npm install
npm run dev            # Vite dev server

# 完整应用（Tauri dev：自动拉起 WebView）
npm run tauri dev

# 生产构建
npm run tauri build
```

### CLI（无需图形环境，CI 用）

```powershell
.\target\release\auralis-studio.exe --version
.\target\release\auralis-studio.exe --self-test
.\target\release\auralis-studio.exe --dump-reference out.json
```

> Release 构建带 `windows_subsystem = "windows"`，**PowerShell 直接调用不会
> 可靠等待**，`$LASTEXITCODE` 可能为空。脚本中请用
> `Start-Process -Wait -PassThru`。`scripts/check-oracle.ps1` 已按此写法。

## 质量检查

```powershell
./scripts/check-boundaries.ps1   # 架构边界：core/dsp 纯净 + 前端无音频运算
./scripts/check-oracle.ps1       # numpy 独立参考实现交叉验证 DSP
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p auralis-desktop ipc -- --nocapture   # IPC 载荷实测
cd apps/desktop && npm run typecheck
```

## 工程原则

1. **React 不参与音频计算。** dB 换算、FFT、降采样全在 Rust 侧完成后才传输。
2. DSP 与 UI、设备 API、文件系统解耦，由 `check-boundaries.ps1` 强制。
3. 实时音频回调不阻塞、不进行无界分配、不等待 UI。
4. 所有核心算法均有确定性数值测试，并由 **numpy 独立实现交叉验证**。
5. 明确每个指标的单位、归一化方式、适用条件与算法版本。
6. **不宣称未经验证的格式、性能或测量准确度**——包括对本项目自己的架构判断。

## 已知限制

- **当前不支持任何音频文件格式**，解码器尚未接入。
- 未在真实图形会话中运行过 GUI；IPC 载荷体积已实测，**端到端延迟未测**。
- CI 配置已写但从未在真实 runner 上执行过。
- 声谱图（WebGL）、实时采集、导出均未实现。
- macOS/Linux 未适配（Tauri 理论支持，但音频路径与打包未验证）。