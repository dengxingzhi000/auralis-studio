/**
 * 全局搜索面板的条目（纯函数）。
 *
 * ## 为什么从 `App.tsx` 里搬出来
 *
 * 这份清单有 100 多行��原���只是躺在 `useMemo` 里，既没法单独看、也没法
 * 单独测。搬出来之后它就是一段普通的「输入 → 输出」代码，条目长什么样、
 * 哪些是文件、哪些是操作，都可以用测试固定住。
 *
 * ## 边界
 *
 * 三组条目，全部是**已算好的东西**或纯界面动作：
 *
 * - 文件：重新分析（Rust 侧重跑一遍，不是前端算）；
 * - 峰值：把频率视口对准它（只是坐标换算）；
 * - 操作：打开、导出、复位缩放、切面板、开对话框。
 *
 * 没有任何一条会触发新的音频计算（ADR-018 规则 1）。
 */

import type { FileInfo, PeakInfo } from "./contracts";
import { fmtHz } from "./format";
import type { SearchableItem } from "./search";

/** 搜索面板的一条条目。 */
export interface PaletteItem extends SearchableItem {
  /** 唯一标识。 */
  id: string;
  /** 执行动作。 */
  run: () => void;
}

/** 组装条目所需的全部输入。 */
export interface PaletteSource {
  /** 已打开过的文件。 */
  files: FileInfo[];
  /** 当前显示里的峰值。 */
  peaks: PeakInfo[];
  /** 打开文件。 */
  onOpenFile: () => void;
  /** 重新分析某个已打开过的文件。 */
  onOpenFileFor: (path: string) => void;
  /** 导出当前分析结果。 */
  onExport: () => void;
  /** 生成并分析合成信号。 */
  onRunSynthetic: () => void;
  /** 把频率视口对准某个峰值。 */
  onFocusPeak: (peak: PeakInfo) => void;
  /** 复位频率缩放。 */
  onResetZoom: () => void;
  /** 显示 / 隐藏资源栏。 */
  onToggleLeft: () => void;
  /** 显示 / 隐藏检查器。 */
  onToggleRight: () => void;
  /** 打开偏好设置。 */
  onOpenPrefs: () => void;
  /** 打开帮助。 */
  onOpenHelp: () => void;
  /** 打开关于。 */
  onOpenAbout: () => void;
}

/**
 * 组装搜索面板的条目。
 *
 * 顺序即面板里的分组顺序：文件 → 峰值 → 操作。
 */
export function buildPaletteItems(source: PaletteSource): PaletteItem[] {
  const items: PaletteItem[] = [];

  for (const f of source.files) {
    items.push({
      id: `file:${f.path}`,
      group: "文件",
      label: f.fileName,
      hint: `${f.containerName} · ${f.sampleRate.toLocaleString("en-US")} Hz`,
      keywords: f.path,
      run: () => void source.onOpenFileFor(f.path),
    });
  }

  for (const [i, p] of source.peaks.entries()) {
    items.push({
      id: `peak:${i}`,
      group: "峰值",
      label: `${fmtHz(p.frequencyHz)} Hz`,
      hint: `${p.amplitudeDbfs.toFixed(2)} dBFS${p.estimated ? "（估算）" : ""}`,
      keywords: "peak 峰值 频率 frequency",
      run: () => source.onFocusPeak(p),
    });
  }

  items.push(
    {
      id: "action:open",
      group: "操作",
      label: "打开文件…",
      hint: "Ctrl+O",
      keywords: "open 打开 文件",
      run: () => void source.onOpenFile(),
    },
    {
      id: "action:export",
      group: "操作",
      label: "导出分析结果…",
      hint: "CSV / JSON",
      keywords: "export 导出 保存",
      run: () => void source.onExport(),
    },
    {
      id: "action:synthetic",
      group: "操作",
      label: "生成并分析合成信号",
      keywords: "synthetic 合成 正弦",
      run: () => void source.onRunSynthetic(),
    },
    {
      id: "action:reset-zoom",
      group: "操作",
      label: "复位频率缩放",
      hint: "全带宽",
      keywords: "reset zoom 复位 缩放",
      run: () => source.onResetZoom(),
    },
    {
      id: "action:toggle-left",
      group: "操作",
      label: "显示 / 隐藏资源栏",
      keywords: "toggle 资源栏 侧栏 panel",
      run: () => source.onToggleLeft(),
    },
    {
      id: "action:toggle-right",
      group: "操作",
      label: "显示 / 隐藏检查器",
      keywords: "toggle 检查器 侧栏 panel",
      run: () => source.onToggleRight(),
    },
    {
      id: "action:prefs",
      group: "操作",
      label: "偏好设置…",
      hint: "Ctrl+,",
      keywords: "preferences 偏好 设置 settings",
      run: () => source.onOpenPrefs(),
    },
    {
      id: "action:help",
      group: "操作",
      label: "帮助与快捷键",
      hint: "F1",
      keywords: "help 帮助 快捷键 shortcut",
      run: () => source.onOpenHelp(),
    },
    {
      id: "action:about",
      group: "操作",
      label: "关于 Auralis Studio",
      keywords: "about 关于 版本 version",
      run: () => source.onOpenAbout(),
    },
  );

  return items;
}
