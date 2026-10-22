/**
 * 本地 UI 偏好持久化（M5）。
 *
 * 只存**界面状态**：面板宽度、面板可见性。不存分析结果，也不存任何
 * 音频数据 —— 那些属于 Rust 侧，且体积不可控。
 *
 * 读写都包在 try/catch 里：
 *   - `localStorage` 在隐私模式或配额满时会抛错；
 *   - 存的内容可能来自旧版本，JSON 解析失败必须回落默认值而不是崩掉界面。
 */

/** 读取并校验一个 JSON 值；缺失或损坏时返回 null。 */
export function readStoredJson<T>(
  key: string,
  validate: (value: unknown) => T | null,
): T | null {
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return null;
    return validate(JSON.parse(raw) as unknown);
  } catch {
    return null;
  }
}

/** 写入一个 JSON 值；失败时静默忽略（不影响主流程）。 */
export function writeStoredJson(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* 配额满 / 隐私模式：界面偏好丢了不影响正确性 */
  }
}

/** 校验一个数值区间内的字段。非有限值视为无效。 */
export function asNumberInRange(
  value: unknown,
  min: number,
  max: number,
): number | null {
  if (typeof value !== "number" || !Number.isFinite(value)) return null;
  if (value < min || value > max) return null;
  return value;
}

/** 校验布尔字段。 */
export function asBoolean(value: unknown): boolean | null {
  return typeof value === "boolean" ? value : null;
}
