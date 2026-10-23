# 开发环境设置（Auralis Studio）

本文记录**本机**实际验证过的工具链安装方式。目标是：

1. C 盘不被 Rust 生态撑爆；
2. 在直连国际站点极慢、且本机代理（`127.0.0.1:7897`）对部分域名 TLS 握手失败的环境下可重复安装。

> 换机器时不必照抄，按同样的「先量测、再决策」思路重做即可。

## 1. 磁盘布局原则

Rust 项目在本机很容易吃掉数 GB（依赖源码、注册表缓存、编译产物）。
本项目把**所有**可增长的内容放在 D 盘，C 盘只保留 KB 级的应用配置。

| 内容 | 位置 | 设置方式 |
|---|---|---|
| Rust 工具链本体 | `D:\DevTools\rustup` | 用户环境变量 `RUSTUP_HOME` |
| Cargo 注册表 / git 缓存 | `D:\DevTools\cargo` | 用户环境变量 `CARGO_HOME` |
| 编译产物 `target/` | `D:\ProgramProject\sonic-scope\target` | 仓库在 D 盘，自然落在这里 |
| 应用配置 `%APPDATA%\AuralisStudio\` | C 盘（数 KB） | 可接受 |
| MSVC 链接器（Build Tools） | `D:\DevTools\vsbuildtools` | 安装器 `--installPath` |
| 下载器与日志 | `D:\DevTools\installers` | — |

安装前应先确认空间：

```powershell
Get-PSDrive -PSProvider FileSystem | Select-Object Name,
    @{n='UsedGB';e={[math]::Round($_.Used/1GB,1)}},
    @{n='FreeGB';e={[math]::Round($_.Free/1GB,1)}}
```

## 2. 网络现实与镜像

实测（2026-10-14）：

| 路径 | 速度 | 可用性 |
|---|---|---|
| 直连 `static.rust-lang.org` | **~28 KB/s** | 可用但极慢（一度 15 分钟下载不完工具链） |
| 走本机代理 `127.0.0.1:7897` | — | **失败**：TLS 握手被重置 |
| `mirrors.ustc.edu.cn/rust-static` | **~860 KB/s** | 可用 ✅ |
| `mirrors.ustc.edu.cn/crates.io-index`（稀疏） | **~370 KB/s** | 可用 ✅ |
| 直连 `crates.io` | ~80 KB/s | 可用但慢 |
| `download.visualstudio.microsoft.com` | ~776 KB/s | 可用 ✅ |
| `github.com` | — | **失败**：`CRYPT_E_REVOCATION_OFFLINE` |

因此本机固定使用 USTC 镜像。

## 3. 安装 Rust

```powershell
# 3.1 目录与持久化变量（用户级，写入注册表，对新进程生效）
New-Item -ItemType Directory -Force D:\DevTools\installers | Out-Null
[Environment]::SetEnvironmentVariable('CARGO_HOME',  'D:\DevTools\cargo',  'User')
[Environment]::SetEnvironmentVariable('RUSTUP_HOME', 'D:\DevTools\rustup', 'User')
[Environment]::SetEnvironmentVariable('RUSTUP_DIST_SERVER', 'https://mirrors.ustc.edu.cn/rust-static',        'User')
[Environment]::SetEnvironmentVariable('RUSTUP_UPDATE_ROOT', 'https://mirrors.ustc.edu.cn/rust-static/rustup',  'User')

# 3.2 本次会话立即生效
$env:CARGO_HOME='D:\DevTools\cargo'; $env:RUSTUP_HOME='D:\DevTools\rustup'
$env:RUSTUP_DIST_SERVER='https://mirrors.ustc.edu.cn/rust-static'
$env:RUSTUP_UPDATE_ROOT='https://mirrors.ustc.edu.cn/rust-static/rustup'

# 3.3 下载并安装（注意：不能用 Invoke-WebRequest，本网络下会 "unexpected EOF"）
curl.exe -sS -L --retry 5 -o D:\DevTools\installers\rustup-init.exe `
  https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe

& D:\DevTools\installers\rustup-init.exe -y `
  --default-toolchain stable --profile minimal -c rustfmt,clippy --no-modify-path

