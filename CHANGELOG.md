# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Because Auralis Studio is pre-1.0, treat `0.x` minor bumps as breaking.

## [Unreleased]

### Added

- **Control lane for capture commands.** `input_devices`, `start_realtime` and
  `stop_realtime` moved out of the slow command queue into their own lane
  (1 worker, bounded queue of 2). They used to share the slow lane with file
  analysis, whose 2 workers were both busy whenever two analyses ran — so
  "stop capture" had to queue behind an analysis that could take minutes,
  while the microphone kept recording and the ring buffer kept dropping
  frames. Measured on a 15-minute WAV with two concurrent analyses: the stop
  command now returns in ~1 ms *while the analyses are still running*; before
  the change it returned only after both had finished.
- **Engine request timeout.** The Electron↔engine bridge now rejects a
  request that gets no answer (default 60 s) instead of leaving the UI
  spinning forever. Commands that legitimately re-decode a whole file
  (`analyze_file`, `export_analysis`, `spectrogram_view`) opt out — a long
  analysis is not a hang, and `cancel_task` remains available.
- **Panic isolation on every RPC lane.** A command that panics now returns a
  normal `ok:false` error. Previously the worker thread simply died: the
  request was never answered (permanent spinner in the UI) and the pool lost
  a worker without a word in the logs.
- **DSP correctness and accuracy pass.**
  - **Waveform min/max, per channel, whole timeline.** The wire format now
    carries `waveformMin` alongside `waveformEnvelope` (channel-major,
    `waveformPoints` per channel). The old envelope was positive-only,
    kept only the tail of the file, and every channel drew channel 0. The new
    `StreamingEnvelope` merges buckets as it decodes, so memory is bounded
    (~64 KB for 2048 points stereo) and independent of file length.
  - **Real-time levels are per channel.** Peak/RMS are computed per channel and
    aggregated energy-preservingly (peak max, RMS `sqrt(mean(rms_c²))`).
    Antiphase stereo (`L=+1, R=-1`) no longer reads as silence; mid-sum is kept
    only for the mono-compatible spectrum display. New
    `peakDbfsPerChannel` / `rmsDbfsPerChannel` fields drive the meters.
  - **Clipping detection is no longer a stub.** `StreamingFlatTop` keeps run
    state across decode blocks, so a flat top split by a block boundary is
    counted once. `clippingRegions` now reflects real data.
  - **Flat-top and 4-term Blackman-Harris windows.** Flat-top cuts worst-case
    scalloping from ≈1.42 dB (Hann) to ≈0.01 dB at ENBW 3.77 bins;
    Blackman-Harris gives ≈-92 dB sidelobes at ENBW 2.00 bins. Both have
    analytic CG/ENBW tests; no UI selector yet.
  - **Peak picking filters sidelobes and noise.** `detect_peaks_with` adds
    prominence (≥6 dB), minimum spacing (≥2 bins) and an 80 dB dynamic-range
    gate; the file spectrum panel uses it. Frame-level callers keep the
    permissive defaults so their behaviour is unchanged.
- **All previously disabled UI entries are now live.** Eight sidebar/footer
  entries were still greyed out with a milestone tag even though the underlying
  capability existed. They now do real work:
  - **Spectrogram frequency zoom** (`Shift` + wheel, `Shift` + drag to select a
    band). The zoom *crops* the visible bins and scales them to the plot area
    instead of stretching the whole matrix — stretching would break the
    frequency↔bin correspondence and make every reading wrong. The frequency
    viewport is shared with the spectrum view, so focusing one focuses both.
  - **Electrical level / metrics view** as a fourth card in the central canvas:
    level meters, signal metrics and data-quality diagnostics (over-full-scale
    samples, flat-top regions, sanitised samples, file integrity).
  - **Background task queue** in the sidebar: running tasks (cancellable) plus
    the analyses that just finished. Rust already exposed `task_queue`; the
    outcome of a finished task is only known to whoever started it, so the
    history is kept on the frontend.
  - **Live-input navigation entry** with a "capturing" indicator.
  - **Preferences dialog** (`Ctrl+,`) unifying the previously scattered
    preferences behind one storage key, with migration from the old keys.
  - **Global search** (`Ctrl+K` / `Ctrl+F`) over files, detected peaks and
    commands.
  - **Help** (`F1`) and **About** dialogs.
