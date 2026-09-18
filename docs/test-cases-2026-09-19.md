# Test Cases — 2026-09-19

Comprehensive test case document for the GitWorkspace Tauri + Vue.js desktop application.
Covers 13 issue areas with 80+ test cases derived from source code analysis.

---

## 1. Git Pull

### TC-1.1: Smart pull — clean fast-forward
- **Precondition**: A repo with local branch behind remote by N commits, no local changes.
- **Steps**:
  1. Open the Changes view with the repo visible.
  2. Click the "Pull" button in the batch operations panel.
  3. Observe the smartPull result for the repo.
- **Expected**: `smartPull` returns `status: "ok"`, the repo is fast-forwarded, and `loadChanges()` refreshes the tree.
- **Edge case**: Repo already up-to-date — `smartPull` returns success silently with no error toast.
- **Source**: `src/views/RepositoryList.vue:2348-2385` (`handlePull` calls `smartPull` per repo)

### TC-1.2: Smart pull — conflict detection queues SmartMergeDialog
- **Precondition**: A repo with diverged branches and conflicting files.
- **Steps**:
  1. Click "Pull" with the conflicting repo in the target set.
  2. Observe that `smartPull` returns `status: "conflict"`.
  3. Verify the `SmartMergeDialog` opens with the conflict file list and `baseOid`.
- **Expected**: Conflicting repos are queued in `smartMergeQueue`; the dialog opens for the first one; non-conflicting repos report success count.
- **Edge case**: Multiple repos conflict — dialog opens sequentially for each, advancing via `onSmartMergeResolved` / `onSmartMergeAborted`.
- **Source**: `src/views/RepositoryList.vue:2358-2427` (conflict queue and `openNextConflict`)

### TC-1.3: Batch pull — all repos up-to-date
- **Precondition**: All repos in the workspace are synced with remote.
- **Steps**:
  1. Click "Pull" with no selection (falls back to all repos via `batchTargetRepos`).
- **Expected**: Each `smartPull` succeeds silently; success count toast shows; no SmartMergeDialog opens.
- **Edge case**: Zero repos in workspace — "no target repos" warning toast.
- **Source**: `src/views/RepositoryList.vue:2001-2005` (`batchTargetRepos`), `:2348-2385`

### TC-1.4: Dry-run pull preview
- **Precondition**: Repos with various upstream states (ahead, behind, diverged, up-to-date).
- **Steps**:
  1. Click "Pull 预演" button.
  2. Observe the dry-run dialog populating with `DryRunItem` rows.
  3. Verify category tags: `up_to_date` (info), `fast_forward` (success), `diverged` (warning), `conflict` (error).
- **Expected**: Table shows category, ahead/behind counts, and detail text. "Execute" button only enabled when `dryRunActionable` (fast_forward items) is non-empty.
- **Edge case**: All repos diverged or conflicting — "Execute" button remains disabled.
- **Source**: `src/views/RepositoryList.vue:2075-2140` (`runDryRun`, `executeDryRun`, `dryRunLabel`, `dryRunTagType`)

### TC-1.5: Dry-run push preview
- **Precondition**: Repos with local commits ahead of remote.
- **Steps**:
  1. Click "Push 预演".
  2. Verify the dialog title changes to "Push 预演".
  3. Click "Execute" on fast-forwardable repos.
- **Expected**: `batchPush` is called with the actionable repo paths.
- **Source**: `src/views/RepositoryList.vue:2080-2112`

### TC-1.6: Individual repo pull failure does not stop batch
- **Precondition**: Batch of 3 repos; the 2nd repo's remote is unreachable.
- **Steps**:
  1. Select all 3 repos and click Pull.
- **Expected**: Repo 1 and 3 succeed; repo 2 failure is caught silently (no abort); success count reflects only successful repos.
- **Source**: `src/views/RepositoryList.vue:2358-2368` (try/catch per repo in loop)

---

## 2. Other Git Operations

### TC-2.1: Batch commit with pre-commit safety scan
- **Precondition**: Staged files include a `.env` file (forbidden pattern).
- **Steps**:
  1. Select files including `.env`, write a commit message, click "提交".
  2. Observe the safety scan dialog (`scanDialog`) appearing with findings.
  3. Click "仍要提交" to override.
- **Expected**: `scanCommit` returns findings; dialog shows kind/path/detail; override re-submits with `allowUnsafe: true`.
- **Source**: `src/views/RepositoryList.vue:1756-1821` (`handleCommit`, `commitWithOverride`)

### TC-2.2: Commit with amend and empty message
- **Precondition**: Repo has at least one prior commit.
- **Steps**:
  1. Check "Amend 上次提交", leave message empty, click "提交".
- **Expected**: `noEdit: true` is set (amend + no message = `--no-edit`); commit succeeds.
- **Source**: `src/views/RepositoryList.vue:1757-1774`

### TC-2.3: Commit then push (thenPush checkbox)
- **Precondition**: Staged files ready.
- **Steps**:
  1. Check "提交后 Push", fill message, click "提交".
