# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

`ecgdisp` is a freshly scaffolded `uv` package (`uv init --package`) with no application code yet. The only source is a placeholder `main()` in `src/ecgdisp/__init__.py`. The README is empty. There are no dependencies, tests, linter or type-checker configured yet. Update this file as those are added.

## Tooling

- Python **3.14** (pinned in `.python-version`; `requires-python = ">=3.14"`).
- Managed with **uv**; build backend is `uv_build` (src layout: the package lives in `src/ecgdisp/`).

## Commands

```sh
uv sync                 # create .venv and install the project
uv run ecgdisp          # run the console script (entry point: ecgdisp:main)
uv add <pkg>            # add a runtime dependency
uv add --dev <pkg>      # add a dev dependency (e.g. pytest, ruff)
uv build                # build sdist + wheel
```

## Git

The local branch is `master`, but `main` is the intended main branch for PRs.
