# Auralis Studio — 开发环境激活脚本
#
# 用法：在此 PowerShell 会话开始时执行
#     . .\scripts\activate-dev.ps1
#
# 作用：把 CARGO_HOME / RUSTUP_HOME / cargo 路径指向 D 盘，
# 保证 C 盘不被 Rust 生态撑爆。
#
# 说明（见 docs/ADR.md ADR-011）：
#   镜像设置（RUSTUP_DIST_SERVER / crates.io source replacement）**不在这里**，
#   它们属于机器网络配置，已经写进用户环境变量与 CARGO_HOME\config.toml。
#   新开的终端会自动继承，无需重复设置。

$ErrorActionPreference = 'Stop'

$DevRoot = 'D:\DevTools'
$env:CARGO_HOME  = "$DevRoot\cargo"
$env:RUSTUP_HOME = "$DevRoot\rustup"

if (-not (Test-Path "$DevRoot\cargo\bin\cargo.exe")) {
    Write-Warning "未找到 $DevRoot\cargo\bin\cargo.exe —— Rust 尚未安装。参见 docs/DEV_ENVIRONMENT.md"
}

$cargoBin = "$DevRoot\cargo\bin"
if (($env:Path -split ';') -notcontains $cargoBin) {
    $env:Path = "$cargoBin;$env:Path"
}

# 本机会话里 cargo 通过 curl 走镜像时不需要代理
Remove-Item Env:HTTP_PROXY,Env:HTTPS_PROXY,Env:ALL_PROXY -ErrorAction SilentlyContinue

Write-Host "开发环境已激活：" -ForegroundColor Green
Write-Host "  CARGO_HOME  = $env:CARGO_HOME"
Write-Host "  RUSTUP_HOME = $env:RUSTUP_HOME"
Write-Host "  cargo       = $((Get-Command cargo -ErrorAction SilentlyContinue).Source)"
Write-Host "  rustc       = $((Get-Command rustc -ErrorAction SilentlyContinue).Source)"