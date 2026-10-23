/**
 * 界面显示用的格式化函数。
 *
 * 只把**已经算好的**数值转成字符串，不做任何音频 / DSP 计算
 * （ADR-018 规则 1）。上游可能给出 NaN / Infinity（例如零点附近插值
 * 失败），因此每个函数都必须能安全处理非有限值。
 */

/**
 * 取路径的文件名部分；没有分隔符时原样返回。
 *
 * 只用于显示标签，不是解析路径 —— 真正的路径处理由 Rust 侧负责。
 */
export function fileNameOf(path: string): string {
  const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return cut >= 0 ? path.slice(cut + 1) : path;
}

/** 频率显示：12000 -> "12.0k"（保留两位小数） */
export function fmtHz(hz: number): string {
  // 上游可能给出 NaN / Infinity，这类值直接交给 toFixed 会抛错并让
  // 整个渲染树崩掉。
  if (!Number.isFinite(hz)) return "—";
  return hz >= 1000 ? `${(hz / 1000).toFixed(2)}k` : hz.toFixed(0);
}

/** 整数频率：12000 -> "12,000" */
export function fmtHzInt(hz: number): string {
  return Math.round(hz).toLocaleString("en-US");
}

/**
 * 缩放范围摘要：`null` 表示全带宽。
 *
 * 只格式化显示值，不做任何计算（ADR-018 规则 1）。
 */
export function fmtRange(r: [number, number] | null): string {
  if (!r) return "全带宽";
  return `${fmtHz(r[0])} – ${fmtHz(r[1])} Hz`;
}

/** 绝对时间：`m:ss.s`，用于声谱图视口范围。 */
export function fmtClock(sec: number): string {
  if (!Number.isFinite(sec) || sec < 0) return "—";
  const m = Math.floor(sec / 60);
  const s = sec - m * 60;
  return `${m}:${s.toFixed(1).padStart(4, "0")}`;
}
