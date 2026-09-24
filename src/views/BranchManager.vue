<template>
  <div class="branch-manager">
    <!-- Header -->
    <div class="branch-header">
      <div class="repo-info">
        <RepoSwitcher @change="onRepoSwitch" />
        <n-tag v-if="overview?.current" size="small" type="success">
          {{ overview.current }}
        </n-tag>
        <n-tag v-else-if="overview" size="small" type="warning">HEAD 游离</n-tag>
      </div>
      <n-button size="small" :loading="loading" @click="load">
        <template #icon><n-icon><RefreshOutline /></n-icon></template>
        刷新
      </n-button>
      <n-button size="small" type="primary" @click="handleCreate">
        <template #icon><n-icon><AddOutline /></n-icon></template>
        新建分支
      </n-button>
      <n-button size="small" @click="openCompare()">
        <template #icon><n-icon><SwapHorizontalOutline /></n-icon></template>
        Compare
      </n-button>
      <n-button size="small" @click="rebaseDialogVisible = true">
        Rebase
      </n-button>
      <!-- T-29：远程平台集成（Open Repo/Issues/PR/CI + Create PR） -->
      <n-dropdown
        trigger="click"
        :options="remoteMenuOptions"
        @select="handleRemoteMenu"
      >
        <n-button size="small" :disabled="!remoteInfo">
          <template #icon><n-icon><GitNetworkOutline /></n-icon></template>
          {{ remoteInfo ? `远程（${remoteInfo.platform}）` : "远程（未识别）" }}
        </n-button>
      </n-dropdown>
      <n-button
        size="small"
        type="primary"
        ghost
        :disabled="!remoteInfo"
        @click="openCreatePr"
      >
        <template #icon><n-icon><GitPullRequestOutline /></n-icon></template>
        Create PR
      </n-button>
    </div>

    <!-- Operation state banners (T-15): resume an interrupted merge/rebase
         after reload or restart -->
    <div v-if="mergeInProgress" class="state-banner merge">
      <span class="banner-text">Merge 进行中：存在冲突待解决（MERGE_HEAD 已置）。</span>
      <n-button size="small" type="primary" dashed @click="handleMergeContinue">
        已解决，继续（Continue）
      </n-button>
      <n-button size="small" type="primary" dashed @click="openResolver">
        打开解决器
      </n-button>
      <n-button size="small" type="error" dashed @click="handleMergeAbort">
        中止（Abort）
      </n-button>
      <span class="banner-hint">请先在变更视图解决冲突并暂存（三方解决器随 T-16 提供）</span>
    </div>
    <div v-if="rebaseState" class="state-banner rebase">
      <span class="banner-text">
        Rebase 进行中：第 {{ rebaseState.position + 1 }}/{{ rebaseState.ops.length }} 步
        （onto {{ rebaseState.onto }}，当前 {{ currentOpLabel }}）。
      </span>
      <n-button size="small" type="primary" dashed @click="handleRebaseContinue">
        已解决，继续（Continue）
      </n-button>
      <n-button size="small" type="warning" dashed @click="handleRebaseSkip">
        跳过（Skip）
      </n-button>
      <n-button size="small" type="primary" dashed @click="openResolver">
        打开解决器
      </n-button>
      <n-button size="small" type="error" dashed @click="handleRebaseAbort">
        中止（Abort）
      </n-button>
    </div>

    <n-spin :show="loading">
      <div class="branch-body">
        <template v-if="overview">
          <!-- Local branches -->
          <Panel title="Local Branches（{{ overview.locals.length }}）" class="branch-section">
            <!-- GF-11：>100 分支仓库常见——虚拟滚动（VirtualList 定高行）。
                 容器高度按行数封顶，短列表时高度=内容高度，无嵌套滚动条。 -->
            <div class="branch-list" :style="{ height: branchListHeight(overview.locals.length) + 'px' }">
              <VirtualList :items="overview.locals" :item-height="BRANCH_ROW_H">
                <template #row="{ item: b }">
                  <div
                    :class="['branch-row', { current: b.isCurrent }]"
                  >
                    <span class="branch-name">
                      {{ b.name }}
                      <n-tag v-if="b.isCurrent" size="small" type="success">当前</n-tag>
                    </span>
                    <span class="branch-track">
                      <template v-if="b.upstream">
                        <span class="upstream">{{ b.upstream }}</span>
                        <span v-if="b.ahead > 0" class="ahead">↑{{ b.ahead }}</span>
                        <span v-if="b.behind > 0" class="behind">↓{{ b.behind }}</span>
                      </template>
                      <span v-else class="no-upstream">无上游</span>
                    </span>
                    <span class="branch-commit" :title="b.lastCommitOid">
                      {{ shortOid(b.lastCommitOid) }} {{ b.lastCommitMessage }}
                    </span>
                    <n-dropdown trigger="click" :options="localBranchOptions(b)" @select="(key: string) => handleLocalCommand(key, b)">
                      <n-button size="small" text>
                        <template #icon><n-icon><EllipsisVerticalOutline /></n-icon></template>
                      </n-button>
                    </n-dropdown>
                  </div>
                </template>
              </VirtualList>
            </div>
            <n-empty v-if="overview.locals.length === 0" description="无本地分支" />
          </Panel>

          <!-- Remote branches -->
          <Panel title="Remote Branches（{{ overview.remotes.length }}）" class="branch-section">
            <div class="branch-list" :style="{ height: branchListHeight(overview.remotes.length) + 'px' }">
              <VirtualList :items="overview.remotes" :item-height="BRANCH_ROW_H">
                <template #row="{ item: r }">
                  <div class="branch-row">
                    <span class="branch-name">{{ r.name }}</span>
                    <span class="branch-track" />
                    <span class="branch-commit" :title="r.lastCommitOid">
                      {{ shortOid(r.lastCommitOid) }} {{ r.lastCommitMessage }}
                    </span>
                    <n-dropdown trigger="click" :options="remoteBranchOptions()" @select="(key: string) => handleRemoteCommand(key, r)">
                      <n-button size="small" text>
                        <template #icon><n-icon><EllipsisVerticalOutline /></n-icon></template>
                      </n-button>
                    </n-dropdown>
                  </div>
                </template>
              </VirtualList>
            </div>
            <n-empty v-if="overview.remotes.length === 0" description="无远程分支" />
          </Panel>

          <!-- Tags (GF-04: 创建 / 推送 / 删除) -->
          <Panel title="Tags（{{ overview.tags.length }}）" class="branch-section">
            <template #actions>
              <n-button size="tiny" @click="handleCreateTag">
                <template #icon><n-icon><AddOutline /></n-icon></template>
                新建标签
              </n-button>
            </template>
            <div class="branch-list" :style="{ height: branchListHeight(overview.tags.length) + 'px' }">
              <VirtualList :items="overview.tags" :item-height="BRANCH_ROW_H">
                <template #row="{ item: t }">
                  <div class="branch-row">
                    <span class="branch-name">{{ t.name }}</span>
                    <span class="branch-track tag-message" :title="t.message ?? ''">{{ t.message ?? "" }}</span>
                    <span class="branch-commit" :title="t.targetOid">{{ shortOid(t.targetOid) }}</span>
                    <n-dropdown trigger="click" :options="tagOptions()" @select="(key: string) => handleTagCommand(key, t)">
                      <n-button size="small" text>
                        <template #icon><n-icon><EllipsisVerticalOutline /></n-icon></template>
                      </n-button>
                    </n-dropdown>
                  </div>
                </template>
              </VirtualList>
            </div>
            <n-empty v-if="overview.tags.length === 0" description="无标签" />
          </Panel>
        </template>
      </div>
    </n-spin>

    <!-- Merge dialog (T-15) -->
    <n-modal v-model:show="mergeDialog.show" preset="card" title="Merge 到当前分支" style="width: 520px">
      <div class="merge-form">
        <div class="merge-line">
          源分支：<strong>{{ mergeDialog.branch }}</strong> → 当前分支：<strong>{{ overview?.current }}</strong>
        </div>
        <n-radio-group v-model:value="mergeDialog.mode" class="merge-modes">
          <n-radio value="normal">普通（可快进则快进）</n-radio>
          <n-radio value="no-ff">--no-ff（始终生成合并提交）</n-radio>
          <n-radio value="squash">--squash（压成暂存更改，不产生合并提交）</n-radio>
        </n-radio-group>
        <!-- GF-17：结构化预演 —— 「Merge 会发生什么」（Roadmap §46） -->
        <n-spin :show="mergePreviewLoading" size="small" class="merge-preview">
          <div v-if="mergePreviewError" class="merge-preview-error">
            预演加载失败：{{ mergePreviewError }}（仍可执行，执行时会再次校验）
          </div>
          <template v-else-if="mergePreview">
            <div class="merge-preview-line">
              <n-tag size="small" :type="mergePreview.conflictPredicted ? 'error' : 'default'">
                {{ mergeKindLabel(mergePreview.kind) }}
              </n-tag>
              将并入
              <strong :class="{ 'danger-text': mergePreview.conflictPredicted }">{{
                mergePreview.incomingCount
              }}</strong>
              个提交，影响
              <strong>{{ mergePreview.affectedFilesCount }}</strong>
              个文件
              <span v-if="mergePreview.conflictPredicted" class="danger-text">
                （预判冲突：{{ mergePreview.conflictFiles.slice(0, 3).join("、") }}{{
                  mergePreview.conflictFiles.length > 3 ? " 等" : ""
                }}）
              </span>
            </div>
            <ul v-if="mergePreview.incomingCommits.length" class="merge-preview-list">
              <li v-for="c in mergePreview.incomingCommits" :key="c.oid">
                <span class="mono">{{ c.shortOid }}</span>
                <span class="merge-preview-msg">{{ c.summary }}</span>
              </li>
            </ul>
            <div v-if="mergePreviewKindDetail" class="dim-text">{{ mergePreviewKindDetail }}</div>
            <div v-if="mergePreview.dirtyBlocked" class="danger-text">
              工作区有 {{ mergePreview.dirtyFilesCount }} 个未提交变更，Merge 会被拒绝（请先提交或
              stash）：{{ mergePreview.dirtyFiles.slice(0, 5).join("、") }}
            </div>
          </template>
        </n-spin>
      </div>
      <template #footer>
        <n-button @click="mergeDialog.show = false">取消</n-button>
        <n-button type="primary" :loading="mergeDialog.loading" @click="runMerge">
          执行 Merge
        </n-button>
      </template>
    </n-modal>

    <!-- Interactive Rebase dialog (T-15) -->
    <RebaseDialog
      v-model="rebaseDialogVisible"
      :repo-path="repoPath"
      :revisions="rebaseRevisions"
      :default-onto="defaultOnto"
      @finished="onRebaseFinished"
    />

    <!-- GF-04：新建标签（留空附注消息 = 轻量标签；目标固定当前 HEAD） -->
    <n-modal v-model:show="tagDialog.show" preset="card" title="新建标签" style="width: 480px">
      <div class="tag-form">
        <div class="tag-field">
          <span class="tag-label">标签名</span>
          <n-input
            v-model:value="tagDialog.name"
            size="small"
            placeholder="如 v1.2.0"
            :status="tagDialog.name && !TAG_NAME_PATTERN.test(tagDialog.name) ? 'error' : undefined"
          />
        </div>
        <div class="tag-field">
          <span class="tag-label">附注消息（留空 = 轻量标签）</span>
          <n-input
            v-model:value="tagDialog.message"
            type="textarea"
            :rows="3"
            size="small"
            placeholder="release notes…"
          />
        </div>
        <div class="tag-hint">
          目标：当前 HEAD {{ overview?.current ?? "（HEAD 游离）" }} {{ shortOid(headOid) }}
        </div>
      </div>
      <template #footer>
        <n-button @click="tagDialog.show = false">取消</n-button>
        <n-button
          type="primary"
          :loading="tagDialog.loading"
          :disabled="!tagDialog.name.trim() || !TAG_NAME_PATTERN.test(tagDialog.name.trim())"
          @click="runCreateTag"
        >
          创建
        </n-button>
      </template>
    </n-modal>

    <!-- GF-04：推送标签。force 默认关闭；Roadmap §47 明示覆盖风险并推荐
         --force-with-lease（远端被他人更新时安全失败）。 -->
    <n-modal v-model:show="tagPushDialog.show" preset="card" title="Push 标签" style="width: 480px">
      <div class="tag-form">
        <div class="tag-field">
          <span class="tag-label">标签</span>
          <span class="tag-value">{{ tagPushDialog.name }} → {{ shortOid(tagPushDialog.targetOid) }}</span>
        </div>
        <n-checkbox v-model:checked="tagPushDialog.force" size="small">Force push</n-checkbox>
        <n-radio-group v-if="tagPushDialog.force" v-model:value="tagPushDialog.forceWithLease" size="small">
          <n-radio :value="true">--force-with-lease（推荐）</n-radio>
          <n-radio :value="false">--force</n-radio>
        </n-radio-group>
        <n-alert v-if="tagPushDialog.force" type="warning" :bordered="false" class="tag-force-alert">
          This may overwrite remote history. 强制推送会用本地标签覆盖远程同名标签；
          --force-with-lease 在远端被他人更新时会安全失败（git 默认就拒绝覆盖远程已有标签）。
        </n-alert>
      </div>
      <template #footer>
        <n-button @click="tagPushDialog.show = false">取消</n-button>
        <n-button type="primary" :loading="tagPushDialog.loading" @click="runTagPush">推送</n-button>
      </template>
    </n-modal>

    <!-- Compare dialog -->
    <n-modal v-model:show="compare.show" preset="card" title="Branch Compare" style="width: 80%; margin-top: 5vh">
      <div class="compare-form">
        <n-select v-model:value="compare.base" filterable placeholder="Base（基准）" style="width: 240px" :options="revisionSelectOptions" />
        <span class="compare-arrow">⇄</span>
        <n-select v-model:value="compare.other" filterable placeholder="Other（对比）" style="width: 240px" :options="revisionSelectOptions" />
        <n-button
          type="primary"
          :loading="compare.loading"
          :disabled="!compare.base || !compare.other"
          @click="runCompare"
        >
          比较
        </n-button>
      </div>

      <div v-if="compare.result" class="compare-result">
        <div class="compare-summary">
          <n-tag type="success">领先 {{ compare.result.ahead.length }}</n-tag>
          <span class="summary-text">{{ compare.result.other }} 领先 {{ compare.result.base }}</span>
          <n-tag type="warning">落后 {{ compare.result.behind.length }}</n-tag>
          <span class="summary-text">{{ compare.result.other }} 落后 {{ compare.result.base }}</span>
        </div>
        <n-tabs v-model:value="compare.tab">
          <n-tab-pane :tab="`领先 Commits（${compare.result.ahead.length}）`" name="ahead">
            <div v-for="c in compare.result.ahead" :key="c.oid" class="commit-row">
              <span class="commit-oid">{{ c.shortOid }}</span>
              <span class="commit-msg">{{ c.message }}</span>
              <span class="commit-meta">{{ c.author }} · {{ c.time }}</span>
            </div>
            <n-empty v-if="compare.result.ahead.length === 0" description="无" />
          </n-tab-pane>
          <n-tab-pane :tab="`落后 Commits（${compare.result.behind.length}）`" name="behind">
            <div v-for="c in compare.result.behind" :key="c.oid" class="commit-row">
              <span class="commit-oid">{{ c.shortOid }}</span>
              <span class="commit-msg">{{ c.message }}</span>
              <span class="commit-meta">{{ c.author }} · {{ c.time }}</span>
            </div>
            <n-empty v-if="compare.result.behind.length === 0" description="无" />
          </n-tab-pane>
          <n-tab-pane :tab="`文件差异（${compare.result.files.length}）`" name="files">
            <div class="compare-files">
              <div class="file-list">
                <div
                  v-for="f in compare.result.files"
                  :key="f.newPath"
                  :class="['file-item', { active: compare.selectedFile?.newPath === f.newPath }]"
                  @click="compare.selectedFile = f"
                >
                  <span :class="['file-status-icon', f.status]">{{ statusIcon(f.status) }}</span>
                  <span class="file-name">{{ f.newPath }}</span>
                </div>
                <n-empty v-if="compare.result.files.length === 0" description="无文件差异" />
              </div>
              <div class="file-diff">
                <UnifiedDiff v-if="compare.selectedFile" :file="compare.selectedFile" />
                <n-empty v-else description="选择文件查看 Diff" />
              </div>
            </div>
          </n-tab-pane>
        </n-tabs>
      </div>
    </n-modal>

    <!-- T-29：Create Pull Request 对话框（Source/Target/Commits/Files 回填 + AI 描述） -->
    <n-modal v-model:show="prDialog.show" preset="card" title="Create Pull Request" style="width: 640px">
      <div class="pr-form">
        <div class="pr-row">
          <div class="pr-field">
            <span class="pr-label">Source 分支</span>
            <n-select v-model:value="prDialog.source" :options="prBranchOptions" size="small" @update:value="loadPrStats" />
          </div>
          <div class="pr-field">
            <span class="pr-label">Target 分支</span>
            <n-select v-model:value="prDialog.target" :options="prBranchOptions" size="small" @update:value="loadPrStats" />
          </div>
        </div>
        <div v-if="prDialog.stats" class="pr-stats">
          {{ prDialog.stats.ahead.length }} 个提交 · {{ prDialog.stats.files.length }} 个文件变更
          （{{ prDialog.stats.behind.length }} 个落后提交）
        </div>
        <div class="pr-field">
          <span class="pr-label">Title</span>
          <n-input v-model:value="prDialog.title" size="small" placeholder="PR 标题" />
        </div>
        <div class="pr-field">
          <span class="pr-label">Description</span>
          <n-input
            v-model:value="prDialog.body"
            type="textarea"
            :rows="8"
            placeholder="PR 描述（可 AI 生成）"
          />
        </div>
        <div class="pr-actions">
          <n-button size="small" @click="fillStructuredDescription">生成描述（提交清单）</n-button>
          <n-button size="small" :loading="prDialog.aiBuilding" @click="aiGenerateDescription">
            AI 生成
          </n-button>
        </div>
        <n-alert v-if="prDialog.tokenMissing" type="warning" :bordered="false" class="pr-token-alert">
          未在系统凭据中找到 {{ remoteInfo?.host }} 的 token，匿名创建大概率失败。
          可在下方填入 token 保存到 OS 凭据库（不落盘明文）。
          <n-input
            v-model:value="prDialog.tokenInput"
            type="password"
            show-password-on="click"
            size="small"
            placeholder="platform token（可选）"
            style="margin-top: 6px"
          />
          <n-button size="tiny" style="margin-top: 6px" @click="saveToken">保存 Token</n-button>
        </n-alert>
        <n-alert v-if="prDialog.resultUrl" type="success" :bordered="false" class="pr-token-alert">
          PR 已创建：{{ prDialog.resultUrl }}
          <n-button size="tiny" style="margin-left: 8px" @click="openExternal(prDialog.resultUrl)">打开</n-button>
        </n-alert>
      </div>
      <template #footer>
        <n-button @click="prDialog.show = false">关闭</n-button>
        <n-button type="primary" :loading="prDialog.submitting" @click="submitPr">创建 PR</n-button>
      </template>
    </n-modal>

    <!-- Smart Merge dialog -->
    <SmartMergeDialog
      v-model:show="smartMerge.show"
      :repo-path="repoPath"
      :conflicts="smartMerge.conflicts"
      :base-oid="smartMerge.baseOid"
      @resolved="onSmartMergeResolved"
      @aborted="onSmartMergeAborted"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useCurrentRepo } from "@/composables/useCurrentRepo";
