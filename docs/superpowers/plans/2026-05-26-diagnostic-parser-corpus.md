# Diagnostic Parser and Corpus Compatibility Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add structured parser diagnostics, corpus compatibility tracking, and open-source baseline polish without breaking existing warnings or parser APIs.

**Architecture:** Add a focused `diagnostics` module to `hwp-core`, attach a defaulted `DiagnosticReport` to `HwpDocument`, and thread it through the HWPX parser beside existing `ParseWarnings`. Use a manifest-driven corpus test over existing fixtures so reliability is measured by diagnostic severity/category thresholds, then expose the structured report through Rust, JSON, and Python.

**Tech Stack:** Rust 2021, `serde`, `quick-xml`, `toml` as a dev-dependency for corpus tests, PyO3 0.22, existing `cargo test`/`clippy`/`fmt` CI workflow.

---

## Spec Reference

- Design spec: `docs/superpowers/specs/2026-05-26-diagnostic-parser-corpus-design.md`

## File Structure

Create:

- `crates/hwp-core/src/diagnostics/mod.rs` - structured diagnostic types, summary counts, helpers, unit tests.
- `crates/hwp-core/tests/corpus/manifest.toml` - curated fixture expectations.
- `crates/hwp-core/tests/corpus_tests.rs` - manifest-driven compatibility test.
- `LICENSE` - MIT license matching package metadata.
- `CHANGELOG.md` - release history and unreleased section.
- `CONTRIBUTING.md` - local setup, checks, fixture/snapshot guidance.
- `SECURITY.md` - supported versions and vulnerability reporting policy.

Modify:

- `crates/hwp-core/src/lib.rs` - expose `diagnostics` module and re-export public types.
- `crates/hwp-core/src/document/mod.rs` - add `diagnostics: DiagnosticReport` to `HwpDocument`.
- `crates/hwp-core/src/parser/hwpx/mod.rs` - pass `document.diagnostics` into HWPX parser stages.
- `crates/hwp-core/src/parser/hwpx/header.rs` - thread diagnostics and record invalid values.
- `crates/hwp-core/src/parser/hwpx/section.rs` - thread diagnostics and record invalid values/unsupported elements.
- `crates/hwp-core/src/parser/hwpx/bindata.rs` - thread diagnostics and record failed binary reads.
- `crates/hwp-core/Cargo.toml` - fix repository metadata and add `toml` dev-dependency.
- `packages/hwpx-python/src/lib.rs` - add `Document.diagnostic_report()`.
- `packages/hwpx-python/python/hwpxkit/__init__.pyi` - add type stub for `diagnostic_report`.
- `packages/hwpx-python/pyproject.toml` - fix project URLs.
- `README.md` - document support status, diagnostics, corpus checks, and verification commands.
- `packages/hwpx-python/README.md` - document Python diagnostics API.

---

## Task 1: Diagnostics Foundation

**Files:**
- Create: `crates/hwp-core/src/diagnostics/mod.rs`
- Modify: `crates/hwp-core/src/lib.rs`

- [ ] **Step 1: Add failing diagnostics unit tests**

Create `crates/hwp-core/src/diagnostics/mod.rs` with only tests and the minimum imports needed for the tests to compile once implementation exists. Add `pub mod diagnostics;` to `crates/hwp-core/src/lib.rs` so the file is compiled.

Use these tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_counts_by_severity_and_category() {
        let mut report = DiagnosticReport::default();
        report.push(
            DiagnosticItem::new(
                DiagnosticSeverity::RecoveredError,
                DiagnosticCategory::InvalidValue,
                "Invalid tab width value",
            )
            .with_context(
                DiagnosticContext::new()
                    .with_source("Contents/section0.xml")
                    .with_section_index(0)
                    .with_element("hp:tab")
                    .with_attribute("width")
                    .with_value("wide")
                    .with_component("hwpx::section"),
            ),
        );
        report.push(DiagnosticItem::new(
            DiagnosticSeverity::DataLoss,
            DiagnosticCategory::SkippedBinary,
            "Failed to read BinData/BIN0001.jpg",
        ));

        let summary = report.summary();
        assert_eq!(summary.total, 2);
        assert_eq!(summary.by_severity.get("RecoveredError"), Some(&1));
        assert_eq!(summary.by_severity.get("DataLoss"), Some(&1));
        assert_eq!(summary.by_category.get("InvalidValue"), Some(&1));
        assert_eq!(summary.by_category.get("SkippedBinary"), Some(&1));
    }

    #[test]
    fn diagnostic_report_serializes_stable_shape() {
        let mut report = DiagnosticReport::default();
        report.push(DiagnosticItem::new(
            DiagnosticSeverity::Unsupported,
            DiagnosticCategory::UnsupportedElement,
            "Unsupported HWPX element hp:chart",
        ));

        let value = serde_json::to_value(&report).expect("report should serialize");
        assert!(value.get("items").is_some());
        assert_eq!(value["items"][0]["severity"], "Unsupported");
        assert_eq!(value["items"][0]["category"], "UnsupportedElement");
    }
}
```

- [ ] **Step 2: Run diagnostics tests and verify they fail**

Run: `cargo test -p hwp-core diagnostics --lib`

Expected: FAIL with missing `DiagnosticReport`, `DiagnosticItem`, `DiagnosticSeverity`, `DiagnosticCategory`, or `DiagnosticContext` definitions.

- [ ] **Step 3: Implement diagnostics types**

Replace the test-only file content with the implementation plus tests:

```rust
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticReport {
    pub items: Vec<DiagnosticItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticItem {
    pub severity: DiagnosticSeverity,
    pub category: DiagnosticCategory,
    pub message: String,
    pub context: DiagnosticContext,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    RecoveredError,
    Unsupported,
    DataLoss,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DiagnosticCategory {
    UnsupportedElement,
    UnsupportedAttribute,
    InvalidValue,
    MissingOptionalPart,
    RecoveredXml,
    SkippedBinary,
    RendererLossHint,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticContext {
    pub source: Option<String>,
    pub section_index: Option<u16>,
    pub element: Option<String>,
    pub attribute: Option<String>,
    pub value: Option<String>,
    pub offset: Option<u64>,
    pub component: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticSummary {
    pub total: usize,
    pub by_severity: BTreeMap<String, usize>,
    pub by_category: BTreeMap<String, usize>,
}
```

Add impl blocks:

```rust
impl DiagnosticReport {
    pub fn push(&mut self, item: DiagnosticItem) {
        self.items.push(item);
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn summary(&self) -> DiagnosticSummary {
        let mut summary = DiagnosticSummary {
            total: self.items.len(),
            by_severity: BTreeMap::new(),
            by_category: BTreeMap::new(),
        };

        for item in &self.items {
            *summary
                .by_severity
                .entry(item.severity.as_str().to_string())
                .or_insert(0) += 1;
            *summary
                .by_category
                .entry(item.category.as_str().to_string())
                .or_insert(0) += 1;
        }

        summary
    }
}

impl DiagnosticItem {
    pub fn new(
        severity: DiagnosticSeverity,
        category: DiagnosticCategory,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            category,
            message: message.into(),
            context: DiagnosticContext::default(),
            suggestion: None,
        }
    }

    pub fn with_context(mut self, context: DiagnosticContext) -> Self {
        self.context = context;
        self
    }

    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }
}

impl DiagnosticSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "Info",
            Self::Warning => "Warning",
            Self::RecoveredError => "RecoveredError",
            Self::Unsupported => "Unsupported",
            Self::DataLoss => "DataLoss",
        }
    }
}

