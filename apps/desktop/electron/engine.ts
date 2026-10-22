/**
 * 音频引擎进程的宿主侧封装。
 *
 * 主进程启动 `auralis-engine` 子进程，用 stdio 上的 JSON-Lines 协议与它
 * 通信（协议细节见 `crates/auralis-engine/src/rpc.rs`）。渲染进程只看到
 * `invoke(cmd, args)`，完全不感知进程边界。
 *
 * ## 为什么放在主进程
 *
 * 渲染进程开 `sandbox: true`、`nodeIntegration: false`，**不能** spawn 进程；
 * 而且把子进程生命周期绑在渲染进程上，页面刷新就会杀掉引擎。主进程持有
 * 句柄、渲染进程只发请求，是唯一稳妥的分工。
 */

import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import path from "node:path";
import readline from "node:readline";

import { app } from "electron";

/** 引擎响应里承载二进制数据的字段名（Rust 侧以 base64 字符串发送）。 */
const BINARY_FIELDS = new Set(["spectrumDb", "waveformEnvelope", "spectrogramDb"]);

/** 一次请求的待决回调。 */
interface Pending {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
}

/** 引擎进程。 */
export class Engine {
  private proc: ChildProcessWithoutNullStreams | null = null;
  private nextId = 1;
  private readonly pending = new Map<number, Pending>();
  /** 启动失败的原因；用于把「引擎没编译」这类问题说清楚。 */
  private launchError: string | null = null;

  /** 可执行文件路径。 */
  private static executablePath(): string {
    const override = process.env.AURALIS_ENGINE;
    if (override) return override;

    // 打包后：<resources>/engine/auralis-engine.exe
    if (app.isPackaged) {
      return path.join(process.resourcesPath, "engine", "auralis-engine.exe");
    }

    // 开发：仓库根目录的 cargo 产物（__dirname = apps/desktop/electron/dist）
    return path.join(__dirname, "..", "..", "..", "..", "target", "debug", "auralis-engine.exe");
  }

  /** 启动子进程。重复调用是安全的。 */
  start(): void {
    if (this.proc) return;

    const exe = Engine.executablePath();
    let child: ChildProcessWithoutNullStreams;
    try {
      child = spawn(exe, [], { stdio: ["pipe", "pipe", "pipe"], windowsHide: true });
    } catch (error) {
      this.launchError = `无法启动音频引擎（${exe}）：${(error as Error).message}`;
      console.error("[engine]", this.launchError);
      return;
    }

    child.on("error", (error) => {
      this.launchError = `无法启动音频引擎（${exe}）：${error.message}\n请先运行 cargo build -p auralis-engine`;
      console.error("[engine]", this.launchError);
    });

    // 引擎日志走 stderr；stdout 只能是 RPC 响应。
    readline.createInterface({ input: child.stderr }).on("line", (line) => {
      console.error("[engine]", line);
    });

    readline.createInterface({ input: child.stdout }).on("line", (line) => {
      this.onLine(line);
    });

    child.on("exit", (code, signal) => {
      this.proc = null;
      const reason = `音频引擎已退出（code=${code ?? "null"}, signal=${signal ?? "null"}）`;
      console.error("[engine]", reason);
      this.failAll(new Error(reason));
    });

    this.proc = child;
  }

  /** 发送一条命令并等待响应。 */
  request(cmd: string, args: Record<string, unknown> = {}): Promise<unknown> {
    if (!this.proc) this.start();

    const proc = this.proc;
    if (!proc) {
      return Promise.reject(new Error(this.launchError ?? "音频引擎未启动"));
    }

    const id = this.nextId++;
    return new Promise<unknown>((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      proc.stdin.write(`${JSON.stringify({ id, cmd, args })}\n`, (error) => {
        if (error) {
          this.pending.delete(id);
          reject(new Error(`写入引擎失败：${error.message}`));
        }
      });
    });
  }

  /**
   * 关闭引擎：先关 stdin 让 Rust 侧排空队列并自行退出，超时再强杀。
   */
  dispose(): void {
    const proc = this.proc;
    if (!proc) return;
    this.proc = null;

    const timer = setTimeout(() => {
      if (!proc.killed) proc.kill();
    }, 2000);

    proc.once("exit", () => clearTimeout(timer));
    proc.stdin.end();
  }

  /** 处理一行响应。 */
  private onLine(line: string): void {
    let message: { id?: number; ok?: boolean; result?: unknown; error?: string };
    try {
      message = JSON.parse(line);
    } catch {
      console.error("[engine] 无法解析的响应行：", line.slice(0, 200));
      return;
    }

    const id = message.id;
    if (typeof id !== "number") return;

    const pending = this.pending.get(id);
    if (!pending) return;
    this.pending.delete(id);

    if (message.ok) {
      pending.resolve(hydrate(message.result));
    } else {
      pending.reject(new Error(message.error ?? "引擎返回了未知错误"));
    }
  }

  /** 让所有等待中的请求失败。 */
  private failAll(error: Error): void {
    for (const pending of this.pending.values()) pending.reject(error);
    this.pending.clear();
  }
}

/**
 * 把响应里的 base64 字符串还原成 `Uint8Array`。
 *
 * 字节字段名与 Rust DTO 的 camelCase 字段一一对应（见 `src/b64.rs`）。
 * 结构化克隆会把 `Uint8Array` 原样传给渲染进程，所以前端拿到的仍是
 * 二进制，不需要再做字符串解码。
 */
function hydrate(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(hydrate);
  if (value === null || typeof value !== "object") return value;

  const source = value as Record<string, unknown>;
  const target: Record<string, unknown> = {};

  for (const [key, entry] of Object.entries(source)) {
    target[key] =
      BINARY_FIELDS.has(key) && typeof entry === "string"
        ? new Uint8Array(Buffer.from(entry, "base64"))
        : hydrate(entry);
  }

  return target;
}