- **Expected**: `thenPush: true` is passed; commit and push tasks are both submitted.
- **Source**: `src/views/RepositoryList.vue:1773`

### TC-2.4: Batch restore (revert working-tree changes)
- **Precondition**: Modified tracked files and untracked files selected.
- **Steps**:
  1. Select files, click "回退".
  2. Confirm the warning dialog.
- **Expected**: `batchRestore` is called; tracked files revert to index; untracked files are deleted.
- **Edge case**: User cancels the confirmation dialog — no action taken.
- **Source**: `src/views/RepositoryList.vue:1714-1754` (`handleRestore`)

### TC-2.5: Batch fetch
- **Precondition**: Multiple repos with remotes configured.
- **Steps**:
  1. Click "Fetch" button.
- **Expected**: `batchFetch` submits tasks for all target repos; success toast with count; `loadChanges()` refreshes.
- **Source**: `src/views/RepositoryList.vue:2330-2346`

### TC-2.6: Push with repo picker dialog
- **Precondition**: Multiple repos; only some have ahead commits.
- **Steps**:
  1. Click "Push" to open the picker dialog.
  2. Observe the data table with checkboxes, branch tags, and ahead counts.
  3. Select repos and click "Push (N)".
- **Expected**: Only selected repos are pushed; repos with `ahead === 0` show "已同步" text.
- **Source**: `src/views/RepositoryList.vue:2429-2455`, `:704-724` (pushColumns)

### TC-2.7: Batch branch checkout with runtime guard
- **Precondition**: A running Runtime application; batch checkout requested.
- **Steps**:
  1. Click "Checkout All", enter branch name, confirm.
  2. `guardRuntimeRunning` detects running processes.
- **Expected**: A warning dialog appears asking to Stop & Switch or Cancel. If cancelled, checkout aborts.
- **Source**: `src/views/RepositoryList.vue:2041-2044`

### TC-2.8: Batch branch delete with force option
- **Precondition**: Repos with an unmerged branch.
- **Steps**:
  1. Click "Delete Branch All", enter branch name, check "强制删除未合并分支".
  2. Confirm the danger dialog.
- **Expected**: `batchBranchOp` is called with `force: true`; affected repo list is shown in the dialog.
- **Source**: `src/views/RepositoryList.vue:2024-2072`

### TC-2.9: Workspace stash — save and restore cycle
- **Precondition**: Multiple repos with uncommitted changes.
- **Steps**:
  1. Open Workspace Stash dialog.
  2. Enter a note, check "包含未跟踪文件", click "暂存选中组".
  3. Verify the summary shows stashed/skipped/failed counts.
  4. Click "恢复" on the stash record.
  5. Observe the pre-check dialog showing branch match/mismatch per repo.
  6. If any branch_mismatch, check "允许在分支不一致的仓库上恢复".
  7. Click "确认恢复".
- **Expected**: Stash saved with `id != null`; restore applies stash to working tree; `loadChanges()` refreshes.
- **Edge case**: All repos clean — `lastSave.id` is null, summary says "没有可暂存的变更".
- **Source**: `src/views/RepositoryList.vue:2142-2328`

### TC-2.10: Commit identity dialog
- **Precondition**: A repo selected in the change tree.
- **Steps**:
  1. Click "提交身份" button.
  2. Observe current identity displayed (source tag: repo/group/默认).
  3. Set scope to "本仓库", fill name and email, click "保存".
- **Expected**: `setRepoIdentity` is called; toast "已保存提交身份".
- **Edge case**: Both name and email left empty — clears custom identity (restore default).
- **Source**: `src/views/RepositoryList.vue:619-661`, `:1918-1963`

### TC-2.11: Change Set — Commit All with safety scan
- **Precondition**: A Change Set with member repos containing staged changes.
- **Steps**:
  1. Open the Change Set view, select a set.
  2. Click "Commit All", fill message, check "提交后 Push".
  3. Safety scan finds a forbidden file.
- **Expected**: Scan dialog appears; "仍要提交" overrides; commit tasks submitted for all repos with changes; skipped repos listed.
- **Source**: `src/views/ChangeSetView.vue:1346-1398`

### TC-2.12: Change Set — Push All skips repos with no ahead commits
- **Precondition**: Change Set with repos where some have `ahead === 0`.
- **Steps**:
  1. Click "Push All".
  2. Observe push candidates (ahead > 0) and skipped repos (ahead === 0).
- **Expected**: Only repos with `ahead > 0` are pushed; skipped repo names shown in "无需推送" text.
- **Source**: `src/views/ChangeSetView.vue:641-648`, `:1404-1418`

### TC-2.13: Change Set — AI Review with diff selection
- **Precondition**: Change Set with dirty repos.
- **Steps**:
  1. Click "AI Assistant", select diff scope in the picker.
  2. Confirm the preview (context preview with token estimate).
  3. Wait for AI review result.
- **Expected**: Review result dialog shows summary and issues list with severity/category/file/description.
- **Edge case**: AI not configured — redirects to AI Settings page with `AiNotConfigured` error.
- **Source**: `src/views/ChangeSetView.vue:1115-1303`

