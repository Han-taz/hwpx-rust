use criterion::{criterion_group, criterion_main, Criterion};
use std::fs;

mod hwpx_bench_data;

fn find_hwpx_fixture() -> Option<Vec<u8>> {
    let paths = [
        "tests/fixtures",
        "tests/snapshots/packages",
        "tests/snapshots/crates",
    ];
    for base in paths {
        if let Ok(entries) = fs::read_dir(base) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "hwpx").unwrap_or(false) {
                    if let Ok(data) = fs::read(&path) {
                        return Some(data);
                    }
                }
            }
        }
    }
    None
}

fn bench_parse(c: &mut Criterion) {
    let synthetic_data = hwpx_bench_data::synthetic_hwpx_document(4, 250, 4);
    c.bench_function("parse_hwpx_synthetic", |b| {
        b.iter(|| {
            let _ = hwp_core::parser::hwpx::parse(&synthetic_data);
        });
    });

    let synthetic_doc = hwp_core::parser::hwpx::parse(&synthetic_data)
        .expect("synthetic HWPX benchmark document should parse");
    c.bench_function("to_markdown_synthetic", |b| {
        let options = hwp_core::viewer::markdown::MarkdownOptions::default();
        b.iter(|| {
            let _ = hwp_core::viewer::markdown::to_markdown(&synthetic_doc, &options);
        });
    });

    let synthetic_table_data = hwpx_bench_data::synthetic_hwpx_table_document(3, 80, 8);
    c.bench_function("parse_hwpx_synthetic_table", |b| {
        b.iter(|| {
            let _ = hwp_core::parser::hwpx::parse(&synthetic_table_data);
        });
    });

    let synthetic_table_doc = hwp_core::parser::hwpx::parse(&synthetic_table_data)
        .expect("synthetic table HWPX benchmark document should parse");
    c.bench_function("to_markdown_synthetic_table", |b| {
        let options = hwp_core::viewer::markdown::MarkdownOptions::default();
        b.iter(|| {
            let _ = hwp_core::viewer::markdown::to_markdown(&synthetic_table_doc, &options);
        });
    });

    if let Some(data) = find_hwpx_fixture() {
        c.bench_function("parse_hwpx", |b| {
            b.iter(|| {
                let _ = hwp_core::parser::hwpx::parse(&data);
            });
        });

        if let Ok(doc) = hwp_core::parser::hwpx::parse(&data) {
            c.bench_function("to_markdown", |b| {
                let options = hwp_core::viewer::markdown::MarkdownOptions::default();
                b.iter(|| {
                    let _ = hwp_core::viewer::markdown::to_markdown(&doc, &options);
                });
            });
        }
    } else {
        eprintln!("No .hwpx fixture found — skipping benchmarks");
    }
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
