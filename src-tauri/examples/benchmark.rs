//! Benchmark entry point: `cargo run --release --example benchmark -- <count> [--json]`
//!
//! - `<count>`  — number of synthetic repositories (default 100).
//! - `--json`  — print the structured result as JSON instead of Markdown.
//!
//! The Markdown path also persists a per-count baseline and, when a previous
//! baseline exists, prints a comparison against it.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let count = args
        .iter()
        .find_map(|a| a.parse::<usize>().ok())
        .unwrap_or(100);
    let json = args.iter().any(|a| a == "--json");

    let result = git_workspace_lib::benchmark::run(count);

    if json {
        println!("{}", git_workspace_lib::benchmark::to_json(&result));
        return;
    }

    print!("{}", git_workspace_lib::benchmark::format_report(&result));

    // Persist a baseline and, if one already exists, diff against it.
    let baseline_dir = std::env::temp_dir().join("gw_bench_baselines");
    if std::fs::create_dir_all(&baseline_dir).is_ok() {
        let baseline_path = baseline_dir.join(format!("baseline_{}.json", count));
        if let Some(prev) = git_workspace_lib::benchmark::load_baseline(&baseline_path) {
            print!("{}", git_workspace_lib::benchmark::format_comparison(&prev, &result));
        }
        let _ = git_workspace_lib::benchmark::save_baseline(&result, &baseline_path);
    }
}
