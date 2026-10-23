#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Enforce the architecture boundaries from AGENTS.md and docs/TAD.md §3.

.DESCRIPTION
    Fails (exit 1) if a "pure" crate gains a forbidden dependency or import,
    or if the shipped frontend code performs audio math.

    Pure crates and their rules:
      auralis-core : no egui/eframe/cpal/symphonia, no std::fs, no async runtime
      auralis-dsp  : same as core, plus no tokio/rayon (no parallelism inside the kernel)

    Rationale: docs/TAD.md §3 and AGENTS.md "Architecture rules" require DSP to be
    testable without a display. This check is the mechanical guard; without it the
    boundary erodes the first time someone adds a convenient import.

    The frontend half is a **whitelist**, not a blacklist. An earlier version
    blacklisted a few spellings (`Math.log10`, `Math.log2`, `fft(` …) and could
    be walked around by writing `Math.log` instead — which is exactly how a
    dB conversion would slip into React. The whitelist says what the frontend
    may do (pure geometry and rounding) and rejects everything else.

    Scope: the shipped renderer (`apps/desktop/src`) and the Electron main
    process (`apps/desktop/electron`). `apps/desktop/dev` is deliberately NOT
    scanned: it holds the render self-check page, which by design must paint
    three canvases *without* the Rust backend (it exists to compare containers
    when the IPC path is not wired up yet). It never ships.

    The guard has a self-test: a boundary check that silently stops matching is
    worse than no check at all.

.NOTES
    Scans Cargo.toml dependency lists AND .rs source, because a crate can reach an
    OS API through std::fs without declaring a dependency.
#>
[CmdletBinding()]
param(
    [string]$WorkspaceRoot = (Split-Path -Parent (Split-Path -Parent $PSCommandPath))
)

$ErrorActionPreference = 'Stop'

$pureCrates = @('auralis-core', 'auralis-dsp')

# Forbidden crates in dependency lists of pure crates.
$forbiddenCrates = @(
    'eframe', 'egui', 'egui_plot', 'egui_extras', 'winit', 'wgpu',
    'cpal', 'symphonia', 'rtrb',
    'rusqlite', 'diesel', 'sqlx', 'tokio', 'async-std', 'rayon', 'rfd',
    # ADR-019: 容器/IPC 端点也不得进入纯算法层。Tauri 已移除，
    # 仍保留在名单里防止旧依赖被无意加回来。
    'tauri', 'tauri-plugin-log'
)

# Forbidden tokens in .rs source of pure crates.
$forbiddenTokens = @(
    'std::fs', 'std::net', 'std::process',
    'egui::', 'eframe::', 'cpal::', 'symphonia::', 'tauri::'
)

# ---------------------------------------------------------------------------
# 前端规则
# ---------------------------------------------------------------------------

# 前端允许使用的 Math 方法（白名单）。
#
# 只有「纯几何 / 取整」类方法可以出现在前端：max/min 用于钳位，
# round/floor/ceil 用于像素对齐，abs 用于距离，sign 用于方向。
# 任何与「对数 / 开方 / 幂 / 三角」有关的方法都不允许——它们是
# dB 换算与 DSP 的入口。`Math.log` 尤其要禁：dB = 20·log₁₀(x) 就差这一步。
$allowedMathMethods = @('abs', 'ceil', 'floor', 'max', 'min', 'round', 'sign', 'PI')

# 即使不涉及 Math，这些写法同样说明前端在做数值工作。
$forbiddenTs = @(
    'new Float64Array', 'fft(', 'hann(', 'hamming(', 'blackman('
)

# 要扫描的前端目录（出货代码）。
$frontendDirs = @('apps\desktop\src', 'apps\desktop\electron')

$failures = New-Object System.Collections.Generic.List[string]

