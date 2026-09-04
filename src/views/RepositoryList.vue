<template>
  <div class="repository-list">
    <!-- Top toolbar（D-05：导航按钮移除，仅保留操作类） -->
    <div class="toolbar">
      <div class="toolbar-left">
        <n-button
          type="primary"
          :loading="repoStore.scanning"
          :disabled="!currentWorkspaceId"
          @click="handleScan"
        >
          <template #icon><n-icon><RefreshOutline /></n-icon></template>
          扫描仓库
        </n-button>
        <n-button
          :disabled="!currentWorkspaceId"
          @click="toggleWatcher"
        >
          <template #icon><n-icon><DesktopOutline /></n-icon></template>
          {{ watcherActive ? "停止监听" : "启动监听" }}
        </n-button>
        <n-button @click="showLogManager = true">
          <template #icon><n-icon><FolderOpenOutline /></n-icon></template>
          日志
        </n-button>
        <n-button :disabled="changes.length === 0" @click="openChangesAssistant">
          <template #icon><n-icon><SparklesOutline /></n-icon></template>
          AI 助手
        </n-button>
      </div>
      <div class="toolbar-right">
        <n-input
          ref="searchInputRef"
          v-model:value="searchQuery"
          placeholder="搜索文件或仓库..."
          style="width: 240px"
          clearable
        >
          <template #prefix><n-icon><SearchOutline /></n-icon></template>
        </n-input>
      </div>
    </div>

    <div class="main-body">
      <div class="tree-pane">
        <!-- Stats summary -->
        <div class="stats-bar">
          <span>共 {{ repoStore.totalCount }} 个仓库</span>
          <span class="separator">|</span>
          <span>{{ dirtyRepoCount }} 个有变更</span>
          <span class="separator">|</span>
          <span>{{ totalChangedFiles }} 个文件变更</span>
          <span v-if="selectedFileCount > 0" class="separator">|</span>
          <span v-if="selectedFileCount > 0" class="selected-info">
            已勾选 {{ selectedRepoCount }} 个仓库 / {{ selectedFileCount }} 个文件
          </span>
          <span class="tree-controls">
            <n-radio-group v-model:value="treeViewMode" size="small">
              <n-radio-button value="tree">目录树</n-radio-button>
              <n-radio-button value="flat">平铺</n-radio-button>
            </n-radio-group>
            <n-button size="small" @click="expandAll">
              <template #icon><n-icon><ExpandOutline /></n-icon></template>
              展开全部
            </n-button>
            <n-button size="small" @click="collapseAll">
              <template #icon><n-icon><ContractOutline /></n-icon></template>
              收起全部
            </n-button>
          </span>
        </div>

        <!-- Scan progress bar -->
        <div v-if="scanProgress" class="scan-progress">
          <n-progress
            type="line"
            :percentage="scanPercentage"
            :status="scanPercentage === 100 ? 'success' : 'default'"
            :stroke-height="16"
            processing
          >
            扫描状态 {{ scanProgress?.current ?? 0 }}/{{ scanProgress?.total ?? 0 }}
          </n-progress>
        </div>

        <!-- Change tree -->
        <!-- F-20：n-spin 加 tree-spin 参与 flex 布局（同 F-18/F-09b 模式）；
             空数据时不渲染树，空状态统一由下方 view 级 .empty-state 展示，
             避免 n-tree 默认「无数据」与内联空状态同时出现。 -->
        <n-spin :show="changesLoading" class="tree-spin">
          <div class="tree-container">
            <ChangeTree
              v-if="changes.length > 0"
              ref="changeTreeRef"
              :changes="changes"
              :view-mode="treeViewMode"
              @selection-change="onTreeSelection"
              @file-dblclick="onFileDblClick"
              @contextmenu="onTreeContextmenu"
            />
            <div
              v-if="!changesLoading && currentWorkspaceId && changes.length === 0"
              class="empty-state"
            >
              <n-empty description="未发现任何 Git 仓库">
                <n-button type="primary" @click="handleScan">重新扫描</n-button>
              </n-empty>
            </div>
            <div
              v-else-if="!currentWorkspaceId"
              class="empty-state"
            >
              <n-empty description="请先添加工作区目录">
                <n-button type="primary" @click="router.push({ name: 'workspaces' })">
                  前往工作区管理
                </n-button>
              </n-empty>
            </div>
          </div>
        </n-spin>
      </div>

      <!-- D-15 Middle: commit graph pane -->
      <!-- D-16：树 | 提交图 splitter（把手贴 graph-pane 左缘） -->
      <div
        v-if="selectedRepoPath && graphCommits.length > 0"
        class="resize-handle"
        @mousedown="startResize('graph', $event)"
      ></div>
      <div
        v-if="selectedRepoPath && graphCommits.length > 0"
        ref="graphPaneEl"
        class="graph-pane"
        :style="graphWidth ? { width: graphWidth + 'px' } : undefined"
      >
        <div class="graph-pane-header">
          <span class="graph-pane-title">{{ repoNameOf(selectedRepoPath) }}</span>
        </div>
        <n-spin :show="graphLoading" class="graph-pane-spin">
          <CommitGraph
            :commits="graphCommits"
            :loading="graphLoading"
            @select="onCommitSelect"
            @contextmenu="onGraphCommitContextmenu"
          />
        </n-spin>
        <!-- D-13：提交节点右键菜单（轻量版：复制 hash / 查看 Diff） -->
        <ContextMenu
          :show="graphCommitMenu.show"
          :options="graphCommitMenuOptions"
          :x="graphCommitMenu.x"
          :y="graphCommitMenu.y"
          @select="onGraphCommitMenuSelect"
          @close="graphCommitMenu.show = false"
        />
      </div>

      <!-- Right: change content of double-clicked file -->
      <!-- F-09b：diff 面板必须是 .main-body 的直接 flex 子元素，n-spin 只能
           包内部内容——否则 spin 容器塌陷后面板会盖到左侧树上。 -->
      <div
        v-if="selectedDiff"
        class="resize-handle"
        @mousedown="startResize('diff', $event)"
      ></div>
      <div
        v-if="selectedDiff"
        ref="diffPaneEl"
        class="diff-pane"
        :style="{ width: diffWidth ? diffWidth + 'px' : '46%' }"
      >
        <div class="diff-pane-header">
          <div class="diff-pane-title">
            <span class="diff-repo">{{ repoNameOf(selectedDiff.repoPath) }}</span>
            <span class="diff-file">{{ selectedDiff.relPath }}</span>
            <n-tag size="small" :bordered="false">
              {{ statusText(selectedDiff.file.status) }}
            </n-tag>
          </div>
          <n-button
            size="small"
            text
            @click="selectedDiff = null"
          >
            <template #icon><n-icon><CloseOutline /></n-icon></template>
          </n-button>
        </div>
        <n-spin :show="diffLoading" class="diff-pane-spin">
          <div class="diff-pane-body">
            <UnifiedDiff :file="selectedDiff.file" />
          </div>
        </n-spin>
      </div>
    </div>

    <!-- Bottom: batch operations panel (always visible, buttons disable instead of hiding) -->
    <div class="commit-panel">
      <div class="commit-panel-header">
        <n-button
          size="small"
          text
          @click="commitPanelOpen = !commitPanelOpen"
        >
          <template #icon>
            <n-icon><ChevronDownOutline v-if="commitPanelOpen" /><ChevronUpOutline v-else /></n-icon>
          </template>
          {{ commitPanelOpen ? "收起" : "展开" }}批量操作
        </n-button>
        <span v-if="commitPanelOpen" class="commit-panel-hint">
          {{ selectedFileCount > 0
            ? `已勾选 ${selectedFileCount} 个文件（${selectedRepoCount} 个仓库）`
            : "在左侧勾选变更文件后即可操作" }}
        </span>
      </div>
      <div v-if="commitPanelOpen" class="commit-panel-body">
        <div class="ops-row">
          <n-button-group>
            <n-button
              size="small"
              :loading="actionLoading"
              :disabled="selectedFileCount === 0"
              @click="handleAdd"
            >
              <template #icon><n-icon><AddCircleOutline /></n-icon></template>
              Add（暂存）
            </n-button>
            <n-button
              size="small"
              :loading="actionLoading"
              @click="() => handlePull()"
            >
              <template #icon><n-icon><RefreshOutline /></n-icon></template>
              Pull
            </n-button>
            <n-button
              size="small"
              :loading="actionLoading"
              @click="() => handleFetch()"
            >
              <template #icon><n-icon><CloudDownloadOutline /></n-icon></template>
              Fetch
            </n-button>
            <n-button
              size="small"
              :loading="actionLoading"
              @click="() => openPushDialog()"
            >
              <template #icon><n-icon><CloudUploadOutline /></n-icon></template>
              Push
            </n-button>
            <n-button
              size="small"
              type="error"
              dashed
              :loading="actionLoading"
              :disabled="selectedFileCount === 0"
              @click="handleRestore"
            >
              <template #icon><n-icon><ArrowUndoOutline /></n-icon></template>
              回退
            </n-button>
          </n-button-group>
          <n-button
            size="small"
            :disabled="!selectedRepoPath"
            @click="viewGraph(selectedRepoPath)"
          >
            <template #icon><n-icon><ShareOutline /></n-icon></template>
            Graph
          </n-button>
          <n-button
            size="small"
            :disabled="!selectedRepoPath"
            @click="viewDiff(selectedRepoPath)"
          >
            <template #icon><n-icon><EyeOutline /></n-icon></template>
            Diff
          </n-button>
          <n-button
            size="small"
            :disabled="!selectedRepoPath"
            @click="viewBranches(selectedRepoPath)"
          >
            <template #icon><n-icon><GridOutline /></n-icon></template>
            分支
          </n-button>
          <n-button
            size="small"
            :disabled="!selectedRepoPath"
            @click="viewStash(selectedRepoPath)"
          >
            <template #icon><n-icon><ArchiveOutline /></n-icon></template>
            Stash
          </n-button>
          <n-button
            size="small"
            :disabled="!selectedRepoPath"
            @click="viewWorktrees(selectedRepoPath)"
          >
            <template #icon><n-icon><DocumentsOutline /></n-icon></template>
            Worktree
          </n-button>
          <n-button
            size="small"
            :disabled="!currentWorkspaceId"
            @click="viewConflicts"
          >
            <template #icon><n-icon><WarningOutline /></n-icon></template>
            冲突
          </n-button>
        </div>
        <!-- F-09f：中间选择器用途不明、可用性存疑，暂时隐藏（保留代码，后续决定去向）。 -->
        <div v-if="false" class="batch-row">
          <n-input
            v-model:value="selectorQuery"
            size="small"
            class="selector-input"
            placeholder="选择器：@group:frontend @tag:p0 @status:dirty 或名称关键字"
            clearable
          />
          <n-tag
            v-for="chip in quickChips"
            :key="chip.token"
            :checkable="true"
            :checked="chip.active"
            @update:checked="(v: boolean) => toggleChip(chip, v)"
          >
            {{ chip.label }}
          </n-tag>
          <span v-if="selectorActive" class="selector-count" :class="{ 'is-empty': selectorPaths.length === 0 }">
            <template v-if="!currentWorkspaceId">请先选择工作区</template>
            <template v-else>
              匹配 {{ selectorPaths.length }} 个仓库
              <template v-if="selectorPaths.length === 0">（无匹配：检查分组/标签/状态条件是否正确）</template>
            </template>
          </span>
        </div>
        <div class="batch-row">
          <n-button-group>
            <n-button size="small" @click="openBranchOp('checkout')">
              Checkout All
            </n-button>
            <n-button size="small" @click="openBranchOp('create')">
              Create Branch All
            </n-button>
            <n-button
              size="small"
              type="error"
              dashed
              @click="openBranchOp('delete')"
            >
              Delete Branch All
            </n-button>
          </n-button-group>
          <n-button-group>
            <n-button size="small" @click="runDryRun('pull')">
              Pull 预演
            </n-button>
            <n-button size="small" @click="runDryRun('push')">
              Push 预演
            </n-button>
          </n-button-group>
          <n-button size="small" @click="openWsStashDialog">
            <template #icon><n-icon><FolderOutline /></n-icon></template>
            Workspace Stash
          </n-button>
        </div>
        <div class="commit-row">
          <div class="commit-input">
            <n-input
              v-model:value="commitForm.message"
              type="textarea"
              :rows="2"
              placeholder="请输入 commit message"
              :disabled="selectedFileCount === 0"
            />
          </div>
          <div class="commit-scope">
            <div
              v-for="(files, repoPath) in selectedFilesByRepo"
              :key="repoPath"
              class="scope-item"
            >
              <span class="scope-repo">{{ repoNameOf(repoPath) }}</span>
              <span class="scope-count">（{{ files.length }} 个文件）</span>
            </div>
          </div>
          <n-button
            type="primary"
            :loading="actionLoading"
            :disabled="selectedFileCount === 0"
            @click="handleCommit()"
          >
            <template #icon><n-icon><CreateOutline /></n-icon></template>
            提交
          </n-button>
        </div>
        <!-- Commit options (T-11) -->
        <div class="commit-options">
          <n-checkbox v-model:checked="commitForm.amend" size="small">
            Amend 上次提交
          </n-checkbox>
          <n-checkbox v-model:checked="commitForm.thenPush" size="small">
            提交后 Push
          </n-checkbox>
          <n-button
            size="small"
            text
            :disabled="!selectedRepoPath"
            @click="openIdentityDialog"
          >
            提交身份
          </n-button>
        </div>
      </div>
    </div>

    <!-- Bulk branch op dialog (T-20) -->
    <n-modal v-model:show="branchOpDialog.show" preset="card" :title="branchOpTitle" style="width: 520px">
      <n-form label-width="80px">
        <n-form-item label="分支名">
          <n-input v-model:value="branchOpDialog.name" placeholder="分支名" />
        </n-form-item>
        <n-form-item v-if="branchOpDialog.op === 'delete'" label="强制">
          <n-checkbox v-model:checked="branchOpDialog.force">
            强制删除未合并分支
          </n-checkbox>
        </n-form-item>
      </n-form>
      <n-alert
        v-if="branchOpDialog.op === 'delete'"
        type="error"
        :bordered="false"
      >
        危险操作：将从以下仓库删除分支
      </n-alert>
      <n-alert
        v-else
        type="info"
        :bordered="false"
      >
        将作用于 {{ branchOpTargets.length }} 个仓库
      </n-alert>
      <ul v-if="branchOpDialog.op === 'delete'" class="affected-repo-list">
        <li v-for="r in branchOpTargets" :key="r">
          {{ repoNameOf(r) }}
        </li>
      </ul>
      <template #footer>
        <n-button @click="branchOpDialog.show = false">取消</n-button>
        <n-button
          :type="branchOpDialog.op === 'delete' ? 'error' : 'primary'"
          :loading="branchOpDialog.loading"
          @click="handleBranchOp"
        >
          {{ branchOpActionLabel }}
        </n-button>
      </template>
    </n-modal>


    <!-- Dry-run impact report dialog (T-20) -->
    <n-modal
      v-model:show="dryRunDialog.show"
      preset="card"
      :title="dryRunDialog.op === 'pull' ? 'Pull 预演（不影响任何仓库）' : 'Push 预演（不影响任何仓库）'"
      style="width: 720px"
    >
      <n-data-table
        :columns="dryRunColumns"
        :data="dryRunDialog.items"
        :loading="dryRunDialog.loading"
        :max-height="400"
      />
      <template #footer>
        <n-button @click="dryRunDialog.show = false">关闭</n-button>
        <n-button
          v-if="dryRunActionable.length > 0"
          type="primary"
          @click="executeDryRun"
        >
          对 {{ dryRunActionable.length }} 个可快进仓库执行
          {{ dryRunDialog.op === 'pull' ? 'Pull' : 'Push' }}
        </n-button>
      </template>
    </n-modal>


    <!-- Workspace Stash dialog (T-21) -->
    <n-modal
      v-model:show="wsStashDialog.show"
      preset="card"
      title="Workspace Stash（多仓库暂存）"
      style="width: 760px"
    >
      <div class="ws-stash-save-row">
        <n-input
          v-model:value="wsStashDialog.message"
          size="small"
          placeholder="备注信息（可选）"
          style="max-width: 260px"
          clearable
        />
        <n-checkbox v-model:checked="wsStashDialog.includeUntracked" size="small">
          包含未跟踪文件
        </n-checkbox>
        <n-button
          size="small"
          type="primary"
          :loading="wsStashDialog.saving"
          :disabled="wsStashTargetCount === 0"
          @click="saveWsStash"
        >
          暂存选中组（{{ wsStashTargetCount }} 个仓库）
        </n-button>
      </div>
      <n-alert
        v-if="wsStashDialog.lastSave"
        :type="wsStashDialog.lastSave.id != null ? 'success' : 'info'"
        :bordered="false"
        class="ws-stash-save-result"
      >
        {{ wsStashSaveSummary }}
      </n-alert>
      <n-data-table
        :columns="wsStashColumns"
        :data="wsStashDialog.list"
        :loading="wsStashDialog.loading"
        :max-height="380"
        :row-key="(row: WorkspaceStashSummary) => row.id"
        :expanded-row-keys="expandedWsStashKeys"
        @update:expanded-row-keys="(keys: (string | number)[]) => onWsStashExpand(keys as number[])"
      />
    </n-modal>

    <!-- Workspace Stash restore: pre-check + §46 Warning confirm (T-21) -->
    <n-modal
      v-model:show="wsStashCheck.show"
      preset="card"
      :title="`恢复 ${wsStashCheck.name}`"
      style="width: 680px"
    >
      <n-alert
        type="warning"
        :bordered="false"
      >
        恢复将把各仓库的 stash 应用回工作区（stash 条目保留，可重复恢复）。以下为影响仓库与恢复前校验结果：
      </n-alert>
      <n-data-table
        :columns="wsStashCheckColumns"
        :data="wsStashCheck.items"
        :loading="wsStashCheck.loading"
        :max-height="320"
      />
      <n-checkbox
        v-if="wsStashCheck.items.some((i) => i.status === 'branch_mismatch')"
        v-model:checked="wsStashCheck.allowMismatch"
        size="small"
        class="ws-stash-mismatch-allow"
      >
        允许在分支不一致的仓库上恢复（变更会落到当前分支）
      </n-checkbox>
      <template #footer>
        <n-button @click="wsStashCheck.show = false">取消</n-button>
        <n-button
          type="warning"
          :loading="wsStashCheck.restoring"
          :disabled="wsStashApplicableCount === 0"
          @click="confirmWsStashRestore"
        >
          确认恢复（{{ wsStashApplicableCount }} 个仓库）
        </n-button>
      </template>
    </n-modal>

    <!-- Commit identity dialog (T-11 §54) -->
    <n-modal v-model:show="identityDialog.show" preset="card" title="提交身份" style="width: 480px">
      <div class="identity-current">
        当前生效：
        <template v-if="identityDialog.current">
          <strong>
            {{ identityDialog.current.name }} &lt;{{ identityDialog.current.email }}&gt;
          </strong>
          <n-tag size="small" style="margin-left: 6px">
            {{ identitySourceLabel }}
          </n-tag>
        </template>
        <n-tag v-else size="small" type="info">
          Git 默认（user.name / user.email）
        </n-tag>
      </div>
      <n-form label-width="70px" style="margin-top: 12px">
        <n-form-item label="作用于">
          <n-radio-group v-model:value="identityDialog.scope">
            <n-radio value="repo">本仓库</n-radio>
            <n-radio value="group" :disabled="identityDialog.groupId == null">
              本分组
            </n-radio>
          </n-radio-group>
        </n-form-item>
        <n-form-item label="Name">
          <n-input v-model:value="identityDialog.name" placeholder="留空并保存 = 清除自定义" />
        </n-form-item>
        <n-form-item label="Email">
          <n-input v-model:value="identityDialog.email" />
        </n-form-item>
      </n-form>
      <template #footer>
        <n-button @click="identityDialog.show = false">取消</n-button>
        <n-button
          type="primary"
          :loading="identityDialog.saving"
          @click="saveIdentity"
        >
          保存
        </n-button>
      </template>
    </n-modal>

    <!-- Pre-commit safety findings dialog (T-11 §5) -->
    <n-modal v-model:show="scanDialog.show" preset="card" title="提交安全检查" style="width: 560px">
      <n-alert
        type="warning"
        :bordered="false"
      >
        发现以下风险项，确认无误后可放行提交：
      </n-alert>
      <ul class="scan-finding-list">
        <li v-for="(f, i) in scanDialog.findings" :key="i">
          <n-tag
            size="small"
            :type="f.kind === 'forbidden' ? 'error' : 'warning'"
          >
            {{ f.kind }}
          </n-tag>
          <span class="scan-path">{{ f.path }}</span>
          <span class="scan-detail">{{ f.detail }}</span>
        </li>
      </ul>
      <template #footer>
        <n-button @click="scanDialog.show = false">取消</n-button>
        <n-button type="error" @click="commitWithOverride">
          仍要提交
        </n-button>
      </template>
    </n-modal>

    <LogManager v-model="showLogManager" />

    <!-- D-13 右键菜单 -->
    <ContextMenu
      :show="contextMenu.show"
      :options="contextMenuOptions"
      :x="contextMenu.x"
      :y="contextMenu.y"
      @select="onContextmenuSelect"
      @close="onContextmenuClose"
    />

    <!-- Push repo picker dialog -->
    <n-modal v-model:show="showPushDialog" preset="card" title="选择要 Push 的仓库" style="width: 680px">
      <n-data-table
        :columns="pushColumns"
        :data="changes"
        :row-key="(row: RepoChanges) => row.repoPath"
        :checked-row-keys="pushSelection"
        @update:checked-row-keys="(keys: (string | number)[]) => onPushSelectionChange(keys as string[])"
        :max-height="360"
      />
      <template #footer>
        <n-button @click="showPushDialog = false">取消</n-button>
        <n-button
          type="primary"
          :loading="actionLoading"
          :disabled="pushSelection.length === 0"
          @click="doPush"
        >
          Push（{{ pushSelection.length }}）
        </n-button>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { computed, h, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import {
  AddCircleOutline,
  ArchiveOutline,
  ArrowUndoOutline,
  CloudDownloadOutline,
  CloudUploadOutline,
  CloseOutline,
  ChevronDownOutline,
  ChevronUpOutline,
  ContractOutline,
  CreateOutline,
  DesktopOutline,
  DocumentsOutline,
  ExpandOutline,
  EyeOutline,
  FolderOpenOutline,
  GridOutline,
  RefreshOutline,
  SearchOutline,
  ShareOutline,
  SparklesOutline,
  WarningOutline,
} from "@vicons/ionicons5";
import { NButton, NIcon, NTag, useMessage, useDialog } from "naive-ui";
import { listen } from "@tauri-apps/api/event";
import { useWorkspaceStore } from "@/stores/workspace";
import { useRepositoryStore } from "@/stores/repository";
import ContextMenu from "@/components/shell/ContextMenu.vue";
import CommitGraph from "@/components/graph/CommitGraph.vue";
import { getCommitHistory } from "@/api/graph";
import type { CommitInfo } from "@/types/graph";
import { startWatcher, stopWatcher, batchCommit, batchFetch, batchPull, batchPush } from "@/api/git_ops";
import { open as openPath } from "@tauri-apps/plugin-shell";
import { getWorkspaceHealth } from "@/api/health";
import {
  scanCommit,
  getCommitIdentity,
  setRepoIdentity,
  setGroupIdentity,
} from "@/api/commit";
import type { CommitScanFinding, CommitIdentity } from "@/types/commit";
import { selectRepos, batchBranchOp, batchDryRun } from "@/api/batch";
import { guardRuntimeRunning } from "@/utils/runtimeGuard";
import type { DryRunItem } from "@/types/batch";
import {
  saveWorkspaceStash,
  listWorkspaceStashes,
  getWorkspaceStashItems,
  checkWorkspaceStash,
  restoreWorkspaceStash,
  deleteWorkspaceStash,
} from "@/api/workspaceStash";
import type {
  SaveWorkspaceStashResult,
  WorkspaceStashCheckItem,
  WorkspaceStashItemEntry,
  WorkspaceStashSummary,
} from "@/types/workspaceStash";
import { getDiff, getUnstagedDiff, readFileAsDiff } from "@/api/git";
import { batchAdd, batchRestore, getWorkspaceChanges, type AddRequest, type RestoreRequest } from "@/api/changes";
import type { CommitRequest } from "@/types/task";
import type { RepoChanges } from "@/types/changes";
import type { ScanProgress } from "@/types/events";
import type { FileDiff } from "@/types/git";
import ChangeTree, {
  type ChangeNode,
  type TreeSelection,
} from "@/components/repo/ChangeTree.vue";
import UnifiedDiff from "@/components/diff/UnifiedDiff.vue";
import LogManager from "@/components/common/LogManager.vue";
import { errMsg } from "@/utils/error";
import { COMMIT_REQUEST_EVENT } from "@/commands/registry";
import { useAiAssistant } from "@/composables/useAiAssistant";

interface SelectedDiff {
  repoPath: string;
  relPath: string;
  file: FileDiff;
}

const router = useRouter();
const route = useRoute();
const workspaceStore = useWorkspaceStore();
const repoStore = useRepositoryStore();
const message = useMessage();
const dialog = useDialog();
const { openAssistant } = useAiAssistant();

// D-05：响应全局工作区 store
const currentWorkspaceId = computed(() => workspaceStore.currentWorkspace?.id ?? null);
const showLogManager = ref(false);
const watcherActive = ref(false);
const searchQuery = ref("");
const changes = ref<RepoChanges[]>([]);
const changesLoading = ref(false);

// 树展示模式（localStorage 持久化，默认 tree 目录树模式）。
const TREE_VIEW_MODE_KEY = "gw-changes-tree-view-mode";
const treeViewMode = ref<"tree" | "flat">(
  (localStorage.getItem(TREE_VIEW_MODE_KEY) as "tree" | "flat") ?? "tree",
);
watch(treeViewMode, (mode) => localStorage.setItem(TREE_VIEW_MODE_KEY, mode));
const actionLoading = ref(false);
const commitPanelOpen = ref(true);
const commitForm = ref({ message: "", amend: false, thenPush: false });
const scanDialog = ref<{
  show: boolean;
  findings: CommitScanFinding[];
  pending: CommitRequest[];
}>({ show: false, findings: [], pending: [] });
const identityDialog = ref({
  show: false,
  saving: false,
  scope: "repo" as "repo" | "group",
  name: "",
  email: "",
  current: null as CommitIdentity | null,
  groupId: null as number | null,
});

// D-15：三栏联动状态
const graphCommits = ref<CommitInfo[]>([]);
const graphLoading = ref(false);

async function loadGraphCommits(repoPath: string) {
  graphLoading.value = true;
  try {
    graphCommits.value = await getCommitHistory(repoPath, 200);
  } catch (e) {
    console.error("Failed to load commits:", e);
    graphCommits.value = [];
  } finally {
    graphLoading.value = false;
  }
}

function onCommitSelect(commit: CommitInfo) {
  // D-15 三栏联动：选中提交 → 右侧 diff 面板显示该提交的变更
  viewCommitDiff(selectedRepoPath.value, commit);
}

// D-13：graph-pane 提交节点右键（轻量菜单，历史操作请进提交图视图）
const graphCommitMenu = ref({
  show: false,
  x: 0,
  y: 0,
  commit: null as CommitInfo | null,
});

const graphCommitMenuOptions = [
  { label: "Copy hash", key: "copy-hash" },
  { label: "查看 Diff", key: "diff" },
];

function onGraphCommitContextmenu(commit: CommitInfo, x: number, y: number) {
  graphCommitMenu.value = { show: true, x, y, commit };
}

async function onGraphCommitMenuSelect(key: string) {
  const commit = graphCommitMenu.value.commit;
  if (!commit) return;
  if (key === "copy-hash") {
    try {
      await navigator.clipboard.writeText(commit.oid);
      message.success(`已复制 ${commit.shortOid}`);
    } catch {
      message.error("复制失败");
    }
    return;
  }
  if (key === "diff") {
    viewCommitDiff(selectedRepoPath.value, commit);
  }
}

function viewCommitDiff(repoPath: string | undefined, commit: CommitInfo) {
  if (!repoPath) return;
  router.push({
    name: "diff-viewer",
    query: { repo: repoPath, commit: commit.oid },
  });
}

// D-13：右键菜单状态
const contextMenu = ref({
  show: false,
  x: 0,
  y: 0,
  node: null as ChangeNode | null,
});

const contextMenuOptions = computed(() => {
  const node = contextMenu.value.node;
  if (!node) return [];

  if (node.type === "repo") {
    return [
      { label: "Fetch", key: "fetch" },
      { label: "Pull", key: "pull" },
      { label: "Push…", key: "push" },
      { type: "divider", key: "d1" },
      { label: "选中该仓库全部文件（准备提交）", key: "commit" },
      { label: "健康检查", key: "health" },
      { label: "在文件管理器显示", key: "reveal" },
      { type: "divider", key: "d2" },
      { label: "查看提交图", key: "graph" },
    ];
  }

  if (node.type === "file") {
    // 文件级 Unstage 无后端能力（仅 hunk/line 级），不提供假入口
    return [
      { label: "Stage", key: "stage" },
      { type: "divider", key: "d1" },
      {
        label: "Discard（丢弃工作区改动）",
        key: "discard",
        props: { style: "color: var(--gw-danger)" },
      },
      { label: "查看 Diff", key: "diff" },
    ];
  }

  return [];
});

function onTreeContextmenu(node: ChangeNode, x: number, y: number) {
  contextMenu.value = { show: true, x, y, node };
}

function repoChangesOf(repoPath: string) {
  return changes.value.find((c) => c.repoPath === repoPath);
}

async function onContextmenuSelect(key: string) {
  const node = contextMenu.value.node;
  if (!node) return;
  const repoPath = node.repoPath;
  if (!repoPath) return;
  const repo = repoChangesOf(repoPath);

  try {
    switch (key) {
      case "fetch":
        await handleFetch([repoPath]);
        break;
      case "pull":
        await handlePull([repoPath]);
        break;
      case "push":
        openPushDialog([repoPath]);
        break;
      case "commit": {
        // 与勾选该仓库全部文件等价：提交本身仍走表单确认（Safety First）
        const files = (repo?.changes ?? [])
          .filter((f) => !f.path.endsWith("/"))
          .map((f) => f.path);
        treeSelection.value = {
          repoPaths: [repoPath],
          filesByRepo: new Map([[repoPath, files]]),
        };
        message.success(`已选中 ${repoNameOf(repoPath)} 的 ${files.length} 个文件，请在上方填写提交信息`);
        break;
      }
      case "health": {
        // 编排已有能力：工作区健康检查 → 展示该仓库评分与异常项
        const ws = workspaceStore.currentWorkspace;
        if (!ws) {
          message.warning("未选择工作区");
          break;
        }
        const health = await getWorkspaceHealth(ws.id);
        const rh = health.repos.find((r) => r.repoPath === repoPath);
        if (!rh) {
          message.warning(`${repoNameOf(repoPath)} 不在健康检查结果中`);
          break;
        }
        dialog.info({
          title: `健康检查 · ${rh.repoName}`,
          content: rh.anomalies.length
            ? `评分 ${rh.score}%，异常项：\n${rh.anomalies.join("、")}`
            : `评分 ${rh.score}%，无异常`,
          positiveText: "知道了",
        });
        break;
      }
      case "reveal":
        await openPath(repoPath);
        break;
      case "stage": {
        if (!repo || !node.relPath) break;
        await batchAdd([
          { repoPath, repoName: repo.repoName, files: [node.relPath] },
        ]);
        message.success(`已暂存 ${node.relPath}`);
        await loadChanges();
        break;
      }
      case "discard": {
        if (!repo || !node.relPath) break;
        const file = node.relPath;
        dialog.error({
          title: "危险操作确认",
          content: `确定丢弃 ${repo.repoName} 中「${file}」的工作区改动吗？此操作不可撤销。`,
          positiveText: "确认丢弃",
          negativeText: "取消",
          onPositiveClick: async () => {
            try {
              await batchRestore([
                { repoPath, repoName: repo.repoName, files: [file] },
              ]);
              message.success(`已丢弃 ${file}`);
              await loadChanges();
            } catch (e) {
              message.error("discard 失败: " + errMsg(e));
            }
          },
        });
        break;
      }
      case "diff":
        viewDiff(repoPath);
        break;
      case "graph":
        viewGraph(repoPath);
        break;
    }
  } catch (e) {
    message.error("操作失败: " + errMsg(e));
  }
}

function onContextmenuClose() {
  contextMenu.value.show = false;
}

// --- Batch selector + repo-level ops (T-20) ---
const selectorQuery = ref("");
const selectorPaths = ref<string[]>([]);
const selectorActive = computed(() => selectorQuery.value.trim().length > 0);
const quickChips = ref([
  { label: "脏", token: "@status:dirty", active: false },
  { label: "冲突", token: "@status:conflict", active: false },
  { label: "Ahead", token: "@status:ahead", active: false },
  { label: "Behind", token: "@status:behind", active: false },
  { label: "收藏", token: "@status:favorite", active: false },
]);
const branchOpTargets = ref<string[]>([]);
const branchOpDialog = ref({
  show: false,
  loading: false,
  op: "checkout" as "checkout" | "create" | "delete",
  name: "",
  force: false,
});
const dryRunDialog = ref({
  show: false,
  loading: false,
  op: "pull" as "pull" | "push",
  items: [] as DryRunItem[],
});
// --- Workspace Stash (T-21) ---
const wsStashDialog = ref({
  show: false,
  loading: false,
  saving: false,
  message: "",
  includeUntracked: true,
  list: [] as WorkspaceStashSummary[],
  items: {} as Record<number, WorkspaceStashItemEntry[]>,
  lastSave: null as SaveWorkspaceStashResult | null,
});
const wsStashCheck = ref({
  show: false,
  loading: false,
  restoring: false,
  id: 0,
  name: "",
  allowMismatch: false,
  items: [] as WorkspaceStashCheckItem[],
});
const scanProgress = ref<{ found: number; current: number; total: number | null } | null>(null);
const selectedDiff = ref<SelectedDiff | null>(null);
const diffLoading = ref(false);

const treeSelection = ref<TreeSelection>({ repoPaths: [], filesByRepo: new Map() });
const changeTreeRef = ref<InstanceType<typeof ChangeTree> | null>(null);
const showPushDialog = ref(false);
const pushSelection = ref<string[]>([]);

// D-16：splitter 位置按视图 key 持久化（gw-splitter:<视图>:<面板>）；
// 旧全局 key gw-diff-width 首次保存后迁移移除。
const DIFF_WIDTH_KEY = "gw-splitter:changes:diff";
const GRAPH_WIDTH_KEY = "gw-splitter:changes:graph";
const LEGACY_DIFF_WIDTH_KEY = "gw-diff-width";

function loadStoredWidth(key: string, legacyKey?: string): number | null {
  const raw =
    localStorage.getItem(key) ??
    (legacyKey ? localStorage.getItem(legacyKey) : null);
  if (raw === null) return null;
  const n = Number(raw);
  return Number.isFinite(n) && n > 0 ? n : null;
}

// 恢复值与拖拽共用同一夹紧：下限保证树/图可用，上限按视口比例
const clampDiffWidth = (w: number) =>
  Math.max(320, Math.min(window.innerWidth * 0.7, w));
const clampGraphWidth = (w: number) =>
  Math.max(240, Math.min(window.innerWidth * 0.5, w));

const storedDiffWidth = loadStoredWidth(DIFF_WIDTH_KEY, LEGACY_DIFF_WIDTH_KEY);
const storedGraphWidth = loadStoredWidth(GRAPH_WIDTH_KEY);
const diffWidth = ref<number | null>(
  storedDiffWidth === null ? null : clampDiffWidth(storedDiffWidth),
);
const graphWidth = ref<number | null>(
  storedGraphWidth === null ? null : clampGraphWidth(storedGraphWidth),
);
const diffPaneEl = ref<HTMLElement | null>(null);
const graphPaneEl = ref<HTMLElement | null>(null);
const expandedWsStashKeys = ref<number[]>([]);

function persistWidth(key: string, value: number, legacyKey?: string) {
  localStorage.setItem(key, String(value));
  if (legacyKey) localStorage.removeItem(legacyKey);
}

type ResizePane = "diff" | "graph";
let resizePane: ResizePane | null = null;
let resizeStartX = 0;
let resizeStartWidth = 0;

// F-20：直接去柯里化——旧写法 startResize(pane) 只返回闭包，模板里
// @mousedown="startResize('graph')" 拿到处理器后被丢弃，拖拽从未生效。
function startResize(pane: ResizePane, e: MouseEvent) {
  e.preventDefault();
  resizePane = pane;
  resizeStartX = e.clientX;
  resizeStartWidth =
    pane === "diff"
      ? diffPaneEl.value?.offsetWidth ?? 600
      : graphPaneEl.value?.offsetWidth ?? 320;
  document.addEventListener("mousemove", onResizeMove);
  document.addEventListener("mouseup", endResize);
}

function onResizeMove(e: MouseEvent) {
  if (!resizePane) return;
  // 两个把手都贴在目标面板左缘：鼠标左移 → 面板变宽
  const delta = resizeStartX - e.clientX;
  if (resizePane === "diff") {
    diffWidth.value = clampDiffWidth(resizeStartWidth + delta);
  } else {
    graphWidth.value = clampGraphWidth(resizeStartWidth + delta);
  }
}

function endResize() {
  document.removeEventListener("mousemove", onResizeMove);
  document.removeEventListener("mouseup", endResize);
  if (resizePane === "diff" && diffWidth.value !== null) {
    persistWidth(DIFF_WIDTH_KEY, diffWidth.value, LEGACY_DIFF_WIDTH_KEY);
  } else if (resizePane === "graph" && graphWidth.value !== null) {
    persistWidth(GRAPH_WIDTH_KEY, graphWidth.value);
  }
  resizePane = null;
}

// D-16：窗口缩放后对已恢复/已保存的宽度重新夹紧，防止挤瘪左侧面板
function onWindowResize() {
  if (diffWidth.value !== null) diffWidth.value = clampDiffWidth(diffWidth.value);
  if (graphWidth.value !== null) graphWidth.value = clampGraphWidth(graphWidth.value);
}
onMounted(() => window.addEventListener("resize", onWindowResize));
onUnmounted(() => window.removeEventListener("resize", onWindowResize));

// T-31：Ctrl+P / Ctrl+Shift+F 聚焦变更页搜索框（命令注册表 focus 查询参数）。
const searchInputRef = ref<{ focus: () => void } | null>(null);
watch(
  () => route.query.focus,
  async (focus) => {
    if (focus !== "search") return;
    await nextTick();
    searchInputRef.value?.focus();
  }
);

// T-31：Ctrl+Enter / Ctrl+Shift+Enter → 提交面板提交（可选推送）。
function onCommitRequest(e: Event) {
  const push = (e as CustomEvent<{ push?: boolean }>).detail?.push ?? false;
  if (!commitPanelOpen.value) {
    message.warning("请先展开提交面板再提交");
    return;
  }
  void handleCommit(push);
}
onMounted(() => window.addEventListener(COMMIT_REQUEST_EVENT, onCommitRequest));
onUnmounted(() =>
  window.removeEventListener(COMMIT_REQUEST_EVENT, onCommitRequest)
);

let unlistenScan: (() => void) | null = null;

const scanPercentage = computed(() => {
  if (!scanProgress.value || !scanProgress.value.total) return 0;
  return Math.round((scanProgress.value.current / scanProgress.value.total) * 100);
});

const dirtyRepoCount = computed(
  () => changes.value.filter((c) => c.changes.length > 0).length,
);

const totalChangedFiles = computed(() =>
  changes.value.reduce((sum, c) => sum + c.changes.length, 0),
);

const selectedRepoCount = computed(() => treeSelection.value.repoPaths.length);

const selectedFileCount = computed(() => {
  let n = 0;
  for (const files of treeSelection.value.filesByRepo.values()) n += files.length;
  return n;
});

const selectedFilesByRepo = computed(() => {
  const result: Record<string, string[]> = {};
  for (const [repoPath, files] of treeSelection.value.filesByRepo.entries()) {
    result[repoPath] = files;
  }
  return result;
});

const selectedRepoPath = computed(() =>
  treeSelection.value.repoPaths.length > 0
    ? treeSelection.value.repoPaths[0]
    : "",
);

function openChangesAssistant() {
  const paths = treeSelection.value.repoPaths.length > 0
    ? treeSelection.value.repoPaths
    : changes.value.map((change) => change.repoPath);
  openAssistant({
    repositoryPaths: paths,
    inferredRole: "gitReviewer",
    origin: `Changes · ${paths.length} 个仓库`,
    draft: "请总结当前仓库变更、风险和建议的下一步。",
  });
}

// F-14：变更页的仓库选中态即「全局当前仓库」——同步进 repository store，
// 供 SideNav 直达的 Git 视图（无 route.query.repo）回落使用。
watch(
  selectedRepoPath,
  (path) => {
    if (path) repoStore.setCurrentRepoPath(path);
  },
  { immediate: true },
);

// --- DataTable columns ---

const dryRunColumns = [
  { title: "仓库", key: "repoName", minWidth: 140 },
  {
    title: "预测",
    key: "category",
    width: 110,
    render: (row: DryRunItem) =>
      h(NTag, { size: "small", type: dryRunTagType(row.category) }, () => dryRunLabel(row.category)),
  },
  {
    title: "前/后",
    key: "ahead",
    width: 90,
    render: (row: DryRunItem) => `+${row.ahead} / -${row.behind}`,
  },
  { title: "说明", key: "detail", minWidth: 220 },
];

const wsStashColumns = [
  { type: "expand" as const, renderExpand: (row: WorkspaceStashSummary) => {
    const items = wsStashDialog.value.items[row.id] ?? [];
    return h("div", { class: "ws-stash-items" }, items.map((item) =>
      h("div", { class: "ws-stash-item", key: item.repoPath }, [
        h("span", { class: "ws-stash-item-repo" }, repoNameOf(item.repoPath)),
        h(NTag, { size: "small", bordered: false }, () => item.branch),
        h("span", { class: "ws-stash-item-oid" }, item.stashOid.slice(0, 8)),
      ]),
    ));
  }},
  { title: "名称", key: "name", minWidth: 150 },
  { title: "备注", key: "message", minWidth: 160, render: (row: WorkspaceStashSummary) => row.message || "—" },
  { title: "仓库数", key: "repoCount", width: 80, align: "center" as const },
  { title: "创建时间", key: "createdAt", minWidth: 170 },
  {
    title: "操作",
    key: "actions",
    width: 150,
    fixed: "right" as const,
    render: (row: WorkspaceStashSummary) =>
      h("div", { style: "display: flex; gap: 4px" }, [
        h(NButton, { size: "small", text: true, type: "primary", onClick: () => openWsStashRestore(row) }, () => "恢复"),
        h(NButton, { size: "small", text: true, type: "error", onClick: () => removeWsStash(row) }, () => "删除"),
      ]),
  },
];

const wsStashCheckColumns = [
  { title: "仓库", key: "repoName", minWidth: 140 },
  {
    title: "记录分支",
    key: "branch",
    width: 110,
    render: (row: WorkspaceStashCheckItem) => h(NTag, { size: "small", bordered: false }, () => row.branch),
  },
  {
    title: "当前分支",
    key: "currentBranch",
    width: 110,
    render: (row: WorkspaceStashCheckItem) => h(NTag, { size: "small", bordered: false }, () => row.currentBranch ?? "—"),
  },
  {
    title: "校验",
    key: "status",
    width: 100,
    render: (row: WorkspaceStashCheckItem) =>
      h(NTag, { size: "small", type: wsStashCheckTagType(row.status) }, () => wsStashCheckLabel(row.status)),
  },
  { title: "说明", key: "detail", minWidth: 180 },
];

const pushColumns = [
  { type: "selection" as const, width: 40 },
  {
    title: "仓库",
    key: "repoName",
    minWidth: 220,
    render: (row: RepoChanges) =>
      h("div", { class: "push-repo-cell" }, [
        h("span", { class: "push-repo-name" }, row.repoName),
        // 工作区根本身是仓库时 relativePath 为空，显示 "." 表示工作区根。
        h("span", { class: "push-repo-rel" }, row.relativePath || "."),
      ]),
  },
  {
    title: "分支",
    key: "branch",
    width: 180,
    render: (row: RepoChanges) => h(NTag, { size: "small", bordered: false }, () => row.branch),
  },
  {
    title: "待推送",
    key: "ahead",
    width: 120,
    align: "center" as const,
    render: (row: RepoChanges) =>
      row.ahead > 0
        ? h(NTag, { type: "warning", size: "small" }, () => `↑${row.ahead} 个提交`)
        : h("span", { class: "text-muted" }, "已同步"),
  },
  {
    title: "变更",
    key: "changes",
    width: 70,
    align: "center" as const,
    render: (row: RepoChanges) => String(row.changes.length),
  },
];

onMounted(async () => {
  await workspaceStore.loadWorkspaces();
  if (workspaceStore.currentWorkspace) {
    await loadChanges();
    await startFileWatcher();
    await applyRoutePrefill();
  }

  // Listen for scan progress events
  unlistenScan = await listen<ScanProgress>("scan_progress", (event) => {
    scanProgress.value = event.payload;
  });
});

/** Prefill from Dashboard quick actions (T-18). */
async function applyRoutePrefill() {
  const selector =
    typeof route.query.selector === "string" ? route.query.selector : "";
  const action = typeof route.query.action === "string" ? route.query.action : "";
  if (!selector && !action) return;
  try {
    if (selector) {
      selectorQuery.value = selector;
      selectorPaths.value = await selectRepos(currentWorkspaceId.value!, selector);
      commitPanelOpen.value = true;
    }
    switch (action) {
      case "fetch": {
        const paths = changes.value.map((c) => c.repoPath);
        if (paths.length === 0) break;
        actionLoading.value = true;
        try {
          const ids = await batchFetch(paths);
          message.success(`已提交 ${ids.length} 个 fetch 任务`);
          await loadChanges();
        } finally {
          actionLoading.value = false;
        }
        break;
      }
      case "pull":
        runDryRun("pull");
        break;
      case "push":
        openPushDialog();
        break;
      case "commit":
        commitPanelOpen.value = true;
        changeTreeRef.value?.expandAll();
        break;
      case "branch-create":
        openBranchOp("create");
        break;
      // T-31：Sync = Fetch 全部 → Pull Clean 预演（推送保持显式操作）。
      case "sync": {
        const paths = changes.value.map((c) => c.repoPath);
        if (paths.length === 0) break;
        actionLoading.value = true;
        try {
          await batchFetch(paths);
          message.success(`已提交 ${paths.length} 个 fetch 任务`);
          await loadChanges();
        } finally {
          actionLoading.value = false;
        }
        runDryRun("pull");
        break;
      }
    }
  } catch (e) {
    message.error("预填快捷操作失败: " + errMsg(e));
  } finally {
    router.replace({ query: {} });
  }
}

onUnmounted(() => {
  if (unlistenScan) {
    unlistenScan();
    unlistenScan = null;
  }
});

async function loadChanges() {
  if (!currentWorkspaceId.value) return;
  changesLoading.value = true;
  try {
    changes.value = await getWorkspaceChanges(currentWorkspaceId.value);
  } catch (e) {
    message.error("加载变更失败: " + errMsg(e));
  } finally {
    changesLoading.value = false;
  }
}

// D-05：监听全局工作区变化
watch(() => workspaceStore.currentWorkspace, () => {
  loadChanges();
  selectedDiff.value = null;
});

function onTreeSelection(selection: TreeSelection) {
  treeSelection.value = selection;
  // D-15：单仓库选中时加载提交图
  if (selection.repoPaths.length === 1) {
    loadGraphCommits(selection.repoPaths[0]);
  } else {
    graphCommits.value = [];
  }
}

function repoNameOf(repoPath: string): string {
  const repo = changes.value.find((c) => c.repoPath === repoPath);
  return repo?.repoName ?? repoPath.split(/[\\/]/).pop() ?? repoPath;
}

/** Double-click a file node: show its change content on the right. */
async function onFileDblClick(node: ChangeNode) {
  if (!node.repoPath || !node.relPath) return;
  diffLoading.value = true;
  try {
    const normRel = node.relPath.replace(/^\.?\/+/, "").replace(/\\/g, "/");
    let files = await getDiff(node.repoPath);
    let match = files.find((f) => norm(f.newPath) === normRel || norm(f.oldPath) === normRel);
    // Untracked / newly added files may be absent from the default diff;
    // fall back to unstaged diff (index → workdir, includes untracked).
    if (!match) {
      files = await getUnstagedDiff(node.repoPath);
      match = files.find((f) => norm(f.newPath) === normRel || norm(f.oldPath) === normRel);
    }
    // Final fallback: read the file directly and return as synthetic diff.
    // Covers libgit2 edge cases where diff APIs omit untracked files.
    if (!match && node.status === "untracked") {
      try {
        files = await readFileAsDiff(node.repoPath, node.relPath);
        match = files.find((f) => norm(f.newPath) === normRel || norm(f.oldPath) === normRel);
      } catch {
        // readFileAsDiff failed (binary file, permission, etc.) — fall through
      }
    }
    if (match) {
      selectedDiff.value = {
        repoPath: node.repoPath,
        relPath: node.relPath,
        file: match,
      };
    } else {
      selectedDiff.value = null;
      message.info("该文件没有可展示的变更内容");
    }
  } catch (e) {
    message.error("加载变更内容失败: " + errMsg(e));
  } finally {
    diffLoading.value = false;
  }
}

/** 归一化路径：统一正斜杠、去前导 ./ 和尾部 /。 */
function norm(p: string | undefined): string {
  return (p ?? "").replace(/^\.?\/+/, "").replace(/\\/g, "/").replace(/\/+$/, "");
}

function statusText(status: string): string {
  const map: Record<string, string> = {
    untracked: "未跟踪",
    modified: "已修改",
    deleted: "已删除",
    added: "新增",
    renamed: "重命名",
    typechange: "类型变更",
  };
  return map[status] ?? status;
}

async function handleScan() {
  if (!currentWorkspaceId.value) return;
  scanProgress.value = null;
  try {
    await repoStore.scanRepositories(currentWorkspaceId.value);
    message.success(`发现 ${repoStore.totalCount} 个仓库`);
    await loadChanges();
    await startFileWatcher();
  } catch (e) {
    message.error("扫描失败: " + errMsg(e));
  } finally {
    scanProgress.value = null;
  }
}

async function handleAdd() {
  const requests: AddRequest[] = [];
  for (const [repoPath, files] of treeSelection.value.filesByRepo.entries()) {
    requests.push({
      repoPath,
      repoName: repoNameOf(repoPath),
      files,
    });
  }
  if (requests.length === 0) {
    message.warning("请先勾选要暂存的文件");
    return;
  }
  actionLoading.value = true;
  try {
    await batchAdd(requests);
    message.success(`已暂存 ${requests.length} 个仓库的文件`);
    await loadChanges();
  } catch (e) {
    message.error("暂存失败: " + errMsg(e));
  } finally {
    actionLoading.value = false;
  }
}

/** Revert working-tree changes for the checked files (with confirmation). */
async function handleRestore() {
  const requests: RestoreRequest[] = [];
  for (const [repoPath, files] of treeSelection.value.filesByRepo.entries()) {
    requests.push({
      repoPath,
      repoName: repoNameOf(repoPath),
      files,
    });
  }
  if (requests.length === 0) {
    message.warning("请先勾选要回退的文件");
    return;
  }
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.warning({
        title: "批量回退",
        content: `确定要回退 ${selectedFileCount.value} 个文件的工作区修改吗？\n已跟踪文件将恢复到 Git 已提交版本（同时取消暂存），未跟踪/新增文件将被删除。`,
        positiveText: "回退",
        negativeText: "取消",
        onPositiveClick: () => resolve(),
        onNegativeClick: () => reject("cancel"),
        onClose: () => reject("cancel"),
      });
    });
  } catch {
    return;
  }
  actionLoading.value = true;
  try {
    await batchRestore(requests);
    message.success(`已回退 ${requests.length} 个仓库的文件`);
    await loadChanges();
  } catch (e) {
    message.error("回退失败: " + errMsg(e));
  } finally {
    actionLoading.value = false;
  }
}

