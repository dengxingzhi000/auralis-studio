/**
 * SearchPalette —— 全局搜索（文件、峰值、操作）。
 *
 * 搜索本身只做字符串匹配与展示切换，不解析音频、不做任何数值换算：
 * 峰值频率与电平在 Rust 侧已经算好，这里只按用户输入筛选。
 *
 * 键盘遵循命令面板惯例：↑↓ 选择、Enter 执行、Esc 关闭。
 */

import { useEffect, useMemo, useRef, useState } from "react";

import type { PaletteItem } from "@/lib/palette";
import { matchItems } from "@/lib/search";

import { IconSearch } from "./icons";
import { Modal } from "./primitives";

export type { PaletteItem };

export interface SearchPaletteProps {
  open: boolean;
  onClose: () => void;
  items: PaletteItem[];
  placeholder?: string;
}

export function SearchPalette({
  open,
  onClose,
  items,
  placeholder = "搜索文件、峰值或操作…",
}: SearchPaletteProps) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setActive(0);
    // 等 dialog 完成 showModal 后再聚焦，否则焦点会被对话框夺走。
    const id = window.requestAnimationFrame(() => inputRef.current?.focus());
    return () => window.cancelAnimationFrame(id);
  }, [open]);

  const filtered = useMemo(
    () => matchItems(items, query),
    [items, query],
  );

  // 结果变短时钳住选中项，避免 Enter 落到越界位置。
  useEffect(() => {
    setActive((i) => Math.min(i, Math.max(0, filtered.length - 1)));
  }, [filtered.length]);

  // 选中项跟随键盘滚动。
  useEffect(() => {
    const list = listRef.current;
    const el = list?.querySelector<HTMLElement>(`[data-index="${active}"]`);
    el?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const groups = useMemo(() => {
    const map = new Map<string, { item: PaletteItem; index: number }[]>();
    filtered.forEach((item, index) => {
      const rows = map.get(item.group) ?? [];
      rows.push({ item, index });
      map.set(item.group, rows);
    });
    return [...map.entries()];
  }, [filtered]);

  const choose = (item: PaletteItem) => {
    // 先关当前弹层，再执行：动作本身可能要打开另一个弹层。
    onClose();
    item.run();
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      hideHeader
      width={640}
      className="modal-palette"
    >
      <div className="palette-input-row">
        <IconSearch size={15} />
        <input
          ref={inputRef}
          className="palette-input"
          value={query}
          placeholder={placeholder}
          spellCheck={false}
          onChange={(e) => {
            setQuery(e.target.value);
            setActive(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setActive((i) => Math.min(i + 1, filtered.length - 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setActive((i) => Math.max(i - 1, 0));
            } else if (e.key === "Enter") {
              e.preventDefault();
              const item = filtered[active];
              if (item) choose(item);
            }
          }}
          aria-label="搜索"
          aria-controls="palette-results"
          aria-activedescendant={
            filtered[active] ? `palette-opt-${active}` : undefined
          }
        />
        <kbd className="kbd">Esc</kbd>
      </div>

      <div className="palette-results" id="palette-results" ref={listRef} role="listbox">
        {filtered.length === 0 ? (
          <div className="palette-empty">
            {items.length === 0
              ? "暂无可搜索的内容。打开一个文件后，峰值会出现在这里。"
              : `没有匹配「${query.trim()}」的结果。`}
          </div>
        ) : (
          groups.map(([group, rows]) => (
            <div className="palette-group" key={group}>
              <div className="palette-group-label">{group}</div>
              {rows.map(({ item, index }) => (
                <button
                  type="button"
                  key={item.id}
                  // DOM id 用**下标**而不是条目 id：条目 id 可能来自文件路径
                  // （空格、反斜杠、冒号），作为 HTML id 不合法，
                  // aria-activedescendant 会引用不到。
                  id={`palette-opt-${index}`}
                  data-index={index}
                  role="option"
                  aria-selected={index === active}
                  className={
                    "palette-item" + (index === active ? " on" : "")
                  }
                  onMouseEnter={() => setActive(index)}
                  onClick={() => choose(item)}
                >
                  <span className="palette-label">{item.label}</span>
                  {item.hint && <span className="palette-hint">{item.hint}</span>}
                </button>
              ))}
            </div>
          ))
        )}
      </div>

      <div className="palette-foot">
        <span>
          <kbd className="kbd">↑</kbd> <kbd className="kbd">↓</kbd> 选择
        </span>
        <span>
          <kbd className="kbd">Enter</kbd> 执行
        </span>
        <span className="spacer" />
        <span className="muted">{filtered.length} 项</span>
      </div>
    </Modal>
  );
}
