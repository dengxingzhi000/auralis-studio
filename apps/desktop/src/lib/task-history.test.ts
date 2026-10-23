/**
 * 「最近完成」历史记录的测试。
 *
 * 这个列表是**前端**记的：Rust 侧只暴露运行中的任务，结束状态只有
 * 发起方（前端自己持有的 promise）知道。因此它必须是一个纯函数，
 * 逻辑简单到一眼能验对 —— 少一条、重复一条、顺序错了，用户都会
 * 以为「这个分析没跑过」。
 */

import { describe, expect, it } from "vitest";

import { pushHistory, type HistoryEntry } from "./task-history";

function entry(id: string, over: Partial<HistoryEntry> = {}): HistoryEntry {
  return {
    id,
    kind: "analyze",
    label: `${id}.wav`,
    outcome: "succeeded",
    elapsedMs: 100,
    endedAt: 1_700_000_000_000,
    ...over,
  };
}

describe("pushHistory", () => {
  it("空列表时加入一条", () => {
    const e = entry("a");
    expect(pushHistory([], e)).toEqual([e]);
  });

  it("最新的排在最前", () => {
    const a = entry("a");
    const b = entry("b");
    expect(pushHistory([a], b)).toEqual([b, a]);
  });

  it("同 id 覆盖而不是重复", () => {
    const a = entry("a");
    const updated = entry("a", { outcome: "failed" });
    const result = pushHistory([a], updated);
    expect(result).toHaveLength(1);
    expect(result[0]?.outcome).toBe("failed");
  });

  it("超出上限时截掉最旧的", () => {
    let list: HistoryEntry[] = [];
    for (const id of ["a", "b", "c", "d", "e", "f", "g", "h"]) {
      list = pushHistory(list, entry(id));
    }
    expect(list).toHaveLength(8);
    expect(list[0]?.id).toBe("h");
    expect(list[7]?.id).toBe("a");
  });

  it("可以自定义上限", () => {
    const list = pushHistory([entry("a"), entry("b")], entry("c"), 1);
    expect(list).toHaveLength(1);
    expect(list[0]?.id).toBe("c");
  });

  it("不修改传入的数组", () => {
    const original = [entry("a")];
    const snapshot = [...original];
    pushHistory(original, entry("b"));
    expect(original).toEqual(snapshot);
  });
});
