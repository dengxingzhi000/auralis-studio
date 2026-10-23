/**
 * 键盘与拖放：把文件送进分析的两条输入通道。
 *
 * 两者放在一起是因为它们是同一件事的两种输入方式：都在「拿到一个文件
 * 路径 → 交给分析」之前打转，各自只有几行监听逻辑。
 *
 * ## 键盘归模态
 *
 * 有模态打开时键盘归模态：否则在对话框里按 Esc 会同时「关闭对话框」
 * 和「取消正在进行的分析」；在搜索面板里按 Ctrl+O 会在面板上再弹一个
 * 文件选择器。
 *
 * ## 拖放为什么走 DOM 事件
 *
 * Electron 不再提供 `onDragDropEvent`，走标准 DOM 拖放。Chromium 不把
 * 磁盘路径交给页面，路径由 preload 的 `getPathForFile` 解析。
 *
 * ## 入参为什么要「稳定」
 *
 * 监听器挂在 `useEffect` 里，依赖变了就会重新挂一遍。传入的回调如果
 * 每次渲染都是新函数（比如 `() => onOpenFile()`），监听器就会每帧重挂
 * —— 没必要。所以这里直接收 `useCallback` / `useState` 产出的稳定引用，
 * 需要开对话框时收 `setDialog` 这样的 setter，而不是包一层的箭头函数。
 */

import { useEffect, type RefObject } from "react";

import { isAudioPath } from "./ipc";
import type { UiError } from "./errors";

/** 可以打开的模态对话框。 */
export type DialogKind = "prefs" | "help" | "about";

/** 快捷键与拖放需要的输入。 */
export interface ShortcutOptions {
  /** 是否有模态对话框打开（偏好 / 帮助 / 关于）。 */
  dialogOpen: boolean;
  /** 搜索面板是否打开。 */
  searchOpen: boolean;
  /** 当前可取消的分析任务；`false` = 没有在跑的任务。 */
  hasActiveTask: boolean;
  /**
   * 最近一次成功打开的路径（ref）。
   *
   * 用 ref 而不是 state：它只在事件发生时读一次，不需要触发重渲染，
   * 也不该让监听器因为它变化而重新注册。
   */
  lastPath: RefObject<string | null>;
  /** 打开文件（Ctrl+O）。 */
  onOpenFile: () => void;
  /** 取消当前分析（Esc）。 */
  onCancelTask: () => void;
  /** 重试上次分析（Ctrl+R）。 */
  onRetry: () => void;
  /** 打开 / 关闭搜索面板（Ctrl+K / Ctrl+F）。 */
  setSearchOpen: (open: boolean) => void;
  /** 打开某个模态对话框。 */
  setDialog: (dialog: DialogKind | null) => void;
  /** 拖入了一个受支持的音频文件。 */
  onDropFile: (path: string) => void;
  /** 拖放失败（不是桌面容器，或格式不支持）。 */
  onError: (error: UiError) => void;
}

/** 注册键盘快捷键与拖放监听。 */
export function useShortcuts(options: ShortcutOptions): void {
  const {
    dialogOpen,
    searchOpen,
    hasActiveTask,
    lastPath,
    onOpenFile,
    onCancelTask,
    onRetry,
    setSearchOpen,
    setDialog,
    onDropFile,
    onError,
  } = options;

  // ---- 键盘：Ctrl/Cmd+O 打开，Esc 取消，Ctrl/Cmd+R 重试，Ctrl/Cmd+K 搜索 ----
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      const key = e.key.toLowerCase();

      if (mod && (key === "k" || key === "f")) {
        e.preventDefault();
        if (!dialogOpen) setSearchOpen(true);
        return;
      }

      if (dialogOpen || searchOpen) return;

      if (mod && key === "o") {
        e.preventDefault();
        onOpenFile();
      } else if (mod && key === ",") {
        e.preventDefault();
        setDialog("prefs");
      } else if (e.key === "F1") {
        e.preventDefault();
        setDialog("help");
      } else if (e.key === "Escape" && hasActiveTask) {
        e.preventDefault();
        onCancelTask();
      } else if (mod && key === "r" && lastPath.current) {
        e.preventDefault();
        onRetry();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [
    hasActiveTask,
    dialogOpen,
    searchOpen,
    lastPath,
    onCancelTask,
    onOpenFile,
    onRetry,
    setDialog,
    setSearchOpen,
  ]);

  // ---- 拖放：桌面音频应用的基本预期 ----
  useEffect(() => {
    const onDragOver = (e: DragEvent) => {
      // 不 preventDefault 的话浏览器会拒绝 drop 事件。
      if (e.dataTransfer?.types.includes("Files")) e.preventDefault();
    };

    const onDrop = (e: DragEvent) => {
      const files = Array.from(e.dataTransfer?.files ?? []);
      if (files.length === 0) return;
      e.preventDefault();

      const api = window.auralis;
      if (!api) {
        onError({ title: "拖放仅在桌面容器中可用", detail: null });
        return;
      }

      const paths = files.map((file) => api.getPathForFile(file));
      const path = paths.find(isAudioPath);
      if (path) onDropFile(path);
      else onError({ title: "拖入的文件不是受支持的音频格式", detail: null });
    };

    window.addEventListener("dragover", onDragOver);
    window.addEventListener("drop", onDrop);
    return () => {
      window.removeEventListener("dragover", onDragOver);
      window.removeEventListener("drop", onDrop);
    };
  }, [onDropFile, onError]);
}
