/**
 * 导出分析结果（React 状态层）。
 *
 * ## 导出为什么单独成一层
 *
 * 导出是**第三条**入口：既不是分析，也不是查看。它有自己的忙碌位
 * （`exporting`），自己的闸门（分析进行中不导出），自己的失败处理。
 * 混在 `App.tsx` 里时它和文件分析共享一堆状态，读者要跨上百行才能
 * 看全一件事。
 *
 * ## 格式由扩展名决定
 *
 * 保存对话框里用户选了 `.csv` 还是 `.json`，Rust 侧会校验两者一致 ——
 * 这样只有一个按钮、没有额外的格式开关。用户取消保存不是错误。
 */

import { useCallback, useState, type RefObject } from "react";

import { exportAnalysis, pickExportDestination } from "./ipc";
import { describeError, type UiError } from "./errors";
import { canStartExport } from "./operations";
import type { FileInfo } from "./contracts";

/** 导出需要的输入。 */
export interface ExportOptions {
  /** 当前选中的文件；`null` = 没有可导出的对象。 */
  selected: FileInfo | null;
  /** 波形包络点数（与导出内容一致）。 */
  envelopePoints: number;
  /** 分析是否在进行中（同步闸门）。 */
  analyzing: RefObject<boolean>;
  /** 导出是否正在进行中（同步闸门，防重入）。 */
  exporting: RefObject<boolean>;
  /** 短消息提示；传 `null` 表示清掉当前提示。 */
  setNotice: (message: string | null) => void;
  /** 错误提示；传 `null` 表示清掉当前错误。 */
  setError: (error: UiError | null) => void;
}

/** 导出的状态与操作。 */
export interface ExportController {
  /** 导出是否正在进行。 */
  exporting: boolean;
  /** 导出当前文件的分析结果。 */
  onExport: () => void;
}

/**
 * 导出分析结果。
 *
 * 导出在 Rust 侧重跑一遍分析（`export_analysis`），不传前端的视图数据：
 * 前端手里只有降采样后的东西，而且要跨越 IPC 传回几百 KB~数 MB。
 * 代价是一次解码，换来「导出内容 = 显示内容 + 可追溯」。
 */
export function useExport(options: ExportOptions): ExportController {
  const {
    selected,
    envelopePoints,
    analyzing,
    exporting,
    setNotice,
    setError,
  } = options;
  const [isExporting, setIsExporting] = useState(false);

  const onExport = useCallback(async () => {
    if (!selected) return;
    // 分析进行中不导出（导出对象可能与屏幕内容对不上）；导出自身不可重入。
    if (!canStartExport({ analyzing: analyzing.current, exporting: exporting.current })) {
      setNotice(
        analyzing.current ? "分析进行中，完成后再导出" : "正在导出，请等待完成",
      );
      return;
    }
    exporting.current = true;
    setIsExporting(true);
    setError(null);
    setNotice(null);
    try {
      const base = selected.fileName.replace(/\.[^.]+$/, "");
      const picked = await pickExportDestination(`${base}.csv`);
      if (!picked) return;
      // 用户可能把扩展名删掉，补一个默认值而不是让 Rust 端报错
      const destination = /\.(csv|json)$/i.test(picked) ? picked : `${picked}.csv`;
      const format = destination.toLowerCase().endsWith(".json")
        ? ("json" as const)
        : ("csv" as const);

      const out = await exportAnalysis(
        selected.path,
        destination,
        format,
        envelopePoints,
      );
      setNotice(
        `已导出 ${out.format.toUpperCase()} · ${(out.bytesWritten / 1024).toFixed(1)} KiB → ${out.destination}`,
      );
    } catch (e) {
      setError(describeError(e, "导出失败"));
    } finally {
      exporting.current = false;
      setIsExporting(false);
    }
  }, [selected, envelopePoints, analyzing, exporting, setNotice, setError]);

  return { exporting: isExporting, onExport };
}