- **Frontend unit-test baseline.** vitest (devDependency only) with `npm test`
  wired into the CI frontend job. The new pure logic — preference validation
  and migration, viewport↔bin conversion, task history, search matching — is
  extracted into DOM-free modules so it can be covered directly.
- **Fixed a wrong frequency axis on the spectrogram.** It drew the *file's*
  matrix but labelled the axis with the *capture device's* sample rate, so the
  axis was globally offset while live capture was running and the picture looked
  perfectly normal.
- **Fixed rapid wheel zooming applying only once.** Consecutive wheel events
  read a stale prop because React had not re-rendered between them, so four
  notches of the wheel moved the viewport one notch.

- **Viewport-chunked spectrogram.** The spectrogram no longer stops at the first
  30 seconds with no way to see the rest. A new `spectrogram_view` command takes
  `startSeconds` and `windowSeconds`, seeks in the container (sample-accurate
  `Accurate` seek, then skips the remainder of the packet), decodes only that
  window, and runs STFT on it. Memory and CPU stay proportional to the window,
  not to the file length. The UI exposes ◀ / 开头 / ▶ buttons plus wheel, drag
  and double-click, shows the absolute time range, and cancels superseded
  requests by task id.
  - `BlockDecoder::seek_to_frame` was added to `auralis-codec` and covered by a
    real-WAV integration test. Containers that cannot seek fall back to decoding
    from the start (correct, slower, with a warning).
  - The first 30 seconds are still embedded in `analyze_file`, so the initial
    view needs no extra round trip.
- **Real-time input capture** (`crates/auralis-audio`): a CPAL input stream with a
  bounded lock-free ring buffer, and an analyser thread that never runs in the
  audio callback. The callback only takes a recycled buffer, copies, pushes, and
  counts drops.
  - `start_capture` negotiates the format against the device's advertised f32
    ranges instead of failing when the request differs from the device default;
    when nothing matches it falls back and **reports the format actually used**
    (`CaptureSession::format`), so the frequency axis cannot silently shift.
  - The analyser keeps a rolling `fft_size` window and downmixes interleaved
    multi-channel data by averaging all channels. Fully anti-phase stereo
    cancels to digital silence and is shown as `—`, not a plausible-looking number.
  - Drops are counted in two places and both are displayed: `dropped_frames`
    (the ring overflowed) and `snapshot_overwrites` (the UI missed a generation).
    Neither counter is incremented per publish, so they stay meaningful.
  - Realtime snapshots cross the IPC boundary as little-endian `f32` bytes
    (`Float32Array` in the UI), per ADR-018 rule 2.
  - **Not verified on real hardware.** Only the missing-device error path and
    logic-level tests have been executed.
- **Session and crash recovery.** The last successfully opened file is persisted
  (`auralis.session.v1`) and re-analysed on the next launch, so a restart does
  not lose the file or its analysis. Electron's main process keeps a
  `auralis-running.lock` marker in `userData`: it is written at startup and
  removed in `will-quit`, so its presence on the next launch means the previous
  run did not exit cleanly. The status bar distinguishes
  “上次会话未正常结束，已恢复：…” from a normal “已恢复上次打开的文件：…”.
  - Verified by three real restarts: forced kill → recovered with the crash
    notice; graceful window close → recovered with the normal notice; both
    re-analysed the 40 s WAV and rebuilt the spectrogram.
  - The restored unit is the file, not an in-flight result. A crash during
    analysis means that analysis runs again on startup (deterministic, at the
    cost of one decode).
