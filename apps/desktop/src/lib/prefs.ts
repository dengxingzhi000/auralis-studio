/**
 * 统一偏好存储（M5 之后的「偏好设置」对话框）。
 *
 * ## 为什么要有这一层
 *
 * 界面上散落着好几处偏好：显示形态、dB 量程、声道排布、两侧栏可见性。
 * 顶栏的复选框、右侧检查器的「显示」分区、以及新的偏好设置对话框
 * 都要读写它们。**如果它们各有各的真值源，用户改了一处、另一处盖回去，
 * 表现就是「改了没反应」** —— 这是最让人困惑的一类界面缺陷。
 *
 * 所以：一份 `Prefs`、一个存储键、一次读写。别的模块只通过
 * `readPrefs` / `writePrefs` 访问。
 *
 * ## 迁移
 *
 * 早期版本把显示偏好存在 `auralis.display.v1`、侧栏可见性存在
 * `auralis.workspace.visibility.v1`。新键 `auralis.prefs.v1` 首次读取时
 * 会把这两处的值合并进来并写回新键 —— 用户已有的设置不能因为换了存储
 * 位置而丢失。旧键**不删**：删键是不可逆的，万一还有别的读取方就麻烦了。
 *
 * ## 校验口径
 *
 * 任一字段缺失或非法 → **整条丢弃**，回到默认值，不猜用户意图。
 * 与 `App.tsx` 里既有的 `readDisplayPrefs` / `readVisibility` 一致。
 *
 * 存储读写全部包 try/catch：`localStorage` 在隐私模式或配额满时会抛错，
 * 偏好丢了不影响正确性。
 */

import { asBoolean, asNumberInRange, readStoredJson, writeStoredJson } from "./persist";

/** dB 量程预设。索引即 `Prefs.dbRange` 的取值。 */
export const DB_RANGES = [
  { label: "全量程 −140…0", min: -140, max: 0 },
  { label: "常用 −100…0", min: -100, max: 0 },
  { label: "细节 −80…−10", min: -80, max: -10 },
  { label: "底噪 −140…−40", min: -140, max: -40 },
] as const;

/** `dbRange` 的合法下标范围。 */
const DB_RANGE_MIN = 0;
const DB_RANGE_MAX = DB_RANGES.length - 1;

/**
 * 校验 dB 量程索引：必须是**整数**且落在预设范围内。
 *
 * 只做范围检查会放过 `1.5`：`DB_RANGES[1.5]` 是 `undefined`，下拉框
 * 匹配不到任何选项，电平表则静默回落到全量程 —— 用户的选择「消失」了，
 * 却不会有任何报错。索引必须是预设数组下标的整数。
 */
function asDbRangeIndex(value: unknown): number | null {
  const n = asNumberInRange(value, DB_RANGE_MIN, DB_RANGE_MAX);
  return n != null && Number.isInteger(n) ? n : null;
}

/** 波形包络点数的候选值。 */
export const ENVELOPE_POINTS: readonly number[] = [512, 1024, 2048, 4096];

/** 统一偏好存储键。 */
export const PREFS_KEY = "auralis.prefs.v1";

/** 旧的显示偏好键（迁移来源）。 */
export const LEGACY_DISPLAY_KEY = "auralis.display.v1";
/** 旧的侧栏可见性键（迁移来源）。 */
export const LEGACY_VISIBILITY_KEY = "auralis.workspace.visibility.v1";
/** 会话恢复记录（上次打开的文件）。 */
export const SESSION_KEY = "auralis.session.v1";
/** 侧栏宽度（由 `Workspace` 组件自持读写）。 */
export const WIDTHS_KEY = "auralis.workspace.widths.v1";

/** 全部偏好。字段缺一个就整条作废，所以这里没有可选字段。 */
export interface Prefs {
  /** 频谱形态：填充（面积）/ 曲线（声线）。 */
  spectrumMode: "area" | "line";
  /** dB 量程预设的索引。 */
  dbRange: number;
  /** 声道排布：分道 / 叠加。 */
  channelLayout: "lanes" | "overlay";
  /** 左侧资源栏是否显示。 */
  showLeft: boolean;
  /** 右侧检查器是否显示。 */
  showRight: boolean;
  /** 文件分析的波形包络点数。 */
  envelopePoints: number;
  /** 启动时自动恢复上次打开的文件。 */
  restoreSession: boolean;
}

/** 默认偏好。 */
export const DEFAULT_PREFS: Prefs = {
  spectrumMode: "area",
  dbRange: 0,
  channelLayout: "lanes",
  showLeft: true,
  showRight: true,
  envelopePoints: 2048,
  restoreSession: true,
};

/* ------------------------------------------------------------ 校验 */

/**
 * 校验一份偏好。
 *
 * 任一字段缺失或非法即返回 `null`（整条丢弃）。半份偏好比没有偏好更危险：
 * 补上默认值等于替用户做了决定。
 */
