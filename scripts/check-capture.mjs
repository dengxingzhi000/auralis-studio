/**
 * 真实声卡采集检查（Windows，需要可用的 WASAPI 输入设备）。
 *
 * 这个脚本补的是 CI 覆盖不到的一层：设备枚举、CPAL 流、回调/环形缓冲、
 * 实时分析线程与最终快照。CI 只跑无设备路径和纯逻辑测试。
 *
 * 用法：
 *   node scripts/check-capture.mjs [--seconds 6] [--device-index 0]
 *
 * 退出码：
 *   0  采集成功，或本机没有输入设备（明确打印 SKIP，不把「无声卡」当失败）
 *   1  引擎缺失、RPC 失败、启动失败、或采集期间没有处理任何帧
 *
 * 输出包含每个快照的 RPC 往返延迟，供 ADR 的高频通道决策引用。
 * 注意：此延迟是「Node 宿主 ↔ 引擎 stdio」这一段，不含 Electron IPC 与绘制。
 */

import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import readline from "node:readline";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function parseArgs(argv) {
  const options = { seconds: 6, deviceIndex: 0 };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--seconds") options.seconds = Number(argv[++i]);
    else if (arg === "--device-index") options.deviceIndex = Number(argv[++i]);
    else if (arg === "--help" || arg === "-h") {
      console.log("usage: node scripts/check-capture.mjs [--seconds 6] [--device-index 0]");
      process.exit(0);
    } else {
      throw new Error(`unknown argument: ${arg}`);
    }
  }
  if (!Number.isFinite(options.seconds) || options.seconds < 2) {
    throw new Error("--seconds must be a number >= 2");
  }
  if (!Number.isInteger(options.deviceIndex) || options.deviceIndex < 0) {
    throw new Error("--device-index must be a non-negative integer");
  }
  return options;
}

function enginePath() {
  if (process.env.AURALIS_ENGINE) return process.env.AURALIS_ENGINE;
  const candidates = [
    path.join(repoRoot, "target", "release", "auralis-engine.exe"),
    path.join(repoRoot, "target", "debug", "auralis-engine.exe"),
    path.join(repoRoot, "target", "release", "auralis-engine"),
    path.join(repoRoot, "target", "debug", "auralis-engine"),
  ];
  const found = candidates.find((candidate) => fs.existsSync(candidate));
  if (!found) {
    throw new Error(
      "auralis-engine not found; run `cargo build -p auralis-engine --release` first",
    );
  }
  return found;
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function percentile(sorted, fraction) {
  if (sorted.length === 0) return 0;
  const index = Math.min(sorted.length - 1, Math.floor(sorted.length * fraction));
  return sorted[index];
}

class EngineClient {
  constructor(exe) {
    this.nextId = 1;
    this.pending = new Map();
    this.stderrLines = [];
    this.proc = spawn(exe, [], { stdio: ["pipe", "pipe", "pipe"], windowsHide: true });

    readline.createInterface({ input: this.proc.stdout }).on("line", (line) => {
      let message;
      try {
        message = JSON.parse(line);
      } catch {
        console.error(`[engine] non-JSON stdout: ${line.slice(0, 160)}`);
        return;
      }
      const pending = this.pending.get(message.id);
      if (!pending) return;
      this.pending.delete(message.id);
      clearTimeout(pending.timer);
      if (message.ok) pending.resolve(message.result);
      else pending.reject(new Error(message.error ?? "unknown engine error"));
    });

    readline.createInterface({ input: this.proc.stderr }).on("line", (line) => {
      this.stderrLines.push(line);
      if (line.includes(" WARN ") || line.includes(" ERROR ")) {
        console.error(`[engine] ${line}`);
      }
    });

    this.proc.on("error", (error) => this.failAll(new Error(`cannot start engine: ${error.message}`)));
    this.proc.on("exit", (code) => {
      this.exited = true;
      this.failAll(new Error(`engine exited (code=${code ?? "null"})`));
    });
  }

  request(cmd, args = {}, timeoutMs = 15_000) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`engine request \`${cmd}\` timed out after ${timeoutMs} ms`));
      }, timeoutMs);
      this.pending.set(id, { resolve, reject, timer });
      this.proc.stdin.write(`${JSON.stringify({ id, cmd, args })}\n`);
    });
  }

  failAll(error) {
    for (const { reject, timer } of this.pending.values()) {
      clearTimeout(timer);
      reject(error);
    }
    this.pending.clear();
  }

  async close() {
    if (this.exited) return;
    this.proc.stdin.end();
    await new Promise((resolve) => {
      const timer = setTimeout(() => {
        this.proc.kill();
        resolve();
      }, 2_000);
      this.proc.once("exit", () => {
        clearTimeout(timer);
        resolve();
      });
    });
  }
}

