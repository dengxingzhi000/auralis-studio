/**
 * 全局操作（分析 / 导出）的忙碌状态与闸门策略。
 *
 * ## 为什么不共用一个 `busy`
 *
 * 文件分析、合成信号分析和导出是三条独立的操作。它们曾经共用一个
 * `busy` 布尔值，于是：
 *
 * - 导出期间顶栏「打开文件」显示成「分析中…」；
 * - 拖放 / 搜索入口被 `analyzePath` 以「已有分析在进行中」拒绝；
 * - 导出期间声谱图翻页被无谓地禁用。
 *
 * 这些都不是类型错误，而是**语义**错误。把判断集中在这里，组件只负责
 * 按结果渲染，并让 `operations.test.ts` 固定住行为。
 *
 * ## 为什么分析中不允许导出
 *
 * 导出会在 Rust 侧重跑一遍分析（`export_analysis`）。文件分析进行时，
 * `selected` / `display` 正在被替换，此时导出的对象与屏幕上的内容可能
 * 对不上。等分析结束再导出，语义最清楚。
 *
 * 反过来，导出不修改分析状态，因此**不阻止**新的分析，也不阻止
 * 声谱图视口加载。
 */

/** 当前正在运行的操作。 */
export interface OperationState {
  /** 文件或合成信号分析正在运行。 */
  analyzing: boolean;
  /** 导出正在运行。 */
  exporting: boolean;
}

export const IDLE_OPERATIONS: OperationState = {
  analyzing: false,
  exporting: false,
};

/** 「打开文件」按钮的文案：只有分析会改变它。 */
export function openFileLabel(state: OperationState): string {
  return state.analyzing ? "分析中…" : "打开文件…";
}

/** 「导出」按钮的文案：只有导出会改变它。 */
export function exportLabel(state: OperationState): string {
  return state.exporting ? "导出中…" : "导出…";
}

/**
 * 是否允许开始一次新分析（打开文件 / 拖放 / 搜索 / 侧栏文件项）。
 *
 * 只看分析自身：导出不修改分析状态，不应误伤第二、三条入口。
 */
export function canStartAnalysis(state: OperationState): boolean {
  return !state.analyzing;
}

/**
 * 是否允许开始一次导出。
 *
 * 导出自身不可重入；分析进行中也不导出（见文件头）。
 * 调用方还要检查是否已经选中文件并拿到显示数据。
 */
export function canStartExport(state: OperationState): boolean {
  return !state.analyzing && !state.exporting;
}

/**
 * 是否允许加载声谱图视口（翻页 / 平移）。
 *
 * 分析会整体替换矩阵，此时请求的结果会被覆盖，应当拒绝；导出不会。
 */
export function canLoadSpectrogramWindow(state: OperationState): boolean {
  return !state.analyzing;
}
