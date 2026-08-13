# GitWorkspace 产品需求与技术架构 Roadmap

> 版本：V1.0  
> 定位：高性能、多仓库、批量自动化的 Git Workspace  
> 技术基线：Tauri 2 + Vue 3 + TypeScript + Rust + SQLite  
> 当前基础：基于现有 GitWorkspace README 及前期功能规划整理  
> 目标：从“多仓库 Git GUI”演进为“面向大型工程的多仓库开发工作空间”

---

# 1. 产品定位

## 1.1 产品名称

**GitWorkspace**

## 1.2 核心定位

GitWorkspace 不是传统意义上的单仓库 Git GUI，而是：

> **高性能、多仓库、批量自动化的 Git Workspace。**

核心解决的问题：

- 一个项目由几十甚至几百个 Git 仓库组成；
- 根目录本身不要求是 Git 仓库；
- 自动发现和管理嵌套 Git 仓库；
- 一次查看整个工作区的代码变更；
- 一次对多个仓库执行 Git 操作；
- 对多个仓库执行可编排任务；
- 通过 AI 辅助 Commit、Review、Conflict Resolution；
- 在大量仓库场景下保持较低资源占用和快速响应。

当前项目已经具备工作区管理、多线程扫描、仓库分组、变更树、批量 Add/Pull/Fetch/Push/Commit、任务队列、Diff、Git Graph、AI Review 和 SQLite FTS5 代码搜索等基础能力。

---

# 2. 产品核心理念

GitWorkspace 的核心模型：

```text
                    GitWorkspace
                         │
          ┌──────────────┼──────────────┐
          │              │              │
      Workspace       Repository        AI
          │              │              │
          │       ┌──────┼──────┐        │
          │       │      │      │        │
          │     Branch  Diff   History   │
          │       │      │      │        │
          │     Stash  Graph  Reflog      │
          │       │      │      │        │
          │     Worktree Merge Rebase     │
          │                              │
          └──────────────┬───────────────┘
                         │
                  Workspace Engine
                         │
          ┌──────────────┼──────────────┐
          │              │              │
       Scanner        Task Engine    Status Engine
          │              │              │
          └──────────────┼──────────────┘
                         │
                    Performance
```

---

# 3. 核心产品目标

## G1：多仓库统一管理

用户可以把任意目录作为 Workspace：

```text
D:\Projects\MyProduct
```

即使：

```text
MyProduct
```

本身不是 Git Repository，也可以自动发现：

```text
MyProduct
├── backend
│   ├── api/.git
│   ├── core/.git
│   └── auth/.git
├── frontend/.git
├── gateway/.git
└── deployment/.git
```

---

## G2：工作区级 Git 操作

支持：

```text
Fetch All
Pull All
Push All
Commit All
Stash All
Checkout All
Create Branch All
```

同时支持单仓库操作。

---

## G3：工作区状态一览

用户打开 GitWorkspace 后，可以立即知道：

```text
Repositories        137

Clean                82
Modified             31
Untracked             8
Conflict              4
Ahead                12
Behind                7
Detached HEAD         1
```

---

## G4：极致性能

核心性能目标：

| 指标 | 目标 |
|---|---:|
| 100 个仓库首次扫描 | < 2 秒 |
| 500 个仓库首次扫描 | < 8 秒 |
| 单仓库状态刷新 | < 100 ms |
| 普通增量刷新 | < 300 ms |
| UI 操作响应 | < 50 ms |
| 1000 仓库空闲内存 | 尽量 < 500 MB |
| 后台任务 | 不阻塞 UI |
| 大型仓库 | 支持渐进式加载 |

实际指标最终应以真实测试环境 Benchmark 为准。

---

# 4. 产品信息架构

建议最终采用：

```text
GitWorkspace
│
├── Dashboard
│
├── Workspaces
│   ├── Workspace A
│   ├── Workspace B
│   └── Workspace C
│
├── Repositories
│
├── Changes
│
├── Branches
│
├── Stashes
│
├── History
│
├── Worktrees
│
├── Search
│
├── Tasks
│
├── AI
│
└── Settings
```

---

# 5. Workspace 模型

## 5.1 Workspace

Workspace 是 GitWorkspace 的一级管理对象。

