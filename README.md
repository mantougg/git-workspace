# GitWorkspace

> High-performance, multi-repository Git workspace — built for the AI coding era.

[English](README.md) | [简体中文](README.zh-CN.md)

![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)
![Tauri 2](https://img.shields.io/badge/Tauri-2-24c8db)
![Vue 3](https://img.shields.io/badge/Vue-3-4FC08D)
![Rust](https://img.shields.io/badge/Rust-2021-dea584)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey)

**GitWorkspace** is a cross-platform desktop application built with **Tauri 2 + Vue 3 + Rust** that unifies the management, scanning, and operation of **dozens or hundreds of Git repositories in a single interface** — from a workspace-wide change tree with batch operations, to a complete Git client (branches, stash, merge/rebase, conflict resolution, reflog, worktrees), to workspace-level automation (pipelines, task DAGs, manifests, unified undo), plus AI-assisted code review and fully offline local code search.

## Why GitWorkspace?

In the AI coding era, code is generated faster than ever — the bottleneck has shifted from *writing* code to *managing changes across many repositories*. Single-repo GUIs and AI coding tools operate one root at a time and cannot see the whole workspace. GitWorkspace fills that gap:

- **Workspace-first**: a project root that is not itself a repo, containing dozens of nested repositories — discovered, grouped, and managed automatically.
- **Deterministic + AI**: precise status / diff / staging is the source of truth; AI assists only where it adds value (code review today, commit messages / conflict resolution / PR descriptions on the roadmap).
- **Batch everything**: select files or repositories once, act on all of them — with progress events, cancellation, DAG orchestration, and a unified undo log for safety.
- **Offline & private**: local SQLite FTS5 code search and review flows never require your code to leave the machine.

## Screenshots

<!-- Drop PNG screenshots into screenshots/ and reference them here, e.g.
![Dashboard](screenshots/dashboard.png)
![Change Tree](screenshots/change-tree.png)
![Commit Graph](screenshots/git-graph.png)
-->

## Features

### 🗂️ Workspace & Multi-Repo

- **Workspace management** — add directories as workspaces with configurable scan depth
- **Parallel repository discovery** — recursive scanning with rayon; automatically skips `node_modules`, `target`, `dist`, `build`, `.next`, `.nuxt`, `venv`, etc.
- **Hierarchical repository groups** (`repo_groups`) for browsing and filtering
- **Workspace Dashboard & Health** (T-18 / T-19) — overview and health checks across the whole workspace
- **Workspace Manifest** (T-33) — manifest-driven bootstrap and batch clone of a repo set
- **Change Sets** (T-22) — named, selectable workspace-level change sets

### 🧩 Complete Git Client

- **Change tree (home page)** — `repo → directory → file` three-level tree of all changes; checkable nodes; double-click to expand directories or open file diffs; live selection aggregation
- **Batch Git operations** — `Add` (recursive; deleted files auto-removed from the index), `Revert` (restore tracked files from HEAD, remove untracked), `Pull` / `Fetch` / `Push` (via system `git` CLI — compatible with Windows Git Credential Manager and SSH), `Commit` per repository with file selection, `amend`, and commit+push
- **Branch manager** (T-09) — create / rename / delete / checkout / compare
- **Stash** (T-10) — push / pop / apply / drop, plus **workspace-wide stash** (T-21)
- **Merge / Rebase** (T-15) with dedicated dialogs
- **Conflict Resolver** (T-16) — ours / theirs / manual editing with conflict-aware views
- **Cherry-pick / Revert / Reset** (T-13)
- **Reflog** (T-14) — view history and recover lost commits
- **Worktrees** (T-17)
- **Diff viewer** — Unified and Side-by-Side views; hunk/line staging (T-12); ignore options (whitespace, case)
- **Commit graph** — SVG swimlane rendering of branches and merges (merge commits marked with purple dots), branch/tag markers, pagination

### ⚙️ Batch & Automation

- **Background task queue** — 8 async workers, live `task_progress` events, cancellation, persistent history with crash recovery
- **Task DAGs** (T-24) — dependency-aware orchestration with parallel execution and partial-failure semantics
- **Workspace Pipelines** (T-23) — orchestrated multi-repository workflows
- **Unified Undo / operation log** (T-34) — every batch action is logged and recoverable
- **File watcher** (T-06) — poll-based `notify` watcher with 500 ms debounce → incremental status refresh and `repo_status_changed` events
- **Git console** — IDE-style live output for fetch / pull / push (`git_command_result` events)

### 🤖 AI & Code Intelligence

- **AI code review** — sends workspace diffs to any OpenAI-compatible API (OpenAI, DeepSeek, etc.); returns a JSON summary plus an issue list with severity / category / file / description; diffs over 10k characters are auto-truncated; API keys are passed per request and **never persisted**
- **Local code search** — SQLite FTS5 full-text index across repositories, relevance-ranked, per-repository rebuild / clear — fully offline, no AI service required
- **Secret protection** (T-08) — private keys and credentials redacted from logs and UI
- **On the roadmap**: AI commit messages (T-25), AI conflict resolution (T-26), AI PR descriptions + security review (T-27)

### 🚀 Performance & Reliability

- **Rust core** — git2 (libgit2) for local operations, system `git` for network operations, tokio + rayon + dashmap + moka caching
- **SQLite with WAL** and single-writer discipline; task history survives restarts
- **Benchmark-gated CI** (`.github/workflows/benchmark.yml`) — hard thresholds enforced on every push:
  - initial scan of 100 repositories **< 2 s**
  - per-repository status refresh **< 100 ms**
  - diff cache hit **< 50 ms**
  - commit graph first screen **< 1 s**

## Tech Stack

| Layer | Technology |
| --- | --- |
| Desktop framework | [Tauri 2](https://tauri.app/) (plugins: shell, dialog) |
| Frontend | Vue 3 + TypeScript + Vite 6 |
| UI components | Element Plus + `@element-plus/icons-vue` |
| State management | Pinia |
| Routing | Vue Router 4 |
| Backend | Rust (edition 2021) |
| Git (local) | [git2](https://crates.io/crates/git2) / libgit2 (status, diff, commit, add, restore, branches, stash, merge, rebase, worktrees) |
| Git (network) | system `git` CLI (fetch / pull / push — credentials & SSH from system config) |
| Database | SQLite (`rusqlite` 0.32, bundled; WAL + FTS5 full-text index) |
| Concurrency | tokio + rayon + dashmap + moka |
| File watching | `notify` (PollWatcher) + custom 500 ms debounce |
| HTTP | reqwest (rustls-tls, AI review requests) |

## Project Structure

```
git-workspace/
├── index.html                  # Frontend entry HTML
├── package.json                # Frontend deps & scripts (pnpm)
├── vite.config.ts              # Vite config (port 1420)
├── src/                        # Frontend source (Vue 3 + TS)
│   ├── main.ts / App.vue       # Entry / root component (incl. task panel)
│   ├── api/                    # Tauri command wrappers (ai, batch, branch, changes,
│   │                           #   changeSet, commit, conflict, diff, git, git_ops,
│   │                           #   graph, group, health, history, logs, manifest,
│   │                           #   merge, operationLog, pipeline, rebase, reflog,
│   │                           #   repository, stash, task, workspace, workspaceStash,
│   │                           #   worktree)
│   ├── components/             # common / diff / graph / repo / branch components
│   ├── composables/            # useRepositories / useTaskProgress
│   ├── router/                 # / (change tree), /diff, /graph, /branches, /stash, ...
│   ├── stores/                 # Pinia stores (repository / task / workspace / changeSet)
│   ├── types/                  # TypeScript types
│   ├── utils/                  # format / error / frameTime helpers
│   └── views/                  # RepositoryList, DiffViewer, GitGraph, BranchManager,
│                               #   ConflictResolver, StashManager, WorktreeManager,
│                               #   Reflog, Dashboard, Health, ChangeSet, Pipeline,
│                               #   Manifest, OperationLog, TaskPanel
└── src-tauri/                  # Rust backend (Tauri 2)
    ├── Cargo.toml              # Rust dependencies
    ├── tauri.conf.json         # Tauri config (window / bundling)
    ├── capabilities/           # Plugin permission declarations
    └── src/
        ├── main.rs / lib.rs    # Entry; registers all Tauri commands
        ├── commands/           # Command layer (workspace, repository, git_ops, diff,
        │                       #   graph, branch, stash, merge_rebase, conflict, reflog,
        │                       #   worktree, change_set, workspace_stash, health, history,
        │                       #   manifest, pipeline, operation_log, batch, ai, task, ...)
        ├── core/               # Business logic: scanner, git_ops, git_status, diff,
        │                       #   graph, branch, stash, merge, rebase, conflict, reflog,
        │                       #   worktree, change_set, workspace_stash, health, history,
        │                       #   manifest, pipeline, operation_log, selector, stage,
        │                       #   secret, ssh, watcher, logger
        ├── db/                 # SQLite (schema.rs / dao.rs)
        ├── models/             # Data models (workspace / repository / group / task / ...)
        ├── task/               # Task engine (manager / queue / worker / dag)
        ├── benchmark/          # Benchmark harness (`cargo run --release --example benchmark`)
        ├── state.rs            # Application global state
        └── error.rs            # Unified error type
```

## Getting Started

### Requirements

- [Node.js](https://nodejs.org/) ≥ 18 (20+ recommended)
- [pnpm](https://pnpm.io/)
- [Rust](https://www.rust-lang.org/) stable toolchain (with `cargo`)
- [Git](https://git-scm.com/) CLI — network operations (fetch / pull / push) use the system git to pick up credential managers and SSH configs
- Tauri 2 system dependencies: WebView2 on Windows (bundled with Win10/11), Xcode CLT on macOS, `webkit2gtk-4.1` etc. on Linux

### Development

```bash
# 1. Install frontend dependencies
pnpm install

# 2. Run in dev mode (starts Vite and compiles Rust, opens the app window)
pnpm tauri dev

# Frontend only (debug UI in browser at http://localhost:1420)
pnpm dev
```

> The first `pnpm tauri dev` compiles all Rust dependencies and takes a while; later runs are incremental.

### Build

```bash
# Type-check + frontend build (output to dist/)
pnpm build

# Package the desktop app (NSIS installer on Windows)
pnpm tauri build
```

### Release & Signing

The project builds a Windows NSIS installer (`.exe`) and publishes it to GitHub Release automatically via GitHub Actions; the workflow is defined in [`.github/workflows/release.yml`](.github/workflows/release.yml).

**Triggers:**

| Trigger | Action | Artifact |
| --- | --- | --- |
| Push version tag | `git tag v0.1.0 && git push origin v0.1.0` | Published Release, version from tag |
| Manual | Repo → Actions → Release → Run workflow | Draft Release, version `0.0.0-dev.<run#>` |

The workflow syncs the tag version into `tauri.conf.json` before building, so artifacts never share a duplicate version number.

**Tauri Updater signing keys (optional, for auto-update verification)**

The workflow reads two secrets: `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. They produce a `.sig` signature for the installer, which Tauri's built-in updater uses to verify installer integrity during auto-updates — **this is the Tauri updater signature, not Windows code signing**.

Generate a keypair once locally (keep the private key safe; if lost, already-installed old versions cannot receive new updates):

```bash
pnpm tauri signer generate -w ~/.tauri/gitworkspace.key
```

This prints a public key (`dW50cnVzdGVk...`) and writes the private key file. Then configure two secrets under the repo's Settings → Secrets and variables → Actions:

| Secret | Value |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | Private key file content (or path) |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password set when generating the key (leave empty if none) |

> Once configured, the build also emits a `*.exe.sig` signature file under `target/release/bundle/nsis/`. To enable auto-updates, also fill the public key into `tauri.conf.json`'s `plugins.updater.pubkey`.

**On the Windows SmartScreen "unknown publisher" warning (important clarification)**

The Tauri updater signature above does **not** clear the SmartScreen warning. SmartScreen recognizes **Windows Authenticode code-signing certificates**, which must be purchased (OV/EV) from a trusted CA (e.g. DigiCert, Sectigo) — a separate mechanism:

- Configure `certificateThumbprint`, `digestAlgorithm` (e.g. `sha256`) and `timestampUrl` (timestamp server) under `bundle.windows` in `tauri.conf.json`;
- In CI, store the `.pfx` certificate base64-encoded as a GitHub Secret (e.g. `WINDOWS_CERTIFICATE` / `WINDOWS_CERTIFICATE_PASSWORD`), decode and import it before build, then `tauri build` invokes `signtool` automatically.

To clear the SmartScreen warning, obtain an Authenticode certificate and follow the [Tauri Windows code signing guide](https://v2.tauri.app/distribute/sign/windows/).

### Benchmark

```bash
# Run the performance benchmark (e.g. 100 repositories)
cargo run --release --example benchmark -- 100

# Diff / commit-graph acceptance benchmark (T-04)
cargo run --release --example benchmark -- diff-graph
```

## Roadmap

Development follows a task-based roadmap in [`docs/`](docs/). Overall progress: **26 / 35 tasks (74%)** — Phase 0 (foundations), Phase 1 (complete Git client) and Phase 2 (multi-repo engine) are done.

| Phase | Scope | Status |
| --- | --- | --- |
| Phase 0 | Foundation hardening: scanner, status engine, SQLite/WAL, task queue, watcher, benchmark, error/logging/secret protection | ✅ 8/8 |
| Phase 1 | Full Git client: branches, stash, commit/diff enhancements, cherry-pick/revert/reset, reflog, merge/rebase, conflict resolver, worktrees | ✅ 9/9 |
| Phase 2 | Multi-repo engine: dashboard, health, batch ops, workspace stash/branch, change sets, pipelines, task DAG, manifest, unified undo | ✅ 9/9 |
| Phase 3 | AI Git Assistant: AI commit messages, AI conflict resolution, AI PR description + security review | ⬜ 0/3 |
| Phase 4/5/6 | Code intelligence (symbol index), remote platform integration, submodules/LFS/hooks, command palette, plugin system, release engineering | ⬜ 0/6 |

## Documentation

- [Task breakdown index (`docs/tasks/README.md`)](docs/tasks/README.md) — 35 task specs with acceptance criteria
- [Product requirements & technical architecture roadmap](docs/GitWorkspace%20产品需求与技术架构%20Roadmap.md)
- [Lightweight development workbench concept](docs/大型企业项目轻量级开发运行工作台.md)

## Data Storage

- SQLite database at the system app-data directory (`gitworkspace.db`)
  - Windows: `%APPDATA%\com.gitworkspace.app`
  - macOS: `~/Library/Application Support/com.gitworkspace.app`
  - Linux: `~/.config/com.gitworkspace.app`
- Main tables: `workspaces`, `repositories` (favorites, tags, groups), `repo_groups`, `task_history`, `code_index` (FTS5), plus operation logs, change sets, pipelines and manifests

## Security

- AI API keys are passed per request from the frontend and **never persisted** to disk
- Private keys and credentials are redacted from logs and UI (secret protection)
- AI calls are fully optional — the app works offline without any external service

## License

[MIT](LICENSE)

## Credits

Built by **mantougg**.
