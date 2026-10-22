/**
 * Workspace —— 可停靠布局骨架。
 *
 * M1 只实现三栏 + 一个竖向分隔条（split view），**不是**完整的
 * dock 系统。理由：ADR-017 要求「不提前建设通用设计系统」，
 * 先只做当下用得到的。可停靠需要面板拖出/吸附/持久化，
 * 在真实交互需求出现前不值得先造。
 *
 * 分隔条用 pointer 事件而非 HTML5 drag&drop：桌面应用里
 * pointer 更精确，且不与 WebView 的原生拖放冲突。
 *
 * ## 宽度约束
 *
 * 两个侧栏的宽度是「用户意图」，只在两种情况下被改写：
 *   1. 用户拖分隔条（立即，受 MIN/MAX 限制）
 *   2. 窗口窄到两侧合计超过可用宽度的 52%（按比例收缩）
 *
 * 关键：约束**只能收缩**。曾经写成「resize 时一律钳到 maxPanel」，
 * 结果窗口只要发生一次尺寸变化（哪怕只是重新加载触发的 resize），
 * 264px 的侧栏就会被永久压到 160px 下限，默认宽度形同虚设。
 *
 * ## 持久化
 *
 * 拖动后的宽度写入 `localStorage`（键见 `WIDTHS_KEY`），下次启动沿用。
 * 只存界面偏好，不存任何分析数据。
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { asNumberInRange, readStoredJson, writeStoredJson } from "@/lib/persist";

export interface WorkspaceProps {
  left: React.ReactNode;
  center: React.ReactNode;
  right: React.ReactNode;
  /** 左侧栏初始宽度 px */
  leftWidth?: number;
  /** 右侧栏初始宽度 px */
  rightWidth?: number;
  leftVisible?: boolean;
  rightVisible?: boolean;
}

// 两侧最小宽度不同：检查器要放「标签 + 数值输入 + 单位」，比只有导航的侧栏更吃宽度。
// 160px 曾把检查器压到 "频—" 这种不可读的状态。
const MIN_PANEL_LEFT = 180;
const MIN_PANEL_RIGHT = 240;
const MAX_PANEL = 520;
const SPLITTER = 5;

/** 面板宽度的持久化键。改默认宽度时**不要**改它，否则用户布局会被丢掉。 */
const WIDTHS_KEY = "auralis.workspace.widths.v1";

function readStoredWidths(): { left: number; right: number } | null {
  const parsed = readStoredJson(WIDTHS_KEY, (v) => {
    if (typeof v !== "object" || v === null) return null;
    const rec = v as Record<string, unknown>;
    const left = asNumberInRange(rec.left, MIN_PANEL_LEFT, MAX_PANEL);
    const right = asNumberInRange(rec.right, MIN_PANEL_RIGHT, MAX_PANEL);
    return left != null && right != null ? { left, right } : null;
  });
  return parsed;
}