[Environment]::SetEnvironmentVariable('Path', "D:\DevTools\cargo\bin;$([Environment]::GetEnvironmentVariable('Path','User'))", 'User')
```

### 两个踩过的坑

1. **`RUSTUP_DIST_SERVER` 不能带结尾的 `/dist`。**
   rustup 自己会拼 `/dist/channel-rust-*.toml`。带了 `/dist` 会请求
   `/dist/dist/channel-rust-stable.toml`，得到 `no release found for 'stable'`。

2. **`Invoke-WebRequest` 在本网络下载 rust-lang / Microsoft 域名会失败**
   （`Received an unexpected EOF or 0 bytes from the transport stream`）。
   改用 `curl.exe` 即可。

安装结果：`rustc 1.99.0`、`cargo 1.99.0`、`rustfmt 1.10.0`、`clippy 0.1.99`。

## 4. Cargo 走 crates.io 镜像

写在 **`CARGO_HOME\config.toml`**（本机为 `D:\DevTools\cargo\config.toml`），
**不进版本库** —— 镜像属于机器/地区配置，不应强加给 CI 和其他开发者。

```toml
[source.crates-io]
replace-with = "ustc"

[source.ustc]
registry = "sparse+https://mirrors.ustc.edu.cn/crates.io-index/"

[net]
retry = 3

[http]
multiplexing = true
timeout = 60
```

仓库内的 `.cargo/config.toml` 只放与项目相关、可复现的设置（alias、颜色等）。

> **完整性**：镜像只替换下载来源。`Cargo.lock` 里的校验和仍由 Cargo 强制校验，
> 因此供应链安全性不因使用镜像而降低。

## 5. 安装 MSVC 链接器（必需）

Rust 的 MSVC 工具链需要 `link.exe`。**没有它，`cargo check` 也会失败**，
因为 `serde_derive`、`thiserror-impl` 这类 proc-macro crate 必须链接才能生成
`.so`/`.dll`。本机初始状态：没有 VS、没有 link.exe、没有 Windows SDK、没有 MinGW。

```powershell
curl.exe -sS -L --retry 5 -o D:\DevTools\installers\logs\vs_BuildTools.exe `
  https://aka.ms/vs/17/release/vs_BuildTools.exe

# 只装链接器必需的最小组件，不要整个 VCTools workload（会多拉几个 GB 的
# CMake / Linux / WSL 工具链）
& D:\DevTools\installers\logs\vs_BuildTools.exe `
  --quiet --wait --norestart --nocache `
  --installPath D:\DevTools\vsbuildtools `
  --add Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
  --add Microsoft.VisualStudio.Component.Windows11SDK.22621
```

装完后 `link.exe` 位于
`D:\DevTools\vsbuildtools\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64\`。
Cargo 会自动发现它，无需手动加 PATH。

### 关于 C 盘的实际情况（与「全部放 D 盘」的期望不符）

**Windows SDK 不接受 `--installPath`。** 即使指定了
`--installPath D:\DevTools\vsbuildtools`，安装器仍把 SDK 装到它写死的默认位置：

```text
C:\Program Files (x86)\Windows Kits\10\Lib\10.0.22621.0\
C:\Program Files (x86)\Microsoft Visual Studio\Installer\
```

实际占用：

| 位置 | 大小 |
|---|---|
| `D:\DevTools\vsbuildtools`（MSVC 工具链 + link.exe） | 1,671 MB |
| `C:\Program Files (x86)\Windows Kits\10`（Windows SDK） | 1,905 MB |
| `C:\Program Files (x86)\Microsoft Visual Studio`（安装器/共享组件） | 数十 MB |

这是 Visual Studio 安装器的行为，命令行无法改变。若必须把 SDK 移到 D 盘，
只能在安装后用目录联接（junction）：

```powershell
# 仅在确实需要时执行；先确认目标目录不存在
Move-Item 'C:\Program Files (x86)\Windows Kits' 'D:\DevTools\WindowsKits'
cmd /c mklink /J "C:\Program Files (x86)\Windows Kits" "D:\DevTools\WindowsKits"
```

**M0 未执行该操作**：它属于对系统目录的侵入性改动，且当前 C 盘剩余空间充足。
若后续 C 盘紧张再启用。

### 磁盘占用实测（2026-10-14 安装完成后）

| 目录 | 大小 |
|---|---|
| `D:\DevTools\vsbuildtools`（MSVC + link.exe） | 1,671 MB |
| `D:\DevTools\rustup`（stable + 1.99.0 两套工具链） | ~1.6 GB |
| `C:\Program Files (x86)\Windows Kits\10`（Windows SDK） | 1,905 MB |

> `stable` 与 `1.99.0` 两套工具链内容相同，可删其一省约 800 MB：
> `rustup toolchain uninstall stable`。M0 保留两者用于交叉验证。

## 6. 日常命令

```powershell
# 进入项目后（PowerShell），最省事的方式：
. .\scripts\activate-dev.ps1

