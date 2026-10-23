/**
 * Electron 主进程 —— 取代 Tauri 作为桌面容器。
 *
 * ## 为什么换容器
 *
 * 实测（2026-10）花屏发生在 Windows 合成链路：窗口最大化收尾时连续替换
 * 合成表面，Intel iGPU @2560×1600@240Hz（PSR2/MPO 开启）出现掉帧与整屏抖动。
 * Tauri 使用系统 WebView2 Runtime，版本随机器而变；Electron 自带固定版本的
 * Chromium，把「运行时版本」从变量里去掉，同时让我们能锁定 Chromium 与
 * 图形开关（见 docs/ADR.md 的容器决策）。
 *
 * ## 开发/生产加载路径
 *
 * - 开发：`electron . --dev` → 加载 Vite dev server（127.0.0.1:5173）
 * - 生产：加载 `dist/index.html`（vite base 已设为相对路径）
 *
 * 窗口参数与 tauri.conf.json 保持一致（1600×980，最小 1100×700，深色底）。
 */

import fs from "node:fs";
import path from "node:path";

import {
  app,
  BrowserWindow,
  dialog,
  ipcMain,
  nativeTheme,
  shell,
  type OpenDialogOptions,
  type SaveDialogOptions,
} from "electron";

import { Engine } from "./engine";
import { isOpenableExternalUrl } from "./external-links";

/** Vite dev server 地址，与 vite.config.ts 的 server.port 保持一致。 */
const DEV_URL = "http://127.0.0.1:5173";

/** 渲染进程产物目录：electron/dist → 上两级是 apps/desktop/dist。 */
const RENDERER_DIST = path.join(__dirname, "..", "..", "dist");

const isDev = process.argv.includes("--dev");

/**
 * `--page=/viz-check.html` —— 仅开发用：让容器加载指定的自检页，
 * 便于同一份数据在 Tauri / Electron 两个容器之间做 A/B 对照。
 */
const pageArg = process.argv.find((a) => a.startsWith("--page="));
const devPath = pageArg ? pageArg.slice("--page=".length) : "";

/** 同一个窗口实例；Windows 上不做多窗口。 */
let mainWindow: BrowserWindow | null = null;

/** 音频引擎（Rust 子进程）。 */
const engine = new Engine();

/**
 * 运行标记文件。启动时写入，正常退出时删除。
 *
 * 下次启动如果文件还在，说明上次没走 `will-quit`（崩溃、强杀、断电），
 * 界面据此提示「会话异常结束」。文件路径在 `app.whenReady` 里确定
 * （`app.getPath` 需要 ready 之后才可靠）。
 */
let crashMarkerPath: string | null = null;
/** 本次启动是否检测到上次异常退出。只用于第一个窗口。 */
let recoveredFromCrash = false;

/** 对话框以主窗口为 owner，保证模态行为正确。 */
function openDialog(options: OpenDialogOptions) {
  return mainWindow ? dialog.showOpenDialog(mainWindow, options) : dialog.showOpenDialog(options);
}

function saveDialog(options: SaveDialogOptions) {
  return mainWindow ? dialog.showSaveDialog(mainWindow, options) : dialog.showSaveDialog(options);
}

/**
 * 渲染进程 → Rust 引擎。
 *
 * 只做转发：命令名与参数原样交给引擎，错误也原样抛回去。这里**不做**
 * 任何音频相关的计算或转换，保持「Rust 是唯一计算方」的边界。
 */
