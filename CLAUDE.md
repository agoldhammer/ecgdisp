# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

- The purpose of this program is to display ecg files contained in ~/Prog/ekgdata/data/ptb/physionet.org/files/ptb-xl/1.0.3/records500/00000

- display all 10 seconds of ecg data in graphical window on Windows using egui

- the time axis should have light ticks (and light grid lines) every 40ms, with bold ticks (and bold grid lines) every 200ms and bolder ones at multiples of 1.0 sec; the amplitude grid follows ECG paper: light lines every 0.1 mV, bold every 0.5 mV

- cmd line should include an option for leads to display: I, II, III, V1 ... V6, AVL, AVF, AVR

- the cursor should display values for each trace

- write unit tests

## Tooling

- Pure **Rust** (edition 2024) egui app — egui has no usable Python plotting binding, so the
  original Python/uv scaffold was replaced; use `cargo add` where the list above says uv.
- Crates: `eframe`/`egui` (window + custom painting), `clap` (CLI), `calamine` (xlsx).
- Layout: `src/wfdb.rs` (WFDB `.hea` + format-16 `.dat` reader, verifies checksums),
  `src/leads.rs` (lead names, `-l` parsing, selection), `src/layout.rs` (GUI-free math: ticks,
  time/pixel mapping, cursor sample lookup, strip scaling — unit-test new logic here),
  `src/cli.rs` (args, record path resolution), `src/nav.rs` (record list for prev/next),
  `src/database.rs` (`ptbxl_database.xlsx` lookup by ecg_id; prints columns K/L/M per record,
  reads sex from D), `src/dsp.rs` (scipy/numpy equivalents: Butterworth SOS, `sosfiltfilt`,
  `find_peaks`, …), `src/analysis.rs` (port of `../ekgdata/src/ekgdata/analyze.py`: QRS
  detection, HR/RR, PR/QRS/QT, axis, LVH voltage — keep it matching the Python output),
  `src/titlebar.rs` (own title bar/border/resize edges, used in WSL), `src/app.rs` (egui painting
  + key handling), `src/main.rs`.
- `tests/ptbxl.rs` runs against the real dataset and skips itself when it is absent.

## Commands

```sh
cargo run -- 1 -l II,V1        # record 00001_hr, leads II and V1 (default: record 1, ALL leads, -d <project>/data500)
cargo run -- --help
cargo test
cargo clippy --all-targets
cargo add <crate>
```

Runs in WSL via WSLg (window appears on the Windows desktop). In WSL the window has no winit
frame and draws its own title bar: winit's Wayland frame leaves a ghost shadow after maximizing,
and X11 windows get no mouse pointer over their contents under WSLg. Screenshot testing without a
display: `Xvfb :99` + `env -u WAYLAND_DISPLAY DISPLAY=:99 cargo run`.

## Git

The local branch is `master`, and use master for PRs as well.