impl DiagnosticCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedElement => "UnsupportedElement",
            Self::UnsupportedAttribute => "UnsupportedAttribute",
            Self::InvalidValue => "InvalidValue",
            Self::MissingOptionalPart => "MissingOptionalPart",
            Self::RecoveredXml => "RecoveredXml",
            Self::SkippedBinary => "SkippedBinary",
            Self::RendererLossHint => "RendererLossHint",
        }
    }
}
```

Add builder methods to `DiagnosticContext`:

```rust
impl DiagnosticContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn with_section_index(mut self, section_index: u16) -> Self {
        self.section_index = Some(section_index);
        self
    }

    pub fn with_element(mut self, element: impl Into<String>) -> Self {
        self.element = Some(element.into());
        self
    }

    pub fn with_attribute(mut self, attribute: impl Into<String>) -> Self {
        self.attribute = Some(attribute.into());
        self
    }

    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    pub fn with_offset(mut self, offset: u64) -> Self {
        self.offset = Some(offset);
        self
    }

    pub fn with_component(mut self, component: impl Into<String>) -> Self {
        self.component = Some(component.into());
        self
    }
}
```

Keep the tests from Step 1 at the bottom of the same file.

- [ ] **Step 4: Add diagnostics re-exports**

Modify `crates/hwp-core/src/lib.rs`:

```rust
pub mod diagnostics;
```

If Step 1 already added this line, do not add it a second time.

Add public re-exports near the existing `pub use` block:

```rust
pub use diagnostics::{
    DiagnosticCategory, DiagnosticContext, DiagnosticItem, DiagnosticReport, DiagnosticSeverity,
    DiagnosticSummary,
};
```

- [ ] **Step 5: Run diagnostics tests and verify they pass**

Run: `cargo test -p hwp-core diagnostics --lib`

Expected: PASS for the diagnostics unit tests.

- [ ] **Step 6: Commit diagnostics foundation**

Run:

```bash
git add crates/hwp-core/src/diagnostics/mod.rs crates/hwp-core/src/lib.rs
git commit -m "feat: add parser diagnostic report types"
```

---

## Task 2: Document Integration and JSON Compatibility

**Files:**
- Modify: `crates/hwp-core/src/document/mod.rs`
- Modify: `crates/hwp-core/src/lib.rs`
- Test: `crates/hwp-core/src/document/mod.rs`

- [ ] **Step 1: Add failing document diagnostics tests**

Add this test module near the bottom of `crates/hwp-core/src/document/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{DiagnosticCategory, DiagnosticItem, DiagnosticSeverity};

    fn test_file_header() -> FileHeader {
        FileHeader {
            signature: "HWP Document File".to_string(),
            version: 0x05010000,
            document_flags: 0,
            license_flags: 0,
            encrypt_version: 0,
            kogl_country: 0,
            reserved: vec![0; 207],
        }
    }

    #[test]
    fn new_document_has_empty_diagnostics() {
        let doc = HwpDocument::new(test_file_header());
        assert!(doc.diagnostics.is_empty());
    }

    #[test]
    fn diagnostics_field_defaults_when_missing_from_json() {
        let original = HwpDocument::new(test_file_header());
        let mut value = serde_json::to_value(&original).expect("document should serialize");
        let object = value
            .as_object_mut()
            .expect("document JSON should be an object");
        object.remove("diagnostics");
        let file_header = object
            .get_mut("file_header")
            .and_then(|file_header| file_header.as_object_mut())
            .expect("file_header should be an object");
        file_header.insert("version".to_string(), serde_json::json!(0x05010000u32));
        file_header.insert("document_flags".to_string(), serde_json::json!(0u32));
        file_header.insert("license_flags".to_string(), serde_json::json!(0u32));
        file_header.insert("reserved".to_string(), serde_json::json!([]));

        let doc: HwpDocument = serde_json::from_value(value).expect("old JSON should deserialize");
        assert!(doc.diagnostics.is_empty());
    }

    #[test]
    fn document_json_includes_diagnostics() {
        let mut doc = HwpDocument::new(test_file_header());
        doc.diagnostics.push(DiagnosticItem::new(
            DiagnosticSeverity::Unsupported,
            DiagnosticCategory::UnsupportedElement,
            "Unsupported element",
        ));

        let value = serde_json::to_value(&doc).expect("document should serialize");
        assert!(value.get("diagnostics").is_some());
        assert_eq!(value["diagnostics"]["items"][0]["severity"], "Unsupported");
    }
}
```

- [ ] **Step 2: Run document tests and verify they fail**

Run: `cargo test -p hwp-core document::tests:: --lib`

Expected: FAIL because `HwpDocument` has no `diagnostics` field.

- [ ] **Step 3: Add diagnostics to `HwpDocument`**

Modify `crates/hwp-core/src/document/mod.rs`:

```rust
/// Structured parser diagnostics / 구조화된 파서 진단 정보
#[serde(default)]
pub diagnostics: crate::diagnostics::DiagnosticReport,
```

Add it after `warnings` or immediately before `warnings`; keep both fields public.

Update `HwpDocument::new`:

```rust
diagnostics: crate::diagnostics::DiagnosticReport::default(),
warnings: crate::error::ParseWarnings::new(),
```

- [ ] **Step 4: Run document tests and verify they pass**

Run: `cargo test -p hwp-core document::tests:: --lib`

Expected: PASS.

- [ ] **Step 5: Run JSON snapshot tests and review structural additions**

Run: `cargo test -p hwp-core hwpx_json_snapshots`

Expected: FAIL when JSON snapshots do not yet include `diagnostics`. Review generated `.snap.new` files and confirm the only expected structural addition is an empty diagnostics report unless fixture diagnostics were already populated.

- [ ] **Step 6: Accept intentional JSON snapshot updates**

Run: `cargo insta accept --workspace`

Expected: updated JSON snapshots include `"diagnostics": { "items": [] }` or the equivalent serialized empty report.

- [ ] **Step 7: Commit document integration**

Run:

```bash
git add crates/hwp-core/src/document/mod.rs crates/hwp-core/tests/snapshots
git commit -m "feat: attach diagnostics to parsed documents"
```

---

## Task 3: Thread Diagnostics Through HWPX Parser

**Files:**
- Modify: `crates/hwp-core/src/parser/hwpx/mod.rs`
- Modify: `crates/hwp-core/src/parser/hwpx/header.rs`
- Modify: `crates/hwp-core/src/parser/hwpx/section.rs`
- Modify: `crates/hwp-core/src/parser/hwpx/bindata.rs`

- [ ] **Step 1: Change function signatures without adding new behavior**

Update HWPX functions to accept both warnings and diagnostics:

```rust
use crate::diagnostics::DiagnosticReport;
```

Target signatures:

```rust
pub fn parse_doc_info(
    container: &mut HwpxContainer,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<DocInfo, HwpError>
```

```rust
fn parse_header_xml_content(
    reader: &mut Reader<&[u8]>,
    doc_info: &mut DocInfo,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<(), HwpError>
```

```rust
pub fn parse_sections(
    container: &mut HwpxContainer,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<BodyText, HwpError>
```

```rust
fn parse_section_xml(
    content: &str,
    index: WORD,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<Section, HwpError>
```

```rust
pub fn parse_bindata(
    container: &mut HwpxContainer,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<BinData, HwpError>
```

Leave `diagnostics` unused temporarily with `_diagnostics` only if the file has not yet been instrumented. Prefer using the real name once Task 4 starts.

- [ ] **Step 2: Update HWPX parse caller**

Modify `crates/hwp-core/src/parser/hwpx/mod.rs`:

```rust
document.doc_info = header::parse_doc_info(
    &mut container,
    &mut document.warnings,
    &mut document.diagnostics,
)?;

document.body_text = section::parse_sections(
    &mut container,
    &mut document.warnings,
    &mut document.diagnostics,
)?;

document.bin_data = bindata::parse_bindata(
    &mut container,
    &mut document.warnings,
    &mut document.diagnostics,
)?;
```

- [ ] **Step 3: Update private tests that call parser helpers**

In `crates/hwp-core/src/parser/hwpx/section.rs`, update calls from:

```rust
parse_section_xml(&xml, 0, &mut ParseWarnings::new())
```

to:

```rust
parse_section_xml(
    &xml,
    0,
    &mut ParseWarnings::new(),
    &mut crate::diagnostics::DiagnosticReport::default(),
)
```

Use a small helper in the test module if there are many call sites:

```rust
fn parse_test_section(xml: &str) -> Section {
    parse_section_xml(
        xml,
        0,
        &mut ParseWarnings::new(),
        &mut crate::diagnostics::DiagnosticReport::default(),
    )
    .unwrap()
}
```

- [ ] **Step 4: Run HWPX parser tests**

Run: `cargo test -p hwp-core parser::hwpx --lib`

Expected: PASS after all call sites are updated.

- [ ] **Step 5: Run workspace compile tests**

Run: `cargo test --workspace --no-run`

Expected: PASS compilation for all tests.

- [ ] **Step 6: Commit diagnostics threading**

Run:

```bash
git add crates/hwp-core/src/parser/hwpx/mod.rs crates/hwp-core/src/parser/hwpx/header.rs crates/hwp-core/src/parser/hwpx/section.rs crates/hwp-core/src/parser/hwpx/bindata.rs
git commit -m "refactor: thread diagnostics through hwpx parser"
```

---

## Task 4: HWPX Diagnostic Instrumentation

**Files:**
- Modify: `crates/hwp-core/src/parser/hwpx/mod.rs`
- Modify: `crates/hwp-core/src/parser/hwpx/header.rs`
- Modify: `crates/hwp-core/src/parser/hwpx/section.rs`
- Modify: `crates/hwp-core/src/parser/hwpx/bindata.rs`

- [ ] **Step 1: Add failing invalid tab diagnostic test**

In the `#[cfg(test)]` module of `crates/hwp-core/src/parser/hwpx/section.rs`, add:

```rust
#[test]
fn invalid_tab_width_records_diagnostic() {
    let xml = r#"
        <hp:sec>
          <hp:p>
            <hp:run>
              <hp:tab width="wide" leader="bad"/>
            </hp:run>
          </hp:p>
        </hp:sec>
    "#;

    let mut warnings = ParseWarnings::new();
    let mut diagnostics = crate::diagnostics::DiagnosticReport::default();
    let section = parse_section_xml(xml, 0, &mut warnings, &mut diagnostics).unwrap();

    assert_eq!(section.paragraphs.len(), 1);
    let summary = diagnostics.summary();
    assert_eq!(summary.by_severity.get("RecoveredError"), Some(&2));
    assert_eq!(summary.by_category.get("InvalidValue"), Some(&2));
    assert!(diagnostics
        .items
        .iter()
        .any(|item| item.context.attribute.as_deref() == Some("width")));
    assert!(diagnostics
        .items
        .iter()
        .any(|item| item.context.attribute.as_deref() == Some("leader")));
}
```

- [ ] **Step 2: Add failing unsupported element diagnostic test**

In the same test module, add:

```rust
#[test]
fn unsupported_section_element_records_diagnostic() {
    let xml = r#"
        <hp:sec>
          <hp:p>
            <hp:run>
              <hp:chart id="chart1"/>
            </hp:run>
          </hp:p>
        </hp:sec>
    "#;

    let mut warnings = ParseWarnings::new();
    let mut diagnostics = crate::diagnostics::DiagnosticReport::default();
    let _section = parse_section_xml(xml, 3, &mut warnings, &mut diagnostics).unwrap();

    assert!(diagnostics.items.iter().any(|item| {
        item.severity == crate::diagnostics::DiagnosticSeverity::Unsupported
            && item.category == crate::diagnostics::DiagnosticCategory::UnsupportedElement
            && item.context.section_index == Some(3)
            && item.context.element.as_deref() == Some("hp:chart")
    }));
}
```

- [ ] **Step 3: Add failing unsupported field command diagnostic test**

In the same section test module, add:

```rust
#[test]
fn unsupported_field_type_records_diagnostic() {
    let xml = r#"
        <hp:sec>
          <hp:p>
            <hp:run>
              <hp:fieldBegin type="DATE" id="0">
                <hp:parameters></hp:parameters>
              </hp:fieldBegin>
              <hp:t>2026-05-26</hp:t>
              <hp:fieldEnd beginIDRef="0"/>
            </hp:run>
          </hp:p>
        </hp:sec>
    "#;

    let mut warnings = ParseWarnings::new();
    let mut diagnostics = crate::diagnostics::DiagnosticReport::default();
    let _section = parse_section_xml(xml, 1, &mut warnings, &mut diagnostics).unwrap();

    assert!(diagnostics.items.iter().any(|item| {
        item.severity == crate::diagnostics::DiagnosticSeverity::Unsupported
            && item.category == crate::diagnostics::DiagnosticCategory::UnsupportedAttribute
            && item.context.element.as_deref() == Some("hp:fieldBegin")
            && item.context.attribute.as_deref() == Some("type")
            && item.context.value.as_deref() == Some("DATE")
    }));
}
```

- [ ] **Step 4: Add failing header invalid value diagnostic test**

In `crates/hwp-core/src/parser/hwpx/header.rs`, add a `#[cfg(test)]` module if one does not exist:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{DiagnosticCategory, DiagnosticReport, DiagnosticSeverity};

    #[test]
    fn invalid_char_height_records_diagnostic() {
        let xml = r#"
            <hh:head>
              <hh:charProperties>
                <hh:charPr height="huge" textColor="#000000"></hh:charPr>
              </hh:charProperties>
            </hh:head>
        "#;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(
            &mut reader,
            &mut doc_info,
            &mut warnings,
            &mut diagnostics,
        )
        .unwrap();

        assert!(diagnostics.items.iter().any(|item| {
            item.severity == DiagnosticSeverity::RecoveredError
                && item.category == DiagnosticCategory::InvalidValue
                && item.context.attribute.as_deref() == Some("height")
        }));
    }
}
```

- [ ] **Step 5: Add missing optional HWPX part diagnostic helper test**

In `crates/hwp-core/src/parser/hwpx/mod.rs`, add a focused unit test for a helper that does not require constructing a ZIP container:

```rust
#[test]
fn missing_optional_part_records_info_diagnostic() {
    let mut diagnostics = crate::diagnostics::DiagnosticReport::default();

    record_missing_optional_part(
        false,
        "Preview/PrvText.txt",
        &mut diagnostics,
    );

    assert!(diagnostics.items.iter().any(|item| {
        item.severity == crate::diagnostics::DiagnosticSeverity::Info
            && item.category == crate::diagnostics::DiagnosticCategory::MissingOptionalPart
            && item.context.source.as_deref() == Some("Preview/PrvText.txt")
    }));
}
```

- [ ] **Step 6: Run new diagnostic tests and verify they fail**

Run:

```bash
cargo test -p hwp-core diagnostic --lib
```

Expected: FAIL because parser code still only writes `ParseWarnings` or ignores unsupported elements.

- [ ] **Step 7: Add local diagnostic helper functions**

In `header.rs` and `section.rs`, import:

```rust
use crate::diagnostics::{
    DiagnosticCategory, DiagnosticContext, DiagnosticItem, DiagnosticReport, DiagnosticSeverity,
};
```

Add local helper in `section.rs`:

```rust
fn record_invalid_value(
    diagnostics: &mut DiagnosticReport,
    source: &str,
    section_index: WORD,
    element: &str,
    attribute: &str,
    value: &str,
    message: impl Into<String>,
) {
    diagnostics.push(
        DiagnosticItem::new(
            DiagnosticSeverity::RecoveredError,
            DiagnosticCategory::InvalidValue,
            message,
        )
        .with_context(
            DiagnosticContext::new()
                .with_source(source)
                .with_section_index(section_index)
                .with_element(element)
                .with_attribute(attribute)
                .with_value(value)
                .with_component("hwpx::section"),
        ),
    );
}
```

Add equivalent `record_header_invalid_value` in `header.rs` with source `Contents/header.xml` and component `hwpx::header`.

- [ ] **Step 8: Instrument invalid numeric fallbacks**

For each existing fallback that already pushes `ParseWarning` in `section.rs`, add a diagnostic next to the warning:

```rust
warnings.push(ParseWarning::warning(format!(
    "Invalid tab width value: {value}"
)));
record_invalid_value(
    diagnostics,
    "Contents/section0.xml",
    index,
    "hp:tab",
    "width",
    &value,
    format!("Invalid tab width value: {value}"),
);
```

Use `format!("Contents/section{index}.xml")` instead of the literal source once inside `parse_section_xml`.

Instrument at least:

- tab `leader`
- tab `width`
- paragraph `prIDRef`
- paragraph `styleIDRef`
- table cell `colSpan`
- table cell `rowSpan`

In `header.rs`, instrument at least:

- `charPr height`
- invalid numeric para shape values already using warnings

- [ ] **Step 9: Instrument unsupported field command types**

In the `fieldBegin` handling in `section.rs`, record any non-`HYPERLINK` `type` value as unsupported:

```rust
let mut field_type: Option<String> = None;

for attr in e.attributes().flatten() {
    if attr.key.as_ref() == b"type" {
        let value = String::from_utf8_lossy(&attr.value).into_owned();
        is_hyperlink = value == "HYPERLINK";
        field_type = Some(value);
    }
}

if let Some(value) = field_type {
    if value != "HYPERLINK" {
        diagnostics.push(
            DiagnosticItem::new(
                DiagnosticSeverity::Unsupported,
                DiagnosticCategory::UnsupportedAttribute,
                format!("Unsupported HWPX fieldBegin type {value}"),
            )
            .with_context(
                DiagnosticContext::new()
                    .with_source(format!("Contents/section{index}.xml"))
                    .with_section_index(index)
                    .with_element("hp:fieldBegin")
                    .with_attribute("type")
                    .with_value(value)
                    .with_component("hwpx::section"),
            ),
        );
    }
}
```

Keep existing hyperlink handling unchanged when `type="HYPERLINK"`.

- [ ] **Step 10: Instrument selected unsupported section elements with per-section aggregation**

Add focused helpers in `section.rs`:

```rust
fn local_name_to_string(name: &[u8]) -> String {
    String::from_utf8_lossy(name).into_owned()
}

fn is_explicitly_unsupported_section_element(name: &[u8]) -> bool {
    name.ends_with(b":chart")
        || name == b"chart"
        || name.ends_with(b":ole")
        || name == b"ole"
        || name.ends_with(b":equation")
        || name == b"equation"
        || name.ends_with(b":video")
        || name == b"video"
}
```

Near the other local state in `parse_section_xml`, add:

```rust
let mut unsupported_element_counts: std::collections::BTreeMap<String, usize> =
    std::collections::BTreeMap::new();
```

In `Event::Start` and `Event::Empty` handling, after existing known-element branches have had a chance to run, count occurrences for the explicit unsupported list:

```rust
if is_explicitly_unsupported_section_element(local_name) {
    let element = local_name_to_string(local_name);
    *unsupported_element_counts.entry(element).or_insert(0) += 1;
}
```

Before returning the parsed `Section`, emit one diagnostic per unsupported element name:

```rust
for (element, count) in unsupported_element_counts {
    diagnostics.push(
        DiagnosticItem::new(
            DiagnosticSeverity::Unsupported,
            DiagnosticCategory::UnsupportedElement,
            format!("Unsupported HWPX element {element} occurred {count} time(s)"),
        )
        .with_context(
            DiagnosticContext::new()
                .with_source(format!("Contents/section{index}.xml"))
                .with_section_index(index)
                .with_element(element)
                .with_component("hwpx::section"),
        ),
    );
}
```

Do not add a catch-all unknown-element warning in this task. Aggregating by element name satisfies the no-noisy-repeated-diagnostics requirement while preserving count information in the message.

- [ ] **Step 11: Instrument missing optional HWPX package parts**

In `parser/hwpx/mod.rs`, add:

```rust
fn record_missing_optional_part(
    exists: bool,
    path: &str,
    diagnostics: &mut crate::diagnostics::DiagnosticReport,
) {
    if !exists {
        diagnostics.push(
            crate::diagnostics::DiagnosticItem::new(
                crate::diagnostics::DiagnosticSeverity::Info,
                crate::diagnostics::DiagnosticCategory::MissingOptionalPart,
                format!("Optional HWPX part {path} was not found"),
            )
            .with_context(
                crate::diagnostics::DiagnosticContext::new()
                    .with_source(path)
                    .with_component("hwpx"),
            ),
        );
    }
}
```

Use it in `parse` for the known optional preview text part:

```rust
let has_preview_text = container.file_exists("Preview/PrvText.txt");
record_missing_optional_part(
    has_preview_text,
    "Preview/PrvText.txt",
    &mut document.diagnostics,
);
if has_preview_text {
    if let Ok(text) = container.read_file_string("Preview/PrvText.txt") {
        document.preview_text = Some(crate::document::PreviewText { text });
    }
}
```

Start with `Preview/PrvText.txt` only. Add more optional parts later after deciding whether the added noise is useful.

- [ ] **Step 12: Instrument failed BinData reads**

In `bindata.rs`, import diagnostics types and change the existing error branch:

```rust
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
                .with_source(file_path)
                .with_component("hwpx::bindata"),
        ),
    );
}
```

- [ ] **Step 13: Add missing referenced BinData reconciliation**

In `bindata.rs`, add a helper that runs after both `body_text` and `bin_data` exist:

```rust
pub fn record_missing_referenced_bindata(
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
```

Add recursive collectors in the same file:

```rust
fn collect_hwpx_image_refs_from_paragraph(
    paragraph: &crate::document::Paragraph,
    refs: &mut std::collections::BTreeSet<String>,
) {
    for record in &paragraph.records {
        collect_hwpx_image_refs_from_record(record, refs);
    }
}

fn collect_hwpx_image_refs_from_record(
    record: &crate::document::ParagraphRecord,
    refs: &mut std::collections::BTreeSet<String>,
) {
    match record {
        crate::document::ParagraphRecord::HwpxImage { binary_item_ref } => {
            refs.insert(binary_item_ref.clone());
        }
        crate::document::ParagraphRecord::Table { table } => {
            for cell in &table.cells {
                for paragraph in &cell.paragraphs {
                    collect_hwpx_image_refs_from_paragraph(paragraph, refs);
                }
            }
        }
        crate::document::ParagraphRecord::CtrlHeader { data } => {
            for paragraph in &data.paragraphs {
                collect_hwpx_image_refs_from_paragraph(paragraph, refs);
            }
            for child in &data.children {
                collect_hwpx_image_refs_from_record(child, refs);
            }
        }
        crate::document::ParagraphRecord::ListHeader { data } => {
            for paragraph in &data.paragraphs {
                collect_hwpx_image_refs_from_paragraph(paragraph, refs);
            }
        }
        crate::document::ParagraphRecord::ShapeComponent { data } => {
            for child in &data.children {
                collect_hwpx_image_refs_from_record(child, refs);
            }
        }
        _ => {}
    }
}
```

In `parser/hwpx/mod.rs`, call the helper after `parse_bindata`:

```rust
bindata::record_missing_referenced_bindata(
    &document.body_text,
    &document.bin_data,
    &mut document.diagnostics,
);
```

- [ ] **Step 14: Add missing referenced BinData test**

Add a unit test in `bindata.rs`:

```rust
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
```

- [ ] **Step 15: Run new diagnostic tests and verify they pass**

Run:

```bash
cargo test -p hwp-core diagnostic --lib
```

Expected: PASS.

- [ ] **Step 16: Run HWPX test suite**

Run: `cargo test -p hwp-core hwpx`

Expected: PASS or JSON snapshot-only failures caused by newly populated diagnostics. If snapshots fail, inspect `.snap.new` files and accept only expected diagnostics additions.

If the snapshot changes are expected, run: `cargo insta accept --workspace`

- [ ] **Step 17: Commit HWPX diagnostic instrumentation**

Run:

```bash
git add crates/hwp-core/src/parser/hwpx/mod.rs crates/hwp-core/src/parser/hwpx/header.rs crates/hwp-core/src/parser/hwpx/section.rs crates/hwp-core/src/parser/hwpx/bindata.rs crates/hwp-core/tests/snapshots
git commit -m "feat: record hwpx parser diagnostics"
```

---

## Task 5: Corpus Compatibility Harness

**Files:**
- Create: `crates/hwp-core/tests/corpus/manifest.toml`
- Create: `crates/hwp-core/tests/corpus_tests.rs`
- Modify: `crates/hwp-core/Cargo.toml`

- [ ] **Step 1: Add manifest parser dependency**

Add to `[dev-dependencies]` in `crates/hwp-core/Cargo.toml`:

```toml
toml = "0.8"
```

- [ ] **Step 2: Create initial corpus manifest**

Create `crates/hwp-core/tests/corpus/manifest.toml`:

```toml
[[documents]]
path = "test-hwpx.hwpx"
format = "hwpx"
expect_parse = "success"
max_data_loss = 0
max_recovered_error = 0
allowed_categories = ["MissingOptionalPart"]
expected_sections = 1

[[documents]]
path = "hyperlink.hwpx"
format = "hwpx"
expect_parse = "success"
max_data_loss = 0
max_recovered_error = 0
allowed_categories = ["MissingOptionalPart"]

[[documents]]
path = "linespacing.hwpx"
format = "hwpx"
expect_parse = "success"
max_data_loss = 0
max_recovered_error = 0
allowed_categories = ["MissingOptionalPart"]
```

These are the curated fast corpus subset. `MissingOptionalPart` is allowed because the first diagnostic pass intentionally reports absent `Preview/PrvText.txt` as an informational optional-part diagnostic. Add more fixtures only after their diagnostic baseline is understood.

- [ ] **Step 3: Add corpus test**

Create `crates/hwp-core/tests/corpus_tests.rs`:

```rust
mod common;

use common::find_fixtures_dir;
use hwp_core::HwpParser;
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Debug, Deserialize)]
struct CorpusManifest {
    documents: Vec<CorpusDocument>,
}

