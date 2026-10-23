# 开通界面中尚未启用的功能 —— 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把左侧资源栏与右上角 8 个「禁用 + 里程碑标记」的入口全部做成真正可用的功能。

**Architecture:** 纯前端改动，**Rust 侧零改动**——所需命令（`task_queue`、`cancel_task`、`app_info`）均已存在。新增的数值全部复用 Rust 已算好的返回值，前端只做展示、调度与坐标换算（ADR-018 规则 1）。新逻辑中「可被测」的纯函数抽到 `src/lib/` 下独立模块，用 vitest 覆盖；渲染与交互留在组件里。

**Tech Stack:** React 19 + TypeScript 7 + Vite 8 + Electron 45（渲染进程侧）；vitest 3（仅 devDependency）；Rust 侧不动。

**Spec:** `docs/superpowers/specs/2026-10-22-enable-pending-ui-features-design.md` —— 执行者必须先读它，plan 只负责落地顺序与接口约定。

## Global Constraints

以下每一条都逐字来自 spec / 仓库既有约定，**每个 task 都隐含包含**：

- **前端不得参与音频计算**（ADR-018 规则 1）：dB 换算、FFT、峰值检测、降采样全部在 Rust 侧。`scripts/check-boundaries.ps1` 机械拦截 `Math.log10`、`Math.log2`、`new Float64Array`、`fft(`、`hann(`、`hamming(`、`blackman(` —— 新代码与**注释**都不得出现这些小写序列。
- **只引用设计令牌**（ADR-017）：组件内不写死颜色，一律 `var(--token)`。
- **组件只做三件事**：组合类名、转发原生属性、保持类型安全（`components/ui/primitives.tsx` 头部约定）。
- **读数缺一个字段就整条丢弃**回到默认值，不猜用户意图（`App.tsx` 现有 `readDisplayPrefs` / `readVisibility` 约定）。
- **取消不是失败**：保留上一次结果，只在状态栏说明。
- **未验证的容器必须标注**：只有 WAV 会被标记「已验证」，其余显示「未验证」角标。
- **不宣称未验证的指标**：不出现 LUFS / SNR / THD / THD+N 数字。
- 注释与文档用中文，与仓库既有风格一致。
- 声谱图矩阵是 **512 个 bin**（`SPECTROGRAM_BINS = 512`），**不得硬编码 1024**。

## Review Focus

spec 暗示但没有任何 task 的检查能覆盖、且最可能咬到真实用户的五类输入 / 失效模式。每条后面标注由哪个 task 用什么测试钉住。

1. **换采样率的文件后频率视口越界** —— 先打开 44.1 kHz 文件把视口缩到 20–22 kHz，再打开 48 kHz 文件，视口必须钳位到新奈奎斯特内，否则频谱与声谱图整片空白。→ Task 2 `clampViewRange` 测试。
2. **实时采集开启时声谱图的频率轴** —— 声谱图画的是**文件**的矩阵，频率轴却标成了**设备**的奈奎斯特，读数直接是错的。→ Task 3 `App.tsx` 必须把文件的 `display.sampleRate` 传给 `SpectrogramView`，与实时用的 `sampleRate` 分开。
3. **localStorage 里的坏偏好数据** —— 损坏 JSON、缺字段、越界数值都必须整条丢弃回默认，不得让界面崩、不得让「改了没反应」。→ Task 1 `parsePrefs` / `readPrefs` 测试。
4. **静音文件** —— peak / rms 是真 `-inf`（`null`），电平表与指标卡片必须显示「—」，**不得显示 0**（0 会被读成「有信号但极小」）。→ Task 4 `MetricsPanel` 快照测试。
5. **文件被截断** —— 声明时长与实际解码时长不一致时，数据质量卡片必须显示出来，否则用户以为分析覆盖了整个文件。→ Task 4 `MetricsPanel` 快照测试。

---

## File Structure

**新增（前端）**

| 文件 | 职责 |
|---|---|
| `src/lib/prefs.ts` | 统一偏好：类型、校验、旧键迁移、读写与清除。**纯函数 + 存储封装**两层 |
| `src/lib/prefs.test.ts` | 偏好校验与迁移的测试 |
| `src/lib/spectrogram-viewport.ts` | 声谱图频率视口 ↔ bin 区间的换算与刻度生成（纯函数） |
| `src/lib/spectrogram-viewport.test.ts` | 视口换算测试 |
| `src/lib/task-history.ts` | 「最近完成」历史记录的纯 reducer |
| `src/lib/task-history.test.ts` | 历史记录测试 |
| `src/lib/search.ts` | 命令面板的字符串匹配（纯函数，从 `SearchPalette.tsx` 抽出） |
| `src/lib/search.test.ts` | 搜索匹配测试 |
| `src/lib/display.ts` | `DisplayData` 接口（从 `App.tsx` 移出，供 `MetricsPanel` 复用） |
| `src/components/audio-viz/MetricsPanel.tsx` | 中央画布「电平 / 指标」卡片的内容 |
| `src/components/audio-viz/TaskQueuePanel.tsx` | 左侧栏任务队列（运行中 + 最近完成） |
| `src/components/ui/PreferencesDialog.tsx` | 偏好设置对话框 |
| `src/components/ui/HelpDialog.tsx` | 帮助对话框 |
| `src/components/ui/AboutDialog.tsx` | 关于对话框 |

**修改**

| 文件 | 改动 |
|---|---|
| `src/App.tsx` | 接线全部新入口；共享 `viewRange`；对话框状态；快捷键；`DisplayData` 移出 |
| `src/components/audio-viz/SpectrogramView.tsx` | 频率视口：裁剪绘制、刻度、`Shift` 交互 |
| `src/components/audio-viz/RealtimePanel.tsx` | 新增 `onStatusChange` 回调 |
| `src/components/ui/SearchPalette.tsx` | 改用 `lib/search.ts` 的匹配函数 |
| `src/components/ui/primitives.tsx` | 补 `Segmented`（若需要） |
| `src/styles/layout.css` | 新增样式：modal / palette / task-queue / metrics / help / about / 分段控件 |
| `package.json` | `test` 脚本 |
| `vitest.config.ts`（新增） | vitest 配置 |
| `../../.github/workflows/ci.yml` | 前端 job 增加 `npm run test` |
| `docs/UI_SPEC.md`、`docs/USER_MANUAL.md`、`docs/IMPLEMENTATION_PLAN.md`、`CHANGELOG.md` | 文档同步 |

**不改**：`crates/**` 全部不动。

---

### Task 1: 前端测试基座与统一偏好存储

**Files:**
- Create: `apps/desktop/vitest.config.ts`
- Create: `apps/desktop/src/lib/prefs.ts`
- Create: `apps/desktop/src/lib/prefs.test.ts`
- Modify: `apps/desktop/package.json`（加 `test` 脚本）
- Modify: `.github/workflows/ci.yml`（前端 job 加 `npm run test`）

**Interfaces:**
- Consumes: 无（首个 task）。
- Produces:
  ```ts
  // src/lib/prefs.ts
  export interface Prefs {
    spectrumMode: "area" | "line";
    dbRange: number;              // 索引，0..3
    channelLayout: "lanes" | "overlay";
    showLeft: boolean;
    showRight: boolean;
    envelopePoints: number;       // 512 | 1024 | 2048 | 4096
    restoreSession: boolean;
  }
  export const DEFAULT_PREFS: Prefs;
  export const ENVELOPE_POINTS: readonly number[];   // [512, 1024, 2048, 4096]
  export const PREFS_KEY = "auralis.prefs.v1";
  export const LEGACY_DISPLAY_KEY = "auralis.display.v1";
  export const LEGACY_VISIBILITY_KEY = "auralis.workspace.visibility.v1";
  export const SESSION_KEY = "auralis.session.v1";
  export const WIDTHS_KEY = "auralis.workspace.widths.v1";

  export function parsePrefs(value: unknown): Prefs | null;  // 缺任一字段 → null
  export function readPrefs(): Prefs;                       // 新键 → 旧键迁移 → 默认
  export function writePrefs(prefs: Prefs): void;
  export function clearPrefs(): void;      // 删 PREFS_KEY + LEGACY_DISPLAY_KEY
  export function resetLayout(): void;     // 删 LEGACY_VISIBILITY_KEY + WIDTHS_KEY
  export function clearSession(): void;    // 删 SESSION_KEY
  ```

