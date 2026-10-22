/**
 * 容器桥接（preload 暴露的 `window.auralis`）。
 *
 * 这一层存在的意义：把「当前跑在哪个容器里」收敛到一个文件。`ipc.ts`
 * 只依赖这里的类型与 `bridge()`，不再直接 import 任何容器 SDK。
 */

/** 渲染进程可见的容器信息。与 `electron/preload.ts` 的 `ContainerInfo` 对应。 */
export interface ContainerInfo {
  /** 容器类型。 */
  kind: "electron";
  /** 实际运行时版本。 */
  versions: {
    electron: string;
    chrome: string;
    node: string;
    v8: string;
  };
}

/** 文件对话框过滤器（与 Electron 的 `FileFilter` 同形）。 */
export interface DialogFilter {
  name: string;
  extensions: string[];
}

/** 打开对话框的参数。 */
export interface OpenFileOptions {
  filters?: DialogFilter[];
  multiple?: boolean;
  directory?: boolean;
}

/** 保存对话框的参数。 */
export interface SaveFileOptions {
  defaultPath?: string;
  filters?: DialogFilter[];
}

/** preload 暴露的 API。 */
export interface AuralisBridge {
  container: ContainerInfo;
  invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T>;
  openFile(options?: OpenFileOptions): Promise<string | string[] | null>;
  saveFile(options?: SaveFileOptions): Promise<string | null>;
  /** 取拖入文件的磁盘路径（Electron `webUtils.getPathForFile`）。 */
  getPathForFile(file: File): string;
}

declare global {
  interface Window {
    /** 仅在桌面容器中由 preload 注入；浏览器里打开 dev server 时为 undefined。 */
    auralis?: AuralisBridge;
  }
}

/**
 * 取桥接对象。
 *
 * # Throws
 * 在普通浏览器里（没有 preload）访问时抛出可读错误，避免出现
 * `Cannot read properties of undefined` 这种没有上下文的报错。
 */
export function bridge(): AuralisBridge {
  const api = window.auralis;
  if (!api) {
    throw new Error("桌面容器桥接不可用：该页面必须在 Auralis Studio 容器中运行");
  }
  return api;
}