- **Realtime panel** in the sidebar: device picker, start/stop, sample rate,
  channel count, processed samples, and the two drop counters. While capturing,
  the spectrum pane is fed by the live snapshot and marked with a "实时" badge.
- **Level meter** (`LevelMeter`): per-channel peak bars with a shared RMS marker
  and an over-full-scale warning. The colour gradient spans the whole track and
  is masked from the right, so a colour position always means the same absolute
  level — a shorter bar does not turn the whole meter green.
- **Display options**, persisted in `localStorage` (`auralis.display.v1`) and
  applied consistently to spectrum, spectrogram and meter:
  - spectrum shape: filled area or line only ("声线"), because a filled trace
    hides detail below roughly -100 dBFS;
  - dB range presets (-140…0, -100…0, -80…-10, -140…-40);
  - channel layout for stereo files: separate lanes or overlaid, for spotting
    differences and phase problems.
- **Data-quality readouts** in the inspector: over-full-scale samples, flat-top
  regions (explicitly labelled heuristic) and sanitised non-finite samples.
  They are shown even when zero, so "checked and clean" is distinguishable from
  "not measured".
- **CSV/JSON export** (`crates/auralis-storage`, 11 tests) with full provenance:
  app version, algorithm version, DSP spec revision, units, analysis parameters
  (FFT size, window, spectrum step, spectrogram grid, hop), source file, codec
  and sample rate. CSV is a long-format table (`series,index,x_hz,x_s,value,unit,quantity`)
  with a `#`-prefixed metadata block; JSON declares units once and carries the
  spectrum/waveform arrays.
  - Non-finite values (silence = `-inf`) become `null` (JSON) / empty fields
    (CSV), never `0`, and their count is reported as `non_finite_values`.
  - Writing is atomic (`.part` + rename); a failed write never leaves a
    half-written file that a user might mistake for data.
  - A destination whose extension contradicts the requested format is rejected
    instead of producing a `.json` file containing CSV.
  - Export re-runs the analysis rather than accepting decimated view data from
    the UI, so the file is reproducible on its own and matches what is on screen.
- **Layout persistence**: panel widths and panel visibility survive restarts
  (`localStorage`, versioned keys, values validated and clamped on read).
- **Cancellable file analysis.** The task id is now allocated by the caller and
  passed into `analyze_file`, because a task id that only arrives on return can
  never be used to cancel work that is still running. `AppState::try_register_token`
  rejects duplicate ids. The UI exposes cancellation from the top bar and the
  status bar, and `Esc` does the same; cancelling keeps the previous result and
  reports "已取消" instead of an error.
- **Drag & drop** of audio files onto the window (any supported extension), and
  clicking an entry in the file list re-analyses that path without reopening the
  native dialog.
- **Retry** from the error banner (`Ctrl+R`), plus `Ctrl+O` for open. The banner
  now carries action buttons instead of only printing a sentence.
- **Per-channel peak readout** in the inspector for multi-channel files.
- Regression test locking the spectrogram's time span to the analysed duration.

### Changed

- **External links are restricted to http(s).** The Electron main process
  handed every URL the renderer asked to open to `shell.openExternal`, which
  launches whatever program the URL's scheme names — a `file://` or a custom
  scheme would have been executed by the system. The decision is now a
  tested predicate (`electron/external-links.ts`), and the vitest run covers
  the `electron/` directory as well as `src/`. Chromium blocks some schemes
  on its own (a `file://` `window.open` never even reaches the handler), but
  that is the browser's policy, not ours — the second layer is ours.
- **External dependency versions are declared once.** `cpal` and
  `crossbeam-queue` were the only two dependencies that carried a version
  number in a member crate instead of the workspace table, which ADR-007 does
  not allow. Both now live in `[workspace.dependencies]`; the resolved
  versions are unchanged, so `Cargo.lock` did not move.
