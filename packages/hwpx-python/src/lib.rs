use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use hwp_core::viewer::html::{to_html, HtmlOptions};
use hwp_core::viewer::markdown::{to_markdown, MarkdownOptions};
use hwp_core::{HwpDocument, HwpParser};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

const MAX_PYTHON_INPUT_SIZE: u64 = 512 * 1024 * 1024;
const MAX_DOCUMENT_CACHE_ENTRIES: usize = 8;
const MAX_DOCUMENT_CACHE_VALUE_BYTES: usize = 16 * 1024 * 1024;

fn input_size_limit_error(operation: &str, actual: u64) -> String {
    format!(
        "resource limit exceeded: input size exceeded for {operation} \
         (limit: {MAX_PYTHON_INPUT_SIZE} bytes, actual: {actual} bytes)"
    )
}

fn validate_input_size(operation: &str, actual: u64) -> Result<(), String> {
    if actual > MAX_PYTHON_INPUT_SIZE {
        return Err(input_size_limit_error(operation, actual));
    }

    Ok(())
}

fn read_file_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = std::fs::metadata(path).map_err(|e| format!("Failed to inspect file: {e}"))?;
    if !metadata.is_file() {
        return Err("Failed to read file: path is not a regular file".to_string());
    }
    validate_input_size("parse_file", metadata.len())?;

    let file = std::fs::File::open(path).map_err(|e| format!("Failed to read file: {e}"))?;
    let mut limited_file = file.take(MAX_PYTHON_INPUT_SIZE + 1);
    let capacity = metadata.len().min(MAX_PYTHON_INPUT_SIZE) as usize;
    let mut data = Vec::with_capacity(capacity);
    limited_file
        .read_to_end(&mut data)
        .map_err(|e| format!("Failed to read file: {e}"))?;

    validate_input_size("parse_file", data.len() as u64)?;

    Ok(data)
}

/// Format version DWORD to "M.n.P.r" string
/// Format: 0xMMnnPPrr (e.g., 0x05000300 = "5.0.3.0")
fn format_version(version: u32) -> String {
    let major = (version >> 24) & 0xFF;
    let minor = (version >> 16) & 0xFF;
    let patch = (version >> 8) & 0xFF;
    let revision = version & 0xFF;
    format!("{major}.{minor}.{patch}.{revision}")
}

fn document_from_parse_result(
    result: Result<HwpDocument, hwp_core::HwpError>,
) -> PyResult<Document> {
    match result {
        Ok(doc) => Ok(Document {
            inner: doc,
            cache: RefCell::new(HashMap::new()),
        }),
        Err(e) => Err(PyValueError::new_err(format!("Parse error: {e}"))),
    }
}

fn collect_plain_text(document: &HwpDocument) -> String {
    let mut result = String::new();
    let mut first = true;
    for section in &document.body_text.sections {
        for paragraph in &section.paragraphs {
            collect_plain_text_from_paragraph(paragraph, &mut result, &mut first);
        }
    }
    result
}

fn collect_plain_text_from_paragraph(
    paragraph: &hwp_core::document::Paragraph,
    result: &mut String,
    first: &mut bool,
) {
    for record in &paragraph.records {
        collect_plain_text_from_record(record, result, first);
    }
}

fn collect_plain_text_from_record(
    record: &hwp_core::document::bodytext::ParagraphRecord,
    result: &mut String,
    first: &mut bool,
) {
    use hwp_core::document::bodytext::ParagraphRecord;

    match record {
        ParagraphRecord::ParaText { data } => append_plain_text(&data.text, result, first),
        ParagraphRecord::Table { table } => {
            for cell in &table.cells {
                for paragraph in &cell.paragraphs {
                    collect_plain_text_from_paragraph(paragraph, result, first);
                }
            }
        }
        ParagraphRecord::CtrlHeader { data } => {
            for child in &data.children {
                collect_plain_text_from_record(child, result, first);
            }
            for paragraph in &data.paragraphs {
                collect_plain_text_from_paragraph(paragraph, result, first);
            }
        }
        ParagraphRecord::ListHeader { data } => {
            for paragraph in &data.paragraphs {
                collect_plain_text_from_paragraph(paragraph, result, first);
            }
        }
        ParagraphRecord::ShapeComponent { data } => {
            for child in &data.children {
                collect_plain_text_from_record(child, result, first);
            }
        }
        _ => {}
    }
}

