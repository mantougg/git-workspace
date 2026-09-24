# GF-08 Git 认证失败无可行动引导（AppError::Git 无 details / suggestedActions）

> 状态：✅ 已完成
> 优先级：P1
> 来源：2026-09-24 Git 使用体验全景盘点；与分析报告「错误可行动性」条目互证。

## 问题描述

Git 域操作（fetch/pull/push/clone）认证失败时，用户看到的是原始
`Authentication failed`（或 Git 的英文 stderr 尾部），**没有任何「重新配置凭据 /
检查 SSH / 查看帮助」的引导**。而后端错误体系本身支持结构化可行动错误——
`NodeNotFound` 就带 suggestedActions，`AppError::Git` 却被 `_ => None` 兜底挡住。

## 定位线索（证据）

> 2026-09-24 复现核验结论（逐条）：
> - `src-tauri/src/error.rs:387`（`_ => None` 兜底挡住 Git 错误 details）：**成立**。当时只有 Node/AI 有专属 details 逻辑；`AppError::Git` 落入 `_ => None`，details 恒为 null。GF-07 落地后该行漂移（序列化分支新增），兜底位置仍在 AI 分支之后。
> - 错误码映射 `src-tauri/src/error.rs:177`（`AppError::Git(_) | AppError::Ssh(_) => "GitError"`）：**漂移后仍成立**——GF-07 后位于 error.rs:210 附近，Git/Ssh 仍共用 "GitError" 码（本任务保持该码不变，分类放进 details.category，不破坏前端既有分支）。
> - 失败输出透出路径：文档原文写 `task/worker.rs:250-264`（readable_error 保留 stderr 尾部 8 行）——**漂移**：`readable_error` 实际住在 `task/console.rs:177-190`（GF-07 起 worker 队列路径与单仓命令路径经 `finish_streaming` 共用它），stderr 尾部 8 行是 `console.rs:25` 的 `STDERR_TAIL_LINES`。worker.rs:250-264 现为 `sync_pull` 命令体。**最终错误形态已核实**：`core/git_ops/remote.rs:343-349` 非零退出先返回 `git <args> exited with code N`，`readable_error` 见 "exited with code" 即用 stderr 尾部重建 `AppError::Git(git2::Error::from_str(tail))`——分类函数吃的就是这段 stderr 文本，已在单测中以典型 stderr 样本覆盖。
> - smart_pull 失败直冒 `commands/git_ops.rs:232`：**漂移**——GF-07 后 smart_pull 位于 git_ops.rs:290 起，fetch 阶段失败经 `finish_streaming`（git_ops.rs:318）冒泡，错误同样是 stderr 尾部重建的 `AppError::Git`。
> - remote.rs token 缺口：**成立**。keyring（`remote:{platform}:{host}`）、`git credential fill` 回退均在 `commands/remote.rs:98-164`；创建 PR 时 token 缺失原本发出匿名请求、拿到 401 后只显示「令牌缺失或权限不足」一句（`remote/api.rs:37-48` 的 http_error），无配置引导。

- details 序列化匹配：`src-tauri/src/error.rs:387`（`_ => None`——Git/Ssh 错误均落入此分支；仅 Node/AI 有专属 details 逻辑 `:349-359`、`:384-385`）
- 错误码映射：`src-tauri/src/error.rs:177`（`AppError::Git(_) | AppError::Ssh(_) => "GitError"`）
- 失败输出透出路径：`src-tauri/src/task/worker.rs:250-264`（`readable_error` 保留 stderr 尾部 8 行）
- smart_pull 失败直冒：`src-tauri/src/commands/git_ops.rs:232`
- 平台 token（PR/CI）有 keyring 存储（`src-tauri/src/commands/remote.rs:98-100`）与 `git credential fill` 回退（`:133-164`），但创建 PR 时 token 缺失同样无引导。

## 修复范围 checklist