# 完整检查（等同 CI）
./scripts/check-boundaries.ps1     # 架构边界
./scripts/check-oracle.ps1         # numpy 参考实现交叉验证
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release

# 产物自检
.\target\release\auralis-studio.exe --self-test
```

新开一个终端时，用户级环境变量会自动生效；
`activate-dev.ps1` 主要是给**当前**会话补设 `CARGO_HOME` / `PATH`。

## 7. Python + numpy（DSP 参考实现用）

`scripts/check-oracle.ps1` 需要 Python 3.9+ 与 numpy（ADR-015）。
Python **不进入产品二进制**，只是验证层工具。

### 本机的坑：PATH 上的 `python.exe` 是失效存根

Windows 上 `python.exe` 默认指向 Microsoft Store 的执行别名，
直接运行会报 `Python was not found`，但真实解释器其实存在：

```text
C:\Users\Deng\AppData\Local\Microsoft\WindowsApps\python.exe   <- 失效存根
C:\Users\Deng\AppData\Local\Programs\Python\Python313\python.exe  <- 真实解释器
```

验证：

```powershell
python --version                                  # 失败：Python was not found
& "$env:LOCALAPPDATA\Programs\Python\Python313\python.exe" --version
```

已装版本：**Python 3.13.10 / numpy 2.5.0 / scipy 1.18.0**（scipy 当前未使用，
保留备用）。

`check-oracle.ps1` 的探测顺序是 `py` → `python3` → `python`，
并且会用 `-c "import sys; sys.version_info>=..."` 实测，
自动跳过失效存根。因此本机直接运行该脚本即可，无需手动指定路径。

### 手动指定 Python

```powershell
./scripts/check-oracle.ps1 -Python "C:\Users\Deng\AppData\Local\Programs\Python\Python313\python.exe"
```

### 缺 numpy 时

```powershell
& "$env:LOCALAPPDATA\Programs\Python\Python313\python.exe" -m pip install numpy
```

> 注意：本机直连 PyPI 很慢（与 crates.io 同因），必要时加镜像：
> `-i https://pypi.tuna.tsinghua.edu.cn/simple`

## 8. 在 PowerShell 里运行 GUI 子系统产物的两个坑

Auralis Studio 的 release 构建带 `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`
（PE Subsystem 2）。这对用户体验是必需的——否则双击会弹出一个黑窗口。
但它给自动化带来两个坑，**两者都在本项目实际踩过**：

### 坑 1：直接 `& $exe` 不会可靠等待，`$LASTEXITCODE` 为空

```powershell
& $exe --version
Write-Output "exit=$LASTEXITCODE"   # 输出 "exit=" —— 空！
```

GUI 子系统程序不继承控制台，PowerShell 不保证同步等待它结束。

**正确写法**：

```powershell
$p = Start-Process -FilePath $exe -ArgumentList '--self-test' -Wait -PassThru -NoNewWindow
if ($p.ExitCode -ne 0) { exit $p.ExitCode }
```

`scripts/check-oracle.ps1` 已按此写法。

### 坑 2：`Start-Process` 配合输出重定向会让 `ExitCode` 变成空

```powershell
$p = Start-Process -FilePath $exe -ArgumentList '--self-test' -Wait -PassThru -NoNewWindow *> $null
Write-Output $p.ExitCode    # 空！尽管程序本身 exit 0
```

加了 `*> $null` 之后退出码就丢了。**不要重定向**，让输出直接显示即可——
自检输出本来就应该被人看见。

### 退出码约定