fn append_plain_text(text: &str, result: &mut String, first: &mut bool) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    if !*first {
        result.push('\n');
    }
    result.push_str(trimmed);
    *first = false;
}

/// Cache key for conversion results
#[derive(Hash, Eq, PartialEq)]
enum CacheKey {
    Markdown {
        use_html: bool,
        include_version: bool,
        image_dir: Option<String>,
    },
    Html {
        image_dir: Option<String>,
    },
    Json,
    Text,
}

fn cache_conversion_result(
    cache: &RefCell<HashMap<CacheKey, String>>,
    key: CacheKey,
    result: &str,
) {
    if result.len() > MAX_DOCUMENT_CACHE_VALUE_BYTES {
        return;
    }

    let mut cache = cache.borrow_mut();
    if cache.contains_key(&key) || cache.len() < MAX_DOCUMENT_CACHE_ENTRIES {
        cache.insert(key, result.to_owned());
    }
}

/// HWP/HWPX Document wrapper for Python
#[pyclass(unsendable)]
struct Document {
    inner: HwpDocument,
    cache: RefCell<HashMap<CacheKey, String>>,
}

#[pymethods]
impl Document {
    /// Get document version as string
    #[getter]
    fn version(&self) -> String {
        format_version(self.inner.file_header.version)
    }

    /// Get number of sections
    #[getter]
    fn section_count(&self) -> usize {
        self.inner.body_text.sections.len()
    }

    /// Convert document to markdown
    ///
    /// Args:
    ///     use_html: Whether to use HTML tags (default: True)
    ///     include_version: Whether to include version info (default: True)
    ///     image_output_dir: Directory to save images (default: None, embeds as base64)
    ///
    /// Returns:
    ///     Markdown string
    #[pyo3(signature = (use_html=true, include_version=true, image_output_dir=None))]
    fn to_markdown(
        &self,
        py: Python<'_>,
        use_html: bool,
        include_version: bool,
        image_output_dir: Option<String>,
    ) -> String {
        let key = CacheKey::Markdown {
            use_html,
            include_version,
            image_dir: image_output_dir.clone(),
        };
        if let Some(cached) = self.cache.borrow().get(&key) {
            return cached.clone();
        }
        let options = MarkdownOptions {
            image_output_dir,
            use_html: Some(use_html),
            include_version: Some(include_version),
            include_page_info: None,
            image_alt_text: None,
        };
        let document = &self.inner;
        let result = py.detach(|| to_markdown(document, &options));
        cache_conversion_result(&self.cache, key, &result);
        result
    }

    /// Convert document to HTML
    ///
    /// Args:
    ///     image_output_dir: Directory to save images (default: None, embeds as base64)
    ///
    /// Returns:
    ///     HTML string
    #[pyo3(signature = (image_output_dir=None))]
    fn to_html(&self, py: Python<'_>, image_output_dir: Option<String>) -> String {
        let key = CacheKey::Html {
            image_dir: image_output_dir.clone(),
        };
        if let Some(cached) = self.cache.borrow().get(&key) {
            return cached.clone();
        }
        let options = HtmlOptions {
            image_output_dir,
            html_output_dir: None,
            include_version: Some(true),
            include_page_info: None,
            css_class_prefix: String::new(),
        };
        let document = &self.inner;
        let result = py.detach(|| to_html(document, &options));
        cache_conversion_result(&self.cache, key, &result);
        result
    }

    /// Convert document to JSON
    ///
    /// Returns:
    ///     JSON string representation of the document
    fn to_json(&self, py: Python<'_>) -> PyResult<String> {
        if let Some(cached) = self.cache.borrow().get(&CacheKey::Json) {
            return Ok(cached.clone());
        }
        let document = &self.inner;
        let result = py
            .detach(|| serde_json::to_string_pretty(document))
            .map_err(|e| PyValueError::new_err(format!("JSON serialization error: {e}")))?;
        cache_conversion_result(&self.cache, CacheKey::Json, &result);
        Ok(result)
    }

    /// Get parsing warnings
    ///
    /// Returns:
    ///     List of warning strings from parsing
    #[getter]
    fn warnings(&self) -> Vec<String> {
        self.inner
            .warnings
            .warnings()
            .iter()
            .map(|w| w.to_string())
            .collect()
    }

    /// Get structured parser diagnostics
    ///
    /// Returns:
    ///     Dictionary with diagnostic items and summary
    fn diagnostic_report<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let report = PyDict::new(py);
        let items = PyList::empty(py);

