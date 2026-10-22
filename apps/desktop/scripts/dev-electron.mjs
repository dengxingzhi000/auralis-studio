/**
 * 一条命令启动开发环境：Vite dev server + Electron 容器。
 *
 * 不用 concurrently 之类的依赖：这里只需要「先等端口就绪，再起 Electron」
 * 两步，自己写反而少一个依赖，也方便在退出时精确杀掉子进程树。
 */

import { spawn } from "node:child_process";
import path from "node:path";

import electron from "electron";

const appDir = path.resolve(import.meta.dirname, "..");
const DEV_URL = "http://127.0.0.1:5173";
const viteBin = path.join(appDir, "node_modules", "vite", "bin", "vite.js");

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** 轮询 dev server；Vite 启动通常在 1 秒内。 */
async function waitForVite(timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(DEV_URL, { method: "HEAD" });
      if (response.ok) return;
    } catch {
      // 还没监听，继续等
    }
    await sleep(200);
  }
  throw new Error(`Vite did not start on ${DEV_URL} within ${timeoutMs} ms`);
}

const vite = spawn(process.execPath, [viteBin], {
  cwd: appDir,
  stdio: "inherit",
  env: { ...process.env, BROWSER: "none" },
});

let electronProc = null;
let shuttingDown = false;

function shutdown(code) {
  if (shuttingDown) return;
  shuttingDown = true;
  electronProc?.kill();
  vite.kill();
  process.exit(code);
}

vite.on("exit", (code) => {
  if (!shuttingDown) {
    console.error(`[dev] Vite exited with code ${code ?? "null"}`);
    shutdown(code ?? 1);
  }
});

process.on("SIGINT", () => shutdown(0));
process.on("SIGTERM", () => shutdown(0));

try {
  await waitForVite();
  console.log(`[dev] Vite ready at ${DEV_URL}, starting Electron…`);

  electronProc = spawn(electron, [".", "--dev"], { cwd: appDir, stdio: "inherit" });

  electronProc.on("exit", (code) => {
    console.log(`[dev] Electron exited with code ${code ?? "null"}`);
    shutdown(code ?? 0);
  });
} catch (error) {
  console.error(`[dev] ${error.message}`);
  shutdown(1);
}