export function parsePrefs(value: unknown): Prefs | null {
  if (typeof value !== "object" || value === null) return null;
  const rec = value as Record<string, unknown>;

  const spectrumMode =
    rec.spectrumMode === "line" || rec.spectrumMode === "area"
      ? rec.spectrumMode
      : null;
  const dbRange = asDbRangeIndex(rec.dbRange);
  const channelLayout =
    rec.channelLayout === "lanes" || rec.channelLayout === "overlay"
      ? rec.channelLayout
      : null;
  const showLeft = asBoolean(rec.showLeft);
  const showRight = asBoolean(rec.showRight);
  const envelopePoints =
    typeof rec.envelopePoints === "number" &&
    ENVELOPE_POINTS.includes(rec.envelopePoints)
      ? rec.envelopePoints
      : null;
  const restoreSession = asBoolean(rec.restoreSession);

  if (
    spectrumMode == null ||
    dbRange == null ||
    channelLayout == null ||
    showLeft == null ||
    showRight == null ||
    envelopePoints == null ||
    restoreSession == null
  ) {
    return null;
  }

  return {
    spectrumMode,
    dbRange,
    channelLayout,
    showLeft,
    showRight,
    envelopePoints,
    restoreSession,
  };
}

/** 旧显示偏好的形状。 */
interface LegacyDisplay {
  mode: "area" | "line";
  dbRange: number;
  channelLayout: "lanes" | "overlay";
}

function parseLegacyDisplay(value: unknown): LegacyDisplay | null {
  if (typeof value !== "object" || value === null) return null;
  const rec = value as Record<string, unknown>;
  const mode = rec.mode === "line" || rec.mode === "area" ? rec.mode : null;
  const dbRange = asDbRangeIndex(rec.dbRange);
  const channelLayout =
    rec.channelLayout === "lanes" || rec.channelLayout === "overlay"
      ? rec.channelLayout
      : null;
  if (mode == null || dbRange == null || channelLayout == null) return null;
  return { mode, dbRange, channelLayout };
}

/** 旧侧栏可见性的形状。 */
interface LegacyVisibility {
  left: boolean;
  right: boolean;
}

function parseLegacyVisibility(value: unknown): LegacyVisibility | null {
  if (typeof value !== "object" || value === null) return null;
  const rec = value as Record<string, unknown>;
  const left = asBoolean(rec.left);
  const right = asBoolean(rec.right);
  if (left == null || right == null) return null;
  return { left, right };
}

/* ------------------------------------------------------------ 读写 */

/**
 * 读取当前偏好。
 *
 * 顺序：新键 → 旧键迁移 → 默认值。迁移时会把合并结果写回新键，
 * 之后只读写新键。
 */
export function readPrefs(): Prefs {
  const current = readStoredJson(PREFS_KEY, parsePrefs);
  if (current) return current;

  const display = readStoredJson(LEGACY_DISPLAY_KEY, parseLegacyDisplay);
  const visibility = readStoredJson(
    LEGACY_VISIBILITY_KEY,
    parseLegacyVisibility,
  );
  if (!display && !visibility) return DEFAULT_PREFS;

  const merged: Prefs = {
    ...DEFAULT_PREFS,
    ...(display
      ? {
          spectrumMode: display.mode,
          dbRange: display.dbRange,
          channelLayout: display.channelLayout,
        }
      : {}),
    ...(visibility
      ? { showLeft: visibility.left, showRight: visibility.right }
      : {}),
  };
  writePrefs(merged);
  return merged;
}

/** 写入偏好。 */
export function writePrefs(prefs: Prefs): void {
  writeStoredJson(PREFS_KEY, prefs);
}

/** 删掉一个存储键；失败时静默忽略。 */
function removeStoredKey(key: string): void {
  try {
    localStorage.removeItem(key);
  } catch {
    /* 隐私模式 / 配额满：删不掉不影响正确性 */
  }
}

/**
 * 重置显示与分析偏好。
 *
 * 只删偏好相关的键；会话记录与面板宽度各归各的，有各自的清除入口。
 *
 * **必须把两个迁移来源一起删掉**：迁移刻意不删旧键（不可逆），所以
 * 旧版本用户的 `LEGACY_VISIBILITY_KEY` 会一直留在存储里。只删新键的话，
 * 紧接着的 `readPrefs()` 又会从旧键迁移一次并写回新键 —— 表现为
 * 「重置了但设置还在」，而且刚删掉的键又出现了。
 */
export function clearPrefs(): void {
  removeStoredKey(PREFS_KEY);
  removeStoredKey(LEGACY_DISPLAY_KEY);
  removeStoredKey(LEGACY_VISIBILITY_KEY);
}

/** 重置面板布局（侧栏可见性与宽度）。 */
export function resetLayout(): void {
  removeStoredKey(LEGACY_VISIBILITY_KEY);
  removeStoredKey(WIDTHS_KEY);
}

/** 清除会话记录（下次启动不再自动恢复上次文件）。 */
export function clearSession(): void {
  removeStoredKey(SESSION_KEY);
}
