<template>
  <el-dialog
    :model-value="modelValue"
    @update:model-value="emit('update:modelValue', $event)"
    title="添加工作区"
    width="500px"
    :close-on-click-modal="false"
  >
    <el-form ref="formRef" :model="form" :rules="rules" label-width="100px">
      <el-form-item label="工作区名称" prop="name">
        <el-input v-model="form.name" placeholder="例如：公司项目" />
      </el-form-item>
      <el-form-item label="目录路径" prop="path">
        <el-input v-model="form.path" placeholder="选择工作区目录">
          <template #append>
            <el-button @click="selectDirectory">浏览</el-button>
          </template>
        </el-input>
      </el-form-item>
      <el-form-item label="扫描深度">
        <el-input-number
          v-model="form.scanDepth"
          :min="1"
          :max="20"
          :step="1"
        />
        <span class="tip">子目录递归层数</span>
      </el-form-item>
    </el-form>
    <template #footer>
      <el-button @click="emit('update:modelValue', false)">取消</el-button>
      <el-button type="primary" :loading="submitting" @click="handleSubmit">
        添加
      </el-button>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { reactive, ref } from "vue";
import { ElMessage, type FormInstance, type FormRules } from "element-plus";
import { open } from "@tauri-apps/plugin-dialog";
import { useWorkspaceStore } from "@/stores/workspace";
import { errMsg } from "@/utils/error";

defineProps<{
  modelValue: boolean;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: boolean];
  added: [];
}>();

const workspaceStore = useWorkspaceStore();
const formRef = ref<FormInstance>();
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
  await formRef.value.validate(async (valid) => {
    if (!valid) return;
    submitting.value = true;
    try {
      await workspaceStore.addWorkspace({
        name: form.name,
        path: form.path,
        scanDepth: form.scanDepth,
      });
      ElMessage.success("工作区添加成功");
      emit("update:modelValue", false);
      emit("added");
      form.name = "";
      form.path = "";
      form.scanDepth = 5;
    } catch (e) {
      ElMessage.error("添加失败: " + errMsg(e));
    } finally {
      submitting.value = false;
    }
  });
}
</script>

<style scoped>
.tip {
  margin-left: 8px;
  color: #909399;
  font-size: 12px;
}
</style>