属性：

```text
id
name
root_path
scan_depth
enabled
created_at
updated_at
```

---

## 5.2 Repository

Repository：

```text
id
workspace_id
name
path
remote_url
current_branch
head_commit
is_dirty
is_favorite
tags
group_id
last_scanned_at
last_status_at
```

现有 SQLite 数据模型已经包含 Workspace、Repository、Group、Task History、Code Index 等主要数据结构。

---

# 6. P0：Git 基础能力补全

P0 是必须优先完成的功能。

---

## 6.1 Branch Manager

### 功能

```text
Create Branch
Checkout Branch
Delete Branch
Rename Branch
Merge Branch
Rebase Branch
Compare Branch
Push Branch
Pull Branch
Track Remote Branch
Set Upstream
```

### Branch UI

```text
Local Branches

● feature/ai-review
● develop
● master

Remote Branches

origin/develop
origin/master
origin/release/9.6

Tags

v1.0.0
v1.1.0
```

显示：

```text
↑ 3
↓ 7
```

含义：

```text
↑ Local Ahead
↓ Remote Ahead
```

---

# 7. P0：Stash

支持：

```text
Stash Changes
Stash Including Untracked
Apply
Pop
Drop
Clear
Show Diff
Create Branch From Stash
```

## Workspace Stash

核心差异化能力：

```text
Workspace
├── repo-a
├── repo-b
├── repo-c
└── repo-d

[Stash Workspace]
```

保存：

```text
Workspace Stash #12

repo-a
repo-b
repo-c
repo-d
```

支持：

```text
Restore Workspace
```

---

# 8. P0：Commit 增强

现有 Commit 能力继续增强。

支持：

```text
Commit
Amend
Commit --no-edit
Commit Selected
Commit Hunk
Commit Line
Commit + Push
```

Commit UI：

```text
Changes

☑ src/a.ts
  ☑ Hunk 1
  ☐ Hunk 2

☑ src/b.ts

Commit Message
────────────────────
feat: xxx
────────────────────

[Commit]
[Commit & Push]
```

---

# 9. P0：Diff 增强

当前已经支持 Unified 与 Side-by-Side Diff。

升级为：

```text
File Diff
Hunk Diff
Line Diff
Commit Diff
Branch Diff
Tag Diff
Commit A ↔ Commit B
Branch A ↔ Branch B
```

增加：

```text
Stage Hunk
Unstage Hunk
Stage Line
Unstage Line
```

Diff 设置：

```text
Ignore Whitespace
Ignore EOL
Ignore Case
Show Generated Files
```

---

# 10. P0：History

Commit History：

```text
●──●──●──●
    │
    └──●──●
```

Commit 操作：

```text
Checkout
Create Branch
Create Tag
Cherry-pick
Revert
Reset
Rebase
Copy SHA
Browse Files
View Diff
AI Analyze
```

---

# 11. P0：Reflog

支持：

```text
HEAD
Branch
Remote
```

显示：

```text
HEAD@{0}
HEAD@{1}
HEAD@{2}
HEAD@{3}
```

操作：

```text
Create Branch Here
Reset Here
Restore State
View Commit
```

目标：

> 降低 Git 高风险操作的恢复成本。

---

# 12. P0：Merge / Rebase

支持：

```text
Merge
Merge --no-ff
Merge --squash

Rebase
Rebase --onto
Interactive Rebase
Continue
Abort
Skip
```

Interactive Rebase UI：

```text
pick   abc123 feat A
reword def456 feat B
squash ghi789 fix
drop   xyz999 temp
```

---

# 13. P0：Conflict Resolver

状态：

```text
CONFLICT
```

进入：

```text
Conflict Resolver
```

三方 Diff：

```text
BASE
OURS
THEIRS
RESULT
```

操作：

```text
Use Ours
Use Theirs
Use Both
Manual Edit
Mark Resolved
Abort
```

---

# 14. P1：Worktree

Repository：

```text
Main Repository

Worktrees

main
feature/login
feature/ai
hotfix/9.6
```

操作：

```text
Create Worktree
Remove Worktree
Checkout Worktree
Open Folder
Create Branch
```

---

# 15. P1：Workspace Dashboard

