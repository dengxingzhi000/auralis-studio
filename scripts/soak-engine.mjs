/**
 * 引擎 soak 测试：循环执行分析、视口声谱图与导出，监控引擎 RSS。
 *
 * 目的不是替代 8 小时实时采集测试，而是回答一个具体问题：
 * 「反复走同一条离线分析路径，内存会不会持续增长、结果会不会漂移」。
 * 8 小时实时采集仍未验证，报告里必须如实区分。
 *
 * 用法：
 *   node scripts/soak-engine.mjs [--minutes 10] [--sample-seconds 10]
 *
 * 退出码：
 *   0  完成且无失败、内存增长在阈值内
 *   1  引擎/RPC/结果错误，或 RSS 超过阈值
 */

import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import readline from "node:readline";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const runRoot = path.join(repoRoot, ".devrun", "soak");

function parseArgs(argv) {
  const options = { minutes: 10, sampleSeconds: 10 };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--minutes") options.minutes = Number(argv[++i]);
    else if (arg === "--sample-seconds") options.sampleSeconds = Number(argv[++i]);
    else if (arg === "--help" || arg === "-h") {
      console.log("usage: node scripts/soak-engine.mjs [--minutes 10] [--sample-seconds 10]");
      process.exit(0);
    } else {
      throw new Error(`unknown argument: ${arg}`);
    }
  }
  if (!Number.isFinite(options.minutes) || options.minutes <= 0) {
    throw new Error("--minutes must be > 0");
  }
  if (!Number.isFinite(options.sampleSeconds) || options.sampleSeconds <= 0) {
    throw new Error("--sample-seconds must be > 0");
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
    throw new Error("auralis-engine not found; run `cargo build -p auralis-engine --release`");
  }
  return found;
}

/** 生成确定性的 16-bit PCM 立体声 WAV。 */
function writeFixture(file) {
  const sampleRate = 48_000;
  const seconds = 5;
  const channels = 2;
  const frames = sampleRate * seconds;
  const dataBytes = frames * channels * 2;
  const buffer = Buffer.alloc(44 + dataBytes);

  buffer.write("RIFF", 0, "ascii");
  buffer.writeUInt32LE(36 + dataBytes, 4);
  buffer.write("WAVE", 8, "ascii");
  buffer.write("fmt ", 12, "ascii");
  buffer.writeUInt32LE(16, 16);
  buffer.writeUInt16LE(1, 20); // PCM
  buffer.writeUInt16LE(channels, 22);
  buffer.writeUInt32LE(sampleRate, 24);
  buffer.writeUInt32LE(sampleRate * channels * 2, 28);
  buffer.writeUInt16LE(channels * 2, 32);
  buffer.writeUInt16LE(16, 34);
  buffer.write("data", 36, "ascii");
  buffer.writeUInt32LE(dataBytes, 40);

  let offset = 44;
  for (let frame = 0; frame < frames; frame += 1) {
    const t = frame / sampleRate;
    const left = 0.5 * Math.sin(2 * Math.PI * 1000 * t);
    const right =
      0.25 * Math.sin(2 * Math.PI * 1000 * t + 0.3) +
      0.1 * Math.sin(2 * Math.PI * 2000 * t);
    for (const value of [left, right]) {
      // 峰值 0.6，留出余量，避免测试样本本身超量程。
      buffer.writeInt16LE(Math.round(value * 32767), offset);
      offset += 2;
    }
  }

  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, buffer);
  return { path: file, durationSeconds: seconds, sampleRate, channels };
}

function engineRssBytes(pid) {
  if (process.platform === "win32") {
    const output = execFileSync(
      "powershell",
      ["-NoProfile", "-Command", `(Get-Process -Id ${pid} -ErrorAction Stop).WorkingSet64`],
      { encoding: "utf8" },
    ).trim();
    return Number(output);
  }
  const status = fs.readFileSync(`/proc/${pid}/status`, "utf8");
  const match = /^VmRSS:\s+(\d+)\s+kB$/m.exec(status);
  if (!match) throw new Error("cannot read RSS from /proc");
  return Number(match[1]) * 1024;
}