### TC-2.14: Change Set — View All Diff with lazy per-repo loading
- **Precondition**: Change Set with multiple dirty repos.
- **Steps**:
  1. Click "View All Diff".
  2. Click different repos in the left panel.
  3. Click a file to view its diff.
- **Expected**: First dirty repo auto-selected; clicking a repo loads its diff (cached after first load); file list shows status icons (A/D/M/R).
- **Source**: `src/views/ChangeSetView.vue:1064-1093` (`diffCache` Map)

---

## 3. Git Console Panel

### TC-3.1: Virtual session does not show "Terminal ready."
- **Precondition**: Terminal panel is open; Git Console tab exists (or will be lazily created).
- **Steps**:
  1. Trigger a git operation (e.g., fetch) so `git_op_output` events fire.
  2. Observe the Git Console tab content.
- **Expected**: No "Terminal ready." line appears (virtual session ID `__git_console__` starts and ends with `__`).
- **Source**: `src/components/terminal/XtermView.vue:132-136` (virtual session check)

### TC-3.2: Real PTY session shows "Terminal ready."
- **Precondition**: A new shell session opened via `openSession()`.
- **Steps**:
  1. Open a new terminal tab.
  2. Observe the xterm content.
- **Expected**: "Terminal ready." is written as the first line (real session, not virtual).
- **Source**: `src/components/terminal/XtermView.vue:132-136`

### TC-3.3: Git Console lazy creation (F-42)
- **Precondition**: Terminal panel open, no git operations performed yet.
- **Steps**:
  1. Verify no Git Console tab exists in the session list.
  2. Perform a git fetch.
  3. Verify the Git Console tab appears.
- **Expected**: `ensureGitConsoleSession()` creates the session only when `git_op_output` events arrive.
- **Source**: `src/stores/terminal.ts:268-286`

### TC-3.4: Git Console output formatting
- **Precondition**: Git operations emit `meta`, `stderr`, and normal lines.
- **Steps**:
  1. Trigger a batch commit that produces meta lines and stderr warnings.
- **Expected**: Meta lines appear in cyan (`\x1b[36m`), stderr in yellow (`\x1b[33m`); repo name prefix `[repoName]` is prepended.
- **Source**: `src/stores/terminal.ts:341-367` (`handleGitOpOutput`)

---

## 4. File Watcher → Change Tree

### TC-4.1: Watcher event triggers debounced refresh
- **Precondition**: File watcher is active; a file is modified in a tracked repo.
- **Steps**:
  1. Modify a file in one of the workspace repos.
  2. Wait for the `repo_status_changed_batch` event.
- **Expected**: After 800ms debounce, `loadChanges()` is called and the change tree refreshes.
- **Source**: `src/views/RepositoryList.vue:1478-1484` (debounced watcher handler)

### TC-4.2: Watcher cleanup on unmount
- **Precondition**: User navigates away from the Changes view.
- **Steps**:
  1. Navigate to a different view.
  2. Verify event listeners are cleaned up.
- **Expected**: `unlistenScan`, `unlistenWatcher` are called; `watcherRefreshTimer` is cleared.
- **Source**: `src/views/RepositoryList.vue:1549-1562` (`onUnmounted`)

### TC-4.3: Watcher toggle (start/stop)
- **Precondition**: Watcher is inactive.
- **Steps**:
  1. Click "启动监听" button.
  2. Verify `startWatcher` is called with all repo paths.
  3. Click "停止监听".
  4. Verify `stopWatcher` is called.
- **Expected**: Button label toggles between "启动监听" and "停止监听"; `watcherActive` ref updates.
- **Source**: `src/views/RepositoryList.vue:2465-2491`

### TC-4.4: Multiple rapid file changes coalesced
- **Precondition**: Watcher active; 10 files modified within 100ms.
- **Steps**:
  1. Rapidly modify multiple files.
- **Expected**: Only one `loadChanges()` call fires after the 800ms debounce window, not 10.
- **Source**: `src/views/RepositoryList.vue:1479-1484` (clearTimeout + setTimeout pattern)

---

## 5. Java Terminal Launch

### TC-5.1: Launch in terminal uses in-app PTY
- **Precondition**: A Java runtime configuration exists.
- **Steps**:
  1. Click the "启动" (primary) button on a runtime row in RuntimeDashboard.
  2. Observe the terminal panel opens with a new tab.
- **Expected**: `runtimeComputeLaunchPreview` returns `[command, cwd]`; `terminalStore.launchInTerminal` creates a PTY session; `runtimeRegisterTerminalProcess` registers the process.
- **Source**: `src/views/RuntimeDashboard.vue:1263-1280` (`onLaunchInTerminal`)

### TC-5.2: Degradation bar shown for terminal-launched sessions
- **Precondition**: A session created via `launchInTerminal` (has `launchedInTerminal: true`).
- **Steps**:
  1. Switch to the terminal-launched session tab.
- **Expected**: A yellow degradation bar appears: "降级模式：此会话无健康检查 / 端口检测 / 日志落盘 / AI 诊断。停止 = 关闭会话（kill 进程树）。"
- **Source**: `src/components/terminal/TerminalPanel.vue:342-348` (`terminal-degradation-bar`)