async function handleCommit(forcePush = false) {
  const amend = commitForm.value.amend;
  const msg = commitForm.value.message.trim();
  if (!msg && !amend) {
    message.warning("请输入提交信息（Amend 可留空 = --no-edit）");
    return;
  }
  const commits: CommitRequest[] = [];
  for (const [repoPath, files] of treeSelection.value.filesByRepo.entries()) {
    commits.push({
      repoPath,
      repoName: repoNameOf(repoPath),
      message: msg,
      files,
      amend,
      noEdit: amend && !msg,
      // T-31：Ctrl+Shift+Enter 以事件参数强制推送，不改动表单状态。
      thenPush: forcePush || commitForm.value.thenPush,
    });
  }
  if (commits.length === 0) {
    message.warning("请先勾选要提交的文件");
    return;
  }
  actionLoading.value = true;
  try {
    const findings: CommitScanFinding[] = [];
    for (const c of commits) {
      findings.push(...(await scanCommit(c.repoPath, c.files, false)));
    }
    if (findings.length > 0) {
      scanDialog.value = { show: true, findings, pending: commits };
      return;
    }
  } catch (e) {
    message.error("安全检查失败: " + errMsg(e));
    return;
  } finally {
    actionLoading.value = false;
  }
  await submitCommits(commits);
}

