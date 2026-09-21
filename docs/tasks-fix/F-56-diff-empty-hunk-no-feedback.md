# F-56 变更页 diff 统一/并排视图空白（零 hunk 文件无空态提示）

> 状态：✅ 已完成
> 优先级：P1（用户实测困惑：以为功能坏了；实际数据是"零行级差异"）
> 来源：2026-09-21 用户实测反馈（变更与批量操作页）。

## 问题描述

变更与批量操作页双击某些变更文件后，diff 面板的「统一」「并排」视图**完全空白**，
只有「全文」能查看内容。

## 根因定位（已用真实仓库数据复现，2026-09-21）

实测仓库 `D:\AWork\Code\lims\rls-CI3.4.1\remote-env\lims-mvp-base_950`（临时探针
`get_workdir_diff_with_config`，跑完即删）：

```
hussar-front/src/themeData/defaultCss.js      status=modified hunks=0
hussar-front/src/themeData/defaultCssVars.js  status=modified hunks=0
hussar-front/src/themeData/defaultScheme.js   status=modified hunks=0
hussar-front/src/themeData/meta.js            status=modified hunks=0
```

这 4 个文件正是 git CLI 警告 `LF will be replaced by CRLF` 的**纯行尾符差异**文件：
libgit2 按 core.autocrlf 规范化后内容与 HEAD 完全一致 → delta 状态为 modified 但
**零 hunk**（git CLI 的 `git diff` 同样不列出这些文件，但 `git status` 标 M）。

前端链路：`onFileDblClick` → `getDiff` 找到该文件（match 成功，pane 打开）→
`UnifiedDiff`/`SideBySideDiff` 的 `rows` 由 `file.hunks` 展开 → `hunks=[]` 时
VirtualList 渲染 0 行 → **空白面板，无任何提示**。而「全文」模式走
`read_workdir_file` 直接读磁盘，与 hunks 无关 → 正常显示。

同类零 hunk 场景：二进制文件（`Patch::from_diff` 返回 None）、纯权限/mode 变化、
untracked 目录（`new_path` 以 `/` 结尾，`full_add_hunk_for_file` 不填充）。

## 修复范围 checklist

- [x] 1. `UnifiedDiff.vue`：`rows` 为空时渲染 `n-empty` 空态（"没有可展示的行级
  差异（可能仅行尾符/权限变化，或为二进制文件）"），替代空白。
- [x] 2. `SideBySideDiff.vue`：同上。
  （放在两个共享组件内，RepositoryList 与 DiffViewer 两个消费方同时修复。）

## 不做（范围控制）

- 后端不从 diff 结果中剔除零 hunk 文件（变更列表来自 status 而非 diff；且 AI /
  staging 等消费方依赖 FileDiff 完整性）。
- 不改动「全文」模式的行状态叠加逻辑。

## 验收标准

1. 双击纯行尾符差异文件（如 themeData/*.js）：统一/并排视图显示明确的空态说明，
   不再空白；全文视图行为不变。
2. 正常修改的文件：统一/并排视图渲染与此前一致。
3. `pnpm build`（vue-tsc + vite）通过。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-21 | 开始修复。已用探针在用户真实仓库复现：4 个 themeData 文件 modified 但零 hunk（纯 EOL 差异），统一/并排 rows 为空 → 空白；全文走磁盘读取不受影响。修法：两个 diff 组件加零 hunk 空态。 |
| 2026-09-21 | 修复完成。`UnifiedDiff`/`SideBySideDiff` 在 rows 为空时渲染 n-empty 说明（RepositoryList 与 DiffViewer 同时受益）；`pnpm build` 绿。实机回归：变更页双击 themeData/defaultCss.js 等纯 EOL 差异文件，统一/并排应显示空态说明而非空白。 |