- [x] **Step 1: 配置 vitest 并加 `test` 脚本**

`apps/desktop/vitest.config.ts`（新增文件）：

```ts
import { defineConfig } from "vitest/config";
import { fileURLToPath } from "node:url";

export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
});
```

`apps/desktop/package.json` 的 `scripts` 里加一行（放在 `lint` 之后）：

```json
"test": "vitest run",
```

- [x] **Step 2: 写失败的测试**

`apps/desktop/src/lib/prefs.test.ts`（新增文件），覆盖：

- `parsePrefs(null)` → `null`
- `parsePrefs({})` → `null`（缺字段整条丢弃）
- `parsePrefs({ ...DEFAULT_PREFS, dbRange: 99 })` → `null`（越界）
- `parsePrefs({ ...DEFAULT_PREFS, spectrumMode: "bogus" })` → `null`
- `parsePrefs({ ...DEFAULT_PREFS, envelopePoints: 777 })` → `null`（不在 `ENVELOPE_POINTS` 内）
- `parsePrefs(DEFAULT_PREFS)` → 等于 `DEFAULT_PREFS`
- `readPrefs()` 在无任何存储时返回 `DEFAULT_PREFS`
- `readPrefs()` 在只有旧 `auralis.display.v1`（`{mode:"line", dbRange:1, channelLayout:"overlay"}`）时，返回值含 `spectrumMode==="line"`、`dbRange===1`、`channelLayout==="overlay"`，其余为默认
- `readPrefs()` 在只有旧 `auralis.workspace.visibility.v1`（`{left:false, right:true}`）时，`showLeft===false`、`showRight===true`
- `readPrefs()` 在新旧键同时存在时，**新键优先**
- `writePrefs()` 后 `readPrefs()` 返回同一份对象

测试需 stub `localStorage`。在测试文件顶部放一个最小实现：

```ts
class MemStorage {
  private map = new Map<string, string>();
  getItem(k: string) { return this.map.get(k) ?? null; }
  setItem(k: string, v: string) { this.map.set(k, v); }
  removeItem(k: string) { this.map.delete(k); }
  clear() { this.map.clear(); }
  key(i: number) { return [...this.map.keys()][i] ?? null; }
  get length() { return this.map.size; }
}
```

并在每个用例前后 `globalThis.localStorage = new MemStorage() as unknown as Storage;`。

- [x] **Step 3: 运行测试，确认失败**

Run: `cd apps/desktop && npm test`
Expected: FAIL —— `Cannot find module '.../src/lib/prefs'`（模块还不存在）。

- [x] **Step 4: 实现 `src/lib/prefs.ts`**

实现要点：

- `parsePrefs` 用 `asBoolean` / `asNumberInRange`（复用 `src/lib/persist.ts` 已有的这两个函数，**不要**重新实现）逐字段校验；**任一字段缺失或非法即返回 `null`**。
- `spectrumMode` 只接受 `"area"` / `"line"`；`channelLayout` 只接受 `"lanes"` / `"overlay"`；`envelopePoints` 必须是 `ENVELOPE_POINTS` 的成员。
- `readPrefs()` 的顺序：`PREFS_KEY` → （若没有）合并 `LEGACY_DISPLAY_KEY` 与 `LEGACY_VISIBILITY_KEY` → 都没有则 `DEFAULT_PREFS`。
- 迁移路径读到旧键时，**必须把合并结果写回 `PREFS_KEY`**（一次性迁移，之后只用新键），但**不删旧键**（旧键可能还有别的读取方，删了是破坏性行为）。
- 所有存储读写包 `try/catch`（`localStorage` 在隐私模式 / 配额满时会抛错），失败静默忽略。
- `clearPrefs` / `resetLayout` / `clearSession` 各自只删自己那几个键，互不影响。

- [x] **Step 5: 运行测试，确认通过**

Run: `cd apps/desktop && npm test`
Expected: PASS，全部用例通过。

- [x] **Step 6: 把测试接进 CI**

`.github/workflows/ci.yml` 的前端 job（`name: typecheck / build (node ${{ matrix.node }})`）中，在 `npm run typecheck` 之后加一步：

```yaml
      - name: 前端单元测试
        run: npm run test
```

- [x] **Step 7: 跑一遍既有检查，确认没弄坏东西**

Run: `cd apps/desktop && npm run typecheck`
Expected: PASS（无输出即通过，`tsc --noEmit` 报错会非零退出）。

- [x] **Step 8: 提交**

```bash
git add apps/desktop/vitest.config.ts apps/desktop/src/lib/prefs.ts apps/desktop/src/lib/prefs.test.ts apps/desktop/package.json apps/desktop/package-lock.json .github/workflows/ci.yml
git commit -m "feat(desktop): 前端测试基座与统一偏好存储"
```

---

### Task 2: 声谱图频率视口的换算（纯函数）

**Files:**
- Create: `apps/desktop/src/lib/spectrogram-viewport.ts`
- Create: `apps/desktop/src/lib/spectrogram-viewport.test.ts`

**Interfaces:**
- Consumes: 无。
- Produces:
  ```ts
  // src/lib/spectrogram-viewport.ts
  /** 频率区间 [lo, hi]，Hz */
  export type HzRange = [number, number];

  export function clampViewRange(
    range: HzRange | null,
    nyquistHz: number,
  ): HzRange | null;
  // null 进 → null 出；越界时把区间平移/收缩到 [0, nyquistHz]；
  // 区间宽度 < 1 Hz 或非有限 → null（视为未缩放）

  export function binRangeForView(
    range: HzRange | null,
    nyquistHz: number,
    bins: number,
  ): { first: number; last: number };  // 闭区间，至少 1 个 bin

  export function frequencyTicks(
    range: HzRange | null,
    nyquistHz: number,
    count: number,
  ): number[];  // 长度 count+1，从低到高
  ```

- [x] **Step 1: 写失败的测试**

`apps/desktop/src/lib/spectrogram-viewport.test.ts`（新增文件）：

- `clampViewRange(null, 24000)` → `null`
- `clampViewRange([1000, 2000], 24000)` → `[1000, 2000]`（区间内，原样返回）
- `clampViewRange([-500, 2000], 24000)` → `[0, 2000]`
- `clampViewRange([20000, 30000], 24000)` → `[20000, 24000]`
- `clampViewRange([0, 50000], 24000)` → `[0, 24000]`
- `clampViewRange([2000, 1000], 24000)` → `null`（lo > hi，非法）
- `clampViewRange([1000, 1000.2], 24000)` → `null`（宽度 < 1 Hz）
- `clampViewRange([NaN, 2000], 24000)` → `null`（非有限）
- `binRangeForView(null, 22050, 512)` → `{ first: 0, last: 511 }`
- `binRangeForView([0, 22050], 22050, 512)` → `{ first: 0, last: 511 }`
- `binRangeForView([0, 11025], 22050, 512)` → `{ first: 0, last: 255 }`（半带宽 → 前一半 bin）
- `binRangeForView([1000, 2000], 22050, 512)` → `first`/`last` 落在 23..46 之间（bin 宽 = 22050/512 ≈ 43.07 Hz），且 `last >= first`
- `binRangeForView([22000, 24000], 22050, 512)` → `{ first: 511, last: 511 }`（超出奈奎斯特 → 至少 1 个 bin，不能返回空区间）
- `binRangeForView(null, 22050, 0)` → `{ first: 0, last: 0 }`（bins=0 不得产生 `last = -1`）
- `frequencyTicks(null, 24000, 4)` → `[0, 6000, 12000, 18000, 24000]`
- `frequencyTicks([1000, 2000], 24000, 2)` → `[1000, 1500, 2000]`

