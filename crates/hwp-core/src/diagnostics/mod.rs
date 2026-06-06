use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_DIAGNOSTIC_ITEMS: usize = 10_000;

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
    DiagnosticLimit,
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
    pub max_items: usize,
    pub truncated: bool,
    pub by_severity: BTreeMap<String, usize>,
    pub by_category: BTreeMap<String, usize>,
}

impl DiagnosticReport {
    pub fn push(&mut self, item: DiagnosticItem) {
        if self.items.len() >= MAX_DIAGNOSTIC_ITEMS {
            return;
        }

        if self.items.len() == MAX_DIAGNOSTIC_ITEMS - 1 {
            self.items.push(DiagnosticItem::new(
                DiagnosticSeverity::Warning,
                DiagnosticCategory::DiagnosticLimit,
                "Additional parser diagnostics suppressed after reaching the diagnostic item limit",
            ));
            return;
        }

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
            max_items: MAX_DIAGNOSTIC_ITEMS,
            truncated: self
                .items
                .iter()
                .any(|item| item.category == DiagnosticCategory::DiagnosticLimit),
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
            Self::DiagnosticLimit => "DiagnosticLimit",
        }
    }
}

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
        let value = serde_json::to_value(&summary).expect("summary should serialize");
        assert_eq!(summary.total, 2);
        assert_eq!(value["max_items"], MAX_DIAGNOSTIC_ITEMS);
        assert_eq!(value["truncated"], false);
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

    #[test]
    fn diagnostic_report_caps_items_and_records_suppression() {
        let mut report = DiagnosticReport::default();

        for index in 0..(MAX_DIAGNOSTIC_ITEMS + 10) {
            report.push(DiagnosticItem::new(
                DiagnosticSeverity::RecoveredError,
                DiagnosticCategory::InvalidValue,
                format!("invalid value {index}"),
            ));
        }

        assert_eq!(report.len(), MAX_DIAGNOSTIC_ITEMS);
        assert_eq!(
            report.items[MAX_DIAGNOSTIC_ITEMS - 1].category,
            DiagnosticCategory::DiagnosticLimit
        );
        assert_eq!(
            report.items[MAX_DIAGNOSTIC_ITEMS - 1].severity,
            DiagnosticSeverity::Warning
        );
        assert!(report.items[MAX_DIAGNOSTIC_ITEMS - 1]
            .message
            .contains("Additional parser diagnostics suppressed"));

        let summary = report.summary();
        let value = serde_json::to_value(&summary).expect("summary should serialize");
        assert_eq!(summary.total, MAX_DIAGNOSTIC_ITEMS);
        assert_eq!(value["max_items"], MAX_DIAGNOSTIC_ITEMS);
        assert_eq!(value["truncated"], true);
        assert_eq!(summary.by_category.get("DiagnosticLimit"), Some(&1));
        assert_eq!(
            summary.by_category.get("InvalidValue"),
            Some(&(MAX_DIAGNOSTIC_ITEMS - 1))
        );
    }
}
