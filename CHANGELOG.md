# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Because Auralis Studio is pre-1.0, treat `0.x` minor bumps as breaking.

## [Unreleased]

Nothing yet.

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
