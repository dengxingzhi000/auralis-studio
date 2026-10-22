/**
 * 用 esbuild 打包 Electron 主进程与 preload。
 *
 * 两者都输出 **CJS**：
 * - 主进程用 CJS 可以直接使用 `__dirname`，不必在 ESM 里重算路径；
 * - Electron 要求 preload 在 `sandbox: true` 下必须是 CJS。
 *
 * `electron` 模块保持 external：它由运行时注入，不打进产物。
 */

import { build } from "esbuild";

/** 两份产物共用的配置。 */
const common = {
  bundle: true,
  platform: "node",
  target: "node22",
  format: "cjs",
  external: ["electron"],
  sourcemap: false,
  logLevel: "info",
};

await build({
  ...common,
  entryPoints: ["electron/main.ts"],
  outfile: "electron/dist/main.cjs",
});

await build({
  ...common,
  entryPoints: ["electron/preload.ts"],
  outfile: "electron/dist/preload.cjs",
});
