#!/usr/bin/env python3
"""Auralis Studio — DSP 独立参考实现（oracle）。

为什么存在这个文件
------------------
`docs/TEST_PLAN.md` §8 要求关键算法具有「参考实现或黄金数据集」。
`auralis-dsp` 自己的单元测试只能证明「实现与作者的预期一致」，
无法排除「实现和预期同时错了」。

本文件用 **numpy/scipy 独立重算**同一批信号，再与 Rust 产品代码导出的
数值逐项对照。两个**独立实现**得出同一个数，比任何单侧断言都有说服力。

对应决策：`docs/ADR.md` ADR-015。
它是 `ADR-014`（技术栈不设限、按证据引入）在工具链上的第一个落地点：
产品代码保持纯 Rust，验证层用 Python。

用法
----
    python scripts/oracle.py --rust-json <path/to/reference.json>

`<path>` 由 `auralis-studio --dump-reference <path>` 生成。
不一致时以非零码退出，便于接入 CI。

容差
----
幅值类用绝对容差；dB 类在低电平处误差会被放大，因此 dB 一律用
绝对 dB 容差（默认 0.01 dB）而不是相对容差。
静音的 dBFS 期望为 JSON `null`（对应 Rust 的真 `-inf`）——
这是 `DSP_SPEC.md` §3「零值使用显示下限或明确的 -inf 数值语义」的落地，
oracle 会显式校验它确实是非有限值，而不是某个被替换掉的大数字。
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path

import numpy as np

# --------------------------------------------------------------------------
# 常量：必须与 auralis-dsp 的定义一致
# --------------------------------------------------------------------------

SAMPLE_RATE = 48_000
TAU = 2.0 * math.pi

# dB 类指标的容差（dB）
TOL_DB = 0.01
# 线性幅值/偏移的绝对容差（FS）
TOL_LIN = 1e-5
# DC 偏移在长序列上的数值噪声容忍
TOL_DC = 1e-6


# --------------------------------------------------------------------------
# 参考实现（numpy）
# --------------------------------------------------------------------------


def dbfs(x: float) -> float | None:
    """线性幅度 -> dBFS。零（或非有限）返回 None，对应 Rust 的真 -inf。"""
    if x == 0.0 or not math.isfinite(x):
        return None
    return 20.0 * math.log10(abs(x))


def rms(x: np.ndarray) -> float:
    """RMS = sqrt(mean(x^2))。

    Rust 用 Welford 在线算法以避免精度损失；这里用 numpy 的 float64 累加。
    两者在 48k 样本上应当一致到远优于 TOL_LIN。
    """
    return float(np.sqrt(np.mean(np.square(x.astype(np.float64)))))


def dc_offset(x: np.ndarray) -> float:
    """DC 偏移 = 均值，保留符号。"""
    return float(np.mean(x.astype(np.float64)))


def peak(x: np.ndarray) -> float:
    """数字峰值 = max|x|。"""
    return float(np.max(np.abs(x.astype(np.float64))))


def crest_db(peak_v: float, rms_v: float) -> float | None:
    """波峰因数 = 20log10(peak/rms)。rms 为 0 时无定义。"""
    if rms_v == 0.0:
        return None
    return 20.0 * math.log10(peak_v / rms_v)


def sanitize(x: np.ndarray) -> tuple[np.ndarray, int]:
    """把 NaN / ±Inf 替换为 0，返回 (清洗后, 被替换个数)。

    对应 `DSP_SPEC.md` §2：「NaN/Inf 的处理策略必须显式定义……不允许静默传播」。
    """
    y = x.astype(np.float64).copy()
    bad = ~np.isfinite(y)
    count = int(np.count_nonzero(bad))
    y[bad] = 0.0
    return y, count


def detect_flat_top(x: np.ndarray, threshold: float = 0.999, min_run: int = 4) -> tuple[int, int, int]:
    """连续贴近满刻度的「平顶」片段检测（启发式）。

    返回 (片段数, 累计长度, 最长长度)。对应 `auralis-dsp::detect_flat_top`。
    """
    a = np.abs(np.where(np.isfinite(x), x, 0.0).astype(np.float64))
    hot = a >= threshold
    regions = 0
    total = 0
    longest = 0
    run = 0
    for v in hot:
        if v:
            run += 1
        else:
            if run >= min_run:
                regions += 1
                total += run
                longest = max(longest, run)
            run = 0
    if run >= min_run:
        regions += 1
        total += run
        longest = max(longest, run)
    return regions, total, longest


def window_coefficients(kind: str, n: int) -> np.ndarray:
    """周期型（periodic / DFT-even）窗系数。

    `DSP_SPEC.md` §5：谱分析必须用周期型 w[n] = f(2πn/N)，n = 0..N。
    对称型 w[n] = f(2πn/(N-1)) 会在频谱两端产生额外镜像旁瓣。
    """
    i = np.arange(n, dtype=np.float64)
    phase = TAU * i / n
    if kind == "rectangular":
        return np.ones(n)
    if kind == "hann":
        return 0.5 - 0.5 * np.cos(phase)
    if kind == "hamming":
        return 0.54 - 0.46 * np.cos(phase)
    if kind == "blackman":
        return 0.42 - 0.5 * np.cos(phase) + 0.08 * np.cos(2.0 * phase)
    raise ValueError(f"unknown window: {kind}")


def window_metrics(kind: str, n: int) -> tuple[float, float, float]:
    """返回 (相干增益, ENBW(bin), 系数和)。

    CG   = W / N
    ENBW = N·Σw² / W²   （单位 bin，与 N 无关）
    """
    w = window_coefficients(kind, n)
    s = float(np.sum(w))
    s2 = float(np.sum(np.square(w)))
    return s / n, n * s2 / (s * s), s


# --------------------------------------------------------------------------
# 标准测试信号：必须与 main.rs 的 canonical_signals() 逐样本一致
# --------------------------------------------------------------------------


def sine(freq_hz: float, amp: float, n: int, fs: int = SAMPLE_RATE) -> np.ndarray:
    """x[i] = amp * sin(2π·f·i/fs)，与 Rust 端同样用 f64 算相位再降到 f32。"""
    i = np.arange(n, dtype=np.float64)
    return (amp * np.sin(TAU * freq_hz * i / fs)).astype(np.float32)


def canonical_signals() -> list[tuple[str, np.ndarray]]:
    """与 `auralis-desktop --dump-reference` 使用完全相同的一组信号。"""
    out: list[tuple[str, np.ndarray]] = []
    out.append(("sine_1khz_amp0p5", sine(1_000.0, 0.5, 48_000)))
    out.append(("sine_1khz_amp1p0", sine(1_000.0, 1.0, 48_000)))
    out.append(("sine_1khz_amp1p5", sine(1_000.0, 1.5, 48_000)))
    out.append(("silence", np.zeros(48_000, dtype=np.float32)))
    out.append(("dc_0p1", np.full(48_000, 0.1, dtype=np.float32)))

    dirty = np.full(1_000, 0.1, dtype=np.float32)
    dirty[10] = np.nan
    dirty[20] = np.inf
    dirty[30] = -np.inf
    out.append(("with_nan_inf", dirty))

    flat = np.concatenate(
        [
            np.arange(10, dtype=np.float32) * 0.2 - 1.0,  # -1.0 .. 0.8
            np.full(12, 0.999, dtype=np.float32),          # 正平顶
            0.8 - np.arange(10, dtype=np.float32) * 0.18,  # 下降段
            np.full(12, -0.999, dtype=np.float32),         # 负平顶
        ]
    ).astype(np.float32)
    out.append(("flat_top_clipped", flat))
    return out


# --------------------------------------------------------------------------
# 对照逻辑
# --------------------------------------------------------------------------


class Report:
    def __init__(self) -> None:
        self.failures: list[str] = []
        self.checks = 0

    def close(self, name: str, rust, expect, tol: float, unit: str = "") -> None:
        self.checks += 1
        label = f"{name}{(' ' + unit) if unit else ''}"

        # 两边都应为 None（真 -inf / 未定义）
        if expect is None or rust is None:
            if (expect is None) != (rust is None):
                self.failures.append(
                    f"{label}: rust={rust!r} 期望={expect!r}（一方为 None 而另一方不是）"
                )
            return

        if not math.isfinite(rust) or not math.isfinite(expect):
            self.failures.append(f"{label}: 非有限值 rust={rust!r} 期望={expect!r}")
            return

        if abs(rust - expect) > tol:
            self.failures.append(
                f"{label}: rust={rust:.9g} numpy={expect:.9g} 差={abs(rust - expect):.3g} > 容差 {tol:g}"
            )


def main() -> int:
    ap = argparse.ArgumentParser(description="Auralis Studio DSP oracle")
    ap.add_argument("--rust-json", required=True, help="auralis-studio --dump-reference 的输出")
    ap.add_argument(
        "--tol-db", type=float, default=TOL_DB, help="dB 类容差 (default 0.01)"
    )
    args = ap.parse_args()

    path = Path(args.rust_json)
    if not path.is_file():
        print(f"找不到参考文件: {path}", file=sys.stderr)
        return 2

    data = json.loads(path.read_text(encoding="utf-8"))
    rep = Report()

    print(f"oracle: {data.get('algorithm_version')}")
    print(f"rust json: {path}")
    print()

    # ---- 逐个信号对照 ----
    rust_cases = {c["name"]: c for c in data["cases"]}
    our_cases = dict(canonical_signals())

    missing = set(rust_cases) ^ set(our_cases)
    if missing:
        print(f"FAIL 信号集合不一致: 仅在一侧出现的 = {sorted(missing)}", file=sys.stderr)
        return 1

    for name, sig in our_cases.items():
        rust = rust_cases[name]
        clean, sanitized = sanitize(sig)

        pk = peak(clean)
        rm = rms(clean)
        dc = dc_offset(clean)
        regions, total, longest = detect_flat_top(clean)
        ofs = int(np.count_nonzero(np.abs(clean) > 1.0))

        if rust["sample_count"] != len(clean):
            rep.failures.append(
                f"{name}.sample_count: rust={rust['sample_count']} numpy={len(clean)}"
            )

        rep.close(f"{name}.peak", rust["peak"], pk, TOL_LIN, "FS")
        rep.close(f"{name}.rms", rust["rms"], rm, TOL_LIN, "FS")
        rep.close(f"{name}.dc", rust["dc"], dc, TOL_DC, "FS")
        rep.close(f"{name}.peak_dbfs", rust["peak_dbfs"], dbfs(pk), args.tol_db, "dBFS")
        rep.close(f"{name}.rms_dbfs", rust["rms_dbfs"], dbfs(rm), args.tol_db, "dBFS")
        rep.close(f"{name}.crest_db", rust["crest_db"], crest_db(pk, rm), args.tol_db, "dB")

        # 整数/布尔量必须完全一致，不容误差
        for key, ours in (
            ("over_full_scale", bool(ofs > 0)),
            ("over_full_scale_samples", ofs),
            ("clipping_regions", regions),
            ("clipping_longest", longest),
            ("clipping_total", total),
            ("sanitized_samples", sanitized),
        ):
            rep.checks += 1
            if rust[key] != ours:
                rep.failures.append(f"{name}.{key}: rust={rust[key]!r} numpy={ours!r}")

    # ---- 窗函数对照 ----
    for rw in data["windows"]:
        kind = rw["id"]
        n = rw["size"]
        cg, enbw, ssum = window_metrics(kind, n)
        rep.close(f"window[{kind}].coherent_gain", rw["coherent_gain"], cg, TOL_LIN)
        rep.close(f"window[{kind}].enbw_bins", rw["enbw_bins"], enbw, TOL_LIN)
        rep.close(f"window[{kind}].coeff_sum", rw["coeff_sum"], ssum, 1e-3)

        ours_first8 = window_coefficients(kind, n)[:8]
        rep.checks += 1
        for k, (r, o) in enumerate(zip(rw["coeff_first8"], ours_first8)):
            if r is None or abs(r - float(o)) > 1e-6:
                rep.failures.append(
                    f"window[{kind}].coeff_first8[{k}]: rust={r!r} numpy={float(o)!r}"
                )

    # ---- 专项：静音必须是真 -inf ----
    rust_silence = rust_cases["silence"]
    rep.checks += 1
    if rust_silence["rms_dbfs"] is not None:
        rep.failures.append(
            f"silence.rms_dbfs 应为 null(=真 -inf)，实为 {rust_silence['rms_dbfs']!r}"
        )

    # ---- 汇总 ----
    print(f"对照项: {rep.checks}")
    if rep.failures:
        print(f"\n不一致 {len(rep.failures)} 项：", file=sys.stderr)
        for f in rep.failures:
            print(f"  FAIL {f}", file=sys.stderr)
        return 1

    print("所有 numpy 参考值与 Rust 输出一致")
    return 0


if __name__ == "__main__":
    sys.exit(main())