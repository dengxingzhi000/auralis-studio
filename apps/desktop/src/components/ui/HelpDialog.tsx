/**
 * HelpDialog —— 帮助。
 *
 * 内容全部来自 `docs/UI_SPEC.md` §6.2 与 `docs/PRD.md`，**不新造事实**。
 * 界面里写的快捷键、术语解释、能力边界，必须与文档一致 —— 否则帮助
 * 本身就成了误导。
 */

import { Modal } from "./primitives";

export interface HelpDialogProps {
  open: boolean;
  onClose: () => void;
}

/** 快捷键表。 */
const SHORTCUTS: [string, string][] = [
  ["Ctrl+O", "打开文件（也可直接拖放到窗口）"],
  ["Ctrl+K / Ctrl+F", "打开全局搜索"],
  ["Ctrl+,", "打开偏好设置"],
  ["F1", "打开本帮助"],
  ["Esc", "取消正在进行的分析"],
  ["Ctrl+R", "重试上一次文件"],
  ["频谱：拖拽框选", "放大到选中的频率区间"],
  ["频谱：滚轮", "以光标位置为中心缩放"],
  ["频谱：Shift + 拖拽", "平移频率视口"],
  ["频谱：双击", "复位到全带宽"],
  ["声谱图：滚轮 / 拖拽", "前后平移一屏 / 拖动平移"],
  ["声谱图：Shift + 滚轮", "以光标频率为中心缩放"],
  ["声谱图：Shift + 拖拽", "框选频段并放大"],
  ["声谱图：双击", "回到文件开头"],
  ["声谱图标题栏 ◀ / 开头 / ▶", "按 80% 屏宽翻页、回到开头"],
];

/** 术语表。 */
const TERMS: [string, string][] = [
  ["dBFS", "相对于数字满刻度的电平，0 dBFS 即满刻度，−140 是显示下限。"],
  ["RMS", "均方根：信号的有效值，用来描述「平均有多响」。"],
  ["Crest（波峰因数）", "峰值与 RMS 的比值，单位 dB。比值越大，瞬态越突出。"],
  ["DC Offset", "直流分量：样本的平均值。理想为 0。"],
  ["STFT", "短时傅里叶变换：把信号切成一段段做频谱，得到声谱图。"],
  ["扇贝损失", "峰值落在两个 bin 之间时，读数低于真实峰值的量。读数旁标「估算」即因此。"],
  ["超量程", "|x| > 1.0 的样本数，即数值越界。"],
  ["平顶片段", "连续多个样本顶到满刻度的片段数。它是启发式计数，不是模拟削波的证据。"],
];

/** 能力边界。 */
const LIMITS = [
  "不提供校准级 SPL 测量：没有参考声压级，读数不能换算成物理声压。",
  "不把数字满刻度推断为模拟削波：超量程只说明数值越界。",
  "不以普通 RMS 代替 LUFS：计权需要完整的算法规范与验证。",
  "不提供 SNR / THD / THD+N：这些指标需要参考信号与已知前提。",
  "WAV / FLAC / AIFF 已用签入样本验证；其余容器虽然可以解码，但界面会标注「未验证」。",
];

export function HelpDialog({ open, onClose }: HelpDialogProps) {
  return (
    <Modal
      open={open}
      onClose={onClose}
      title="帮助"
      subtitle="Auralis Studio —— 本地音频分析工作台"
      width={680}
      footer={
        <>
          <span className="muted">所有分析都在本机完成，音频不会上传。</span>
          <span className="spacer" />
          <button type="button" className="btn" onClick={onClose}>
            关闭
          </button>
        </>
      }
    >
      <div className="help-section">
        <h3 className="label">界面导览</h3>
        <ul className="help-list">
          <li>
            <b>顶部命令栏</b>：打开文件、合成信号、实时采集、导出；右侧可隐藏两侧栏。
          </li>
          <li>
            <b>左侧资源栏</b>：文件列表、分析视图入口、实时采集控制、后台任务队列。
          </li>
          <li>
            <b>中央画布</b>：频谱、声谱图、波形、电平与指标。可纵向滚动。
          </li>
          <li>
            <b>右侧检查器</b>：合成信号参数、文件信息、显示选项与版本信息。
          </li>
          <li>
            <b>底部状态栏</b>：采样率、端到端耗时、载荷、任务状态与错误。
          </li>
        </ul>
      </div>

      <div className="help-section">
        <h3 className="label">快捷键</h3>
        <table className="help-table">
          <tbody>
            {SHORTCUTS.map(([key, what]) => (
              <tr key={key}>
                <td className="help-key">
                  <kbd className="kbd">{key}</kbd>
                </td>
                <td>{what}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div className="help-section">
        <h3 className="label">术语</h3>
        <table className="help-table">
          <tbody>
            {TERMS.map(([term, what]) => (
              <tr key={term}>
                <td className="help-term">{term}</td>
                <td>{what}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div className="help-section">
        <h3 className="label">能力边界</h3>
        <ul className="help-list">
          {LIMITS.map((text) => (
            <li key={text}>{text}</li>
          ))}
        </ul>
      </div>
    </Modal>
  );
}
