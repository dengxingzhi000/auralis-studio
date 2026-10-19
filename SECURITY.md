# Security Policy

## Supported versions

Auralis Studio is pre-1.0 (`0.1.0`) and Windows-first. Only the current
`master` branch receives fixes.

| Version | Supported |
|---|---|
| `master` | ✅ |
| Anything older | ❌ |

## Reporting a vulnerability

**Do not open a public issue for a security problem.**

Use GitHub's private reporting form:
**Security → Report a vulnerability** on
<https://github.com/dengxingzhi000/auralis-studio>.

That opens a private advisory visible only to the maintainer. Please include:

- what you did, step by step
- the affected file or crate, and the version or commit
- the impact you expect
- a minimal WAV or synthetic signal that reproduces it, if one does

You should get an acknowledgement within a few days. If a report is not a
vulnerability, it is a bug — it gets redirected to a normal issue once we have
confirmed that, and you are welcome to file it yourself.

## Threat model

This is a local desktop application. It imports audio files the user chose and
reads from audio input devices the user chose. It has no server component, no
account system, no network calls, and no telemetry.

That means the realistic attack surface is:

| Surface | What a malformed input can do |
|---|---|
| **Container parsing** | WAV/FLAC/MP3/AIFF headers reach `symphonia`. A malformed file must produce a typed error, never a panic or a runaway allocation. |
| **Block decoding** | Very large or very long files must stay memory-bounded. Decode is chunked and cancellable for this reason. |
| **DSP on untrusted PCM** | NaN, Inf, denormals, and full-scale values must be sanitized and counted, not propagated into the display layer. |
| **Audio input path** | The CPAL callback is real-time. A device that floods or stalls the callback must degrade by dropping frames and reporting the count — not by blocking. |

## What we consider in scope

- Parsing a file that causes a panic, an abort, or unbounded memory growth
- A crafted signal producing NaN or Inf that reaches the UI unchecked
- The audio callback blocking on a lock, a channel, a file, or the UI
- Bypassing the boundary checks that keep `auralis-core` and `auralis-dsp`
  free of I/O

## What we consider out of scope

- Vulnerabilities in a dependency with no reachable call path from this
  application
- Denial of service by opening a legitimately huge file on purpose
- Anything requiring an attacker to already control the user's machine

## Disclosure

We aim to acknowledge within a few days and to ship a fix or a mitigation
before public disclosure. We are happy to credit you in the advisory; tell us
how you would like to be named.