- **`App.tsx` is now an assembly layer instead of everything.** It had grown to
  ~1 240 lines owning file opening, analysis, live capture, the task queue,
  export, search, drag-and-drop and keyboard shortcuts in one component. The
  concerns that stand on their own now live in `src/lib/`:

  | Concern | Module |
  |---|---|
  | Display and analysis preferences | `use-prefs.ts` |
  | Background task queue and history | `use-task-queue.ts` |
  | Live-capture readouts | `use-live-capture.ts` |
  | Keyboard and drag-and-drop | `use-shortcuts.ts` |
  | Export | `use-export.ts` |
  | Search-palette entries | `palette.ts` (pure function) |

  `App.tsx` is ~920 lines and keeps what is genuinely coupled: the file
  session, the synthetic-signal analysis and the spectrogram viewport — all of
  them read and write the same `display` (what is currently on screen), so
  splitting them further would mean passing that state around instead of
  removing it.

  The search palette became a pure function with 8 unit tests, which is the
  part that had no coverage at all. Behaviour is unchanged; the app was
  re-verified end to end in the real container (mount, five-region layout,
  Ctrl+K / Esc / Ctrl+, shortcuts, preference persistence, task queue,
  analysis of a real file, canvas pixels, export button, live panel).
- **`App.tsx` split into presentational components and pure helpers.** The file
  had grown past 2 200 lines, mixing application state, IPC and roughly 900 lines
  of JSX. The sidebar, inspector and central canvas now live in
  `src/components/panels/` (`ResourcePanel`, `InspectorPanel`, `AnalysisCanvas`)
  and hold no state of their own; the label / error / format / scroll / session /
  view helpers moved to `src/lib/`. `App.tsx` is now ~1 300 lines and owns only
  state, effects and handlers. The extracted markup was normalized and diffed
  against the original, then the app was re-run end-to-end; there is no intended
  behaviour change.
- **Desktop container replaced: Tauri 2 → Electron 45** (ADR-019). On the
  reference machine, maximizing the Tauri window deterministically produced
  screen-wide artifacts and taskbar jitter. A minimal Electron shell loading
  the same Vite page did not reproduce them, so the container was replaced
  rather than gambling on a WebView2/driver combination. Electron bundles its
  own Chromium, which removes the machine-dependent runtime version from the
  equation; the cost is a larger installer and higher memory use.
- **The Rust command layer is now a separate process** (`crates/auralis-engine`,
  previously `apps/desktop/src-tauri`). Electron's main process spawns it and
  communicates over a stdio JSON-Lines RPC protocol: no TCP port, no auth, and
  lifetime tied to stdin. Slow commands run on two bounded workers; the
  high-frequency polling commands stay inline so realtime snapshots never queue
  behind a multi-second file analysis. Binary view data still crosses the
  boundary as bytes (base64 on the wire, `Uint8Array` in the renderer), never
  as JSON number arrays.
- **Renderer capabilities narrowed to a preload bridge.** `sandbox: true`,
  `contextIsolation: true`, `nodeIntegration: false`; `window.auralis` exposes
  only `invoke`, file dialogs, and `getPathForFile`.
- **Drag-and-drop now uses the DOM drop path** and resolves the file path in
  preload via `webUtils.getPathForFile` (Chromium no longer exposes `File.path`).
  The previous Tauri `onDragDropEvent` listener is gone.
- **Development is one command again**: `npm run electron:dev` starts Vite,
  waits for the port, then launches Electron; `npm run electron:start` runs the
  production flow. The render self-check moved from `src/dev/` to `dev/` so the
  production audio-math boundary scan does not treat synthetic fixtures as UI
  code. `?open=<absolute path>` on the dev URL auto-analyses a file, which is
  what made the real-file UI checks scriptable without driving the native
  dialog.