| 命令 | 退出码 |
|---|---|
| `--version` | 0 |
| `--self-test` | 0 通过 / 1 数值回归 |
| `--dump-reference <path>` | 0 |
| `--dump-reference`（缺路径） | 2 |

## 9. 工具链升级

`rust-toolchain.toml` 固定 `1.99.0`。升级时：

1. 修改 `rust-toolchain.toml` 的 `channel`；
2. 运行一次 `cargo build`，rustup 会自动装新工具链；
3. 重跑全部测试 —— LLVM 版本变化可能引起浮点结果末位差异；
4. 更新 `docs/DEV_ENVIRONMENT.md` 的版本记录与 `docs/ADR.md`（若涉及）。

## 10. npm 依赖的 install 脚本（allowScripts）

### 10.1 npm 11 的真实行为：只警告，不拦截

npm 11.19.1 对「未审阅的 install 脚本」默认**只输出 `npm warn`，脚本照常执行**。
实测（2026-10-23，一个本地 `file:` 依赖带 `install` 脚本）：

| 命令 | 结果 |
|---|---|
| `npm install`（无 allowScripts） | 警告一行，**脚本执行**（标记文件被写出） |
| `npm ci`（无 allowScripts） | 同上，**脚本执行** |
| `npm ci --strict-allow-scripts` | `ESTRICTALLOWSCRIPTS`，exit 1，**脚本未执行** |

因此「默认拦截」是不成立的：仅靠警告，一个新传入的 `postinstall`
可以在无人察觉时执行。本项目用下面的组合把它变成硬约束：

- `apps/desktop/.npmrc`：`strict-allow-scripts=true`
- `apps/desktop/package.json` 的 `allowScripts`：审阅通过的依赖清单

### 10.2 新增依赖时怎么办

```powershell
cd apps/desktop
npm install-scripts ls            # 列出尚未审阅、且确实带 install 脚本的依赖
# 打开 node_modules/<pkg>/package.json 看 scripts 到底做什么
npm install-scripts approve <pkg> # 写入 allowScripts（默认按版本钉住，如 esbuild@0.28.2）
```

清单里的条目是**精确版本**：依赖升级后旧钉子不再匹配，`npm ci` 会失败并提示
重新审阅。这是刻意的——升级等于换了要执行的代码，必须再看一眼。

当前清单与理由：

| 条目 | 脚本 | 为什么允许 |
|---|---|---|
| `esbuild@0.28.2` | `postinstall: node install.js` | 校验并按平台准备二进制；构建链必需 |
| `electron-winstaller@5.4.0` | `install: node ./script/select-7z-arch.js` | 从 `vendor/7z-<arch>.*` 复制出 `7z.exe`；electron-builder 的相关目标依赖它 |
| `fsevents@2.3.3` | `install: node-gyp rebuild` | 仅 macOS 安装（`os: darwin`、optional）；Windows/Linux 上 npm 标记为 inert，不进审阅列表 |

`fsevents` 是为 macOS 开发者预留的：该平台上它会真的编译，
不加这一条会在 macOS 上把严格模式撞成失败。

### 10.3 Electron 二进制不会再自动下载（electron 45 的行为变化）

electron@45.0.0 的 package.json **没有 `postinstall`**，只提供 `install-electron`
这个 bin。实测：`npm ci` 会清掉 `node_modules/electron/dist` 且不重新生成，结果是

- `npm run electron:dev` 报 "Electron failed to install correctly"；
- electron-builder 的 `electronDist: node_modules/electron/dist` 不存在，
  `npm run package:win` 直接失败。

因此 `apps/desktop/package.json` 加了

```json
"postinstall": "node scripts/ensure-electron.mjs"
```

`scripts/ensure-electron.mjs` 的行为：

- `ELECTRON_SKIP_BINARY_DOWNLOAD=1` → 直接跳过（CI 只做类型检查/构建，
  不启动 Electron；避免每个 job 下载约 100 MB）；
- 否则调用 electron 自带的 `install.js`（内含 `isInstalled()` 快路径，
  已装好时几乎零开销）；
- 未设置 `ELECTRON_MIRROR` 时默认用 npmmirror，可用环境变量覆盖
  （本网络下官方地址实测 `fetch failed`）。

想强制重装二进制：删掉 `node_modules/electron/dist` 再跑 `npm run postinstall`。
