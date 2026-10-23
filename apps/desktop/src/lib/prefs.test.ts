/**
 * 偏好存储的测试。
 *
 * 重点是**坏数据**：`localStorage` 里的内容可能来自旧版本、可能被人手工改过、
 * 可能写到一半被截断。任何一种情况下界面都必须回到默认值，而不是崩掉，
 * 更不能出现「改了没反应」——那是两处真值源打架，不是数据坏了。
 *
 * 这里用内存版 localStorage 替身：测试之间完全隔离，也不会污染真实浏览器。
 */

import { beforeEach, describe, expect, it } from "vitest";

import {
  DEFAULT_PREFS,
  LEGACY_DISPLAY_KEY,
  LEGACY_VISIBILITY_KEY,
  PREFS_KEY,
  SESSION_KEY,
  WIDTHS_KEY,
  clearPrefs,
  clearSession,
  parsePrefs,
  readPrefs,
  resetLayout,
  writePrefs,
} from "./prefs";

/** 最小可用的 localStorage 替身。 */
class MemStorage {
  private map = new Map<string, string>();
  getItem(key: string): string | null {
    return this.map.get(key) ?? null;
  }
  setItem(key: string, value: string): void {
    this.map.set(key, value);
  }
  removeItem(key: string): void {
    this.map.delete(key);
  }
  clear(): void {
    this.map.clear();
  }
  key(index: number): string | null {
    return [...this.map.keys()][index] ?? null;
  }
  get length(): number {
    return this.map.size;
  }
}

beforeEach(() => {
  Object.defineProperty(globalThis, "localStorage", {
    value: new MemStorage(),
    configurable: true,
    writable: true,
  });
});

describe("parsePrefs", () => {
  it("拒绝 null", () => {
    expect(parsePrefs(null)).toBeNull();
  });

  it("拒绝缺字段的对象", () => {
    expect(parsePrefs({})).toBeNull();
  });

  it("拒绝越界的 dB 量程索引", () => {
    expect(parsePrefs({ ...DEFAULT_PREFS, dbRange: 99 })).toBeNull();
  });

  it("拒绝未知的频谱形态", () => {
    expect(parsePrefs({ ...DEFAULT_PREFS, spectrumMode: "bogus" })).toBeNull();
  });

  it("拒绝不在候选内的包络点数", () => {
    expect(parsePrefs({ ...DEFAULT_PREFS, envelopePoints: 777 })).toBeNull();
  });

  it("接受完整的默认偏好", () => {
    expect(parsePrefs(DEFAULT_PREFS)).toEqual(DEFAULT_PREFS);
  });
});

describe("readPrefs", () => {
  it("没有任何存储时返回默认偏好", () => {
    expect(readPrefs()).toEqual(DEFAULT_PREFS);
  });

  it("从旧的显示偏好键迁移", () => {
    localStorage.setItem(
      LEGACY_DISPLAY_KEY,
      JSON.stringify({
        mode: "line",
        dbRange: 1,
        channelLayout: "overlay",
      }),
    );
    const prefs = readPrefs();
    expect(prefs.spectrumMode).toBe("line");
    expect(prefs.dbRange).toBe(1);
    expect(prefs.channelLayout).toBe("overlay");
    // 旧键里没有的字段回到默认值
    expect(prefs.envelopePoints).toBe(DEFAULT_PREFS.envelopePoints);
  });

  it("从旧的可见性键迁移", () => {
    localStorage.setItem(
      LEGACY_VISIBILITY_KEY,
      JSON.stringify({ left: false, right: true }),
    );
    const prefs = readPrefs();
    expect(prefs.showLeft).toBe(false);
    expect(prefs.showRight).toBe(true);
  });

  it("新旧键同时存在时以新键为准", () => {
    localStorage.setItem(PREFS_KEY, JSON.stringify(DEFAULT_PREFS));
    localStorage.setItem(
      LEGACY_DISPLAY_KEY,
      JSON.stringify({ mode: "line", dbRange: 2, channelLayout: "overlay" }),
    );
    expect(readPrefs().spectrumMode).toBe(DEFAULT_PREFS.spectrumMode);
  });

  it("迁移后把结果写回新键", () => {
    localStorage.setItem(
      LEGACY_DISPLAY_KEY,
      JSON.stringify({
        mode: "line",
        dbRange: 1,
        channelLayout: "overlay",
      }),
    );
    readPrefs();
    const written = localStorage.getItem(PREFS_KEY);
    expect(written).not.toBeNull();
    expect(JSON.parse(written ?? "{}")).toMatchObject({
      spectrumMode: "line",
      dbRange: 1,
    });
  });
});

describe("writePrefs", () => {
  it("写进去能被读回来", () => {
    const prefs = { ...DEFAULT_PREFS, dbRange: 2, envelopePoints: 4096 };
    writePrefs(prefs);
    expect(readPrefs()).toEqual(prefs);
  });
});

describe("清除", () => {
  it("clearPrefs 只删偏好键，保留会话与布局", () => {
    writePrefs(DEFAULT_PREFS);
    localStorage.setItem(SESSION_KEY, JSON.stringify({ path: "C:/a.wav" }));
    localStorage.setItem(WIDTHS_KEY, JSON.stringify({ left: 200, right: 300 }));

    clearPrefs();

    expect(localStorage.getItem(PREFS_KEY)).toBeNull();
    expect(localStorage.getItem(SESSION_KEY)).not.toBeNull();
    expect(localStorage.getItem(WIDTHS_KEY)).not.toBeNull();
  });

  it("resetLayout 删可见性与宽度，保留偏好与会话", () => {
    writePrefs(DEFAULT_PREFS);
    localStorage.setItem(SESSION_KEY, JSON.stringify({ path: "C:/a.wav" }));
    localStorage.setItem(LEGACY_VISIBILITY_KEY, JSON.stringify({ left: true, right: true }));
    localStorage.setItem(WIDTHS_KEY, JSON.stringify({ left: 200, right: 300 }));

    resetLayout();

    expect(localStorage.getItem(LEGACY_VISIBILITY_KEY)).toBeNull();
    expect(localStorage.getItem(WIDTHS_KEY)).toBeNull();
    expect(localStorage.getItem(PREFS_KEY)).not.toBeNull();
    expect(localStorage.getItem(SESSION_KEY)).not.toBeNull();
  });

  it("clearSession 只删会话键", () => {
    writePrefs(DEFAULT_PREFS);
    localStorage.setItem(SESSION_KEY, JSON.stringify({ path: "C:/a.wav" }));

    clearSession();

    expect(localStorage.getItem(SESSION_KEY)).toBeNull();
    expect(localStorage.getItem(PREFS_KEY)).not.toBeNull();
  });
});
