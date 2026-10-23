/**
 * MetricsPanel 的渲染契约测试。
 *
 * 这个卡片最容易出的错不是「画得不好看」，而是**编造读数**：
 * 把 `null`（真静音 / 未统计）显示成 0，或者在「实时」徽标下继续显示
 * 已打开文件的旧读数。这两种情况在界面上都看起来很正常，所以用
 * `renderToStaticMarkup` 直接断言输出 HTML。
 *
 * 组件没有 hooks / DOM 依赖，因此不需要 jsdom：node 环境足够。
 */

import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type { DisplayData } from "@/lib/display";

import { MetricsPanel } from "./MetricsPanel";

function makeDisplay(over: Partial<DisplayData> = {}): DisplayData {
  return {
    db: null,
    startHz: 0,
    stepHz: 1,
    env: null,
    sampleRate: 48000,
    peakDbfs: -3,
    rmsDbfs: -12,
    crestDb: 9,
    dc: 0,
    outcome: "succeeded",
    elapsedMs: 5,
    bytes: 1000,
    peaks: [],
    fftSize: 4096,
    window: "hann",
    spectrogram: null,
    spectrogramFrames: 0,
    spectrogramBins: 0,
    spectrogramHopMs: 0,
    spectrogramStartSeconds: 0,
    peakPerChannel: [],
    overFullScaleSamples: 0,
    clippingRegions: 0,
    sanitizedSamples: 0,
    truncated: false,
    declaredDurationSeconds: 2,
    decodedDurationSeconds: 2,
    ...over,
  };
}

function render(
  display: DisplayData | null,
  live: { peakDbfs: number | null; rmsDbfs: number | null } | null,
): string {
  return renderToStaticMarkup(
    <MetricsPanel display={display} live={live} minDb={-140} maxDb={0} />,
  );
}

describe("MetricsPanel", () => {
  it("没有文件也没有实时输入时显示空状态", () => {
    const html = render(null, null);
    expect(html).toContain("打开一个音频文件");
    expect(html).not.toContain("metrics-grid");
  });

  it("静音文件把 Peak / RMS 显示成「—」而不是 0", () => {
    const html = render(
      makeDisplay({ peakDbfs: null, rmsDbfs: null, crestDb: null }),
      null,
    );
    expect(html).toMatch(/Peak<\/span><span class="num">—<\/span>/);
    expect(html).toMatch(/RMS<\/span><span class="num">—<\/span>/);
    expect(html).not.toContain("0.0000");
  });

  it("文件已检查且没有超量程时显示 0（区分「检查过」与「没检查」）", () => {
    const html = render(makeDisplay(), null);
    expect(html).toMatch(/超量程样本<\/span><span class="num">0<\/span>/);
    expect(html).toContain("无超量程");
  });

  it("截断文件说明分析覆盖的是实际存在的那一段", () => {
    const html = render(
      makeDisplay({
        truncated: true,
        declaredDurationSeconds: 2,
        decodedDurationSeconds: 1.5,
      }),
      null,
    );
    expect(html).toContain("不完整 · 声明 2.00 s，实际 1.50 s");
    expect(html).toContain("只分析了文件里实际存在的那一部分");
  });

  it("纯实时输入不把文件级数据质量显示成 0", () => {
    const html = render(null, { peakDbfs: -20, rmsDbfs: -25 });
    // 没有文件 → 三个计数都是「—」，不能声称「检查过，没发现问题」
    expect(html).toMatch(/超量程样本<\/span><span class="num">—<\/span>/);
    expect(html).toMatch(/平顶片段<\/span><span class="num">—<\/span>/);
    expect(html).toMatch(/清理样本<\/span><span class="num">—<\/span>/);
    expect(html).not.toContain("无超量程");
    expect(html).toContain("实时输入只统计电平");
  });

  it("实时采集时不显示已打开文件的逐声道峰值与超量程计数", () => {
    const html = render(
      makeDisplay({
        peakPerChannel: [-6, -6],
        overFullScaleSamples: 123,
        clippingRegions: 4,
      }),
      { peakDbfs: -20, rmsDbfs: -25 },
    );
    // 电平表应回落到单条实时读数，而不是文件的两个 CH
    expect(html).toContain("MIX");
    expect(html).not.toContain("CH1");
    // 文件的超量程计数不能挂在「实时」徽标下
    expect(html).not.toContain("超量程 123");
    expect(html).not.toContain("无超量程");
    expect(html).toContain("实时");
  });
});