/** Resubmit the pending commits with the safety override (T-11 可放行). */
async function commitWithOverride() {
  const commits = scanDialog.value.pending.map((c) => ({
    ...c,
    allowUnsafe: true,
  }));
  scanDialog.value.show = false;
  await submitCommits(commits);
}

async function submitCommits(commits: CommitRequest[]) {
  actionLoading.value = true;
  try {
    const taskIds = await batchCommit(commits);
    message.success(`已提交 ${taskIds.length} 个 commit 任务`);
    commitForm.value.message = "";
    await loadChanges();
  } catch (e) {
    message.error("提交失败: " + errMsg(e));
  } finally {
    actionLoading.value = false;
  }
}

const identitySourceLabel = computed(() => {
  switch (identityDialog.value.current?.source) {
    case "repo":
      return "本仓库配置";
    case "group":
      return "分组配置";
    case "mixed":
      return "仓库/分组混合";
    default:
      return "";
  }
});

/** Open the commit-identity dialog for the first selected repo (T-11 §54). */
async function openIdentityDialog() {
  const repo = selectedRepoPath.value;
  if (!repo) return;
  const d = identityDialog.value;
  d.show = true;
  d.scope = "repo";
  d.name = "";
  d.email = "";
  const found = repoStore.repositories.find(
    (r) => r.repository.path === repo,
  );
  d.groupId = found?.repository.groupId ?? null;
  try {
    d.current = await getCommitIdentity(repo);
  } catch {
    d.current = null;
  }
}

