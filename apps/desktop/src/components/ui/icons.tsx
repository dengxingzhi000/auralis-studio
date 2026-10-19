/**
 * 图标集 —— 内联 SVG，不引入图标库。
 *
 * 规格与参考界面一致：16×16 视口、1.5 线宽、currentColor 描边、圆头线帽。
 * 尺寸通过 `size` 控制，默认与 12px 正文对齐（14px）。
 *
 * 为什么不用图标库：ADR-017 明确「不引入完整组件库」，图标同理——
 * 本项目只需要十几个图标，一个 2 KB 的内联模块比多一个运行时依赖划算。
 */

export interface IconProps {
  /** 边长，px。默认 14 */
  size?: number;
  className?: string;
}

const base = (size: number) => ({
  width: size,
  height: size,
  viewBox: "0 0 16 16",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.5,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
  "aria-hidden": true,
  focusable: false,
});

/** 打开音频文件 */
export function IconFilePlus({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className}>
      <path d="M9 1.5H4.5A1.5 1.5 0 0 0 3 3v10a1.5 1.5 0 0 0 1.5 1.5h7A1.5 1.5 0 0 0 13 13V5.5z" />
      <path d="M9 1.5V5.5H13" />
      <path d="M8 8v4M6 10h4" />
    </svg>
  );
}

/** 波形 */
export function IconWaveform({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className}>
      <path d="M2 8h1.2l1-3.5L6 12l1.6-9 1.8 6.4L11 6h3" />
    </svg>
  );
}

/** 频谱 */
export function IconSpectrum({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className}>
      <path d="M2 13V9M5 13V6M8 13V2.5M11 13V7M14 13v-2" />
    </svg>
  );
}

/** 声谱图 */
export function IconSpectrogram({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className}>
      <rect x="2" y="2.5" width="12" height="11" rx="1.5" />
      <path d="M2 10.5c2.5 0 3-2 5-2s2.5 2 5 2" />
      <path d="M6 6.5h.01M9 8.5h.01" />
    </svg>
  );
}

/** 指标 / 汇总 */
export function IconGauge({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className}>
      <path d="M2.5 12a6.5 6.5 0 1 1 11 0" />
      <path d="M8 12l3-4" />
    </svg>
  );
}

/** 设置 */
export function IconSettings({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className}>
      <circle cx="8" cy="8" r="2" />
      <path d="M8 1.5v1.8M8 12.7v1.8M14.5 8h-1.8M3.3 8H1.5M12.6 3.4l-1.3 1.3M4.7 11.3l-1.3 1.3M12.6 12.6l-1.3-1.3M4.7 4.7L3.4 3.4" />
    </svg>
  );
}

/** 帮助 */
export function IconHelp({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className}>
      <circle cx="8" cy="8" r="6.5" />
      <path d="M6.3 6.2a1.8 1.8 0 1 1 2.3 2c-.4.2-.6.5-.6 1v.3" />
      <path d="M8 11.6h.01" />
    </svg>
  );
}

/** 搜索 */
export function IconSearch({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className}>
      <circle cx="7.2" cy="7.2" r="4.7" />
      <path d="M10.6 10.6L14 14" />
    </svg>
  );
}

/** 展开 / 收起指示 */
export function IconChevronDown({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className}>
      <path d="M4 6.5L8 10.5l4-4" />
    </svg>
  );
}

/** 更多操作 */
export function IconMore({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className}>
      <path d="M8 3.2h.01M8 8h.01M8 12.8h.01" strokeWidth={2} />
    </svg>
  );
}

/** 关闭 */
export function IconX({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className}>
      <path d="M4 4l8 8M12 4l-8 8" />
    </svg>
  );
}

/** 任务 / 队列 */
export function IconTasks({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className}>
      <path d="M2.5 4.5h11M2.5 8h11M2.5 11.5h7" />
    </svg>
  );
}

/** 设备 / 输入 */
export function IconDevice({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className}>
      <rect x="2" y="3" width="12" height="8" rx="1.5" />
      <path d="M5.5 13.5h5" />
      <path d="M8 11v2.5" />
    </svg>
  );
}

/** 警告（错误条与提示用） */
export function IconAlert({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className}>
      <path d="M8 2.5l5.5 10h-11z" />
      <path d="M8 6.5v3M8 11.4h.01" />
    </svg>
  );
}