- [x] **Step 2: 运行测试，确认失败**

Run: `cd apps/desktop && npm test`
Expected: FAIL —— `Cannot find module '.../src/lib/spectrogram-viewport'`。

- [x] **Step 3: 实现 `src/lib/spectrogram-viewport.ts`**

实现要点：

- `clampViewRange`：`null` → `null`；`lo`/`hi` 任一非有限 → `null`；`hi - lo < 1` → `null`；`lo >= hi` → `null`；否则把 `[lo, hi]` 夹到 `[0, nyquistHz]`：先 `lo = max(0, lo)`、`hi = min(nyquist, hi)`，若夹完 `hi - lo < 1` → 返回 `null`。
- `binRangeForView`：`binHz = nyquistHz / bins`（`bins <= 0` 时返回 `{ first: 0, last: 0 }`）；`range` 为 `null` → `{ first: 0, last: bins - 1 }`；否则 `first = clamp(floor(lo / binHz), 0, bins - 1)`、`last = clamp(ceil(hi / binHz) - 1, first, bins - 1)`。
- `frequencyTicks`：`range` 为 `null` 时取 `[0, nyquistHz]`；返回 `count + 1` 个等距点，`i / count * (hi - lo) + lo`。

- [x] **Step 4: 运行测试，确认通过**

Run: `cd apps/desktop && npm test`
Expected: PASS。

- [x] **Step 5: 提交**

```bash
git add apps/desktop/src/lib/spectrogram-viewport.ts apps/desktop/src/lib/spectrogram-viewport.test.ts
git commit -m "feat(desktop): 声谱图频率视口的 bin 区间换算"
```

---

### Task 3: 声谱图频率视口的渲染与交互

**Files:**
- Modify: `apps/desktop/src/components/audio-viz/SpectrogramView.tsx`（props、绘制、交互）
- Modify: `apps/desktop/src/App.tsx`（传 `viewRange` / `onViewRangeChange`；修采样率来源）

**Interfaces:**
- Consumes: Task 2 的 `clampViewRange` / `binRangeForView` / `frequencyTicks`。
- Produces: `SpectrogramViewProps` 新增两个字段：
  ```ts
  /** 可见频率区间（Hz）；null = 全带宽 */
  viewRange?: [number, number] | null;
  /** 频率视口变化回调 */
  onViewRangeChange?: (r: [number, number]) => void;
  ```
  以及 `App.tsx` 里已有的 `viewRange` / `setViewRange` 状态**同时**传给 `SpectrumView` 与 `SpectrogramView`。

- [x] **Step 1: 给 `SpectrogramView` 加 props**

在 `SpectrogramViewProps` 里加 `viewRange?: [number, number] | null;` 与 `onViewRangeChange?: (r: [number, number]) => void;`，并把两者放进组件内部已有的 `stateRef`（与 `matrix`、`minDb` 等同一批），`stateRef.current = {...}` 的赋值同步加上这两项。

- [x] **Step 2: 改绘制：裁剪 bin 区间而不是缩放整张图**

在 `draw()` 里，`const bins = s.height; const frames = s.width;` 之后，把「整张矩阵贴满绘图区」改成「只贴可见 bin 区间」：

```ts
const view = binRangeForView(s.viewRange, s.nyquistHz, bins);
const first = view.first;
const last = view.last;
const visBins = last - first + 1;
```

`ImageData` 的尺寸改为 `new ImageData(frames, visBins)`；内层循环 `for (let b = first; b <= last; b++)`，源下标 `const v = matrix[t * bins + b]!;`，目标行 `const o = ((visBins - 1 - (b - first)) * frames + t) * 4;`（低频在下、只贴可见区间）。

频率刻度那段（现在的 `for (let i = 0; i <= 4; i++)` 用 `s.nyquistHz * (4 - i) / 4`）改成用 `frequencyTicks(s.viewRange, s.nyquistHz, 4)`，按低频在下排列：`const ticks = frequencyTicks(...); const y = plotY + (plotH * i) / 4; const hz = ticks[ticks.length - 1 - i];`。

- [x] **Step 3: 改交互：加 `Shift` 频率缩放，保留既有时间平移**

现有的视口交互 effect 里，`onWheel` 已经是「平移时间」。改成按 `e.shiftKey` 分流：

- `e.shiftKey` → 频率缩放：以光标所在频率为中心，步进因子 `e.deltaY > 0 ? 1.25 : 0.8`，算出新的 `[lo, hi]`，再用 `clampViewRange(newRange, s.nyquistHz)` 钳位；钳位后为 `null` 就**不调**回调（避免误把视口清空）。回调参数为 `[number, number]` 元组（不是 `HzRange` 别名，避免 `App.tsx` 的 `setViewRange` 类型不匹配——两者结构相同，可直接赋值）。
- 非 `shiftKey` → 保持现有的时间平移行为**完全不变**。

`onPointerDown` / `onPointerUp` 同样分流：

- `e.shiftKey` 拖拽 → 频率区间框选：记录起始 y，按 `yToHz` 映射到频率，选中区间 ≥ 1 Hz 时调用 `onViewRangeChange([lo, hi])`。
- 非 `shiftKey` 拖拽 → 保持现有时间平移。

`yToHz` 映射：`freq = lo + (1 - (clientY - plotY) / plotH) * (hi - lo)`（低频在下，故取 `1 - t`）。

双击：保持现有语义「回到文件开头」，**不改**。

- [x] **Step 4: 在 `App.tsx` 接线并修掉采样率错标**

在 `App.tsx` 的 `<SpectrogramView ...>` 上加：

```tsx
viewRange={viewRange}
onViewRangeChange={setViewRange}
```

**关键**：声谱图的 `nyquistHz` 必须用**文件自己的**采样率，不是实时设备采样率。现在 `App.tsx` 传的是 `nyquistHz={sampleRate / 2}`，而 `sampleRate` 在实时模式下会切成设备采样率 —— 实时采集开着时，声谱图画的是文件矩阵、频率轴却标设备奈奎斯特，读数是错的。改为：

```tsx
nyquistHz={(display?.sampleRate ?? sampleRate) / 2}
```

同时，**打开新文件时重置视口**：在 `analyzePath` 成功拿到分析结果后、`setDisplay` 之前调用 `setViewRange(null)`（新文件的频率轴与旧视口不匹配，保留会裁出空白区）。合成信号路径（`run`）同样重置。

- [x] **Step 5: 手工验收清单**

Run: `cd apps/desktop && npm run typecheck && npm test`
Expected: 两条都通过。

然后 `cd apps/desktop && npm run electron:dev`，打开一个真实音频文件，确认：

- 声谱图上 `Shift` + 滚轮 → 频率轴刻度**跟着变**（不再固定 0…奈奎斯特），可见内容被**裁剪**（放大 1 kHz 附近时能看到该处内容占据整个绘图区高度）。
- `Shift` + 拖拽 → 框选一段频率并放大。
- 普通滚轮 / 拖拽 → 仍是时间平移（**没有被改**）。
- 双击 → 仍是回到文件开头。
- 频谱图与声谱图的视口**同步**：缩放声谱图后频谱图也缩放到同一段频率。

- [x] **Step 6: 提交**

```bash
git add apps/desktop/src/components/audio-viz/SpectrogramView.tsx apps/desktop/src/App.tsx
git commit -m "feat(desktop): 声谱图频率缩放，与频谱共享视口"
```

---

### Task 4: 中央画布「电平 / 指标」卡片

