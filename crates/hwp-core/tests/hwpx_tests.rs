//! HWPX format tests
//! HWPX 형식 테스트

mod common;
use common::*;

use hwp_core::HwpParser;
use insta::with_settings;

/// Helper macro for HWPX snapshot assertions
macro_rules! assert_hwpx_snapshot {
    ($name:expr, $value:expr) => {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let snapshots_dir = std::path::Path::new(manifest_dir)
            .join("tests")
            .join("snapshots");
        with_settings!({
            snapshot_path => snapshots_dir
        }, {
            insta::assert_snapshot!($name, $value);
        });
    };
}

#[test]
fn test_hwpx_parse() {
    let hwpx_files = find_all_hwpx_files();
    assert!(!hwpx_files.is_empty(), "No HWPX files found in fixtures");

    let parser = HwpParser::new();

    for file_path in &hwpx_files {
        let file_name = std::path::Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        let data = std::fs::read(file_path).expect("Failed to read HWPX file");
        let result = parser.parse(&data);

        assert!(
            result.is_ok(),
            "Failed to parse HWPX file {}: {:?}",
            file_name,
            result.err()
        );

        let document = result.unwrap();
        println!(
            "{}: parsed {} sections, {} paragraphs",
            file_name,
            document.body_text.sections.len(),
            document
                .body_text
                .sections
                .iter()
                .map(|s| s.paragraphs.len())
                .sum::<usize>()
        );
    }
}

#[test]
fn test_hwpx_markdown_snapshots() {
    let hwpx_files = find_all_hwpx_files();
    assert!(
        !hwpx_files.is_empty(),
        "HWPX fixture files required for snapshot tests"
    );

    let parser = HwpParser::new();

    for file_path in &hwpx_files {
        let file_name = std::path::Path::new(file_path)
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        let snapshot_name = format!("hwpx_{}_markdown", file_name.replace(['-', '.'], "_"));

        let data = std::fs::read(file_path).expect("Failed to read HWPX file");
        let document = parser.parse(&data).expect("Failed to parse HWPX");

        let options = hwp_core::viewer::markdown::MarkdownOptions {
            image_output_dir: None,
            use_html: Some(false),
            include_version: Some(false),
            include_page_info: Some(false),
            image_alt_text: None,
        };
        let markdown = document.to_markdown(&options);

        assert_hwpx_snapshot!(snapshot_name, markdown);
    }
}

#[test]
fn test_hwpx_html_snapshots() {
    let hwpx_files = find_all_hwpx_files();
    assert!(
        !hwpx_files.is_empty(),
        "HWPX fixture files required for snapshot tests"
    );

    let parser = HwpParser::new();

    for file_path in &hwpx_files {
        let file_name = std::path::Path::new(file_path)
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        let snapshot_name = format!("hwpx_{}_html", file_name.replace(['-', '.'], "_"));

        let data = std::fs::read(file_path).expect("Failed to read HWPX file");
        let document = parser.parse(&data).expect("Failed to parse HWPX");

        let options = hwp_core::viewer::html::HtmlOptions {
            image_output_dir: None,
            html_output_dir: None,
            include_version: Some(false),
            include_page_info: Some(false),
            css_class_prefix: "hwp-".to_string(),
        };
        let html = document.to_html(&options);

        assert_hwpx_snapshot!(snapshot_name, html);
    }
}

#[test]
fn test_hwpx_json_snapshots() {
    let hwpx_files = find_all_hwpx_files();
    assert!(
        !hwpx_files.is_empty(),
        "HWPX fixture files required for snapshot tests"
    );

    let parser = HwpParser::new();

    for file_path in &hwpx_files {
        let file_name = std::path::Path::new(file_path)
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        let snapshot_name = format!("hwpx_{}_json", file_name.replace(['-', '.'], "_"));

        let data = std::fs::read(file_path).expect("Failed to read HWPX file");
        let document = parser.parse(&data).expect("Failed to parse HWPX");

        let json = serde_json::to_string_pretty(&document).expect("Failed to convert to JSON");

        assert_hwpx_snapshot!(snapshot_name, json);
    }
}

#[test]
fn test_hwpx_specific_file() {
    // Test specific HWPX fixture file — required
    let file_path =
        find_hwpx_fixture_file("test-hwpx.hwpx").expect("test-hwpx.hwpx fixture is required");
    let data = std::fs::read(&file_path).expect("Failed to read HWPX file");
    let parser = HwpParser::new();
    let document = parser.parse(&data).expect("Failed to parse HWPX");

    // Basic assertions
    assert!(
        !document.body_text.sections.is_empty(),
        "Document should have sections"
    );

    // Print document structure for debugging
    println!("=== test-hwpx.hwpx Structure ===");
    println!("Sections: {}", document.body_text.sections.len());
    for (i, section) in document.body_text.sections.iter().enumerate() {
        println!("  Section {}: {} paragraphs", i, section.paragraphs.len());
    }
    println!("BinData items: {}", document.bin_data.items.len());
}
