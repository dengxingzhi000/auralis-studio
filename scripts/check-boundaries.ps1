#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Enforce the architecture boundaries from AGENTS.md and docs/TAD.md §3.

.DESCRIPTION
    Fails (exit 1) if a "pure" crate gains a forbidden dependency or import.

    Pure crates and their rules:
      auralis-core : no egui/eframe/cpal/symphonia, no std::fs, no async runtime
      auralis-dsp  : same as core, plus no tokio/rayon (no parallelism inside the kernel)

    Rationale: docs/TAD.md §3 and AGENTS.md "Architecture rules" require DSP to be
    testable without a display. This check is the mechanical guard; without it the
    boundary erodes the first time someone adds a convenient import.

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
    # ADR-016: Tauri/Rust IPC 端点也不得进入纯算法层
    'tauri', 'tauri-plugin-log'
)

# Forbidden tokens in .rs source of pure crates.
$forbiddenTokens = @(
    'std::fs', 'std::net', 'std::process',
    'egui::', 'eframe::', 'cpal::', 'symphonia::', 'tauri::'
)

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
# React 不得参与音频计算。这里机械检查前端不出现 FFT / dB 对数等
# 数值运算调用——它们必须留在 Rust 侧。
$srcDir = Join-Path $WorkspaceRoot 'apps\desktop\src'
if (Test-Path $srcDir) {
    $tsFiles = Get-ChildItem $srcDir -Recurse -Include '*.ts', '*.tsx'
    $forbiddenTs = @(
        'Math.log10', 'Math.log2', 'new Float64Array', 'fft(', 'hann(', 'hamming(', 'blackman('
    )
    foreach ($f in $tsFiles) {
        $text = Get-Content $f.FullName -Raw
        foreach ($bad in $forbiddenTs) {
            if ($text -match [regex]::Escape($bad)) {
                $rel = $f.FullName.Substring($WorkspaceRoot.Length + 1)
                $failures.Add("$rel contains '$bad' - audio math belongs in Rust (ADR-018)")
            }
        }
    }
    if ($failures.Count -gt 0) {
        Write-Host "FRONTEND BOUNDARY VIOLATIONS:" -ForegroundColor Red
        $failures | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
        Write-Host ""
        Write-Host "ADR-018: React must not perform audio computation. dB conversion," -ForegroundColor Yellow
        Write-Host "FFT, resampling and peak detection all belong in the Rust core."
        exit 1
    }
}

Write-Host "boundary check OK: $($pureCrates -join ', ') are free of UI/device/fs/db deps" -ForegroundColor Green
Write-Host "boundary check OK: frontend src contains no audio math (ADR-018)" -ForegroundColor Green
exit 0