/**
 * Windows 发布包构建：Rust 引擎 + Electron 运行时 + NSIS 安装包。
 *
 * 产物（apps/desktop/release/）：
 * - win-unpacked/                    免安装目录，用于发布前自检
 * - Auralis-Studio-<version>-setup.exe  NSIS 安装包
 *
 * 关键约束（ADR-019）：
 * - 引擎必须位于 <resources>/engine/auralis-engine.exe，electron/engine.ts 依赖该路径；
 * - 许可证清单、项目许可证与用户手册随包分发；
 * - 安装包**未签名**——没有配置代码签名证书，也不自动发现本机证书。
 */

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const appDir = path.resolve(import.meta.dirname, "..");
const repoRoot = path.resolve(appDir, "..", "..");
const packageJson = JSON.parse(fs.readFileSync(path.join(appDir, "package.json"), "utf8"));
const version = packageJson.version;

/**
 * 输出目录。
 *
 * 默认 `release`（与 package.json 的 build.directories.output 一致）。
 * AURALIS_PACKAGE_OUTPUT 可以改写到别处——**运行中的安装目录会被锁**，
 * 想在不打断用户当前实例的前提下重打包时用它。
 */
const outputDir = process.env.AURALIS_PACKAGE_OUTPUT || "release";

function run(command, args, options = {}) {
  console.log(`\n$ ${command} ${args.join(" ")}`);
  execFileSync(command, args, { cwd: appDir, stdio: "inherit", ...options });
}

function requireFile(file, hint) {
  if (!fs.existsSync(file)) {
    throw new Error(`missing ${file}${hint ? ` — ${hint}` : ""}`);
  }
  return file;
}

if (process.platform !== "win32") {
  throw new Error("package:win must run on Windows (the packaged engine is a Windows PE binary)");
}

// 1) Rust 引擎（release + locked，与 CI 一致）
run("cargo", ["build", "-p", "auralis-engine", "--release", "--locked"], { cwd: repoRoot });

// 2) 许可证清单必须与当前 lockfile 一致
run(process.execPath, [path.join(repoRoot, "scripts", "generate-licenses.mjs")], {
  cwd: repoRoot,
});

// 3) 图标（从 PNG 生成 ICO）
run(process.execPath, [path.join(appDir, "scripts", "make-icon.mjs")]);

// 4) 组装 extraResources：只放确定需要的文件
const resourceRoot = path.join(appDir, "build", "package-resources");
fs.rmSync(resourceRoot, { recursive: true, force: true });
for (const directory of ["engine", "licenses", "docs"]) {
  fs.mkdirSync(path.join(resourceRoot, directory), { recursive: true });
}

fs.copyFileSync(
  path.join(repoRoot, "target", "release", "auralis-engine.exe"),
  path.join(resourceRoot, "engine", "auralis-engine.exe"),
);
for (const name of ["THIRD_PARTY_LICENSES.md", "LICENSE-MIT", "LICENSE-APACHE"]) {
  fs.copyFileSync(path.join(repoRoot, name), path.join(resourceRoot, "licenses", name));
}
fs.copyFileSync(
  requireFile(
    path.join(repoRoot, "docs", "USER_MANUAL.md"),
    "write the user manual before packaging",
  ),
  path.join(resourceRoot, "docs", "USER_MANUAL.md"),
);

// 5) 渲染进程与主进程
run(process.execPath, [path.join(appDir, "node_modules", "typescript", "bin", "tsc"), "--noEmit"]);
run(process.execPath, [path.join(appDir, "node_modules", "vite", "bin", "vite.js"), "build"]);
run(process.execPath, [path.join(appDir, "scripts", "build-electron.mjs")]);

// 6) electron-builder（不会自动签名）
//    - Electron 运行时直接使用 node_modules/electron/dist，不再重复下载；
//    - `--publish never`：本脚本只产出安装包，绝不推送 GitHub Release。
//      不加这个参数时 electron-builder 会按 package.json 的 repository
//      推断 provider，并在 tag 构建或 CI 上真的尝试上传。
//    - NSIS / winCodeSign 等工具默认从 GitHub 下载，在本项目的网络环境下
//      不可靠，默认走 npmmirror；可用环境变量覆盖。
const builderCli = requireFile(
  path.join(appDir, "node_modules", "electron-builder", "cli.js"),
  "run npm install first",
);
run(
  process.execPath,
  [
    builderCli,
    "--win",
    "nsis",
    "--x64",
    "--publish",
    "never",
    `--config.directories.output=${outputDir}`,
  ],
  {
    env: {
      ...process.env,
      CSC_IDENTITY_AUTO_DISCOVERY: "false",
      ELECTRON_BUILDER_BINARIES_MIRROR:
        process.env.ELECTRON_BUILDER_BINARIES_MIRROR ??
        "https://npmmirror.com/mirrors/electron-builder-binaries/",
    },
  },
);

// 7) 产物自检：路径与分发内容必须真的存在
const unpacked = path.join(appDir, outputDir, "win-unpacked");
const expected = [
  path.join(unpacked, "Auralis Studio.exe"),
  path.join(unpacked, "resources", "engine", "auralis-engine.exe"),
  path.join(unpacked, "resources", "licenses", "THIRD_PARTY_LICENSES.md"),
  path.join(unpacked, "resources", "licenses", "LICENSE-MIT"),
  path.join(unpacked, "resources", "licenses", "LICENSE-APACHE"),
  path.join(unpacked, "resources", "docs", "USER_MANUAL.md"),
  path.join(unpacked, "LICENSES.chromium.html"),
  path.join(appDir, outputDir, `Auralis-Studio-${version}-setup.exe`),
];
for (const file of expected) requireFile(file, "electron-builder output is incomplete");

const uncompressed = fs.statSync(path.join(unpacked, "resources", "engine", "auralis-engine.exe")).size;
console.log("\n--- package summary ---");
console.log(`version      : ${version}`);
console.log(`output dir   : ${path.join(appDir, outputDir)}`);
console.log(`unpacked     : ${unpacked}`);
console.log(`engine size  : ${(uncompressed / 1024 / 1024).toFixed(2)} MiB`);
console.log(`installer    : ${path.join(appDir, outputDir, `Auralis-Studio-${version}-setup.exe`)}`);
console.log("signing      : none (no certificate configured)");
