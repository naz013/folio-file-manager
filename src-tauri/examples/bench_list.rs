//! Times `fs::list_dir` and its JSON encoding: `cargo run --release --example bench_list -- <dir>`.
#[path = "../src/fs/mod.rs"]
mod fs;

use std::time::Instant;

fn main() {
    let dir = std::env::args().nth(1).expect("usage: bench_list <dir>");
    for _ in 0..3 {
        let t = Instant::now();
        let entries = fs::list_dir(dir.as_ref()).unwrap();
        let list = t.elapsed();
        let t = Instant::now();
        let json = serde_json::to_vec(&entries).unwrap();
        println!(
            "{} entries: list {:.1} ms, json {:.1} ms ({} KB)",
            entries.len(),
            list.as_secs_f64() * 1e3,
            t.elapsed().as_secs_f64() * 1e3,
            json.len() / 1024
        );
    }
}
