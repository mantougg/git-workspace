//! Benchmark entry point: `cargo run --release --example benchmark -- <count> [--json]`
//!
//! - `<count>`  — number of synthetic repositories (default 100).
//! - `--json`  — print the structured result as JSON instead of Markdown.
//! - `diff-graph [commits]` — run the T-04 acceptance benchmarks instead
//!   (diff cache hit + graph first screen on a big repo, default 10000 commits).
//! - `runtime [repos] [modules]` — run the R-08 Runtime benchmark on a
//!   synthetic `repos × modules` Maven workspace (default 10 × 10), with §99
//!   PASS/FAIL verdicts. `runtime --matrix` runs the full §96 matrix
//!   (10/50/100 × 10/50/100, release build recommended).
//! - `build [repos] [modules]` — run the R-09 Build benchmark (real `mvn`,
//!   two Classpath Run builds on a self-contained synthetic workspace,
//!   default 2 × 2). No §99 budget: first/second build times are trends
//!   for human reading; skipped when no `mvn` is on PATH.
//!
//! The Markdown paths also persist baselines and, when a previous baseline
//! exists, print a comparison against it. Git benchmark baselines live in the
//! temp dir; R-08 Runtime baselines are archived in the repository at
//! `docs/tasks-runtime/benchmarks/` so trends survive across runs and machines
//! (override with `--baseline-dir <dir>`, skip with `--no-save`).

use git_workspace_lib::benchmark::runtime::{
    build_to_json, format_build_report, format_runtime_comparison, format_runtime_report, load_runtime_baseline,
    run_build_benchmark, run_runtime, runtime_baseline_path, runtime_to_json, save_runtime_baseline,
};
use git_workspace_lib::benchmark::{diff_graph_to_json, format_diff_graph_report, GRAPH_BIG_REPO_COMMITS};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");

    if args.iter().any(|a| a == "diff-graph") {
        let commits = args
            .iter()
            .find_map(|a| a.parse::<usize>().ok())
            .unwrap_or(GRAPH_BIG_REPO_COMMITS);
        let result = git_workspace_lib::benchmark::run_diff_graph(commits);
        if json {
            println!("{}", diff_graph_to_json(&result));
        } else {
            print!("{}", format_diff_graph_report(&result));
        }
        return;
    }

    if args.iter().any(|a| a == "runtime") {
        run_runtime_benchmark(&args, json);
        return;
    }

    if args.iter().any(|a| a == "build") {
        let numbers: Vec<usize> = args
            .iter()
            .skip_while(|a| *a != "build")
            .skip(1)
            .filter_map(|a| a.parse::<usize>().ok())
            .collect();
        let repos = numbers.first().copied().unwrap_or(2);
        let modules = numbers.get(1).copied().unwrap_or(2);
        match run_build_benchmark(repos, modules) {
            Some(result) => {
                if json {
                    println!("{}", build_to_json(&result));
                } else {
                    print!("{}", format_build_report(&result));
                }
            }
            None => println!("build benchmark skipped: no `mvn` on PATH"),
        }
        return;
    }

    let count = args.iter().find_map(|a| a.parse::<usize>().ok()).unwrap_or(100);

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

/// R-08 Runtime benchmark driver.
fn run_runtime_benchmark(args: &[String], json: bool) {
    // Positional numbers after the `runtime` keyword: repos, then modules.
    let numbers: Vec<usize> = args
        .iter()
        .skip_while(|a| *a != "runtime")
        .skip(1)
        .filter_map(|a| a.parse::<usize>().ok())
        .collect();

    // §96 matrix: 10 / 50 / 100 modules × 10 / 50 / 100 repositories.
    let points: Vec<(usize, usize)> = if args.iter().any(|a| a == "--matrix") {
        [10, 50, 100]
            .into_iter()
            .flat_map(|repos| [10, 50, 100].into_iter().map(move |modules| (repos, modules)))
            .collect()
    } else {
        vec![(
            numbers.first().copied().unwrap_or(10),
            numbers.get(1).copied().unwrap_or(10),
        )]
    };

    let no_save = args.iter().any(|a| a == "--no-save");
    let baseline_dir = args
        .iter()
        .position(|a| a == "--baseline-dir")
        .and_then(|pos| args.get(pos + 1))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(default_runtime_baseline_dir);

    for (repos, modules) in points {
        let result = run_runtime(repos, modules);
        if json {
            println!("{}", runtime_to_json(&result));
            continue;
        }

        print!("{}", format_runtime_report(&result));

        if no_save {
            continue;
        }
        let baseline_path = runtime_baseline_path(&baseline_dir, repos, modules);
        if let Some(prev) = load_runtime_baseline(&baseline_path) {
            print!("{}", format_runtime_comparison(&prev, &result));
        }
        if let Err(error) = save_runtime_baseline(&result, &baseline_path) {
            eprintln!("warning: could not save baseline {}: {error}", baseline_path.display());
        } else {
            println!("- Baseline saved: {}", baseline_path.display());
        }
    }
}

/// R-08 baselines are versioned with the repo so trends are traceable.
fn default_runtime_baseline_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("docs/tasks-runtime/benchmarks")
}