        for item in &self.inner.diagnostics.items {
            let item_dict = PyDict::new(py);
            item_dict.set_item("severity", item.severity.as_str())?;
            item_dict.set_item("category", item.category.as_str())?;
            item_dict.set_item("message", &item.message)?;

            let context = PyDict::new(py);
            context.set_item("source", item.context.source.as_deref())?;
            context.set_item("section_index", item.context.section_index)?;
            context.set_item("element", item.context.element.as_deref())?;
            context.set_item("attribute", item.context.attribute.as_deref())?;
            context.set_item("value", item.context.value.as_deref())?;
            context.set_item("offset", item.context.offset)?;
            context.set_item("component", item.context.component.as_deref())?;
            item_dict.set_item("context", context)?;

            item_dict.set_item("suggestion", item.suggestion.as_deref())?;
            items.append(item_dict)?;
        }

        let summary = self.inner.diagnostics.summary();
        let summary_dict = PyDict::new(py);
        summary_dict.set_item("total", summary.total)?;
        summary_dict.set_item("max_items", summary.max_items)?;
        summary_dict.set_item("truncated", summary.truncated)?;

        let by_severity = PyDict::new(py);
        for (key, value) in summary.by_severity {
            by_severity.set_item(key, value)?;
        }
        summary_dict.set_item("by_severity", by_severity)?;

        let by_category = PyDict::new(py);
        for (key, value) in summary.by_category {
            by_category.set_item(key, value)?;
        }
        summary_dict.set_item("by_category", by_category)?;

        report.set_item("items", items)?;
        report.set_item("summary", summary_dict)?;
        Ok(report)
    }

    /// Get plain text content from the document
    fn get_text(&self, py: Python<'_>) -> String {
        if let Some(cached) = self.cache.borrow().get(&CacheKey::Text) {
            return cached.clone();
        }
        let document = &self.inner;
        let result = py.detach(|| collect_plain_text(document));
        cache_conversion_result(&self.cache, CacheKey::Text, &result);
        result
    }
}

/// Parse HWP/HWPX file from bytes
///
/// Args:
///     data: File content as bytes
///
/// Returns:
///     Document object
///
/// Raises:
///     ValueError: If the file format is invalid or parsing fails
#[pyfunction]
fn parse(py: Python<'_>, data: &[u8]) -> PyResult<Document> {
    validate_input_size("parse", data.len() as u64).map_err(PyValueError::new_err)?;
    let data = data.to_vec();
    let result = py.detach(move || HwpParser::new().parse(&data));
    document_from_parse_result(result)
}

/// Parse HWP/HWPX file from file path
///
/// Args:
///     path: Path to the HWP/HWPX file
///
/// Returns:
///     Document object
///
/// Raises:
///     ValueError: If the file cannot be read or parsing fails
#[pyfunction]
fn parse_file(py: Python<'_>, path: &str) -> PyResult<Document> {
    let path = PathBuf::from(path);
    let result = py.detach(move || {
        let data = read_file_bounded(&path)?;
        HwpParser::new()
            .parse(&data)
            .map_err(|e| format!("Parse error: {e}"))
    });

    match result {
        Ok(doc) => Ok(Document {
            inner: doc,
            cache: RefCell::new(HashMap::new()),
        }),
        Err(e) => Err(PyValueError::new_err(e)),
    }
}