### TC-5.3: Managed launch vs terminal launch — lifecycle difference
- **Precondition**: Same runtime config available.
- **Steps**:
  1. Click "托管启动" from the dropdown.
  2. Observe the full lifecycle (preparing → resolving → building → starting → running).
  3. Click "启动" (terminal launch) for comparison.
- **Expected**: Managed launch has health checks, port detection, log persistence, and AI diagnostics. Terminal launch shows degradation bar and lacks all of these.
- **Source**: `src/views/RuntimeDashboard.vue:996-1043` (moreOptions dropdown)

---

## 6. PID/Port Propagation

### TC-6.1: PTY child PID backfilled to runtime_processes
- **Precondition**: A runtime launched via terminal.
- **Steps**:
  1. After `launchInTerminal` returns `sessionId`.
  2. Call `runtimeRegisterTerminalProcess(workspaceId, runtimeName, sessionId)`.
  3. Check the process list.
- **Expected**: The process record has the PTY child's PID populated (via `TerminalManager.get_session_pid`).
- **Source**: `src-tauri/src/process/pty.rs:566-571` (`get_session_pid`), `src/views/RuntimeDashboard.vue:1273-1275`

### TC-6.2: Unregister terminal process on exit
- **Precondition**: A terminal-launched runtime is running.
- **Steps**:
  1. Exit the shell (or kill the process).
  2. Observe the `terminal_exit` event.
- **Expected**: `handleExit` in the terminal store calls `runtimeUnregisterTerminalProcess(sessionId, exitCode)`; the session is marked `alive: false`.
- **Source**: `src/stores/terminal.ts:322-338` (`handleExit`)

### TC-6.3: Reader thread reclaims dead session from table
- **Precondition**: A PTY session where the shell exits on its own.
- **Steps**:
  1. Open a shell session.
  2. Send `exit` command.
  3. Wait for `terminal_exit` event.
  4. Call `terminal_list`.
- **Expected**: The session is no longer in the list (reader thread removes it from the session table on EOF).
- **Source**: `src-tauri/src/process/pty.rs:693-700` (reader reclaims session), `:1092-1139` (smoke test)

### TC-6.4: Close does not block list during grace period
- **Precondition**: A shell with `trap '' TERM` (ignores SIGTERM).
- **Steps**:
  1. Open a session, send `trap '' TERM; sleep 30`.
  2. Call `close()` on another thread.
  3. Call `list()` on the main thread during the 2s grace timeout.
- **Expected**: `list()` returns within 500ms and does not contain the closed session (PAF-24 fix: close removes from table before grace wait).
- **Source**: `src-tauri/src/process/pty.rs:488-501` (PAF-24: remove before grace wait), `:1141-1199` (test)

---

## 7. Terminal Copy/Paste/Link

### TC-7.1: Ctrl+C with selection copies text
- **Precondition**: Terminal has selected text.
- **Steps**:
  1. Select text in the xterm.
  2. Press Ctrl+C (no Shift).
- **Expected**: Selected text is copied to clipboard; no interrupt signal is sent.
- **Source**: `src/components/terminal/XtermView.vue:173-186`

### TC-7.2: Ctrl+C without selection sends interrupt
- **Precondition**: No text selected in xterm.
- **Steps**:
  1. Press Ctrl+C.
- **Expected**: Interrupt signal (0x03) is sent to the PTY; returns `true` from the key handler.
- **Source**: `src/components/terminal/XtermView.vue:173-186`

### TC-7.3: Ctrl+Shift+C always copies
- **Precondition**: Text selected or not.
- **Steps**:
  1. Select text, press Ctrl+Shift+C.
  2. Clear selection, press Ctrl+Shift+C.
- **Expected**: Case 1: selected text copied. Case 2: no-op (empty selection); `return false` prevents default.
- **Source**: `src/components/terminal/XtermView.vue:163-171`

### TC-7.4: Ctrl+Shift+V pastes clipboard to terminal
- **Precondition**: Clipboard contains "hello world".
- **Steps**:
  1. Press Ctrl+Shift+V in the terminal.
- **Expected**: `navigator.clipboard.readText()` returns the text; it is base64-encoded and emitted as `input` event, written to the PTY.
- **Source**: `src/components/terminal/XtermView.vue:153-161`

### TC-7.5: Right-click context menu — Copy/Paste/Clear/Close
- **Precondition**: Terminal panel open with an active session.
- **Steps**:
  1. Right-click in the terminal area.
  2. Click "复制" (Ctrl+Shift+C shortcut shown).
  3. Right-click again, click "粘贴".
  4. Right-click, click "清屏".
  5. Right-click, click "关闭 Tab".
- **Expected**: Copy uses `getTerminal().getSelection()`; paste writes to session via `writeToSession`; clear calls `xterm.clear()`; close tab calls `closeTab`.
- **Source**: `src/components/terminal/TerminalPanel.vue:112-162`, `:370-402`