import RepoSwitcher from "@/components/shell/RepoSwitcher.vue";
import { EllipsisVerticalOutline, AddOutline, RefreshOutline, SwapHorizontalOutline, GitNetworkOutline, GitPullRequestOutline } from "@vicons/ionicons5";
import { useMessage, useDialog } from "naive-ui";
import { prompt } from "@/utils/prompt";
import { open as openExternal } from "@tauri-apps/plugin-shell";
import {
  createPullRequest,
  detectRemote,
  getCiStatus,
  remoteOpenUrl,
  resolveRemoteToken,
  saveRemoteToken,
  type RemoteInfo,
} from "@/api/remote";
import {
  aiApproveRequest,
  aiBuildContextPreview,
  aiGetRequestStatus,
  aiSubmitRequest,
} from "@/api/ai";
import type { RequestPhase } from "@/types/ai";
import { guardRuntimeRunning } from "@/utils/runtimeGuard";
import {
  checkoutBranch,
  compareBranches,
  createBranch,
  createTag,
  deleteBranch,
  deleteTag,
  listBranches,
  pushBranch,
  pushTag,
  renameBranch,
  setUpstream,
  tagPushedToRemote,
  trackRemoteBranch,
} from "@/api/branch";
import type {
  BranchEntry,
  BranchOverview,
  CompareResult,
  RemoteBranchEntry,
  TagEntry,
} from "@/types/branch";
import type { FileDiff } from "@/types/git";
import { smartPull } from "@/api/git_ops";
import SmartMergeDialog from "@/components/git/SmartMergeDialog.vue";
import UnifiedDiff from "@/components/diff/UnifiedDiff.vue";
import RebaseDialog from "@/components/branch/RebaseDialog.vue";
import Panel from "@/components/shell/Panel.vue";
import VirtualList from "@/components/common/VirtualList.vue";
import { getMergeInProgress, mergeAbort, mergeBranch, mergeContinue } from "@/api/merge";
import { getRebaseState, rebaseAbort, rebaseContinue, rebaseSkip } from "@/api/rebase";
import { previewMerge } from "@/api/preview";
import type { MergeOutcome } from "@/types/merge";
import type { MergePreview } from "@/types/preview";
import type { RebaseOutcome, RebaseState } from "@/types/rebase";
import { errMsg } from "@/utils/error";