- **Visual revision of the desktop UI**, modelled on two reference interfaces
  (a dark developer-tool dashboard and a shadcn-style component sheet): new
  neutral elevation ramp (`bg-app` → `bg-sidebar` → `bg-panel` → `bg-elevated`),
  4/6/8 px radius scale, 22/26/30 px control heights, 44 px top bar, card
  headers with title + metadata, sidebar navigation with milestone tags,
  disclosure sections in the inspector, and segmented/icon/toggle controls.
  Chart rendering was reworked to match: gradient fills under the spectrum and
  waveform traces, a dB colour-scale bar on the spectrogram, magnitude-adaptive
  time axes (µs/ms/s) instead of fixed `0:00.000`, and explicit empty states.
- Added the L1 component layer ADR-017 called for, which did not exist:
  `components/ui/{primitives,icons}.tsx` (Button, IconButton, Input, Select,
  Checkbox, FormField, DataRow, Collapsible, NavItem, NavGroup, ErrorBar,
  EmptyState, inline SVG icon set). No new runtime dependencies.
- **Frontend toolchain upgraded**: Vite 6.4 → 8.3, `@vitejs/plugin-react`
  4.7 → 6.1, TypeScript 5.9 → 7.0 (the Go-native compiler, reached via the
  6.0 intermediate release Microsoft recommends). TypeScript went 5.9 → 6.0 →
  7.0 rather than straight to 7.0 because 7.0 turns 6.0's deprecations into
  hard errors.
- `build.minify` moved from `"esbuild"` to `"oxc"`. Vite 8 no longer bundles
  esbuild; Oxc is the new default minifier.
- Added `esbuild` as a devDependency for one reason only: `build.target` is
  still a real browser version, so Vite's `vite:esbuild-transpile` plugin does
  not skip and needs esbuild present for downleveling. It does not minify.

### Fixed

- **The realtime rolling window reallocated its buffer on every round.** The
  window was reserved for `fft_size * 2` samples, but a round can push up to
  64 callback buffers (16 384 frames) before the old samples are trimmed, so
  the `Vec` outgrew its capacity and reallocated — every round, in the
  analysis worker. The capacity is now reserved for the whole round up front
  and only ever grows.
- **Task progress events were never actually throttled.** The comment claimed
  "进度事件节流" but there was no throttling: a long analysis emits one
  progress notification per decoded block, which fills the 256-slot channel
  and squeezes out everything else — including the terminal event. Progress
  is now throttled to one per 100 ms, and the terminal event is delivered
  with a bounded wait instead of being dropped along with the rest.
- **The realtime recycle pool could reallocate on every callback on
  multi-channel devices.** The pool was preheated with `frames * 4` samples,
  which quietly assumes at most four channels. A six- or eight-channel
  interface delivers more samples than that, so `extend_from_slice` had to
  grow the buffer — inside the audio callback, which `AGENTS.md` forbids.
  The capacity now follows the resolved channel count, and `resolve_format`'s
  fallback (which can turn a 2-channel request into an 8-channel stream)
  no longer turns a steady state into a per-callback allocation.
- **The ring buffer's capacity was counted in items but named in frames.**
  `SampleRing::new` took a `capacity_frames` argument that was really the
  number of *items* the queue holds. With 256 frames per callback buffer,
  "4096 frames" actually buffered 1 048 576 frames — 21.8 s at 48 kHz, not
  85 ms. Anyone sizing the ring from the name would have been off by three
  orders of magnitude. The parameter is now `capacity_items`, and
  `capacity_frames()` returns the real frame capacity.
- **The ring buffer's fill ratio was inverted.** It was computed as
  *frames consumed* over capacity, so the more the consumer drained the
  buffer, the fuller it looked — and after a couple of seconds of normal
  operation the reading sat at 1.0 permanently. It now reports actual
  occupancy (pushed − popped − evicted) over the real capacity, and the
  capacity can no longer be passed in wrong, because the ring knows it.
- **The default peak-picking config silently filtered frame-level peaks.**
  `PeakPickConfig`'s documentation said the default "keeps the old
  behaviour (no prominence / dynamic-range filtering)", but the default
  `dynamic_range_db` was 120 — which *is* a filter: any peak more than
  120 dB below the loudest one was dropped. `detect_peaks` is the entry
  point for the frame-level paths (STFT, live capture, synthetic analysis),
  so their peak lists quietly changed. The default is now "no gate"
  (infinity), which is what the documentation always claimed; callers that
  want a gate have to pass one explicitly, as the file-spectrum panel does.