Dashboard 是多仓库场景的核心入口。

```text
┌───────────────────────────────────────┐
│ GitWorkspace                          │
│                                       │
│ Workspace: 9.6 Release                │
│                                       │
│ Repositories             137          │
│                                       │
│ Clean                     82          │
│ Modified                  31          │
│ Untracked                  8          │
│ Conflict                   4          │
│ Ahead                     12          │
│ Behind                     7          │
│ Detached HEAD              1          │
└───────────────────────────────────────┘
```

---

# 16. P1：Workspace Batch Operations

支持：

```text
Fetch All
Pull All
Push All
Commit All
Stash All
Checkout All
Create Branch All
Delete Branch All
```

必须支持：

```text
Select Repositories
Select Groups
Select Tags
Select Status
```

例如：

```text
@group:frontend
```

只操作前端仓库。

---

# 17. P1：Workspace Change Set

这是 GitWorkspace 的核心差异化功能。

创建：

```text
Change Set

Feature: AI Review
```

关联：

```text
repo-a → feature/ai-review
repo-b → feature/ai-review
repo-c → feature/ai-review
```

统一展示：

```text
Repositories       3
Files              73
Added             1832
Deleted            427
Commits             21
```

提供：

```text
View All Diff
AI Review
Commit All
Push All
Create PRs
```

---

# 18. P1：Workspace Pipeline

将多个 Git 操作编排成任务流。

示例：

```text
Fetch All
    ↓
Check Status
    ↓
Pull Clean Repositories
    ↓
Run Build
    ↓
Run Test
    ↓
Report
```

支持：

```text
Sequential
Parallel
Conditional
Retry
Timeout
Cancel
```

---

# 19. P1：Workspace Health

自动检测：

```text
Dirty
Conflict
Ahead
Behind
Detached
Missing Remote
Diverged
Untracked
Large Files
LFS Error
Submodule Error
```

健康评分：

```text
Workspace Health

██████████████████░░ 91%
```

---

# 20. P1：任务系统升级

现有后台任务系统已经采用 worker pool、进度事件、取消和历史记录机制。

升级为：

```text
Task
├── Pending
├── Running
├── Success
├── Failed
├── Cancelled
└── Partial Success
```

任务：

```text
Workspace
 ├── repo-a ✓
 ├── repo-b ✓
 ├── repo-c ✗
 │      └── Pull Conflict
 └── repo-d ✓
```

---

# 21. P1：任务依赖 DAG

任务不再只是队列：

```text
Fetch
 ├── repo-a
 ├── repo-b
 └── repo-c
        ↓
      Pull
        ↓
      Build
        ↓
      Test
```

任务系统支持：

```text
Dependency
Parallelism
Retry
Cancellation
Timeout
Partial Failure
```

---

# 22. P1：AI Git Assistant

现有 AI Review 使用 OpenAI-compatible API，并且 API Key 当前不落盘。

扩展为：

```text
AI Git Assistant
│
├── Code Review
├── Commit Message
├── Commit Summary
├── PR Description
├── Conflict Resolution
├── Commit Explanation
├── File Explanation
├── Security Review
└── Bug Detection
```

---

# 23. AI Commit Message

输入：

```text
Git Diff
```

输出：

```text
feat: add multi-repository task execution

- Add workspace task queue
- Support parallel repository operations
- Add progress reporting
- Improve repository synchronization
```

用户确认后：

```text
Commit
```

---

# 24. AI Conflict Resolution

输入：

```text
Base
Ours
Theirs
Project Context
```

输出：

```text
Recommended Resolution
```

必须经过：

```text
AI Suggestion
↓
Diff Preview
↓
User Confirmation
↓
Apply
```

禁止默认直接覆盖工作区。

---

# 25. P1：Workspace Code Search

当前已经具备 SQLite FTS5 全文索引能力，并且搜索不依赖外部 AI。

升级为：

```text
Workspace Search
```

支持：

```text
Text Search
File Search
Symbol Search
Repository Search
Path Search
```

搜索过滤：

```text
@repo:
@group:
@ext:
@path:
@status:
```

例如：

```text
@group:backend @ext:java UserService
```

---

# 26. P2：Symbol Index

在 FTS5 之上增加代码结构索引。