/// hwpxkit - Python bindings for HWP/HWPX document parser
///
/// This module provides functions to parse and convert HWP/HWPX documents.
///
/// Example:
///     >>> import hwpxkit
///     >>> doc = hwpxkit.parse_file("document.hwpx")
///     >>> print(doc.to_markdown())
///     >>> print(doc.to_html())
///     >>> print(doc.get_text())
#[pymodule]
#[pyo3(name = "_native")]
fn hwpxkit(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(parse, m)?)?;
    m.add_function(wrap_pyfunction!(parse_file, m)?)?;
    m.add_class::<Document>()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(not(feature = "extension-module"))]
    use hwp_core::diagnostics::MAX_DIAGNOSTIC_ITEMS;
    #[cfg(not(feature = "extension-module"))]
    use hwp_core::document::bodytext::list_header::{
        LineBreak, ListHeader, ListHeaderAttribute, TextDirection, VerticalAlign,
    };
    #[cfg(not(feature = "extension-module"))]
    use hwp_core::document::bodytext::table::{
        CellAttributes, PageBreakBehavior, Table, TableAttribute, TableAttributes, TableCell,
        TablePadding,
    };
    #[cfg(not(feature = "extension-module"))]
    use hwp_core::document::bodytext::{ParaTextData, Paragraph, ParagraphRecord, Section};
    #[cfg(not(feature = "extension-module"))]
    use hwp_core::types::HWPUNIT;
    #[cfg(not(feature = "extension-module"))]
    use hwp_core::{
        DiagnosticCategory, DiagnosticItem, DiagnosticReport, DiagnosticSeverity, FileHeader,
        HwpDocument,
    };
    #[cfg(not(feature = "extension-module"))]
    use pyo3::prelude::*;
    #[cfg(not(feature = "extension-module"))]
    use pyo3::types::PyDict;
    #[cfg(not(feature = "extension-module"))]
    use std::cell::RefCell;
    #[cfg(not(feature = "extension-module"))]
    use std::collections::HashMap;
    use std::fs::{self, OpenOptions};
    use std::io::Write;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "hwpx-python-{name}-{}-{nanos}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ))
    }

    #[cfg(not(feature = "extension-module"))]
    fn test_file_header() -> FileHeader {
        FileHeader {
            signature: "HWP Document File".to_string(),
            version: 0x05000300,
            document_flags: 0,
            license_flags: 0,
            encrypt_version: 0,
            kogl_country: 0,
            reserved: vec![0; 207],
        }
    }

    #[cfg(not(feature = "extension-module"))]
    fn document_with_diagnostics(diagnostics: DiagnosticReport) -> super::Document {
        let mut inner = HwpDocument::new(test_file_header());
        inner.diagnostics = diagnostics;
        super::Document {
            inner,
            cache: RefCell::new(HashMap::new()),
        }
    }

    #[cfg(not(feature = "extension-module"))]
    fn empty_document() -> super::Document {
        super::Document {
            inner: HwpDocument::new(test_file_header()),
            cache: RefCell::new(HashMap::new()),
        }
    }

    #[cfg(not(feature = "extension-module"))]
    fn document_with_text(text: String) -> super::Document {
        let mut inner = HwpDocument::new(test_file_header());
        inner.body_text.sections.push(Section {
            index: 0,
            paragraphs: vec![paragraph_with_text(text)],
        });

        super::Document {
            inner,
            cache: RefCell::new(HashMap::new()),
        }
    }

    #[cfg(not(feature = "extension-module"))]
    fn paragraph_with_text(text: String) -> Paragraph {
        Paragraph {
            records: vec![ParagraphRecord::ParaText {
                data: Box::new(ParaTextData {
                    text,
                    ..Default::default()
                }),
            }],
            ..Default::default()
        }
    }

    #[cfg(not(feature = "extension-module"))]
    fn minimal_list_header() -> ListHeader {
        ListHeader {
            paragraph_count: 1,
            attribute: ListHeaderAttribute {
                text_direction: TextDirection::Horizontal,
                line_break: LineBreak::Normal,
                vertical_align: VerticalAlign::Top,
            },
        }
    }

    #[cfg(not(feature = "extension-module"))]
    fn minimal_table_with_cell_text(text: String) -> Table {
        Table {
            attributes: TableAttributes {
                attribute: TableAttribute {
                    page_break: PageBreakBehavior::NoBreak,
                    header_row_repeat: false,
                },
                row_count: 1,
                col_count: 1,
                cell_spacing: 0,
                padding: TablePadding {
                    left: 0,
                    right: 0,
                    top: 0,
                    bottom: 0,
                },
                row_sizes: vec![],
                border_fill_id: 0,
                zones: vec![],
            },
            cells: vec![TableCell {
                list_header: minimal_list_header(),
                cell_attributes: CellAttributes {
                    col_address: 0,
                    row_address: 0,
                    col_span: 1,
                    row_span: 1,
                    width: HWPUNIT(1000),
                    height: HWPUNIT(1000),
                    left_margin: 0,
                    right_margin: 0,
                    top_margin: 0,
                    bottom_margin: 0,
                    border_fill_id: 0,
                },
                paragraphs: vec![paragraph_with_text(text)],
            }],
        }
    }

    #[cfg(not(feature = "extension-module"))]
    fn document_with_table_cell_text() -> super::Document {
        let mut inner = HwpDocument::new(test_file_header());
        inner.body_text.sections.push(Section {
            index: 0,
            paragraphs: vec![Paragraph {
                records: vec![
                    ParagraphRecord::ParaText {
                        data: Box::new(ParaTextData {
                            text: "outer".to_string(),
                            ..Default::default()
                        }),
                    },
                    ParagraphRecord::Table {
                        table: minimal_table_with_cell_text("cell".to_string()),
                    },
                ],
                ..Default::default()
            }],
        });

        super::Document {
            inner,
            cache: RefCell::new(HashMap::new()),
        }
    }

    #[test]
    fn read_file_bounded_reads_small_files() {
        let path = temp_path("small");
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.write_all(b"abc").unwrap();
        drop(file);

        let data = super::read_file_bounded(&path).unwrap();

        fs::remove_file(&path).unwrap();
        assert_eq!(data, b"abc");
    }

    #[test]
    fn read_file_bounded_rejects_oversized_files_before_reading() {
        let path = temp_path("oversized");
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.set_len(super::MAX_PYTHON_INPUT_SIZE + 1).unwrap();
        drop(file);

        let err = super::read_file_bounded(&path).unwrap_err();

        fs::remove_file(&path).unwrap();
        assert!(err.contains("resource limit"));
        assert!(err.contains("parse_file"));
        assert!(err.contains("input size"));
    }

    #[test]
    fn read_file_bounded_rejects_non_regular_files_before_reading() {
        let path = temp_path("directory");
        fs::create_dir(&path).unwrap();

        let err = super::read_file_bounded(&path).unwrap_err();

        fs::remove_dir(&path).unwrap();
        assert!(err.contains("regular file"));
    }

    #[test]
    fn parse_input_size_limit_rejects_oversized_byte_payload_without_allocation() {
        let err =
            super::validate_input_size("parse", super::MAX_PYTHON_INPUT_SIZE + 1).unwrap_err();

        assert!(err.contains("resource limit"));
        assert!(err.contains("parse"));
        assert!(err.contains("input size"));
    }

    #[cfg(not(feature = "extension-module"))]
    #[test]
    fn document_conversion_cache_is_bounded_by_entry_count() {
        let document = empty_document();

        Python::initialize();
        Python::attach(|py| {
            for index in 0..16 {
                let _ = document.to_html(py, Some(format!("images-{index}")));
            }
        });

        assert!(
            document.cache.borrow().len() <= 8,
            "conversion cache should stay bounded for option churn"
        );
    }

    #[cfg(not(feature = "extension-module"))]
    #[test]
    fn oversized_conversion_result_is_not_cached() {
        let oversized_text = "x".repeat(super::MAX_DOCUMENT_CACHE_VALUE_BYTES + 1);
        let oversized_len = oversized_text.len();
        let document = document_with_text(oversized_text);

        Python::initialize();
        let converted_len = Python::attach(|py| document.get_text(py).len());

        assert_eq!(converted_len, oversized_len);
        assert!(
            document
                .cache
                .borrow()
                .get(&super::CacheKey::Text)
                .is_none(),
            "oversized conversion results should not be retained in the document cache"
        );
    }

    #[cfg(not(feature = "extension-module"))]
    #[test]
    fn get_text_includes_table_cell_paragraphs() {
        let document = document_with_table_cell_text();

        Python::initialize();
        let text = Python::attach(|py| document.get_text(py));

        assert_eq!(text, "outer\ncell");
    }

    #[cfg(not(feature = "extension-module"))]
    #[test]
    fn diagnostic_report_exposes_limit_metadata_to_python() -> PyResult<()> {
        let mut diagnostics = DiagnosticReport::default();
        for index in 0..(MAX_DIAGNOSTIC_ITEMS + 1) {
            diagnostics.push(DiagnosticItem::new(
                DiagnosticSeverity::RecoveredError,
                DiagnosticCategory::InvalidValue,
                format!("invalid value {index}"),
            ));
        }
        let document = document_with_diagnostics(diagnostics);

        Python::initialize();
        Python::attach(|py| {
            let report = document.diagnostic_report(py)?;
            let summary_obj = report
                .get_item("summary")?
                .expect("diagnostic_report should include summary");
            let summary = summary_obj.cast::<PyDict>()?;

            let max_items = summary
                .get_item("max_items")?
                .expect("summary should include max_items")
                .extract::<usize>()?;
            let truncated = summary
                .get_item("truncated")?
                .expect("summary should include truncated")
                .extract::<bool>()?;

            assert_eq!(max_items, MAX_DIAGNOSTIC_ITEMS);
            assert!(truncated);
            Ok(())
        })
    }
}