- **The frontend architecture guard was a blacklist that `Math.log` walked
  straight through.** `scripts/check-boundaries.ps1` rejected
  `Math.log10` and `Math.log2` — but a dB conversion only needs
  `Math.log`, which was allowed, so the guard gave false assurance about
  the rule it exists to enforce. It is now a whitelist: the shipped renderer
  and the Electron main process may only use plain geometry and rounding
  (`abs`, `ceil`, `floor`, `max`, `min`, `round`, `sign`, `PI`). The guard
  also self-tests, so a rule that stops matching fails loudly instead of
  silently passing everything.
- **The realtime ring buffer's `popped_frames` counter could wrap to
  `u64::MAX`.** The `DropOldest` policy decremented a counter that had never
  been incremented for the evicted item. Evictions are now counted separately
  in `evicted_frames` (they are lost data and must be visible); `popped_frames`
  keeps its meaning of "frames the consumer actually took".
- **The realtime session lock was held across slow work.** Opening a device
  or joining the analysis thread happened while holding the mutex that the
  30 Hz snapshot poll needs, so the live spectrum could freeze for the whole
  duration of a device open. The lock now only guards the pointer swap.
- **Export no longer masquerades as analysis.** Export and analysis shared a
  single `busy` flag, so while a CSV/JSON export was running the top bar and the
  sidebar CTA both read 「分析中…」, and drag-and-drop or the command palette were
  rejected with 「已有分析在进行中」. The two operations now have separate flags
  (`analyzing` / `exporting`) and the busy/label policy lives in
  `src/lib/operations.ts` behind unit tests. Analysis may start during an export
  (the export re-analyses a path it captured itself, so it does not depend on the
  current selection), whereas an export is blocked during an analysis (the
  on-screen selection and display are being replaced). The spectrogram viewport is
  gated by analysis alone, not by export.
- **「分析合成信号」 looked like "the default data page".** Running the self-test
  replaced the canvas but kept the file's labels: the waveform panel still said
  `file.flac`, its time axis still spanned the file's duration, and the inspector
  still reported the file's properties — so a one-frame 1 kHz tone drawn across a
  204-second axis read as "nothing was analysed, here is some default data".
  `DisplayData` now records where it came from (`source: "file" | "synthetic"`),
  and every label follows it: the waveform and spectrum panel titles show a
  「合成信号」 tag instead of the file name, the waveform axis uses the actual
  signal length (`sampleCount ÷ sampleRate`, ≈85 ms at 48 kHz, not the envelope
  point count), the spectrogram says it has no data for a synthetic signal
  instead of「等待数据」, and the inspector's file section is marked 「未显示」 with
  an explicit note. The self-test section is renamed 「合成信号自检」 and starts
  collapsed. One click on the file name restores the file's analysis, and the
  status-bar notice says so.
- **The spectrogram time axis was short by the downsampling factor.** When the
  STFT produced more than 1024 frames, `to_matrix` reduced them to 1024 columns,
  but `spectrogram_hop_ms` still reported the source hop. A 30-second window was
  therefore labelled as ~10.9 seconds. The reported hop is now the *effective*
  hop (`source frames × hop ÷ output frames`), so `frames × hop` always equals
  the real span. This is locked by a 12-second regression test.
- **The canvas backing-store reallocation storm during window resize.** Every
  chart used to assign `canvas.width/height` on every draw, which destroys and
  recreates the GPU texture each time; one maximize triggered 16 reallocations
  in the same ~100 ms window as the dropped frames. The new `canvas-fit.ts`
  helper writes the backing store only when the device-pixel size actually
  changes and snaps CSS size to whole device pixels. Measured after the fix:
  42 draws produce 12 writes (the two real size changes). This fix is
  independent of the container migration.