/** Save (or clear, when both fields are empty) the identity override. */
async function saveIdentity() {
  const d = identityDialog.value;
  const repo = selectedRepoPath.value;
  if (!repo) return;
  const name = d.name.trim() || null;
  const email = d.email.trim() || null;
  if ((name === null) !== (email === null)) {
    message.warning("Name 和 Email 需同时填写或同时留空");
    return;
  }
  d.saving = true;
  try {
    if (d.scope === "group" && d.groupId != null) {
      await setGroupIdentity(d.groupId, name, email);
    } else {
      await setRepoIdentity(repo, name, email);
    }
    message.success(name ? "已保存提交身份" : "已清除自定义身份（恢复默认）");
    d.show = false;
  } catch (e) {
    message.error("保存失败: " + errMsg(e));
  } finally {
    d.saving = false;
  }
}

/** Selector query (debounced) against the in-memory repo facets (T-20). */
let selectorTimer: number | undefined;
watch(selectorQuery, (q) => {
  for (const chip of quickChips.value) {
    chip.active = q.split(/\s+/).includes(chip.token);
  }
  window.clearTimeout(selectorTimer);
  selectorTimer = window.setTimeout(async () => {
    const query = q.trim();
    if (!query || !currentWorkspaceId.value) {
      selectorPaths.value = [];
      return;
    }
    try {
      selectorPaths.value = await selectRepos(currentWorkspaceId.value, query);
    } catch (e) {
      message.error("选择器查询失败: " + errMsg(e));
    }
  }, 300);
});