建议使用：

```text
Tree-sitter
```

建立：

```text
Symbol
Function
Class
Struct
Interface
Method
Variable
Reference
```

最终支持：

```text
Go To Definition
Find References
Symbol Search
Call Hierarchy
```

---

# 27. P2：GitHub / GitLab / Gitea 集成

第一阶段不需要完整实现远程平台。

提供：

```text
Open Repository
Open Issue
Open Pull Request
Create Pull Request
View CI
```

支持：

```text
GitHub
GitLab
Gitea
Gitee
Bitbucket
```

---

# 28. P2：Pull Request

当前 Branch：

```text
feature/ai-review
```

点击：

```text
Create Pull Request
```

自动生成：

```text
Source:
feature/ai-review

Target:
develop

Commits:
7

Files:
23
```

AI 自动生成：

```text
Title
Description
Summary
Testing
Risk
```

---

# 29. P2：Git Hooks

支持：

```text
pre-commit
prepare-commit-msg
commit-msg
post-commit
pre-push
post-checkout
post-merge
```

提供：

```text
View
Edit
Run
Enable
Disable
```

---

# 30. P2：Submodule

支持：

```text
Init
Update
Sync
Status
Add
Remove
```

展示：

```text
Submodules

common-lib
  ✓ synced

third-party
  ! modified
```

---

# 31. P2：Git LFS

支持：

```text
LFS Status
LFS Fetch
LFS Pull
LFS Push
LFS Locks
```

---

# 32. P2：Binary Diff

针对：

```text
PNG
JPG
GIF
SVG
PDF
```

提供：

```text
Before
After
```

对于文本型资源：

```text
JSON
XML
YAML
```

提供格式化 Diff。

---

# 33. Command Palette

快捷键：

```text
Ctrl + Shift + P
```

支持：

```text
> fetch
> pull
> push
> branch
> checkout
> merge
> rebase
> stash
> reset
> reflog
> worktree
> sync workspace
> AI review
```

---

# 34. 快捷键

建议：

```text
Ctrl + Shift + P   Command Palette
Ctrl + P           Repository Search
Ctrl + Shift + F   Code Search
Ctrl + Shift + D   Diff
Ctrl + Shift + G   Git Graph

Ctrl + Enter       Commit
Ctrl + Shift + Enter Commit & Push

F5                 Refresh
```

---

# 35. 性能架构

## 35.1 总体架构

```text
Vue UI
  │
  │ Tauri IPC
  ↓
Rust Application
  │
  ├── Workspace Manager
  ├── Repository Manager
  ├── Status Engine
  ├── Git Engine
  ├── Scanner
  ├── Watcher
  ├── Task Engine
  ├── Index Engine
  └── AI Engine
       │
       ├── SQLite
       ├── libgit2
       └── system git
```

当前项目已经采用 Tauri 2 + Vue 3 + Rust、git2、系统 Git CLI、SQLite、tokio/rayon/dashmap、notify 等技术。

---

# 36. Git Engine

采用：

```text
libgit2
```

处理：

```text
Status
Diff
Commit
Add
Restore
History
Branch
Tag
```

系统：

```text
git CLI
```

处理：

```text
Fetch
Pull
Push
Credential
SSH
LFS
复杂 Git 操作
```

当前项目已经采用类似混合架构：本地 Git 操作使用 libgit2，网络操作使用系统 Git CLI。

---

# 37. Status Engine

禁止频繁全量扫描。

目标：

```text
File Watcher
      ↓
Changed Path
      ↓
Affected Repository
      ↓
Incremental Status
      ↓
UI Event
```

而不是：

```text
File Changed
      ↓
Scan Entire Workspace
      ↓
Scan All Repository
```

---

# 38. Scanner

扫描器需要：

```text
Parallel
Incremental
Cancelable
Ignore Rules
Depth Limit
Symlink Protection
```

默认忽略：

```text
node_modules
target
dist
build
.next
.nuxt
venv
.git
```

当前扫描器已经支持自定义扫描深度，并通过 rayon 并行扫描仓库。

---

# 39. File Watcher

当前已经采用 notify + 500ms debounce。

建议升级：

```text
OS Native Watcher
```

优先：

