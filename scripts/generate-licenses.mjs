/**
 * 生成 THIRD_PARTY_LICENSES.md。
 *
 * 范围：
 * - Rust：从 workspace 成员出发，沿 **normal** 依赖边可达的第三方 crate，
 *   目标平台固定为 x86_64-pc-windows-msvc（发行版实际目标）。
 *   dev / build 依赖不进入发行产物，因此不计入。
 * - npm：package-lock.json 中实际安装在 apps/desktop/node_modules 的包。
 *   未安装的可选平台二进制（例如 Linux 专用的 esbuild binding）不计入。
 * - Electron：npm 包本身在清单中；打包产物另带 Electron 发行版自带的
 *   LICENSE 与 LICENSES.chromium.html（Chromium 的完整第三方许可 HTML），
 *   本文件不重复复制那个 HTML。
 *
 * 许可证文本按内容去重；表格里的 Txxxx 指向文末的全文。
 * 包内没有许可证文件的条目标记为「—」，但仍保留 SPDX 表达式。
 *
 * 用法：
 *   node scripts/generate-licenses.mjs           # 写入
 *   node scripts/generate-licenses.mjs --check   # 只校验是否最新
 */

import { execFileSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const appDir = path.join(repoRoot, "apps", "desktop");
const packageLockPath = path.join(appDir, "package-lock.json");
const outputPath = path.join(repoRoot, "THIRD_PARTY_LICENSES.md");
const checkOnly = process.argv.includes("--check");

const LICENSE_FILE_RE = /^(LICEN[CS]E|COPYING|UNLICENSE)([._-].*)?$/i;
const TARGET = "x86_64-pc-windows-msvc";

function normalizeText(text) {
  return `${text
    .replace(/^\uFEFF/, "")
    .replace(/\r\n?/g, "\n")
    .split("\n")
    .map((line) => line.replace(/[ \t]+$/u, ""))
    .join("\n")
    .trim()}\n`;
}

function hashText(text) {
  return crypto.createHash("sha256").update(text, "utf8").digest("hex");
}

function licenseFilesIn(directory) {
  if (!fs.existsSync(directory)) return [];
  return fs
    .readdirSync(directory)
    .filter((name) => LICENSE_FILE_RE.test(name))
    .sort((a, b) => a.localeCompare(b, "en"));
}

function cell(value) {
  return String(value).replaceAll("|", "\\|");
}

function comparePackages(a, b) {
  return (
    a.name.localeCompare(b.name, "en") ||
    a.version.localeCompare(b.version, "en", { numeric: true })
  );
}

// ---------------------------------------------------------------- Rust

function rustDependencies() {
  const raw = execFileSync(
    "cargo",
    [
      "metadata",
      "--format-version",
      "1",
      "--locked",
      "--filter-platform",
      TARGET,
    ],
    { cwd: repoRoot, encoding: "utf8", maxBuffer: 128 * 1024 * 1024 },
  );
  const metadata = JSON.parse(raw);
  const byId = new Map(metadata.packages.map((pkg) => [pkg.id, pkg]));
  const workspaceIds = new Set(metadata.workspace_members);
  const resolveNodes = new Map(metadata.resolve.nodes.map((node) => [node.id, node]));

  const reachable = new Set();
  const walk = (id) => {
    if (reachable.has(id)) return;
    reachable.add(id);
    const node = resolveNodes.get(id);
    if (!node) return;
    for (const dependency of node.deps) {
      const normal = dependency.dep_kinds.some((kind) => kind.kind === null);
      if (normal) walk(dependency.pkg);
    }
  };
  for (const id of workspaceIds) walk(id);

  const thirdParty = [...reachable]
    .filter((id) => !workspaceIds.has(id))
    .map((id) => byId.get(id))
    .filter(Boolean)
    .map((pkg) => ({
      ecosystem: "crate",
      name: pkg.name,
      version: pkg.version,
      license: pkg.license ?? "(license_file)",
      directory: path.dirname(pkg.manifest_path),
    }))
    .sort(comparePackages);

  const firstParty = metadata.packages
    .filter((pkg) => workspaceIds.has(pkg.id))
    .map((pkg) => ({
      name: pkg.name,
      version: pkg.version,
      license: pkg.license ?? "(license_file)",
    }))
    .sort(comparePackages);

  return { thirdParty, firstParty };
}

// ---------------------------------------------------------------- npm

function npmDependencies() {
  const lock = JSON.parse(fs.readFileSync(packageLockPath, "utf8"));
  const root = lock.packages[""] ?? {};
  const packages = [];

  for (const [key, entry] of Object.entries(lock.packages)) {
    if (!key.startsWith("node_modules/")) continue;
    const directory = path.join(appDir, key);
    if (!fs.existsSync(directory)) continue;

    let name = key.slice("node_modules/".length);
    try {
      const manifest = JSON.parse(
        fs.readFileSync(path.join(directory, "package.json"), "utf8"),
      );
      if (typeof manifest.name === "string") name = manifest.name;
    } catch {
      // 没有 package.json 就退回 lock 里的路径作为名称。
    }

    packages.push({
      ecosystem: "npm",
      name,
      version: entry.version ?? "unknown",
      license: entry.license ?? "(unknown)",
      directory,
    });
  }

  packages.sort(comparePackages);
  return {
    packages,
    firstParty: [
      {
        name: root.name ?? "@auralis/desktop",
        version: root.version ?? "0.1.0",
        license: root.license ?? "MIT OR Apache-2.0",
      },
    ],
  };
}

// ---------------------------------------------------------------- render

const { thirdParty: rustPackages, firstParty: rustFirstParty } = rustDependencies();
const { packages: npmPackages, firstParty: npmFirstParty } = npmDependencies();

const allPackages = [...rustPackages, ...npmPackages];
const texts = [];
const textByHash = new Map();

function addPackageTexts(pkg) {
  const ids = [];
  for (const file of licenseFilesIn(pkg.directory)) {
    const text = normalizeText(fs.readFileSync(path.join(pkg.directory, file), "utf8"));
    if (!text.trim()) continue;
    const hash = hashText(text);
    let entry = textByHash.get(hash);
    if (!entry) {
      entry = { id: `T${String(texts.length + 1).padStart(4, "0")}`, text };
      textByHash.set(hash, entry);
      texts.push(entry);
    }
    ids.push(entry.id);
  }
  pkg.textIds = [...new Set(ids)];
}

for (const pkg of allPackages) addPackageTexts(pkg);

const rustMissing = rustPackages.filter((pkg) => pkg.textIds.length === 0);
const npmMissing = npmPackages.filter((pkg) => pkg.textIds.length === 0);

const lines = [];
lines.push("# Third-Party Licenses");
lines.push("");
lines.push(
  "Auralis Studio itself is licensed under **MIT OR Apache-2.0** " +
    "(see [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE)).",
);
lines.push("");
lines.push(
  "This file is generated by `node scripts/generate-licenses.mjs`. Do not edit it by hand.",
);
lines.push("");
lines.push("## Scope");
lines.push("");
lines.push(
  `- **Rust**: third-party crates reachable through normal dependency edges from the ` +
    `workspace, resolved for \`${TARGET}\` by the committed \`Cargo.lock\`. ` +
    `Dev and build dependencies are excluded because they are not shipped.`,
);
lines.push(
  "- **npm**: packages actually installed under `apps/desktop/node_modules`, as pinned by " +
    "`package-lock.json`. Optional platform binaries that are not installed (for example " +
    "non-Windows esbuild/rolldown bindings) are excluded.",
);
lines.push(
  "- **Electron / Chromium**: the npm package's own license is listed below. The packaged " +
    "application also carries Electron's `LICENSE` and `LICENSES.chromium.html` from the " +
    "Electron distribution; the Chromium HTML is not reproduced here.",
);
lines.push(
  "- License files are reproduced verbatim after normalising line endings. Entries with " +
    "`—` in the text column do not ship a license file in the package; their SPDX expression " +
    "is still recorded.",
);
lines.push("");
lines.push("## First-party components");
lines.push("");
lines.push("| Component | Version | License |");
lines.push("|---|---|---|");
for (const pkg of [...rustFirstParty, ...npmFirstParty]) {
  lines.push(`| ${cell(pkg.name)} | ${cell(pkg.version)} | ${cell(pkg.license)} |`);
}
lines.push("");

function packageTable(title, packages) {
  lines.push(`## ${title} (${packages.length})`);
  lines.push("");
  lines.push("| Package | Version | License | Texts |");
  lines.push("|---|---|---|---|");
  for (const pkg of packages) {
    const ids = pkg.textIds.length > 0 ? pkg.textIds.join(", ") : "—";
    lines.push(
      `| ${cell(pkg.name)} | ${cell(pkg.version)} | ${cell(pkg.license)} | ${ids} |`,
    );
  }
  lines.push("");
}

packageTable("Rust dependencies", rustPackages);
packageTable("npm dependencies", npmPackages);

lines.push(`## License texts (${texts.length})`);
lines.push("");
lines.push(
  "Each `Txxxx` identifier is referenced from the package tables above. Identical texts are " +
    "stored once.",
);
lines.push("");
for (const entry of texts) {
  lines.push(`### ${entry.id}`);
  lines.push("");
  lines.push("````text");
  lines.push(entry.text.trimEnd());
  lines.push("````");
  lines.push("");
}

if (rustMissing.length > 0) {
  lines.push("## Packages without an included license file (Rust)");
  lines.push("");
  lines.push(
    "These crates declare an SPDX expression but do not ship a `LICENSE`/`COPYING` file in " +
      "the published crate. The expression is authoritative; no text is reproduced.",
  );
  lines.push("");
  for (const pkg of rustMissing) {
    lines.push(`- \`${pkg.name} ${pkg.version}\` — ${pkg.license}`);
  }
  lines.push("");
}

if (npmMissing.length > 0) {
  lines.push("## Packages without an included license file (npm)");
  lines.push("");
  lines.push(
    "These installed packages declare an SPDX expression but do not ship a `LICENSE`/`COPYING` " +
      "file in their published tarball. The expression is authoritative; no text is reproduced.",
  );
  lines.push("");
  for (const pkg of npmMissing) {
    lines.push(`- \`${pkg.name} ${pkg.version}\` — ${pkg.license}`);
  }
  lines.push("");
}

const output = `${lines.join("\n").trimEnd()}\n`;

if (checkOnly) {
  if (!fs.existsSync(outputPath)) {
    console.error("THIRD_PARTY_LICENSES.md is missing; run node scripts/generate-licenses.mjs");
    process.exit(1);
  }
  const current = fs.readFileSync(outputPath, "utf8").replace(/\r\n?/g, "\n");
  if (current !== output) {
    console.error(
      "THIRD_PARTY_LICENSES.md is out of date; run node scripts/generate-licenses.mjs",
    );
    process.exit(1);
  }
  console.log(
    `licenses are up to date (${allPackages.length} packages, ${texts.length} distinct texts)`,
  );
} else {
  fs.writeFileSync(outputPath, output, "utf8");
  console.log(
    `wrote THIRD_PARTY_LICENSES.md: ${rustPackages.length} Rust + ${npmPackages.length} npm packages, ` +
      `${texts.length} distinct license texts`,
  );
}
