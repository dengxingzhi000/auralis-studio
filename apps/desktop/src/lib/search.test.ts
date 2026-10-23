/**
 * 命令面板匹配逻辑的测试。
 *
 * 搜索只做**字符串匹配**：不解析音频、不做数值换算。峰值频率与电平
 * 在 Rust 侧已经算好，这里只按用户输入筛选。
 *
 * 匹配规则必须明确，否则用户会以为「搜不到」是程序坏了：
 *   - 大小写不敏感；
 *   - 多个词是 **AND** 语义（都命中才算命中）；
 *   - 分组名、标签、右侧说明、隐藏关键词都参与匹配。
 */

import { describe, expect, it } from "vitest";

import { matchItems, type SearchableItem } from "./search";

const ITEMS: SearchableItem[] = [
  { group: "文件", label: "tone.wav", hint: "WAV · 44,100 Hz" },
  { group: "文件", label: "kick.flac", hint: "FLAC · 48,000 Hz" },
  { group: "峰值", label: "1000 Hz", hint: "-9.18 dBFS", keywords: "peak 频率" },
  { group: "操作", label: "打开文件", keywords: "open load" },
];

describe("matchItems", () => {
  it("空查询返回全部", () => {
    expect(matchItems(ITEMS, "")).toHaveLength(ITEMS.length);
  });

  it("纯空白视为空查询", () => {
    expect(matchItems(ITEMS, "   ")).toHaveLength(ITEMS.length);
  });

  it("大小写不敏感", () => {
    const hits = matchItems(ITEMS, "TONE");
    expect(hits).toHaveLength(1);
    expect(hits[0]?.label).toBe("tone.wav");
  });

  it("多个词是 AND 语义", () => {
    expect(matchItems(ITEMS, "tone wav")).toHaveLength(1);
    expect(matchItems(ITEMS, "tone flac")).toHaveLength(0);
  });

  it("隐藏关键词也参与匹配", () => {
    expect(matchItems(ITEMS, "频率")).toHaveLength(1);
    expect(matchItems(ITEMS, "open")).toHaveLength(1);
  });

  it("分组名参与匹配", () => {
    const hits = matchItems(ITEMS, "操作");
    expect(hits).toHaveLength(1);
    expect(hits[0]?.label).toBe("打开文件");
  });

  it("没有命中时返回空数组", () => {
    expect(matchItems(ITEMS, "不存在的词")).toEqual([]);
  });

  it("受上限限制", () => {
    expect(matchItems(ITEMS, "", 2)).toHaveLength(2);
  });

  it("保持输入顺序", () => {
    expect(matchItems(ITEMS, "").map((i) => i.label)).toEqual(
      ITEMS.map((i) => i.label),
    );
  });
});
