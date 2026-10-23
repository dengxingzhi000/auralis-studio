/**
 * L1 基础控件（ADR-017 分层表第一批）。
 *
 * 这些组件只做三件事：组合类名、转发原生属性、保持类型安全。
 * 样式全部在 styles/layout.css 里按令牌定义，组件内**不写死颜色**，
 * 这样换主题只需要改 tokens.css。
 *
 * 刻意不做 `asChild` / Slot 这类灵活性：本项目没有「把按钮渲染成链接」
 * 的需求，多一层 Radix Slot 只会让 props 类型变复杂。
 */

import { useEffect, useId, useRef, useState } from "react";

import { IconAlert, IconChevronDown, IconX } from "./icons";

/* ---------------------------------------------------------------- Button */

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  /** 主操作按钮，整屏只应出现一次 */
  variant?: "default" | "primary";
  /** 紧凑尺寸，用于卡片头与标签内 */
  size?: "default" | "tiny";
  wide?: boolean;
}

export function Button({
  variant = "default",
  size = "default",
  wide = false,
  className,
  type = "button",
  ...rest
}: ButtonProps) {
  const cls = [
    "btn",
    variant === "primary" ? "primary" : "",
    size === "tiny" ? "tiny" : "",
    wide ? "wide" : "",
    className ?? "",
  ]
    .filter(Boolean)
    .join(" ");
  return <button type={type} className={cls} {...rest} />;
}

/* ------------------------------------------------------------- IconButton */

export interface IconButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  /** 无障碍名称；图标按钮没有可见文本，必须提供 */
  label: string;
}

export function IconButton({
  label,
  className,
  type = "button",
  ...rest
}: IconButtonProps) {
  return (
    <button
      type={type}
      className={["btn-icon", className ?? ""].filter(Boolean).join(" ")}
      title={label}
      aria-label={label}
      {...rest}
    />
  );
}

/* ------------------------------------------------------------------ Input */

export interface InputProps
  extends Omit<React.InputHTMLAttributes<HTMLInputElement>, "onChange"> {
  onValueChange?: (value: string) => void;
}

export function Input({ onValueChange, className, ...rest }: InputProps) {
  return (
    <input
      className={["input", className ?? ""].filter(Boolean).join(" ")}
      onChange={(e) => onValueChange?.(e.target.value)}
      {...rest}
    />
  );
}

/* ----------------------------------------------------------------- Select */

export interface SelectProps
  extends Omit<React.SelectHTMLAttributes<HTMLSelectElement>, "onChange"> {
  onValueChange?: (value: string) => void;
}

export function Select({ onValueChange, className, ...rest }: SelectProps) {
  return (
    <select
      className={["input", className ?? ""].filter(Boolean).join(" ")}
      onChange={(e) => onValueChange?.(e.target.value)}
      {...rest}
    />
  );
}

/* --------------------------------------------------------------- Checkbox */

export interface CheckboxProps
  extends Omit<React.InputHTMLAttributes<HTMLInputElement>, "type"> {
  label: React.ReactNode;
}

export function Checkbox({ label, className, ...rest }: CheckboxProps) {
  return (
    <label className={["toggle", className ?? ""].filter(Boolean).join(" ")}>
      <input type="checkbox" {...rest} />
      {label}
    </label>
  );
}

/* ------------------------------------------------------------- FormField */

export interface FormFieldProps {
  label: string;
  /** 单位，等宽小字显示在控件右侧 */
  unit?: string;
  /** 控件宽度，px */
  width?: number;
  children: React.ReactNode;
}

/**
 * 检查器里的一行表单：左标签 + 右控件。
 * 用 label 包裹是为了点击标签能聚焦控件（原生行为，无需 JS）。
 */
export function FormField({ label, unit, width, children }: FormFieldProps) {
  return (
    <label className="field">
      <span className="field-label">{label}</span>
      <span className="field-input" style={width ? { width } : undefined}>
        {children}
        {unit && <span className="field-unit">{unit}</span>}
      </span>
    </label>
  );
}

/* -------------------------------------------------------------- DataRow */

export interface DataRowProps {
  label: React.ReactNode;
  /** 已格式化好的读数；不要在这里做数值计算（ADR-018 规则 1） */
  value: React.ReactNode;
  /** 读数是否等宽（数值应为 true，状态文本为 false） */
  mono?: boolean;
  title?: string;
}

export function DataRow({ label, value, mono = true, title }: DataRowProps) {
  return (
    <div className="metric-row" title={title}>
      <span className="muted">{label}</span>
      <span className={mono ? "num" : undefined}>{value}</span>
    </div>
  );
}

/* ----------------------------------------------------------- Collapsible */

export interface CollapsibleProps {
  title: string;
  /** 标题右侧的计数或状态 */
  meta?: React.ReactNode;
  defaultOpen?: boolean;
  children: React.ReactNode;
}

/**
 * 折叠分区（对应参考界面的 Disclosure）。
 *
 * 用受控 state + button[aria-expanded] 而不是 <details>：
 * <details> 的展开动画与自定义箭头在 WebView2 上表现不一致，
 * 且我们需要 aria-expanded 给测试与无障碍树用。
 */