### TC-7.6: WebLinksAddon opens URLs in system browser
- **Precondition**: Terminal output contains a URL (e.g., `https://example.com`).
- **Steps**:
  1. Hover over the URL in xterm.
  2. Click the link.
- **Expected**: `tauriOpen(uri)` is called, opening the URL in the system browser.
- **Source**: `src/components/terminal/XtermView.vue:126-128`

---

## 8. Source Module Launch

### TC-8.1: ClasspathRun uses target/classes
- **Precondition**: A `LaunchPlan::JavaClasspath` with `classpath` containing `target/classes` and dependency jars.
- **Steps**:
  1. Assemble the launch command via `launch_command()`.
  2. Inspect the resulting `Command`.
- **Expected**: Command is `java -cp <target/classes>:<jar1>:<jar2> <main_class> <args>`; `current_dir` is set; env vars include `MARKER_PROCESS_ID` and `MARKER_RUNTIME_NAME`.
- **Source**: `src-tauri/src/runtime/launch/launcher.rs:50-76` (`JavaClasspath` variant), `:431-451` (test)

### TC-8.2: JavaJar launch command assembly
- **Precondition**: A `LaunchPlan::JavaJar` with vm_options, jar_path, and program_arguments.
- **Steps**:
  1. Call `launch_command(&plan, 42, "app")`.
- **Expected**: Command is `java <vm_options> -jar <jar_path> <program_arguments>`; env has `DB_PASSWORD=secret`, `GITWORKSPACE_PROCESS_ID=42`, `GITWORKSPACE_RUNTIME_NAME=app`.
- **Source**: `src-tauri/src/runtime/launch/launcher.rs:38-49`, `:395-430` (test)

### TC-8.3: Script launch with platform wrapper (Windows cmd /C)
- **Precondition**: A `LaunchPlan::Script` with executable `npm.cmd` (Windows) or `/usr/bin/npm` (Unix).
- **Steps**:
  1. Call `launch_command()` on Windows.
- **Expected**: On Windows, command is `cmd /C <executable> <args>` (via `needs_cmd_c`); on Unix, command is `<executable> <args>` directly.
- **Source**: `src-tauri/src/runtime/launch/launcher.rs:78-96`, `:453-485` (test)

### TC-8.4: MavenGoal delegates to executor
- **Precondition**: A `LaunchPlan::MavenGoal` with `spring-boot:run` goal.
- **Steps**:
  1. Call `launch_command()`.
- **Expected**: Command delegates to `executor::build_process(request, env)`; current_dir is the Maven working directory.
- **Source**: `src-tauri/src/runtime/launch/launcher.rs:29-31`, `:488-507` (test)

### TC-8.5: plan_preview returns the stored preview string
- **Precondition**: Any `LaunchPlan` variant with a `preview` field.
- **Steps**:
  1. Call `plan_preview(&plan)`.
- **Expected**: Returns the exact `preview` string from the plan.
- **Source**: `src-tauri/src/runtime/launch/launcher.rs:113-120`

---

## 9. Credential File Fallback

### TC-9.1: OS Credential Store roundtrip
- **Precondition**: OS credential store available (keyring works).
- **Steps**:
  1. `mgr.set("ai-provider:p1", "sk-test", true)`.
  2. `mgr.get("ai-provider:p1")`.
  3. `mgr.delete("ai-provider:p1")`.
- **Expected**: Location is `OsStore`; get returns `"sk-test"`; after delete, get returns `None`.
- **Source**: `src-tauri/src/ai/credentials.rs:694-704` (test)

### TC-9.2: OS unavailable falls back to encrypted file store
- **Precondition**: OS credential store unavailable (e.g., no Secret Service on Linux).
- **Steps**:
  1. `mgr.set("ai-provider:p1", "sk-test", true)`.
  2. Verify location is `FileStore`.
  3. `mgr.get("ai-provider:p1")` returns the value.
  4. Verify the file exists at `<app_data_dir>/credentials/ai-provider_p1.json`.
- **Expected**: File contains JSON with `nonce` and `ciphertext` fields (base64-encoded XChaCha20-Poly1305).
- **Source**: `src-tauri/src/ai/credentials.rs:706-717` (test), `:273-404` (FileCredentialStore)

### TC-9.3: Session-only credential not persisted
- **Precondition**: `persist = false`.
- **Steps**:
  1. `mgr.set("ai-provider:p1", "sk-test", false)`.
  2. Verify `is_session_only` returns true.
  3. Restart the application.
  4. `mgr.get("ai-provider:p1")` returns `None`.
- **Expected**: Credential lives only in memory; `session_count()` is 1; process exit clears it.
- **Source**: `src-tauri/src/ai/credentials.rs:719-730` (test)

### TC-9.4: Single location invariant
- **Precondition**: Credential stored in OS store.
- **Steps**:
  1. `mgr.set("ref", "sk-os", true)` — stored in OS.
  2. `mgr.set("ref", "sk-session", false)` — switch to session.
  3. Verify OS copy is cleared; `is_session_only` is true.
  4. `mgr.set("ref", "sk-os2", true)` — switch back to OS.
  5. Verify session copy is cleared.
