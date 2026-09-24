# GF-08 Git 认证失败无可行动引导（AppError::Git 无 details / suggestedActions）

> 状态：⬜ 未开始
> 优先级：P1
> 来源：2026-09-24 Git 使用体验全景盘点；与分析报告「错误可行动性」条目互证。

## 问题描述

Git 域操作（fetch/pull/push/clone）认证失败时，用户看到的是原始
`Authentication failed`（或 Git 的英文 stderr 尾部），**没有任何「重新配置凭据 /
检查 SSH / 查看帮助」的引导**。而后端错误体系本身支持结构化可行动错误——
`NodeNotFound` 就带 suggestedActions，`AppError::Git` 却被 `_ => None` 兜底挡住。

## 定位线索（证据）

- details 序列化匹配：`src-tauri/src/error.rs:387`（`_ => None`——Git/Ssh 错误均落入此分支；仅 Node/AI 有专属 details 逻辑 `:349-359`、`:384-385`）
- 错误码映射：`src-tauri/src/error.rs:177`（`AppError::Git(_) | AppError::Ssh(_) => "GitError"`）
- 失败输出透出路径：`src-tauri/src/task/worker.rs:250-264`（`readable_error` 保留 stderr 尾部 8 行）
- smart_pull 失败直冒：`src-tauri/src/commands/git_ops.rs:232`
- 平台 token（PR/CI）有 keyring 存储（`src-tauri/src/commands/remote.rs:98-100`）与 `git credential fill` 回退（`:133-164`），但创建 PR 时 token 缺失同样无引导。

## 修复范围 checklist

- [ ] 1. 错误分类：Git 错误按 authentication / network / lock / dirty-tree / rejected(non-ff) 分类（可从 stderr 模式匹配 + libgit2 error class 提取）。
- [ ] 2. authentication 类走 details_json：携带 suggestedActions（如「打开系统凭据管理器」「检查 SSH key 配置」「查看文档」），不携带任何敏感信息。
- [ ] 3. 前端消费：任务失败 / 单仓操作失败的展示处呈现动作入口（Git Console 失败行旁或错误 toast 带动作按钮）。
- [ ] 4. 平台 token 缺失（创建 PR 时）给出「去配置 token」引导（remote.rs 已有 save_remote_token 入口）。

## 不做（范围控制）

- 不自建 git 凭据管理 UI（凭据走系统 GCM / ssh-agent，`core/git_ops/mod.rs:24-28` 设计注释明确 libgit2 刻意不用凭据）。
- 不改 smart_pull 的 fetch 失败冒泡语义（只改错误的可行动包装）。

## 验收标准

1. 用一个需要认证的远程触发 pull/push：错误提示含分类原因 + 至少一个可行动动作。
2. 非认证类 Git 错误（如 lock 冲突、non-ff reject）行为不劣化，仍显示原有信息。
3. details 中无 token/密码泄漏（ Check：序列化输出 grep 无敏感串）。
4. `cargo test --lib`（GW_TEST_MANIFEST=1）通过；错误分类纯函数可单测。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：体验盘点发现（`error.rs:387` 兜底挡住 Git 错误 details）。待复现与修复。 |
