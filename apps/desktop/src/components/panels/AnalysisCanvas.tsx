/**
 * AnalysisCanvas —— 中央画布（频谱 / 声谱图 / 波形 / 指标）。
 *
 * 纯展示组件：所有数据都是 Rust 已经算好并降采样的结果，所有回调都由
 * `App` 提供。这里不读 IPC、不读存储，也不做任何数值计算（ADR-018）。
 */

import type { RefObject } from "react";

import { MetricsPanel } from "@/components/audio-viz/MetricsPanel";
import { SpectrogramView } from "@/components/audio-viz/SpectrogramView";
import { SpectrumView } from "@/components/audio-viz/SpectrumView";
import { WaveformView } from "@/components/audio-viz/WaveformView";
import { Button, ErrorBar } from "@/components/ui/primitives";
import type { FileInfo, PeakInfo } from "@/lib/contracts";
import type { DisplayData } from "@/lib/display";
import type { UiError } from "@/lib/errors";
import { fmtClock, fmtHz, fmtHzInt } from "@/lib/format";
import type { ChannelLayout, SpectrumMode } from "@/lib/view";

/**
 * 稳定的空峰值数组。
 *
 * `liveDb ? [] : (display?.peaks ?? [])` 每次渲染都会产生新引用，而
 * SpectrumView 的绘制 effect 依赖 `peaks` —— 任意一次重渲染都会重绘频谱
 * 并重建后备存储。实时态下峰值本来就该留空（频率位置对不上），
 * 用模块级常量固定引用。
 */
const NO_PEAKS: PeakInfo[] = [];

export interface AnalysisCanvasProps {
  error: UiError | null;
  /** 有可重试的上一次路径 */
  canRetry: boolean;
  onRetry: () => void;
  onDismissError: () => void;

  /** 频谱卡片的容器；`App` 用它观察宽度并调整频谱点数。 */
  spectrumRef: RefObject<HTMLDivElement | null>;

  liveDb: Float32Array | null;
  liveStepHz: number;
  livePeak: number | null;
  liveRms: number | null;
  display: DisplayData | null;
  selected: FileInfo | null;

  /** 当前用于频率轴的采样率（实时态下是设备采样率） */
  sampleRate: number;
  minDb: number;
  maxDb: number;
  cursorHz: number | null;
  onHoverHz: (hz: number | null) => void;
  viewRange: [number, number] | null;
  onViewRangeChange: (r: [number, number] | null) => void;
  spectrumMode: SpectrumMode;
  channelLayout: ChannelLayout;

  spectroStart: number;
  spectrogramSeconds: number;
  /** 一屏请求的时长（文件短于 30 秒时就是文件时长） */
  spectrogramWindow: number;
  durationSeconds: number;
  showSpectroPaging: boolean;
  /** 是否允许翻页 / 加载视口（分析进行中为 false） */
  canPage: boolean;
  spectroLoading: boolean;
  onLoadSpectrogramWindow: (start: number) => void;
  onPanSpectrogram: (delta: number) => void;
}

