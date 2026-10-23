/**
 * 显示与分析偏好的 React 状态层。
 *
 * `prefs.ts` 负责**存储**（读、校验、迁移、落盘），本模块负责**状态**：
 * 把一份偏好变成组件能直接用的 state，并把改动写回存储。
 *
 * ## 为什么要单独一层
 *
 * 偏好散落在三处：顶栏的两个复选框、右侧检查器的「显示」分区、偏好设置
 * 对话框。三处各写各的 state 的话，用户改一处、另一处盖回去，表现就是
 * 「改了没反应」—— 这是最让人困惑的一类界面缺陷（`prefs.ts` 文件头有
 * 详细说明）。所以这里给出一份真值源，三处都从它读。
 *
 * ## 旧键为什么还在写
 *
 * 早期版本把显示偏好存在 `auralis.display.v1`、侧栏可见性存在
 * `auralis.workspace.visibility.v1`。`prefs.ts` 的迁移会读它们，但刻意
 * **不删**（删键不可逆，万一还有别的读取方）。这里继续写这两个键，
 * 是为了让降级到旧版本的用户仍然拿到自己原来的设置。
 */

import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";

import { writeStoredJson } from "./persist";
import {
  clearPrefs,
  clearSession,
  readPrefs,
  resetLayout,
  writePrefs,
  type Prefs,
} from "./prefs";
import type { ChannelLayout, SpectrumMode } from "./view";

/** 面板可见性的持久化键（旧版键，仍在写，见文件头）。 */
const VISIBILITY_KEY = "auralis.workspace.visibility.v1";
/** 显示偏好的持久化键（旧版键，仍在写，见文件头）。 */
const DISPLAY_KEY = "auralis.display.v1";

/** 偏好的状态与操作。 */
export interface PrefsController {
  /** 频谱形态：填充 / 曲线。 */
  spectrumMode: SpectrumMode;
  /** dB 量程预设的索引。 */
  dbRangeIdx: number;
  /** 声道排布：分道 / 叠加。 */
  channelLayout: ChannelLayout;
  /** 左侧资源栏是否显示。 */
  showLeft: boolean;
  /** 右侧检查器是否显示。 */
  showRight: boolean;
  /** 文件分析的波形包络点数。 */
  envelopePoints: number;
  /** 启动时是否自动恢复上次打开的文件。 */
  restoreSession: boolean;
  setSpectrumMode: Dispatch<SetStateAction<SpectrumMode>>;
  setDbRangeIdx: Dispatch<SetStateAction<number>>;
  setChannelLayout: Dispatch<SetStateAction<ChannelLayout>>;
  setShowLeft: Dispatch<SetStateAction<boolean>>;
  setShowRight: Dispatch<SetStateAction<boolean>>;
  setEnvelopePoints: Dispatch<SetStateAction<number>>;
  setRestoreSession: Dispatch<SetStateAction<boolean>>;
  /** 当前偏好的快照，供偏好设置对话框渲染。 */
  prefs: Prefs;
  /** 把一份偏好应用到全部状态。 */
  applyPrefs: (value: Prefs) => void;
  /**
   * 重置某一组本地设置。
   *
   * 删除对应的存储键，并把运行中的界面同步回默认值 —— 只删键不刷新
   * 界面的话，用户会看到「按钮按了没反应」。
   */
  reset: (kind: "prefs" | "layout" | "session") => void;
  /**
   * 布局重置计数：递增会重建 Workspace，让它重新读一遍面板宽度。
   *
   * 侧栏宽度由 Workspace 组件自持；只有重建它才会重新读一次存储。
   */
  layoutEpoch: number;
}

/**
 * 偏好的状态与持久化。
 *
 * @param onNotice 提示用户「已重置」之类的短消息。
 */