function toggleChip(
  chip: { token: string; active: boolean },
  checked: boolean,
) {
  chip.active = checked;
  const tokens = selectorQuery.value
    .split(/\s+/)
    .filter(Boolean)
    .filter((t) => t !== chip.token);
  if (checked) tokens.push(chip.token);
  selectorQuery.value = tokens.join(" ");
}


function batchTargetRepos(): string[] {
  if (selectorActive.value) return selectorPaths.value;
  if (treeSelection.value.repoPaths.length > 0) return treeSelection.value.repoPaths;
  return changes.value.map((c) => c.repoPath);
}


const branchOpTitle = computed(() =>
  ({
    checkout: "Checkout All（批量检出分支）",
    create: "Create Branch All（批量建分支）",
    delete: "Delete Branch All（批量删分支）",
  })[branchOpDialog.value.op],
);


const branchOpActionLabel = computed(() =>
  ({ checkout: "检出", create: "创建", delete: "删除" })[
    branchOpDialog.value.op
  ],
);


function openBranchOp(op: "checkout" | "create" | "delete") {
  const targets = batchTargetRepos();
  if (targets.length === 0) {
    message.warning("没有目标仓库（先用选择器或勾选）");
    return;
  }
  branchOpTargets.value = targets;
  branchOpDialog.value = { show: true, loading: false, op, name: "", force: false };
}


