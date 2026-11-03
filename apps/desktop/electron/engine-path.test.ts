import path from "node:path";

import { describe, expect, it } from "vitest";

import { resolveEnginePath } from "./engine-path";

// 所有路径都用 path.join 构造：测试跑在 Windows 本机与 Linux CI 上，
// 硬编码反斜杠会在 Linux 下与函数内部的 path.join 结果不一致。
const resourcesPath = path.join("app", "resources");
const devRepoRoot = path.join("repo");
const asarAppPath = path.join(resourcesPath, "app.asar");
const packaged = path.join(resourcesPath, "engine", "auralis-engine.exe");
const dev = path.join(devRepoRoot, "target", "debug", "auralis-engine.exe");
const custom = path.join("custom", "engine.exe");

describe("resolveEnginePath", () => {
  it("uses AURALIS_ENGINE when set", () => {
    expect(
      resolveEnginePath({
        override: custom,
        resourcesPath,
        // 即使同时满足安装包条件，覆盖也必须优先
        appPath: asarAppPath,
        packagedExists: true,
        devRepoRoot,
      }),
    ).toBe(custom);
  });

  it("treats an empty override as unset", () => {
    expect(
      resolveEnginePath({
        override: "",
        resourcesPath,
        appPath: path.join("repo", "apps", "desktop"),
        packagedExists: false,
        devRepoRoot,
      }),
    ).toBe(dev);
  });

  it("resolves the packaged path when the app runs from an asar", () => {
    expect(
      resolveEnginePath({
        resourcesPath,
        appPath: asarAppPath,
        packagedExists: true,
        devRepoRoot,
      }),
    ).toBe(packaged);
  });

  it("still reports the packaged path when the engine file is missing", () => {
    // 安装后引擎被误删：报错应指向用户机器上的期望位置，
    // 而不是开发机的 target/debug。
    expect(
      resolveEnginePath({
        resourcesPath,
        appPath: asarAppPath,
        packagedExists: false,
        devRepoRoot,
      }),
    ).toBe(packaged);
  });

  it("falls back to the dev build when running from source", () => {
    // 这是 Electron 45 的回归点：源码运行时 app.isPackaged 也是 true，
    // 但 app.getAppPath() 指向项目目录，不以 .asar 结尾。
    expect(
      resolveEnginePath({
        resourcesPath: path.join("repo", "apps", "desktop", "node_modules", "electron", "dist", "resources"),
        appPath: path.join("repo", "apps", "desktop"),
        packagedExists: false,
        devRepoRoot,
      }),
    ).toBe(dev);
  });

  it("matches .asar case-insensitively", () => {
    expect(
      resolveEnginePath({
        resourcesPath,
        appPath: path.join(resourcesPath, "APP.ASAR"),
        packagedExists: false,
        devRepoRoot,
      }),
    ).toBe(packaged);
  });

  it("prefers an existing packaged engine in a non-asar layout", () => {
    expect(
      resolveEnginePath({
        resourcesPath,
        appPath: path.join("usr", "share", "auralis"),
        packagedExists: true,
        devRepoRoot,
      }),
    ).toBe(packaged);
  });
});