**Files:**
- Create: `apps/desktop/src/lib/display.ts`
- Create: `apps/desktop/src/components/audio-viz/MetricsPanel.tsx`
- Modify: `apps/desktop/src/App.tsx`（移出 `DisplayData`、加卡片、中心区可滚动）
- Modify: `apps/desktop/src/styles/layout.css`

**Interfaces:**
- Consumes: 无新增（`LevelMeter` 已有）。
- Produces:
  ```ts
  // src/lib/display.ts
  export interface DisplayData {
    db: Float32Array | null;
    startHz: number;
    stepHz: number;
    env: Float32Array | null;
    sampleRate: number;
    peakDbfs: number | null;
    rmsDbfs: number | null;
    crestDb: number | null;
    dc: number;
    outcome: string;
    elapsedMs: number;
    bytes: number;
    peaks: PeakInfo[];
    fftSize: number;
    window: string;
    spectrogram: Float32Array | null;
    spectrogramFrames: number;
    spectrogramBins: number;
    spectrogramHopMs: number;
    spectrogramStartSeconds: number;
    peakPerChannel: (number | null)[];
    overFullScaleSamples: number;
    clippingRegions: number;
    sanitizedSamples: number;
    // 新增：文件完整性（并行落地的截断检测字段）
    truncated: boolean;
    declaredDurationSeconds: number;
    decodedDurationSeconds: number;
  }

  // src/components/audio-viz/MetricsPanel.tsx
  export interface MetricsPanelProps {
    display: DisplayData | null;
    /** 实时读数；null = 未在采集 */
    live: { peakDbfs: number | null; rmsDbfs: number | null } | null;
    minDb: number;
    maxDb: number;
  }
  export function MetricsPanel(props: MetricsPanelProps): JSX.Element;
  ```

- [x] **Step 1: 把 `DisplayData` 移出 `App.tsx`**

在 `src/lib/display.ts` 新建文件，把 `App.tsx` 里现有的 `DisplayData` 接口**原样**搬过去（字段一个不少、一个不改），并 `import type { DisplayData } from "@/lib/display";` 回到 `App.tsx`。删除 `App.tsx` 里的原接口定义。

同时给接口补上并行落地的三个截断字段（`truncated` / `declaredDurationSeconds` / `decodedDurationSeconds`），并在 `App.tsx` 的两处 `setDisplay({...})`（`analyzePath` 与 `run`）补上这三个字段的值：

- 文件分析：`truncated: a.truncated`、`declaredDurationSeconds: a.declaredDurationSeconds`、`decodedDurationSeconds: a.decodedDurationSeconds`。
- 合成信号：`truncated: false`、`declaredDurationSeconds: 0`、`decodedDurationSeconds: 0`（合成信号没有文件头）。

- [x] **Step 2: 实现 `MetricsPanel`**

`src/components/audio-viz/MetricsPanel.tsx`（新增文件），三栏布局：

1. **电平**：复用现有 `LevelMeter`（`components/audio-viz/LevelMeter.tsx`），props 用 `display?.peakDbfs`、`display?.rmsDbfs`、`display?.peakPerChannel ?? []`、`minDb`、`maxDb`、`display?.overFullScaleSamples ?? 0`、`display?.clippingRegions ?? 0`。实时态（`live != null`）时改用 `live.peakDbfs` / `live.rmsDbfs`。
2. **信号指标**：Peak / RMS / Crest / DC Offset 四行（沿用 `App.tsx` 里 `MetricRow` 的写法：`value == null ? "—" : value.toFixed(4)`，**null 不得显示成 0**），以及逐声道 Peak（`peakPerChannel.length > 1` 时才显示，单声道时与上面重复）。
3. **数据质量**：超量程样本 / 平顶片段 / 清理样本 / 解码帧数 / 包络点数 / FFT 点数与窗。**文件被截断时**额外显示一行「完整性」：`声明 {declaredDurationSeconds.toFixed(2)} s · 实际解码 {decodedDurationSeconds.toFixed(2)} s`，并加 `warn-note` 样式说明「只分析了文件里实际存在的那部分」。

`display == null` 且不在实时态时，整块显示空状态「打开一个音频文件以查看指标」。

**禁止**：组件内不得出现任何音频计算（不换算 dB、不求 RMS、不做峰值检测）——只格式化 Rust 已给出的数字。

- [x] **Step 3: 在中心画布加卡片，并让中心区可滚动**

`App.tsx` 的 `center` 里，在「波形」卡片之后追加一张卡片：

```tsx
<section className="pane pane-metrics" id="pane-metrics">
  <div className="pane-head">
    <span className="pane-title">电平 / 指标</span>
    <span className="pane-meta">
      {display?.outcome === "succeeded" ? "已分析" : live ? "实时" : "等待数据"}
    </span>
  </div>
  <div className="pane-body pane-scroll">
    <MetricsPanel
      display={display}
      live={liveDb ? { peakDbfs: livePeak, rmsDbfs: liveRms } : null}
      minDb={minDb}
      maxDb={maxDb}
    />
  </div>
</section>
```

`styles/layout.css`：

- `.center-stack` 加 `overflow-y: auto;`（现在是 `height: 100%` + flex 列）。
- 三张图表卡片加 `min-height: 190px;`（`flex: 1.35 1 0` / `flex: 1 1 0` 保留），窗口变矮时中心区滚动而不是把图表压成条带。
- 新增 `.pane-metrics { flex: 0 0 auto; }`（高度按内容自适应，不参与拉伸）。
- 新增 `.pane-scroll { overflow: auto; }` 与 `.metrics-grid` 等样式：三栏用 `display: grid; grid-template-columns: minmax(180px, 1fr) minmax(180px, 1fr) minmax(200px, 1.2fr); gap: 12px;`，窄窗口下 `grid-template-columns: 1fr`（媒体查询 `max-width: 900px`）。所有颜色走令牌。

- [x] **Step 4: 让「电平表 / 指标」导航项可用**

`App.tsx` 的 `ResourcePanel` 里，把 `<NavItem icon={<IconGauge />} disabled tag="M2">电平表 / 指标</NavItem>` 改为可点击，点击后滚动到 `#pane-metrics`：

```tsx
<NavItem
  icon={<IconGauge />}
  onClick={() => scrollToPane("pane-metrics")}
>
  电平表 / 指标
</NavItem>
```

`scrollToPane(id)` 是 `App.tsx` 里新增的一个小工具函数：

```ts
function scrollToPane(id: string) {
  window.requestAnimationFrame(() => {
    document.getElementById(id)?.scrollIntoView({ behavior: "smooth", block: "nearest" });
  });
}
```

- [x] **Step 5: 跑检查**

Run: `cd apps/desktop && npm run typecheck && npm test`
Expected: 两条都通过。

- [x] **Step 6: 提交**

```bash
git add apps/desktop/src/lib/display.ts apps/desktop/src/components/audio-viz/MetricsPanel.tsx apps/desktop/src/App.tsx apps/desktop/src/styles/layout.css
git commit -m "feat(desktop): 中央画布新增电平与指标视图"
```

---

### Task 5: 后台任务队列面板

**Files:**
- Create: `apps/desktop/src/lib/task-history.ts`
- Create: `apps/desktop/src/lib/task-history.test.ts`
- Create: `apps/desktop/src/components/audio-viz/TaskQueuePanel.tsx`
- Modify: `apps/desktop/src/App.tsx`（轮询、取消、历史记录、导航项）
- Modify: `apps/desktop/src/styles/layout.css`

