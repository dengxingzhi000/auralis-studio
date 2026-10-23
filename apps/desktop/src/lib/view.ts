/**
 * 界面显示模式的类型。
 *
 * 与 `Prefs` 里的字段保持一致；单独成文件是为了让 `App` 与检查器
 * 共享同一份定义，而不是各自写一个字面量联合。
 */

/** 频谱显示模式：填充（面积）或仅曲线。 */
export type SpectrumMode = "area" | "line";

/** 多声道波形的排布：分道 / 叠加。 */
export type ChannelLayout = "lanes" | "overlay";