function Remove-RustComments([string]$text) {
    # Strip line comments (//, ///, //!) and block comments (/* */) so that prose
    # such as "must not use std::fs" in a doc comment is not mistaken for a real
    # import. String literals are handled well enough for a boundary scan: we only
    # need to avoid cutting a token in half at a "//" inside a string, which does
    # not occur in this codebase's source strings.
    $sb = New-Object System.Text.StringBuilder
    $inBlock = $false
    foreach ($line in ($text -split "`r?`n")) {
        $trimmed = $line.TrimStart()
        if ($inBlock) {
            $idx = $trimmed.IndexOf('*/')
            if ($idx -ge 0) { $inBlock = $false; $trimmed = $trimmed.Substring($idx + 2) }
            else { continue }
        }
        if ($trimmed.StartsWith('//')) { continue }
        # strip trailing line comment (approximate: no '//' inside our source strings)
        $cut = $line.IndexOf('//')
        if ($cut -ge 0) {
            $head = $line.Substring(0, $cut)
            # keep block-comment openers that start the line
            if ($head.TrimEnd().EndsWith('/*')) { $inBlock = $true; [void]$sb.AppendLine($head); continue }
            $line = $head
        }
        [void]$sb.AppendLine($line)
    }
    return $sb.ToString()
}

function Remove-TypeScriptComments([string]$text) {
    # TypeScript 版注释剥离器。与 Rust 版不同的两点：
    #   1) 块注释 /* … */ 先整体去掉（含 JSDoc）。
    #   2) 行注释 // 只有在「它前面引号数是偶数」时才算注释的开始，
    #      这样字符串里的 "//"（"http://…"、"file://…"）不会被误切。
    if ([string]::IsNullOrEmpty($text)) { return '' }

    $sb = New-Object System.Text.StringBuilder
    $inBlock = $false
    foreach ($line in ($text -split "`r?`n")) {
        $trimmed = $line.TrimStart()
        if ($inBlock) {
            $idx = $trimmed.IndexOf('*/')
            if ($idx -ge 0) {
                $inBlock = $false
                $trimmed = $trimmed.Substring($idx + 2)
            }
            else { continue }
        }
        $open = $trimmed.IndexOf('/*')
        if ($open -ge 0) {
            $close = $trimmed.IndexOf('*/', $open + 2)
            if ($close -ge 0) {
                $trimmed = $trimmed.Substring(0, $open) + $trimmed.Substring($close + 2)
            }
            else {
                $inBlock = $true
                $trimmed = $trimmed.Substring(0, $open)
            }
        }
        [void]$sb.AppendLine($trimmed)
    }

    $lines = $sb.ToString() -split "`r?`n"
    for ($i = 0; $i -lt $lines.Count; $i++) {
        $line = $lines[$i]
        $cut = -1
        $pos = 0
        while ($true) {
            $found = $line.IndexOf('//', $pos)
            if ($found -lt 0) { break }
            $before = $line.Substring(0, $found)
            $quotes = ([regex]::Matches($before, '(?<!\\)(["''`])')).Count
            if (($quotes % 2) -eq 0) { $cut = $found; break }
            $pos = $found + 2
        }
        if ($cut -ge 0) { $lines[$i] = $line.Substring(0, $cut) }
    }
    return ($lines -join "`n")
}

function Test-FrontendAudioMath([string]$text, [string]$label) {
    # 返回该文件里的违规描述；空数组表示通过。
    $hits = @()
    foreach ($m in [regex]::Matches($text, '\bMath\.([A-Za-z0-9_$]+)')) {
        $name = $m.Groups[1].Value
        if ($allowedMathMethods -notcontains $name) {
            $hits += "$label 使用了 Math.$name —— 前端不做音频数学（dB 换算 / 开方 / 幂），这些必须留在 Rust 侧（ADR-018）"
        }
    }
    foreach ($bad in $forbiddenTs) {
        if ($text.Contains($bad)) {
            $hits += "$label 包含 '$bad' —— 音频数学属于 Rust 侧（ADR-018）"
        }
    }
    return $hits
}

# ---------------------------------------------------------------------------
# 守卫自测：确认规则本身还能抓到东西
# ---------------------------------------------------------------------------
# 一个「永远通过」的守卫比没有守卫更危险——规则改坏了没人会发现。
# 这里用几段合成代码正反各测一次。
$selfTestFailures = @()

foreach ($bad in @(
    'const db = 20 * Math.log(x / y);',
    'const v = Math.pow(a, b);',
    'const r = Math.sqrt(s);',
    'const e = Math.sin(t);',
    'const w = new Float64Array(8);',
    'const s = fft(samples);'
)) {
    if (@(Test-FrontendAudioMath $bad 'self-test').Count -eq 0) {
        $selfTestFailures += "应该被拦下但没有：$bad"
    }
}

