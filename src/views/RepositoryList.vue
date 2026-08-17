<template>
  <div class="repository-list">
    <!-- Top toolbar -->
    <div class="toolbar">
      <div class="toolbar-left">
        <el-select
          v-model="selectedWorkspaceId"
          placeholder="选择工作区"
          style="width: 200px"
          @change="onWorkspaceChange"
        >
          <el-option
            v-for="ws in workspaceStore.workspaces"
            :key="ws.id"
            :label="ws.name"
            :value="ws.id"
          />
        </el-select>
        <el-button @click="showAddWorkspace = true">
          <el-icon><Plus /></el-icon>
          添加工作区
        </el-button>
        <el-button
          type="primary"
          :loading="repoStore.scanning"
          :disabled="!selectedWorkspaceId"
          @click="handleScan"
        >
          <el-icon><Refresh /></el-icon>
          扫描仓库
        </el-button>
        <el-button
          :disabled="!selectedWorkspaceId"
          @click="toggleWatcher"
        >
          <el-icon><Monitor /></el-icon>
          {{ watcherActive ? "停止监听" : "启动监听" }}
        </el-button>
      </div>
      <div class="toolbar-right">
        <el-button
          v-if="taskStore.tasks.length > 0"
          @click="taskStore.togglePanel()"
        >
          <el-icon><Bell /></el-icon>
          任务 ({{ taskStore.tasks.length }})
        </el-button>
        <el-button @click="router.push({ name: 'health' })">
          <el-icon><Odometer /></el-icon>
          健康检查
        </el-button>
        <el-button @click="router.push({ name: 'change-sets' })">
          <el-icon><Collection /></el-icon>
          Change Set
        </el-button>
        <el-button @click="router.push({ name: 'pipeline' })">
          <el-icon><Connection /></el-icon>
          Pipeline
        </el-button>
        <el-button @click="router.push({ name: 'manifest' })">
          <el-icon><Document /></el-icon>
          Manifest
        </el-button>
        <el-button @click="router.push({ name: 'operation-log' })">
          <el-icon><Clock /></el-icon>
          操作日志
        </el-button>
        <el-button @click="showLogManager = true">
          <el-icon><FolderOpened /></el-icon>
          日志
        </el-button>
        <el-input
          v-model="searchQuery"
          placeholder="搜索文件或仓库..."
          style="width: 240px"
          clearable
          :prefix-icon="Search"
        />
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
            <el-button size="small" @click="expandAll">
              <el-icon><Expand /></el-icon>
              展开全部
            </el-button>
            <el-button size="small" @click="collapseAll">
              <el-icon><Fold /></el-icon>
              收起全部
            </el-button>
          </span>
        </div>

        <!-- Scan progress bar -->
        <div v-if="scanProgress" class="scan-progress">
          <el-progress
            :percentage="scanPercentage"
            :status="scanPercentage === 100 ? 'success' : ''"
            :stroke-width="16"
            :text-inside="true"
            :format="() => `扫描状态 ${scanProgress?.current ?? 0}/${scanProgress?.total ?? 0}`"
          />
        </div>

        <!-- Change tree -->
        <div class="tree-container" v-loading="changesLoading">
          <ChangeTree
            ref="changeTreeRef"
            :changes="changes"
            @selection-change="onTreeSelection"
            @file-dblclick="onFileDblClick"
          />
          <div
            v-if="!changesLoading && selectedWorkspaceId && changes.length === 0"
            class="empty-state"
          >
            <el-empty description="未发现任何 Git 仓库">
              <el-button type="primary" @click="handleScan">重新扫描</el-button>
            </el-empty>
          </div>
          <div
            v-else-if="!selectedWorkspaceId"
            class="empty-state"
          >
            <el-empty description="请先添加工作区目录">
              <el-button type="primary" @click="showAddWorkspace = true">
                添加工作区
              </el-button>
            </el-empty>
          </div>
        </div>
      </div>

      <!-- Right: change content of double-clicked file -->
      <div class="resize-handle" @mousedown="startResize"></div>
      <div
        v-if="selectedDiff"
        ref="diffPaneEl"
        class="diff-pane"
        :style="{ width: diffWidth ? diffWidth + 'px' : '46%' }"
        v-loading="diffLoading"
      >
        <div class="diff-pane-header">
          <div class="diff-pane-title">
            <span class="diff-repo">{{ repoNameOf(selectedDiff.repoPath) }}</span>
            <span class="diff-file">{{ selectedDiff.relPath }}</span>
            <el-tag size="small" effect="plain">
              {{ statusText(selectedDiff.file.status) }}
            </el-tag>
          </div>
          <el-button
            size="small"
            text
            :icon="Close"
            @click="selectedDiff = null"
          />
        </div>
        <div class="diff-pane-body">
          <UnifiedDiff v-if="selectedDiff" :file="selectedDiff.file" />
        </div>
      </div>
    </div>

    <!-- Bottom: batch operations panel (always visible, buttons disable instead of hiding) -->
    <div class="commit-panel">
      <div class="commit-panel-header">
        <el-button
          size="small"
          text
          :icon="commitPanelOpen ? ArrowDown : ArrowUp"
          @click="commitPanelOpen = !commitPanelOpen"
        >
          {{ commitPanelOpen ? "收起" : "展开" }}批量操作
        </el-button>
        <span v-if="commitPanelOpen" class="commit-panel-hint">
          {{ selectedFileCount > 0
            ? `已勾选 ${selectedFileCount} 个文件（${selectedRepoCount} 个仓库）`
            : "在左侧勾选变更文件后即可操作" }}
        </span>
      </div>
      <div v-if="commitPanelOpen" class="commit-panel-body">
        <div class="ops-row">
          <el-button-group>
            <el-button
              size="small"
              :loading="actionLoading"
              :disabled="selectedFileCount === 0"
              @click="handleAdd"
            >
              <el-icon><CirclePlus /></el-icon>
              Add（暂存）
            </el-button>
            <el-button
              size="small"
              :loading="actionLoading"
              @click="handlePull"
            >
              <el-icon><Refresh /></el-icon>
              Pull
            </el-button>
            <el-button
              size="small"
              :loading="actionLoading"
              @click="handleFetch"
            >
              <el-icon><Download /></el-icon>
              Fetch
            </el-button>
            <el-button
              size="small"
              :loading="actionLoading"
              @click="openPushDialog"
            >
              <el-icon><Upload /></el-icon>
              Push
            </el-button>
            <el-button
              size="small"
              type="danger"
              plain
              :loading="actionLoading"
              :disabled="selectedFileCount === 0"
              @click="handleRestore"
            >
              <el-icon><RefreshLeft /></el-icon>
              回退
            </el-button>
          </el-button-group>
          <el-button
            size="small"
            :disabled="!selectedRepoPath"
            @click="viewGraph(selectedRepoPath)"
          >
            <el-icon><Share /></el-icon>
            Graph
          </el-button>
          <el-button
            size="small"
            :disabled="!selectedRepoPath"
            @click="viewDiff(selectedRepoPath)"
          >
            <el-icon><View /></el-icon>
            Diff
          </el-button>
          <el-button
            size="small"
            :disabled="!selectedRepoPath"
            @click="viewBranches(selectedRepoPath)"
          >
            <el-icon><Grid /></el-icon>
            分支
          </el-button>
          <el-button
            size="small"
            :disabled="!selectedRepoPath"
            @click="viewStash(selectedRepoPath)"
          >
            <el-icon><Box /></el-icon>
            Stash
          </el-button>
          <el-button
            size="small"
            :disabled="!selectedRepoPath"
            @click="viewWorktrees(selectedRepoPath)"
          >
            <el-icon><Files /></el-icon>
            Worktree
          </el-button>
          <el-button
            size="small"
            :disabled="!selectedWorkspaceId"
            @click="viewConflicts"
          >
            <el-icon><Warning /></el-icon>
            冲突
          </el-button>
        </div>
        <!-- Batch selector + repo-level ops (T-20) -->
        <div class="batch-row">
          <el-input
            v-model="selectorQuery"
            size="small"
            class="selector-input"
            placeholder="选择器：@group:frontend @tag:p0 @status:dirty 或名称关键字"
            clearable
          />
          <el-check-tag
            v-for="chip in quickChips"
            :key="chip.token"
            :checked="chip.active"
            @change="(v: boolean) => toggleChip(chip, v)"
          >
            {{ chip.label }}
          </el-check-tag>
          <span v-if="selectorActive" class="selector-count" :class="{ 'is-empty': selectorPaths.length === 0 }">
            <template v-if="!selectedWorkspaceId">请先选择工作区</template>
            <template v-else>
              匹配 {{ selectorPaths.length }} 个仓库
              <template v-if="selectorPaths.length === 0">（无匹配：检查分组/标签/状态条件是否正确）</template>
            </template>
          </span>
        </div>
        <div class="batch-row">
          <el-button-group>
            <el-button size="small" @click="openBranchOp('checkout')">
              Checkout All
            </el-button>
            <el-button size="small" @click="openBranchOp('create')">
              Create Branch All
            </el-button>
            <el-button
              size="small"
              type="danger"
              plain
              @click="openBranchOp('delete')"
            >
              Delete Branch All
            </el-button>
          </el-button-group>
          <el-button-group>
            <el-button size="small" @click="runDryRun('pull')">
              Pull 预演
            </el-button>
            <el-button size="small" @click="runDryRun('push')">
              Push 预演
            </el-button>
          </el-button-group>
          <el-button size="small" @click="openWsStashDialog">
            <el-icon><Collection /></el-icon>
            Workspace Stash
          </el-button>
        </div>
        <div class="commit-row">
          <div class="commit-input">
            <el-input
              v-model="commitForm.message"
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
          <el-button
            type="primary"
            :loading="actionLoading"
            :disabled="selectedFileCount === 0"
            @click="handleCommit"
          >
            <el-icon><EditPen /></el-icon>
            提交
          </el-button>
        </div>
        <!-- Commit options (T-11) -->
        <div class="commit-options">
          <el-checkbox v-model="commitForm.amend" size="small">
            Amend 上次提交
          </el-checkbox>
          <el-checkbox v-model="commitForm.thenPush" size="small">
            提交后 Push
          </el-checkbox>
          <el-button
            size="small"
            text
            :disabled="!selectedRepoPath"
            @click="openIdentityDialog"
          >
            提交身份
          </el-button>
        </div>
      </div>
    </div>

    <!-- Bulk branch op dialog (T-20) -->
    <el-dialog v-model="branchOpDialog.show" :title="branchOpTitle" width="520px">
      <el-form label-width="80px">
        <el-form-item label="分支名">
          <el-input v-model="branchOpDialog.name" placeholder="分支名" />
        </el-form-item>
        <el-form-item v-if="branchOpDialog.op === 'delete'" label="强制">
          <el-checkbox v-model="branchOpDialog.force">
            强制删除未合并分支
          </el-checkbox>
        </el-form-item>
      </el-form>
      <el-alert
        v-if="branchOpDialog.op === 'delete'"
        type="error"
        :closable="false"
        show-icon
        title="危险操作：将从以下仓库删除分支"
      />
      <el-alert
        v-else
        type="info"
        :closable="false"
        show-icon
        :title="`将作用于 ${branchOpTargets.length} 个仓库`"
      />
      <ul v-if="branchOpDialog.op === 'delete'" class="affected-repo-list">
        <li v-for="r in branchOpTargets" :key="r">
          {{ repoNameOf(r) }}
        </li>
      </ul>
      <template #footer>
        <el-button @click="branchOpDialog.show = false">取消</el-button>
        <el-button
          :type="branchOpDialog.op === 'delete' ? 'danger' : 'primary'"
          :loading="branchOpDialog.loading"
          @click="handleBranchOp"
        >
          {{ branchOpActionLabel }}
        </el-button>
      </template>
    </el-dialog>


    <!-- Dry-run impact report dialog (T-20) -->
    <el-dialog
      v-model="dryRunDialog.show"
      :title="dryRunDialog.op === 'pull' ? 'Pull 预演（不影响任何仓库）' : 'Push 预演（不影响任何仓库）'"
      width="720px"
    >
      <el-table
        :data="dryRunDialog.items"
        v-loading="dryRunDialog.loading"
        max-height="400"
      >
        <el-table-column prop="repoName" label="仓库" min-width="140" />
        <el-table-column label="预测" width="110">
          <template #default="{ row }">
            <el-tag size="small" :type="dryRunTagType(row.category)">
              {{ dryRunLabel(row.category) }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column label="前/后" width="90">
          <template #default="{ row }">+{{ row.ahead }} / -{{ row.behind }}</template>
        </el-table-column>
        <el-table-column prop="detail" label="说明" min-width="220" />
      </el-table>
      <template #footer>
        <el-button @click="dryRunDialog.show = false">关闭</el-button>
        <el-button
          v-if="dryRunActionable.length > 0"
          type="primary"
          @click="executeDryRun"
        >
          对 {{ dryRunActionable.length }} 个可快进仓库执行
          {{ dryRunDialog.op === 'pull' ? 'Pull' : 'Push' }}
        </el-button>
      </template>
    </el-dialog>


    <!-- Workspace Stash dialog (T-21): save the selected repo set as one
         named multi-repo stash, and manage / restore the records. -->
    <el-dialog
      v-model="wsStashDialog.show"
      title="Workspace Stash（多仓库暂存）"
      width="760px"
    >
      <div class="ws-stash-save-row">
        <el-input
          v-model="wsStashDialog.message"
          size="small"
          placeholder="备注信息（可选）"
          style="max-width: 260px"
          clearable
        />
        <el-checkbox v-model="wsStashDialog.includeUntracked" size="small">
          包含未跟踪文件
        </el-checkbox>
        <el-button
          size="small"
          type="primary"
          :loading="wsStashDialog.saving"
          :disabled="wsStashTargetCount === 0"
          @click="saveWsStash"
        >
          暂存选中组（{{ wsStashTargetCount }} 个仓库）
        </el-button>
      </div>
      <el-alert
        v-if="wsStashDialog.lastSave"
        :type="wsStashDialog.lastSave.id != null ? 'success' : 'info'"
        :closable="false"
        show-icon
        class="ws-stash-save-result"
        :title="wsStashSaveSummary"
      />
      <el-table
        :data="wsStashDialog.list"
        v-loading="wsStashDialog.loading"
        max-height="380"
        @expand-change="onWsStashExpand"
      >
        <el-table-column type="expand">
          <template #default="{ row }">
            <div class="ws-stash-items">
              <div
                v-for="item in wsStashDialog.items[row.id] ?? []"
                :key="item.repoPath"
                class="ws-stash-item"
              >
                <span class="ws-stash-item-repo">{{ repoNameOf(item.repoPath) }}</span>
                <el-tag size="small" effect="plain">{{ item.branch }}</el-tag>
                <span class="ws-stash-item-oid">{{ item.stashOid.slice(0, 8) }}</span>
              </div>
            </div>
          </template>
        </el-table-column>
        <el-table-column prop="name" label="名称" min-width="150" />
        <el-table-column label="备注" min-width="160">
          <template #default="{ row }">
            <span>{{ row.message || "—" }}</span>
          </template>
        </el-table-column>
        <el-table-column label="仓库数" width="80" align="center">
          <template #default="{ row }">{{ row.repoCount }}</template>
        </el-table-column>
        <el-table-column prop="createdAt" label="创建时间" min-width="170" />
        <el-table-column label="操作" width="150" fixed="right">
          <template #default="{ row }">
            <el-button size="small" text type="primary" @click="openWsStashRestore(row)">
              恢复
            </el-button>
            <el-button size="small" text type="danger" @click="removeWsStash(row)">
              删除
            </el-button>
          </template>
        </el-table-column>
        <template #empty>暂无 Workspace Stash 记录</template>
      </el-table>
    </el-dialog>

    <!-- Workspace Stash restore: pre-check + §46 Warning confirm (T-21) -->
    <el-dialog
      v-model="wsStashCheck.show"
      :title="`恢复 ${wsStashCheck.name}`"
      width="680px"
    >
      <el-alert
        type="warning"
        :closable="false"
        show-icon
        title="恢复将把各仓库的 stash 应用回工作区（stash 条目保留，可重复恢复）。以下为影响仓库与恢复前校验结果："
      />
      <el-table
        :data="wsStashCheck.items"
        v-loading="wsStashCheck.loading"
        max-height="320"
      >
        <el-table-column prop="repoName" label="仓库" min-width="140" />
        <el-table-column label="记录分支" width="110">
          <template #default="{ row }">
            <el-tag size="small" effect="plain">{{ row.branch }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="当前分支" width="110">
          <template #default="{ row }">
            <el-tag size="small" effect="plain">{{ row.currentBranch ?? "—" }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="校验" width="100">
          <template #default="{ row }">
            <el-tag size="small" :type="wsStashCheckTagType(row.status)">
              {{ wsStashCheckLabel(row.status) }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column prop="detail" label="说明" min-width="180" />
      </el-table>
      <el-checkbox
        v-if="wsStashCheck.items.some((i) => i.status === 'branch_mismatch')"
        v-model="wsStashCheck.allowMismatch"
        size="small"
        class="ws-stash-mismatch-allow"
      >
        允许在分支不一致的仓库上恢复（变更会落到当前分支）
      </el-checkbox>
      <template #footer>
        <el-button @click="wsStashCheck.show = false">取消</el-button>
        <el-button
          type="warning"
          :loading="wsStashCheck.restoring"
          :disabled="wsStashApplicableCount === 0"
          @click="confirmWsStashRestore"
        >
          确认恢复（{{ wsStashApplicableCount }} 个仓库）
        </el-button>
      </template>
    </el-dialog>

    <!-- Commit identity dialog (T-11 §54) -->
    <el-dialog v-model="identityDialog.show" title="提交身份" width="480px">
      <div class="identity-current">
        当前生效：
        <template v-if="identityDialog.current">
          <strong>
            {{ identityDialog.current.name }} &lt;{{ identityDialog.current.email }}&gt;
          </strong>
          <el-tag size="small" style="margin-left: 6px">
            {{ identitySourceLabel }}
          </el-tag>
        </template>
        <el-tag v-else size="small" type="info">
          Git 默认（user.name / user.email）
        </el-tag>
      </div>
      <el-form label-width="70px" style="margin-top: 12px">
        <el-form-item label="作用于">
          <el-radio-group v-model="identityDialog.scope">
            <el-radio value="repo">本仓库</el-radio>
            <el-radio value="group" :disabled="identityDialog.groupId == null">
              本分组
            </el-radio>
          </el-radio-group>
        </el-form-item>
        <el-form-item label="Name">
          <el-input v-model="identityDialog.name" placeholder="留空并保存 = 清除自定义" />
        </el-form-item>
        <el-form-item label="Email">
          <el-input v-model="identityDialog.email" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="identityDialog.show = false">取消</el-button>
        <el-button
          type="primary"
          :loading="identityDialog.saving"
          @click="saveIdentity"
        >
          保存
        </el-button>
      </template>
    </el-dialog>

    <!-- Pre-commit safety findings dialog (T-11 §5) -->
    <el-dialog v-model="scanDialog.show" title="提交安全检查" width="560px">
      <el-alert
        type="warning"
        :closable="false"
        show-icon
        title="发现以下风险项，确认无误后可放行提交："
      />
      <ul class="scan-finding-list">
        <li v-for="(f, i) in scanDialog.findings" :key="i">
          <el-tag
            size="small"
            :type="f.kind === 'forbidden' ? 'danger' : 'warning'"
          >
            {{ f.kind }}
          </el-tag>
          <span class="scan-path">{{ f.path }}</span>
          <span class="scan-detail">{{ f.detail }}</span>
        </li>
      </ul>
      <template #footer>
        <el-button @click="scanDialog.show = false">取消</el-button>
        <el-button type="danger" @click="commitWithOverride">
          仍要提交
        </el-button>
      </template>
    </el-dialog>

    <!-- Add workspace dialog -->
    <WorkspaceManager v-model="showAddWorkspace" @added="onWorkspaceAdded" />
    <LogManager v-model="showLogManager" />

    <div class="app-footer">by mantougg · v0.1.0</div>

    <!-- Push repo picker dialog -->
    <el-dialog v-model="showPushDialog" title="选择要 Push 的仓库" width="680px">
      <el-table
        ref="pushTableRef"
        :data="changes"
        @selection-change="onPushSelectionChange"
        height="360px"
      >
        <el-table-column type="selection" width="40" />
        <el-table-column label="仓库" min-width="220">
          <template #default="{ row }">
            <div class="push-repo-cell">
              <span class="push-repo-name">{{ row.repoName }}</span>
              <span class="push-repo-rel">{{ row.relativePath }}</span>
            </div>
          </template>
        </el-table-column>
        <el-table-column label="分支" width="180">
          <template #default="{ row }">
            <el-tag size="small" effect="plain">{{ row.branch }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="待推送" width="120" align="center">
          <template #default="{ row }">
            <el-tag
              v-if="row.ahead > 0"
              type="warning"
              size="small"
            >
              ↑{{ row.ahead }} 个提交
            </el-tag>
            <span v-else class="text-muted">已同步</span>
          </template>
        </el-table-column>
        <el-table-column label="变更" width="70" align="center">
          <template #default="{ row }">{{ row.changes.length }}</template>
        </el-table-column>
      </el-table>
      <template #footer>
        <el-button @click="showPushDialog = false">取消</el-button>
        <el-button
          type="primary"
          :loading="actionLoading"
          :disabled="pushSelection.length === 0"
          @click="doPush"
        >
          Push（{{ pushSelection.length }}）
        </el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import {
  Plus,
  Refresh,
  Search,
  Bell,
  Monitor,
  CirclePlus,
  Upload,
  EditPen,
  Share,
  View,
  Close,
  ArrowDown,
  ArrowUp,
  Expand,
  Fold,
  RefreshLeft,
  Download,
  FolderOpened,
  Grid,
  Box,
  Files,
  Warning,
  Collection,
  Odometer,
  Connection,
  Document,
  Clock,
} from "@element-plus/icons-vue";
import { ElMessage, ElMessageBox } from "element-plus";
import { listen } from "@tauri-apps/api/event";
import { useWorkspaceStore } from "@/stores/workspace";
import { useRepositoryStore } from "@/stores/repository";
import { useTaskStore } from "@/stores/task";
import { startWatcher, stopWatcher, batchCommit, batchFetch, batchPull, batchPush } from "@/api/git_ops";
import {
  scanCommit,
  getCommitIdentity,
  setRepoIdentity,
  setGroupIdentity,
} from "@/api/commit";
import type { CommitScanFinding, CommitIdentity } from "@/types/commit";
import { selectRepos, batchBranchOp, batchDryRun } from "@/api/batch";
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
import { getDiff } from "@/api/git";
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
import WorkspaceManager from "@/components/common/WorkspaceManager.vue";
import LogManager from "@/components/common/LogManager.vue";
import { errMsg } from "@/utils/error";

interface SelectedDiff {
  repoPath: string;
  relPath: string;
  file: FileDiff;
}

const router = useRouter();
const route = useRoute();
const workspaceStore = useWorkspaceStore();
const repoStore = useRepositoryStore();
const taskStore = useTaskStore();

const selectedWorkspaceId = ref<number | null>(null);
const showAddWorkspace = ref(false);
const showLogManager = ref(false);
const watcherActive = ref(false);
const searchQuery = ref("");
const changes = ref<RepoChanges[]>([]);
const changesLoading = ref(false);
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
const pushTableRef = ref();
const diffWidth = ref<number | null>(null);
const diffPaneEl = ref<HTMLElement | null>(null);
let resizeStartX = 0;
let resizeStartWidth = 0;

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

onMounted(async () => {
  await workspaceStore.loadWorkspaces();
  if (workspaceStore.currentWorkspace) {
    selectedWorkspaceId.value = workspaceStore.currentWorkspace.id;
    await loadChanges();
    await startFileWatcher();
    await applyRoutePrefill();
  }

  // Listen for scan progress events
  unlistenScan = await listen<ScanProgress>("scan_progress", (event) => {
    scanProgress.value = event.payload;
  });
});

/** Prefill from Dashboard quick actions (T-18): `?selector=@status:xxx`
 * primes the T-20 selector (resolved immediately, not via the debounced
 * watcher), `?action=...` triggers the matching batch flow. The query is
 * cleared afterwards so a refresh does not re-fire the action. */
async function applyRoutePrefill() {
  const selector =
    typeof route.query.selector === "string" ? route.query.selector : "";
  const action = typeof route.query.action === "string" ? route.query.action : "";
  if (!selector && !action) return;
  try {
    if (selector) {
      selectorQuery.value = selector;
      selectorPaths.value = await selectRepos(selectedWorkspaceId.value!, selector);
      commitPanelOpen.value = true;
    }
    switch (action) {
      case "fetch": {
        // Fetch All: every repo in the workspace, not just dirty ones.
        const paths = changes.value.map((c) => c.repoPath);
        if (paths.length === 0) break;
        actionLoading.value = true;
        try {
          const ids = await batchFetch(paths);
          ElMessage.success(`已提交 ${ids.length} 个 fetch 任务`);
          await loadChanges();
        } finally {
          actionLoading.value = false;
        }
        break;
      }
      case "pull":
        // Clean repos were prefilled; the dry-run dialog decides which are
        // fast-forwardable before anything mutates (T-20 safety flow).
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
    }
  } catch (e) {
    ElMessage.error("预填快捷操作失败: " + errMsg(e));
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
  if (!selectedWorkspaceId.value) return;
  changesLoading.value = true;
  try {
    changes.value = await getWorkspaceChanges(selectedWorkspaceId.value);
  } catch (e) {
    ElMessage.error("加载变更失败: " + errMsg(e));
  } finally {
    changesLoading.value = false;
  }
}

function onWorkspaceChange(id: number) {
  selectedWorkspaceId.value = id;
  loadChanges();
  selectedDiff.value = null;
}

function onTreeSelection(selection: TreeSelection) {
  treeSelection.value = selection;
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
    const files = await getDiff(node.repoPath);
    const match = files.find(
      (f) => f.newPath === node.relPath || f.oldPath === node.relPath,
    );
    if (match) {
      selectedDiff.value = {
        repoPath: node.repoPath,
        relPath: node.relPath,
        file: match,
      };
    } else {
      selectedDiff.value = null;
      ElMessage.info("该文件没有可展示的变更内容");
    }
  } catch (e) {
    ElMessage.error("加载变更内容失败: " + errMsg(e));
  } finally {
    diffLoading.value = false;
  }
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

/** Start dragging the diff-pane width. */
function startResize(e: MouseEvent) {
  e.preventDefault();
  resizeStartX = e.clientX;
  resizeStartWidth = diffPaneEl.value?.offsetWidth ?? 600;
  document.addEventListener("mousemove", onResizeMove);
  document.addEventListener("mouseup", endResize);
}

function onResizeMove(e: MouseEvent) {
  const delta = resizeStartX - e.clientX; // drag left -> wider diff
  const maxW = window.innerWidth * 0.7;
  diffWidth.value = Math.max(320, Math.min(maxW, resizeStartWidth + delta));
}

function endResize() {
  document.removeEventListener("mousemove", onResizeMove);
  document.removeEventListener("mouseup", endResize);
}

async function handleScan() {
  if (!selectedWorkspaceId.value) return;
  scanProgress.value = null;
  try {
    await repoStore.scanRepositories(selectedWorkspaceId.value);
    ElMessage.success(`发现 ${repoStore.totalCount} 个仓库`);
    await loadChanges();
    await startFileWatcher();
  } catch (e) {
    ElMessage.error("扫描失败: " + errMsg(e));
  } finally {
    scanProgress.value = null;
  }
}

function onWorkspaceAdded() {
  if (workspaceStore.currentWorkspace) {
    selectedWorkspaceId.value = workspaceStore.currentWorkspace.id;
    handleScan();
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
    ElMessage.warning("请先勾选要暂存的文件");
    return;
  }
  actionLoading.value = true;
  try {
    await batchAdd(requests);
    ElMessage.success(`已暂存 ${requests.length} 个仓库的文件`);
    await loadChanges();
  } catch (e) {
    ElMessage.error("暂存失败: " + errMsg(e));
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
    ElMessage.warning("请先勾选要回退的文件");
    return;
  }
  try {
    await ElMessageBox.confirm(
      `确定要回退 ${selectedFileCount.value} 个文件的工作区修改吗？\n已跟踪文件将恢复到 Git 已提交版本（同时取消暂存），未跟踪/新增文件将被删除。`,
      "批量回退",
      { type: "warning", confirmButtonText: "回退", cancelButtonText: "取消" },
    );
  } catch {
    return; // cancelled
  }
  actionLoading.value = true;
  try {
    await batchRestore(requests);
    ElMessage.success(`已回退 ${requests.length} 个仓库的文件`);
    await loadChanges();
  } catch (e) {
    ElMessage.error("回退失败: " + errMsg(e));
  } finally {
    actionLoading.value = false;
  }
}

async function handleCommit() {
  const amend = commitForm.value.amend;
  const message = commitForm.value.message.trim();
  if (!message && !amend) {
    ElMessage.warning("请输入提交信息（Amend 可留空 = --no-edit）");
    return;
  }
  const commits: CommitRequest[] = [];
  for (const [repoPath, files] of treeSelection.value.filesByRepo.entries()) {
    commits.push({
      repoPath,
      repoName: repoNameOf(repoPath),
      message,
      files,
      amend,
      noEdit: amend && !message,
      thenPush: commitForm.value.thenPush,
    });
  }
  if (commits.length === 0) {
    ElMessage.warning("请先勾选要提交的文件");
    return;
  }
  // Pre-commit safety scan (T-11 §5): block on findings until the user
  // explicitly overrides via the findings dialog.
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
    ElMessage.error("安全检查失败: " + errMsg(e));
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
    ElMessage.success(`已提交 ${taskIds.length} 个 commit 任务`);
    commitForm.value.message = "";
    await loadChanges();
  } catch (e) {
    ElMessage.error("提交失败: " + errMsg(e));
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
    ElMessage.warning("Name 和 Email 需同时填写或同时留空");
    return;
  }
  d.saving = true;
  try {
    if (d.scope === "group" && d.groupId != null) {
      await setGroupIdentity(d.groupId, name, email);
    } else {
      await setRepoIdentity(repo, name, email);
    }
    ElMessage.success(name ? "已保存提交身份" : "已清除自定义身份（恢复默认）");
    d.show = false;
  } catch (e) {
    ElMessage.error("保存失败: " + errMsg(e));
  } finally {
    d.saving = false;
  }
}

/** Selector query (debounced) against the in-memory repo facets (T-20). */
let selectorTimer: number | undefined;
watch(selectorQuery, (q) => {
  // Keep the quick chips in sync when the query is edited by hand.
  for (const chip of quickChips.value) {
    chip.active = q.split(/\s+/).includes(chip.token);
  }
  window.clearTimeout(selectorTimer);
  selectorTimer = window.setTimeout(async () => {
    const query = q.trim();
    if (!query || !selectedWorkspaceId.value) {
      selectorPaths.value = [];
      return;
    }
    try {
      selectorPaths.value = await selectRepos(selectedWorkspaceId.value, query);
    } catch (e) {
      ElMessage.error("选择器查询失败: " + errMsg(e));
    }
  }, 300);
});


/** Toggle a quick-filter chip: add/remove its @status token in the query.
 * `checked` comes from el-check-tag's change event (its own toggle state),
 * so the chip state and the query stay consistent. */
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


/** Target repo set for repo-level batch ops: selector > tree > all dirty. */
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
    ElMessage.warning("没有目标仓库（先用选择器或勾选）");
    return;
  }
  branchOpTargets.value = targets;
  branchOpDialog.value = { show: true, loading: false, op, name: "", force: false };
}


/** Execute the bulk branch op; delete goes through a §46 danger confirm. */
async function handleBranchOp() {
  const d = branchOpDialog.value;
  if (!d.name.trim()) {
    ElMessage.warning("请输入分支名");
    return;
  }
  if (d.op === "delete") {
    try {
      await ElMessageBox.confirm(
        `确定从 ${branchOpTargets.value.length} 个仓库删除分支「${d.name}」吗？`,
        "危险操作确认",
        { type: "error", confirmButtonText: "确认删除", cancelButtonText: "取消" },
      );
    } catch {
      return;
    }
  }
  d.loading = true;
  try {
    const ids = await batchBranchOp(branchOpTargets.value, d.op, d.name.trim(), d.force);
    ElMessage.success(`已提交 ${ids.length} 个分支任务`);
    d.show = false;
  } catch (e) {
    ElMessage.error("操作失败: " + errMsg(e));
  } finally {
    d.loading = false;
  }
}


/** Dry-run report rows that can proceed (fast-forward only). */
const dryRunActionable = computed(() =>
  dryRunDialog.value.items.filter((i) => i.category === "fast_forward"),
);


/** Pull/Push dry-run: compute the impact report without mutating repos. */
async function runDryRun(op: "pull" | "push") {
  const targets = batchTargetRepos();
  if (targets.length === 0) {
    ElMessage.warning("没有目标仓库");
    return;
  }
  dryRunDialog.value = { show: true, loading: true, op, items: [] };
  try {
    dryRunDialog.value.items = await batchDryRun(targets, op);
  } catch (e) {
    ElMessage.error("预演失败: " + errMsg(e));
    dryRunDialog.value.show = false;
  } finally {
    dryRunDialog.value.loading = false;
  }
}


/** Execute the real batch op for the fast-forwardable repos only. */
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
    ElMessage.success(`已提交 ${paths.length} 个任务`);
  } catch (e) {
    ElMessage.error("执行失败: " + errMsg(e));
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


function dryRunTagType(c: string): "success" | "warning" | "danger" | "info" {
  return (
    {
      up_to_date: "info",
      fast_forward: "success",
      diverged: "warning",
      conflict: "danger",
      no_upstream: "info",
      error: "danger",
    } as const
  )[c] ?? "info";
}

// --- Workspace Stash (T-21) ---

/** Target repo count for the save button (selector > tree > all, T-20). */
const wsStashTargetCount = computed(() => batchTargetRepos().length);

/** One-line summary of the last save result. */
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
  if (!selectedWorkspaceId.value) return;
  wsStashDialog.value.show = true;
  wsStashDialog.value.lastSave = null;
  loadWsStashes();
}

async function loadWsStashes() {
  if (!selectedWorkspaceId.value) return;
  wsStashDialog.value.loading = true;
  try {
    wsStashDialog.value.list = await listWorkspaceStashes(selectedWorkspaceId.value);
    wsStashDialog.value.items = {};
  } catch (e) {
    ElMessage.error("加载 Workspace Stash 记录失败: " + errMsg(e));
  } finally {
    wsStashDialog.value.loading = false;
  }
}

/** Save the current target repo set as one `Workspace Stash #N` (T-21). */
async function saveWsStash() {
  const targets = batchTargetRepos();
  if (targets.length === 0) {
    ElMessage.warning("没有目标仓库（先用选择器或勾选）");
    return;
  }
  const d = wsStashDialog.value;
  d.saving = true;
  try {
    const result = await saveWorkspaceStash(
      selectedWorkspaceId.value!,
      targets,
      d.message.trim() || undefined,
      d.includeUntracked,
    );
    d.lastSave = result;
    const failed = result.items.filter((i) => i.status === "failed");
    if (failed.length > 0) {
      ElMessageBox.alert(
        failed.map((f) => `${f.repoName}：${f.detail}`).join("\n"),
        "部分仓库暂存失败",
        { type: "warning" },
      );
    }
    if (result.id != null) {
      d.message = "";
      await loadWsStashes();
    }
    await loadChanges();
  } catch (e) {
    ElMessage.error("暂存失败: " + errMsg(e));
  } finally {
    d.saving = false;
  }
}

/** Lazy-load the per-repo items when a record row is expanded. */
async function onWsStashExpand(row: WorkspaceStashSummary, expanded: WorkspaceStashSummary[]) {
  if (!expanded.includes(row) || wsStashDialog.value.items[row.id]) return;
  try {
    wsStashDialog.value.items[row.id] = await getWorkspaceStashItems(row.id);
  } catch (e) {
    ElMessage.error("加载记录明细失败: " + errMsg(e));
  }
}

/** Restore flow step 1 (§46): pre-check every repo, then show the Warning
 * confirmation with the affected-repo list. */
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
    ElMessage.error("恢复前校验失败: " + errMsg(e));
    wsStashCheck.value.show = false;
  } finally {
    wsStashCheck.value.loading = false;
  }
}

