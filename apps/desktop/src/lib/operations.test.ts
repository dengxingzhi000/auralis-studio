/**
 * 全局操作（分析 / 导出）的忙碌策略测试。
 *
 * ## 为什么单独测这个
 *
 * 分析和导出曾经共用一个 `busy` 布尔值：导出期间「打开文件」会显示
 * 「分析中…」，拖放/搜索入口也会以「已有分析在进行中」拒绝。这类缺陷
 * 在类型上完全合法，只有把**策略**从组件里拿出来用测试固定住，才不会
 * 在下次加按钮时又退回去。
 *
 * 这里只测纯函数：不渲染组件、不碰 DOM，与 `prefs.test.ts` 同一口径。
 */

import { describe, expect, it } from "vitest";

import {
  canLoadSpectrogramWindow,
  canStartAnalysis,
  canStartExport,
  exportLabel,
  openFileLabel,
  type OperationState,
} from "./operations";

const idle: OperationState = { analyzing: false, exporting: false };
const analyzing: OperationState = { analyzing: true, exporting: false };
const exporting: OperationState = { analyzing: false, exporting: true };

describe("操作文案", () => {
  it("只有文件分析会把「打开文件」改成「分析中…」", () => {
    expect(openFileLabel(analyzing)).toBe("分析中…");
    expect(openFileLabel(exporting)).toBe("打开文件…");
    expect(openFileLabel(idle)).toBe("打开文件…");
  });

  it("只有导出会把「导出」改成「导出中…」", () => {
    expect(exportLabel(exporting)).toBe("导出中…");
    expect(exportLabel(analyzing)).toBe("导出…");
    expect(exportLabel(idle)).toBe("导出…");
  });
});

describe("操作闸门", () => {
  it("导出期间允许开始新分析（拖放 / 搜索的第二、三条入口）", () => {
    expect(canStartAnalysis(exporting)).toBe(true);
    expect(canStartAnalysis(idle)).toBe(true);
  });

  it("分析进行中拒绝第二条分析", () => {
    expect(canStartAnalysis(analyzing)).toBe(false);
  });

  it("导出期间仍可加载声谱图视口", () => {
    expect(canLoadSpectrogramWindow(exporting)).toBe(true);
  });

  it("分析期间不加载声谱图视口（矩阵即将被替换）", () => {
    expect(canLoadSpectrogramWindow(analyzing)).toBe(false);
  });

  it("导出不可重入，分析进行中也不导出", () => {
    expect(canStartExport(idle)).toBe(true);
    expect(canStartExport(exporting)).toBe(false);
    expect(canStartExport(analyzing)).toBe(false);
  });
});
