/**
 * 声谱图频率视口换算的测试。
 *
 * 这些函数是**纯坐标换算**：把用户选的频率区间换算成矩阵里的 bin 下标。
 * 它们不做任何音频计算——矩阵里的 dB 值是 Rust 算好的，这里只决定
 * 「画哪几行」。但正因为只是坐标换算，错了不会有任何报错提示：
 * 画面照样渲染，只是频率对不上。所以这些边界必须钉死。
 *
 * 矩阵是 **512 个 bin**（Rust 侧 `SPECTROGRAM_BINS = 512`），
 * 不是 1024。bin 宽 = 奈奎斯特 / bin 数。
 */

import { describe, expect, it } from "vitest";

import {
  binRangeForView,
  clampViewRange,
  frequencyTicks,
} from "./spectrogram-viewport";

describe("clampViewRange", () => {
  it("null 表示未缩放，原样返回", () => {
    expect(clampViewRange(null, 24000)).toBeNull();
  });

  it("区间内的范围原样返回", () => {
    expect(clampViewRange([1000, 2000], 24000)).toEqual([1000, 2000]);
  });

  it("下界越界时收到 0", () => {
    expect(clampViewRange([-500, 2000], 24000)).toEqual([0, 2000]);
  });

  it("上界越界时收到奈奎斯特", () => {
    expect(clampViewRange([20000, 30000], 24000)).toEqual([20000, 24000]);
  });

  it("整体超出奈奎斯特时收到全带宽", () => {
    expect(clampViewRange([0, 50000], 24000)).toEqual([0, 24000]);
  });

  it("下界大于上界是非法区间", () => {
    expect(clampViewRange([2000, 1000], 24000)).toBeNull();
  });

  it("窄于 1 Hz 的区间视为未缩放", () => {
    expect(clampViewRange([1000, 1000.2], 24000)).toBeNull();
  });

  it("非有限值是非法区间", () => {
    expect(clampViewRange([NaN, 2000], 24000)).toBeNull();
  });

  it("钳位后区间为空时视为未缩放", () => {
    expect(clampViewRange([25000, 30000], 24000)).toBeNull();
  });
});

describe("binRangeForView", () => {
  it("未缩放时给出全部 bin", () => {
    expect(binRangeForView(null, 22050, 512)).toEqual({ first: 0, last: 511 });
  });

  it("全带宽区间给出全部 bin", () => {
    expect(binRangeForView([0, 22050], 22050, 512)).toEqual({
      first: 0,
      last: 511,
    });
  });

  it("半带宽给出前一半 bin", () => {
    expect(binRangeForView([0, 11025], 22050, 512)).toEqual({
      first: 0,
      last: 255,
    });
  });

  it("窄区间落在预期的 bin 上", () => {
    const range = binRangeForView([1000, 2000], 22050, 512);
    expect(range.first).toBeGreaterThanOrEqual(23);
    expect(range.last).toBeLessThanOrEqual(46);
    expect(range.last).toBeGreaterThanOrEqual(range.first);
  });

  it("超出奈奎斯特时至少给出一个 bin", () => {
    const range = binRangeForView([22000, 24000], 22050, 512);
    expect(range.last).toBeGreaterThanOrEqual(range.first);
    expect(range.last).toBeLessThanOrEqual(511);
  });

  it("bin 数为 0 时不产生负下标", () => {
    expect(binRangeForView(null, 22050, 0)).toEqual({ first: 0, last: 0 });
  });
});

describe("frequencyTicks", () => {
  it("未缩放时按奈奎斯特等分", () => {
    expect(frequencyTicks(null, 24000, 4)).toEqual([
      0, 6000, 12000, 18000, 24000,
    ]);
  });

  it("缩放后在可见区间内等分", () => {
    expect(frequencyTicks([1000, 2000], 24000, 2)).toEqual([1000, 1500, 2000]);
  });
});
