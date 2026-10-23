/**
 * 会话恢复记录的解析。
 *
 * 存储里可能是上一版本留下的任意 JSON；只接受「有非空 path 字符串」
 * 这一种形状，其余一律当成没有记录。
 */

export interface SessionRecord {
  path: string;
}

export function readSession(value: unknown): SessionRecord | null {
  if (typeof value !== "object" || value === null) return null;
  const rec = value as Record<string, unknown>;
  return typeof rec.path === "string" && rec.path.length > 0
    ? { path: rec.path }
    : null;
}
