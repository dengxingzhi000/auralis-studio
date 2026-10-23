/**
 * PreferencesDialog —— 偏好设置。
 *
 * ## 单一真值源
 *
 * 这个对话框**不持有**自己的状态：它渲染 `prefs` 并通过 `onChange`
 * 把改动交出去。App 把同一份状态同时喂给顶栏复选框、右侧检查器的
 * 「显示」分区和这里 —— 三处永远一致。
 *
 * 「改了没反应」是这类界面最让人困惑的缺陷，而它的根因几乎总是
 * 同一个：两处各写各的。
 */

import { DB_RANGES, ENVELOPE_POINTS, type Prefs } from "@/lib/prefs";

import { Button, Checkbox, FormField, Modal, Select } from "./primitives";

export interface PreferencesDialogProps {
  open: boolean;
  onClose: () => void;
  /** 当前偏好（由 App 派生） */
  prefs: Prefs;
  /** 应用一份新偏好 */
  onChange: (next: Prefs) => void;
  /** 重置某一组设置：`prefs` / `layout` / `session` */
  onReset: (kind: "prefs" | "layout" | "session") => void;
}

export function PreferencesDialog({
  open,
  onClose,
  prefs,
  onChange,
  onReset,
}: PreferencesDialogProps) {
  const patch = (part: Partial<Prefs>) => onChange({ ...prefs, ...part });

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="偏好设置"
      subtitle="这些设置保存在本机，不会上传。"
      width={560}
      footer={
        <>
          <span className="muted">
            改动立即生效，并在下次启动时沿用。
          </span>
          <span className="spacer" />
          <Button onClick={onClose}>关闭</Button>
        </>
      }
    >
      <div className="prefs-section">
        <div className="label">显示</div>
        <FormField label="频谱形态">
          <Select
            value={prefs.spectrumMode}
            onValueChange={(v) =>
              patch({ spectrumMode: v === "line" ? "line" : "area" })
            }
          >
            <option value="area">填充（面积）</option>
            <option value="line">曲线（声线）</option>
          </Select>
        </FormField>
        <FormField label="dB 量程">
          <Select
            value={String(prefs.dbRange)}
            onValueChange={(v) => patch({ dbRange: Number(v) })}
          >
            {DB_RANGES.map((r, i) => (
              <option key={r.label} value={i}>
                {r.label}
              </option>
            ))}
          </Select>
        </FormField>
        <FormField label="声道显示">
          <Select
            value={prefs.channelLayout}
            onValueChange={(v) =>
              patch({ channelLayout: v === "overlay" ? "overlay" : "lanes" })
            }
          >
            <option value="lanes">分道</option>
            <option value="overlay">叠加（对比）</option>
          </Select>
        </FormField>
      </div>

      <div className="prefs-section">
        <div className="label">布局</div>
        <Checkbox
          label="显示左侧资源栏"
          checked={prefs.showLeft}
          onChange={(e) => patch({ showLeft: e.target.checked })}
        />
        <Checkbox
          label="显示右侧检查器"
          checked={prefs.showRight}
          onChange={(e) => patch({ showRight: e.target.checked })}
        />
      </div>

      <div className="prefs-section">
        <div className="label">分析</div>
        <FormField label="包络点数" unit="点">
          <Select
            className="num"
            value={String(prefs.envelopePoints)}
            onValueChange={(v) => patch({ envelopePoints: Number(v) })}
          >
            {ENVELOPE_POINTS.map((n) => (
              <option key={n} value={n}>
                {n.toLocaleString("en-US")}
              </option>
            ))}
          </Select>
        </FormField>
        <p className="note">
          波形包络的点数。点数越多，波形概览的细节越多、载荷也越大；
          导出的 CSV 用的是同一个值，所以屏幕与导出始终一致。
        </p>
        <Checkbox
          label="启动时恢复上次打开的文件"
          checked={prefs.restoreSession}
          onChange={(e) => patch({ restoreSession: e.target.checked })}
        />
      </div>

      <div className="prefs-section">
        <div className="label">数据</div>
        <div className="prefs-actions">
          <Button size="tiny" onClick={() => onReset("prefs")}>
            重置显示与分析偏好
          </Button>
          <Button size="tiny" onClick={() => onReset("layout")}>
            重置面板布局
          </Button>
          <Button size="tiny" onClick={() => onReset("session")}>
            清除会话记录
          </Button>
        </div>
        <p className="note">
          这些操作只删除本机保存的界面偏好与分析参数，不影响任何音频文件。
        </p>
      </div>
    </Modal>
  );
}
