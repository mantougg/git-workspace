<template>
  <n-modal
    :show="modelValue"
    preset="card"
    title="添加工作区"
    style="width: 500px"
    :close-on-click-modal="false"
    @update:show="(v: boolean) => emit('update:modelValue', v)"
  >
    <n-form ref="formRef" :model="form" :rules="rules" label-width="100px">
      <n-form-item label="工作区名称" path="name">
        <n-input v-model:value="form.name" placeholder="例如：公司项目" />
      </n-form-item>
      <n-form-item label="目录路径" path="path">
        <n-input v-model:value="form.path" placeholder="选择工作区目录">
          <template #suffix>
            <n-button size="small" @click="selectDirectory">浏览</n-button>
          </template>
        </n-input>
      </n-form-item>
      <n-form-item label="扫描深度">
        <n-input-number
          v-model:value="form.scanDepth"
          :min="1"
          :max="20"
          :step="1"
        />
        <span class="tip">子目录递归层数</span>
      </n-form-item>
    </n-form>
    <template #footer>
      <n-button @click="emit('update:modelValue', false)">取消</n-button>
      <n-button type="primary" :loading="submitting" @click="handleSubmit">
        添加
      </n-button>
    </template>
  </n-modal>
</template>

<script setup lang="ts">
import { reactive, ref } from "vue";
import type { FormInst, FormRules } from "naive-ui";
import { useMessage } from "naive-ui";
import { open } from "@tauri-apps/plugin-dialog";
import { useWorkspaceStore } from "@/stores/workspace";
import { errMsg } from "@/utils/error";

const message = useMessage();

defineProps<{
  modelValue: boolean;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: boolean];
  added: [];
}>();

const workspaceStore = useWorkspaceStore();
const formRef = ref<FormInst | null>(null);
const submitting = ref(false);

const form = reactive({
  name: "",
  path: "",
  scanDepth: 5,
});

const rules: FormRules = {
  name: [{ required: true, message: "请输入工作区名称", trigger: "blur" }],
  path: [{ required: true, message: "请选择目录路径", trigger: "blur" }],
};

async function selectDirectory() {
  const selected = await open({
    directory: true,
    multiple: false,
    title: "选择工作区目录",
  });
  if (typeof selected === "string") {
    form.path = selected;
  }
}

async function handleSubmit() {
  if (!formRef.value) return;
  try {
    await formRef.value.validate();
  } catch {
    return;
  }
  submitting.value = true;
  try {
    await workspaceStore.addWorkspace({
      name: form.name,
      path: form.path,
      scanDepth: form.scanDepth,
    });
    message.success("工作区添加成功");
    emit("update:modelValue", false);
    emit("added");
    form.name = "";
    form.path = "";
    form.scanDepth = 5;
  } catch (e) {
    message.error("添加失败: " + errMsg(e));
  } finally {
    submitting.value = false;
  }
}
</script>

<style scoped>
.tip {
  margin-left: 8px;
  color: var(--gw-text-dim);
  font-size: 12px;
}
</style>
