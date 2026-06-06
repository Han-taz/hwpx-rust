/// HWPX BinData parser
///
/// BinData folder contains binary files like images, OLE objects, etc.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use unicode_normalization::UnicodeNormalization;

use crate::diagnostics::{
    DiagnosticCategory, DiagnosticContext, DiagnosticItem, DiagnosticReport, DiagnosticSeverity,
};
use crate::document::bindata::{BinData, BinaryDataItem};
use crate::document::{Paragraph, ParagraphRecord};
use crate::error::{HwpError, ParseWarning, ParseWarnings};
use crate::types::WORD;

use super::container::HwpxContainer;

/// Parse BinData folder and create BinData structure
pub fn parse_bindata(
    container: &mut HwpxContainer,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<BinData, HwpError> {
    let bindata_files = container.get_bindata_files();

    let mut items = Vec::new();
    let mut seen_names = std::collections::BTreeSet::new();

    for file_path in &bindata_files {
        let Some(name) = normalize_hwpx_bindata_ref(file_path) else {
            continue;
        };
        if !seen_names.insert(name.clone()) {
            return Err(HwpError::InvalidHwpxStructure {
                reason: format!("duplicate HWPX BinData item name {name}: {file_path}"),
            });
        }

        match container.read_file(file_path) {
            Ok(data) => {
                // Convert binary data to base64
                let base64_data = STANDARD.encode(&data);

                let index = hwpx_bindata_item_index(items.len(), file_path)?;
                items.push(BinaryDataItem {
                    index,
                    data: base64_data,
                    name: Some(name),
                });
            }
            Err(e) => {
                warnings.push(ParseWarning::recovered_error(format!(
                    "Failed to read BinData file {file_path}: {e}"
                )));
                diagnostics.push(
                    DiagnosticItem::new(
                        DiagnosticSeverity::DataLoss,
                        DiagnosticCategory::SkippedBinary,
                        format!("Failed to read BinData file {file_path}: {e}"),
                    )
                    .with_context(
                        DiagnosticContext::new()
                            .with_source(file_path.as_str())
                            .with_component("hwpx::bindata"),
                    ),
                );
            }
        }
    }

    Ok(BinData { items })
}

fn hwpx_bindata_item_index(item_count: usize, source: &str) -> Result<WORD, HwpError> {
    WORD::try_from(item_count).map_err(|_| HwpError::ResourceLimitExceeded {
        resource: "HWPX BinData item index",
        path: source.to_string(),
        limit: WORD::MAX as u64,
        actual: item_count as u64,
    })
}

pub(crate) fn normalize_hwpx_binary_item_ref(value: &str) -> Option<String> {
    normalize_hwpx_bindata_ref(value)
}

fn normalize_hwpx_bindata_ref(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('/')
        || trimmed.contains('\\')
        || trimmed.contains(':')
        || trimmed.chars().any(char::is_control)
    {
        return None;
    }

    let file_name = match trimmed.strip_prefix("BinData/") {
        Some(rest) => rest,
        None => trimmed,
    };

    if file_name.is_empty()
        || file_name.contains('/')
        || file_name == "."
        || file_name == ".."
        || file_name.starts_with('.')
    {
        return None;
    }

    let stem = file_name
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(file_name);

    if stem.is_empty() {
        None
    } else {
        Some(stem.nfc().collect())
    }
}

pub(super) fn record_missing_referenced_bindata(
    body_text: &crate::document::BodyText,
    bin_data: &BinData,
    diagnostics: &mut DiagnosticReport,
) {
    let available: std::collections::BTreeSet<String> = bin_data
        .items
        .iter()
        .filter_map(|item| item.name.as_deref().and_then(normalize_hwpx_bindata_ref))
        .collect();
    let mut referenced = std::collections::BTreeSet::new();

    for section in &body_text.sections {
        for paragraph in &section.paragraphs {
            collect_hwpx_image_refs_from_paragraph(paragraph, &mut referenced);
        }
    }

    for image_ref in referenced {
        let Some(normalized_ref) = normalize_hwpx_binary_item_ref(&image_ref) else {
            diagnostics.push(
                DiagnosticItem::new(
                    DiagnosticSeverity::DataLoss,
                    DiagnosticCategory::SkippedBinary,
                    format!("Referenced HWPX BinData item {image_ref} was not a safe item name"),
                )
                .with_context(
                    DiagnosticContext::new()
                        .with_source(image_ref.clone())
                        .with_component("hwpx::bindata"),
                ),
            );
            continue;
        };
        if !available.contains(&normalized_ref) {
            diagnostics.push(
                DiagnosticItem::new(
                    DiagnosticSeverity::DataLoss,
                    DiagnosticCategory::SkippedBinary,
                    format!("Referenced HWPX BinData item {image_ref} was not found"),
                )
                .with_context(
                    DiagnosticContext::new()
                        .with_source(image_ref.clone())
                        .with_component("hwpx::bindata"),
                ),
            );
        }
    }
}

fn collect_hwpx_image_refs_from_paragraph(
    paragraph: &Paragraph,
    refs: &mut std::collections::BTreeSet<String>,
) {
    for record in &paragraph.records {
        collect_hwpx_image_refs_from_record(record, refs);
    }
}

fn collect_hwpx_image_refs_from_record(
    record: &ParagraphRecord,
    refs: &mut std::collections::BTreeSet<String>,
) {
    match record {
        ParagraphRecord::HwpxImage {
            binary_item_ref, ..
        } => {
            refs.insert(binary_item_ref.clone());
        }
        ParagraphRecord::Table { table } => {
            for cell in &table.cells {
                for paragraph in &cell.paragraphs {
                    collect_hwpx_image_refs_from_paragraph(paragraph, refs);
                }
            }
        }
        ParagraphRecord::CtrlHeader { data } => {
            for paragraph in &data.paragraphs {
                collect_hwpx_image_refs_from_paragraph(paragraph, refs);
            }
            for child in &data.children {
                collect_hwpx_image_refs_from_record(child, refs);
            }
        }
        ParagraphRecord::ListHeader { data } => {
            for paragraph in &data.paragraphs {
                collect_hwpx_image_refs_from_paragraph(paragraph, refs);
            }
        }
        ParagraphRecord::ShapeComponent { data } => {
            for child in &data.children {
                collect_hwpx_image_refs_from_record(child, refs);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{DiagnosticCategory, DiagnosticReport, DiagnosticSeverity};
    use std::io::{Cursor, Write};
    use zip::{write::SimpleFileOptions, ZipWriter};

    fn zip_with_bindata_directory() -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);
        zip.add_directory("BinData/", SimpleFileOptions::default())
            .unwrap();
        zip.start_file("BinData/image1.png", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"png-data").unwrap();
        zip.start_file("BinData/image2.jpg", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"jpg-data").unwrap();
        zip.finish().unwrap().into_inner()
    }

    fn zip_with_nested_bindata_entry_before_file() -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);
        zip.start_file("BinData/nested/image0.png", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"nested-png-data").unwrap();
        zip.start_file("BinData/image1.png", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"png-data").unwrap();
        zip.finish().unwrap().into_inner()
    }

    fn zip_with_duplicate_bindata_stems() -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);
        zip.start_file("BinData/image1.png", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"png-data").unwrap();
        zip.start_file("BinData/image1.jpg", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"jpg-data").unwrap();
        zip.finish().unwrap().into_inner()
    }

    fn zip_with_unicode_equivalent_bindata_stems() -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);
        zip.start_file("BinData/caf\u{e9}.png", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"png-data").unwrap();
        zip.start_file("BinData/cafe\u{301}.jpg", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"jpg-data").unwrap();
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn missing_referenced_bindata_records_data_loss_diagnostic() {
        let body_text = crate::document::BodyText {
            sections: vec![crate::document::Section {
                index: 0,
                paragraphs: vec![crate::document::Paragraph {
                    para_header: Default::default(),
                    records: vec![crate::document::ParagraphRecord::HwpxImage {
                        binary_item_ref: "image1".to_string(),
                        brightness: None,
                        contrast: None,
                        effect: None,
                        alpha: None,
                    }],
                }],
            }],
        };
        let bin_data = BinData { items: vec![] };
        let mut diagnostics = DiagnosticReport::default();

        record_missing_referenced_bindata(&body_text, &bin_data, &mut diagnostics);

        assert!(diagnostics.items.iter().any(|item| {
            item.severity == DiagnosticSeverity::DataLoss
                && item.category == DiagnosticCategory::SkippedBinary
                && item.message.contains("image1")
        }));
    }

    #[test]
    fn referenced_bindata_path_with_extension_matches_item_stem() {
        let body_text = crate::document::BodyText {
            sections: vec![crate::document::Section {
                index: 0,
                paragraphs: vec![crate::document::Paragraph {
                    para_header: Default::default(),
                    records: vec![crate::document::ParagraphRecord::HwpxImage {
                        binary_item_ref: "BinData/image1.jpg".to_string(),
                        brightness: None,
                        contrast: None,
                        effect: None,
                        alpha: None,
                    }],
                }],
            }],
        };
        let bin_data = BinData {
            items: vec![BinaryDataItem {
                index: 0,
                data: String::new(),
                name: Some("image1".to_string()),
            }],
        };
        let mut diagnostics = DiagnosticReport::default();

        record_missing_referenced_bindata(&body_text, &bin_data, &mut diagnostics);

        assert!(
            diagnostics.items.iter().all(|item| {
                item.severity != DiagnosticSeverity::DataLoss
                    || item.category != DiagnosticCategory::SkippedBinary
            }),
            "path+extension refs should match BinData item stems: {diagnostics:?}"
        );
    }

    #[test]
    fn referenced_bindata_matches_unicode_normalization_equivalent_item_stem() {
        let body_text = crate::document::BodyText {
            sections: vec![crate::document::Section {
                index: 0,
                paragraphs: vec![crate::document::Paragraph {
                    para_header: Default::default(),
                    records: vec![crate::document::ParagraphRecord::HwpxImage {
                        binary_item_ref: "caf\u{e9}".to_string(),
                        brightness: None,
                        contrast: None,
                        effect: None,
                        alpha: None,
                    }],
                }],
            }],
        };
        let bin_data = BinData {
            items: vec![BinaryDataItem {
                index: 0,
                data: String::new(),
                name: Some("cafe\u{301}".to_string()),
            }],
        };
        let mut diagnostics = DiagnosticReport::default();

        record_missing_referenced_bindata(&body_text, &bin_data, &mut diagnostics);

        assert!(
            diagnostics.items.iter().all(|item| {
                item.severity != DiagnosticSeverity::DataLoss
                    || item.category != DiagnosticCategory::SkippedBinary
            }),
            "Unicode-normalization equivalent BinData names should match: {diagnostics:?}"
        );
    }

    #[test]
    fn parse_bindata_indexes_files_without_directory_entries() {
        let data = zip_with_bindata_directory();
        let mut container =
            HwpxContainer::open(&data).expect("test HWPX archive should open successfully");
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let bin_data = parse_bindata(&mut container, &mut warnings, &mut diagnostics)
            .expect("BinData should parse");

        assert_eq!(bin_data.items.len(), 2);
        assert_eq!(bin_data.items[0].index, 0);
        assert_eq!(bin_data.items[0].name.as_deref(), Some("image1"));
        assert_eq!(bin_data.items[1].index, 1);
        assert_eq!(bin_data.items[1].name.as_deref(), Some("image2"));
    }

    #[test]
    fn parse_bindata_ignores_nested_entries_without_indexing_them() {
        let data = zip_with_nested_bindata_entry_before_file();
        let mut container =
            HwpxContainer::open(&data).expect("test HWPX archive should open successfully");
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let bin_data = parse_bindata(&mut container, &mut warnings, &mut diagnostics)
            .expect("BinData should parse");

        assert_eq!(bin_data.items.len(), 1);
        assert_eq!(bin_data.items[0].index, 0);
        assert_eq!(bin_data.items[0].name.as_deref(), Some("image1"));
    }

    #[test]
    fn parse_bindata_rejects_duplicate_normalized_item_names() {
        let data = zip_with_duplicate_bindata_stems();
        let mut container =
            HwpxContainer::open(&data).expect("test HWPX archive should open successfully");
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_bindata(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("duplicate normalized BinData item names should be rejected");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("duplicate HWPX BinData item name")
                    && reason.contains("image1")
                    && reason.contains("BinData/image1.jpg")
        ));
    }

    #[test]
    fn parse_bindata_rejects_unicode_equivalent_item_names() {
        let data = zip_with_unicode_equivalent_bindata_stems();
        let mut container =
            HwpxContainer::open(&data).expect("test HWPX archive should open successfully");
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_bindata(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("Unicode-normalization equivalent BinData item names should be rejected");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("duplicate HWPX BinData item name")
                    && reason.contains("caf")
                    && reason.contains("BinData/cafe")
        ));
    }

    #[test]
    fn bindata_index_over_word_range_is_rejected() {
        let err = hwpx_bindata_item_index(u16::MAX as usize + 1, "BinData/overflow.png")
            .expect_err("BinData index over WORD range should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX BinData item index"
                && path == "BinData/overflow.png"
                && limit == u16::MAX as u64
                && actual == u16::MAX as u64 + 1
        ));
    }
}