async function handleBranchOp() {
  const d = branchOpDialog.value;
  if (!d.name.trim()) {
    message.warning("请输入分支名");
    return;
  }
  if (d.op === "checkout") {
    // R-21 §49：批量检出前检查运行中 Runtime 应用（Stop & Switch / Cancel）。
    if (!(await guardRuntimeRunning(dialog, message))) return;
  }
  if (d.op === "delete") {
    try {
      await new Promise<void>((resolve, reject) => {
        dialog.error({
          title: "危险操作确认",
          content: `确定从 ${branchOpTargets.value.length} 个仓库删除分支「${d.name}」吗？`,
          positiveText: "确认删除",
          negativeText: "取消",
          onPositiveClick: () => resolve(),
          onNegativeClick: () => reject("cancel"),
          onClose: () => reject("cancel"),
        });
      });
    } catch {
      return;
    }
  }
  d.loading = true;
  try {
    const ids = await batchBranchOp(branchOpTargets.value, d.op, d.name.trim(), d.force);
    message.success(`已提交 ${ids.length} 个分支任务`);
    d.show = false;
  } catch (e) {
    message.error("操作失败: " + errMsg(e));
  } finally {
    d.loading = false;
  }
}


const dryRunActionable = computed(() =>
  dryRunDialog.value.items.filter((i) => i.category === "fast_forward"),
);


