# AGENTS.md — Auralis Studio Engineering Contract

## Mission

Build Auralis Studio, a professional Windows-first desktop audio analysis application in Rust. Prioritize visual quality, numerical correctness, real-time stability, maintainability, and reproducible builds.

## Before coding

1. Inspect the repository and existing files before changing anything.
2. Read `README.md` and the relevant documents under `docs/`.
3. State the smallest useful implementation plan and identify affected crates.
4. Implement one vertical slice at a time.
5. Run formatting, Clippy, tests, and build checks appropriate to the change.
6. Report actual command results; never claim checks passed if they were not run.

## Architecture rules

- Use a Rust workspace with multiple crates and a desktop application under `apps/auralis-desktop`.
- Keep `auralis-core` and `auralis-dsp` free of UI, device APIs, filesystem access, and database dependencies.
- Keep UI code out of the real-time audio callback.
- Separate offline analysis scheduling from real-time streaming.
- Use bounded queues/buffers for real-time data flow.
- Prefer explicit typed configuration and typed errors over stringly typed maps.
- Avoid premature dynamic plugins, microservice-like architecture, and unnecessary abstraction.
- Keep dependency versions compatible and commit `Cargo.lock` for the application.

## Real-time safety

The CPAL callback must not:
- block on locks, channels, files, database, network, or UI;
- perform unbounded allocation;
- synchronously log large payloads;
- wait for analysis completion.

Use a bounded, preallocated handoff buffer. Define and test overflow/drop behavior. Keep callback work minimal and instrument dropped frames.

## DSP correctness

- Document all units, reference levels, scaling, window correction, and assumptions.
- Distinguish amplitude spectrum, power spectrum, and power spectral density.
- Handle silence, DC, NaN/Inf, partial frames, and Nyquist/DC bins explicitly.
- Use deterministic synthetic signals and checked-in fixtures.
- Do not claim calibrated SPL, LUFS compliance, SNR, THD, or THD+N until the algorithms and preconditions are implemented and validated.

## UI rules

- Dark professional measurement-instrument design, not a generic admin dashboard.
- Use design tokens; maintain legible labels and compact but understandable information density.
- Use custom texture-based rendering for dense spectrograms.
- Avoid sending every PCM sample to the UI. Publish decimated view data or immutable snapshots.
- UI redraw cadence must be decoupled from the audio callback cadence.

## Definition of done

A task is done only when:
- code compiles for its intended target;
- relevant tests pass;
- error states and invalid input are considered;
- documentation is updated where interfaces or behavior change;
- known limitations are reported honestly.

## First task

Do not implement all features at once. First propose the workspace structure, resolve dependency compatibility, create a compilable skeleton, add CI, and implement one vertical slice: import a supported WAV file, inspect metadata, compute deterministic RMS/Peak, and render a basic waveform and spectrum.
