/**
 * AboutDialog —— 关于与版本信息。
 *
 * 版本信息分两处来源：算法侧来自 Rust 引擎的 `app_info`（版本、算法
 * 版本、DSP 规范修订），容器侧来自 preload 暴露的运行时版本。
 * 两处都如实显示，取不到就显示「—」，不编造。
 */

import { useMemo } from "react";

import { bridge } from "@/lib/bridge";
import type { AppInfo } from "@/lib/contracts";

import { Modal } from "./primitives";

export interface AboutDialogProps {
  open: boolean;
  onClose: () => void;
  /** 引擎给出的应用与算法信息 */
  info: AppInfo | null;
}

export function AboutDialog({ open, onClose, info }: AboutDialogProps) {
  // 容器版本：在没有 preload 的环境里（浏览器里打开 dev server）
  // bridge() 会抛错 —— 取不到就显示「—」，不能因此让对话框打不开。
  const versions = useMemo(() => {
    try {
      return bridge().container.versions;
    } catch {
      return null;
    }
  }, []);

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Auralis Studio"
      subtitle="本地音频分析工作台"
      width={520}
      footer={
        <>
          <span className="muted">MIT OR Apache-2.0</span>
          <span className="spacer" />
          <button type="button" className="btn" onClick={onClose}>
            关闭
          </button>
        </>
      }
    >
      <div className="about-list">
        <div className="about-row">
          <span className="muted">应用版本</span>
          <span className="num">{info?.version ?? "—"}</span>
        </div>
        <div className="about-row">
          <span className="muted">算法版本</span>
          <span className="num">{info?.algorithmVersion ?? "—"}</span>
        </div>
        <div className="about-row">
          <span className="muted">DSP 规范修订</span>
          <span className="num">{info?.dspSpecRevision ?? "—"}</span>
        </div>

        <div className="about-divider" />

        <div className="about-row">
          <span className="muted">容器</span>
          <span className="num">Electron {versions?.electron ?? "—"}</span>
        </div>
        <div className="about-row">
          <span className="muted">Chromium</span>
          <span className="num">{versions?.chrome ?? "—"}</span>
        </div>
        <div className="about-row">
          <span className="muted">Node.js</span>
          <span className="num">{versions?.node ?? "—"}</span>
        </div>

        <div className="about-divider" />

        <p className="note">
          dB 换算、FFT、降采样、峰值检测全部在 Rust 引擎侧完成，
          前端只负责展示（ADR-018）。真实文件与实时输入共用同一条计算链路。
        </p>
        <p className="note">
          第三方依赖与许可证清单见仓库根目录的{" "}
          <code>THIRD_PARTY_LICENSES.md</code>；安装包内也随附一份。
        </p>
      </div>
    </Modal>
  );
}
