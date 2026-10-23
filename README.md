<div align="center">

**English** · [简体中文](./README.zh-CN.md)

# Auralis Studio

### A Windows-first desktop audio analyzer where every number is traceable.

**A Rust DSP kernel, a React display layer, and a hard rule against
reporting a figure nobody measured.**

[![CI](https://img.shields.io/github/actions/workflow/status/dengxingzhi000/auralis-studio/ci.yml?branch=master&style=flat-square&logo=github&label=ci)](https://github.com/dengxingzhi000/auralis-studio/actions/workflows/ci.yml)
[![Stars](https://img.shields.io/github/stars/dengxingzhi000/auralis-studio?color=ffcb47&labelColor=black&style=flat-square&logo=github)](https://github.com/dengxingzhi000/auralis-studio/stargazers)
[![Forks](https://img.shields.io/github/forks/dengxingzhi000/auralis-studio?color=8ae8ff&labelColor=black&style=flat-square&logo=github)](https://github.com/dengxingzhi000/auralis-studio/network/members)
[![Issues](https://img.shields.io/github/issues/dengxingzhi000/auralis-studio?color=ff80eb&labelColor=black&style=flat-square&logo=github)](https://github.com/dengxingzhi000/auralis-studio/issues)
[![Rust](https://img.shields.io/badge/Rust-1.99-007396?logo=rust&logoColor=white&style=flat-square)](https://www.rust-lang.org/)
[![Electron](https://img.shields.io/badge/Electron-45-47848F?logo=electron&logoColor=white&style=flat-square)](https://www.electronjs.org/)
[![React](https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=black&style=flat-square)](https://react.dev/)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue?style=flat-square)](#license)

**[Quick start](#quick-start) · [What it does](#what-it-does) · [Tech stack](#tech-stack) · [Architecture](#architecture) · [Verification](#verification) · [FAQ](#faq) · [Docs](#documentation)**

```powershell
git clone https://github.com/dengxingzhi000/auralis-studio
cargo build -p auralis-engine
cd auralis-studio\apps\desktop && npm install && npm run electron:dev
```

</div>

> [!IMPORTANT]
> **Auralis Studio is not a calibrated measurement instrument, and this release
> does not pretend to be.** There is no SPL, no LUFS, no SNR, no THD, and no
> THD+N. Those algorithms are not implemented. Read
> [Known limitations](#known-limitations) before you trust any number here —
> it is a short list and it is honest.

> [!NOTE]
> **Windows-only.** macOS and Linux are unadapted. The kernel is portable in
> principle; the shell, the CPAL capture path, and the static-CRT build are
> not.

---

Most audio tools on a desktop are a UI wrapped around a black box. When a
spectrum looks wrong, you cannot tell whether the window function was wrong,
the normalization was wrong, the file was decoded wrong, or nobody checked.

Auralis Studio is built the other way around. Every figure it displays comes
from a Rust function with a deterministic unit test, and the test suite
cross-validates that function against an **independent numpy implementation**
(`scripts/oracle.py`) that shares no code with the Rust side. When a benchmark
misses its target, the miss is recorded in
[docs/BENCHMARKS.md](./docs/BENCHMARKS.md) rather than quietly dropped.

The architectural spine is a single constraint: **the audio callback copies
samples and increments counters. Nothing else.** This is not caution — it is
measured. A 4096-point FFT takes 271 µs on the reference machine, against a
20.8 µs callback budget at 48 kHz. Anything heavier in the callback drops
frames by construction.

The result is an analyzer that is deliberately boring about its claims.

## Quick start

> [!NOTE]
> **The version is pinned.** `rust-toolchain.toml` fixes Rust 1.99.0. CI builds
> the workspace with `--locked`, so `Cargo.lock` is committed and must stay
> that way.

### Prerequisites

| Tool | Version | Why |
|---|---|---|
| **Rust** | 1.99.0 | Pinned by `rust-toolchain.toml` |
| **MSVC build tools** | — | `+crt-static` is enabled so the exe is genuinely standalone (ADR-013) |
| **Node.js** | 22.12+ or 24 | Required by Electron 45, and by vitest 5 (the unit tests need `node >= 22.12`). CI also builds the renderer on Node 20 (typecheck + build only; branch protection keeps that check name) |
| **Python** | 3.12 | Only for the numpy oracle cross-check |

Local installation paths and mirror configuration are in
[docs/DEV_ENVIRONMENT.md](./docs/DEV_ENVIRONMENT.md).

### 1. Frontend only

```powershell
cd apps\desktop
npm install
npm run dev          # Vite dev server
```

### 2. Full application

```powershell
cargo build -p auralis-engine          # the Rust engine is a separate process
cd apps\desktop
npm run electron:dev                  # starts Vite + Electron together
```

`npm run electron:dev` waits for the Vite dev server, then launches
Electron. The window loads `http://127.0.0.1:5173` and talks to the engine over
stdio; see [ADR-019](./docs/ADR.md#adr-019桌面容器从-tauri-2-迁移到-electron).

### 3. Production build

```powershell
cargo build -p auralis-engine --release
cd apps\desktop
npm run electron:start                # vite build + esbuild main/preload + electron .
```

There is **no installer yet** (M6). The engine binary must exist at
`target\debug\auralis-engine.exe` (or `AURALIS_ENGINE` must point at it);
a packaged build is expected to read `<resources>\engine\auralis-engine.exe`.

> [!WARNING]
> **The release binary is a GUI-subsystem executable.** Calling it directly
> from PowerShell does not reliably wait, so `$LASTEXITCODE` can come back
> empty. Use `Start-Process -Wait -PassThru` in any script that needs the exit
> code — `scripts/check-oracle.ps1` already does this.

### CLI (no window required — what CI runs)

```powershell
.\target\release\auralis-engine.exe --version
.\target\release\auralis-engine.exe --self-test
.\target\release\auralis-engine.exe --dump-reference out.json
```

`--dump-reference` writes the reference vectors that the numpy oracle is
compared against.

## What it does

| Icon | Area | What works today |
| ---- | ---- | ---------------- |
| 📂 | **Import** | WAV with metadata inspection, cancellable chunked PCM decode. FLAC and AIFF are verified against checked-in samples; MP3 decodes but is **not verified** — see [the table below](#container-support--measured-not-assumed) |
| 📈 | **Time-domain** | Peak, RMS, DC offset, crest factor, over-full-scale sample counting, per-channel independence |
| 📊 | **Frequency-domain** | FFT with explicit amplitude correction, peak detection with markers, zoom and pan, cursor readout |
| 🌈 | **Spectrogram** | STFT with configurable window and hop, rendered to a texture-backed canvas |
| 🔴 | **Real-time** | CPAL capture through a bounded lock-free ring buffer with buffer recycling and drop instrumentation |
| 🧮 | **Verified DSP** | Criterion benchmarks plus cross-validation against an independent numpy implementation |
| 🧱 | **Enforced boundaries** | CI fails if the DSP crates acquire I/O, or if audio math appears in the frontend |
| 🪟 | **Instrument UI** | Dark measurement-instrument design system, custom Canvas 2D rendering, Radix headless primitives |

## Tech stack

| Layer | Choice |
| ----- | ------ |
| **Desktop shell** | Electron 45 with a bundled Chromium — runtime version is fixed per release |
| **UI** | React 19 + TypeScript 7 + Vite 8 (sandboxed renderer, `contextIsolation`) |
| **Design system** | CSS custom-property tokens + Radix primitives (headless) + in-house components |
| **Visualization** | In-house Canvas 2D for waveform and spectrum; texture-backed canvas for the spectrogram |
| **DSP** | Rust: `auralis-dsp`, depending only on `rustfft` 6.4 |
| **Codec** | `symphonia` 0.6 with `mp3` and `aiff` features declared explicitly |
| **Concurrency** | Lock-free SPSC ring (`crossbeam-queue`), `catch_unwind` task isolation |
| **IPC** | Rust engine subprocess + stdio JSON-Lines; view data is pre-decimated and travels as base64 → `Uint8Array` |
| **Verification** | Rust unit tests + numpy oracle + CI |
| **Benchmarks** | Criterion 0.8 |

**No Ant Design, no MUI** (ADR-017). The panels in this application are
instruments, not forms.

## Architecture

```
   ┌──────────────────────────────────────────────────────┐
   │  React 19 · TypeScript · Vite  (sandboxed renderer)  │
   │  ── displays finished buffers. Computes no DSP. ──   │
   │                                                      │
   │  Workspace · Spectrum · Spectrogram · Inspector      │
   └───────────────────────┬──────────────────────────────┘
                           │  preload: window.auralis.invoke
                           │  structured clone (strings, Uint8Array)
   ┌───────────────────────▼──────────────────────────────┐
   │  Electron main process                               │
   │  window · dialogs · engine subprocess · lifecycle    │
   └───────────────────────┬──────────────────────────────┘
                           │  stdio JSON-Lines RPC
                 scalars  │  base64 field → Uint8Array
   ┌───────────────────────▼──────────────────────────────┐
   │  auralis-engine  (separate process)                  │
   │  fast lane inline · control lane · slow lane (2/8)   │
   └───────┬───────────────────────────┬──────────────────┘
           │                           │
   ┌───────▼────────┐         ┌────────▼─────────────────┐
   │  Audio thread  │         │  Task engine             │
   │  ┌───────────┐ │         │  auralis-task            │
   │  │ CPAL      │ │         │  cancel token · progress │
   │  │ callback  │ │         │  queue · catch_unwind    │
   │  └─────┬─────┘ │         └────────┬─────────────────┘
   │        │ recycle buffer          │ pull chunks
   │        ▼                         ▼
   │  ┌──────────────────────────────────┐
   │  │  SPSC ring  (bounded, lock-free)  │  ◀── drops are counted, not hidden
   │  └───────────────┬──────────────────┘
   │                  │
   │  ┌───────────────▼──────────────────┐
   │  │  auralis-codec   auralis-dsp     │
   │  │  decode          RMS · Peak · DC │
   │  │  symphonia       FFT · STFT · peaks│
   │  └───────────────┬──────────────────┘
   │                  │
   │  ┌───────────────▼──────────────────┐
   │  │  auralis-core  (pure)            │  no UI · no device API
   │  │  types · units · errors · time   │  no filesystem · no DB
   │  └──────────────────────────────────┘
   └──────────────────────────────────────┘
```

**The one rule the whole diagram encodes:** React never computes audio. dB
conversion, FFT, decimation, and envelope building all finish in Rust before
anything crosses the IPC boundary. `scripts/check-boundaries.ps1` fails the
build if that stops being true, or if `auralis-core` / `auralis-dsp` acquire a
dependency on `std::fs`, `cpal`, `symphonia`, or anything UI-shaped.

### Buffer recycling — why the capture code looks the way it does

A CPAL callback has roughly 20.8 µs at 48 kHz. Allocating a `Vec<f32>` per
callback would be one allocation per callback forever, which the brief forbids
outright. So the callback takes a buffer from a fixed pool of 64, `clear()`s
it, refills it (capacity is already sufficient, so no reallocation), and pushes
it into the ring. The consumer returns buffers to the pool when it is done.

```rust
// what the callback does, in full
let mut buf = recycle.pop().unwrap_or_else(Vec::with_capacity);
buf.clear();
buf.extend_from_slice(input);
let _ = ring.push(buf);   // Err(buf) on overflow — reuse it, don't drop it
```

The first few callbacks allocate while the pool fills. After that the steady
state is zero allocations, and the pool is bounded, so it cannot grow without
limit.

"Capacity is already sufficient" only holds if the pool is preheated for the
*resolved* channel count: one interleaved callback delivers `frames × channels`
samples. Preheating for a fixed four channels — as the code used to — makes
every callback reallocate on a six- or eight-channel interface, and
`resolve_format`'s fallback can turn a 2-channel request into an 8-channel
stream. The capacity is now derived from the format the device actually got.

This is also why `SampleRing::push` returns `Result<(), T>` rather than `bool`:
on overflow the buffer has to come back to the caller, or recycling is
impossible.

## Container support — measured, not assumed

A decoder that compiles is not a format that works. A format is marked verified
only when a real sample round-trips through import → DSP and the result is
checked.

| Container | Compiled in | Verified against checked-in samples |
| --------- | ----------- | ------------------------------------ |
| **WAV** | yes | **yes** — 13 integration tests covering write → decode → DSP |
| **FLAC** | yes | **yes** — 4 integration tests + a fixture generator (`--ignored`), sample produced by the third-party `flacenc` encoder |
| **AIFF** | yes (`aiff` feature declared explicitly) | **yes** — 4 integration tests + a fixture generator (`--ignored`), sample produced by this crate's hand-written writer |
| MP3 | yes (`mp3` feature declared explicitly) | no |

Sample strength differs and the docs should not blur it: WAV and AIFF fixtures
come from hand-written writers in this repository, FLAC from a third-party
encoder. None of them is a file captured from the real world, so "verified"
here means "the writer and symphonia are two independent implementations that
agree", not "tested against arbitrary field recordings".

`verified_containers()` is a hard constraint in the UI: anything not in that
list is rendered with an **unverified** marker rather than presented as
supported. See `docs/TEST_PLAN.md` §3 and the "do not claim unverified formats"
rule in [AGENTS.md](./AGENTS.md).

> [!NOTE]
> symphonia's default features **do not** include mp3 or aiff — measured
> against symphonia 0.6.1, not assumed.

> [!NOTE]
> MP3 stays unverified because this machine has no MP3 encoder (`ffmpeg`,
> `lame`, and `sox` are all absent) and downloading one is not an option here.
> A reproducible sample cannot be produced, so no claim is made.

## Verification

This is the part of the project that is unusual, so it goes first in the docs
and it is worth reading before trusting any figure.

### Cross-validation against numpy

`scripts/oracle.py` is a second implementation of the DSP written in numpy. It
shares no code with the Rust side, so agreement between them is evidence that
the Rust implementation is right rather than merely self-consistent.

```powershell
cargo build --workspace --release --locked
./scripts/check-oracle.ps1
```

CI additionally asserts that eleven named DSP tests still exist, so the
properties cannot be quietly dropped:

| Test | What it pins down |
| ---- | ----------------- |
| `dc_offset_is_the_mean` / `dc_offset_is_signed` | DC offset is the mean, and sign is preserved |
| `silence_produces_zeroes_not_nan` | Silence does not produce NaN |
| `non_finite_samples_are_sanitized_and_counted` | NaN / Inf are handled and counted |
| `over_full_scale_is_counted_not_clipped` | Out-of-range values are counted, not clamped |
| `flat_topped_waveform_is_detected_as_suspicious` | Flat tops are flagged |
| `clean_sine_is_not_reported_as_clipping` | …and a clean sine is not |
| `channel_isolation_l_and_r_are_independent` | Channels do not bleed into each other |
| `peak_of_sine_tracks_amplitude` | Peak amplitude is correct |
| `rms_of_sine_is_amplitude_over_sqrt_two` | RMS normalization is correct |
| `crest_factor_of_full_scale_sine_is_about_3_01_db` | Crest factor is ≈ 3.01 dB |

### Benchmarks

Criterion, release profile, 48 kHz. Full method and caveats are in
[docs/BENCHMARKS.md](./docs/BENCHMARKS.md).

| Benchmark | Median |
| --------- | ------ |
| `fft/fft_1024` | 77.2 µs |
| `fft/fft_4096` | 270.7 µs |
| `fft/fft_16384` | 1.054 ms |
| `peak_detection/detect_peaks_2049_bins` | 65.4 µs |
| `stft/stft_2048_512_1s` | 3.470 ms |
| `stft/stft_range_2048_512_0p1s` | 263.8 µs |
| `downsample/to_800_from_8193` | 19.8 µs |

Against the 20.8 µs callback budget at 48 kHz, that makes the architecture a
measurement rather than an opinion:

| Operation | Median | × budget |
| --------- | ------ | -------- |
| FFT 4096 | 271 µs | **13.0×** |
| FFT 1024 | 77 µs | 3.7× |
| Peak detection | 65 µs | 3.1× |

> [!WARNING]
> **The FFT missed its original target, and the record says so.** The design
> document set "< 100 µs for a 4096-point FFT". Measured, it is 271 µs — 2.7×
> over. The target was an estimate written before any measurement existed;
> the benchmark is the truth. This project does not claim that FFT meets any
> performance target.

### The full check suite

```powershell
./scripts/check-boundaries.ps1   # architecture boundaries
./scripts/check-oracle.ps1       # numpy cross-validation of the DSP
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace           # 237 tests
cargo test -p auralis-engine ipc -- --nocapture   # IPC payload sizes
cd apps\desktop && npm run typecheck
cd apps\desktop && npm run electron:typecheck
cd apps\desktop && npm run electron:build
```

## Milestones

| Milestone | Content | Status |
| --------- | ------- | ------ |
| **M0** | Rust workspace, toolchain, dark theme, DSP kernel skeleton, CI | ✅ done |
| **M0.5** | UI stack switched to Tauri 2 + React, IPC path verified | ✅ done (container later replaced by Electron) |
| **M1** | WAV import, metadata, chunked PCM, cancellation | ✅ done |
| **M2** | FFT, amplitude correction, peak detection, waveform pyramid | ✅ done |
| **M3** | Spectrum interaction: cursor, zoom/pan, parameter inspector, peak markers | ✅ done |
| **M4** | Real-time capture (CPAL) + live spectrogram | 🚧 in progress — the CPAL stream, analyser thread, live spectrum, and the offset-aware chunked spectrogram are in; the high-refresh channel is not. **Not yet run against real audio hardware.** |
| **M4.5** | Desktop container replaced: Tauri 2 → Electron 45; Rust command layer extracted to `auralis-engine` (ADR-019) | ✅ done (2026-10-22) |
| M5 | Export, config persistence, crash recovery | ✅ done — CSV/JSON export, layout persistence, and process-level session restore (including an unclean-exit notice). The `auralis-app` use-case layer is deliberately deferred |
| M6 | Installer, user manual, license inventory | ⬜ not started |

## Project structure

```
auralis-studio/
├── crates/
│   ├── auralis-core/            # shared vocabulary: types, units, errors, time — pure
│   ├── auralis-dsp/             # RMS, Peak, DC, FFT, STFT, peak detection — pure
│   ├── auralis-codec/           # container and PCM decoding (symphonia)
│   ├── auralis-audio/           # SPSC ring, device enumeration, CPAL capture
│   ├── auralis-task/            # offline task scheduling, progress, cancellation
│   ├── auralis-storage/         # export (CSV/JSON) + atomic writes
│   ├── auralis-visualization/   # waveform pyramid, spectrogram frames
│   └── auralis-engine/          # command layer + stdio RPC endpoint (no UI, no window)
├── apps/desktop/
│   ├── src/                     # React 19 renderer — display only
│   │   ├── components/
│   │   │   ├── audio-viz/       # WaveformView · SpectrumView · SpectrogramView
│   │   │   └── ui/              # primitives.tsx · icons.tsx
│   │   ├── layouts/             # Workspace
│   │   ├── lib/                 # contracts.ts · ipc.ts · bridge.ts · persist.ts
│   │   └── styles/              # tokens.css · layout.css · globals.css
│   ├── electron/                # main process, preload bridge, engine host
│   ├── scripts/                 # dev launcher + esbuild config
│   └── dev/                     # dev-only render check (not in production build)
├── scripts/
│   ├── check-boundaries.ps1     # fails if the architecture is violated
│   ├── check-oracle.ps1         # drives the numpy cross-check
│   ├── oracle.py                # independent numpy DSP implementation
│   └── activate-dev.ps1         # local toolchain environment
├── docs/
│   ├── PRD.md  TAD.md  DSP_SPEC.md  UI_SPEC.md
│   ├── TEST_PLAN.md  IMPLEMENTATION_PLAN.md  ADR.md  BENCHMARKS.md
│   └── screenshots/
└── AGENTS.md                    # the engineering contract this project is held to
```

## Design principles

These are the rules the code is actually built around. They come from
[AGENTS.md](./AGENTS.md), and `check-boundaries.ps1` and CI enforce the ones
that can be enforced mechanically.

1. **React never computes audio.** dB conversion, FFT, and decimation all
   finish in Rust before anything crosses the IPC boundary.
2. **The DSP crates stay pure.** No UI, no device APIs, no filesystem, no
   database. They take data in and give data back.
3. **The audio callback never blocks, allocates without bound, or waits on the
   UI.** It copies samples and bumps counters. Dropped frames are instrumented,
   never silently swallowed.
4. **Every core algorithm has a deterministic numeric test** and is
   cross-validated against an independent numpy implementation.
5. **Every metric documents its units, reference level, normalization, and
   preconditions**, and names the version of the algorithm.
6. **No claim about a format, a performance figure, or a measurement accuracy
   that has not been verified** — including claims about this project's own
   architecture.

## Known limitations

Read this before trusting a number. It is short on purpose.

- **No calibrated measurement.** No SPL, no LUFS, no SNR, no THD, no THD+N.
  Those algorithms are not implemented. This release makes no accuracy claim
  of any kind.
- **WAV, FLAC, and AIFF are verified against checked-in samples; MP3 is not.**
  The fixtures are generated (hand-written writers for WAV/AIFF, `flacenc` for
  FLAC), not captured from real-world sources, so this proves the decoders
  agree with an independent encode-side implementation — not that arbitrary
  files in the wild will work. MP3 decodes but has no sample, and the UI marks
  it unverified.
- **Real-time capture has never run against a real sound card.** The CPAL
  stream, the analyser thread, the drop/overwrite counters, and the live
  spectrum in the UI are implemented and unit-tested, but the only executed
  paths are the missing-device error and pure-logic tests. Every statement
  about real capture behaviour (latency, drop rate, device hot-plug) is
  unverified.
- **Real-time analysis discards intermediate samples.** When the ring backs up,
  the worker keeps only the most recent `fft_size` samples per refresh rather
  than analysing every window. The display stays current at the cost of not
  producing a spectrum for every hop; `snapshot_overwrites` counts the frames
  the UI missed.
- **FFT is 2.7× over its original design target.** 271 µs against < 100 µs.
  See [docs/BENCHMARKS.md](./docs/BENCHMARKS.md).
- **Clipping detection returns 0 at the command layer.** A real
  flat-topped-waveform detector needs a per-sample trace, which conflicts with
  chunked, memory-bounded decoding. It is scheduled for M2's waveform
  pyramid work. Returning 0 is deliberate; guessing is not.
- **Drag-and-drop import is implemented but only typechecked.** It uses
  Electron's `webUtils.getPathForFile` in preload; the code path has not yet
  been exercised with a real OS drop on this machine.
- **IPC round-trip has been measured once, not under load.** A synthetic frame
  (4096-point FFT included) took ~11 ms end to end on the dev machine. Sustained
  realtime throughput has not been measured.
- **The Electron migration was verified on one machine only.** It removed the
  maximize-time artifacts here; it is not a guarantee for every GPU/driver
  combination. See [ADR-019](./docs/ADR.md#adr-019桌面容器从-tauri-2-迁移到-electron).
- **Session recovery is best effort.** It reopens the last successfully probed
  file and re-runs its analysis; it cannot restore an in-flight result. If the
  marker file cannot be written, the app still runs but cannot tell you the
  previous session ended uncleanly.
- **The UI is Chinese-only.** There is no i18n layer yet.
- **macOS and Linux are unadapted.**

## FAQ

<details>
<summary><strong>Why is the FFT in a worker thread instead of the audio callback?</strong></summary>

Because a 4096-point FFT takes 271 µs and the callback budget at 48 kHz is
20.8 µs. That is a 13× overrun — it is not a close call, and no amount of
careful coding closes a 13× gap. The measurement is in
[docs/BENCHMARKS.md](./docs/BENCHMARKS.md).

The callback therefore does the only two things it can afford to do: copy
samples into a buffer, and increment counters.

</details>

<details>
<summary><strong>Why does <code>SampleRing::push</code> return <code>Result&lt;(), T&gt;</code> instead of a bool?</strong></summary>

Because on overflow the caller needs the buffer back. `push` used to return
`bool` and drop the item, which means a full ring destroys the recycled buffer
and forces the next callback to allocate — exactly the failure the recycling
scheme exists to prevent.

Returning `Err(item)` hands the buffer back so it can be reused. `DropOldest`
does the same if the re-push fails.

</details>

<details>
<summary><strong>Why does the project not claim calibrated measurements?</strong></summary>

Because the algorithms are not implemented. Calibrated SPL needs a known
reference level and a defined measurement bandwidth; LUFS needs the full
ITU-R BS.1770 gating and K-weighting chain; THD needs distortion components
isolated from the fundamental. None of that exists in this codebase.

`AGENTS.md` says not to claim measurement accuracy until the algorithms and
their preconditions are implemented and validated. Claiming it anyway would
make every other number in the README less trustworthy.

</details>

<details>
<summary><strong>MP3 compiles in. Why is it not marked supported?</strong></summary>

Because symphonia's default features do not include mp3 — that was measured,
not assumed — so `mp3` is declared explicitly in the workspace manifest. That
makes it *decode*. It does not make it *verified*.

A format is marked verified only when a checked-in sample round-trips through
import → DSP with checked results. WAV, FLAC, and AIFF pass that today; MP3 has
no sample because this machine has no MP3 encoder. The UI reflects this by
showing an unverified marker rather than pretending.

</details>

<details>
<summary><strong>Why an independent numpy implementation instead of more Rust tests?</strong></summary>

Because Rust tests can only confirm that the code does what the code does. The
oracle is written from the DSP spec, not from the Rust source, so when the two
agree it is evidence that the implementation matches the specification — not
just that it is internally consistent.

The reference vectors come out of the binary itself
(`--dump-reference out.json`), which is what keeps the two implementations
honest about the same inputs.

</details>

<details>
<summary><strong>Why did the shell move from Tauri to Electron?</strong></summary>

Because the app had a deterministic, screen-wide rendering failure on the
target machine when the window was maximized, and a minimal Electron shell
loading the same page did not. The investigation, the measurements that ruled
out the driver and the refresh rate, and the exact scope of what was and was
not proven are in [ADR-019](./docs/ADR.md#adr-019桌面容器从-tauri-2-迁移到-electron).

Electron is not claimed to be artifact-free everywhere. It removes the
machine-dependent WebView2 runtime from the equation, at the cost of a larger
installer and a separate Rust engine process.

</details>

<details>
<summary><strong>The app is Windows-only. Why does CI run on Linux?</strong></summary>

The kernel is portable and the crates are cross-platform, so the matrix job
also builds on `ubuntu-latest`. It catches platform-specific breakage in the
pure crates before it reaches Windows. Since the Electron migration, the Rust
side needs only ALSA development headers (`libasound2-dev`) on Linux; the
Tauri GTK/WebKit headers are no longer required.

No Linux artifact is produced — the artifact job is Windows-only.

</details>

## Documentation

Start here for the reasoning behind the code:

| Document | Contents |
| -------- | -------- |
| [User manual (Chinese)](./docs/USER_MANUAL.md) | Install, import, views, live input, metrics, export, shortcuts, troubleshooting |
| [Product requirements (PRD)](./docs/PRD.md) | Users, scope, functional and non-functional requirements, product boundaries |
| [Technical architecture (TAD)](./docs/TAD.md) | Layering, dependency direction, threading model, error taxonomy |
| [DSP specification](./docs/DSP_SPEC.md) | Definitions and error targets for RMS / Peak / DC / FFT / STFT / clipping |
| [UI/UX specification](./docs/UI_SPEC.md) | Design tokens, layout, view components, rendering performance constraints |
| [Test and acceptance plan](./docs/TEST_PLAN.md) | Test layering, synthetic signal set, acceptance targets, CI commands |
| [Implementation plan](./docs/IMPLEMENTATION_PLAN.md) | M0–M6 milestones and vertical slices |
| [Benchmarks](./docs/BENCHMARKS.md) | Criterion results, method, and what they imply about the architecture |
| [Architecture decision records](./docs/ADR.md) | ADR-001 … ADR-020, including the ones that were reversed |
| [Third-party licenses](./THIRD_PARTY_LICENSES.md) | Generated inventory with SPDX expressions and full license texts |
| [Development environment](./docs/DEV_ENVIRONMENT.md) | Toolchain, mirrors, and path conventions |
| [Engineering contract](././AGENTS.md) | The rules this project is held to |

Security reports go through [SECURITY.md](./SECURITY.md) — not the issue
tracker.

## Star history

<a href="https://www.star-history.com/?repos=dengxingzhi000%2Fauralis-studio&amp;type=date">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/chart?repos=dengxingzhi000/auralis-studio&amp;type=date&amp;theme=dark&amp;legend=top-left" />
    <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/chart?repos=dengxingzhi000/auralis-studio&amp;type=date" />
    <img alt="Auralis Studio GitHub star history" src="https://api.star-history.com/chart?repos=dengxingzhi000/auralis-studio&amp;type=date" />
  </picture>
</a>

## Contributing

Please read [CONTRIBUTING.md](./CONTRIBUTING.md) first — it covers the build,
the branch conventions, and exactly what CI will check.

The short version: this project holds a hard line on not claiming things it
has not measured. A pull request that reports honestly what it could and could
not verify is welcome. One that quietly overstates is not.

1. Fork the repository
2. Create a branch (`git checkout -b feature/…`)
3. Run the full check suite before pushing — the commands are in
   [CONTRIBUTING.md](./CONTRIBUTING.md#the-checks-ci-runs)
4. Open a pull request and fill in the template's "How it was verified"

Participation is governed by the [Code of Conduct](./CODE_OF_CONDUCT.md).

## License

Dual-licensed under [MIT](./LICENSE-MIT) **or** [Apache-2.0](./LICENSE-APACHE),
at your option. This matches the `license` field in the workspace manifest.

---

<div align="center">

**Built for people who need the number to be right.**

</div>