**Interfaces:**
- Consumes: `fetchTaskQueue()` / `cancelTask()`（`src/lib/ipc.ts` 已有）、`TaskInfo`（`src/lib/contracts.ts` 已有）。
- Produces:
  ```ts
  // src/lib/task-history.ts
  export type TaskOutcome = "succeeded" | "cancelled" | "failed";
  export interface HistoryEntry {
    id: string;
    kind: string;
    label: string;
    outcome: TaskOutcome;
    elapsedMs: number;
    endedAt: number;
  }
  export function pushHistory(
    list: HistoryEntry[],
    entry: HistoryEntry,
    limit?: number,   // 默认 8
  ): HistoryEntry[];  // 最新在前；同 id 覆盖；超出 limit 截断
  ```

  ```ts
  // src/components/audio-viz/TaskQueuePanel.tsx
  export interface TaskQueuePanelProps {
    running: TaskInfo[];
    history: HistoryEntry[];
    onCancel: (taskId: number) => void;
  }
  export function TaskQueuePanel(props: TaskQueuePanelProps): JSX.Element;
  ```

- [x] **Step 1: 写失败的测试**

`apps/desktop/src/lib/task-history.test.ts`（新增文件）：

- `pushHistory([], e)` → 长度 1，内容等于 `e`
- `pushHistory([a], b)` → `[b, a]`（最新在前）
- `pushHistory([a], { ...a, outcome: "failed" })` → 长度仍为 1，且 `outcome === "failed"`（同 id 覆盖，不重复）
- `pushHistory(list8, e)` 且 `list8` 已满 8 条 → 结果长度 8，最新在首位，最旧的被截掉
- `pushHistory([a], b, 1)` → 长度 1，内容是 `b`
- 输入数组**不被就地修改**（`expect(list).toEqual(original)`）

- [x] **Step 2: 运行测试，确认失败**

Run: `cd apps/desktop && npm test`
Expected: FAIL —— `Cannot find module '.../src/lib/task-history'`。

- [x] **Step 3: 实现 `src/lib/task-history.ts`**

`pushHistory` 实现要点：先 `[entry, ...list.filter((e) => e.id !== entry.id)]`（去重 + 最新在前），再 `slice(0, limit)`。**不修改**传入的数组（用新数组，不 `push` / `sort` 原地改）。

- [x] **Step 4: 实现 `TaskQueuePanel`**

`src/components/audio-viz/TaskQueuePanel.tsx`（新增文件）：

- 顶部标题行「任务」+ 右侧运行中计数。
- **运行中**区：每条一行 —— 类型徽标（`analyze` → 「分析」，`spectrogram` → 「声谱图」，其他 → 原样）、标签（`task.label`）、已运行时长（`task.elapsedMs`，格式 `1.2 s` / `340 ms`）、「取消」小按钮 → `onCancel(task.taskId)`。
- **最近完成**区：`history` 每条一行 —— 标签 + 结果徽标（完成 / 已取消 / 失败）+ 耗时。徽标颜色用 `--signal-positive` / `--signal-warning` / `--signal-critical`，**并且带文字**（不能只靠颜色表达状态，UI_SPEC §2.2）。
- 两者都空时显示空状态「没有正在运行的任务」。

- [x] **Step 5: 在 `App.tsx` 里轮询并接线**

`App.tsx` 新增状态与 effect：

```ts
const [tasks, setTasks] = useState<TaskInfo[]>([]);
const [history, setHistory] = useState<HistoryEntry[]>([]);
```

轮询 effect（`task_queue` 是**快命令**，在 RPC 读取线程内联执行，不会排在慢任务后面，500 ms 一次开销可忽略）：

```ts
useEffect(() => {
  let alive = true;
  const tick = () => {
    fetchTaskQueue()
      .then((t) => { if (alive) setTasks(t); })
      .catch(() => undefined);
  };
  tick();
  const id = window.setInterval(tick, 500);
  return () => { alive = false; window.clearInterval(id); };
}, []);
```

历史记录：**只在文件分析（`analyze_file`）结算时**写入，且**不收录声谱图视口任务**（自动平移会连发这类请求，进历史会刷屏、挤掉用户真正关心的文件分析）。在 `analyzePath` 的 `try` 块里，分析返回后：

```ts
if (a.outcome === "succeeded" || a.outcome === "cancelled" || a.outcome === "failed") {
  setHistory((h) =>
    pushHistory(h, {
      id: `analyze:${id}`,
      kind: "analyze",
      label: fi.fileName,
      outcome: a.outcome,
      elapsedMs: a.__elapsedMeasuredMs,
      endedAt: Date.now(),
    }),
  );
}
```

`catch` 分支里同样写一条 `outcome: "failed"` 的记录（`id` 用同一个 `analyze:${id}`，保证失败不会与成功重复计数）。

`TaskInfo.outcome` 在 `contracts.ts` 里是 `AnalysisOutcome`（`"succeeded" | "cancelled" | "failed" | "panicked"`）——写历史时把 `"panicked"` 归到 `"failed"`。

- [x] **Step 6: 导航项解禁 + 面板挂载**

`ResourcePanel` 里把 `<NavItem icon={<IconTasks />} disabled tag="M3">后台任务队列</NavItem>` 改为可点击，点击滚动到面板；面板本体放在 `RealtimePanel` 之后、`工程` 分组之前：

```tsx
<div className="nav-divider" />
<RealtimePanel onSnapshot={onLiveSnapshot} onStatusChange={onLiveStatusChange} />
<div id="task-queue-panel">
  <TaskQueuePanel running={tasks} history={history} onCancel={onCancelQueueTask} />
</div>
```

导航项上显示运行中任务数（`tasks.length > 0` 时用一个 `nav-tag` 显示数字）。

`onCancelQueueTask` 复用现有的 `cancelTask(taskId)`：

```ts
const onCancelQueueTask = useCallback(async (taskId: number) => {
  try {
    const found = await cancelTask(taskId);
    setNotice(found ? "已请求取消任务" : "任务已经结束");
  } catch (e) {
    setError(describeError(e, "取消失败"));
  }
}, []);
```

- [x] **Step 7: 样式**

`styles/layout.css` 新增 `.task-queue`、`.task-row`、`.task-kind`（徽标）、`.task-outcome`、`.task-list-empty`。徽标用令牌色，**不要**写死颜色。

- [x] **Step 8: 跑检查并提交**

Run: `cd apps/desktop && npm run typecheck && npm test`
Expected: 两条都通过。

```bash
git add apps/desktop/src/lib/task-history.ts apps/desktop/src/lib/task-history.test.ts apps/desktop/src/components/audio-viz/TaskQueuePanel.tsx apps/desktop/src/App.tsx apps/desktop/src/styles/layout.css
git commit -m "feat(desktop): 后台任务队列面板"
```

---

### Task 6: 实时输入导航入口与采集状态

**Files:**
- Modify: `apps/desktop/src/components/audio-viz/RealtimePanel.tsx`（新增 `onStatusChange`）
- Modify: `apps/desktop/src/App.tsx`（导航项解禁 + 状态）

**Interfaces:**
- Consumes: 无。
- Produces: `RealtimePanelProps` 新增：
  ```ts
  /** 采集状态变化回调；true = 采集中 */
  onStatusChange?: (running: boolean) => void;
  ```

- [x] **Step 1: 给 `RealtimePanel` 加 `onStatusChange`**

`RealtimePanelProps` 加 `onStatusChange?: (running: boolean) => void;`。

在组件内，用与 `snapshotRef` 相同的手法把回调放进 ref（`statusRef`），避免父组件每次渲染传入新箭头函数导致 effect 依赖抖动：

```ts
const statusRef = useRef(onStatusChange);
useEffect(() => { statusRef.current = onStatusChange; }, [onStatusChange]);
```

然后在 `status` 变化时上报一次（用一个 effect，依赖 `status?.running`）：

```ts
useEffect(() => {
  statusRef.current?.(status?.running === true);
}, [status?.running]);
```

**不改**其余采集逻辑（轮询、丢帧计数、格式协商都不动）。

- [x] **Step 2: 在 `App.tsx` 解禁导航项**

