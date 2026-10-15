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
 */

import { useCallback, useEffect, useRef, useState } from "react";

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

const MIN_PANEL = 160;
const MAX_PANEL = 520;
const SPLITTER = 5;

export function Workspace({
  left,
  center,
  right,
  leftWidth = 240,
  rightWidth = 280,
  leftVisible = true,
  rightVisible = true,
}: WorkspaceProps) {
  const [lw, setLw] = useState(leftWidth);
  const [rw, setRw] = useState(rightWidth);
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
    const clamped = Math.max(MIN_PANEL, Math.min(MAX_PANEL, next));
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

  // 窗口缩小时防止侧栏把中心区挤没
  useEffect(() => {
    const wrap = wrapRef.current;
    if (!wrap) return;
    const ro = new ResizeObserver(() => {
      const avail = wrap.clientWidth - SPLITTER * 2;
      const maxPanel = Math.max(MIN_PANEL, avail * 0.4);
      setLw((v) => Math.min(v, maxPanel));
      setRw((v) => Math.min(v, maxPanel));
    });
    ro.observe(wrap);
    return () => ro.disconnect();
  }, []);

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