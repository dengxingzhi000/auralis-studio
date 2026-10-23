/**
 * 把未知异常翻译成用户能看懂的界面错误。
 *
 * M1 阶段直接把 `String(e)` 铺在状态栏，结果是满屏 `TypeError: ...`：
 * 既看不出是什么操作失败，也无法定位。这里做最小翻译，未知错误原样保留
 * detail，不编造原因。
 */

export interface UiError {
  title: string;
  detail: string | null;
}

export function describeError(e: unknown, action: string): UiError {
  const raw = e instanceof Error ? `${e.name}: ${e.message}` : String(e);
  if (raw.includes("is not a multiple of 4")) {
    return { title: `${action}：后端返回的二进制载荷长度非法`, detail: raw };
  }
  if (raw.includes("not found") || raw.includes("No such file")) {
    return { title: `${action}：文件不存在或已被移动`, detail: raw };
  }
  if (raw.includes("Unsupported") || raw.includes("unsupported")) {
    return { title: `${action}：该格式尚未验证支持`, detail: raw };
  }
  // 截断与损坏要分开说：截断通常意味着文件没下完/拷贝不完整，重新获取即可；
  // 混在一句话里，用户会以为是格式不支持而白折腾。
  if (raw.includes("truncated")) {
    return { title: `${action}：文件不完整（已截断）`, detail: raw };
  }
  if (raw.includes("corrupt")) {
    return { title: `${action}：文件已损坏`, detail: raw };
  }
  return { title: `${action}（详见悬停提示）`, detail: raw };
}
