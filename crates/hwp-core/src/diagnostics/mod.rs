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