- **A panic during analysis no longer takes the app with it.** `analyse_isolated`
  wraps the analysis in `catch_unwind`; a panic becomes an `Err` naming the file
  and the panic text, which surfaces in the error banner. This is the command-layer
  half of ADR-009's "one bad file must not bring down the app" (the task queue
  already had the other half).
- **The spectrogram was unreadable: transposed data and a stretched time axis.**
  Two independent bugs. (1) `SpectrogramView` indexed the matrix as
  `[bin][frame]` with a stride of `frames`, while `Stft::to_matrix` documents
  `[frame][bin]` with a stride of `bins`, so a 1 kHz tone rendered as vertical
  stripes instead of a horizontal line. (2) `analyze_file` passed the *caps*
  `SPECTROGRAM_FRAMES`/`SPECTROGRAM_BINS` as the exact target grid, so a 4 s
  file was upsampled into 1024 columns (~10.9 s of axis) and every column no
  longer corresponded to one hop. Both are fixed; the axis now spans the
  analysed window (`frames × hop`), not the file duration.
- **Every chart rendered empty.** `decodeF32` assumed `invoke` returned a
  `Uint8Array`; under Tauri 2 + WebView2 it returns a plain number array, so
  `bytes.buffer` was `undefined` and each analysis threw
  `Cannot read properties of undefined`. The helper now normalises both shapes,
  and `ipc.ts` types the payload as `number[] | Uint8Array` instead of asserting
  `Uint8Array` over the runtime difference.
- Raw `String(e)` no longer lands in the status bar. Failures are translated to
  one readable sentence (`describeError`), with the original text kept in the
  hover title.
- `Workspace` no longer collapses the sidebars to their minimum on any resize.
  The clamp is computed over the sum of both sidebars and only shrinks; before,
  every resize rewrote each panel to `avail * 0.4`.
- `SpectrogramView` no longer prints a time axis when it has no matrix, and
  `WaveformView` picks column aggregation vs. polyline by samples-per-pixel
  instead of always drawing a polyline.
- Added the missing `src/vite-env.d.ts`. The project never had Vite's standard
  client-types scaffold, which declares `*.css` modules. TypeScript 6 enables
  side-effect import checking by default, so `import "./styles/tokens.css"`
  started failing with TS2882.
- Removed `baseUrl` from `tsconfig.json`. It is deprecated in TypeScript 6
  (TS5101) and stops functioning in 7.0. `paths` has not needed it since
  TypeScript 4.1.
- Replaced `__dirname` with `import.meta.dirname` in `vite.config.ts`. The
  package is `"type": "module"`; `__dirname` only ever existed because Vite's
  bundled config loader injected it, and Vite 8 warns it goes away once
  `configLoader: 'native'` becomes the default.
- Removed all 7 `color-mix()` calls from `layout.css`. Their results are now
  precomputed tokens (`--btn-top`, `--btn-top-strong`, `--bg-note`,
  `--accent-border-strong`, `--accent-pressed`, `--signal-critical-border`,
  `--signal-critical-surface`), so the CSS runtime floor is really Chrome 110
  as `build.target` declares.

### Known limitations

These are real and are not going to be papered over:

- **The upgrade is not runtime-verified.** Typecheck, production build, the
  dev server, `cargo check`, and `cargo test` all pass, but the packaged Tauri
  application was never launched and no MSI/NSIS bundle was produced. Audio
  path and WebView2 rendering are unconfirmed end to end.
  *Update:* the Tauri debug binary was subsequently launched against the Vite
  dev server for the UI review, and the app rendered at devicePixelRatio 1.5
  (see `docs/screenshots/ui-dark-2026-10.png`); the bundled MSI/NSIS path is
  still unverified.
- **Spectrogram rendering is not screenshot-verified.** Loading a real file
  through the UI needs the native file dialog, which cannot be driven from the
  automation used here; the spectrogram, peak markers and its colour-scale bar
  are therefore code-reviewed but visually unconfirmed. The synthetic-signal
  path (spectrum + waveform) is what the screenshot covers.