foreach ($good in @(
    'const x = Math.max(1, 2);',
    'const y = Math.round(v);',
    'const z = Math.abs(w);',
    'const p = Math.PI;',
    'const label = "http://127.0.0.1:5173";'
)) {
    $hits = @(Test-FrontendAudioMath $good 'self-test')
    if ($hits.Count -gt 0) {
        $selfTestFailures += "应该被放行但被拦下：$good  ($($hits -join '; '))"
    }
}

if ($selfTestFailures.Count -gt 0) {
    Write-Host "BOUNDARY GUARD SELF-TEST FAILED:" -ForegroundColor Red
    $selfTestFailures | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    Write-Host ""
    Write-Host "守卫本身坏了，规则不再生效——先修守卫，再看边界。" -ForegroundColor Yellow
    exit 1
}

# ---------------------------------------------------------------------------
# Rust 纯算法层
# ---------------------------------------------------------------------------
foreach ($crate in $pureCrates) {
    $dir = Join-Path $WorkspaceRoot "crates\$crate"
    if (-not (Test-Path $dir)) {
        $failures.Add("crate not found: $dir")
        continue
    }

    $tomlPath = Join-Path $dir 'Cargo.toml'
    if (Test-Path $tomlPath) {
        $toml = Get-Content $tomlPath -Raw
        foreach ($bad in $forbiddenCrates) {
            # match `bad = ...` or `bad.workspace = true`
            if ($toml -match "(?m)^\s*$([regex]::Escape($bad))\s*=") {
                $failures.Add("$crate/Cargo.toml declares forbidden dependency '$bad'")
            }
        }
    }

    $rsFiles = Get-ChildItem $dir -Recurse -Filter '*.rs'
    foreach ($file in $rsFiles) {
        $code = Remove-RustComments (Get-Content $file.FullName -Raw)
        foreach ($bad in $forbiddenTokens) {
            if ($code -match [regex]::Escape($bad)) {
                $rel = $file.FullName.Substring($WorkspaceRoot.Length + 1)
                $failures.Add("$rel uses forbidden token '$bad' in code")
            }
        }
    }
}

if ($failures.Count -gt 0) {
    Write-Host "BOUNDARY VIOLATIONS ($($failures.Count)):" -ForegroundColor Red
    $failures | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    Write-Host ""
    Write-Host "docs/TAD.md 3 and AGENTS.md require these crates to stay free of UI," -ForegroundColor Yellow
    Write-Host "device APIs, filesystem access and database dependencies."
    exit 1
}

# ---------------------------------------------------------------------------
# 前端音频边界（ADR-018 规则 1）
# ---------------------------------------------------------------------------
# React 不得参与音频计算。dB 换算、开方、幂运算、FFT 必须留在 Rust 侧，
# 前端只画已经算好的数据。
foreach ($dir in $frontendDirs) {
    $dirPath = Join-Path $WorkspaceRoot $dir
    if (-not (Test-Path $dirPath)) { continue }
    $tsFiles = Get-ChildItem $dirPath -Recurse -Include '*.ts', '*.tsx'
    foreach ($f in $tsFiles) {
        $rel = $f.FullName.Substring($WorkspaceRoot.Length + 1)
        $text = Remove-TypeScriptComments (Get-Content $f.FullName -Raw)
        foreach ($hit in (Test-FrontendAudioMath $text $rel)) {
            $failures.Add($hit)
        }
    }
}

if ($failures.Count -gt 0) {
    Write-Host "FRONTEND BOUNDARY VIOLATIONS ($($failures.Count)):" -ForegroundColor Red
    $failures | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    Write-Host ""
    Write-Host "ADR-018: React must not perform audio computation. dB conversion," -ForegroundColor Yellow
    Write-Host "FFT, resampling and peak detection all belong in the Rust core."
    Write-Host "允许的 Math 方法：$($allowedMathMethods -join ', ')"
    exit 1
}

Write-Host "boundary check OK: $($pureCrates -join ', ') are free of UI/device/fs/db deps" -ForegroundColor Green
Write-Host "boundary check OK: shipped frontend ($($frontendDirs -join ', ')) contains no audio math (ADR-018)" -ForegroundColor Green
Write-Host "boundary check OK: guard self-test passed" -ForegroundColor Green
exit 0