- **Expected**: At any time, the credential exists in exactly one location.
- **Source**: `src-tauri/src/ai/credentials.rs:732-744` (test)

### TC-9.5: Late keyring unlock recovery (PAF-21)
- **Precondition**: Keyring initially locked (unavailable), then unlocked.
- **Steps**:
  1. `mgr.set("ref", "sk", true)` — falls back to `FileStore`.
  2. Unlock the keyring.
  3. `mgr.set("ref", "sk", true)` again — `refresh_availability` re-probes.
- **Expected**: Second set succeeds with `OsStore`; file copy cleared; no restart needed.
- **Source**: `src-tauri/src/ai/credentials.rs:652-670` (test)

### TC-9.6: File store encrypt/decrypt roundtrip
- **Precondition**: `FileCredentialStore` with a temp directory.
- **Steps**:
  1. `store.set("ai-provider:p1", "sk-file-secret")`.
  2. `store.get("ai-provider:p1")`.
  3. Verify file exists as `ai-provider_p1.json` (`:` sanitized to `_`).
  4. `store.delete("ai-provider:p1")`.
- **Expected**: get returns `"sk-file-secret"`; after delete, file is removed; get returns `None`.
- **Source**: `src-tauri/src/ai/credentials.rs:778-803` (tests)

### TC-9.7: OS backend failure degrades to session with cache invalidation
- **Precondition**: OS store initially unavailable, then becomes available.
- **Steps**:
  1. `cached.is_available()` returns false (cached).
  2. Set a session-only credential.
  3. Make the backend available.
  4. `cached.is_available()` re-probes and returns true.
  5. Verify `is_session_only` still returns true (credential not in OS).
- **Expected**: Unavailable operations invalidate the cache; next probe is fresh.
- **Source**: `src-tauri/src/ai/credentials.rs:673-691` (test)

---

## 10. Workspace Cleanup

### TC-10.1: Cleaner scan is non-blocking
- **Precondition**: A workspace with large directories.
- **Steps**:
  1. Call `cleaner_scan` with a scan request.
  2. Observe that the command returns immediately with a `token`.
  3. Listen for `cleaner_scan_progress` events.
  4. Listen for `cleaner_scan_result` event.
- **Expected**: Token returned instantly; progress events fire with `visited`/`matched` counts; final result event contains `items`, `visited`, `matched`, `warnings`.
- **Source**: `src-tauri/src/commands/cleaner.rs:110-223`

### TC-10.2: Cleaner compute sizes asynchronously
- **Precondition**: A completed scan with directory items (size = null).
- **Steps**:
  1. Call `cleaner_compute_sizes(token)`.
  2. Listen for `cleaner_size_progress` events per directory.
- **Expected**: Each event has `path` and `size`; final event has `done: true` and empty path. UI stays responsive during computation.
- **Source**: `src-tauri/src/commands/cleaner.rs:227-295`

### TC-10.3: Cleaner execute — triple gate
- **Precondition**: A scan session with items.
- **Steps**:
  1. Call `cleaner_execute` with `confirmed: false`.
  2. Verify rejection.
  3. Call with `confirmed: true` but a path not in the scan session.
  4. Verify rejection.
  5. Call with `confirmed: true` and valid paths from the session.
- **Expected**: Step 2: error "请在前端完成 DELETE 确认后以 confirmed=true 调用". Step 4: error "路径不在本次扫描清单内". Step 5: deletion executes, result event fires.
- **Source**: `src-tauri/src/commands/cleaner.rs:304-374`

### TC-10.4: Cleaner session is one-shot
- **Precondition**: A scan was performed and execute was called.
- **Steps**:
  1. Call `cleaner_execute` again with the same token.
- **Expected**: Error "扫描会话不存在或已过期，请重新扫描" (session cleared after execution).
- **Source**: `src-tauri/src/commands/cleaner.rs:353-359` (session cleared after execute)

### TC-10.5: Protected paths are excluded from deletion
- **Precondition**: Scan includes the application exe directory or app data directory.
- **Steps**:
  1. Run a scan that would match the app's own directory.
  2. Attempt to delete it.
- **Expected**: `protected_paths()` canonicalizes the exe and app data dir; `cleaner::execute` skips or rejects them.
- **Source**: `src-tauri/src/commands/cleaner.rs:92-104`

### TC-10.6: Stale size computation thread exits on session change
- **Precondition**: A size computation is running for session A.
- **Steps**:
  1. Start a new scan (session B replaces session A).
  2. Observe the old computation thread.
- **Expected**: Old thread detects `session.token != token` and returns early; no stale events emitted.
- **Source**: `src-tauri/src/commands/cleaner.rs:260-266`

---

## 11. Other UI Freezes

### TC-11.1: Repo tools loadAll is synchronous but bounded
- **Precondition**: A repo with many submodules, hooks, and LFS files.
- **Steps**:
  1. Open the Repo Tools view.
  2. Observe the `n-spin` overlay during loading.