- [x] 1. 错误分类：Git 错误按 authentication / network / lock / dirty-tree / rejected(non-ff) 分类（stderr 模式匹配 + libgit2 error class 兜底）。
- [x] 2. authentication 类走 details_json：携带 suggestedActions（打开系统凭据管理器 / 检查 SSH key 配置 / 确认令牌状态），details 只含固定文案，不携带任何敏感信息（token/密码/私钥内容一律不进 details，有单测断言）。
- [x] 3. 前端消费：任务失败行（TaskPanel）呈现分类 chip + 原因 + 建议操作 + 「重试」按钮；单仓操作失败 toast 经 `utils/error.ts::errMsg` 追加引导文案（覆盖 RepositoryList / BranchManager 等在改视图）；Git Console 工具条新增「认证引导 / 失败引导」气泡（打开系统凭据管理器 / 打开 SSH key 目录按钮）。
- [x] 4. 平台 token 缺失（创建 PR 时）给出「去配置 token」引导：`create_pull_request` 在 token 解析为空时直接返回 `AppError::RemoteAuth`（details 携带 suggestedActions，不再发必然 401 的匿名请求）；平台 API 401/403 同样映射为 `RemoteAuth`（token 过期场景）。

## 不做（范围控制）

- 不自建 git 凭据管理 UI（凭据走系统 GCM / ssh-agent，`core/git_ops/mod.rs:24-28` 设计注释明确 libgit2 刻意不用凭据）。
- 不改 smart_pull 的 fetch 失败冒泡语义（只改错误的可行动包装）。
- 不改 `ErrorResponse` 线格式与 `GitError` 错误码（新增 `RemoteAuthRequired` 码与 details 内容，向后兼容）。

## 验收标准

1. 用一个需要认证的远程触发 pull/push：错误提示含分类原因 + 至少一个可行动动作。（✅ 后端 details + 任务面板/ toast / Git Console 三处消费）
2. 非认证类 Git 错误（如 lock 冲突、non-ff reject）行为不劣化，仍显示原有信息。（✅ 未分类时 details 为 null、message 原样；有分类时原 message 作为前缀保留）
3. details 中无 token/密码泄漏（ Check：序列化输出 grep 无敏感串）。（✅ 单测 `git_error_details_never_leak_credentials`：消息内嵌 `user:ghp_supersecrettoken123` 时 details 无泄漏）
4. `cargo test --lib`（GW_TEST_MANIFEST=1）通过；错误分类纯函数可单测。（✅ 9 个新单测全过；与干净 HEAD 基线 worktree 对比，失败集合仅存量环境依赖失败，无新增）

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：体验盘点发现（`error.rs:387` 兜底挡住 Git 错误 details）。待复现与修复。 |
| 2026-09-24 | 复现核验 + 修复完成。根因：`AppError::Git`/`Ssh` 序列化走 `_ => None` 兜底，details 恒空；前端 `errMsg` 只读 message，认证失败只有裸 stderr。修法：`error.rs` 新增纯函数分类器 `classify_git_error`（authentication/network/lock/dirtyTree/rejected，stderr 模式匹配 + `git2::ErrorClass::Ssh/Net/Http/Ssl` 兜底）与 `git_error_details`（details 只含 category/reason/suggestedActions 固定文案）；`AppError::Git/Ssh` 与新增 `AppError::RemoteAuth{platform,host}`（码 `RemoteAuthRequired`）走该 details；`remote/api.rs::http_error` 401/403 映射 RemoteAuth，`commands/remote.rs::create_pull_request` token 缺失时直接返回 RemoteAuth（不再发匿名请求）；`commands/integration.rs` 新增 `open_git_credential_manager` 命令（Windows `control /name Microsoft.CredentialManager` / macOS `open -b com.apple.keychainaccess` / Linux seahorse，命令行构造为纯函数可单测）；前端 `utils/gitError.ts`（后端分类镜像）+ `utils/error.ts`（`errMsg` 追加引导，非 Git 域不变）+ TaskPanel 失败行引导与重试 + 终端面板 Git Console 失败引导气泡。验证：`GW_TEST_MANIFEST=1 cargo test --lib` 965 passed / 16 failed（基线 worktree 同命令 952 passed / 17 failed，失败均为存量 real_maven/node_vite/pty 等环境依赖失败，集合一致且基线多一个 flaky；新增 9 个单测全过）；`pnpm build`（vue-tsc + vite）通过。 |