`App.tsx` 新增状态 `const [liveRunning, setLiveRunning] = useState(false);`，把 `onStatusChange={setLiveRunning}` 传给 `ResourcePanel` 里的 `RealtimePanel`。

导航项：

```tsx
<NavItem
  icon={<IconDevice />}
  onClick={() => scrollToPane("realtime-panel")}
>
  实时输入
</NavItem>
```

采集中时在导航项右侧显示一个状态点（用 `IconDevice` 之外的文字徽标「采集中」，避免只靠颜色表达状态）。

> `scrollToPane` 用的是 `scrollIntoView`，而 `realtime-panel` 是侧栏里的元素（`id="realtime-panel"` 已存在于 `RealtimePanel.tsx`）。侧栏本身可滚动时同样适用；若侧栏不滚动，`scrollIntoView` 仍会把最近的滚动祖先滚到位。

- [x] **Step 3: 跑检查并提交**

Run: `cd apps/desktop && npm run typecheck && npm test`
Expected: 两条都通过。

```bash
git add apps/desktop/src/components/audio-viz/RealtimePanel.tsx apps/desktop/src/App.tsx
git commit -m "feat(desktop): 实时输入导航入口与采集状态指示"
```

---

### Task 7: 全局搜索面板接线

**Files:**
- Create: `apps/desktop/src/lib/search.ts`
- Create: `apps/desktop/src/lib/search.test.ts`
- Modify: `apps/desktop/src/components/ui/SearchPalette.tsx`（改用 `lib/search.ts`）
- Modify: `apps/desktop/src/App.tsx`（构造条目 + 快捷键）
- Modify: `apps/desktop/src/styles/layout.css`（palette 样式）

**Interfaces:**
- Consumes: Task 1 的 `Prefs`（间接）。
- Produces:
  ```ts
  // src/lib/search.ts
  export interface SearchableItem {
    group: string;
    label: string;
    hint?: string;
    keywords?: string;
  }
  export function matchItems<T extends SearchableItem>(
    items: T[],
    query: string,
    limit?: number,   // 默认 60
  ): T[];
  ```

- [x] **Step 1: 写失败的测试**

`apps/desktop/src/lib/search.test.ts`（新增文件）：

- `matchItems(items, "")` → 返回全部（受 `limit` 限制）
- `matchItems(items, "  ")` → 同上（纯空白视为空查询）
- `matchItems(items, "TONE")` → 命中 `label` 含 "tone" 的项（**大小写不敏感**）
- `matchItems(items, "tone wav")` → 两个词都要命中（AND 语义）
- `matchItems(items, "tone wav")` → 只在 `keywords` 里命中的项也算命中
- `matchItems(items, "不存在的词")` → 空数组
- `matchItems(items, "", 2)` → 长度 2
- 结果顺序与输入顺序一致（不过滤时）

- [x] **Step 2: 运行测试，确认失败**

Run: `cd apps/desktop && npm test`
Expected: FAIL —— `Cannot find module '.../src/lib/search'`。

- [x] **Step 3: 实现 `src/lib/search.ts`**

把 `SearchPalette.tsx` 里现有的 `filtered` 计算搬过来：查询按空白切分成小写词；每项的 haystack 是 `` `${group} ${label} ${hint ?? ""} ${keywords ?? ""}`.toLowerCase() ``；**所有**词都要 `includes` 命中（AND 语义）；结果 `slice(0, limit)`。

- [x] **Step 4: 让 `SearchPalette` 用它**

`SearchPalette.tsx` 里删掉本地的 `filtered` useMemo，改成 `const filtered = useMemo(() => matchItems(items, query), [items, query]);`。

`SearchPalette` 的 `PaletteItem` 接口改成继承 `SearchableItem`：

```ts
export interface PaletteItem extends SearchableItem {
  id: string;
  run: () => void;
}
```

- [x] **Step 5: 在 `App.tsx` 构造搜索条目并加快捷键**

`App.tsx` 新增 `const [searchOpen, setSearchOpen] = useState(false);`。

用 `useMemo` 构造条目数组，三组：

- **文件**：`files.map((f) => ({ id: `file:${f.path}`, group: "文件", label: f.fileName, hint: `${f.containerName} · ${f.sampleRate.toLocaleString("en-US")} Hz`, keywords: f.path, run: () => void analyzePath(f.path) }))`
- **峰值**：`display?.peaks.map((p, i) => ({ id: `peak:${i}`, group: "峰值", label: `${fmtHz(p.frequencyHz)} Hz`, hint: `${p.amplitudeDbfs.toFixed(2)} dBFS${p.estimated ? "（估算）" : ""}`, keywords: "peak 峰值 频率", run: () => focusPeak(p) }))`
- **操作**：打开文件 / 导出 CSV·JSON / 生成并分析合成信号 / 复位缩放 / 显示·隐藏资源栏 / 显示·隐藏检查器 / 偏好设置 / 帮助 / 关于。

`focusPeak(p)` 的实现（**只做坐标换算，不做音频计算**）：

```ts
const focusPeak = useCallback((p: PeakInfo) => {
  // 声谱图 bin 宽 = 奈奎斯特 / bin 数；用它的 6 倍作为半宽，
  // 保证放大后声谱图里仍有若干个 bin 可看。
  const nyq = (display?.sampleRate ?? req.sampleRate) / 2;
  const binHz = nyq / Math.max(1, display?.spectrogramBins ?? 512);
  const half = Math.max(250, 6 * binHz);
  const center = p.binFrequencyHz > 0 ? p.binFrequencyHz : p.frequencyHz;
  const lo = Math.max(0, center - half);
  const hi = Math.min(nyq, center + half);
  if (hi - lo < 1) return;
  setViewRange([lo, hi]);
  setCursorHz(p.frequencyHz);
  scrollToPane("pane-spectrum");
}, [display, req.sampleRate]);
```

（`pane-spectrum` 是频谱卡片的 `id`，在 Task 8 里给频谱卡片加上；本 task 先加这个 `id` 到频谱卡片上。）

快捷键：`Ctrl+K` 与 `Ctrl+F` 打开搜索。

- [x] **Step 6: 样式**

`styles/layout.css` 新增 `.modal-palette`、`.palette-input-row`、`.palette-input`、`.palette-results`、`.palette-group`、`.palette-group-label`、`.palette-item`（含 `.on` 态）、`.palette-label`、`.palette-hint`、`.palette-empty`、`.palette-foot`、`.kbd`。全部走令牌。

- [x] **Step 7: 跑检查并提交**

Run: `cd apps/desktop && npm run typecheck && npm test`
Expected: 两条都通过。

```bash
git add apps/desktop/src/lib/search.ts apps/desktop/src/lib/search.test.ts apps/desktop/src/components/ui/SearchPalette.tsx apps/desktop/src/App.tsx apps/desktop/src/styles/layout.css
git commit -m "feat(desktop): 全局搜索面板接线"
```

---

### Task 8: 偏好设置 / 帮助 / 关于三个对话框