const router = useRouter();
const message = useMessage();
const { resolveCurrentRepo } = useCurrentRepo();
const dialog = useDialog();

const repoPath = ref("");
// ── T-29：远程平台集成状态 ───────────────────────────────
const remoteInfo = ref<RemoteInfo | null>(null);
const ciStatusText = ref("");
const prDialog = reactive<{
  show: boolean;
  source: string;
  target: string;
  title: string;
  body: string;
  stats: CompareResult | null;
  tokenMissing: boolean;
  tokenInput: string;
  resultUrl: string;
  submitting: boolean;
  aiBuilding: boolean;
}>({
  show: false,
  source: "",
  target: "",
  title: "",
  body: "",
  stats: null,
  tokenMissing: false,
  tokenInput: "",
  resultUrl: "",
  submitting: false,
  aiBuilding: false,
});

const remoteMenuOptions = [
  { label: "打开仓库", key: "repo" },
  { label: "打开 Issues", key: "issues" },
  { label: "打开 Pull Requests", key: "pulls" },
  { label: "查看 CI 状态", key: "ci" },
];

const prBranchOptions = computed(() => {
  const names: string[] = [];
  if (overview.value) {
    names.push(...overview.value.locals.map((b) => b.name));
    names.push(...overview.value.remotes.map((b) => b.name));
  }
  return Array.from(new Set(names)).map((n) => ({ label: n, value: n }));
});

