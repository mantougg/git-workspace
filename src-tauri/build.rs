fn main() {
    tauri_build::build();

    // Windows 单测 exe 清单注入（0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND 修复）。
    //
    // 背景：tauri 应用清单只内嵌进 app bin（embed-resource 作用于 bin target），
    // `cargo test` 的测试 exe 没有清单 → comctl32 解析为 System32 v5.82，缺少
    // rfd（tauri-plugin-dialog）需要的 TaskDialogIndirect 导出，进程加载即失败，
    // Windows 上全部单测无法运行。注入 test.manifest（comctl32 v6 依赖）解决。
    //
    // 必须门控：该链接参数对所有 target 生效，app bin 已由 tauri 内嵌清单，
    // 重复注入会 CVT1100 RT_MANIFEST 资源冲突——故仅在 GW_TEST_MANIFEST=1 时
    // 注入（build script 无法区分当前构建的是 bin 还是 test target）。
    //
    // 用法（Git Bash）：GW_TEST_MANIFEST=1 cargo test --lib
    //      （cmd.exe）：set GW_TEST_MANIFEST=1 && cargo test --lib
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("GW_TEST_MANIFEST").is_ok()
    {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{manifest_dir}\\test.manifest");
    }
    println!("cargo:rerun-if-env-changed=GW_TEST_MANIFEST");
    println!("cargo:rerun-if-changed=test.manifest");
}
