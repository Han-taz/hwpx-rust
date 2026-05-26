mod common;

use common::find_fixtures_dir;
use hwp_core::{
    DiagnosticCategory, DiagnosticContext, DiagnosticItem, DiagnosticReport, DiagnosticSeverity,
    HwpParser,
};
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CorpusManifest {
    documents: Vec<CorpusDocument>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CorpusDocument {
    path: String,
    format: String,
    expect_parse: String,
    expected_error: Option<String>,
    max_data_loss: Option<usize>,
    max_recovered_error: Option<usize>,
    allowed_categories: Option<Vec<String>>,
    expected_sections: Option<usize>,
}

#[test]
fn corpus_manifest_documents_match_expectations() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus");
    let manifest_path = manifest_dir.join("manifest.toml");
    let manifest_text =
        std::fs::read_to_string(&manifest_path).expect("corpus manifest should be readable");
    let manifest: CorpusManifest =
        toml::from_str(&manifest_text).expect("corpus manifest should parse");
    let fixtures_dir = find_fixtures_dir().expect("fixtures directory should exist");
    let parser = HwpParser::new();

    assert!(
        !manifest.documents.is_empty(),
        "corpus manifest must include at least one document"
    );

    for document in manifest.documents {
        let path = fixtures_dir.join(&document.path);
        assert_eq!(
            document.format.as_str(),
            path.extension().and_then(|s| s.to_str()).unwrap_or(""),
            "manifest format should match fixture extension for {}",
            document.path
        );

        let data = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("failed to read corpus fixture {}: {e}", path.display()));
        let result = parser.parse(&data);

        match document.expect_parse.as_str() {
            "success" => {
                let parsed = result.unwrap_or_else(|e| {
                    panic!("expected {} to parse successfully, got {e}", document.path)
                });

                if let Some(expected_sections) = document.expected_sections {
                    assert_eq!(
                        parsed.body_text.sections.len(),
                        expected_sections,
                        "{} section count changed",
                        document.path
                    );
                }

                assert_diagnostic_thresholds(&document, &parsed.diagnostics);
            }
            "error" => {
                let err = result.expect_err(&format!("expected {} to fail parsing", document.path));
                if let Some(expected_error) = &document.expected_error {
                    let debug = format!("{err:?}");
                    let display = err.to_string();
                    assert!(
                        debug.contains(expected_error) || display.contains(expected_error),
                        "{} failed with wrong error.\nexpected contains: {}\ndebug: {}\ndisplay: {}",
                        document.path,
                        expected_error,
                        debug,
                        display
                    );
                }
            }
            other => panic!(
                "unsupported expect_parse value for {}: {other}",
                document.path
            ),
        }
    }
}

#[test]
fn corpus_manifest_rejects_unknown_fields() {
    let manifest_text = r#"
[[documents]]
path = "typo.hwpx"
format = "hwpx"
expect_parse = "success"
max_data_los = 0
"#;

    let err = toml::from_str::<CorpusManifest>(manifest_text)
        .expect_err("unknown manifest fields should be rejected");

    assert!(
        err.to_string().contains("unknown field"),
        "unexpected manifest parse error: {err}"
    );
}

#[test]
fn formatted_diagnostics_include_full_context_and_suggestions() {
    let mut report = DiagnosticReport::default();
    report.push(
        DiagnosticItem::new(
            DiagnosticSeverity::DataLoss,
            DiagnosticCategory::SkippedBinary,
            "missing binary payload",
        )
        .with_context(
            DiagnosticContext::new()
                .with_source("Contents/section0.xml")
                .with_section_index(0)
                .with_element("hp:pic")
                .with_attribute("binaryItemIDRef")
                .with_value("BIN0001")
                .with_offset(42)
                .with_component("hwpx::bindata"),
        )
        .with_suggestion("add BinData/BIN0001.jpg to the archive"),
    );

    let formatted = format_diagnostics(&report);

    for expected in [
        "source=\"Contents/section0.xml\"",
        "section=0",
        "element=\"hp:pic\"",
        "attribute=\"binaryItemIDRef\"",
        "value=\"BIN0001\"",
        "offset=42",
        "component=\"hwpx::bindata\"",
        "suggestion=\"add BinData/BIN0001.jpg to the archive\"",
    ] {
        assert!(
            formatted.contains(expected),
            "formatted diagnostics did not include {expected}: {formatted}"
        );
    }
}

fn assert_diagnostic_thresholds(document: &CorpusDocument, report: &hwp_core::DiagnosticReport) {
    let summary = report.summary();
    let data_loss = summary.by_severity.get("DataLoss").copied().unwrap_or(0);
    let recovered = summary
        .by_severity
        .get("RecoveredError")
        .copied()
        .unwrap_or(0);

    if let Some(max_data_loss) = document.max_data_loss {
        assert!(
            data_loss <= max_data_loss,
            "{} exceeded max_data_loss: expected <= {}, got {}\n{}",
            document.path,
            max_data_loss,
            data_loss,
            format_diagnostics(report)
        );
    }

    if let Some(max_recovered_error) = document.max_recovered_error {
        assert!(
            recovered <= max_recovered_error,
            "{} exceeded max_recovered_error: expected <= {}, got {}\n{}",
            document.path,
            max_recovered_error,
            recovered,
            format_diagnostics(report)
        );
    }

    let allowed: BTreeSet<String> = document
        .allowed_categories
        .clone()
        .unwrap_or_default()
        .into_iter()
        .collect();

    for item in &report.items {
        let category = item.category.as_str().to_string();
        assert!(
            allowed.contains(&category),
            "{} emitted unlisted diagnostic category {}:\n{}",
            document.path,
            category,
            format_diagnostics(report)
        );
    }
}

fn format_diagnostics(report: &hwp_core::DiagnosticReport) -> String {
    report
        .items
        .iter()
        .map(|item| {
            let context = format_diagnostic_context(item);
            format!(
                "- [{}/{}] {} ({})",
                item.severity.as_str(),
                item.category.as_str(),
                item.message,
                context
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_diagnostic_context(item: &DiagnosticItem) -> String {
    let mut parts = Vec::new();

    if let Some(source) = &item.context.source {
        parts.push(format!("source={source:?}"));
    }
    if let Some(section_index) = item.context.section_index {
        parts.push(format!("section={section_index}"));
    }
    if let Some(element) = &item.context.element {
        parts.push(format!("element={element:?}"));
    }
    if let Some(attribute) = &item.context.attribute {
        parts.push(format!("attribute={attribute:?}"));
    }
    if let Some(value) = &item.context.value {
        parts.push(format!("value={value:?}"));
    }
    if let Some(offset) = item.context.offset {
        parts.push(format!("offset={offset}"));
    }
    if let Some(component) = &item.context.component {
        parts.push(format!("component={component:?}"));
    }
    if let Some(suggestion) = &item.suggestion {
        parts.push(format!("suggestion={suggestion:?}"));
    }

    if parts.is_empty() {
        "context=none".to_string()
    } else {
        parts.join(" ")
    }
}
