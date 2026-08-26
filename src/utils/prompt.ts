/**
 * Naive UI prompt dialog utility.
 *
 * Element Plus provides ElMessageBox.prompt() which Naive UI lacks.
 * This utility wraps NDialog + NInput into a Promise-based prompt.
 */
import { h, ref } from "vue";
import { NInput } from "naive-ui";
import type { DialogReactive } from "naive-ui";

export interface PromptOptions {
  title: string;
  content?: string;
  placeholder?: string;
  defaultValue?: string;
  confirmText?: string;
  cancelText?: string;
  /** Regex pattern for validation */
  pattern?: RegExp;
  /** Error message shown when pattern validation fails */
  patternError?: string;
}

/**
 * Show a prompt dialog and return the user's input.
 * Resolves with the input value, or rejects with "cancel" if dismissed.
 */
export function prompt(
  dialog: DialogReactive | ReturnType<typeof import("naive-ui").useDialog>,
  options: PromptOptions,
): Promise<string> {
  return new Promise((resolve, reject) => {
    const inputValue = ref(options.defaultValue ?? "");
    const errorText = ref("");

    const d = (dialog as ReturnType<typeof import("naive-ui").useDialog>).create({
      title: options.title,
      content: () =>
        h("div", [
          options.content ? h("p", { style: "margin-bottom: 8px" }, options.content) : null,
          h(NInput, {
            value: inputValue.value,
            "onUpdate:value": (v: string) => {
              inputValue.value = v;
              errorText.value = "";
            },
            placeholder: options.placeholder ?? "",
            status: errorText.value ? "error" : undefined,
          }),
          errorText.value
            ? h("p", { style: "color: #d03050; font-size: 12px; margin-top: 4px" }, errorText.value)
            : null,
        ]),
      positiveText: options.confirmText ?? "确认",
      negativeText: options.cancelText ?? "取消",
      onPositiveClick: () => {
        const val = inputValue.value.trim();
        if (!val) {
          errorText.value = "输入不能为空";
          return false; // prevent close
        }
        if (options.pattern && !options.pattern.test(val)) {
          errorText.value = options.patternError ?? "输入不合法";
          return false; // prevent close
        }
        resolve(val);
      },
      onNegativeClick: () => reject("cancel"),
      onClose: () => reject("cancel"),
    });
  });
}
