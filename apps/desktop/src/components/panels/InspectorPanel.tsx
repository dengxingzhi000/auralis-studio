/**
 * InspectorPanel —— 右侧检查器。
 *
 * 电平、信号指标、参数与显示设置。全部是已算好的结果或纯界面状态；
 * 组件本身不触发音频计算，也不读存储（ADR-018 规则 1）。
 *
 * `analyzing` 只表示分析正在进行：合成信号按钮不应因为导出而改文案或禁用。
 */

import type { Dispatch, SetStateAction } from "react";

import { LevelMeter } from "@/components/audio-viz/LevelMeter";
import {
  Button,
  Collapsible,
  DataRow,
  ErrorBar,
  FormField,
  MetricRow,
  Select,
} from "@/components/ui/primitives";
import type { AppInfo, FileInfo, SyntheticRequest } from "@/lib/contracts";
import type { DisplayData } from "@/lib/display";
import { fmtHz, fmtRange } from "@/lib/format";
import { DB_RANGES } from "@/lib/prefs";
import type { ChannelLayout, SpectrumMode } from "@/lib/view";

export interface InspectorPanelProps {
  req: SyntheticRequest;
  setReq: Dispatch<SetStateAction<SyntheticRequest>>;
  display: DisplayData | null;
  selected: FileInfo | null;
  /** 分析进行中；导出不置位 */
  analyzing: boolean;
  onRun: () => void;
  info: AppInfo | null;
  viewRange: [number, number] | null;
  onResetViewRange: () => void;
  /** 当前 dB 量程（与中央画布一致） */
  minDb: number;
  maxDb: number;
  spectrumMode: SpectrumMode;
  onSpectrumModeChange: (m: SpectrumMode) => void;
  dbRangeIdx: number;
  onDbRangeChange: (idx: number) => void;
  channelLayout: ChannelLayout;
  onChannelLayoutChange: (l: ChannelLayout) => void;
}

/**
 * FFT 点数选择器。
 *
 * 用固定候选而非自由输入 + log2 取整：
 *   - DSP_SPEC §5 只定义 2 的幂长度，自由输入会被静默取整，用户不知情
 *   - 避免在前端做任何数值运算（ADR-018：前端只做展示）
 */
const FFT_SIZES = [1024, 2048, 4096, 8192, 16384] as const;

function FftSizeField({
  value,
  onChange,
}: {
  value: number;
  onChange: (v: number) => void;
}) {
  return (
    <FormField label="FFT 点数">
      <Select
        className="num"
        value={value}
        onValueChange={(v) => onChange(Number(v))}
      >
        {FFT_SIZES.map((n) => (
          <option key={n} value={n}>
            {n}
          </option>
        ))}
      </Select>
    </FormField>
  );
}