```text
Windows → ReadDirectoryChangesW
Linux   → inotify
macOS   → FSEvents
```

只有必要时才使用 PollWatcher。

---

# 40. Cache Architecture

建议增加：

```text
Memory Cache
      ↓
SQLite Persistent Cache
      ↓
Git Repository
```

缓存：

```text
Repository Status
HEAD
Branch
Remote
Ahead/Behind
Commit Metadata
Graph
File Metadata
Search Index
```

---

# 41. SQLite Schema

建议最终：

```text
workspaces
repositories
repo_groups

branches
remote_branches
tags

commits
commit_parents
commit_files

worktrees
stashes

repo_status
file_status

tasks
task_items
task_dependencies

change_sets
change_set_repositories

code_index
symbols
references

ai_reviews
ai_tasks
```

---

# 42. 数据库设计原则

SQLite：

```text
WAL
```

启用：

```text
foreign_keys
busy_timeout
synchronous=NORMAL
```

大批量写入：

```text
Transaction
Batch Insert
Prepared Statement
```

禁止：

```text
每个文件单独 INSERT
```

---

# 43. IPC 设计

Tauri Command：

```text
workspace.*
repository.*
branch.*
stash.*
commit.*
diff.*
graph.*
history.*
worktree.*
task.*
search.*
ai.*
```

事件：

```text
repository_status_changed
repository_discovered
task_progress
task_completed
git_command_output
index_progress
ai_progress
```

---

# 44. 错误处理

统一：

```text
GitWorkspaceError
```

分类：

```text
RepositoryError
GitError
NetworkError
ConflictError
TaskError
IndexError
AIError
PermissionError
IOError
```

错误必须提供：

```text
code
message
repository
operation
details
recoverable
```

---

# 45. 多仓库并发策略

不能简单：

```text
1000 repo
1000 concurrent git processes
```

建议：

```text
Global Concurrency Limit
Repository Concurrency Limit
Network Concurrency Limit
CPU Concurrency Limit
```

例如：

```text
Status       16
Fetch         8
Pull          4
Push          4
Index         4
```

根据实际 Benchmark 动态调整。

---

# 46. Git Operation Safety

危险操作必须分级。

## Safe

```text
Fetch
Status
Log
Diff
Branch List
```

## Warning

```text
Pull
Push
Merge
Rebase
Stash Drop
Branch Delete
```

## Dangerous

```text
Reset --hard
Clean
Force Push
Delete Remote Branch
```

危险操作必须：

```text
二次确认
```

并明确：

```text
Repository
Branch
Files
Potential Data Loss
```

---

# 47. Force Push Safety

默认：

```text
Force Push Disabled
```

用户必须显式：

```text
Force Push
```

并提示：

```text
This may overwrite remote history.
```

进一步支持：

```text
--force-with-lease
```

作为默认推荐方案。

---

# 48. UI 设计原则

核心原则：

> 信息密度高，但不制造视觉噪音。

适合：

```text
IDE
Developer Tool
```

而不是：

```text
Consumer Application
```

---

# 49. 主界面

建议：

```text
┌───────────────────────────────────────────────────────┐
│ GitWorkspace                         Search     ⚙     │
├────────────┬──────────────────────────────────────────┤
│ Workspace  │                                          │
│            │ Dashboard                                │
│ ▼ Project  │                                          │
│   repo-a   │ Clean       82                            │
│   repo-b   │ Modified    31                            │
│   repo-c   │ Conflict     4                            │
│            │                                          │
│ Groups     │ Changes                                  │
│            │ ├── repo-a                               │
│ Favorites  │ ├── repo-b                               │
│            │ └── repo-c                               │
├────────────┴──────────────────────────────────────────┤
│ Task Queue                                             │
└───────────────────────────────────────────────────────┘
```

---

# 50. Repository Detail

```text
Repository
├── Overview
├── Changes
├── Branches
├── History
├── Stashes
├── Worktrees
├── Remotes
├── Tags
├── Hooks
├── Submodules
└── Settings
```

---

# 51. Repository Overview

显示：

```text
Current Branch
Ahead / Behind
Working Tree
Remote
Last Commit
Last Fetch
```

操作：

```text
Fetch
Pull
Push
Commit
Branch
Stash
```