export function usePrefs(onNotice: (message: string) => void): PrefsController {
  // 初值来自 `readPrefs()`：它会先读新键，再从旧键迁移，所以已有的用户
  // 设置不会因为换了存储位置而丢失。
  // `useState` 的惰性初始化只跑一次；写成 `useRef(readPrefs()).current`
  // 会在每次渲染都读一次 localStorage 并解析 JSON。
  const [initialPrefs] = useState(readPrefs);
  const [spectrumMode, setSpectrumMode] = useState<SpectrumMode>(
    initialPrefs.spectrumMode,
  );
  const [dbRangeIdx, setDbRangeIdx] = useState(initialPrefs.dbRange);
  const [channelLayout, setChannelLayout] = useState<ChannelLayout>(
    initialPrefs.channelLayout,
  );
  const [showLeft, setShowLeft] = useState(initialPrefs.showLeft);
  const [showRight, setShowRight] = useState(initialPrefs.showRight);
  const [envelopePoints, setEnvelopePoints] = useState(
    initialPrefs.envelopePoints,
  );
  const [restoreSession, setRestoreSession] = useState(
    initialPrefs.restoreSession,
  );
  const [layoutEpoch, setLayoutEpoch] = useState(0);

  useEffect(() => {
    writeStoredJson(VISIBILITY_KEY, { left: showLeft, right: showRight });
  }, [showLeft, showRight]);

  useEffect(() => {
    writeStoredJson(DISPLAY_KEY, {
      mode: spectrumMode,
      dbRange: dbRangeIdx,
      channelLayout,
    });
  }, [spectrumMode, dbRangeIdx, channelLayout]);

  // 统一偏好存储键。状态是真值源，这个 effect 只是把它落盘。
  useEffect(() => {
    writePrefs({
      spectrumMode,
      dbRange: dbRangeIdx,
      channelLayout,
      showLeft,
      showRight,
      envelopePoints,
      restoreSession,
    });
  }, [
    spectrumMode,
    dbRangeIdx,
    channelLayout,
    showLeft,
    showRight,
    envelopePoints,
    restoreSession,
  ]);

  /** 把一份偏好应用到全部状态。 */
  const applyPrefs = useCallback((value: Prefs) => {
    setSpectrumMode(value.spectrumMode);
    setDbRangeIdx(value.dbRange);
    setChannelLayout(value.channelLayout);
    setShowLeft(value.showLeft);
    setShowRight(value.showRight);
    setEnvelopePoints(value.envelopePoints);
    setRestoreSession(value.restoreSession);
  }, []);

  /** 当前偏好的快照，供偏好设置对话框渲染。 */
  const prefs = useMemo<Prefs>(
    () => ({
      spectrumMode,
      dbRange: dbRangeIdx,
      channelLayout,
      showLeft,
      showRight,
      envelopePoints,
      restoreSession,
    }),
    [
      spectrumMode,
      dbRangeIdx,
      channelLayout,
      showLeft,
      showRight,
      envelopePoints,
      restoreSession,
    ],
  );

  const reset = useCallback(
    (kind: "prefs" | "layout" | "session") => {
      if (kind === "prefs") {
        clearPrefs();
        applyPrefs(readPrefs());
        onNotice("显示与分析偏好已重置");
      } else if (kind === "layout") {
        resetLayout();
        setLayoutEpoch((n) => n + 1);
        applyPrefs({ ...readPrefs(), showLeft: true, showRight: true });
        onNotice("面板布局已重置");
      } else {
        clearSession();
        onNotice("会话记录已清除，下次启动不再自动恢复文件");
      }
    },
    [applyPrefs, onNotice],
  );

  return {
    spectrumMode,
    dbRangeIdx,
    channelLayout,
    showLeft,
    showRight,
    envelopePoints,
    restoreSession,
    setSpectrumMode,
    setDbRangeIdx,
    setChannelLayout,
    setShowLeft,
    setShowRight,
    setEnvelopePoints,
    setRestoreSession,
    prefs,
    applyPrefs,
    reset,
    layoutEpoch,
  };
}