/** 平台识别：失败静默（无 origin / 无法识别时禁用远程按钮）。 */
async function detectRemoteSilently() {
  remoteInfo.value = null;
  ciStatusText.value = "";
  try {
    remoteInfo.value = await detectRemote(repoPath.value);
  } catch {
    // 无 origin 或平台不可识别——远程功能禁用即可
  }
}

function handleRemoteMenu(key: string) {
  if (!remoteInfo.value || !repoPath.value) return;
  if (key === "ci") {
    void showCiStatus();
    return;
  }
  void remoteOpenUrl(repoPath.value, key)
    .then((url) => openExternal(url))
    .catch((e) => message.error("打开远程页面失败: " + errMsg(e)));
}

async function showCiStatus() {
  if (!remoteInfo.value) return;
  const gitRef = overview.value?.current ?? "HEAD";
  message.loading("查询 CI 状态…");
  try {
    const status = await getCiStatus(repoPath.value, gitRef);
    ciStatusText.value = status.state;
    message.info(`CI（${gitRef}）：${status.state}${status.url ? " · " + status.url : ""}`);
    if (status.url) openExternal(status.url);
  } catch (e) {
    message.error("CI 状态查询失败: " + errMsg(e));
  }
}

async function openCreatePr() {
  const current = overview.value?.current ?? "";
  const fallbackTarget = prBranchOptions.value.some((o) => o.value === "main")
    ? "main"
    : prBranchOptions.value.some((o) => o.value === "master")
      ? "master"
      : (prBranchOptions.value[0]?.value ?? "");
  prDialog.source = current;
  prDialog.target = fallbackTarget;
  prDialog.title = "";
  prDialog.body = "";
  prDialog.stats = null;
  prDialog.resultUrl = "";
  prDialog.show = true;
  // 凭据预检（提示但不阻塞——系统 git 凭据助手也可能可用）
  if (remoteInfo.value) {
    try {
      const token = await resolveRemoteToken(remoteInfo.value.platform, remoteInfo.value.host);
      prDialog.tokenMissing = !token;
    } catch {
      prDialog.tokenMissing = true;
    }
  }
  await loadPrStats();
}

async function loadPrStats() {
  if (!prDialog.source || !prDialog.target || prDialog.source === prDialog.target) {
    prDialog.stats = null;
    return;
  }
  try {
    prDialog.stats = await compareBranches(repoPath.value, prDialog.target, prDialog.source);
    if (!prDialog.title && prDialog.stats.ahead.length > 0) {
      prDialog.title = prDialog.stats.ahead[0].message.split("\n")[0] ?? "";
    }
  } catch (e) {
    prDialog.stats = null;
    message.error("分支比较失败: " + errMsg(e));
  }
}

/** 结构化描述：提交清单 + 文件统计（无 AI 时的可编辑底稿）。 */
function fillStructuredDescription() {
  const s = prDialog.stats;
  if (!s) return;
  const commits = s.ahead
    .map((c) => `- ${c.oid.slice(0, 7)} ${c.message.split("\n")[0]}`)
    .join("\n");
  const files = s.files
    .slice(0, 50)
    .map((f) => `- ${f.status} ${f.newPath}`)
    .join("\n");
  prDialog.body =
    `## 变更摘要\n\n${commits || "-（无提交）"}\n\n## 文件（${s.files.length}）\n\n${files}\n`;
}

/** AI 生成：复用 T-27 上下文预览管线（gitScenario=prDescription）。 */
async function aiGenerateDescription() {
  const s = prDialog.stats;
  if (!s) return;
  prDialog.aiBuilding = true;
  try {
    const preview = await aiBuildContextPreview({
      taskKind: "commitMessage",
      gitScenario: "prDescription",
      providerId: null,
      modelId: null,
      workspaceId: null,
      repoPath: repoPath.value,
      runtimeName: null,
      processId: null,
      project: null,
      userInstruction:
        `为从 ${prDialog.target} 到 ${prDialog.source} 的 Pull Request 生成标题与描述。` +
        `提交清单见补充上下文。`,
      supplementary: [
        {
          role: "history",
          kind: "log",
          sourceId: "pr-commits",
          displayName: `提交（${s.ahead.length}）`,
          content:
            s.ahead.map((c) => `${c.oid.slice(0, 7)} ${c.message}`).join("\n") || "（无提交）",
        },
        {
          role: "changeSummary",
          kind: "diff",
          sourceId: "pr-files",
          displayName: `文件（${s.files.length}）`,
          content: s.files.map((f) => `${f.status} ${f.newPath}`).join("\n") || "（无文件）",
        },
      ],
    });
    const submitted = await aiSubmitRequest({ ...preview.request, useCache: true });
    const approved =
      submitted.phase === "previewRequired" ? await aiApproveRequest(submitted.requestId) : submitted;
    // 轮询到终态（与 AiGitAssistantDialog 相同节奏）
    let snapshot = approved;
    const terminal: RequestPhase[] = ["succeeded", "cancelled", "rejected", "failed"];
    for (let i = 0; i < 120 && !terminal.includes(snapshot.phase); i++) {
      await new Promise((r) => setTimeout(r, 350));
      const next = await aiGetRequestStatus(submitted.requestId);
      if (!next) break;
      snapshot = next;
    }
    if (snapshot.phase === "succeeded" && snapshot.result?.type === "prDescription") {
      const payload = snapshot.result.payload;
      prDialog.title = payload.title;
      const sections: string[] = [];
      if (payload.description) sections.push(payload.description);
      if (payload.summary.length > 0) sections.push("## 摘要\n\n" + payload.summary.map((x) => `- ${x}`).join("\n"));
      if (payload.testing.length > 0) sections.push("## 测试\n\n" + payload.testing.map((x) => `- ${x}`).join("\n"));
      if (payload.risks.length > 0) sections.push("## 风险\n\n" + payload.risks.map((x) => `- ${x}`).join("\n"));
      prDialog.body = sections.join("\n\n");
      message.success("AI 描述已生成");
    } else {
      message.error("AI 请求未完成" + (snapshot.error ? ": " + snapshot.error : ""));
    }
  } catch (e) {
    message.error("AI 生成失败: " + errMsg(e));
  } finally {
    prDialog.aiBuilding = false;
  }
}

async function saveToken() {
  if (!remoteInfo.value || !prDialog.tokenInput) return;
  try {
    await saveRemoteToken(remoteInfo.value.platform, remoteInfo.value.host, prDialog.tokenInput);
    prDialog.tokenInput = "";
    prDialog.tokenMissing = false;
    message.success("Token 已保存到 OS 凭据库");
  } catch (e) {
    message.error("Token 保存失败: " + errMsg(e));
  }
}

async function submitPr() {
  if (!remoteInfo.value) return;
  if (!prDialog.source || !prDialog.target) {
    message.warning("请选择 Source / Target 分支");
    return;
  }
  if (!prDialog.title.trim()) {
    message.warning("请填写 PR 标题");
    return;
  }
  prDialog.submitting = true;
  try {
    const result = await createPullRequest({
      repoPath: repoPath.value,
      source: prDialog.source,
      target: prDialog.target,
      title: prDialog.title.trim(),
      body: prDialog.body,
      draft: false,
    });
    prDialog.resultUrl = result.url;
    message.success(`PR #${result.number} 已创建`);
  } catch (e) {
    message.error("创建 PR 失败: " + errMsg(e));
  } finally {
    prDialog.submitting = false;
  }
}
const overview = ref<BranchOverview | null>(null);
const loading = ref(false);

