/**
 * LevelMeter —— 峰值 / RMS 电平表（UI_SPEC §4）。
 *
 * 取值全部来自 Rust 已经算好的 dBFS 指标，前端只做**线性映射到像素宽度**：
 * 把 `[minDb, maxDb]` 映射到 `[0%, 100%]`。不做任何 dB 换算、不做平滑、
 * 不做峰值保持算法（那会把「峰值」变成另一个数）。
 *
 * ## 为什么用 DOM 而不是 Canvas
 *
 * 电平条是每帧最多刷几次的少量矩形，DOM + CSS 足够；Canvas 反而要自己管
 * DPR 与重绘。声谱图那种密集纹理才需要 Canvas。
 *
 * ## 色彩语义
 *
 * 渐变铺满整条轨道，再用遮罩盖住右侧未达到的部分——这样**颜色位置始终
 * 表示绝对电平**（绿=安全、黄=接近满刻度、红=满刻度），不会因为条子变短
 * 而把整条染成绿色。
 */

export interface LevelMeterProps {
  /** 整体峰值 dBFS；null = 静音 */
  peakDbfs: number | null;
  /** 整体 RMS dBFS；null = 静音 */
  rmsDbfs: number | null;
  /** 逐声道峰值 dBFS；空数组时只显示一条总体电平 */
  peakPerChannel: (number | null)[];
  /** 量程下限 dB */
  minDb: number;
  /** 量程上限 dB */
  maxDb: number;
  /** 超过满刻度的样本数（|x| > 1）；`null` = 未统计（例如实时输入） */
  overFullScale: number | null;
  /** 削波平顶片段数（启发式）；`null` = 未统计 */
  clippingRegions: number | null;
}

/** dB -> 轨道百分比。非有限值（静音）按 0% 处理，绝不编造一个电平。 */
function pct(db: number | null, minDb: number, maxDb: number): number {
  if (db == null || !Number.isFinite(db)) return 0;
  const span = Math.max(1e-6, maxDb - minDb);
  return Math.min(100, Math.max(0, ((db - minDb) / span) * 100));
}

function fmtDb(db: number | null): string {
  if (db == null || !Number.isFinite(db)) return "—";
  return db.toFixed(1);
}

export function LevelMeter({
  peakDbfs,
  rmsDbfs,
  peakPerChannel,
  minDb,
  maxDb,
  overFullScale,
  clippingRegions,
}: LevelMeterProps) {
  const channels =
    peakPerChannel.length > 0
      ? peakPerChannel.map((p, i) => ({
          label: `CH${i + 1}`,
          peak: p,
          // 逐声道的 RMS 没有单独计算，这里不假装有：只标整体 RMS
          rms: peakPerChannel.length === 1 ? rmsDbfs : null,
        }))
      : [{ label: "MIX", peak: peakDbfs, rms: rmsDbfs }];

  return (
    <div className="meter">
      {channels.map((c) => (
        <div className="meter-row" key={c.label}>
          <span className="meter-label">{c.label}</span>
          <span
            className="meter-track"
            title={`峰值 ${fmtDb(c.peak)} dBFS${
              c.rms != null ? ` · RMS ${fmtDb(c.rms)} dBFS` : ""
            }`}
          >
            <span className="meter-grad" />
            <span
              className="meter-mask"
              style={{ width: `${100 - pct(c.peak, minDb, maxDb)}%` }}
            />
            {c.rms != null && (
              <span
                className="meter-rms"
                style={{ left: `${pct(c.rms, minDb, maxDb)}%` }}
                title={`RMS ${fmtDb(c.rms)} dBFS`}
              />
            )}
          </span>
          <span className="meter-value num">{fmtDb(c.peak)}</span>
        </div>
      ))}

      <div className="meter-foot">
        <span className="num muted">
          {minDb} … {maxDb} dBFS
        </span>
        <span className="spacer" />
        {overFullScale == null ? (
          // 未知不等于「没有」：实时输入没有文件级统计，显示 0 会变成
          // 「检查过，没发现超量程」的假声明。
          <span className="muted" title="实时输入不统计文件级数据质量">
            —
          </span>
        ) : overFullScale > 0 ? (
          <span
            className="meter-clip"
            title={`${overFullScale.toLocaleString("en-US")} 个样本超过满刻度；平顶片段 ${
              clippingRegions ?? 0
            }`}
          >
            超量程 {overFullScale.toLocaleString("en-US")}
          </span>
        ) : (
          <span className="muted">无超量程</span>
        )}
      </div>
    </div>
  );
}