class EngineClient {
  constructor(exe) {
    this.nextId = 1;
    this.pending = new Map();
    this.stderrLines = [];
    this.proc = spawn(exe, [], { stdio: ["pipe", "pipe", "pipe"], windowsHide: true });
    this.pid = this.proc.pid;

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
      if (line.includes(" ERROR ")) console.error(`[engine] ${line}`);
    });

    this.proc.on("error", (error) => this.failAll(new Error(`cannot start engine: ${error.message}`)));
    this.proc.on("exit", (code) => {
      this.exited = true;
      this.failAll(new Error(`engine exited (code=${code ?? "null"})`));
    });
  }

  request(cmd, args = {}, timeoutMs = 120_000) {
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
const fixture = writeFixture(path.join(runRoot, "fixture-5s-stereo.wav"));
const exportCsv = path.join(runRoot, "out.csv");
const exportJson = path.join(runRoot, "out.json");
const deadline = Date.now() + options.minutes * 60_000;

let client;
let iterations = 0;
let failures = 0;
let expectedPeak = null;
let expectedRms = null;
let lastSampleAt = 0;
let baselineRss = null;
let peakRss = 0;
let lastRss = 0;
const samples = [];

try {
  const exe = enginePath();
  console.log(`engine: ${exe}`);
  console.log(
    `fixture: ${fixture.path} (${fixture.durationSeconds}s, ${fixture.channels}ch, ${fixture.sampleRate}Hz)`,
  );
  client = new EngineClient(exe);
  await client.request("app_info");
  await client.request("probe_file", { path: fixture.path });

  // 预热：让分配器、解码器与线程池进入稳定状态，再取基线。
  for (let i = 0; i < 3; i += 1) {
    const result = await client.request("analyze_file", {
      path: fixture.path,
      envelopePoints: 2048,
    });
    if (i === 0) {
      expectedPeak = result.peakDbfs;
      expectedRms = result.rmsDbfs;
    }
  }
  await client.request("spectrogram_view", {
    path: fixture.path,
    startSeconds: 0,
    windowSeconds: 3,
  });

  baselineRss = engineRssBytes(client.pid);
  peakRss = baselineRss;
  lastRss = baselineRss;
  console.log(`baseline RSS: ${(baselineRss / 1024 / 1024).toFixed(1)} MiB`);

  const startedAt = Date.now();
  while (Date.now() < deadline) {
    try {
      const analysis = await client.request("analyze_file", {
        path: fixture.path,
        envelopePoints: 2048,
      });
      const peakDrift = Math.abs(analysis.peakDbfs - expectedPeak);
      const rmsDrift = Math.abs(analysis.rmsDbfs - expectedRms);
      if (peakDrift > 1e-6 || rmsDrift > 1e-6) {
        throw new Error(
          `analysis drifted: peak ${analysis.peakDbfs} (expected ${expectedPeak}, Δ${peakDrift}), ` +
            `rms ${analysis.rmsDbfs} (expected ${expectedRms}, Δ${rmsDrift})`,
        );
      }

      const start = (iterations * 1.7) % 2;
      await client.request("spectrogram_view", {
        path: fixture.path,
        startSeconds: start,
        windowSeconds: 3,
      });

      if (iterations % 2 === 0) {
        await client.request("export_analysis", {
          source: fixture.path,
          destination: iterations % 4 === 0 ? exportCsv : exportJson,
          format: iterations % 4 === 0 ? "csv" : "json",
          envelopePoints: 2048,
        });
        const destination = iterations % 4 === 0 ? exportCsv : exportJson;
        if (!fs.existsSync(destination) || fs.statSync(destination).size === 0) {
          throw new Error(`export produced no data: ${destination}`);
        }
        const leftovers = fs
          .readdirSync(runRoot)
          .filter((name) => name.endsWith(".part"));
        if (leftovers.length > 0) {
          throw new Error(`atomic export left temporary files: ${leftovers.join(", ")}`);
        }
      }

      iterations += 1;

      const now = Date.now();
      if (now - lastSampleAt >= options.sampleSeconds * 1000) {
        lastSampleAt = now;
        lastRss = engineRssBytes(client.pid);
        peakRss = Math.max(peakRss, lastRss);
        samples.push(lastRss);
        const growth = (lastRss - baselineRss) / 1024 / 1024;
        console.log(
          `+${((now - startedAt) / 60_000).toFixed(1)} min | iterations ${iterations} | ` +
            `RSS ${(lastRss / 1024 / 1024).toFixed(1)} MiB (${growth >= 0 ? "+" : ""}${growth.toFixed(1)})`,
        );
      }
    } catch (error) {
      failures += 1;
      console.error(`iteration ${iterations} failed: ${error.message}`);
      if (failures >= 3) throw new Error("three failures; stopping soak");
    }
  }

  lastRss = engineRssBytes(client.pid);
  peakRss = Math.max(peakRss, lastRss);
  await client.close();

  const growth = (lastRss - baselineRss) / 1024 / 1024;
  const peakGrowth = (peakRss - baselineRss) / 1024 / 1024;
  const engineWarnings = client.stderrLines.filter((line) => line.includes(" WARN ")).length;
  const engineErrors = client.stderrLines.filter((line) => line.includes(" ERROR ")).length;

  console.log("");
  console.log("--- soak summary ---");
  console.log(`duration          : ${((Date.now() - startedAt) / 60_000).toFixed(1)} min (requested ${options.minutes.toFixed(1)})`);
  console.log(`iterations        : ${iterations}`);
  console.log(`failures          : ${failures}`);
  console.log(`baseline RSS      : ${(baselineRss / 1024 / 1024).toFixed(1)} MiB`);
  console.log(`final RSS         : ${(lastRss / 1024 / 1024).toFixed(1)} MiB (${growth >= 0 ? "+" : ""}${growth.toFixed(1)} MiB)`);
  console.log(`peak RSS          : ${(peakRss / 1024 / 1024).toFixed(1)} MiB (+${peakGrowth.toFixed(1)} MiB)`);
  console.log(`engine warnings   : ${engineWarnings}; errors: ${engineErrors}`);
  console.log(`exports           : ${exportCsv} / ${exportJson}`);

  if (failures > 0) throw new Error(`${failures} iteration(s) failed`);
  if (growth > 32) throw new Error(`RSS grew by ${growth.toFixed(1)} MiB (> 32 MiB)`);
  if (peakGrowth > 96) throw new Error(`peak RSS grew by ${peakGrowth.toFixed(1)} MiB (> 96 MiB)`);
  console.log("PASS: no failures and no sustained memory growth beyond the threshold");
} catch (error) {
  console.error(`FAIL: ${error.message}`);
  if (client) await client.close().catch(() => undefined);
  process.exitCode = 1;
}