// --- GF-04: tags ---
/** 标签名与分支名同一套字符集（后端 `validate_tag_name` 同样拒绝前导 '-'）。 */
const TAG_NAME_PATTERN = /^[^\s~^:?*[\]\\]+$/;

// --- GF-11：分支/标签长列表虚拟滚动参数 ---
/** VirtualList 固定行高（px）——.branch-row 显式撑满该高度。
 *  34px 覆盖含 NTag（small 20px）的自然行高（~33px），不裁切。 */
const BRANCH_ROW_H = 34;
/** 折叠滚动条前可见的最大行数（超出后列表内部滚动，页面其他部分不动）。 */
const BRANCH_LIST_MAX_ROWS = 11;

/** 列表容器高度：内容高度与封顶值取小，短列表无嵌套滚动、无多余空白。 */
function branchListHeight(count: number): number {
  return Math.min(count, BRANCH_LIST_MAX_ROWS) * BRANCH_ROW_H;
}
const tagDialog = reactive({ show: false, name: "", message: "", loading: false });
const tagPushDialog = reactive({
  show: false,
  name: "",
  targetOid: "",
  /** Roadmap §47：force push 默认关闭，用户显式开启。 */
  force: false,
  /** force 开启时的默认模式：--force-with-lease（推荐）。 */
  forceWithLease: true,
  loading: false,
});
/** 当前 HEAD 的 oid（标签面板创建入口据此展示目标）。 */
const headOid = computed(
  () => overview.value?.locals.find((b) => b.isCurrent)?.lastCommitOid ?? "",
);

// --- T-15 merge / rebase state ---
const mergeDialog = reactive({ show: false, branch: "", mode: "normal", loading: false });
/** GF-17：merge 结构化预演（只读命令 preview_merge 的结果）。 */
const mergePreview = ref<MergePreview | null>(null);
const mergePreviewLoading = ref(false);
const mergePreviewError = ref("");

/** GF-17：打开 merge 确认框 / 切换模式时刷新预演（只读，失败不阻塞执行流）。 */
async function loadMergePreview() {
  if (!repoPath.value || !mergeDialog.branch) return;
  mergePreviewLoading.value = true;
  mergePreviewError.value = "";
  try {
    mergePreview.value = await previewMerge(
      repoPath.value,
      mergeDialog.branch,
      mergeDialog.mode as "normal" | "no-ff" | "squash",
    );
  } catch (e) {
    mergePreview.value = null;
    mergePreviewError.value = errMsg(e);
  } finally {
    mergePreviewLoading.value = false;
  }
}

watch(
  () => mergeDialog.show,
  (show) => {
    if (show) loadMergePreview();
  },
);

watch(
  () => mergeDialog.mode,
  () => {
    if (mergeDialog.show) loadMergePreview();
  },
);

/** 预演类型的中文标签。 */
function mergeKindLabel(kind: string): string {
  switch (kind) {
    case "fast_forward":
      return "可快进（不产生合并提交）";
    case "up_to_date":
      return "已是最新（无需合并）";
    default:
      return "将创建合并提交";
  }
}

/** 预演类型的补充说明（无预演 / 无补充时为空）。 */
const mergePreviewKindDetail = computed(() => {
  const p = mergePreview.value;
  if (!p) return "";
  if (p.kind === "up_to_date") return "目标分支的提交已全部在当前分支中。";
  if (p.kind === "fast_forward") return "当前分支落后且无分叉，Merge 只会移动分支指针。";
  if (p.incomingCommits.length < p.incomingCount || p.affectedFiles.length < p.affectedFilesCount) {
    return `列表为截断显示：提交显示前 ${p.incomingCommits.length}/${p.incomingCount} 个，文件显示前 ${p.affectedFiles.length}/${p.affectedFilesCount} 个。`;
  }
  return "";
});
const rebaseDialogVisible = ref(false);
const mergeInProgress = ref(false);
const rebaseState = ref<RebaseState | null>(null);

// Smart merge dialog state
const smartMerge = reactive({
  show: false,
  conflicts: [] as string[],
  baseOid: null as string | null,
});

/** Onto candidates: every local branch except the current one, plus remotes. */
const rebaseRevisions = computed<string[]>(() => {
  if (!overview.value) return [];
  return [
    ...overview.value.locals.filter((b) => !b.isCurrent).map((b) => b.name),
    ...overview.value.remotes.map((r) => r.name),
  ];
});

const defaultOnto = computed<string>(() => {
  const opts = rebaseRevisions.value;
  return (
    opts.find((n) => n === "master") ??
    opts.find((n) => n === "main") ??
    opts[0] ??
    ""
  );
});

const currentOpLabel = computed<string>(() => {
  const s = rebaseState.value;
  if (!s || s.position >= s.ops.length) return "—";
  const op = s.ops[s.position];
  return `${op.action} ${op.oid.slice(0, 7)} ${op.subject}`;
});

const compare = reactive<{
  show: boolean;
  base: string;
  other: string;
  loading: boolean;
  result: CompareResult | null;
  tab: "ahead" | "behind" | "files";
  selectedFile: FileDiff | null;
}>({
  show: false,
  base: "",
  other: "",
  loading: false,
  result: null,
  tab: "ahead",
  selectedFile: null,
});

/** Compare revisions: local + remote branch names and tags. */
const revisionOptions = computed<string[]>(() => {
  if (!overview.value) return [];
  return [
    ...overview.value.locals.map((b) => b.name),
    ...overview.value.remotes.map((r) => r.name),
    ...overview.value.tags.map((t) => t.name),
  ];
});

/** n-select compatible options array for revision selectors. */
const revisionSelectOptions = computed(() =>
  revisionOptions.value.map((o) => ({ label: o, value: o })),
);

/** n-dropdown options for local branch rows. */
function localBranchOptions(b: BranchEntry) {
  const opts: { label: string; key: string; props?: Record<string, unknown> }[] = [];
  if (!b.isCurrent) opts.push({ label: "Checkout", key: "checkout" });
  opts.push({ label: "Rename", key: "rename" });
  opts.push({ label: "Set Upstream", key: "set-upstream" });
  if (b.isCurrent) opts.push({ label: "Pull（--ff-only）", key: "pull" });
  opts.push({ label: "Push", key: "push" });
  opts.push({ label: "Compare", key: "compare" });
  opts.push({ label: "Merge 到当前分支…", key: "merge" });
  if (!b.isCurrent) {
    opts.push({ type: "divider", key: "d1" } as never);
    opts.push({ label: "Delete", key: "delete", props: { style: "color: var(--gw-danger)" } });
  }
  return opts;
}

/** n-dropdown options for remote branch rows. */
function remoteBranchOptions() {
  return [
    { label: "Track（检出为本地分支）", key: "track" },
    { label: "Compare", key: "compare" },
  ];
}

/** n-dropdown options for tag rows (GF-04). */
function tagOptions() {
  return [
    { label: "Push…", key: "push" },
    { type: "divider", key: "d1" } as never,
    { label: "Delete", key: "delete", props: { style: "color: var(--gw-danger)" } },
  ];
}

onMounted(async () => {
  // F-14/F-17：query → 全局当前仓库 → 工作区首仓库兜底（SideNav 直达）。
  const repo = await resolveCurrentRepo();
  if (!repo) {
    message.warning("当前工作区没有可用仓库，请先在变更页扫描");
    router.push({ name: "changes" });
    return;
  }
  repoPath.value = repo;
  await load();
});

