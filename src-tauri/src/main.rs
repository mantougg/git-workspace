// Prevents additional console window on Windows in release, DO NOT REMOVE.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // AI-12 CLI Adapter：`git-workspace ai-tools ...` 无头运行后直接退出，
    // 不启动 GUI（list 离线可用，call 中继到运行中的应用实例）。
    let cli_args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(exit_code) = git_workspace_lib::ai::external::cli::run_cli(cli_args) {
        std::process::exit(exit_code);
    }
    git_workspace_lib::run();
}
