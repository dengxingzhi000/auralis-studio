import { describe, expect, it } from "vitest";

import { resolveEnginePath } from "./engine-path";

const resourcesPath = "D:\\app\\resources";
const devRepoRoot = "D:\\repo";
const packaged = "D:\\app\\resources\\engine\\auralis-engine.exe";
const dev = "D:\\repo\\target\\debug\\auralis-engine.exe";

describe("resolveEnginePath", () => {
  it("uses AURALIS_ENGINE when set", () => {
    expect(
      resolveEnginePath({
        override: "D:\\custom\\engine.exe",
        resourcesPath,
        // 即使同时满足安装包条件，覆盖也必须优先
        appPath: "D:\\app\\resources\\app.asar",
        packagedExists: true,
        devRepoRoot,
      }),
    ).toBe("D:\\custom\\engine.exe");
  });

  it("treats an empty override as unset", () => {
    expect(
      resolveEnginePath({
        override: "",
        resourcesPath,
        appPath: "D:\\repo\\apps\\desktop",
        packagedExists: false,
        devRepoRoot,
      }),
    ).toBe(dev);
  });

  it("resolves the packaged path when the app runs from an asar", () => {
    expect(
      resolveEnginePath({
        resourcesPath,
        appPath: "D:\\app\\resources\\app.asar",
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
        appPath: "D:\\app\\resources\\app.asar",
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
        resourcesPath: "D:\\repo\\apps\\desktop\\node_modules\\electron\\dist\\resources",
        appPath: "D:\\repo\\apps\\desktop",
        packagedExists: false,
        devRepoRoot,
      }),
    ).toBe(dev);
  });

  it("matches .asar case-insensitively", () => {
    expect(
      resolveEnginePath({
        resourcesPath,
        appPath: "D:\\app\\resources\\APP.ASAR",
        packagedExists: false,
        devRepoRoot,
      }),
    ).toBe(packaged);
  });

  it("prefers an existing packaged engine in a non-asar layout", () => {
    expect(
      resolveEnginePath({
        resourcesPath,
        appPath: "D:\\usr\\share\\auralis",
        packagedExists: true,
        devRepoRoot,
      }),
    ).toBe(packaged);
  });
});