async function load() {
  loading.value = true;
  try {
    overview.value = await listBranches(repoPath.value);
    // Resume surface for an interrupted merge/rebase (T-15 restart recovery).
    mergeInProgress.value = await getMergeInProgress(repoPath.value);
    rebaseState.value = await getRebaseState(repoPath.value);
  } catch (e) {
    message.error("获取分支列表失败: " + errMsg(e));
  } finally {
    loading.value = false;
  }
  void detectRemoteSilently();
}

// F-22：切换仓库后重置视图状态并重载。
async function onRepoSwitch(path: string) {
  repoPath.value = path;
  overview.value = null;
  mergeInProgress.value = false;
  rebaseState.value = null;
  compare.show = false;
  await load();
}

function openResolver() {
  router.push({ name: "conflict-resolver", query: { repo: repoPath.value } });
}

function shortOid(oid: string): string {
  return oid ? oid.slice(0, 7) : "";
}

function statusIcon(status: string): string {
  switch (status) {
    case "added":
    case "untracked":
      return "A";
    case "deleted":
      return "D";
    case "modified":
      return "M";
    case "renamed":
      return "R";
    default:
      return "?";
  }
}

// ---------------------------------------------------------------------------
// Local branch commands (§46: Delete = Dangerous 二次确认; Push = Warning 确认)
// ---------------------------------------------------------------------------

async function handleLocalCommand(cmd: string, b: BranchEntry) {
  switch (cmd) {
    case "checkout":
      // R-21 §49：有 Runtime 应用运行中 → Stop & Switch / Cancel 确认。
      if (!(await guardRuntimeRunning(dialog, message))) return;
      await runOp(`已切换到分支 ${b.name}`, () => checkoutBranch(repoPath.value, b.name));
      break;
    case "rename":
      await handleRename(b);
      break;
    case "set-upstream":
      await handleSetUpstream(b);
      break;
    case "push":
      await handlePush(b);
      break;
    case "pull":
      await handleSmartPull();
      break;
    case "compare":
      openCompare(b.name);
      break;
    case "merge":
      mergeDialog.branch = b.name;
      mergeDialog.mode = "normal";
      mergeDialog.show = true;
      break;
    case "delete":
      await handleDelete(b);
      break;
  }
}

async function handleRemoteCommand(cmd: string, r: RemoteBranchEntry) {
  switch (cmd) {
    case "track":
      await runOp(`已创建跟踪分支（${r.name}）`, () =>
        trackRemoteBranch(repoPath.value, r.name),
      );
      break;
    case "compare":
      openCompare(r.name);
      break;
  }
}

// ---------------------------------------------------------------------------
// GF-04 tag commands (§46: Delete = Dangerous 二次确认; Push = Warning 确认,
// force 默认关闭)
// ---------------------------------------------------------------------------

async function handleTagCommand(cmd: string, t: TagEntry) {
  if (cmd === "push") await handleTagPush(t);
  else if (cmd === "delete") await handleTagDelete(t);
}

/** 面板级「新建标签」入口：目标固定当前 HEAD（后端支持传 target，UI 暂不暴露）。 */
function handleCreateTag() {
  tagDialog.name = "";
  tagDialog.message = "";
  tagDialog.show = true;
}

async function runCreateTag() {
  const name = tagDialog.name.trim();
  if (!TAG_NAME_PATTERN.test(name)) {
    message.error("标签名不合法");
    return;
  }
  tagDialog.loading = true;
  try {
    await runOp(`已创建标签 ${name}`, () =>
      createTag(repoPath.value, name, tagDialog.message.trim() || undefined),
    );
    tagDialog.show = false;
  } finally {
    tagDialog.loading = false;
  }
}

function handleTagPush(t: TagEntry) {
  tagPushDialog.name = t.name;
  tagPushDialog.targetOid = t.targetOid;
  tagPushDialog.force = false;
  tagPushDialog.forceWithLease = true;
  tagPushDialog.show = true;
}

async function runTagPush() {
  tagPushDialog.loading = true;
  try {
    await pushTag(
      repoPath.value,
      tagPushDialog.name,
      tagPushDialog.force,
      tagPushDialog.force ? tagPushDialog.forceWithLease : false,
    );
    tagPushDialog.show = false;
    message.success(`已推送标签 ${tagPushDialog.name}`);
    await load();
  } catch (e) {
    message.error("Push 标签失败: " + errMsg(e));
  } finally {
    tagPushDialog.loading = false;
  }
}

/**
 * Dangerous (§46)：删除本地标签二次确认。先查远程标签名单——该标签已推送时
 * 必须在弹窗里明示「此标签已推送到远程」，避免用户误以为远程副本一并消失。
 * 查询失败（离线 / 无远程）时用通用文案兜底（本地删除本身仍不可撤销）。
 */
