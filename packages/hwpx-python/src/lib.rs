#![allow(clippy::useless_conversion)]

use std::cell::RefCell;
use std::collections::HashMap;

use hwp_core::viewer::html::{to_html, HtmlOptions};
use hwp_core::viewer::markdown::{to_markdown, MarkdownOptions};
use hwp_core::{HwpDocument, HwpParser};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

/// Format version DWORD to "M.n.P.r" string
/// Format: 0xMMnnPPrr (e.g., 0x05000300 = "5.0.3.0")
fn format_version(version: u32) -> String {
    let major = (version >> 24) & 0xFF;
    let minor = (version >> 16) & 0xFF;
    let patch = (version >> 8) & 0xFF;
    let revision = version & 0xFF;
    format!("{major}.{minor}.{patch}.{revision}")
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
        let result = to_markdown(&self.inner, &options);
        self.cache.borrow_mut().insert(key, result.clone());
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
    fn to_html(&self, image_output_dir: Option<String>) -> String {
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
        let result = to_html(&self.inner, &options);
        self.cache.borrow_mut().insert(key, result.clone());
        result
    }

    /// Convert document to JSON
    ///
    /// Returns:
    ///     JSON string representation of the document
    fn to_json(&self) -> PyResult<String> {
        if let Some(cached) = self.cache.borrow().get(&CacheKey::Json) {
            return Ok(cached.clone());
        }
        let result = serde_json::to_string_pretty(&self.inner)
            .map_err(|e| PyValueError::new_err(format!("JSON serialization error: {e}")))?;
        self.cache
            .borrow_mut()
            .insert(CacheKey::Json, result.clone());
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
        let report = PyDict::new_bound(py);
        let items = PyList::empty_bound(py);

        for item in &self.inner.diagnostics.items {
            let item_dict = PyDict::new_bound(py);
            item_dict.set_item("severity", item.severity.as_str())?;
            item_dict.set_item("category", item.category.as_str())?;
            item_dict.set_item("message", &item.message)?;

            let context = PyDict::new_bound(py);
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
        let summary_dict = PyDict::new_bound(py);
        summary_dict.set_item("total", summary.total)?;

        let by_severity = PyDict::new_bound(py);
        for (key, value) in summary.by_severity {
            by_severity.set_item(key, value)?;
        }
        summary_dict.set_item("by_severity", by_severity)?;

        let by_category = PyDict::new_bound(py);
        for (key, value) in summary.by_category {
            by_category.set_item(key, value)?;
        }
        summary_dict.set_item("by_category", by_category)?;

        report.set_item("items", items)?;
        report.set_item("summary", summary_dict)?;
        Ok(report)
    }

    /// Get plain text content from the document
    fn get_text(&self) -> String {
        if let Some(cached) = self.cache.borrow().get(&CacheKey::Text) {
            return cached.clone();
        }
        let mut result = String::new();
        let mut first = true;
        for section in &self.inner.body_text.sections {
            for paragraph in &section.paragraphs {
                for record in &paragraph.records {
                    if let hwp_core::document::bodytext::ParagraphRecord::ParaText { data } = record
                    {
                        let trimmed = data.text.trim();
                        if !trimmed.is_empty() {
                            if !first {
                                result.push('\n');
                            }
                            result.push_str(trimmed);
                            first = false;
                        }
                    }
                }
            }
        }
        self.cache
            .borrow_mut()
            .insert(CacheKey::Text, result.clone());
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
fn parse(data: &[u8]) -> PyResult<Document> {
    let parser = HwpParser::new();
    match parser.parse(data) {
        Ok(doc) => Ok(Document {
            inner: doc,
            cache: RefCell::new(HashMap::new()),
        }),
        Err(e) => Err(PyValueError::new_err(format!("Parse error: {e}"))),
    }
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
fn parse_file(path: &str) -> PyResult<Document> {
    let data = std::fs::read(path)
        .map_err(|e| PyValueError::new_err(format!("Failed to read file: {e}")))?;
    parse(&data)
}

/// hwpx - Python bindings for HWP/HWPX document parser
///
/// This module provides functions to parse and convert HWP/HWPX documents.
///
/// Example:
///     >>> import hwpx
///     >>> doc = hwpx.parse_file("document.hwpx")
///     >>> print(doc.to_markdown())
///     >>> print(doc.to_html())
///     >>> print(doc.get_text())
#[pymodule]
fn hwpx(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(parse, m)?)?;
    m.add_function(wrap_pyfunction!(parse_file, m)?)?;
    m.add_class::<Document>()?;
    Ok(())
}