- **Expected**: `loadAll()` sequentially calls `listSubmodules`, `listHooks`, `lfsList`; each failure is caught independently (LFS failure does not block hooks).
- **Edge case**: LFS not installed — `lfsList` throws, caught silently, `lfsFiles` set to empty.
- **Source**: `src/views/RepoToolsView.vue:190-211`

### TC-11.2: Runtime dashboard does not block on reload
- **Precondition**: A workspace with many runtime configs.
- **Steps**:
  1. Click "刷新" on the Runtime Dashboard.
  2. Verify the `n-spin` overlay shows while `store.reloadAll()` runs.
- **Expected**: `reload()` calls `store.reloadAll()` then loads node projects and scheduler config; UI remains responsive (no main thread block).
- **Source**: `src/views/RuntimeDashboard.vue:1472-1491`

### TC-11.3: Terminal panel event listeners register with Promise.allSettled
- **Precondition**: Terminal panel opened for the first time.
- **Steps**:
  1. Open the terminal panel.
  2. Simulate one event channel failing to register.
- **Expected**: `Promise.allSettled` ensures other listeners still register; if all fail, `listenersReady` is reset to `null` for retry on next open.
- **Source**: `src/stores/terminal.ts:179-244` (`registerEventListeners`)

### TC-11.4: Diff load race condition protection (PAF-15)
- **Precondition**: User double-clicks file A, then quickly double-clicks file B.
- **Steps**:
  1. Double-click file A (starts async diff load).
  2. Immediately double-click file B.
- **Expected**: The `diffLoadSeq` counter increments; when file A's response arrives, `seq !== diffLoadSeq` so it is discarded; file B's diff is displayed.
- **Source**: `src/views/RepositoryList.vue:1606-1652`

---

## 12. Terminal vs Managed Launch

### TC-12.1: Managed launch has full lifecycle
- **Precondition**: A Java runtime config.
- **Steps**:
  1. Click "托管启动" from the dropdown menu.
  2. Observe the status progression: preparing → resolving → building → starting → running.
- **Expected**: `store.start(name)` submits a task; health checks run; port detection works; logs are persisted; AI diagnostics are available.
- **Source**: `src/views/RuntimeDashboard.vue:1253-1261`, `:996` (managed_start option)

### TC-12.2: Terminal launch — stop uses runtimeStopTerminalProcess
- **Precondition**: A runtime launched via terminal (has `terminalSessionId`).
- **Steps**:
  1. Click "停止".
- **Expected**: `onStop` detects `p.terminalSessionId` and calls `runtimeStopTerminalProcess(p.processId)` instead of `store.stop(name)`.
- **Source**: `src/views/RuntimeDashboard.vue:1282-1299`

### TC-12.3: Terminal launch — session exit unregisters process
- **Precondition**: A terminal-launched runtime is running.
- **Steps**:
  1. The shell process exits (crash or manual exit).
  2. `terminal_exit` event fires.
- **Expected**: `handleExit` in the terminal store detects `session.launchedInTerminal` and calls `runtimeUnregisterTerminalProcess(sessionId, exitCode)`.
- **Source**: `src/stores/terminal.ts:322-338`

### TC-12.4: Managed launch — Start/Stop/Restart button states
- **Precondition**: Runtime in various states.
- **Steps**:
  1. When stopped: "启动" enabled, "停止" disabled, "重启" disabled.
  2. When building: all three disabled (`isBusy` returns true).
  3. When running: "启动" disabled, "停止" enabled, "重启" enabled.
- **Expected**: Button disabled states correctly reflect the current process status.
- **Source**: `src/views/RuntimeDashboard.vue:683-691` (`isBusy`, `isRunning`)

---

## 13. Cross-cutting Concerns

### TC-13.1: CREATE_NO_WINDOW for Windows subprocesses
- **Precondition**: Windows platform; a streaming subprocess is spawned.
- **Steps**:
  1. Spawn a subprocess via `spawn_streaming_ext`.
  2. Verify `CREATE_NO_WINDOW` flag is set.
- **Expected**: Process spawned without a visible console window.
- **Source**: `src-tauri/src/process/pty.rs:14` (documented in module comment), AGENTS.md platform constraints

### TC-13.2: process_group(0) on unix for killpg support
- **Precondition**: Unix platform; a runtime launch command is assembled.
- **Steps**:
  1. Call `launch_command()` for any LaunchPlan variant.
  2. Inspect the `Command` for `process_group(0)`.
- **Expected**: `CommandExt::process_group(0)` is called, making the child a new process group leader; Stop's SIGTERM/SIGKILL via `killpg` covers adopted grandchildren.
- **Source**: `src-tauri/src/runtime/launch/launcher.rs:97-106`

### TC-13.3: PTY does not use process_group(0) — forkpty semantics
- **Precondition**: Unix platform; a PTY session is opened.
- **Steps**:
  1. Open a terminal session via `TerminalManager::open`.
  2. Verify no `process_group(0)` is set.
- **Expected**: PTY uses `forkpty` which creates a new session automatically; `process_group(0)` is not needed and not applied.
- **Source**: `src-tauri/src/process/pty.rs:344-346` (comment explaining forkpty semantics)