export function Collapsible({
  title,
  meta,
  defaultOpen = false,
  children,
}: CollapsibleProps) {
  const [open, setOpen] = useState(defaultOpen);
  const id = useId();
  return (
    <div className="disclosure">
      <button
        type="button"
        className="disclosure-head"
        aria-expanded={open}
        aria-controls={id}
        onClick={() => setOpen((v) => !v)}
      >
        {title}
        {meta != null && <span className="muted num">{meta}</span>}
        <span className="chev">
          <IconChevronDown size={13} />
        </span>
      </button>
      {open && (
        <div className="disclosure-body" id={id}>
          {children}
        </div>
      )}
    </div>
  );
}

/* ------------------------------------------------------------------ Modal */

export interface ModalProps {
  open: boolean;
  onClose: () => void;
  title?: string;
  subtitle?: React.ReactNode;
  /** 卡片宽度，默认 520px */
  width?: number;
  /** 不渲染标题栏；内容自己负责标题与关闭按钮 */
  hideHeader?: boolean;
  className?: string;
  footer?: React.ReactNode;
  children: React.ReactNode;
}

/**
 * 模态框。
 *
 * 用原生 `<dialog>` + `showModal()`：焦点陷阱、`Esc`、背景惰性和
 * 顶层合成都由浏览器保证，比手写 div 覆盖层可靠。样式仍走令牌。
 *
 * `open` 与 DOM 的 open 状态分开维护：调用方只描述意图，
 * 这里负责 `showModal()` / `close()`，避免直接操作布尔属性导致
 * 背景滚动锁失效。
 */
export function Modal({
  open,
  onClose,
  title,
  subtitle,
  width = 520,
  hideHeader = false,
  className,
  footer,
  children,
}: ModalProps) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (open && !el.open) el.showModal();
    else if (!open && el.open) el.close();
  }, [open]);

  return (
    <dialog
      ref={ref}
      className={["modal", className ?? ""].filter(Boolean).join(" ")}
      aria-labelledby={title ? titleId : undefined}
      onCancel={(e) => {
        // 默认行为是直接关闭 DOM；走调用方状态，避免 UI 与 DOM 脱节。
        e.preventDefault();
        onClose();
      }}
      onClose={onClose}
      onMouseDown={(e) => {
        // 点在背景（dialog 自身）上关闭；点卡片内容是子元素，不触发。
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="modal-card" style={{ width }}>
        {!hideHeader && (
          <header className="modal-head">
            <div className="modal-head-text">
              <h2 className="modal-title" id={title ? titleId : undefined}>
                {title}
              </h2>
              {subtitle != null && (
                <div className="modal-subtitle">{subtitle}</div>
              )}
            </div>
            <IconButton label="关闭" onClick={onClose}>
              <IconX size={15} />
            </IconButton>
          </header>
        )}
        <div className="modal-body">{children}</div>
        {footer && <footer className="modal-foot">{footer}</footer>}
      </div>
    </dialog>
  );
}

/* -------------------------------------------------------------- EmptyState */

export function EmptyState({
  title,
  hint,
}: {
  title: string;
  hint?: string;
}) {
  return (
    <div className="empty">
      <span className="empty-title">{title}</span>
      {hint && <span className="empty-hint">{hint}</span>}
    </div>
  );
}

/* --------------------------------------------------------------- ErrorBar */

/**
 * 错误条。
 *
 * 只显示调用方给的**人类可读**文案；原始异常文本作为次级详情折叠展示，
 * 且允许选中复制。裸 `String(e)` 直接铺在界面上的做法在 M1 阶段造成了
 * 「状态栏一串 TypeError」这种既看不懂也定位不了的表现。
 *
 * `action` 放操作按钮（重试 / 关闭）。错误必须能就地处理，
 * 否则用户只能靠「再点一次打开文件」来猜。
 */
export function ErrorBar({
  title,
  detail,
  action,
}: {
  title: string;
  detail?: string | null;
  action?: React.ReactNode;
}) {
  return (
    <div className="error-bar" role="alert">
      <IconAlert size={14} />
      <div className="error-bar-main">
        <div className="error-title">{title}</div>
        {detail && <div className="error-detail selectable">{detail}</div>}
      </div>
      {action && <div className="error-bar-actions">{action}</div>}
    </div>
  );
}

/* -------------------------------------------------------------- NavItem */

export interface NavItemProps {
  icon?: React.ReactNode;
  /** 右侧次要标记，例如里程碑编号 */
  tag?: string;
  active?: boolean;
  disabled?: boolean;
  title?: string;
  onClick?: () => void;
  children: React.ReactNode;
}

export function NavItem({
  icon,
  tag,
  active = false,
  disabled = false,
  title,
  onClick,
  children,
}: NavItemProps) {
  return (
    <li>
      <button
        type="button"
        className={"nav-item" + (active ? " on" : "")}
        disabled={disabled}
        title={title}
        aria-current={active ? "true" : undefined}
        onClick={onClick}
      >
        {icon && <span className="icon">{icon}</span>}
        <span className="nav-text">{children}</span>
        {tag && <span className="nav-tag">{tag}</span>}
      </button>
    </li>
  );
}

export function NavGroup({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="nav-group">
      <span className="label">{label}</span>
      <ul className="nav">{children}</ul>
    </div>
  );
}
