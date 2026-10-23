/**
 * MetricsPanel —— 「电平 / 指标」视图（UI_SPEC §3 / §4 的 `AnalysisSummaryView`）。
 *
 * ## 这一块只显示，不计算
 *
 * 所有读数都来自 Rust 已经算好的结果。前端**不做**任何 dB 换算、
 * 不求 RMS、不检测峰值（ADR-018 规则 1）。这里唯一的「计算」是
 * 把数字格式化成字符串。
 *
 * ## 为什么「—」和 0 必须分开
 *
 * `peakDbfs` 为 `null` 表示**真静音**（`-inf`），不是「峰值是 0」。
 * 显示成 0 会让用户以为「有信号但极小」，那是编造的读数。
 *
 * ## 数据质量为什么要单列
 *
 * 超量程样本、平顶片段、被清理的非有限样本 —— 这三个计数为 0 也有意义：
 * 它们区分「检查过，没发现问题」与「根本没检查」。同样地，文件被截断时
 * 必须说出「分析只覆盖了文件里实际存在的那一段」，否则用户会以为
 * 分析覆盖了整个文件。
 */

import type { DisplayData } from "@/lib/display";

import { LevelMeter } from "./LevelMeter";
import { DataRow, MetricRow } from "@/components/ui/primitives";

export interface MetricsPanelProps {
  /** 文件分析结果；null = 还没打开文件 */
  display: DisplayData | null;
  /** 实时读数；null = 未在采集 */
  live: { peakDbfs: number | null; rmsDbfs: number | null } | null;
  /** 电平表量程下限 dB */
  minDb: number;
  /** 电平表量程上限 dB */
  maxDb: number;
}

export function MetricsPanel({
  display,
  live,
  minDb,
  maxDb,
}: MetricsPanelProps) {
  if (display === null && live === null) {
    return (
      <div className="metrics-empty">
        <span className="empty-hint">
          打开一个音频文件，或启动实时采集，这里会显示电平、信号指标与数据质量。
        </span>
      </div>
    );
  }

  // 实时采集时电平表显示**当前**读数；其余指标仍属于已打开的文件。
  const peak = live ? live.peakDbfs : (display?.peakDbfs ?? null);
  const rms = live ? live.rmsDbfs : (display?.rmsDbfs ?? null);

  return (
    <div className="metrics-grid">
      {/* ---- 电平 ---- */}
      <section className="metrics-col">
        <h3 className="metrics-title">
          电平
          {live && <span className="rt-badge">实时</span>}
        </h3>
        <LevelMeter
          peakDbfs={peak}
          rmsDbfs={rms}
          peakPerChannel={display?.peakPerChannel ?? []}
          minDb={minDb}
          maxDb={maxDb}
          overFullScale={display?.overFullScaleSamples ?? 0}
          clippingRegions={display?.clippingRegions ?? 0}
        />
        {display && display.peakPerChannel.length > 1 && (
          <div className="metrics-sub">
            {display.peakPerChannel.map((p, i) => (
              <DataRow
                key={i}
                label={`CH${i + 1} Peak`}
                value={p == null ? "—（静音）" : `${p.toFixed(2)} dBFS`}
              />
            ))}
          </div>
        )}
      </section>

      {/* ---- 信号指标 ---- */}
      <section className="metrics-col">
        <h3 className="metrics-title">信号指标</h3>
        <MetricRow label="Peak" value={display?.peakDbfs} unit="dBFS" />
        <MetricRow label="RMS" value={display?.rmsDbfs} unit="dBFS" />
        <MetricRow label="Crest" value={display?.crestDb} unit="dB" />
        <DataRow
          label="DC Offset"
          value={display ? display.dc.toExponential(2) : "—"}
        />
        <div className="metrics-sub">
          <DataRow
            label="FFT"
            value={display ? `${display.fftSize} 点 · ${display.window}` : "—"}
            mono={false}
          />
        </div>
      </section>

      {/* ---- 数据质量 ---- */}
      <section className="metrics-col">
        <h3 className="metrics-title">数据质量</h3>
        <DataRow
          label="超量程样本"
          value={(display?.overFullScaleSamples ?? 0).toLocaleString("en-US")}
          title="|x| > 1.0 的样本数"
        />
        <DataRow
          label="平顶片段"
          value={(display?.clippingRegions ?? 0).toLocaleString("en-US")}
          title="启发式计数，不是模拟削波的证据"
        />
        <DataRow
          label="清理样本"
          value={(display?.sanitizedSamples ?? 0).toLocaleString("en-US")}
          title="NaN / Inf 被替换为 0 的数量"
        />
        <DataRow
          label="包络点数"
          value={
            display?.env?.length != null
              ? display.env.length.toLocaleString("en-US")
              : "—"
          }
        />
        <DataRow
          label="文件完整性"
          value={
            display === null
              ? "—"
              : display.truncated
                ? `不完整 · 声明 ${display.declaredDurationSeconds.toFixed(2)} s，` +
                  `实际 ${display.decodedDurationSeconds.toFixed(2)} s`
                : "完整"
          }
          mono={false}
        />
        {display?.truncated && (
          <p className="note warn-note metrics-note">
            只分析了文件里实际存在的那一部分。重新获取完整文件可得到覆盖全长的结果。
          </p>
        )}
      </section>
    </div>
  );
}
