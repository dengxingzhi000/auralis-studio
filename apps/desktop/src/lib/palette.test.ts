import { describe, expect, it, vi } from "vitest";

import type { FileInfo, PeakInfo } from "./contracts";
import { buildPaletteItems, type PaletteSource } from "./palette";

function file(path: string, fileName: string): FileInfo {
  return {
    path,
    fileName,
    container: "wav",
    containerName: "WAV",
    sampleRate: 48000,
    channels: 2,
    durationSeconds: 12.5,
    bitDepth: 16,
    codec: "pcm",
  } as unknown as FileInfo;
}

function peak(frequencyHz: number, amplitudeDbfs: number, estimated = false): PeakInfo {
  return {
    frequencyHz,
    amplitudeDbfs,
    estimated,
    binFrequencyHz: frequencyHz,
  } as unknown as PeakInfo;
}

function source(overrides: Partial<PaletteSource> = {}): PaletteSource {
  return {
    files: [],
    peaks: [],
    onOpenFile: vi.fn(),
    onOpenFileFor: vi.fn(),
    onExport: vi.fn(),
    onRunSynthetic: vi.fn(),
    onFocusPeak: vi.fn(),
    onResetZoom: vi.fn(),
    onToggleLeft: vi.fn(),
    onToggleRight: vi.fn(),
    onOpenPrefs: vi.fn(),
    onOpenHelp: vi.fn(),
    onOpenAbout: vi.fn(),
    ...overrides,
  };
}

describe("buildPaletteItems", () => {
  it("总是给出全部操作条目", () => {
    const items = buildPaletteItems(source());
    const actions = items.filter((i) => i.group === "操作");
    expect(actions).toHaveLength(9);
    expect(actions.map((i) => i.id)).toContain("action:open");
    expect(actions.map((i) => i.id)).toContain("action:about");
  });

  it("文件条目带容器名与采样率", () => {
    const items = buildPaletteItems(
      source({ files: [file("C:/a.wav", "a.wav")] }),
    );
    const f = items.find((i) => i.id === "file:C:/a.wav");
    expect(f).toBeDefined();
    expect(f?.label).toBe("a.wav");
    expect(f?.hint).toContain("WAV");
    expect(f?.hint).toContain("48,000 Hz");
    // 路径也要能被搜到
    expect(f?.keywords).toBe("C:/a.wav");
  });

  it("文件条目执行时带上自己的路径", () => {
    const onOpenFileFor = vi.fn();
    const items = buildPaletteItems(
      source({
        files: [file("C:/a.wav", "a.wav"), file("C:/b.wav", "b.wav")],
        onOpenFileFor,
      }),
    );
    items.find((i) => i.id === "file:C:/b.wav")?.run();
    expect(onOpenFileFor).toHaveBeenCalledWith("C:/b.wav");
  });

  it("峰值条目显示频率与电平，估算值有标注", () => {
    const items = buildPaletteItems(
      source({ peaks: [peak(1000, -6.5), peak(2000, -12.25, true)] }),
    );
    const p0 = items.find((i) => i.id === "peak:0");
    const p1 = items.find((i) => i.id === "peak:1");
    expect(p0?.label).toBe("1.00k Hz");
    expect(p0?.hint).toContain("-6.50 dBFS");
    expect(p0?.hint).not.toContain("估算");
    expect(p1?.hint).toContain("估算");
  });

  it("峰值条目执行时把峰值对象传回去", () => {
    const onFocusPeak = vi.fn();
    const p = peak(3000, -3);
    const items = buildPaletteItems(source({ peaks: [p], onFocusPeak }));
    items.find((i) => i.id === "peak:0")?.run();
    expect(onFocusPeak).toHaveBeenCalledWith(p);
  });

  it("每个操作条目都接到对应的动作上", () => {
    const s = source();
    const items = buildPaletteItems(s);
    const byId = new Map(items.map((i) => [i.id, i]));

    byId.get("action:open")?.run();
    expect(s.onOpenFile).toHaveBeenCalled();
    byId.get("action:export")?.run();
    expect(s.onExport).toHaveBeenCalled();
    byId.get("action:synthetic")?.run();
    expect(s.onRunSynthetic).toHaveBeenCalled();
    byId.get("action:reset-zoom")?.run();
    expect(s.onResetZoom).toHaveBeenCalled();
    byId.get("action:toggle-left")?.run();
    expect(s.onToggleLeft).toHaveBeenCalled();
    byId.get("action:toggle-right")?.run();
    expect(s.onToggleRight).toHaveBeenCalled();
    byId.get("action:prefs")?.run();
    expect(s.onOpenPrefs).toHaveBeenCalled();
    byId.get("action:help")?.run();
    expect(s.onOpenHelp).toHaveBeenCalled();
    byId.get("action:about")?.run();
    expect(s.onOpenAbout).toHaveBeenCalled();
  });

  it("分组顺序是 文件 → 峰值 → 操作", () => {
    const items = buildPaletteItems(
      source({
        files: [file("C:/a.wav", "a.wav")],
        peaks: [peak(1000, -6)],
      }),
    );
    const groups = items.map((i) => i.group);
    const firstAction = groups.indexOf("操作");
    expect(groups.indexOf("文件")).toBe(0);
    expect(groups.indexOf("峰值")).toBe(1);
    expect(groups.slice(firstAction).every((g) => g === "操作")).toBe(true);
  });

  it("条目 id 不重复", () => {
    const items = buildPaletteItems(
      source({
        files: [file("C:/a.wav", "a.wav")],
        peaks: [peak(1000, -6), peak(2000, -9)],
      }),
    );
    const ids = items.map((i) => i.id);
    expect(new Set(ids).size).toBe(ids.length);
  });
});