export function InspectorPanel({
  req,
  setReq,
  display,
  selected,
  analyzing,
  onRun,
  info,
  viewRange,
  onResetViewRange,
  minDb,
  maxDb,
  spectrumMode,
  onSpectrumModeChange,
  dbRangeIdx,
  onDbRangeChange,
  channelLayout,
  onChannelLayoutChange,
}: InspectorPanelProps) {
  const set = <K extends keyof SyntheticRequest>(
    k: K,
    v: SyntheticRequest[K],
  ) => setReq((r) => ({ ...r, [k]: v }));

  return (
    <div className="panel-body">
      {display?.outcome === "failed" && (
        <div style={{ marginBottom: 8 }}>
          <ErrorBar title="分析未成功完成" />
        </div>
      )}

      <div className="label" style={{ marginBottom: 5 }}>
        电平
      </div>
      <div className="readout">
        <LevelMeter
          peakDbfs={display?.peakDbfs ?? null}
          rmsDbfs={display?.rmsDbfs ?? null}
          peakPerChannel={display?.peakPerChannel ?? []}
          rmsPerChannel={[]}
          minDb={minDb}
          maxDb={maxDb}
          overFullScale={display ? display.overFullScaleSamples : null}
          clippingRegions={display ? display.clippingRegions : null}
        />
      </div>

      <div className="label" style={{ marginBottom: 5, marginTop: 12 }}>
        信号指标
      </div>
      <div className="readout">
        <MetricRow label="Peak" value={display?.peakDbfs} unit="dBFS" />
        <MetricRow label="RMS" value={display?.rmsDbfs} unit="dBFS" />
        <MetricRow label="Crest" value={display?.crestDb} unit="dB" />
        <DataRow
          label="DC Offset"
          value={display ? display.dc.toExponential(2) : "—"}
        />
        <DataRow
          label="包络点数"
          value={
            display ? display.env?.length.toLocaleString("en-US") ?? "—" : "—"
          }
        />
        {/* 多声道文件的逐声道峰值：单声道时与上面的 Peak 完全重复，不显示 */}
        {display && display.peakPerChannel.length > 1 && (
          <div
            style={{
              marginTop: 4,
              paddingTop: 4,
              borderTop: "1px solid var(--grid-minor)",
            }}
          >
            {display.peakPerChannel.map((p, i) => (
              <DataRow
                key={i}
                label={`CH${i + 1} Peak`}
                value={p == null ? "—（静音）" : `${p.toFixed(2)} dBFS`}
              />
            ))}
          </div>
        )}
        {/* 数据质量：这些计数为 0 也要显示，否则「有没有检查过」无法区分 */}
        <DataRow
          label="超量程样本"
          value={
            display
              ? display.overFullScaleSamples.toLocaleString("en-US")
              : "—"
          }
        />
        <DataRow
          label="平顶片段"
          value={
            display ? display.clippingRegions.toLocaleString("en-US") : "—"
          }
          title="启发式计数，不是模拟削波的证据"
        />
        <DataRow
          label="清理样本"
          value={
            display ? display.sanitizedSamples.toLocaleString("en-US") : "—"
          }
          title="NaN / Inf 被替换为 0 的数量"
        />
      </div>

      {display && display.peaks.length > 0 && (
        <div style={{ marginTop: 12 }}>
          <div className="label" style={{ marginBottom: 5 }}>
            峰值
          </div>
          <div className="readout">
            {display.peaks.slice(0, 8).map((p, i) => (
              <div className="metric-row" key={i}>
                <span className="muted num">{i + 1}</span>
                <span className="num">
                  {fmtHz(p.frequencyHz)}
                  <span className="muted"> Hz · </span>
                  {p.amplitudeDbfs.toFixed(2)}
                  <span className="muted"> dBFS</span>
                  {p.estimated && (
                    <span
                      className="est-tag"
                      title={`扇贝损失 ${p.scallopingDb.toFixed(2)} dB，读数不可靠`}
                    >
                      估算
                    </span>
                  )}
                </span>
              </div>
            ))}
          </div>
          <div className="note">
            FFT {display.fftSize} · {display.window} ·{" "}
            <span className="num">{display.stepHz.toFixed(3)}</span> Hz/bin
          </div>
        </div>
      )}

      <div style={{ marginTop: 12 }}>
        <div className="label" style={{ marginBottom: 2 }}>
          参数
        </div>
        <Collapsible title="合成信号自检">
          <FormField label="频率" unit="Hz">
            <input
              type="number"
              className="num"
              value={req.frequencyHz}
              step={50}
              onChange={(e) => {
                const v = Number(e.target.value);
                if (Number.isFinite(v)) set("frequencyHz", v);
              }}
            />
          </FormField>
          <FormField label="幅值" unit="FS">
            <input
              type="number"
              className="num"
              value={req.amplitude}
              step={0.1}
              onChange={(e) => {
                const v = Number(e.target.value);
                if (Number.isFinite(v)) set("amplitude", v);
              }}
            />
          </FormField>
          <FftSizeField
            value={req.fftSize}
            onChange={(v) => set("fftSize", v)}
          />
          <div style={{ marginTop: 8 }}>
            <Button
              variant="primary"
              wide
              onClick={onRun}
              disabled={analyzing}
            >
              {analyzing ? "计算中…" : "分析合成信号"}
            </Button>
          </div>
          <p className="note">
            合成 1 kHz 正弦，用于验证 IPC 通路与渲染；不读取也不修改任何文件。
            {selected
              ? "运行后会顶替画布上当前文件的结果，点左侧文件名可重新分析恢复。"
              : ""}
          </p>
        </Collapsible>

        {selected && (
          <Collapsible
            title="文件"
            meta={
              [
                // 合成信号分析会把画布换掉：此时这一节讲的是**已打开的文件**，
                // 不是屏幕上画的东西，必须点名，否则就是在标签上撒谎。
                display?.source === "synthetic" ? "未显示" : null,
                selected.verified ? null : "未验证",
              ]
                .filter(Boolean)
                .join(" · ") || undefined
            }
            defaultOpen
          >
            {display?.source === "synthetic" && (
              <p className="note warn-note">
                下列是已打开文件的属性；画布上显示的是合成信号，不是这份文件。
              </p>
            )}
            <DataRow label="容器" value={selected.containerName} />
            <DataRow label="编码" value={selected.codec} />
            <DataRow
              label="采样率"
              value={`${selected.sampleRate.toLocaleString("en-US")} Hz`}
            />
            <DataRow label="声道" value={String(selected.channels)} />
            <DataRow
              label="时长"
              value={
                selected.durationSeconds == null
                  ? "—"
                  : `${selected.durationSeconds.toFixed(3)} s`
              }
            />
            <DataRow
              label="文件大小"
              value={`${(selected.fileSizeBytes / 1024).toFixed(1)} KiB`}
            />
            {!selected.verified && (
              <p className="note warn-note">{selected.verificationNote}</p>
            )}
          </Collapsible>
        )}

        <Collapsible title="显示" defaultOpen>
          <FormField label="频谱形态">
            <Select
              value={spectrumMode}
              onValueChange={(v) =>
                onSpectrumModeChange(v === "line" ? "line" : "area")
              }
            >
              <option value="area">填充（面积）</option>
              <option value="line">曲线（声线）</option>
            </Select>
          </FormField>
          <FormField label="dB 量程">
            <Select
              value={String(dbRangeIdx)}
              onValueChange={(v) => onDbRangeChange(Number(v))}
            >
              {DB_RANGES.map((r, i) => (
                <option key={r.label} value={i}>
                  {r.label}
                </option>
              ))}
            </Select>
          </FormField>
          <FormField label="声道显示">
            <Select
              value={channelLayout}
              onValueChange={(v) =>
                onChannelLayoutChange(v === "overlay" ? "overlay" : "lanes")
              }
            >
              <option value="lanes">分道</option>
              <option value="overlay">叠加（对比）</option>
            </Select>
          </FormField>
          <DataRow label="当前量程" value={`${minDb} … ${maxDb} dBFS`} />
          <DataRow label="视口" value={fmtRange(viewRange)} />
          <DataRow label="频谱点数" value={String(req.targetBins)} />
          <div style={{ marginTop: 8 }}>
            <Button
              size="tiny"
              wide
              onClick={onResetViewRange}
              disabled={!viewRange}
            >
              复位缩放
            </Button>
          </div>
        </Collapsible>

        <Collapsible title="关于">
          <DataRow label="版本" value={info?.version ?? "—"} />
          <DataRow label="算法" value={info?.algorithmVersion ?? "—"} mono />
          <DataRow label="DSP 规范" value={info?.dspSpecRevision ?? "—"} />
          <p className="note">
            dB 换算、FFT、降采样全部在 Rust 侧完成，前端只画结果（ADR-018）。
            真实文件与实时输入共用同一条计算链路。
          </p>
        </Collapsible>
      </div>
    </div>
  );
}
