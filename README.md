# GitWorkspace

> The desktop workbench for projects split across dozens of Git repositories — batch Git operations, build & run Spring Boot / Node.js services without an IDE, AI-assisted review with your own keys. Free (MIT), offline-first — built with Tauri 2 + Vue 3 + Rust; scans 100 repositories in under 2 seconds.

[English](README.md) | [简体中文](README.zh-CN.md)

![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)
![Tauri 2](https://img.shields.io/badge/Tauri-2-24c8db)
![Vue 3](https://img.shields.io/badge/Vue-3-4FC08D)
![Rust](https://img.shields.io/badge/Rust-2021-dea584)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey)

**GitWorkspace** is a cross-platform desktop application for engineers who work with **dozens or hundreds of separate Git repositories in one project**. It combines four things that normally require four different tools:

1. **Multi-repo workspace engine** — discover, group and monitor every nested repository under one root; see the whole workspace's changes in a single tree; run batch operations with task-queue orchestration and a unified undo log.
2. **Complete Git client** — branches, stash, interactive rebase (drag & drop), merge, conflict resolution, cherry-pick / revert / reset, reflog, worktrees, commit graph, hunk/line staging.
3. **Runtime workbench** — build, run, stop and monitor Spring Boot and Node.js services without opening IntelliJ or VS Code: JDK / Maven / Node toolchain management, streaming logs, health probes, port management, incremental build with auto-restart.
4. **AI assistant** — bring your own key (OpenAI Chat/Responses, Anthropic Messages, or any compatible gateway): AI code review, AI commit messages, AI conflict resolution, AI PR descriptions with security review. Every mutating action is previewed before execution.

## Why GitWorkspace?

In the AI coding era, code is generated faster than ever — the bottleneck has shifted from *writing* code to *managing changes across many repositories* and *running many local services*. Single-repo Git GUIs and IDEs see one root at a time. GitWorkspace works on the workspace as a whole:

- **Workspace-first** — a project root that is not itself a Git repo, containing dozens of nested repositories: discovered, grouped and managed automatically.
- **Deterministic core, AI on top** — precise status / diff / staging is the source of truth; AI only assists where it adds value, and always shows a preview before acting.
- **Batch everything** — select files or repositories once, act on all of them, with progress events, cancellation, DAG orchestration and an undoable operation log.
- **Offline & private** — scanning, diff, build, run and logs are fully local. AI calls are optional, use your own API key, and secrets are redacted before any request leaves the machine.

## Best for / Not for

**Best for**

- Teams working on projects split across **many separate Git repositories** (microservices, platform codebases)
- Developers, testers, integrators and ops who need to **build / run / stop Spring Boot or Node.js services locally** without launching a full IDE
- Anyone doing **batch Git operations** (fetch / pull / commit / push) across many repositories every day

**Not for**

- Deep single-repository workflows — tools like Fork, Sublime Merge or GitKraken may fit better
- Code editing — pair GitWorkspace with your IDE; it complements rather than replaces it

## Features

### 🗂️ Workspace & Multi-Repo Engine

- **Workspace management** — add directories as workspaces with configurable scan depth
- **Parallel repository discovery** — rayon-based recursive scanning; automatically skips `node_modules`, `target`, `dist`, `build`, `.next`, `venv`, etc.
- **Hierarchical repository groups** for browsing and filtering
- **Workspace dashboard & health checks** — overview and anomaly detection (detached HEAD, LFS/submodule issues, stale branches…) across the whole workspace
- **Workspace manifest** — export your repo set as a JSON manifest and batch-clone it anywhere: team onboarding in one step
- **Change sets** — named, selectable workspace-level change sets
- **Commit heatmap** — GitHub-style contribution heatmap aggregated across all workspace repositories

### 🧩 Complete Git Client

- **Change tree (home page)** — `repo → directory → file` three-level tree of all workspace changes; checkable nodes; double-click for diffs; live selection aggregation
- **Batch Git operations** — `Add` / `Revert` / `Pull` / `Fetch` / `Push` / `Commit` across many repositories at once (network operations use the system `git` CLI — compatible with Windows Credential Manager and SSH)
- **Branch manager** — create / rename / delete / checkout / compare
- **Stash** — push / pop / apply / drop, plus **workspace-wide stash**
- **Interactive rebase** — drag & drop commit ordering with pick / reword / squash / drop; continue / skip / abort wired into the conflict resolver
- **Merge & conflict resolver** — ours / theirs / manual editing with conflict-aware views
- **Cherry-pick / Revert / Reset** · **Reflog** (recover lost commits) · **Worktrees**
- **Diff viewer** — unified and side-by-side views; hunk/line staging; whitespace/case ignore options
- **Commit graph** — SVG swimlane rendering with branch/tag markers and pagination
- **Three-pane changes view** — repository tree, commit graph and diff, synchronized

### 🛠️ Runtime Workbench — run services without an IDE

- **Spring Boot** — Maven/POM discovery, main-class inference, runtime closure, build engine (`mvn` / `mvnw`, **mvnd daemon + build cache** for acceleration), launch with graceful stop and process-tree kill
- **Node.js** — toolchain detection (including nvm / fnm / volta / mise version managers), package-manager decision chain (npm / pnpm / yarn), dev-server launch with port preflight
- **Multi-service environments** — define, start and stop groups of services together
- **Log engine** — streaming, searchable per-service logs
- **Health probes** — Port / HTTP / TCP / Spring Boot Actuator, continuously monitored
- **Port manager** — see which process occupies a port, free it safely, change configured ports (dedicated tool page)
- **File watch → incremental build → auto-restart**
- **Runtime templates & launch presets** — per-service config: env vars, JDK override, JVM/Node arguments (including IDEA-style Spring Boot presets)
- **Dependency-graph visualization** across repositories, with Git awareness (status hints, operation protection around running services)

### 🤖 AI Assistant (optional, bring your own key)

- **Provider / model / credential management** — OpenAI Chat Completions, OpenAI Responses and Anthropic Messages protocols; works with OpenAI, Anthropic, DeepSeek and any compatible gateway
- **AI code review** — structured issues on workspace diffs with severity / category / file
- **AI commit messages · AI conflict resolution · AI PR descriptions** with security review / bug detection / commit explanation
- **Assistant drawer** — chat with read-only Git & runtime tools (status, diff, logs, local FTS5 code search) and **action proposals**: every mutating action is previewed before it runs
- **Private by design** — secrets are redacted before prompts leave the machine; API keys are stored in the **OS keychain** (Windows Credential Manager / macOS Keychain / Secret Service)

### 🖥️ Desktop Experience

- **Command palette** (`Ctrl/Cmd+K`) with grouped navigation and actions
- **Keyboard shortcuts** (`Ctrl+1..9` view switching and more)
- **Theme** — dark & light design tokens, follows system appearance, persisted
- **Window state memory, panel splitters with position memory, context menus**
- **About page with in-app updater** — three-platform releases (Windows / macOS / Linux) built by CI

### ⚙️ Batch & Automation

- **Background task queue** — async workers, live `task_progress` events, cancellation, persistent history with crash recovery
- **Task DAG** — dependency-aware orchestration with parallel execution and partial-failure semantics
- **Workspace pipelines** — orchestrated multi-repository workflows
- **Unified undo / operation log** — every batch action is logged and recoverable
- **File watcher** — `notify`-based with debounce → incremental status refresh (batched `repo_status_changed` events)
- **Git console** — IDE-style live output for fetch / pull / push

### ⚡ Performance & Reliability

- **Rust core** — git2 (libgit2) for local operations, system `git` for network operations, tokio + rayon + dashmap + moka caching
- **SQLite with WAL** and single-writer discipline; task history survives restarts
- **Benchmark-gated CI** (`.github/workflows/benchmark.yml`) — hard thresholds enforced on every push:
  - initial scan of 100 repositories **< 2 s**
  - per-repository status refresh **< 100 ms**
  - diff cache hit **< 50 ms**
  - commit graph first screen **< 1 s**

## How it compares

| | GitWorkspace | GitKraken / Fork / Sourcetree | IntelliJ IDEA / VS Code |
| --- | --- | --- | --- |
| Unit of work | workspace of many repos | one repo at a time (GitKraken Workspaces adds batch fetch/pull) | one project |
| Batch ops across repos | ✅ core feature | limited | ❌ |
| Build / run services, logs, ports, health | ✅ built-in | ❌ | ✅ but heavyweight |
| Works without an IDE | ✅ | ✅ | — |
| AI review / commit / conflict assist | ✅ with your own keys | partial (GitKraken AI) | via plugins |
| License & price | free, MIT | free tier / paid | community / paid |

For deep single-repo interaction (precise blame, complex history surgery), dedicated Git GUIs remain excellent — GitWorkspace is complementary there, not a replacement.

## FAQ

**How do I batch pull / fetch / push across dozens of repositories?**
Add your project root as a workspace, select the repositories (or select all), run the operation once. Everything goes through the background task queue with live progress, cancellation, and an operation log you can undo from.

**Can I run Spring Boot / Node.js services without opening IntelliJ or VS Code?**
Yes — that is the Runtime workbench. GitWorkspace detects your JDK / Maven / Node toolchains, infers how to build and launch each app, streams logs, runs health probes and manages ports. It is designed for testers, integrators and ops roles (and AI-assisted flows) that don't need a code editor.

**Does my code leave my machine?**
No. Scanning, status, diff, search, build and run are all local. AI features are strictly optional: you bring your own API key (stored in the OS keychain), and secrets are redacted before any request.

**Is it free?**
MIT-licensed, free, no account required.

## Tech Stack

| Layer | Technology |
| --- | --- |
| Desktop framework | [Tauri 2](https://tauri.app/) (plugins: shell, dialog, updater) |
| Frontend | Vue 3 + TypeScript + Vite 6 |
| UI components | Naive UI |
| State management | Pinia |
| Routing | Vue Router 4 |
| Backend | Rust (edition 2021) |
| Git (local) | [git2](https://crates.io/crates/git2) / libgit2 |
| Git (network) | system `git` CLI (credentials & SSH from system config) |
| Database | SQLite (`rusqlite`, bundled; WAL) |
| AI | reqwest (rustls-tls); OpenAI Chat/Responses + Anthropic Messages protocols; `keyring` for OS keychain storage |
| Concurrency | tokio + rayon + dashmap + moka |
| File watching | `notify` + debounce |

## Download

Prebuilt packages (Windows NSIS installer, macOS and Linux bundles) are published on the [Releases](https://github.com/mantougg/git-workspace/releases) page by CI. Windows may show a SmartScreen warning for the unsigned binary — see [docs/release.md](docs/release.md) for what that means.

## Building from Source

### Requirements

- [Node.js](https://nodejs.org/) ≥ 18 (20+ recommended) and [pnpm](https://pnpm.io/)
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

### Build & Package

```bash
# Type-check + frontend build (output to dist/)
pnpm build

# Package the desktop app (NSIS installer on Windows)
pnpm tauri build
```

### Benchmark

```bash
# Run the performance benchmark (e.g. 100 repositories)
cargo run --release --example benchmark -- 100

# Diff / commit-graph acceptance benchmark
cargo run --release --example benchmark -- diff-graph
```

## Roadmap

Development follows task-based roadmaps in [`docs/`](docs/). Core progress: **29 / 35 tasks (83%)** — Phases 0–3 complete. The desktop shell overhaul, Runtime workbench, Node.js runtime and AI assistant tracks are all shipped.

| Phase | Scope | Status |
| --- | --- | --- |
| Phase 0 | Foundation: scanner, status engine, SQLite/WAL, task queue, file watcher, benchmark, error/logging/secret protection | ✅ 8/8 |
| Phase 1 | Full Git client: branches, stash, commit/diff enhancements, cherry-pick/revert/reset, reflog, merge/rebase, conflict resolver, worktrees | ✅ 9/9 |
| Phase 2 | Multi-repo engine: dashboard, health, batch ops, workspace stash/branch, change sets, pipelines, task DAG, manifest, unified undo | ✅ 9/9 |
| Phase 3 | AI Git assistant: AI commit messages, AI conflict resolution, AI PR description + security review | ✅ 3/3 |
| Phase 4/5/6 | Code intelligence (symbol index), remote platform integration (PR/CI), submodule/LFS/hooks, plugin system, release engineering | ⬜ 0/6 |
| Side tracks | Desktop shell (D-01–17), Runtime workbench (R-01–21), Node.js runtime (N-01–10) | ✅ done |

Planned runtime extensions: Gradle support, debug collaboration (JDWP), Docker/Kubernetes runtime, JVM metrics.

## Documentation

- [Task breakdown index (`docs/tasks/README.md`)](docs/tasks/README.md) — core task specs with acceptance criteria
- [Release & code signing (`docs/release.md`)](docs/release.md)
- [Product requirements & technical architecture roadmap](docs/GitWorkspace%20产品需求与技术架构%20Roadmap.md)
- [Lightweight development workbench concept](docs/大型企业项目轻量级开发运行工作台.md)
- Other task tracks: [`docs/tasks-desktop/`](docs/tasks-desktop/README.md) · [`docs/tasks-runtime/`](docs/tasks-runtime/README.md) · [`docs/tasks-node/`](docs/tasks-node/README.md) · [`docs/tasks-ai/`](docs/tasks-ai/)

## Data Storage

- SQLite database at the system app-data directory (`gitworkspace.db`)
  - Windows: `%APPDATA%\com.gitworkspace.app`
  - macOS: `~/Library/Application Support/com.gitworkspace.app`
  - Linux: `~/.config/com.gitworkspace.app`
- Main tables: `workspaces`, `repositories` (favorites, tags, groups), `repo_groups`, `task_history`, runtime and node-project indexes, plus operation logs, change sets, pipelines and manifests

## Security

- AI is optional and off by default — the app works fully offline without any external service
- API keys are stored in the **OS keychain** (`keyring`), never in plaintext files
- Private keys and credentials are redacted from logs, UI and AI prompts (secret protection)

## License

[MIT](LICENSE)

## Credits

Built by **mantougg**.