### TC-13.4: Path normalization — backslash to forward slash
- **Precondition**: Windows paths with mixed separators.
- **Steps**:
  1. Open a diff for a file with path `src\main\java\App.java`.
  2. Verify the `norm()` function normalizes to `src/main/java/App.java`.
- **Expected**: `norm(p)` replaces backslashes with forward slashes, strips leading `./` and trailing `/`.
- **Source**: `src/views/RepositoryList.vue:1654-1657` (`norm` function)

### TC-13.5: PTY master kept alive on Windows (ConPTY HPCON)
- **Precondition**: Windows platform; a PTY session is opened.
- **Steps**:
  1. Open a session via `TerminalManager::open`.
  2. Verify `PtyPair` is split: slave dropped, master kept in `PtySession`.
  3. Close the session.
- **Expected**: Master (`Arc<Mutex<Box<dyn MasterPty>>>`) stays alive with the session; dropping slave is safe (unix closes parent fd, Windows is a reference). On close, master is dropped only after process termination.
- **Source**: `src-tauri/src/process/pty.rs:363-367` (pair split), `:180-198` (PtySession holds master)

### TC-13.6: PTY write avoids holding session table lock during I/O
- **Precondition**: A PTY session exists.
- **Steps**:
  1. Call `write(sessionId, data_base64)`.
  2. Concurrently call `list()` from another thread.
- **Expected**: `write` clones the `Arc<Mutex<Writer>>` inside the table lock, releases the table lock, then does blocking I/O under only the writer lock. `list()` is not blocked by the write.
- **Source**: `src-tauri/src/process/pty.rs:420-451`

### TC-13.7: PTY reader uses byte-level read, not read_line
- **Precondition**: A PTY session producing multi-byte UTF-8 output (e.g., Chinese characters).
- **Steps**:
  1. Run a command that outputs Chinese text.
  2. Verify the output arrives correctly in xterm.
- **Expected**: Reader uses `reader.read(&mut buf)` (byte-level), not `read_line`; `flush_aggregate` base64-encodes raw bytes; xterm decodes and renders correctly. Multi-byte characters split across chunks are reassembled by xterm.
- **Source**: `src-tauri/src/process/pty.rs:611-661` (reader_thread_loop), `:703-714` (flush_aggregate)

### TC-13.8: Write buffer overflow protection (PAF-12)
- **Precondition**: A terminal session with panel hidden (paused) generating massive output.
- **Steps**:
  1. Hide the terminal panel (session paused, writeCallback absent).
  2. Let output accumulate for an extended period.
  3. Re-show the panel.
- **Expected**: `WRITE_BUFFER_MAX_CHUNKS` (5000) limits the buffer; `trimWriteBuffer` splices oldest chunks when exceeded; memory does not grow unbounded.
- **Source**: `src/stores/terminal.ts:43-49` (WRITE_BUFFER_MAX_CHUNKS, trimWriteBuffer)

### TC-13.9: Pending output drain after IPC await
- **Precondition**: PTY session being created (IPC in flight); PTY reader already sending events.
- **Steps**:
  1. Open a new session.
  2. During the `await terminalApi.terminalOpen()` call, PTY output events arrive.
  3. After the session object is created, verify buffered output is drained.
- **Expected**: `drainPendingOutput(pendingOutput, sessionId, session)` moves buffered bytes into `session.writeBuffer`; then `registerWriteCallback` flushes them to xterm.
- **Source**: `src/stores/terminal.ts:67-78` (drainPendingOutput), `:576-578`

### TC-13.10: Shell detection order (F-42)
- **Precondition**: Different platforms.
- **Steps**:
  1. On Windows: verify detection order is pwsh → powershell → cmd.
  2. On Unix: verify order is $SHELL → zsh → bash → sh.
  3. Verify `detect_available_shells()` preserves candidate order.
- **Expected**: First found shell becomes the default; order is locked by the `shell_candidates()` function and verified by `detected_shells_preserve_candidate_order` test.
- **Source**: `src-tauri/src/process/pty.rs:48-88` (shell_candidates), `:1037-1069` (order test)

### TC-13.11: Xterm theme uses CSS custom properties
- **Precondition**: Application with dark/light theme.
- **Steps**:
  1. Open a terminal.
  2. Verify xterm background matches `--gw-bg-app`, foreground matches `--gw-text`, cursor matches `--gw-accent`.
  3. Switch themes.
- **Expected**: `getXtermTheme()` reads from `getComputedStyle(document.documentElement)` at mount time; theme changes require re-mount to reflect.
- **Source**: `src/components/terminal/XtermView.vue:54-88`

### TC-13.12: ResizeObserver triggers fit on container size change
- **Precondition**: Terminal panel is resized (drag or maximize).
- **Steps**:
  1. Drag the terminal panel resize handle.
  2. Observe xterm refitting.
- **Expected**: `ResizeObserver` fires, `fitAddon.fit()` is called, `emit("resize", cols, rows)` notifies the store, which calls `terminalResize` on the backend.
- **Source**: `src/components/terminal/XtermView.vue:189-203`

---

*Total test cases: 79*
