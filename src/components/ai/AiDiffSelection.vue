<template>
  <div class="selection">
    <n-empty v-if="repositories.length === 0" description="没有可选择的变更仓库" />
    <n-collapse v-else>
      <n-collapse-item
        v-for="repository in repositories"
        :key="repository.repoPath"
        :name="repository.repoPath"
      >
        <template #header>
          <n-checkbox
            :checked="isRepositoryIncluded(repository.repoPath)"
            @click.stop
            @update:checked="(checked: boolean) => setRepository(repository.repoPath, checked)"
          >
            {{ repository.name }}
            <span class="count">{{ repository.files.length }} 个变更文件</span>
          </n-checkbox>
        </template>

        <div class="paths">
          <div v-for="group in groupsFor(repository)" :key="group.directory" class="path-group">
            <n-checkbox
              :checked="isPathIncluded(repository.repoPath, group.directory)"
              :disabled="!isRepositoryIncluded(repository.repoPath)"
              @update:checked="
                (checked: boolean) => setPath(repository.repoPath, group.directory, checked)
              "
            >
              <span class="directory">{{ group.directory }}</span>
              <span class="count">{{ group.files.length }} 个文件</span>
            </n-checkbox>
            <div class="files">
              <n-checkbox
                v-for="file in group.files"
                :key="file"
                :checked="isPathIncluded(repository.repoPath, file)"
                :disabled="!isRepositoryIncluded(repository.repoPath)"
                @update:checked="(checked: boolean) => setPath(repository.repoPath, file, checked)"
              >
                <span class="file mono">{{ file }}</span>
              </n-checkbox>
            </div>
          </div>
        </div>
      </n-collapse-item>
    </n-collapse>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import type { DiffRepositorySelection, GitDiffSelection } from "@/types/ai";

interface AiDiffRepositoryOption {
  repoPath: string;
  name: string;
  files: string[];
}

const props = defineProps<{
  repositories: AiDiffRepositoryOption[];
}>();

const selection = defineModel<GitDiffSelection>({
  default: () => ({ repositories: [] }),
});

const repositories = computed(() => props.repositories);

function emptySelection(repoPath: string): DiffRepositorySelection {
  return { repoPath, includePaths: [], excludePaths: [] };
}

function effectiveSelections(): DiffRepositorySelection[] {
  const current = selection.value.repositories;
  if (current.length > 0) return current.map((repo) => ({
    ...repo,
    includePaths: [...repo.includePaths],
    excludePaths: [...repo.excludePaths],
  }));
  return repositories.value.map((repo) => emptySelection(repo.repoPath));
}

function updateSelections(next: DiffRepositorySelection[]) {
  selection.value = { repositories: next };
}

function findSelection(repoPath: string, source = effectiveSelections()) {
  return source.find((repo) => repo.repoPath === repoPath) ?? emptySelection(repoPath);
}

function isRepositoryIncluded(repoPath: string): boolean {
  if (selection.value.repositories.length === 0) return true;
  return selection.value.repositories.some((repo) => repo.repoPath === repoPath);
}

function pathMatches(selector: string, path: string): boolean {
  const normalizedSelector = selector.replaceAll("\\", "/").replace(/^\.\//, "").replace(/\/+$/, "");
  const normalizedPath = path.replaceAll("\\", "/").replace(/^\.\//, "");
  return (
    normalizedSelector === "" ||
    normalizedSelector === "." ||
    normalizedPath === normalizedSelector ||
    normalizedPath.startsWith(`${normalizedSelector}/`)
  );
}

function isPathIncluded(repoPath: string, path: string): boolean {
  if (!isRepositoryIncluded(repoPath)) return false;
  const repo = findSelection(repoPath);
  const explicitlyIncluded =
    repo.includePaths.length === 0 || repo.includePaths.some((selector) => pathMatches(selector, path));
  return explicitlyIncluded && !repo.excludePaths.some((selector) => pathMatches(selector, path));
}

function setRepository(repoPath: string, included: boolean) {
  const next = effectiveSelections().filter((repo) => repo.repoPath !== repoPath);
  if (included) next.push(emptySelection(repoPath));
  updateSelections(next);
}

function setPath(repoPath: string, path: string, included: boolean) {
  const next = effectiveSelections();
  const repo = findSelection(repoPath, next);
  const remove = (items: string[]) => items.filter((item) => item !== path);
  if (included) {
    repo.excludePaths = remove(repo.excludePaths);
    if (repo.includePaths.length > 0 && !repo.includePaths.some((item) => pathMatches(item, path))) {
      repo.includePaths.push(path);
    }
  } else {
    repo.includePaths = remove(repo.includePaths);
    repo.excludePaths = [...repo.excludePaths.filter((item) => item !== path), path];
  }
  updateSelections(next);
}

function groupsFor(repository: AiDiffRepositoryOption) {
  const groups = new Map<string, string[]>();
  for (const file of repository.files) {
    const slash = file.lastIndexOf("/");
    const directory = slash > 0 ? file.slice(0, slash) : ".";
    const files = groups.get(directory) ?? [];
    files.push(file);
    groups.set(directory, files);
  }
  return [...groups.entries()]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([directory, files]) => ({ directory, files: files.sort() }));
}
</script>

<style scoped>
.selection {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
}

.paths,
.path-group {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-1);
}

.path-group {
  padding: var(--gw-space-1) 0;
}

.files {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-1);
  padding-left: var(--gw-space-5);
}

.directory {
  font-weight: 600;
}

.count {
  color: var(--gw-text-dim);
  font-size: var(--gw-text-xs);
  margin-left: var(--gw-space-1);
}

.mono {
  font-family: var(--gw-font-mono);
  font-size: var(--gw-text-xs);
}
</style>