---

# 52. Workspace Selection

支持：

```text
☑ Repository
☑ Group
☑ Tag
☑ Status
```

快速筛选：

```text
Dirty
Conflict
Ahead
Behind
Favorite
```

---

# 53. 配置中心

```text
General
Git
Workspace
Performance
Task
Diff
Terminal
AI
Search
Security
Advanced
```

---

# 54. Git 配置

支持读取：

```text
git config --global
git config --local
```

展示：

```text
user.name
user.email
core.autocrlf
core.editor
credential.helper
pull.rebase
```

允许编辑，但危险配置必须提示。

---

# 55. SSH

当前项目已经预留 SSH 相关 Rust 模块。

建议：

```text
SSH Key Detection
SSH Agent Detection
Known Hosts
Test Connection
```

不要自行实现复杂 SSH Credential Storage，优先复用系统 SSH Agent。

---

# 56. Terminal 集成

支持：

```text
Open Terminal Here
Open PowerShell
Open CMD
Open Git Bash
Open Windows Terminal
```

Linux：

```text
Open Terminal
```

macOS：

```text
Open Terminal
```

---

# 57. IDE 集成

支持：

```text
Open in VS Code
Open in IntelliJ IDEA
Open in Cursor
Open in Zed
```

命令：

```text
Open Repository
Open File
Open Worktree
```

---

# 58. 自动发现

Workspace 添加后：

```text
Scan
 ↓
Discover Repositories
 ↓
Detect Remote
 ↓
Detect Branch
 ↓
Detect Status
 ↓
Persist
```

支持：

```text
Refresh
Rescan
Scan Selected
```

---

# 59. Ignore Rules

除了默认目录，还允许：

```text
.gitworkspaceignore
```

例如：

```text
vendor/
third_party/
generated/
cache/
```

支持 Workspace 级：

```text
.gitworkspaceignore
```

以及 Repository 级配置。

---

# 60. 导入 / 导出 Workspace

支持：

```text
Export Workspace
```

生成：

```text
gitworkspace.json
```

例如：

```json
{
  "name": "9.6 Release",
  "root": "D:/Projects/9.6",
  "scanDepth": 5,
  "groups": [],
  "repositories": []
}
```

换电脑：

```text
Import Workspace
```

自动重新发现 Repository。

---

# 61. Workspace 模板

支持：

```text
Create Workspace Template
```

例如：

```text
Java Enterprise Project

Default Groups:
Backend
Frontend
Infrastructure
Third Party
```

---

# 62. Git Repository 模板

记录：

```text
Branch
Remote
Hooks
Tags
Group
```

---

# 63. 日志系统

开发者模式：

```text
Debug
Info
Warn
Error
Trace
```

日志：

```text
app.log
git.log
task.log
ai.log
performance.log
```

支持：

```text
Open Logs
Export Logs
Clear Logs
```

---

# 64. Benchmark 系统

GitWorkspace 必须建立独立 Benchmark。

测试：

```text
10 repositories
50 repositories
100 repositories
500 repositories
1000 repositories
```

每组测试：

```text
Initial Scan
Status Refresh
File Watch
Branch Load
Graph Load
Search
Batch Fetch
Batch Pull
Batch Push
```

记录：

```text
Time
CPU
Memory
Disk IO
Thread Count
IPC Count
Git Process Count
```

---

# 65. 性能目标

产品发布前至少保证：

```text
100 repositories
< 2 sec initial scan

500 repositories
< 8 sec initial scan

1000 repositories
UI remains responsive

File modification
< 500 ms visible update

Search
< 100 ms for indexed content
```

具体数字最终必须以真实机器 Benchmark 校准。

---

# 66. 测试体系

## Unit Test

Rust：

```text
scanner
git_status
diff
graph
task
database
```

---

## Integration Test

测试：

```text
Create Repository
Commit
Branch
Merge
Rebase
Conflict
Stash
Reset
Worktree
```

---

## Multi-Repo Test

模拟：

```text
10
50
100
500
1000
```

Repository。

---

## UI Test

测试：

```text
Navigation
Selection
Batch Operations
Diff
Graph
Task
Conflict Resolver
```

---

# 67. Crash Recovery

程序异常退出：

