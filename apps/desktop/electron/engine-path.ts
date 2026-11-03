/**
 * 引擎可执行文件路径的**纯函数**判定。
 *
 * ## 为什么单独抽出来
 *
 * Electron 45.0.0 有一个实测过的事实：用 `electron .` 从源码运行时，
 * `app.isPackaged` 仍返回 `true`（而 `process.defaultApp` 是 `true`）。
 * 单测不能启动 Electron，因此把「怎么判断运行形态」从 `engine.ts`
 * 抽到这里，只依赖显式传入的值，可以被 vitest 直接覆盖。
 *
 * 判定顺序：
 * 1. `AURALIS_ENGINE` 覆盖（开发/调试/测试用）；
 * 2. 应用从 asar 加载（安装包形态）→ `<resources>/engine/auralis-engine.exe`；
 *    即使该文件缺失也返回**期望路径**，让报错指向用户机器上的正确位置；
 * 3. 开发形态下若打包路径恰好存在（例如自定义 Electron 分发）→ 用它；
 * 4. 否则回退到仓库的 cargo 产物 `target/debug/auralis-engine.exe`。
 */

import path from "node:path";

/** [`resolveEnginePath`] 的输入。 */
export interface EnginePathInput {
  /** `AURALIS_ENGINE` 环境变量；空字符串视为未设置。 */
  override?: string | null;
  /** Electron 的 `process.resourcesPath`。 */
  resourcesPath: string;
  /** Electron 的 `app.getAppPath()`。安装包形态下以 `.asar` 结尾。 */
  appPath: string;
  /** 打包路径的引擎文件是否真的存在。由调用方用 `fs.existsSync` 计算。 */
  packagedExists: boolean;
  /** 开发形态下仓库根目录（`target/` 的父目录）。 */
  devRepoRoot: string;
}

/** 安装包形态下引擎相对于 `resourcesPath` 的位置。 */
const PACKAGED_SUBPATH = path.join("engine", "auralis-engine.exe");
/** 开发形态下引擎相对于仓库根目录的位置。 */
const DEV_SUBPATH = path.join("target", "debug", "auralis-engine.exe");

/**
 * 解析引擎可执行文件路径。
 *
 * 纯函数：不读环境变量、不碰文件系统，所有外部状态由 [`EnginePathInput`] 传入。
 */
export function resolveEnginePath(input: EnginePathInput): string {
  const override = input.override?.trim();
  if (override) return override;

  const packaged = path.join(input.resourcesPath, PACKAGED_SUBPATH);
  // 安装包形态判据：Electron 从 app.asar 加载应用。
  // 不用 `app.isPackaged` —— 它在源码运行时也会是 true（见文件头说明）。
  const runningFromAsar = input.appPath.toLowerCase().endsWith(".asar");

  if (runningFromAsar || input.packagedExists) return packaged;

  return path.join(input.devRepoRoot, DEV_SUBPATH);
}