async function runDryRun(op: "pull" | "push") {
  const targets = batchTargetRepos();
  if (targets.length === 0) {
    message.warning("没有目标仓库");
    return;
  }
  dryRunDialog.value = { show: true, loading: true, op, items: [] };
  try {
    dryRunDialog.value.items = await batchDryRun(targets, op);
  } catch (e) {
    message.error("预演失败: " + errMsg(e));
    dryRunDialog.value.show = false;
  } finally {
    dryRunDialog.value.loading = false;
  }
}


async function executeDryRun() {
  const paths = dryRunActionable.value.map((i) => i.repoPath);
  const op = dryRunDialog.value.op;
  dryRunDialog.value.show = false;
  try {
    if (op === "pull") {
      await batchPull(paths);
    } else {
      await batchPush(paths);
    }
    message.success(`已提交 ${paths.length} 个任务`);
  } catch (e) {
    message.error("执行失败: " + errMsg(e));
  }
}


function dryRunLabel(c: string): string {
  return (
    {
      up_to_date: "已同步",
      fast_forward: "可快进",
      diverged: "分叉",
      conflict: "预计冲突",
      no_upstream: "无上游",
      error: "错误",
    } as Record<string, string>
  )[c] ?? c;
}


function dryRunTagType(c: string): "success" | "warning" | "error" | "info" {
  return (
    {
      up_to_date: "info",
      fast_forward: "success",
      diverged: "warning",
      conflict: "error",
      no_upstream: "info",
      error: "error",
    } as const
  )[c] ?? "info";
}

// --- Workspace Stash (T-21) ---

const wsStashTargetCount = computed(() => batchTargetRepos().length);

const wsStashSaveSummary = computed(() => {
  const r = wsStashDialog.value.lastSave;
  if (!r) return "";
  const stashed = r.items.filter((i) => i.status === "stashed").length;
  const skipped = r.items.filter((i) => i.status === "skipped_clean").length;
  const failed = r.items.filter((i) => i.status === "failed").length;
  const base = `${r.name}：已暂存 ${stashed} 个仓库，跳过干净仓库 ${skipped} 个`;
  const failText = failed > 0 ? `，失败 ${failed} 个` : "";
  return r.id != null ? `${base}${failText}` : `没有可暂存的变更（${base}${failText}），未生成记录`;
});

function openWsStashDialog() {
  if (!currentWorkspaceId.value) return;
  wsStashDialog.value.show = true;
  wsStashDialog.value.lastSave = null;
  loadWsStashes();
}

async function loadWsStashes() {
  if (!currentWorkspaceId.value) return;
  wsStashDialog.value.loading = true;
  try {
    wsStashDialog.value.list = await listWorkspaceStashes(currentWorkspaceId.value);
    wsStashDialog.value.items = {};
  } catch (e) {
    message.error("加载 Workspace Stash 记录失败: " + errMsg(e));
  } finally {
    wsStashDialog.value.loading = false;
  }
}

async function saveWsStash() {
  const targets = batchTargetRepos();
  if (targets.length === 0) {
    message.warning("没有目标仓库（先用选择器或勾选）");
    return;
  }
  const d = wsStashDialog.value;
  d.saving = true;
  try {
    const result = await saveWorkspaceStash(
      currentWorkspaceId.value!,
      targets,
      d.message.trim() || undefined,
      d.includeUntracked,
    );
    d.lastSave = result;
    const failed = result.items.filter((i) => i.status === "failed");
    if (failed.length > 0) {
      dialog.warning({
        title: "部分仓库暂存失败",
        content: failed.map((f) => `${f.repoName}：${f.detail}`).join("\n"),
      });
    }
    if (result.id != null) {
      d.message = "";
      await loadWsStashes();
    }
    await loadChanges();
  } catch (e) {
    message.error("暂存失败: " + errMsg(e));
  } finally {
    d.saving = false;
  }
}

/** Lazy-load the per-repo items when a record row is expanded. */
async function onWsStashExpand(keys: number[]) {
  expandedWsStashKeys.value = keys;
  for (const id of keys) {
    if (!wsStashDialog.value.items[id]) {
      try {
        wsStashDialog.value.items[id] = await getWorkspaceStashItems(id);
      } catch (e) {
        message.error("加载记录明细失败: " + errMsg(e));
      }
    }
  }
}

async function openWsStashRestore(row: WorkspaceStashSummary) {
  wsStashCheck.value = {
    show: true,
    loading: true,
    restoring: false,
    id: row.id,
    name: row.name,
    allowMismatch: false,
    items: [],
  };
  try {
    wsStashCheck.value.items = await checkWorkspaceStash(row.id);
  } catch (e) {
    message.error("恢复前校验失败: " + errMsg(e));
    wsStashCheck.value.show = false;
  } finally {
    wsStashCheck.value.loading = false;
  }
}

const wsStashApplicableCount = computed(() =>
  wsStashCheck.value.items.filter(
    (i) =>
      i.status === "ok" ||
      (i.status === "branch_mismatch" && wsStashCheck.value.allowMismatch),
  ).length,
);

async function confirmWsStashRestore() {
  const c = wsStashCheck.value;
  c.restoring = true;
  try {
    const outcomes = await restoreWorkspaceStash(c.id, c.allowMismatch);
    const applied = outcomes.filter((o) => o.status === "applied").length;
    const skipped = outcomes.filter((o) => o.status === "skipped").length;
    const failed = outcomes.filter((o) => o.status === "failed");
    c.show = false;
    if (failed.length > 0) {
      dialog.warning({
        title: `已恢复 ${applied} 个仓库，${failed.length} 个失败`,
        content: failed.map((f) => `${f.repoName}：${f.detail}`).join("\n"),
      });
    } else {
      message.success(
        `已恢复 ${applied} 个仓库${skipped > 0 ? `，跳过 ${skipped} 个` : ""}`,
      );
    }
    await loadChanges();
  } catch (e) {
    message.error("恢复失败: " + errMsg(e));
  } finally {
    c.restoring = false;
  }
}

async function removeWsStash(row: WorkspaceStashSummary) {
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.warning({
        title: "删除 Workspace Stash 记录",
        content: `确定删除记录「${row.name}」吗？仅删除关联记录，各仓库的 stash 条目仍保留在各自的 stash 栈中。`,
        positiveText: "删除",
        negativeText: "取消",
        onPositiveClick: () => resolve(),
        onNegativeClick: () => reject("cancel"),
        onClose: () => reject("cancel"),
      });
    });
  } catch {
    return;
  }
  try {
    await deleteWorkspaceStash(row.id);
    message.success("已删除记录");
    await loadWsStashes();
  } catch (e) {
    message.error("删除失败: " + errMsg(e));
  }
}

function wsStashCheckLabel(s: string): string {
  return (
    {
      ok: "可恢复",
      branch_mismatch: "分支不一致",
      stash_missing: "stash 缺失",
      repo_missing: "仓库缺失",
      error: "错误",
    } as Record<string, string>
  )[s] ?? s;
}

function wsStashCheckTagType(s: string): "success" | "warning" | "error" | "info" {
  return (
    {
      ok: "success",
      branch_mismatch: "warning",
      stash_missing: "error",
      repo_missing: "error",
      error: "error",
    } as const
  )[s] ?? "info";
}