- **The JS module count dropped from 42 to 29** under Rolldown. Bundle content
  was probed for expected component strings and the output got *smaller*
  (260.75 → 256.49 kB), but without running the app that is not proof of
  equivalent behaviour.
- **The CSS minifier silently changed** from esbuild to Lightning CSS
  (16.47 → 16.29 kB). A different minifier can produce different output; no
  visual comparison was done.

## [0.1.0] — 2026-10-19

First published version. The application builds, runs, and imports WAV files.
It does **not** yet do calibrated measurement, and this section says so
explicitly rather than leaving it to be discovered.

### Added

**Audio engine**

- Rust workspace with `auralis-core`, `auralis-dsp`, `auralis-codec`,
  `auralis-audio`, `auralis-task`, `auralis-visualization`, and the
  `auralis-desktop` Tauri shell
- WAV import with metadata inspection, cancellable chunked PCM decode, and
  deterministic Peak / RMS / DC offset / crest factor
- FFT with explicit amplitude correction, plus peak detection over the
  magnitude spectrum
- STFT with configurable window size and hop, feeding a spectrogram and a
  multi-level waveform pyramid
- Bounded lock-free SPSC ring buffer with `DropNewest` / `DropOldest` policies
  and drop instrumentation
- CPAL input capture with **buffer recycling** — the callback takes a buffer
  from a fixed 64-slot pool, refills it, and returns it to the ring, so steady
  state allocates nothing
- Criterion benchmarks for FFT, STFT, peak detection, and decimation

**Desktop application**

- Tauri 2 shell on the system WebView2, React 19 + TypeScript + Vite frontend
- Dark measurement-instrument UI with design tokens and custom Canvas 2D
  waveform, spectrum, and spectrogram rendering
- Spectrum zoom/pan, cursor readout, parameter inspector, and peak markers

**Verification**

- Independent numpy reference implementation (`scripts/oracle.py`) that
  cross-checks the Rust DSP
- Architecture boundary checker (`scripts/check-boundaries.ps1`) that fails if
  `auralis-core` / `auralis-dsp` acquire I/O, or if audio math appears in the
  frontend
- GitHub Actions CI: fmt, clippy `-D warnings`, tests, release build, IPC
  payload check, DSP regression against the oracle, Windows artifact upload
- 205 workspace tests

**Documentation**

- PRD, TAD, DSP spec, UI spec, test plan, implementation plan, and ADRs
  001–018 under `docs/`

### Fixed

- Removed `tauri-plugin-log`: it called `log::set_boxed_logger` during setup,
  which collided with `tracing-subscriber` and panicked on window open. CLI
  subcommands never reach `run()`, which is why this had gone unnoticed.
- `SampleRing::push` now returns the buffer on overflow instead of dropping it,
  which is what makes recycling possible on the real-time path.

### Known limitations

These are real and are not going to be papered over:

- **Only WAV has been verified** against real samples. FLAC, MP3, and AIFF
  compile in and decode, but have not been validated end to end.
- **No calibrated measurement.** No SPL, LUFS, SNR, THD, or THD+N. Those
  algorithms are not implemented, and nothing in this release should be read as
  a claim about measurement accuracy.
- **Real-time capture is not yet wired end to end.** The capture session and
  ring buffer exist and are tested; the live spectrogram path lands in M4.
- **FFT does not meet its original target.** `docs/BENCHMARKS.md` records
  271 µs for a 4096-point transform against a design goal of < 100 µs. The
  design goal was an unmeasured estimate; the benchmark is the truth.
- **Drop-and-import is not implemented** — only the file picker.
- **Windows only.** macOS and Linux are unadapted.

[Unreleased]: https://github.com/dengxingzhi000/auralis-studio/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/dengxingzhi000/auralis-studio/releases/tag/v0.1.0