async function handleTagDelete(t: TagEntry) {
  let onRemote: boolean | null = null;
  const pending = message.loading("查询远程标签…");
  try {
    onRemote = await tagPushedToRemote(repoPath.value, t.name);
  } catch {
    onRemote = null;
  } finally {
    pending.destroy();
  }

  const pushedNote = onRemote
    ? "⚠ 此标签已推送到远程仓库——删除本地不会删除远程副本。"
    : "本地删除不可撤销（远程副本不受影响）。";
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.error({
        title: "删除标签确认（Dangerous）",
        content:
          `仓库：${repoPath.value}\n` +
          `确认删除本地标签 ${t.name}？\n${pushedNote}\n` +
          `目标提交 ${shortOid(t.targetOid)}。`,
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
  await runOp(`已删除标签 ${t.name}`, () => deleteTag(repoPath.value, t.name));
}

/** Run an op, toast the result, reload on success. */
async function runOp(successMsg: string, op: () => Promise<unknown>) {
  try {
    await op();
    message.success(successMsg);
    await load();
  } catch (e) {
    message.error(errMsg(e));
  }
}

async function handleCreate() {
  try {
    const name = await prompt(dialog, {
      title: "新建分支",
      content: "新分支名称（基于当前 HEAD；可在下方输入框留空目标）",
      confirmText: "创建",
      cancelText: "取消",
      pattern: /^[^\s~^:?*[\]\\]+$/,
      patternError: "分支名不合法",
    });
    if (!name) return;
    await runOp(`已创建分支 ${name}`, () => createBranch(repoPath.value, name));
  } catch (e) {
    if (e !== "cancel") message.error("创建分支失败: " + errMsg(e));
  }
}

async function handleRename(b: BranchEntry) {
  try {
    const newName = await prompt(dialog, {
      title: "Rename Branch",
      content: `将分支 ${b.name} 重命名为：`,
      confirmText: "重命名",
      cancelText: "取消",
      defaultValue: b.name,
      pattern: /^[^\s~^:?*[\]\\]+$/,
      patternError: "分支名不合法",
    });
    if (!newName || newName === b.name) return;
    await runOp(`已重命名为 ${newName}`, () =>
      renameBranch(repoPath.value, b.name, newName),
    );
  } catch (e) {
    if (e !== "cancel") message.error("重命名失败: " + errMsg(e));
  }
}

async function handleSetUpstream(b: BranchEntry) {
  if (!overview.value) return;
  const options = overview.value.remotes.map((r) => r.name);
  try {
    const value = await prompt(dialog, {
      title: "Set Upstream",
      content: `设置 ${b.name} 的上游（输入远程分支名，如 origin/main；输入 "-" 清除上游）：`,
      confirmText: "确定",
      cancelText: "取消",
      defaultValue: b.upstream ?? (options.length === 1 ? options[0] : ""),
      pattern: /^.+$/,
      patternError: "必须是现有远程分支名，或 - 清除",
    });
    // Manual validation for custom logic
    if (value !== "-" && !options.includes(value)) {
      message.error("必须是现有远程分支名，或 - 清除");
      return;
    }
    const upstream = value === "-" ? undefined : value;
    await runOp(
      upstream ? `已设置上游 ${upstream}` : "已清除上游",
      () => setUpstream(repoPath.value, b.name, upstream),
    );
  } catch (e) {
    if (e !== "cancel") message.error("设置上游失败: " + errMsg(e));
  }
}

async function handleSmartPull() {
  try {
    const result = await smartPull(repoPath.value);
    if (result.status === "conflict") {
      smartMerge.conflicts = result.files;
      smartMerge.baseOid = result.baseOid;
      smartMerge.show = true;
    } else if (result.status === "upToDate") {
      message.info("已是最新");
      await load();
    } else {
      message.success("Pull 完成");
      await load();
    }
  } catch (e) {
    message.error("Pull 失败: " + errMsg(e));
  }
}

function onSmartMergeResolved() {
  smartMerge.show = false;
  smartMerge.conflicts = [];
  smartMerge.baseOid = null;
  load();
}

function onSmartMergeAborted() {
  smartMerge.show = false;
  smartMerge.conflicts = [];
  smartMerge.baseOid = null;
  load();
}

async function handlePush(b: BranchEntry) {
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.warning({
        title: "Push 确认",
        content: `推送本地分支 ${b.name} 到 ${b.upstream ?? "默认远程"}？（不启用 force）`,
        positiveText: "Push",
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
    const output = await pushBranch(repoPath.value, b.name);
    message.success(output ? `Push 完成：${output}` : "Push 完成");
    await load();
  } catch (e) {
    message.error("Push 失败: " + errMsg(e));
  }
}

/** Dangerous op (§46): 二次确认；未合入时升级为强制删除确认。 */
async function handleDelete(b: BranchEntry) {
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.error({
        title: "Delete 确认（Dangerous）",
        content: `确认删除本地分支 ${b.name}？此操作不可撤销（可用 reflog 尝试找回）。`,
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
    await deleteBranch(repoPath.value, b.name);
    message.success(`已删除分支 ${b.name}`);
    await load();
  } catch (e) {
    const msg = errMsg(e);
    if (msg.includes("not fully merged")) {
      // Second gate: force-delete confirmation for unmerged branches.
      try {
        await new Promise<void>((resolve, reject) => {
          dialog.error({
            title: "强制删除确认（Dangerous）",
            content: `分支 ${b.name} 未完全合入当前 HEAD，删除可能丢失提交。确认强制删除？`,
            positiveText: "强制删除",
            negativeText: "取消",
            onPositiveClick: () => resolve(),
            onNegativeClick: () => reject("cancel"),
            onClose: () => reject("cancel"),
          });
        });
      } catch {
        return;
      }
      await runOp(`已强制删除分支 ${b.name}`, () =>
        deleteBranch(repoPath.value, b.name, true),
      );
    } else {
      message.error("删除失败: " + msg);
    }
  }
}

// ---------------------------------------------------------------------------
// Merge / Rebase (T-15): Warning 确认 + 中断恢复
// ---------------------------------------------------------------------------

async function runMerge() {
  const { branch, mode } = mergeDialog;
  // Warning-level confirm (§46): history-changing op with impact scope.
  // GF-17: 二次确认正文附带结构化预演事实（并入提交数 / 影响文件数 / 冲突预判）。
  const p = mergePreview.value;
  const previewLine = p
    ? `\n将并入 ${p.incomingCount} 个提交，影响 ${p.affectedFilesCount} 个文件` +
      (p.conflictPredicted ? `；预判冲突（${p.conflictFiles.slice(0, 3).join("、")}${p.conflictFiles.length > 3 ? " 等" : ""}）` : "；未预判冲突") +
      (p.dirtyBlocked ? `\n注意：工作区有 ${p.dirtyFilesCount} 个未提交变更，Merge 会被拒绝` : "")
    : "";
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.warning({
        title: "Merge 确认（Warning）",
        content: `仓库：${repoPath.value}\n将把分支 ${branch} 合并到当前分支 ${overview.value?.current ?? "HEAD"}（模式：${mode}）。${previewLine}\n若产生冲突，仓库会进入 Merge 状态，可解决后继续或中止恢复。`,
        positiveText: "执行 Merge",
        negativeText: "取消",
        onPositiveClick: () => resolve(),
        onNegativeClick: () => reject("cancel"),
        onClose: () => reject("cancel"),
      });
    });
  } catch {
    return;
  }

  mergeDialog.loading = true;
  try {
    const outcome = await mergeBranch(repoPath.value, branch, mode);
    mergeDialog.show = false;
    handleMergeOutcome(outcome);
    await load();
  } catch (e) {
    message.error("Merge 失败: " + errMsg(e));
  } finally {
    mergeDialog.loading = false;
  }
}

function handleMergeOutcome(outcome: MergeOutcome) {
  switch (outcome.status) {
    case "upToDate":
      message.info("已是最新，无需合并");
      break;
    case "fastForward":
      message.success(`已快进到 ${outcome.to.slice(0, 7)}`);
      break;
    case "merged":
      message.success(`合并完成（${outcome.commitOid.slice(0, 7)}）`);
      break;
    case "squashed":
      message.success("Squash 结果已暂存，请在变更视图提交");
      break;
    case "conflict":
      message.warning(
        `合并冲突（${outcome.files.length} 个文件）：${outcome.files.join("、")}。请在变更视图解决后回来 Continue，或 Abort 恢复。`,
      );
      break;
  }
}

async function handleMergeContinue() {
  try {
    const oid = await mergeContinue(repoPath.value);
    message.success(`Merge 已完成（${oid.slice(0, 7)}）`);
    await load();
  } catch (e) {
    message.error(errMsg(e));
  }
}

async function handleMergeAbort() {
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.error({
        title: "Merge Abort 确认（Dangerous）",
        content: `仓库：${repoPath.value}\n将放弃本次合并并恢复到合并前状态（hard reset），冲突中的修改将丢失。`,
        positiveText: "中止并恢复",
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
    await mergeAbort(repoPath.value);
    message.success("已中止 Merge 并恢复");
    await load();
  } catch (e) {
    message.error("Abort 失败: " + errMsg(e));
  }
}

async function onRebaseFinished(outcome: RebaseOutcome | null) {
  if (!outcome) return;
  if (outcome.status === "success") {
    message.success(`Rebase 完成（重写 ${outcome.rewritten} 个提交）`);
  } else {
    message.warning(
      `Rebase 在第 ${outcome.position + 1}/${outcome.total} 步冲突（${outcome.files.join("、")}）。解决后可 Continue / Skip / Abort。`,
    );
  }
  await load();
}

async function handleRebaseContinue() {
  try {
    const outcome = await rebaseContinue(repoPath.value);
    if (outcome.status === "success") {
      message.success(`Rebase 完成（重写 ${outcome.rewritten} 个提交）`);
    } else {
      message.warning(
        `第 ${outcome.position + 1}/${outcome.total} 步再次冲突：${outcome.files.join("、")}`,
      );
    }
    await load();
  } catch (e) {
    message.error(errMsg(e));
  }
}

async function handleRebaseSkip() {
  try {
    const outcome = await rebaseSkip(repoPath.value);
    if (outcome.status === "success") {
      message.success(`Rebase 完成（重写 ${outcome.rewritten} 个提交）`);
    } else {
      message.warning(
        `第 ${outcome.position + 1}/${outcome.total} 步再次冲突：${outcome.files.join("、")}`,
      );
    }
    await load();
  } catch (e) {
    message.error("Skip 失败: " + errMsg(e));
  }
}

async function handleRebaseAbort() {
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.error({
        title: "Rebase Abort 确认（Dangerous）",
        content: `仓库：${repoPath.value}\n将放弃本次 Rebase 并恢复到 rebase 前位置（hard reset 到 ${rebaseState.value?.originalHead.slice(0, 7) ?? "?"}），进行中的修改将丢失。`,
        positiveText: "中止并恢复",
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
    await rebaseAbort(repoPath.value);
    message.success("已中止 Rebase 并恢复");
    await load();
  } catch (e) {
    message.error("Abort 失败: " + errMsg(e));
  }
}

// ---------------------------------------------------------------------------
// Compare
// ---------------------------------------------------------------------------

function openCompare(presetOther?: string) {
  compare.base = overview.value?.current ?? overview.value?.locals[0]?.name ?? "";
  compare.other = presetOther ?? "";
  compare.result = null;
  compare.selectedFile = null;
  compare.tab = "ahead";
  compare.show = true;
}

async function runCompare() {
  compare.loading = true;
  compare.selectedFile = null;
  try {
    compare.result = await compareBranches(repoPath.value, compare.base, compare.other);
    compare.tab = "ahead";
  } catch (e) {
    message.error("Compare 失败: " + errMsg(e));
  } finally {
    compare.loading = false;
  }
}
</script>

<style scoped>
.branch-manager {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.branch-header {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
  padding: 8px 16px;
  border-bottom: 1px solid var(--gw-border);
  background: var(--gw-bg-panel);
}

.repo-info {
  flex: 1;
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  min-width: 0;
}

.repo-path {
  font-size: 14px;
  font-weight: 500;
  font-family: var(--gw-font-mono);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.branch-body {
  flex: 1;
  overflow-y: auto;
  padding: var(--gw-space-3) var(--gw-space-4);
  background: var(--gw-bg-hover);
}

/* D-10：section 外壳已替换为 Panel 组件；间距沿用原列表节奏 */
.branch-section {
  margin-bottom: 12px;
}

/* Panel 标题行下加分隔线，对齐原 .section-title 视觉 */
.branch-section :deep(.panel-header) {
  border-bottom: 1px solid var(--gw-border);
}

/* GF-11：VirtualList 定高行（BRANCH_ROW_H）——分支行显式撑满行包裹层，
   内容超出省略（box-sizing:border-box 下 padding 计入高度）。 */
.branch-row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
  padding: 6px 12px;
  border-bottom: 1px solid var(--gw-border);
  font-size: 13px;
  height: 100%;
  overflow: hidden;
}

/* 分支行是固定栅格 + ellipsis，覆盖 VirtualList 默认的 max-content 宽度，
   否则长 message 会把行撑出横向滚动条。 */
.branch-list :deep(.virtual-list-spacer),
.branch-list :deep(.virtual-list-window) {
  width: 100%;
}

.branch-row.current {
  background: var(--gw-bg-hover);
}

.branch-name {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 220px;
  font-weight: 500;
  font-family: var(--gw-font-mono);
  flex-shrink: 0;
}

.branch-track {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 200px;
  flex-shrink: 0;
}

.upstream {
  color: var(--gw-text-dim);
}

.ahead {
  color: var(--gw-success);
  font-weight: 600;
}

.behind {
  color: var(--gw-warning);
  font-weight: 600;
}

.no-upstream {
  color: var(--gw-text-dim);
  font-size: 12px;
}

.tag-message {
  color: var(--gw-text-dim);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.branch-commit {
  flex: 1;
  color: var(--gw-text-dim);
  font-family: var(--gw-font-mono);
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.danger-item {
  color: var(--gw-danger);
}

/* GF-04：标签创建 / 推送对话框 */
.tag-form {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3);
}

.tag-field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}

.tag-label {
  font-size: 12px;
  color: var(--gw-text-dim);
}

.tag-value {
  font-family: var(--gw-font-mono);
  font-size: 13px;
  word-break: break-all;
}

.tag-hint {
  font-size: 12px;
  color: var(--gw-text-dim);
  font-family: var(--gw-font-mono);
}

.tag-force-alert {
  font-size: 12px;
}

.compare-form {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
  margin-bottom: 12px;
}

.compare-arrow {
  color: var(--gw-text-dim);
}

.compare-summary {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  margin-bottom: 8px;
}

.summary-text {
  color: var(--gw-text-dim);
  font-size: 13px;
  margin-right: 8px;
}

.commit-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 4px 8px;
  font-size: 13px;
  border-bottom: 1px solid var(--gw-border);
}

.commit-oid {
  font-family: var(--gw-font-mono);
  color: var(--gw-accent);
  width: 70px;
  flex-shrink: 0;
}

.commit-msg {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.commit-meta {
  color: var(--gw-text-dim);
  font-size: 12px;
  flex-shrink: 0;
}

.compare-files {
  display: flex;
  height: 50vh;
  border: 1px solid var(--gw-border);
}

.file-list {
  width: 260px;
  border-right: 1px solid var(--gw-border);
  overflow-y: auto;
}

.file-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 10px;
  cursor: pointer;
  font-size: 13px;
  border-bottom: 1px solid var(--gw-border);
}

.file-item:hover {
  background: var(--gw-bg-hover);
}

.file-item.active {
  background: var(--gw-bg-hover);
}

.file-status-icon {
  width: 16px;
  text-align: center;
  font-weight: bold;
  flex-shrink: 0;
}

.file-status-icon.added,
.file-status-icon.untracked {
  color: var(--gw-success);
}

.file-status-icon.deleted {
  color: var(--gw-danger);
}

.file-status-icon.modified {
  color: var(--gw-warning);
}

.file-status-icon.renamed {
  color: var(--gw-text-dim);
}

.file-name {
  font-family: var(--gw-font-mono);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file-diff {
  flex: 1;
  overflow: hidden;
}

/* T-29：Create PR 对话框 */
.pr-form {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3);
}

.pr-row {
  display: flex;
  gap: var(--gw-space-3);
}

.pr-field {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}

.pr-label {
  font-size: 12px;
  color: var(--gw-text-dim);
}

.pr-stats {
  font-size: 12px;
  color: var(--gw-text-dim);
  font-family: var(--gw-font-mono);
}

.pr-actions {
  display: flex;
  gap: var(--gw-space-2);
}

.pr-token-alert {
  font-size: 12px;
}
</style>

<style scoped>
.state-banner {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 16px;
  font-size: 13px;
  border-bottom: 1px solid var(--gw-danger);
}

.state-banner.merge {
  background: var(--gw-warning);
  border-bottom-color: var(--gw-warning);
}

.state-banner.rebase {
  background: var(--gw-danger);
}

.banner-text {
  font-weight: 500;
  color: var(--gw-text-dim);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.banner-hint {
  color: var(--gw-text-dim);
  font-size: 12px;
}

.merge-line {
  margin-bottom: 12px;
  font-size: 13px;
  color: var(--gw-text-dim);
}

.merge-modes {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
}

/* GF-17：结构化预演区块（Roadmap §46 Repository/Branch/Files/Potential Data Loss） */
.merge-preview {
  display: block;
  margin-top: var(--gw-space-3);
  padding-top: var(--gw-space-3);
  border-top: 1px solid var(--gw-border);
  min-height: 24px;
}

.merge-preview-line {
  font-size: 13px;
  line-height: 1.8;
}

.merge-preview-list {
  margin: var(--gw-space-1) 0;
  padding-left: var(--gw-space-4);
  max-height: 140px;
  overflow-y: auto;
  font-size: 12px;
  line-height: 1.7;
}

.merge-preview-msg {
  margin-left: var(--gw-space-2);
  color: var(--gw-text-dim);
}

.merge-preview-error {
  font-size: 12px;
  color: var(--gw-warning);
}

.danger-text {
  color: var(--gw-danger);
}

.dim-text {
  color: var(--gw-text-dim);
}

.mono {
  font-family: var(--gw-font-mono);
}
</style>