```text
Task Queue
Watcher
Index
Database
```

重新启动后：

```text
Recover
```

不能造成：

```text
Database corruption
Task deadlock
Repository lock
```

---

# 68. Offline First

核心 Git 功能：

```text
不依赖网络
```

包括：

```text
Status
Diff
Commit
Branch
Merge
Rebase
History
Stash
Reflog
Search
```

AI 和远程服务属于增强能力。

---

# 69. Security

## API Key

当前设计是不落盘保存 API Key。

后续建议：

```text
OS Credential Store
```

例如：

```text
Windows Credential Manager
macOS Keychain
Linux Secret Service
```

---

# 70. AI 数据安全

AI Review 必须明确：

```text
发送 Repository Diff
```

并在发送前：

```text
Preview
```

支持：

```text
Exclude File
Exclude Directory
Mask Secret
```

检测：

```text
API Key
Password
Token
Private Key
Credential
```

---

# 71. Secret Protection

AI 请求前进行：

```text
Secret Detection
```

例如：

```text
AWS Key
GitHub Token
JWT
Private Key
Password
Database URL
```

发现后：

```text
Warning
Mask
Exclude
```

---

# 72. Git Commit 安全检查

Commit 前：

```text
Secret Scan
Large File Scan
Forbidden File Scan
```

例如：

```text
.env
*.pem
*.key
credentials.json
```

---

# 73. Roadmap

## Phase 0：基础稳定

目标：

> 当前功能稳定化。

```text
[ ] Scanner
[ ] Status
[ ] Diff
[ ] Git Graph
[ ] Task Queue
[ ] AI Review
[ ] FTS5
[ ] File Watcher
[ ] SQLite
```

---

## Phase 1：完整 Git Client

目标：

> 达到成熟 Git GUI 的核心能力。

```text
[ ] Branch Manager
[ ] Stash
[ ] Hunk Stage
[ ] Cherry-pick
[ ] Revert
[ ] Reset
[ ] Reflog
[ ] Merge
[ ] Rebase
[ ] Conflict Resolver
[ ] Worktree
```

---

## Phase 2：Multi-Repo Engine

目标：

> 建立产品核心差异化。

```text
[ ] Workspace Dashboard
[ ] Workspace Health
[ ] Batch Operations
[ ] Workspace Stash
[ ] Workspace Branch
[ ] Workspace Change Set
[ ] Workspace Pipeline
[ ] Task DAG
```

---

## Phase 3：AI Git

目标：

> 从 AI Review 升级为 AI Git Assistant。

```text
[ ] AI Commit Message
[ ] AI Commit Summary
[ ] AI PR Description
[ ] AI Conflict Resolution
[ ] AI Security Review
[ ] AI Bug Detection
[ ] AI Commit Explanation
```

---

## Phase 4：Code Intelligence

```text
[ ] Tree-sitter
[ ] Symbol Index
[ ] Definition Search
[ ] Reference Search
[ ] Call Hierarchy
[ ] Semantic Search
```

---

## Phase 5：Remote Platform

```text
[ ] GitHub
[ ] GitLab
[ ] Gitea
[ ] Gitee
[ ] Bitbucket
[ ] Pull Request
[ ] Issue
[ ] CI
```

---

## Phase 6：Automation Platform

```text
[ ] Workspace Pipeline
[ ] Custom Actions
[ ] Scripts
[ ] Hooks
[ ] Task Templates
[ ] Scheduled Tasks
[ ] Plugin System
```

---

# 74. P0 / P1 / P2 总表

| 模块 | 功能 | 优先级 |
|---|---|---:|
| Git | Branch | P0 |
| Git | Stash | P0 |
| Git | Merge | P0 |
| Git | Rebase | P0 |
| Git | Reset | P0 |
| Git | Revert | P0 |
| Git | Cherry-pick | P0 |
| Git | Reflog | P0 |
| Git | Conflict Resolver | P0 |
| Diff | Hunk Stage | P0 |
| Diff | Line Stage | P0 |
| Git | Worktree | P1 |
| Workspace | Dashboard | P1 |
| Workspace | Health | P1 |
| Workspace | Batch Operation | P1 |
| Workspace | Change Set | P1 |
| Workspace | Pipeline | P1 |
| Task | DAG | P1 |
| AI | Commit Message | P1 |
| AI | Conflict Resolution | P1 |
| AI | PR Description | P1 |
| Search | Symbol Search | P2 |
| Remote | GitHub/GitLab | P2 |
| Git | Submodule | P2 |
| Git | LFS | P2 |
| Git | Hooks | P2 |
| UI | Command Palette | P2 |
| AI | Semantic Search | P2 |
| Automation | Plugin System | P3 |