export function AnalysisCanvas({
  error,
  canRetry,
  onRetry,
  onDismissError,
  spectrumRef,
  liveDb,
  liveStepHz,
  livePeak,
  liveRms,
  display,
  selected,
  sampleRate,
  minDb,
  maxDb,
  cursorHz,
  onHoverHz,
  viewRange,
  onViewRangeChange,
  spectrumMode,
  channelLayout,
  spectroStart,
  spectrogramSeconds,
  spectrogramWindow,
  durationSeconds,
  showSpectroPaging,
  canPage,
  spectroLoading,
  onLoadSpectrogramWindow,
  onPanSpectrogram,
}: AnalysisCanvasProps) {
  return (
    <div className="center-stack">
      {error && (
        <div style={{ marginBottom: 8 }}>
          <ErrorBar
            title={error.title}
            detail={error.detail}
            action={
              <>
                {canRetry && (
                  <Button size="tiny" onClick={onRetry} title="Ctrl+R">
                    重试
                  </Button>
                )}
                <Button size="tiny" onClick={onDismissError}>
                  关闭
                </Button>
              </>
            }
          />
        </div>
      )}
      <section className="pane" id="pane-spectrum" style={{ flex: "1.35" }}>
        <div className="pane-head">
          <span className="pane-title">
            频谱
            {liveDb && <span className="rt-badge">实时</span>}
          </span>
          <span className="pane-meta">
            {liveDb
              ? `${liveDb.length} bins`
              : display?.db
                ? `${display.db.length} bins`
                : "等待数据"}
            <span className="sep">·</span>
            {liveDb
              ? `${liveStepHz.toFixed(1)} Hz/bin`
              : display
                ? `${display.fftSize} 点 FFT`
                : "FFT 4096"}
            <span className="sep">·</span>
            {fmtHzInt(sampleRate / 2)} Hz 奈奎斯特
            {/* 画的是合成信号时必须标出来：否则用户会以为这是文件的频谱。 */}
            {!liveDb && display?.source === "synthetic" && (
              <>
                <span className="sep">·</span>
                <span className="est-tag">合成信号</span>
              </>
            )}
            {viewRange && (
              <>
                <span className="sep">·</span>
                <span className="num">
                  {fmtHz(viewRange[0])}–{fmtHz(viewRange[1])} Hz
                </span>
              </>
            )}
          </span>
          <span className="spacer" />
          {/* 光标读数只在悬停时出现：常驻一个 "—" 只是噪声 */}
          {cursorHz != null && (
            <span className="pane-meta num" title="光标频率">
              {fmtHz(cursorHz)} Hz
            </span>
          )}
          {viewRange && (
            <Button
              size="tiny"
              onClick={() => onViewRangeChange(null)}
              title="回到全带宽"
            >
              复位缩放
            </Button>
          )}
        </div>
        <div className="pane-body" ref={spectrumRef}>
          {/* 实时数据上不能叠文件分析出的峰值标记：频率位置对不上，
              会让人以为那是当前信号的峰值。 */}
          <SpectrumView
            db={liveDb ?? display?.db ?? null}
            stepHz={liveDb ? liveStepHz : (display?.stepHz ?? 1)}
            sampleRate={sampleRate}
            minDb={minDb}
            maxDb={maxDb}
            cursorHz={cursorHz}
            peaks={liveDb ? NO_PEAKS : display?.peaks}
            viewRange={viewRange}
            onViewRangeChange={onViewRangeChange}
            onHoverHz={onHoverHz}
            mode={spectrumMode}
          />
        </div>
        <div className="pane-note">
          拖拽框选缩放 · 滚轮以光标为中心缩放 · 双击复位
          <span className="sep">·</span>
          dBFS，未做校准
        </div>
      </section>

      <div className="splitter splitter-h" />

      <section className="pane" id="pane-spectrogram" style={{ flex: 1 }}>
        <div className="pane-head">
          <span className="pane-title">声谱图</span>
          <span className="pane-meta">
            STFT 2048/512
            <span className="sep">·</span>
            {display && display.spectrogramFrames > 0
              ? `${display.spectrogramFrames} × ${display.spectrogramBins}`
              : display?.source === "synthetic"
                ? "合成信号不产生声谱图"
                : "等待数据"}
            {display && display.spectrogramHopMs > 0 && (
              <>
                <span className="sep">·</span>
                <span className="num">
                  {display.spectrogramHopMs.toFixed(2)} ms/列
                </span>
              </>
            )}
            {display && display.spectrogramFrames > 0 && (
              <>
                <span className="sep">·</span>
                <span className="num" title="矩阵覆盖的绝对时间范围">
                  {fmtClock(spectroStart)}–
                  {fmtClock(spectroStart + spectrogramSeconds)}
                </span>
              </>
            )}
          </span>
          <span className="spacer" />
          {viewRange && (
            <>
              <span className="pane-meta num" title="当前频率视口">
                {fmtHz(viewRange[0])}–{fmtHz(viewRange[1])} Hz
              </span>
              <Button
                size="tiny"
                onClick={() => onViewRangeChange(null)}
                title="恢复全带宽频率视口"
              >
                频率复位
              </Button>
            </>
          )}
          {showSpectroPaging && (
            <>
              <Button
                size="tiny"
                onClick={() =>
                  onLoadSpectrogramWindow(
                    spectroStart - spectrogramSeconds * 0.8,
                  )
                }
                disabled={!canPage || spectroLoading || spectroStart <= 0}
                title="向前一屏"
              >
                ◀
              </Button>
              <Button
                size="tiny"
                onClick={() => onLoadSpectrogramWindow(0)}
                disabled={!canPage || spectroLoading || spectroStart <= 0}
                title="回到文件开头"
              >
                开头
              </Button>
              <Button
                size="tiny"
                onClick={() =>
                  onLoadSpectrogramWindow(
                    spectroStart + spectrogramSeconds * 0.8,
                  )
                }
                disabled={
                  !canPage ||
                  spectroLoading ||
                  spectroStart >=
                    Math.max(0, durationSeconds - spectrogramWindow) - 1e-6
                }
                title="向后一屏"
              >
                ▶
              </Button>
            </>
          )}
          {spectroLoading && <span className="pane-meta">载入…</span>}
        </div>
        <div className="pane-body">
          <SpectrogramView
            matrix={display?.spectrogram ?? null}
            width={display?.spectrogramFrames ?? 0}
            height={display?.spectrogramBins ?? 0}
            minDb={minDb}
            maxDb={maxDb}
            cursorHz={cursorHz}
            // 声谱图画的是**文件**的矩阵，频率轴必须用文件自己的
            // 采样率。实时采集开着时 `sampleRate` 会切成设备的采样率，
            // 那样频率刻度会整体错位 —— 而画面看上去完全正常。
            nyquistHz={(display?.sampleRate ?? sampleRate) / 2}
            durationSeconds={spectrogramSeconds}
            startSeconds={spectroStart}
            onPan={onPanSpectrogram}
            viewRange={viewRange}
            onViewRangeChange={onViewRangeChange}
          />
        </div>
        <div className="pane-note">
          滚轮 / 拖拽平移时间
          <span className="sep">·</span>
          Shift + 滚轮缩放频率 · Shift + 拖拽框选频段
          <span className="sep">·</span>
          双击回到文件开头
        </div>
      </section>

      <div className="splitter splitter-h" />

      <section className="pane" id="pane-waveform" style={{ flex: 1 }}>
        <div className="pane-head">
          <span className="pane-title">波形</span>
          <span className="pane-meta">
            min/max 包络
            <span className="sep">·</span>
            {display?.env?.length ?? 0} 点
            {/* 标题跟着**显示数据的来源**走：合成信号不能顶着文件名，
                否则看起来就是一个挂着文件名的默认页。 */}
            {display?.source === "synthetic" ? (
              <>
                <span className="sep">·</span>
                <span className="est-tag">合成信号</span>
              </>
            ) : (
              selected && (
                <>
                  <span className="sep">·</span>
                  {selected.fileName}
                </>
              )
            )}
          </span>
        </div>
        <div className="pane-body">
          {/* 合成信号是单声道：别拿已打开文件的声道数去切它的包络。 */}
          <WaveformView
            envelope={display?.env ?? null}
            sampleRate={sampleRate}
            channels={
              display?.source === "synthetic" ? 1 : (selected?.channels ?? 1)
            }
            durationSeconds={durationSeconds}
            layout={channelLayout}
          />
        </div>
      </section>

      <div className="splitter splitter-h" />

      <section className="pane pane-metrics" id="pane-metrics">
        <div className="pane-head">
          <span className="pane-title">电平 / 指标</span>
          <span className="pane-meta">
            {/* 跟着**显示数据**的来源走：合成信号不能只写「已分析」，
                否则这张卡看起来像在报已打开文件的指标。 */}
            {liveDb
              ? "实时采集"
              : display
                ? display.source === "synthetic"
                  ? "合成信号"
                  : "已分析"
                : "等待数据"}
            {display && (
              <>
                <span className="sep">·</span>
                <span className="num">{display.elapsedMs.toFixed(1)} ms</span>
              </>
            )}
          </span>
        </div>
        <div className="pane-body pane-scroll">
          <MetricsPanel
            display={display}
            live={
              liveDb ? { peakDbfs: livePeak, rmsDbfs: liveRms } : null
            }
            minDb={minDb}
            maxDb={maxDb}
          />
        </div>
      </section>
    </div>
  );
}
