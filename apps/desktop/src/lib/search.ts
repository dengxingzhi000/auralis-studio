/**
 * 命令面板的匹配逻辑（纯函数）。
 *
 * 搜索只做**字符串匹配**：不解析音频、不做任何数值换算。峰值频率与
 * 电平在 Rust 侧已经算好，这里只按用户输入筛选（ADR-018 规则 1）。
 *
 * 规则（与界面提示一致，避免用户以为「搜不到」是程序坏了）：
 *
 * - 大小写不敏感；
 * - 多个词是 **AND** 语义：每个词都要命中；
 * - 参与匹配的文本 = 分组名 + 标签 + 右侧说明 + 隐藏关键词。
 */

/** 可被搜索的条目。 */
export interface SearchableItem {
  /** 分组标题，例如「文件」「峰值」「操作」 */
  group: string;
  /** 主标签 */
  label: string;
  /** 右侧说明或数值 */
  hint?: string;
  /** 额外的可搜索文本（不显示） */
  keywords?: string;
}

/** 结果条数上限。太多会让面板失去「一按就到」的意义。 */
export const SEARCH_LIMIT = 60;

/**
 * 按查询筛选条目。
 *
 * 空查询（去掉空白后为空）返回全部（仍受上限限制）。
 */
export function matchItems<T extends SearchableItem>(
  items: T[],
  query: string,
  limit: number = SEARCH_LIMIT,
): T[] {
  const terms = query
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean);

  if (terms.length === 0) return items.slice(0, limit);

  const matched = items.filter((item) => {
    const haystack =
      `${item.group} ${item.label} ${item.hint ?? ""} ${item.keywords ?? ""}`.toLowerCase();
    return terms.every((term) => haystack.includes(term));
  });

  return matched.slice(0, limit);
}
