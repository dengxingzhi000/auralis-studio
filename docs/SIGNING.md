# 代码签名与 SmartScreen

本仓库发布的 Windows 安装包目前**未签名**。本文说明这在用户侧的实际后果、
不签名的风险边界，以及启用签名时需要改哪些配置。发版流程本身见
[RELEASING_DESKTOP.md](./RELEASING_DESKTOP.md)。

## 当前状态（v0.2.0）

- `apps/desktop/scripts/package-win.mjs` 在调用 electron-builder 时写死
  `CSC_IDENTITY_AUTO_DISCOVERY=false`，且构建环境没有配置任何证书
  （`CSC_LINK` / `CSC_KEY_PASSWORD` 均未设置），因此产物是未签名的 NSIS 安装包；
  构建日志末尾会打印 `signing      : none (no certificate configured)`。
- 发布出去的 `latest.yml` 只有 `version`、`files[].sha512`、`size`、`releaseDate`，
  **没有 `signature` 字段**（v0.2.0 实际产物已确认）。
- `electron-updater` 更新时只按 `latest.yml` 里的 `sha512` 校验下载内容是否完整，
  **不校验发布者身份**。

## 用户会看到什么

- 首次运行 `Auralis-Studio-<version>-setup.exe`：Windows SmartScreen 显示
  「Windows 已保护你的电脑 / 未知发布者」，需要点「更多信息 → 仍要运行」。
- 从浏览器下载的文件带 Mark-of-the-Web，会加重这一提示；通过自动更新安装的
  新版本同样会触发。
- 企业环境的 AppLocker / SmartScreen 策略可能直接阻止未签名程序运行。
- 这些是未签名的**预期表现**，不代表安装包损坏。用户可对照 Release 页面公布的
  `latest.yml` 中的 `sha512` 自行校验完整性。

## 不签名的风险边界

- **完整性 ≠ 真实性**：`sha512` 只能发现下载损坏或被截断；如果攻击者能改动
  Release 资产，就能同时替换安装包与 `latest.yml` 里的 `sha512`，更新校验依然通过。
- 用户无法从系统层面确认发布者，只能依赖「从官方 GitHub Release 下载」这一约定。
- 当前的实际防线是 GitHub 账号与 HTTPS 链路安全，而不是安装包本身。
- 不签名不影响功能可用性，也不影响自动更新的下载与安装。

## 启用签名需要改什么

1. **准备证书**
   - 传统方式：组织验证（OV）或扩展验证（EV）代码签名证书，导出为 `.pfx`；
   - 或使用云签名服务（如 Azure Trusted Signing），需接入对应的签名步骤。
   - 证书与私钥**不要提交进仓库**。

2. **CI 注入 secret**
   - 在仓库 Secrets 中配置 `CSC_LINK`（`.pfx` 的 base64 或路径）与
     `CSC_KEY_PASSWORD`；
   - 在 `.github/workflows/release-desktop.yml` 的「构建安装包」步骤里把这两个
     变量传进 `env`（当前该步骤只设了 `CSC_IDENTITY_AUTO_DISCOVERY`）。

3. **放开构建脚本的硬编码**
   - `apps/desktop/scripts/package-win.mjs` 里把
     `CSC_IDENTITY_AUTO_DISCOVERY: "false"` 改为允许签名（例如仅在无证书时保留
     该开关），否则 electron-builder 不会尝试签名。
   - 本地打包时同样需要设置 `CSC_LINK` / `CSC_KEY_PASSWORD`。

4. **签名后的校验行为（需实测）**
   - 安装包会带上 Authenticode 签名，可用
     `Get-AuthenticodeSignature .\Auralis-Studio-<version>-setup.exe` 确认
     `Status = Valid`；
   - Windows 上 `electron-updater` 会结合 `app-update.yml` 的 `publisherName`
     校验更新包的发布者；`publisherName` 需要与证书主体一致。
   - OV 证书签名后 SmartScreen 仍可能需要一段时间的信誉积累，EV 证书通常更直接；
     两者在**本仓库均未实测**。
   - 启用签名后，`latest.yml` 是否新增字段取决于 electron-builder 版本，需以实际
     产物为准，不要手工编辑。

## 验证清单（启用签名时执行）

- [ ] `Get-AuthenticodeSignature` 对安装包返回 `Valid`
- [ ] 在未安装过的机器上首次运行，SmartScreen 不再提示「未知发布者」
- [ ] 从上一版本自动更新到签名版本（真实机器升级，当前尚未验证）
- [ ] Release 的 `latest.yml` 与实际安装包 `sha512` 仍然一致