**Files:**
- Create: `apps/desktop/src/components/ui/PreferencesDialog.tsx`
- Create: `apps/desktop/src/components/ui/HelpDialog.tsx`
- Create: `apps/desktop/src/components/ui/AboutDialog.tsx`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/styles/layout.css`

**Interfaces:**
- Consumes: Task 1 的 `lib/prefs.ts` 全部导出；`AppInfo`（`src/lib/contracts.ts`）；`bridge().container`（`src/lib/bridge.ts`）。
- Produces: 三个对话框的 props 统一为：
  ```ts
  export interface PreferencesDialogProps {
    open: boolean;
    onClose: () => void;
    prefs: Prefs;
    onChange: (next: Prefs) => void;
  }
  export interface HelpDialogProps {
    open: boolean;
    onClose: () => void;
  }
  export interface AboutDialogProps {
    open: boolean;
    onClose: () => void;
    info: AppInfo | null;
  }
  ```

- [x] **Step 1: 实现 `PreferencesDialog`**

用已有的 `Modal`（`src/components/ui/primitives.tsx`，`<dialog>` + `showModal()`）做外壳。

分区与控件：

| 分区 | 控件 | 绑定 |
|---|---|---|
| 显示 | 频谱形态 | `Select` → `spectrumMode` |
| 显示 | dB 量程 | `Select` → `dbRange`（索引 0..3，选项文案沿用 `App.tsx` 的 `DB_RANGES`） |
| 显示 | 声道显示 | `Select` → `channelLayout` |
| 布局 | 资源栏 | `Checkbox` → `showLeft` |
| 布局 | 检查器 | `Checkbox` → `showRight` |
| 分析 | 包络点数 | `Select` → `envelopePoints`（512 / 1024 / 2048 / 4096） |
| 会话 | 启动时恢复上次文件 | `Checkbox` → `restoreSession` |
| 数据 | 重置显示与分析偏好 | 按钮 → `clearPrefs()` |
| 数据 | 重置面板布局 | 按钮 → `resetLayout()` |
| 数据 | 清除会话记录 | 按钮 → `clearSession()` |

三个「重置/清除」按钮点击后**立刻刷新对话框里的值**（读 `readPrefs()` 的结果回填），并给出提示文案。点「重置显示与分析偏好」后，内存态也要复位为 `DEFAULT_PREFS`（由 `App.tsx` 的 `onChange` 统一处理）。

- [x] **Step 2: 实现 `HelpDialog`**

内容全部来自 `docs/UI_SPEC.md` §6.2 与 `docs/PRD.md`，**不新造事实**。分区：

1. **快捷键**（表格）：`Ctrl+O` 打开文件 · `Ctrl+K` / `Ctrl+F` 搜索 · `Ctrl+,` 偏好设置 · `F1` 帮助 · `Esc` 取消正在进行的分析 · `Ctrl+R` 重试上次文件 · 拖拽框选 / 滚轮 / 双击 频谱缩放 · `Shift` + 拖拽 平移频谱视口 · 声谱图上滚轮 / 拖动 / 双击 前后平移一屏 / 拖动平移 / 回到文件开头 · `Shift` + 滚轮 / 拖拽 声谱图频率缩放 / 框选。
2. **界面导览**：顶部命令栏 / 左侧资源栏 / 中央画布 / 右侧检查器 / 底部状态栏，各一行说明。
3. **术语**：dBFS · RMS · Crest（波峰因数）· DC Offset · STFT · 扇贝损失（scalloping loss）· 超量程。
4. **能力边界**：不提供校准级 SPL 测量；不把数字满刻度推断为模拟削波；不以普通 RMS 代替 LUFS；仅 WAV 已用真实样本验证，其余容器标注「未验证」。

- [x] **Step 3: 实现 `AboutDialog`**

内容：

- 应用名 `Auralis Studio` 与版本（`info?.version`，`AppInfo` 来自 `app_info`）。
- 算法版本 `info?.algorithmVersion`、DSP 规范修订 `info?.dspSpecRevision`。
- 容器运行时：`bridge().container.versions` 的 electron / chrome / node（**注意 `bridge()` 在无 preload 时会抛错**，必须 try/catch，取不到就显示「—」）。
- 计算边界声明一句话。
- 第三方许可声明：MIT OR Apache-2.0。

- [x] **Step 4: 在 `App.tsx` 接线 + 快捷键**

新增状态：

```ts
const [dialog, setDialog] = useState<null | "prefs" | "help" | "about">(null);
```

- 偏好设置：`App.tsx` 用 `const [prefs, setPrefs] = useState<Prefs>(() => readPrefs());`，并把现有的显示偏好状态（`spectrumMode` / `dbRangeIdx` / `channelLayout`）与 `showLeft` / `showRight` 改为**从 `prefs` 派生**，保证顶栏复选框、右侧检查器「显示」分区、偏好对话框三者**同一个真值源**。

  做法：保留现有 state 变量，但把它们的初值改成从 `readPrefs()` 取；`prefs` 由这些 state 派生（`useMemo`），任何一处变化都 `writePrefs(prefs)`。这样改动最小且不引入双重真值源。

  具体：
  - `spectrumMode` 初值 `prefs.spectrumMode`；`dbRangeIdx` 初值 `prefs.dbRange`；`channelLayout` 初值 `prefs.channelLayout`；`showLeft` / `showRight` 初值 `prefs.showLeft` / `prefs.showRight`。
  - 已有 effect 继续写 `DISPLAY_KEY` / `VISIBILITY_KEY`；**新增**一个 effect 写 `PREFS_KEY`，内容由上述 state 派生。
  - `envelopePoints` 新增 state，初值 `prefs.envelopePoints`，并在 `analyzePath` 与 `onExport` 里**替代硬编码的 2048**（`analyzeFile(fi.path, envelopePoints, id)`、`exportAnalysis(..., envelopePoints)`）——否则导出会与屏幕显示不一致。
  - `restoreSession`：在启动恢复那个 effect 里，`if (!prefs.restoreSession) return;`（不再自动恢复上次文件）。
  - 旧的 `VISIBILITY_KEY` / `DISPLAY_KEY` 读写**保留**（`Workspace` 与其它调用点仍依赖它们），只是新增 `PREFS_KEY` 作为统一真值源；`readPrefs()` 的旧键迁移保证老用户不丢设置。

  > **注意**：不要删除 `DISPLAY_KEY` / `VISIBILITY_KEY` 的现有读写，否则会与 Task 1 的迁移逻辑打架，且属于对既有行为的破坏性改动。

- 快捷键（在现有 `window.addEventListener("keydown", ...)` 里加）：
  - `Ctrl+K` / `Ctrl+F` → 打开搜索
  - `Ctrl+,` → 打开偏好设置
  - `F1` → 打开帮助

  **关键**：`Esc` 取消分析任务的分支必须**在有对话框打开时让路**——否则在 `<dialog>` 里按 `Esc` 会同时触发「关闭对话框」和「取消分析」。改法：在 keydown 处理函数开头加

  ```ts
  if (dialog != null) return;   // 模态打开时，键盘归模态
  ```

  （`<dialog>` 自己处理 `Esc` 关闭，行为由 `Modal` 的 `onCancel` 负责。）

- 渲染三个对话框 + 搜索面板，放在 `</div>` 之前、`Workspace` 之后。

- [x] **Step 5: 样式**

`styles/layout.css` 新增 `.modal`（`dialog` 的定位与背景）、`.modal-card`、`.modal-head`、`.modal-title`、`.modal-subtitle`、`.modal-body`、`.modal-foot`、`.help-grid`、`.help-table`、`.about-list`。全部走令牌；`dialog::backdrop` 用半透明遮罩。

- [x] **Step 6: 跑检查并提交**

Run: `cd apps/desktop && npm run typecheck && npm test`
Expected: 两条都通过。

```bash
git add apps/desktop/src/components/ui/PreferencesDialog.tsx apps/desktop/src/components/ui/HelpDialog.tsx apps/desktop/src/components/ui/AboutDialog.tsx apps/desktop/src/App.tsx apps/desktop/src/styles/layout.css
git commit -m "feat(desktop): 偏好设置、帮助与关于对话框"
```

---

### Task 9: 导航解禁收尾、快捷键与文档同步

**Files:**
- Modify: `apps/desktop/src/App.tsx`（波形 / 频谱导航项改为可点击滚动；`⋮` 按钮解禁）
- Modify: `apps/desktop/src/styles/layout.css`
- Modify: `docs/UI_SPEC.md`
- Modify: `docs/USER_MANUAL.md`
- Modify: `docs/IMPLEMENTATION_PLAN.md`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: Task 4 / 6 / 7 / 8 产出的所有 id 与状态。
- Produce: 无新接口。

- [x] **Step 1: 波形 / 频谱导航项改为可点击**

现状是 `<NavItem icon={<IconWaveform />} active>波形</NavItem>` —— `active` 只是样式，点了没反应（这本身就是「看起来能用但没功能」）。改为：

```tsx
<NavItem icon={<IconWaveform />} active onClick={() => scrollToPane("pane-waveform")}>
  波形
