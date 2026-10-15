#!/usr/bin/env pwsh
<#
.SYNOPSIS
    用独立实现（numpy/scipy）交叉验证 auralis-dsp 的数值结果。

.DESCRIPTION
    流程：
      1. 让产品二进制导出它在一组标准信号上的计算结果（JSON）；
      2. 让 scripts/oracle.py 用 numpy 独立重算同一批信号；
      3. 逐项对照，不一致则退出码非零。

    为什么值得单独做成脚本：auralis-dsp 自己的单元测试只能证明
    「实现与作者的预期一致」，无法排除「实现和预期同时错了」。
    两个独立实现得出同一个数，才是可信的证据（ADR-015）。

.PARAMETER Exe
    auralis-studio 可执行文件路径。默认取 target\release。

.PARAMETER Python
    Python 解释器。默认按顺序探测 py / python3 / python。
    注意：Windows 的 Microsoft Store 版 python.exe 是失效的存根，
    探测时会跳过它。

.NOTES
    需要 numpy。不需要 scipy——当前参考实现只用 numpy。
#>
[CmdletBinding()]
param(
    [string]$Exe = '',
    [string]$Python = ''
)

$ErrorActionPreference = 'Stop'
$WorkspaceRoot = Split-Path -Parent (Split-Path -Parent $PSCommandPath)

function Fail($msg) { Write-Host "ORACLE FAILED: $msg" -ForegroundColor Red; exit 1 }

# ---- 定位可执行文件 ----
if (-not $Exe) {
    foreach ($cand in @(
        "$WorkspaceRoot\target\release\auralis-studio.exe",
        "$WorkspaceRoot\target\debug\auralis-studio.exe")) {
        if (Test-Path $cand) { $Exe = $cand; break }
    }
}
if (-not $Exe -or -not (Test-Path $Exe)) {
    Fail "找不到 auralis-studio 可执行文件（先跑 cargo build）"
}

# ---- 定位 Python ----
if (-not $Python) {
    foreach ($cand in @('py', 'python3', 'python')) {
        $cmd = Get-Command $cand -ErrorAction SilentlyContinue
        if (-not $cmd) { continue }
        # 跳过 Microsoft Store 的失效存根
        try {
            $null = & $cmd.Source -c "import sys; sys.exit(0 if sys.version_info>=(3,9) else 1)" 2>$null
            if ($LASTEXITCODE -eq 0) { $Python = $cmd.Source; break }
        } catch { continue }
    }
}
if (-not $Python) { Fail "找不到可用的 Python 3.9+" }

Write-Host "python : $Python"
Write-Host "exe    : $Exe"

# ---- 确认 numpy ----
& $Python -c "import numpy" 2>$null
if ($LASTEXITCODE -ne 0) { Fail "Python 缺少 numpy，请先执行: $Python -m pip install numpy" }

# ---- 导出参考值 ----
# 注意：release 构建带 `windows_subsystem = "windows"`，是 GUI 子系统程序。
# 直接 `& $exe ...` 时 PowerShell **不保证**等待它结束，$LASTEXITCODE 可能为空。
# 必须用 Start-Process -Wait -PassThru 才能可靠拿到退出码。
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) "auralis-oracle-$([guid]::NewGuid().ToString('N')).json"
try {
    $proc = Start-Process -FilePath $Exe -ArgumentList @('--dump-reference', $tmp) `
        -Wait -PassThru -NoNewWindow
    if ($proc.ExitCode -ne 0) { Fail "--dump-reference 退出码 $($proc.ExitCode)" }
    if (-not (Test-Path $tmp)) { Fail "--dump-reference 未产出文件" }

    # ---- 对照 ----
    & $Python "$WorkspaceRoot\scripts\oracle.py" --rust-json $tmp
    if ($LASTEXITCODE -ne 0) { Fail "numpy 参考实现与 Rust 输出不一致（exit $LASTEXITCODE）" }
}
finally {
    Remove-Item $tmp -Force -ErrorAction SilentlyContinue
}

Write-Host "ORACLE OK: numpy 参考实现与 Rust 输出逐项一致" -ForegroundColor Green
exit 0