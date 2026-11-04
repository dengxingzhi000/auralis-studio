# 风险接受：minimatch（GHSA-g3hv-75wg-m249）

- 状态：**已接受（accepted）**
- 接受日期：2026-11-04
- 适用版本：`apps/desktop` 的 lockfile 中所有 `minimatch` 副本（3.1.5 / 5.1.9 / 9.0.9 / 10.2.6）
- 公告：<https://github.com/advisories/GHSA-g3hv-75wg-m249>

## 结论

不升级、不 fork、不 vendored patch；在 Dependabot 中把该告警 dismiss 为
`vulnerable code is not used`。等上游把 `minimatch` 依赖推进到 `>= 11.0.1`
后，由分组更新自动关闭本条风险。

## 事实依据

1. **仅在开发期工具链**：`minimatch` 只出现在 `electron-builder` 的依赖树里
   （打包/构建用），`scope` 为 `development`，不进入安装包、不参与运行时。
   发行产物里既没有该包，也没有调用它的代码路径。
2. **没有可达的修复路径**：公告给出的最低修复版本是 `11.0.1`（跨两个大版本），
   而依赖树中的六个父包分别锁定：

   | 父包 | 版本 | 约束 |
   | --- | --- | --- |
   | `@electron/asar` | 3.4.1 | `^3.0.4` |
   | `dir-compare` | 4.2.0 | `^3.0.5` |
   | `filelist` | 1.0.6 | `^5.0.1` |
   | `glob` | 7.2.3 | `^3.1.1` |
   | `app-builder-lib` | 26.18.0 | `^10.2.5` |
   | `@electron/universal`（旧版） | 2.0.3 | `^9.0.3` |

   即使把 `electron-builder` 升到当前的 26.18.0，`app-builder-lib` 仍要求
   `^10.2.5`，semver 无法解析到 `11.0.1`，所以 Dependabot 只能报
   `security_update_not_possible`。
3. **强推 11.x 会破坏打包**：`@electron/asar 3.x`、`filelist 5.x`、`glob 7.x`
   仍按 `minimatch` v3/v5 的“模块可直接调用”语义使用；用 npm `overrides`
   强推到 11.0.1 会让 electron-builder 打包流程在运行时失败，而 CI 不跑打包，
   无法提前发现。
4. **生产依赖无漏洞**：`npm audit --omit=dev`（在 `apps/desktop`）输出
   `found 0 vulnerabilities`，说明该公告不触及发行态依赖。

## 影响评估

- 攻击面：需要攻击者控制 webpack/asar/filelist 等构建期的 glob 模式输入，
  且发生在开发者本机构建过程中，不涉及终端用户。
- 后果上限：构建期 CPU 占用（ReDoS），可被本地开发者中断。
- 与“引入不兼容的 override 导致打包失败”相比，接受该风险的成本更低。

## 复核触发条件

出现以下任一情况时重新评估（并把本条改为重新打开或直接关闭）：

- `electron-builder` / `@electron/asar` / `filelist` / `glob` 任一上游把
  `minimatch` 约束提升到 `>= 11.0.1`；
- 该公告的受影响范围或评分被上调（例如升为 high/critical）；
- 仓库引入在**运行时**使用 `minimatch` 的依赖。

## 相关记录

- Dependabot 告警：`GHSA-g3hv-75wg-m249`（dismissed，reason `not_used`）
- 触发本次评估的失败 run：`41617573479`（`security_update_not_possible`）