const options = parseArgs(process.argv.slice(2));
let client;

try {
  const exe = enginePath();
  console.log(`engine: ${exe}`);
  client = new EngineClient(exe);

  const appInfo = await client.request("app_info");
  console.log(`version: ${appInfo.version} (${appInfo.algorithmVersion})`);

  const devices = await client.request("input_devices");
  if (!Array.isArray(devices) || devices.length === 0) {
    console.log("SKIP: no input devices on this machine (this is not a failure)");
    await client.close();
    process.exit(0);
  }
  for (const [index, device] of devices.entries()) {
    console.log(
      `device[${index}]: ${device.name} | ${device.inputChannels} ch | sample rates ${device.supportedSampleRates.join(", ") || "unreported"}`,
    );
  }
  if (options.deviceIndex >= devices.length) {
    throw new Error(`--device-index ${options.deviceIndex} is out of range`);
  }
  const device = devices[options.deviceIndex];

  const started = await client.request("start_realtime", { deviceId: device.id });
  console.log(
    `capture started: ${started.sampleRate} Hz, ${started.channels} ch, running=${started.running}`,
  );

  const latenciesMs = [];
  const peaks = [];
  const rms = [];
  let payloadBytes = 0;
  let snapshots = 0;
  let last = null;
  const startedAt = process.hrtime.bigint();
  const deadline = Date.now() + options.seconds * 1000;

  while (Date.now() < deadline) {
    await sleep(33);
    const t0 = process.hrtime.bigint();
    const snap = await client.request("realtime_snapshot");
    const t1 = process.hrtime.bigint();
    latenciesMs.push(Number(t1 - t0) / 1e6);
    snapshots += 1;
    last = snap;
    payloadBytes = Buffer.from(snap.spectrumDb, "base64").length;
    if (typeof snap.peakDbfs === "number") peaks.push(snap.peakDbfs);
    if (typeof snap.rmsDbfs === "number") rms.push(snap.rmsDbfs);
  }

  const elapsedSeconds = Number(process.hrtime.bigint() - startedAt) / 1e9;
  const status = await client.request("capture_status_cmd");
  await client.request("stop_realtime");
  await client.close();

  latenciesMs.sort((a, b) => a - b);
  const expectedFrames = elapsedSeconds * last.sampleRate;
  const ratio = expectedFrames > 0 ? last.processedFrames / expectedFrames : 0;
  const warnings = client.stderrLines.filter((line) => line.includes(" WARN ")).length;
  const errors = client.stderrLines.filter((line) => line.includes(" ERROR ")).length;

  console.log("");
  console.log("--- capture summary ---");
  console.log(`elapsed          : ${elapsedSeconds.toFixed(2)} s`);
  console.log(`processed frames : ${status.processedFrames} (${ratio.toFixed(3)} x realtime)`);
  console.log(`dropped frames   : ${status.droppedFrames}`);
  console.log(`snapshot overwrites: ${status.snapshotOverwrites} (UI missed generations)`);
  console.log(`snapshots polled : ${snapshots}`);
  console.log(`snapshot payload : ${payloadBytes} bytes (${last.binHz.toFixed(3)} Hz/bin)`);
  console.log(
    `rpc latency      : p50 ${percentile(latenciesMs, 0.5).toFixed(2)} ms | p95 ${percentile(latenciesMs, 0.95).toFixed(2)} ms | max ${latenciesMs.at(-1).toFixed(2)} ms`,
  );
  console.log(
    `peak dBFS range  : ${peaks.length ? `${Math.min(...peaks).toFixed(2)} … ${Math.max(...peaks).toFixed(2)}` : "no non-silent snapshot"}`,
  );
  console.log(
    `rms dBFS range   : ${rms.length ? `${Math.min(...rms).toFixed(2)} … ${Math.max(...rms).toFixed(2)}` : "no non-silent snapshot"}`,
  );
  console.log(`engine warnings  : ${warnings}; errors: ${errors}`);

  if (!status.running || status.processedFrames === 0 || ratio < 0.5) {
    throw new Error("capture did not process frames at the expected rate");
  }
  console.log("PASS: real capture path is working");
} catch (error) {
  console.error(`FAIL: ${error.message}`);
  if (client) await client.close().catch(() => undefined);
  process.exitCode = 1;
}