/** Repos the restore will actually touch. */
const wsStashApplicableCount = computed(() =>
  wsStashCheck.value.items.filter(
    (i) =>
      i.status === "ok" ||
      (i.status === "branch_mismatch" && wsStashCheck.value.allowMismatch),
  ).length,
);

/** Restore flow step 2: apply per repo, collect partial failures. */
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
      ElMessageBox.alert(
        failed.map((f) => `${f.repoName}：${f.detail}`).join("\n"),
        `已恢复 ${applied} 个仓库，${failed.length} 个失败`,
        { type: "warning" },
      );
    } else {
      ElMessage.success(
        `已恢复 ${applied} 个仓库${skipped > 0 ? `，跳过 ${skipped} 个` : ""}`,
      );
    }
    await loadChanges();
  } catch (e) {
    ElMessage.error("恢复失败: " + errMsg(e));
  } finally {
    c.restoring = false;
  }
}

/** Delete a record; the per-repo stashes stay on each repo's stack. */
async function removeWsStash(row: WorkspaceStashSummary) {
  try {
    await ElMessageBox.confirm(
      `确定删除记录「${row.name}」吗？仅删除关联记录，各仓库的 stash 条目仍保留在各自的 stash 栈中。`,
      "删除 Workspace Stash 记录",
      { type: "warning", confirmButtonText: "删除", cancelButtonText: "取消" },
    );
  } catch {
    return;
  }
  try {
    await deleteWorkspaceStash(row.id);
    ElMessage.success("已删除记录");
    await loadWsStashes();
  } catch (e) {
    ElMessage.error("删除失败: " + errMsg(e));
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

function wsStashCheckTagType(s: string): "success" | "warning" | "danger" | "info" {
  return (
    {
      ok: "success",
      branch_mismatch: "warning",
      stash_missing: "danger",
      repo_missing: "danger",
      error: "danger",
    } as const
  )[s] ?? "info";
}

async function handleFetch() {
  // Selector > tree selection > all dirty repos (T-20 selector precedence).
  const paths = batchTargetRepos();
  if (paths.length === 0) {
    ElMessage.warning("没有可操作的仓库");
    return;
  }
  actionLoading.value = true;
  try {
    const taskIds = await batchFetch(paths);
    ElMessage.success(`已提交 ${taskIds.length} 个 fetch 任务`);
    await loadChanges();
  } catch (e) {
    ElMessage.error("fetch 失败: " + errMsg(e));
  } finally {
    actionLoading.value = false;
  }
}

async function handlePull() {
  // Selector > tree selection > all dirty repos (T-20 selector precedence).
  const paths = batchTargetRepos();
  if (paths.length === 0) {
    ElMessage.warning("没有可操作的仓库");
    return;
  }
  actionLoading.value = true;
  try {
    const taskIds = await batchPull(paths);
    ElMessage.success(`已提交 ${taskIds.length} 个 pull 任务`);
    await loadChanges();
  } catch (e) {
    ElMessage.error("pull 失败: " + errMsg(e));
  } finally {
    actionLoading.value = false;
  }
}

/** Open the push picker dialog, defaulting to the current selection or all repos. */
function openPushDialog() {
  const defaultSelected = batchTargetRepos();
  pushSelection.value = defaultSelected;
  showPushDialog.value = true;
  // Pre-check matching rows after the dialog/table renders.
  setTimeout(() => {
    const table = pushTableRef.value;
    if (!table) return;
    changes.value.forEach((row) => {
      table.toggleRowSelection(row, defaultSelected.includes(row.repoPath));
    });
  }, 50);
}

function onPushSelectionChange(rows: RepoChanges[]) {
  pushSelection.value = rows.map((r) => r.repoPath);
}

async function doPush() {
  if (pushSelection.value.length === 0) {
    ElMessage.warning("请选择要 Push 的仓库");
    return;
  }
  actionLoading.value = true;
  try {
    const taskIds = await batchPush(pushSelection.value);
    ElMessage.success(`已提交 ${taskIds.length} 个 push 任务`);
    showPushDialog.value = false;
    await loadChanges();
  } catch (e) {
    ElMessage.error("push 失败: " + errMsg(e));
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
      ElMessage.info("文件监听已停止");
    } catch (e) {
      ElMessage.error("停止监听失败: " + errMsg(e));
    }
  } else {
    await startFileWatcher();
    if (watcherActive.value) {
      ElMessage.success("文件监听已启动");
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

/** Open the worktree manager for the given repo (T-17). */
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
  padding: 12px 16px;
  gap: 8px;
}

.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.toolbar-left {
  display: flex;
  gap: 8px;
  align-items: center;
}

.toolbar-right {
  display: flex;
  gap: 8px;
  align-items: center;
}

.main-body {
  display: flex;
  flex: 1;
  overflow: hidden;
  gap: 12px;
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
  background: #409eff;
}

.diff-pane {
  width: 46%;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  border: 1px solid #ebeef5;
  border-radius: 4px;
  overflow: hidden;
}

.diff-pane-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 8px;
  border-bottom: 1px solid #ebeef5;
  background: #fafafa;
}

.diff-pane-title {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.diff-repo {
  font-weight: 600;
  color: #409eff;
  font-size: 13px;
  flex-shrink: 0;
}

.diff-file {
  font-family: monospace;
  font-size: 12px;
  color: #606266;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.diff-pane-body {
  flex: 1;
  overflow: auto;
  padding: 4px 0;
}

.stats-bar {
  font-size: 13px;
  color: #606266;
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
  color: #409eff;
  font-weight: 500;
}

.scan-progress {
  margin-bottom: 8px;
  padding: 0 4px;
}

.tree-container {
  flex: 1;
  overflow: hidden;
  border: 1px solid #ebeef5;
  border-radius: 4px;
  min-height: 0;
}

.commit-panel {
  border: 1px solid #ebeef5;
  border-radius: 4px;
  background: #fafafa;
  flex-shrink: 0;
}

.commit-panel-header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 2px 8px;
}

.commit-panel-hint {
  font-size: 12px;
  color: #909399;
}

.commit-panel-body {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 0 12px 10px 12px;
}

.ops-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.commit-row {
  display: flex;
  align-items: flex-start;
  gap: 12px;
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
  color: #409eff;
  font-size: 12px;
}

.scope-count {
  color: #909399;
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
  font-size: 12px;
  color: #909399;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.separator {
  margin: 0 8px;
  color: #dcdfe6;
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
  color: #c0c4cc;
  padding: 2px 4px 0;
  flex-shrink: 0;
}

.commit-options {
  display: flex;
  align-items: center;
  gap: 16px;
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
  gap: 8px;
  padding: 6px 0;
  border-bottom: 1px solid #f0f0f0;
  font-size: 13px;
}

.scan-path {
  font-family: monospace;
  color: #303133;
}

.scan-detail {
  color: #909399;
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
  color: #909399;
}

.selector-count.is-empty {
  color: #e6a23c;
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
  gap: 12px;
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
  gap: 8px;
  font-size: 13px;
}

.ws-stash-item-repo {
  font-weight: 600;
  color: #409eff;
}

.ws-stash-item-oid {
  font-family: monospace;
  color: #909399;
  font-size: 12px;
}

.ws-stash-mismatch-allow {
  margin-top: 10px;
}
</style>
