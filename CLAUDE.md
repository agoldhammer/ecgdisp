# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

- The purpose of this program is to display ecg files contained in ~/Prog/ekgdata/data/ptb/physionet.org/files/ptb-xl/1.0.3/records500/00000

- display all 10 seconds of ecg data in graphical window on Windows using egui

- the time axis should have light ticks (and light grid lines) every 25ms, with bolder ticks (and bolder grid lines) at multiples of 0.5 and 1.0 sec

- cmd line should include an option for leads to display: I, II, III, V1 ... V6, AVL, AVF, AVR

- the cursor should display values for each trace

- use uv to add necessary packages

- write unit tests

## Tooling

- Pure **Rust** (edition 2024) egui app — egui has no usable Python plotting binding, so the
  original Python/uv scaffold was replaced; use `cargo add` where the list above says uv.
- Crates: `eframe`/`egui` (window + custom painting), `clap` (CLI).
- Layout: `src/wfdb.rs` (WFDB `.hea` + format-16 `.dat` reader, verifies checksums),
  `src/leads.rs` (lead names, `-l` parsing, selection), `src/layout.rs` (GUI-free math: ticks,
  time/pixel mapping, cursor sample lookup, strip scaling — unit-test new logic here),
  `src/cli.rs` (args, record path resolution), `src/app.rs` (egui painting), `src/main.rs`.
- `tests/ptbxl.rs` runs against the real dataset and skips itself when it is absent.

## Commands

```sh
cargo run -- 1 -l II,V1        # record 00001_hr, leads II and V1 (default: record 1, ALL leads)
cargo run -- --help
cargo test
cargo clippy --all-targets
cargo add <crate>
```

Runs in WSL via WSLg (window appears on the Windows desktop). Screenshot testing without a
display: `Xvfb :99` + `env -u WAYLAND_DISPLAY DISPLAY=:99 cargo run`.

## Git

The local branch is `master`, and use master for PRs as well.