---

# 75. 最终产品竞争策略

GitWorkspace 不应该和传统 Git GUI 比：

```text
谁的 Git 功能更多
```

而应该比：

```text
谁管理 100 个 Git Repository 更舒服
```

核心竞争维度：

```text
                     GitWorkspace
                           │
          ┌────────────────┼────────────────┐
          │                │                │
       Performance     Multi-Repo           AI
          │                │                │
       Rust/Tauri       Workspace          Review
       Incremental      Dashboard           Commit
       Cache            Batch               Conflict
       Parallel         Pipeline            Search
          │                │                │
          └────────────────┼────────────────┘
                           │
                  Developer Workspace
```

---

# 76. 最终目标形态

用户打开一个大型项目目录：

```text
D:\AWork\Code\9.6.0-release.2
```

GitWorkspace 自动发现：

```text
137 repositories
```

首页立即显示：

```text
Workspace Health
────────────────────────

Repositories          137
Clean                   82
Modified                31
Conflict                 4
Ahead                   12
Behind                   7

────────────────────────

[Fetch All]
[Pull Clean]
[Push]
[Commit]
[Stash]
[Create Branch]
```

用户选择：

```text
Feature: AI Review
```

GitWorkspace 创建：

```text
Workspace Change Set
```

关联：

```text
repo-a
repo-b
repo-c
repo-d
repo-e
```

然后：

```text
Edit
 ↓
Diff
 ↓
AI Review
 ↓
Commit All
 ↓
Push All
 ↓
Create PRs
```

整个过程中：

```text
Task Engine
    ↓
Parallel Execution
    ↓
Progress
    ↓
Partial Failure Recovery
    ↓
Final Report
```

最终 GitWorkspace 的产品形态不再是：

> “一个 Rust 写的 Git GUI”

而是：

> **一个面向大型多仓库工程的高性能 Git Developer Workspace。**

---

# 77. 最重要的研发原则

整个项目后续开发应始终遵循四条原则：

### 原则一：Multi-Repo First

所有核心功能都应该问：

> 能不能同时作用于多个 Repository？

---

### 原则二：Performance First

禁止：

```text
全量扫描
重复计算
无意义 IPC
无限并发
阻塞 UI
```

优先：

```text
Cache
Incremental
Parallel
Lazy Loading
Batch
```

---

### 原则三：Safety First

危险 Git 操作：

```text
Reset
Clean
Force Push
Delete
Rebase
```

必须：

```text
明确影响范围
明确数据风险
用户确认
可恢复
```

---

### 原则四：AI as Assistant

AI：

```text
建议
解释
分析
生成
```

而不是默认：

```text
自动修改
自动提交
自动 Push
```

高风险操作始终保留用户控制权。

---

# 78. MVP 后的最终优先级

如果研发资源有限，最终只需要牢牢抓住下面 **10 个核心能力**：

```text
01. Multi-Repository Workspace
02. 极致性能
03. Workspace Dashboard
04. Batch Git Operations
05. Workspace Change Set
06. Branch / Stash / Worktree
07. Conflict Resolver
08. Workspace Pipeline
09. AI Git Assistant
10. Workspace Code Intelligence
```

其中：

> **01 + 02 + 03 + 04 + 05**

是 GitWorkspace 最应该形成产品壁垒的部分；

> **06 + 07**

是成为成熟 Git Client 的基础；

> **08 + 09 + 10**

则决定它能否从 Git GUI 进一步演进成下一代 Developer Workspace。

当前项目已经具备多仓库发现、批量 Git 操作、异步任务、Diff、Graph、AI Review 和本地代码索引等关键底座，因此下一阶段不建议推倒重做，而应该围绕现有 Rust/Tauri 架构进行纵向扩展。