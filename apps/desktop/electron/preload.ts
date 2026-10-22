/**
 * preload —— 渲染进程唯一的能力入口。
 *
 * 暴露三件事，全部是窄接口，渲染进程拿不到 Node、拿不到 `ipcRenderer` 本体：
 *
 * - `container`：容器身份与运行时版本（诊断、复现问题用）
 * - `invoke`：调用 Rust 引擎命令，与迁移前 Tauri 的 `invoke` 语义一致
 * - `openFile` / `saveFile`：系统文件对话框
 *
 * 迁移前这些能力来自 `@tauri-apps/api` 与 `@tauri-apps/plugin-dialog`。
 * 现在换成容器自带能力，`src/lib/ipc.ts` 只改导入，不改命令名与参数。
 */

import { contextBridge, ipcRenderer, webUtils } from "electron";

/** 渲染进程可见的容器信息。 */
export interface ContainerInfo {
  /** 容器类型；前端据此选择 IPC 适配器。 */
  kind: "electron";
  /** 实际运行时版本，用于诊断与问题复现。 */
  versions: {
    electron: string;
    chrome: string;
    node: string;
    v8: string;
  };
}

const container: ContainerInfo = {
  kind: "electron",
  versions: {
    electron: process.versions.electron ?? "unknown",
    chrome: process.versions.chrome ?? "unknown",
    node: process.versions.node,
    v8: process.versions.v8,
  },
};

contextBridge.exposeInMainWorld("auralis", {
  container,

  /** 调用 Rust 引擎命令。 */
  invoke: (cmd: string, args?: Record<string, unknown>) =>
    ipcRenderer.invoke("auralis:invoke", cmd, args ?? {}),

  /** 打开文件选择器；取消返回 null。 */
  openFile: (options?: unknown) => ipcRenderer.invoke("auralis:open-file", options ?? {}),

  /** 打开保存对话框；取消返回 null。 */
  saveFile: (options?: unknown) => ipcRenderer.invoke("auralis:save-file", options ?? {}),

  /**
   * 取拖入文件的磁盘路径。
   *
   * Chromium 出于安全考虑不把路径暴露给页面，`File.path` 在 Electron 32
   * 之后已被移除。`webUtils.getPathForFile` 是官方替代，且只能在 preload
   * 里调用 —— 渲染进程拿不到 Node，自然也不该自己拼路径。
   */
  getPathForFile: (file: File) => webUtils.getPathForFile(file),
});