function registerIpc(): void {
  ipcMain.handle("auralis:invoke", (_event, cmd: unknown, args: unknown) => {
    if (typeof cmd !== "string" || cmd.length === 0) {
      throw new Error("invoke: command name must be a non-empty string");
    }
    return engine.request(cmd, (args as Record<string, unknown> | undefined) ?? {});
  });

  ipcMain.handle("auralis:open-file", async (_event, raw: unknown) => {
    const request = (raw ?? {}) as {
      filters?: OpenDialogOptions["filters"];
      multiple?: boolean;
      directory?: boolean;
    };

    const properties: OpenDialogOptions["properties"] = request.directory
      ? ["openDirectory"]
      : request.multiple
        ? ["openFile", "multiSelections"]
        : ["openFile"];

    const result = await openDialog({ properties, filters: request.filters });
    if (result.canceled || result.filePaths.length === 0) return null;
    return request.multiple ? result.filePaths : result.filePaths[0];
  });

  ipcMain.handle("auralis:save-file", async (_event, raw: unknown) => {
    const request = (raw ?? {}) as {
      defaultPath?: string;
      filters?: SaveDialogOptions["filters"];
    };

    const result = await saveDialog({
      defaultPath: request.defaultPath,
      filters: request.filters,
    });
    return result.canceled || !result.filePath ? null : result.filePath;
  });
}

function createWindow(): void {
  mainWindow = new BrowserWindow({
    title: "Auralis Studio",
    width: 1600,
    height: 980,
    minWidth: 1100,
    minHeight: 700,
    // 深色底：避免创建到首帧之间闪白，测量仪器不该有白闪。
    backgroundColor: "#101318",
    // 先不显示，等首帧就绪再 show，消除白屏过渡。
    show: false,
    autoHideMenuBar: true,
    webPreferences: {
      preload: path.join(__dirname, "preload.cjs"),
      // 渲染进程不碰 Node，能力只经由 preload 的窄接口暴露。
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      spellcheck: false,
    },
  });

  // 首帧就绪后再显示，避免白闪。
  mainWindow.once("ready-to-show", () => {
    mainWindow?.show();
  });

  // 外链一律交给系统浏览器，窗口本身不导航出去。
  //
  // 只放行 http(s)：`shell.openExternal` 会让系统按 URL 的协议启动对应
  // 程序，渲染层若被诱导产生一个自定义协议链接，就会变成「一条数据就能
  // 拉起本机程序」。判定与理由见 `external-links.ts`。
  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    if (isOpenableExternalUrl(url)) {
      void shell.openExternal(url);
    } else {
      console.error("[app] refused to open a non-http(s) url:", url);
    }
    return { action: "deny" };
  });
  mainWindow.webContents.on("will-navigate", (event, url) => {
    const allowed = isDev ? url.startsWith(DEV_URL) : url.startsWith("file://");
    if (!allowed) event.preventDefault();
  });

  if (isDev) {
    const url = new URL(DEV_URL + devPath);
    if (recoveredFromCrash) url.searchParams.set("recovered", "1");
    void mainWindow.loadURL(url.toString());
  } else {
    void mainWindow.loadFile(
      path.join(RENDERER_DIST, "index.html"),
      recoveredFromCrash ? { query: { recovered: "1" } } : undefined,
    );
  }
  // 恢复提示只属于本次启动的第一个窗口。
  recoveredFromCrash = false;

  mainWindow.on("closed", () => {
    mainWindow = null;
  });
}

// 深色标题栏，与 UI 一致。
nativeTheme.themeSource = "dark";

// Windows 任务栏分组标识，与安装包 identifier 保持一致。
app.setAppUserModelId("com.auralis.studio");

void app.whenReady().then(() => {
  crashMarkerPath = path.join(app.getPath("userData"), "auralis-running.lock");
  try {
    recoveredFromCrash = fs.existsSync(crashMarkerPath);
    fs.writeFileSync(crashMarkerPath, `${process.pid}\n`, "utf8");
    if (recoveredFromCrash) {
      console.error("[app] previous session did not exit cleanly; restoring it");
    }
  } catch (error) {
    // 标记文件写不了不能让应用起不来；只是失去崩溃检测。
    console.error("[app] could not maintain the crash marker:", error);
    crashMarkerPath = null;
  }

  registerIpc();
  engine.start();
  createWindow();

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

// 退出前关掉引擎：先关 stdin 让它在 Rust 侧正常收尾，超时再强杀。
// 同时删除运行标记，下一次启动因此知道上次是正常退出。
app.on("will-quit", () => {
  engine.dispose();
  if (crashMarkerPath) {
    try {
      fs.unlinkSync(crashMarkerPath);
    } catch {
      // 已经不存在（例如第一次写失败）不是错误。
    }
  }
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});