async function handleFetch(paths?: string[]) {
  const targets = paths ?? batchTargetRepos();
  if (targets.length === 0) {
    message.warning("没有可操作的仓库");
    return;
  }
  actionLoading.value = true;
  try {
    const taskIds = await batchFetch(targets);
    message.success(`已提交 ${taskIds.length} 个 fetch 任务`);
    await loadChanges();
  } catch (e) {
    message.error("fetch 失败: " + errMsg(e));
  } finally {
    actionLoading.value = false;
  }
}

async function handlePull(paths?: string[]) {
  const targets = paths ?? batchTargetRepos();
  if (targets.length === 0) {
    message.warning("没有可操作的仓库");
    return;
  }
  actionLoading.value = true;
  try {
    const taskIds = await batchPull(targets);
    message.success(`已提交 ${taskIds.length} 个 pull 任务`);
    await loadChanges();
  } catch (e) {
    message.error("pull 失败: " + errMsg(e));
  } finally {
    actionLoading.value = false;
  }
}

function openPushDialog(paths?: string[]) {
  const defaultSelected = paths ?? batchTargetRepos();
  pushSelection.value = defaultSelected;
  showPushDialog.value = true;
}

function onPushSelectionChange(keys: string[]) {
  pushSelection.value = keys;
}

async function doPush() {
  if (pushSelection.value.length === 0) {
    message.warning("请选择要 Push 的仓库");
    return;
  }
  actionLoading.value = true;
  try {
    const taskIds = await batchPush(pushSelection.value);
    message.success(`已提交 ${taskIds.length} 个 push 任务`);
    showPushDialog.value = false;
    await loadChanges();
  } catch (e) {
    message.error("push 失败: " + errMsg(e));
  } finally {
    actionLoading.value = false;
  }
}

function expandAll() {
  changeTreeRef.value?.expandAll();
}

function collapseAll() {
  changeTreeRef.value?.collapseAll();
}

async function startFileWatcher() {
  const paths = changes.value.map((c) => c.repoPath);
  if (paths.length === 0) return;
  try {
    await startWatcher(paths);
    watcherActive.value = true;
  } catch (e) {
    console.error("Failed to start watcher:", e);
  }
}

async function toggleWatcher() {
  if (watcherActive.value) {
    try {
      await stopWatcher();
      watcherActive.value = false;
      message.info("文件监听已停止");
    } catch (e) {
      message.error("停止监听失败: " + errMsg(e));
    }
  } else {
    await startFileWatcher();
    if (watcherActive.value) {
      message.success("文件监听已启动");
    }
  }
}

function viewDiff(repoPath: string) {
  router.push({ name: "diff-viewer", query: { repo: repoPath } });
}

function viewGraph(repoPath: string) {
  router.push({ name: "git-graph", query: { repo: repoPath } });
}

function viewBranches(repoPath: string) {
  router.push({ name: "branch-manager", query: { repo: repoPath } });
}

function viewStash(repoPath: string) {
  router.push({ name: "stash-manager", query: { repo: repoPath } });
}

function viewWorktrees(repo: string) {
  router.push({ name: "worktree-manager", query: { repo } });
}

function viewConflicts() {
  const ws = workspaceStore.currentWorkspace;
  if (!ws) return;
  router.push({
    name: "conflict-resolver",
    query: { workspace: String(ws.id), name: ws.name },
  });
}
</script>

<style scoped>
.repository-list {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: var(--gw-space-3) var(--gw-space-4);
  gap: var(--gw-space-2);
}

.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.toolbar-left {
  display: flex;
  gap: var(--gw-space-2);
  align-items: center;
}

.toolbar-right {
  display: flex;
  gap: var(--gw-space-2);
  align-items: center;
}

.main-body {
  display: flex;
  flex: 1;
  overflow: hidden;
  gap: var(--gw-space-3);
  min-height: 0;
}

.tree-pane {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.resize-handle {
  width: 5px;
  flex-shrink: 0;
  cursor: col-resize;
  background: transparent;
  border-radius: 2px;
  margin: 0 -1px;
  transition: background 0.15s;
}

.resize-handle:hover {
  background: var(--gw-accent);
}

/* D-15：提交图面板 */
.graph-pane {
  width: 320px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--gw-border);
  border-radius: var(--gw-radius-md);
  overflow: hidden;
  background: var(--gw-bg-panel);
}

.graph-pane-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--gw-space-2) var(--gw-space-3);
  border-bottom: 1px solid var(--gw-border);
  background: var(--gw-bg-panel);
}

.graph-pane-title {
  font-size: var(--gw-text-sm);
  font-weight: 600;
  color: var(--gw-text);
  font-family: var(--gw-font-mono);
}

.graph-pane-spin {
  flex: 1;
  min-height: 0;
}

.diff-pane {
  width: 46%;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--gw-border);
  border-radius: 4px;
  overflow: hidden;
  min-height: 0;
}

.diff-pane-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 8px;
  border-bottom: 1px solid var(--gw-border);
  background: var(--gw-bg-hover);
}

.diff-pane-title {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  min-width: 0;
}

.diff-repo {
  font-weight: 600;
  color: var(--gw-accent);
  font-size: 13px;
  flex-shrink: 0;
}

.diff-file {
  font-family: var(--gw-font-mono);
  font-size: 12px;
  color: var(--gw-text-dim);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.diff-pane-body {
  flex: 1;
  min-height: 0;
  padding: 4px 0;
}

/* F-09b：n-spin 作为 diff 内容容器参与 flex 布局。 */
.diff-pane-spin {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

/* 同 F-20 树区模式：高度链经 .n-spin-content 打通到 .diff-pane-body，
   否则大文档（如未跟踪文件全文 diff）会把 .virtual-list 撑出可视区——
   竖向无滚动，横向滚动条被 .diff-pane 的 overflow:hidden 裁掉。 */
.diff-pane-spin :deep(.n-spin-content) {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.stats-bar {
  font-size: 13px;
  color: var(--gw-text-dim);
  padding: 4px 0;
  display: flex;
  align-items: center;
  flex-wrap: wrap;
}

.tree-controls {
  margin-left: auto;
  display: inline-flex;
  gap: 4px;
}

.selected-info {
  color: var(--gw-accent);
  font-weight: 500;
}

.scan-progress {
  margin-bottom: 8px;
  padding: 0 4px;
}

.tree-container {
  height: 100%;
  overflow: hidden;
  border: 1px solid var(--gw-border);
  border-radius: 4px;
  min-height: 0;
}

/* F-20：树区 n-spin 参与 .tree-pane 的 flex 布局，高度链经 .n-spin-content
   打通到 .tree-container（同 F-18 / F-09b 模式），空状态得以竖直居中。 */
.tree-spin {
  flex: 1;
  min-height: 0;
}

.tree-spin :deep(.n-spin-content) {
  height: 100%;
}

.commit-panel {
  border: 1px solid var(--gw-border);
  border-radius: 4px;
  background: var(--gw-bg-hover);
  flex-shrink: 0;
}

.commit-panel-header {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  padding: 2px 8px;
}

.commit-panel-hint {
  font-size: 12px;
  color: var(--gw-text-dim);
}

.commit-panel-body {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
  padding: 0 12px 10px 12px;
}

.ops-row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
}

.commit-row {
  display: flex;
  align-items: flex-start;
  gap: var(--gw-space-3);
}

.commit-input {
  flex: 1;
  min-width: 0;
}

.commit-scope {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 12px;
  max-height: 64px;
  overflow-y: auto;
  max-width: 40%;
}

.scope-repo {
  font-weight: 600;
  color: var(--gw-accent);
  font-size: 12px;
}

.scope-count {
  color: var(--gw-text-dim);
  font-size: 12px;
}

.push-repo-cell {
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.push-repo-name {
  font-weight: 600;
  font-size: 13px;
}

.push-repo-rel {
  font-family: var(--gw-font-mono);
  font-size: 12px;
  color: var(--gw-text-dim);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.separator {
  margin: 0 8px;
  color: var(--gw-border);
}

.empty-state {
  display: flex;
  justify-content: center;
  align-items: center;
  height: 100%;
}

.app-footer {
  text-align: right;
  font-size: 12px;
  color: var(--gw-text-dim);
  padding: 2px 4px 0;
  flex-shrink: 0;
}

.commit-options {
  display: flex;
  align-items: center;
  gap: var(--gw-space-4);
  margin-top: 6px;
}

.identity-current {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
}

.scan-finding-list {
  margin: 12px 0 0;
  padding: 0;
  list-style: none;
  max-height: 300px;
  overflow-y: auto;
}

.scan-finding-list li {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  padding: 6px 0;
  border-bottom: 1px solid var(--gw-border);
  font-size: 13px;
}

.scan-path {
  font-family: var(--gw-font-mono);
  color: var(--gw-text);
}

.scan-detail {
  color: var(--gw-text-dim);
  font-size: 12px;
}

.batch-row {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 8px;
  flex-wrap: wrap;
}

.selector-input {
  max-width: 420px;
}

.selector-count {
  font-size: 12px;
  color: var(--gw-text-dim);
}

.selector-count.is-empty {
  color: var(--gw-warning);
}

.affected-repo-list {
  margin: 12px 0 0;
  padding-left: 18px;
  max-height: 200px;
  overflow-y: auto;
  font-size: 13px;
}

.ws-stash-save-row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
  margin-bottom: 10px;
}

.ws-stash-save-result {
  margin-bottom: 10px;
}

.ws-stash-items {
  padding: 4px 16px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.ws-stash-item {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  font-size: 13px;
}

.ws-stash-item-repo {
  font-weight: 600;
  color: var(--gw-accent);
}

.ws-stash-item-oid {
  font-family: var(--gw-font-mono);
  color: var(--gw-text-dim);
  font-size: 12px;
}

.ws-stash-mismatch-allow {
  margin-top: 10px;
}
</style>
