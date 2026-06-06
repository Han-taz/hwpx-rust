#[path = "../benches/hwpx_bench_data.rs"]
mod hwpx_bench_data;

#[test]
fn synthetic_hwpx_benchmark_document_parses_and_converts() {
    let data = hwpx_bench_data::synthetic_hwpx_document(2, 3, 2);
    let document = hwp_core::parser::hwpx::parse(&data)
        .expect("synthetic HWPX benchmark document should parse");

    assert_eq!(document.body_text.sections.len(), 2);
    assert_eq!(
        document
            .body_text
            .sections
            .iter()
            .map(|section| section.paragraphs.len())
            .sum::<usize>(),
        6
    );

    let markdown = hwp_core::viewer::markdown::to_markdown(&document, &Default::default());
    assert!(markdown.contains("section 1 paragraph 2 run 1"));
}

#[test]
fn synthetic_hwpx_table_benchmark_document_parses_and_converts() {
    let data = hwpx_bench_data::synthetic_hwpx_table_document(2, 3, 4);
    let document = hwp_core::parser::hwpx::parse(&data)
        .expect("synthetic table HWPX benchmark document should parse");

    assert_eq!(document.body_text.sections.len(), 2);
    assert_eq!(
        document
            .body_text
            .sections
            .iter()
            .map(|section| section.paragraphs.len())
            .sum::<usize>(),
        2
    );

    let markdown = hwp_core::viewer::markdown::to_markdown(&document, &Default::default());
    assert!(markdown.contains("cell 1 2 3"));
}