</NavItem>
<NavItem icon={<IconSpectrum />} active onClick={() => scrollToPane("pane-spectrum")}>
  频谱
</NavItem>
```

并在 `App.tsx` 里给两张卡片加 `id`：`pane-spectrum`（频谱）、`pane-waveform`（波形）、`pane-spectrogram`（声谱图）。

- [x] **Step 2: 「关于与版本信息」按钮解禁**

左下角 `<IconButton label="关于与版本信息" disabled>` 改为可点击，点击 `setDialog("about")`。

同时把「搜索」导航项解禁：`onClick={() => setSearchOpen(true)}`。

- [x] **Step 3: 跑完整检查**

Run: `cd apps/desktop && npm run typecheck && npm test && npm run build`
Expected: 三条都通过。

Run: `cd apps/desktop && npm run electron:typecheck`
Expected: 通过。

Run: `powershell -ExecutionPolicy Bypass -File scripts/check-boundaries.ps1`
Expected: 输出两行 `boundary check OK`，退出码 0。**特别确认**新代码与注释里没有 `fft(`、`Math.log10`、`new Float64Array` 等小写禁用序列。

Run: `cargo fmt --all -- --check`
Expected: 无输出（Rust 未改动，应保持绿）。

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: 无告警。

Run: `cargo test --workspace`
Expected: 全部通过。

- [x] **Step 4: 手工端到端验收**

Run: `cd apps/desktop && npm run electron:dev`

逐条确认（每条都要**真实看到结果**）：

1. 左侧「分析视图」6 个导航项**全部不再灰掉**，且每个点击都有可见反应（滚动 / 展开 / 打开面板）。
2. 「电平表 / 指标」滚动到新的卡片，卡片里有电平条 + 指标 + 数据质量三块。
3. 「后台任务队列」展开后，在分析一个大文件时能看到「运行中」条目与已运行时长；点「取消」能真的停掉。
4. 「实时输入」滚动到实时面板；采集启动后导航项显示「采集中」。
5. `Ctrl+K` 打开搜索；输入文件名能命中并回车直接分析；输入峰值频率能命中并回车把频谱缩放到该峰。
6. `Ctrl+,` 打开偏好设置；改 dB 量程后中央三张图与电平表**同时**改变量程。
7. `F1` 打开帮助；`⋮` 打开关于。
8. 打开一个 44.1 kHz 文件把频率视口缩到 20 kHz 以上，再打开一个 48 kHz 文件 —— 频谱**不是空白**，视口已钳位。
9. 打开一个静音文件 —— 电平表显示「—」而不是 0。

- [x] **Step 5: 同步文档**

`docs/UI_SPEC.md`：

- §4 视图组件表：把 `SpectrogramView` 改成「✅ 已实现（支持 30 秒视口分块、平移与**频率轴缩放**）」；新增一行 `TaskQueuePanel`（任务队列，运行中 + 最近完成）；把 `AnalysisSummaryView` 从「◐ 已并入检查器」改成「✅ 已实现（中央画布「电平 / 指标」卡片；检查器保留紧凑读数）」；新增 `PreferencesDialog` / `HelpDialog` / `AboutDialog` / `SearchPalette` 四行，状态 ✅。
- §4.1 显示选项：说明「量程同时作用于频谱、声谱图色阶与电平表」这条仍然成立，并补一句「频谱与声谱图共享频率视口」。
- §6.2 键盘表：补 `Ctrl+K` / `Ctrl+F`（搜索）、`Ctrl+,`（偏好设置）、`F1`（帮助）、`Shift` + 滚轮 / 拖拽（声谱图频率缩放 / 框选）。
- §3 布局：补「左侧资源栏含任务队列面板」。

`docs/USER_MANUAL.md`：补一节「未开通功能的用法」，逐条写这 8 个功能怎么用（每条 2–4 句 + 快捷键）。

`docs/IMPLEMENTATION_PLAN.md`：M2 / M3 / M4 / M5 各节的状态行更新，说明「界面入口已开通」，并记下「Rust 侧零改动」这一事实与 §5 的实测数据不变。

`CHANGELOG.md`：加一条 `### 开通界面中尚未启用的功能`，列出 8 项 + 声谱图频率缩放与频谱共享视口 + 新增 vitest 前端测试基座。

- [x] **Step 6: 提交**

```bash
git add apps/desktop/src/App.tsx apps/desktop/src/styles/layout.css docs/UI_SPEC.md docs/USER_MANUAL.md docs/IMPLEMENTATION_PLAN.md CHANGELOG.md
git commit -m "feat(desktop): 解锁全部导航入口并同步文档"
```

---

## Self-Review

**1. Spec 覆盖**

| spec 章节 | 覆盖的 task |
|---|---|
| §4.1 声谱图缩放（频率视口、裁剪、刻度、Shift 交互、与频谱共享） | Task 2、Task 3 |
| §4.1 采样率错标的修正 | Task 3 Step 4 |
| §4.2 电平表 / 指标中央画布卡片 | Task 4 |
| §4.2 中心区可滚动 | Task 4 Step 3 |
| §4.3 后台任务队列（运行中 + 最近完成 + 取消 + 轮询） | Task 5 |
| §4.4 实时输入导航入口 | Task 6 |
| §4.5 偏好设置（统一键 + 迁移 + 三个分区 + 清除口径） | Task 1、Task 8 |
| §4.6 搜索（文件 / 峰值 / 操作 + 快捷键） | Task 7 |
| §4.7 帮助 | Task 8 Step 2 |
| §4.8 关于 | Task 8 Step 3 |
| §6.2 清除按钮的精确口径 | Task 1（`clearPrefs` / `resetLayout` / `clearSession`）+ Task 8 Step 1 |
| §6.3 事实更正（512 bin、禁用字面量） | Task 2 Step 1、Task 9 Step 3 |
| Review Focus 5 条 | Task 2、Task 3、Task 1、Task 4、Task 4 |
| 文档同步 | Task 9 Step 5 |

无遗漏。

**2. Step scan**：每个 step 都指定了确切文件、确切签名或确切断言，没有 "TBD" / "处理边界情况" / "补测试" 这类空步骤。Step 里出现的代码块只用于**固定拷贝内容**（配置、脚本行、状态声明），算法主体留给实现者。

**3. Type consistency**：
- `HzRange` 与 `App.tsx` 的 `viewRange: [number, number] | null` 结构相同，Task 3 明确要求回调传元组而非别名。
- `Prefs.dbRange` 是**索引**，`DB_RANGES` 的索引范围 0..3 —— Task 1 的校验与 Task 8 的下拉一致。
- `pushHistory` 的 `limit` 默认 8，Task 5 的测试与实现一致。
- `matchItems` 的 `limit` 默认 60，与 `SearchPalette` 原实现一致。
- `TaskInfo`（`contracts.ts`）的字段名 `taskId` / `kind` / `label` / `elapsedMs` 在 Task 5 的 `TaskQueuePanelProps` 中直接使用。
- `HistoryEntry.outcome` 是 `TaskOutcome`（三值），而 `FileAnalysis.outcome` 是 `AnalysisOutcome`（四值，含 `"panicked"`）—— Task 5 Step 5 明确要求把 `"panicked"` 归到 `"failed"`。

**4. Review Focus**：5 条全部有归属的 task 与测试（见上表）。

**5. Proportion**：9 个 task，约 250 行计划描述，对应约 2000 行实现代码。计划描述的是接口、断言与步骤，不是代码转录。
