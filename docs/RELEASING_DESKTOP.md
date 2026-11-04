# 发布 Windows 桌面版

本流程只处理**桌面安装包**（NSIS）。`v0.1.0` 已发布，不要重跑或覆盖它的资产。

## 版本策略

- 仅修 bug / 无行为变化的重构（patch）：`0.2.0 → 0.2.1`；
- 新功能、SDK/API 变更、引擎行为变化（minor）：`0.2.x → 0.3.0`；
- 破坏性变更（major）：`1.0.0` 起才用，或在 0.x 阶段明确标注「breaking」并在
  `CHANGELOG` 写迁移说明。
- 当前处于 `0.2.x`：按 [Semantic Versioning](https://semver.org/) 的 0.x 约定，
  **minor 位即视为可能破坏**，升级前先读 `CHANGELOG`。

## 前置

- 版本号先改：`apps/desktop/package.json` 的 `version`（安装包名与 `latest.yml` 的
  `version` 都来自这里），`Cargo.toml` 的 `[workspace.package]` 与 `Cargo.lock`、
  `apps/desktop/package-lock.json` 必须同步。
- 签名仍为空：构建脚本写死 `CSC_IDENTITY_AUTO_DISCOVERY=false`，
  产物是**未签名**的 `setup.exe`，Windows SmartScreen 会提示「未知发布者」。
  分发前应补 OV 证书（本轮不做，仅记录）。
- 更新校验依赖 `latest.yml` 里的 `sha512`：它由 electron-builder 在打包时对安装包
  计算，`electron-updater` 下载后按该值校验完整性。**不要手工编辑 `latest.yml`**。

## 发一版

1. **改版本并合入**（PR 走常规流程）：

   ```powershell
   # apps/desktop/package.json -> "version": "0.2.0"
   ```

2. **打 tag 并推送**（唯一会触发发布流水线的动作）：

   ```powershell
   git tag v0.2.0
   git push origin v0.2.0
   ```

   `.github/workflows/release-desktop.yml` 会：
   - 跑 `npm run package:win`（cargo 引擎 → 许可证 → icon → tsc/vite → esbuild → electron-builder）；
   - 校验产物存在（`setup.exe` + `latest.yml` + `*.blockmap`）；
   - 上传到同名 GitHub Release（不存在则用 `gh release create` 创建）。

3. **校验 Release 资产**（三项齐全才算成功）：

   ```powershell
   gh release view v0.2.0 --json assets --jq '.assets[].name'
   # Auralis-Studio-0.2.0-setup.exe
   # Auralis-Studio-0.2.0-setup.exe.blockmap
   # latest.yml
   ```

4. **核对 `latest.yml` 关键字段**：

   ```powershell
   gh release download v0.2.0 --pattern latest.yml --dir $env:TEMP\auralis-latest
   Get-Content $env:TEMP\auralis-latest\latest.yml
   ```

   必须满足：`version` 与新 tag 一致；`files[0].url` 指向 `setup.exe`；
   `files[0].sha512` 非空且与 `sha512sum` 计算结果一致。

   ```powershell
   # sha512 复核（base64 形式比较）
   $f = "$env:TEMP\auralis-latest\Auralis-Studio-0.2.0-setup.exe"
   (Get-FileHash $f -Algorithm SHA512).Hash
   ```

5. **不推 tag 时的演练**：用 `workflow_dispatch` 手动触发同一 workflow，
   只构建并上传 Actions artifact，不会写 Release。

## 客户端行为

- 打包态应用启动 5 秒后静默检查更新（不阻塞 UI）；
- 有新版本时后台下载，完成后弹系统通知，点击才重启安装；
- 未签名安装包仍可更新，但 `electron-updater` 只校验 `latest.yml` 的 `sha512`，
  不校验发布者身份 —— 这也是需要补签名的原因。