#[derive(Debug, Deserialize)]
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
        let data = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("failed to read corpus fixture {}: {e}", path.display()));
        let result = parser.parse(&data);

        match document.expect_parse.as_str() {
            "success" => {
                let parsed = result.unwrap_or_else(|e| {
                    panic!("expected {} to parse successfully, got {e}", document.path)
                });
                assert_eq!(
                    document.format.as_str(),
                    path.extension().and_then(|s| s.to_str()).unwrap_or(""),
                    "manifest format should match fixture extension for {}",
                    document.path
                );

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
                let err = result
                    .expect_err(&format!("expected {} to fail parsing", document.path));
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
            other => panic!("unsupported expect_parse value for {}: {other}", document.path),
        }
    }
}

fn assert_diagnostic_thresholds(
    document: &CorpusDocument,
    report: &hwp_core::DiagnosticReport,
) {
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
            let context = format!(
                "source={:?} section={:?} element={:?}",
                item.context.source, item.context.section_index, item.context.element
            );
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
```

- [ ] **Step 4: Run corpus test and verify the curated subset is clean**

Run: `cargo test -p hwp-core corpus_manifest_documents_match_expectations --test corpus_tests`

Expected: PASS. If this fails, remove the noisy fixture from the initial curated subset and replace it with another existing HWPX fixture that passes strict thresholds. Do not loosen thresholds for the first corpus commit.

- [ ] **Step 5: Ensure missing binary expectation is explicit**

If a fixture emits missing binary diagnostics, the manifest entry must allow:

```toml
max_data_loss = 1
allowed_categories = ["MissingOptionalPart", "SkippedBinary"]
```

Do not hide `SkippedBinary` under a generic warning threshold.

- [ ] **Step 6: Run all corpus tests**

Run: `cargo test -p hwp-core --test corpus_tests`

Expected: PASS.

- [ ] **Step 7: Commit corpus harness**

Run:

```bash
git add crates/hwp-core/Cargo.toml Cargo.lock crates/hwp-core/tests/corpus/manifest.toml crates/hwp-core/tests/corpus_tests.rs
git commit -m "test: add corpus compatibility harness"
```

---

## Task 6: Python Diagnostic Report API

**Files:**
- Modify: `packages/hwpx-python/src/lib.rs`
- Modify: `packages/hwpx-python/python/hwpxkit/__init__.pyi`
- Modify: `packages/hwpx-python/README.md`

- [ ] **Step 1: Declare Python stub API**

Modify `packages/hwpx-python/python/hwpxkit/__init__.pyi` first so the expected API is explicit:

```python
from typing import Any, Dict, Optional
```

Add to `class Document`:

```python
def diagnostic_report(self) -> Dict[str, Any]:
    """
    Return structured parser diagnostics.

    Returns:
        Dictionary with keys: items, summary.
    """
    ...
```

- [ ] **Step 2: Confirm Rust binding does not expose the method yet**

Run:

```bash
rg -n "diagnostic_report|PyDict|PyList" packages/hwpx-python/src/lib.rs packages/hwpx-python/python/hwpxkit/__init__.pyi
```

Expected: the stub file contains `diagnostic_report`; `packages/hwpx-python/src/lib.rs` does not yet contain a `diagnostic_report` method.

- [ ] **Step 3: Implement exact report shape**

Import PyO3 types:

```rust
use pyo3::types::{PyDict, PyList};
```

Implement `diagnostic_report` with this Python shape:

```python
{
    "items": [
        {
            "severity": "RecoveredError",
            "category": "InvalidValue",
            "message": "...",
            "context": {
                "source": "Contents/section0.xml",
                "section_index": 0,
                "element": "hp:tab",
                "attribute": "width",
                "value": "wide",
                "offset": None,
                "component": "hwpx::section",
            },
            "suggestion": None,
        }
    ],
    "summary": {
        "total": 1,
        "by_severity": {"RecoveredError": 1},
        "by_category": {"InvalidValue": 1},
    },
}
```

Use these exact top-level keys: `items`, `summary`. Use these exact summary keys:
`total`, `by_severity`, `by_category`.

Rust implementation outline:

```rust
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
```

- [ ] **Step 4: Run Python package compile check**

Run: `cargo test -p hwpx-python --no-run`

Expected: PASS.

- [ ] **Step 5: Document Python diagnostics API**

Add to `packages/hwpx-python/README.md`:

```python
report = doc.diagnostic_report()
print(report["summary"])
for item in report["items"]:
    print(item["severity"], item["category"], item["message"])
```

Mention that `doc.warnings` remains available for string compatibility.

- [ ] **Step 6: Commit Python API**

Run:

```bash
git add packages/hwpx-python/src/lib.rs packages/hwpx-python/python/hwpxkit/__init__.pyi packages/hwpx-python/README.md
git commit -m "feat: expose parser diagnostics to python"
```

---

## Task 7: Open-Source Baseline

**Files:**
- Create: `LICENSE`
- Create: `CHANGELOG.md`
- Create: `CONTRIBUTING.md`
- Create: `SECURITY.md`
- Modify: `README.md`
- Modify: `crates/hwp-core/Cargo.toml`
- Modify: `packages/hwpx-python/pyproject.toml`

- [ ] **Step 1: Fix repository metadata**

In `crates/hwp-core/Cargo.toml`, replace:

```toml
repository = "https://github.com/ohah/hwpjs"
```

with:

```toml
repository = "https://github.com/Han-taz/hwpx-rust"
```

In `packages/hwpx-python/pyproject.toml`, replace both project URLs with:

```toml
[project.urls]
Homepage = "https://github.com/Han-taz/hwpx-rust"
Repository = "https://github.com/Han-taz/hwpx-rust"
```

- [ ] **Step 2: Add MIT license file**

Create `LICENSE` with standard MIT license text. Use:

```text
Copyright (c) 2026 kevin
```

because workspace metadata currently lists `kevin <ftkevin12@gmail.com>` as author.

- [ ] **Step 3: Add changelog**

Create `CHANGELOG.md`:

```markdown
# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog, and this project follows semantic versioning
while the public API is stabilizing.

## [Unreleased]

### Added
- Structured parser diagnostics for HWPX reliability reporting.
- Corpus compatibility harness for fixture-based parser checks.

## [0.2.0]

### Added
- Performance optimizations for HWPX parsing and conversion.
- Python conversion result caching.

### Changed
- Updated package authorship metadata.
```

- [ ] **Step 4: Add contribution guide**

Create `CONTRIBUTING.md` with:

````markdown
# Contributing

## Local Checks

Run these before opening a pull request:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

## Fixtures and Snapshots

- Put small public test documents in `crates/hwp-core/tests/fixtures/`.
- Do not add private or sensitive documents.
- If a JSON/HTML/Markdown snapshot changes intentionally, inspect the `.snap.new`
  file before accepting it with `cargo insta accept --workspace`.

## Parser Diagnostics

Unsupported or lossy parsing should be represented with structured diagnostics rather
than silently ignored.
````

- [ ] **Step 5: Add security policy**

Create `SECURITY.md`:

```markdown
# Security Policy

## Supported Versions

Security fixes are accepted for the current `main` branch.

## Reporting a Vulnerability

Please report vulnerabilities by opening a private security advisory on GitHub when
available, or by contacting the repository owner. Do not include sensitive documents in
public issues.

## Document Safety

HWP/HWPX files are untrusted input. Parser bugs that can cause crashes, excessive
resource use, or unsafe file writes are treated as security relevant.
```

- [ ] **Step 6: Update README support status**

In `README.md`, add a support status table near "지원 형식":

```markdown
| Format | Status | Notes |
| ------ | ------ | ----- |
| HWP 5.0 | Maintained / frozen | Existing parser support is kept, but new feature work focuses on HWPX. |
| HWPX | Active | Primary target for parser reliability and diagnostics improvements. |
```

Add a diagnostics section:

```markdown
## Diagnostics

The parser exposes both legacy string warnings and structured diagnostics. Diagnostics
identify unsupported elements, recovered invalid values, skipped binary assets, and
likely data loss so conversion problems are visible instead of silently ignored.
```

Add verification commands matching CI.

- [ ] **Step 7: Run metadata/documentation checks**

Run:

```bash
rg -n "ohah/hwpjs|github.com/Han-taz/hwpx-rust|diagnostic_report|Diagnostics" README.md crates/hwp-core/Cargo.toml packages/hwpx-python/pyproject.toml packages/hwpx-python/README.md
```

Expected: no old `ohah/hwpjs` URL remains; new repository URL and diagnostics docs appear.

- [ ] **Step 8: Commit open-source baseline**

Run:

```bash
git add LICENSE CHANGELOG.md CONTRIBUTING.md SECURITY.md README.md crates/hwp-core/Cargo.toml packages/hwpx-python/pyproject.toml packages/hwpx-python/README.md
git commit -m "docs: update open source project baseline"
```

---

## Task 8: Full Verification and Cleanup

**Files:**
- Review only: all files changed by Tasks 1-7

- [ ] **Step 1: Run formatting check**

Run: `cargo fmt --all -- --check`

Expected: PASS. If it fails, run `cargo fmt --all`, inspect the formatting diff, and commit the formatting with the task that introduced it if still uncommitted; otherwise create `chore: format diagnostic parser changes`.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`

Expected: PASS.

- [ ] **Step 3: Run full test suite**

Run: `cargo test --workspace`

Expected: PASS.

- [ ] **Step 4: Review final diff since plan start**

Run: `git log --oneline origin/main..HEAD`

Expected: commits include the spec commit plus implementation commits for diagnostics, document integration, HWPX instrumentation, corpus harness, Python API, and open-source baseline.

Run: `git status --short --branch`

Expected: clean except pre-existing unrelated untracked files such as `claude/`, if still present.

- [ ] **Step 5: Document residual risk**

If any fixture thresholds were loosened in `tests/corpus/manifest.toml`, add a short note to the final implementation summary explaining which fixture emitted diagnostics and why the threshold is accepted.

- [ ] **Step 6: Final handoff**

Report:

- files changed by category
- commands run and pass/fail status
- commits created
- any snapshot updates
- remaining risks or follow-up candidates for conversion-quality work
