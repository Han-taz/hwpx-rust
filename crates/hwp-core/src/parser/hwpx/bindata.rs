/// HWPX BinData parser
///
/// BinData folder contains binary files like images, OLE objects, etc.
use base64::{engine::general_purpose::STANDARD, Engine as _};

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

    for (index, file_path) in bindata_files.iter().enumerate() {
        // Skip directories
        if file_path.ends_with('/') {
            continue;
        }

        match container.read_file(file_path) {
            Ok(data) => {
                // Convert binary data to base64
                let base64_data = STANDARD.encode(&data);

                // Extract filename without extension for name lookup
                // e.g., "BinData/image1.jpg" -> "image1"
                let name = file_path
                    .rsplit('/')
                    .next()
                    .and_then(|filename| filename.rsplit_once('.'))
                    .map(|(name_part, _)| name_part.to_string());

                items.push(BinaryDataItem {
                    index: index as WORD,
                    data: base64_data,
                    name,
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

pub(super) fn record_missing_referenced_bindata(
    body_text: &crate::document::BodyText,
    bin_data: &BinData,
    diagnostics: &mut DiagnosticReport,
) {
    let available: std::collections::BTreeSet<&str> = bin_data
        .items
        .iter()
        .filter_map(|item| item.name.as_deref())
        .collect();
    let mut referenced = std::collections::BTreeSet::new();

    for section in &body_text.sections {
        for paragraph in &section.paragraphs {
            collect_hwpx_image_refs_from_paragraph(paragraph, &mut referenced);
        }
    }

    for image_ref in referenced {
        if !available.contains(image_ref.as_str()) {
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
        ParagraphRecord::HwpxImage { binary_item_ref } => {
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

    #[test]
    fn missing_referenced_bindata_records_data_loss_diagnostic() {
        let body_text = crate::document::BodyText {
            sections: vec![crate::document::Section {
                index: 0,
                paragraphs: vec![crate::document::Paragraph {
                    para_header: Default::default(),
                    records: vec![crate::document::ParagraphRecord::HwpxImage {
                        binary_item_ref: "image1".to_string(),
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
}
