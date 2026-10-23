/**
 * 兜底解压 Electron 运行时二进制。
 *
 * ## 为什么需要它（实测，不是推测）
 *
 * electron@45.0.0 的 package.json **没有** `postinstall`，只提供
 * `install-electron` 这个 bin（指向 install.js）。因此 `npm ci` / `npm install`
 * 不再自动把二进制解压到 `node_modules/electron/dist`，后果是：
 *
 *   - `npm run electron:dev` 报错 "Electron failed to install correctly"；
 *   - electron-builder 配置的 `electronDist: node_modules/electron/dist`
 *     不存在，`npm run package:win` 直接失败。
 *
 * 旧版 electron 靠自己的 postinstall 完成这一步；升级到 45 后该行为消失。
 * 实测 `npm ci` 前后对照：装前 dist 存在，装后 dist 被清空且未重新生成。
 *
 * ## 行为
 *
 * - `ELECTRON_SKIP_BINARY_DOWNLOAD=1` 时直接跳过。CI 只做类型检查、
 *   前端构建与主进程打包，并不启动 Electron，没必要下载 ~100 MB。
 * - 否则调用 electron 自带的 install.js。它有 `isInstalled()` 快路径，
 *   已装好时几乎零开销，因此重复执行是安全的。
 *
 * ## 镜像
 *
 * 与 scripts/package-win.mjs 对 electron-builder 二进制的处理保持一致：
 * 未显式设置 `ELECTRON_MIRROR` 时默认走 npmmirror，可用环境变量覆盖。
 * 本项目网络环境下 Electron 官方下载地址不可达（实测 `fetch failed`）。
 */

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

if (process.env.ELECTRON_SKIP_BINARY_DOWNLOAD === "1") {
  console.log("[electron] ELECTRON_SKIP_BINARY_DOWNLOAD=1，跳过二进制下载");
  process.exit(0);
}

const appDir = path.resolve(import.meta.dirname, "..");
const installer = path.join(appDir, "node_modules", "electron", "install.js");

// 依赖还没装（例如只安装了部分 workspace）：不在这里制造失败。
if (!fs.existsSync(installer)) {
  console.log("[electron] node_modules/electron 不存在，跳过二进制下载");
  process.exit(0);
}

execFileSync(process.execPath, [installer], {
  cwd: path.dirname(installer),
  stdio: "inherit",
  env: {
    ...process.env,
    ELECTRON_MIRROR:
      process.env.ELECTRON_MIRROR ?? "https://npmmirror.com/mirrors/electron/",
  },
});