export function Workspace({
  left,
  center,
  right,
  // 默认宽度按内容定：侧栏要容纳「图标 + 中文标签 + 里程碑标记」，
  // 检查器要容纳「标签 + 78px 输入框 + 单位」而不换行。
  leftWidth = 264,
  rightWidth = 320,
  leftVisible = true,
  rightVisible = true,
}: WorkspaceProps) {
  // 记住上次拖出来的宽度：每次启动都回到默认值对一个专业工具来说很烦。
  // 存的是**用户意图**，仍会被下面的窄窗口约束收缩。
  const stored = useRef(readStoredWidths()).current;
  const [lw, setLw] = useState(stored?.left ?? leftWidth);
  const [rw, setRw] = useState(stored?.right ?? rightWidth);
  // ResizeObserver 回调里要读「另一个侧栏」的当前宽度，用 ref 同步，
  // 避免把 lw/rw 放进 effect 依赖导致反复重建观察器。
  const lwRef = useRef(lw);
  const rwRef = useRef(rw);
  lwRef.current = lw;
  rwRef.current = rw;
  const wrapRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ side: "left" | "right"; startX: number; startW: number } | null>(
    null,
  );

  const onPointerDown = useCallback(
    (side: "left" | "right") => (e: React.PointerEvent<HTMLDivElement>) => {
      e.currentTarget.setPointerCapture(e.pointerId);
      dragRef.current = {
        side,
        startX: e.clientX,
        startW: side === "left" ? lw : rw,
      };
      document.body.style.cursor = "col-resize";
      document.body.style.userSelect = "none";
    },
    [lw, rw],
  );

  const onPointerMove = useCallback((e: React.PointerEvent<HTMLDivElement>) => {
    const d = dragRef.current;
    if (!d) return;
    const delta = e.clientX - d.startX;
    // 左侧栏：向右拖变宽；右侧栏：向左拖变宽
    const next = d.side === "left" ? d.startW + delta : d.startW - delta;
    const min = d.side === "left" ? MIN_PANEL_LEFT : MIN_PANEL_RIGHT;
    const clamped = Math.max(min, Math.min(MAX_PANEL, next));
    if (d.side === "left") setLw(clamped);
    else setRw(clamped);
  }, []);

  const onPointerUp = useCallback((e: React.PointerEvent<HTMLDivElement>) => {
    if (dragRef.current) {
      e.currentTarget.releasePointerCapture(e.pointerId);
      dragRef.current = null;
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    }
  }, []);

  // 窗口缩小时防止侧栏把中心区挤没。
  //
  // 约束按「两侧合计」算，且只在超限时收缩：
  // 早先的实现每次 resize 都把两侧各自钳到 avail*0.4，于是 264px 的侧栏在
  // 任何一次窗口变化后都会被压到下限，默认宽度形同虚设。
  useEffect(() => {
    const wrap = wrapRef.current;
    if (!wrap) return;
    const clamp = () => {
      const avail = wrap.clientWidth - SPLITTER * 2;
      // 两侧合计不超过可用宽度的 52%：最小窗口（1100）下中心区仍有约 47%，
      // 波形/频谱不会被挤成条带。
      const maxTotal = Math.max(MIN_PANEL_LEFT + MIN_PANEL_RIGHT, avail * 0.52);
      const total = lwRef.current + rwRef.current;
      if (total <= maxTotal) return;
      const ratio = maxTotal / total;
      setLw(Math.max(MIN_PANEL_LEFT, Math.round(lwRef.current * ratio)));
      setRw(Math.max(MIN_PANEL_RIGHT, Math.round(rwRef.current * ratio)));
    };
    clamp();
    const ro = new ResizeObserver(clamp);
    ro.observe(wrap);
    return () => ro.disconnect();
  }, []);

  // 记住宽度：节流 200ms，避免拖动时每帧写一次 localStorage。
  // 只在拖动结束后落盘，写入量小且不会在指针移动路径上做同步 I/O。
  useEffect(() => {
    const timer = window.setTimeout(() => {
      writeStoredJson(WIDTHS_KEY, { left: lw, right: rw });
    }, 200);
    return () => window.clearTimeout(timer);
  }, [lw, rw]);

  return (
    <div className="workspace" ref={wrapRef}>
      {leftVisible && (
        <>
          <aside className="panel panel-left" style={{ width: lw }}>
            {left}
          </aside>
          <div
            className="splitter splitter-v"
            onPointerDown={onPointerDown("left")}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
            role="separator"
            aria-orientation="vertical"
            aria-label="调整左侧栏宽度"
          />
        </>
      )}

      <main className="panel panel-center">{center}</main>

      {rightVisible && (
        <>
          <div
            className="splitter splitter-v"
            onPointerDown={onPointerDown("right")}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
            role="separator"
            aria-orientation="vertical"
            aria-label="调整右侧栏宽度"
          />
          <aside className="panel panel-right" style={{ width: rw }}>
            {right}
          </aside>
        </>
      )}
    </div>
  );
}