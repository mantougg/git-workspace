//! Benchmark entry point: `cargo run --release --bin benchmark -- <count>`
fn main() {
    let count = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    print!("{}", git_workspace_lib::benchmark::run(count));
}
