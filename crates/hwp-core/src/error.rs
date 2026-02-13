/// Error types for HWP file parsing
///
/// This module defines all error types that can occur during HWP file parsing.
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Parsing context for better error messages
///
/// Contains location and context information about where an error occurred.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParseContext {
    /// Source file or stream name (e.g., "section0.xml", "BodyText/Section0")
    pub source: Option<String>,
    /// Byte offset in the source
    pub offset: Option<usize>,
    /// Line number (1-based, for XML sources)
    pub line: Option<usize>,
    /// Column number (1-based, for XML sources)
    pub column: Option<usize>,
    /// Parent element or record type
    pub parent: Option<String>,
    /// Current element or record being parsed
    pub element: Option<String>,
}

impl ParseContext {
    /// Create a new empty context
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the source file/stream name
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Set the byte offset
    pub fn with_offset(mut self, offset: usize) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Set line and column (1-based)
    pub fn with_position(mut self, line: usize, column: usize) -> Self {
        self.line = Some(line);
        self.column = Some(column);
        self
    }

    /// Set the parent element
    pub fn with_parent(mut self, parent: impl Into<String>) -> Self {
        self.parent = Some(parent.into());
        self
    }

    /// Set the current element
    pub fn with_element(mut self, element: impl Into<String>) -> Self {
        self.element = Some(element.into());
        self
    }

    /// Check if context has any information
    pub fn is_empty(&self) -> bool {
        self.source.is_none()
            && self.offset.is_none()
            && self.line.is_none()
            && self.parent.is_none()
            && self.element.is_none()
    }
}

impl std::fmt::Display for ParseContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts = Vec::new();

        if let Some(ref source) = self.source {
            parts.push(format!("in '{}'", source));
        }

        if let (Some(line), Some(col)) = (self.line, self.column) {
            parts.push(format!("at line {}:{}", line, col));
        } else if let Some(offset) = self.offset {
            parts.push(format!("at offset {}", offset));
        }

        if let Some(ref element) = self.element {
            parts.push(format!("parsing <{}>", element));
        }

        if let Some(ref parent) = self.parent {
            parts.push(format!("inside <{}>", parent));
        }

        if parts.is_empty() {
            write!(f, "(no context)")
        } else {
            write!(f, "{}", parts.join(" "))
        }
    }
}

/// Warning severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarningSeverity {
    /// Informational message (no action needed)
    Info,
    /// Warning: something unexpected but recoverable
    Warning,
    /// Error that was recovered from (data may be incomplete)
    RecoveredError,
}

/// A parsing warning (non-fatal issue)
///
/// Warnings are collected during parsing and can be inspected after parsing completes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseWarning {
    /// Warning severity
    pub severity: WarningSeverity,
    /// Warning message
    pub message: String,
    /// Context where the warning occurred
    pub context: ParseContext,
    /// Suggested action or workaround (optional)
    pub suggestion: Option<String>,
}

impl ParseWarning {
    /// Create a new info-level warning
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            severity: WarningSeverity::Info,
            message: message.into(),
            context: ParseContext::new(),
            suggestion: None,
        }
    }

    /// Create a new warning
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: WarningSeverity::Warning,
            message: message.into(),
            context: ParseContext::new(),
            suggestion: None,
        }
    }

    /// Create a recovered error warning
    pub fn recovered_error(message: impl Into<String>) -> Self {
        Self {
            severity: WarningSeverity::RecoveredError,
            message: message.into(),
            context: ParseContext::new(),
            suggestion: None,
        }
    }

    /// Add context to the warning
    pub fn with_context(mut self, context: ParseContext) -> Self {
        self.context = context;
        self
    }

    /// Add a suggestion
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }
}

impl std::fmt::Display for ParseWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let severity = match self.severity {
            WarningSeverity::Info => "INFO",
            WarningSeverity::Warning => "WARN",
            WarningSeverity::RecoveredError => "RECOVERED",
        };

        write!(f, "[{}] {}", severity, self.message)?;

        if !self.context.is_empty() {
            write!(f, " ({})", self.context)?;
        }

        if let Some(ref suggestion) = self.suggestion {
            write!(f, " - Suggestion: {}", suggestion)?;
        }

        Ok(())
    }
}

/// Collection of parsing warnings
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParseWarnings {
    warnings: Vec<ParseWarning>,
}

impl ParseWarnings {
    /// Create a new empty warnings collection
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a warning
    pub fn push(&mut self, warning: ParseWarning) {
        self.warnings.push(warning);
    }

    /// Get all warnings
    pub fn warnings(&self) -> &[ParseWarning] {
        &self.warnings
    }

    /// Get warnings by severity
    pub fn by_severity(&self, severity: WarningSeverity) -> Vec<&ParseWarning> {
        self.warnings
            .iter()
            .filter(|w| w.severity == severity)
            .collect()
    }

    /// Check if there are any warnings
    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }

    /// Get the count of warnings
    pub fn len(&self) -> usize {
        self.warnings.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.warnings.is_empty()
    }

    /// Merge another warnings collection into this one
    pub fn merge(&mut self, other: ParseWarnings) {
        self.warnings.extend(other.warnings);
    }

    /// Clear all warnings
    pub fn clear(&mut self) {
        self.warnings.clear();
    }
}

/// Main error type for HWP parsing operations
#[derive(Debug, Clone, Error)]
pub enum HwpError {
    // ===== CFB related errors =====
    /// Failed to parse CFB structure
    #[error("Failed to parse CFB structure: {0}")]
    CfbParse(String),

    /// Stream not found in CFB structure
    #[error("Stream not found: '{stream_name}' (path: {path})")]
    StreamNotFound { stream_name: String, path: String },

    /// Failed to read stream from CFB
    #[error("Failed to read stream '{stream_name}': {reason}")]
    StreamReadError { stream_name: String, reason: String },

    /// CFB file is too small
    #[error("CFB file too small: expected at least {expected} bytes, got {actual} bytes")]
    CfbFileTooSmall { expected: usize, actual: usize },

    /// Invalid directory sector in CFB
    #[error("Invalid CFB directory sector: {reason}")]
    InvalidDirectorySector { reason: String },

    /// Invalid sector size in CFB header
    #[error("Invalid sector size shift: {value} (must be <= 12)")]
    InvalidSectorSize { value: u32 },

    // ===== Decompression errors =====
    /// Failed to decompress data
    #[error("Failed to decompress {format} data: {reason}")]
    DecompressError {
        format: CompressionFormat,
        reason: String,
    },

    // ===== Parsing errors =====
    /// Insufficient data for parsing
    #[error("Insufficient data for field '{field}': expected at least {expected} bytes, got {actual} bytes")]
    InsufficientData {
        field: String,
        expected: usize,
        actual: usize,
    },

    /// Unexpected value encountered during parsing
    #[error("Unexpected value for field '{field}': expected '{expected}', got '{found}'")]
    UnexpectedValue {
        field: String,
        expected: String,
        found: String,
    },

    /// Failed to parse a record
    #[error("Failed to parse record '{record_type}': {reason}")]
    RecordParseError { record_type: String, reason: String },

    /// Failed to parse record tree structure
    #[error("Failed to parse record tree: {reason}")]
    RecordTreeParseError { reason: String },

    // ===== Document structure errors =====
    /// Required stream is missing
    #[error("Required stream missing: '{stream_name}'")]
    RequiredStreamMissing { stream_name: String },

    /// Unsupported document version
    #[error("Unsupported document version: {version} (supported versions: {supported_versions})")]
    UnsupportedVersion {
        version: String,
        supported_versions: String,
    },

    /// Invalid document signature
    #[error("Invalid HWP document signature: expected 'HWP Document File', got '{found}'")]
    InvalidSignature { found: String },

    /// Unknown file format
    #[error("Unknown file format: unable to detect HWP or HWPX format from file header")]
    UnknownFormat,

    /// Unsupported file format (detected but not yet implemented)
    #[error("Unsupported format '{format}': {reason}")]
    UnsupportedFormat { format: String, reason: String },

    // ===== HWPX specific errors =====
    /// Failed to parse ZIP archive (HWPX)
    #[error("Failed to parse ZIP archive: {0}")]
    ZipParseError(String),

    /// Failed to parse XML content (HWPX)
    #[error("Failed to parse XML: {0}")]
    XmlParseError(String),

    /// Required file not found in HWPX archive
    #[error("Required file not found in HWPX: {path}")]
    HwpxFileNotFound { path: String },

    /// Invalid HWPX structure
    #[error("Invalid HWPX structure: {reason}")]
    InvalidHwpxStructure { reason: String },

    // ===== Other errors =====
    /// IO error
    #[error("IO error: {0}")]
    Io(String),

    /// Encoding/decoding error
    #[error("Encoding error: {reason}")]
    EncodingError { reason: String },

    /// JSON serialization error
    #[error("JSON serialization error: {0}")]
    JsonError(String),

    /// Internal error (unexpected situation)
    #[error("Internal error: {message}")]
    InternalError { message: String },

    // ===== Errors with context =====
    /// Parsing error with context information
    #[error("{} ({})", .0.message, .0.context)]
    ParseErrorWithContext(Box<ParseErrorWithContextData>),

    /// Attribute parsing error (recoverable)
    #[error("Failed to parse attribute '{}' with value '{}': {}", .0.attribute, .0.value, .0.reason)]
    AttributeParseError(Box<AttributeParseErrorData>),
}

/// Data for [`HwpError::ParseErrorWithContext`]
#[derive(Debug, Clone)]
pub struct ParseErrorWithContextData {
    pub message: String,
    pub context: ParseContext,
}

/// Data for [`HwpError::AttributeParseError`]
#[derive(Debug, Clone)]
pub struct AttributeParseErrorData {
    pub attribute: String,
    pub value: String,
    pub reason: String,
    pub context: Option<ParseContext>,
}

/// Compression format type
#[derive(Debug, Clone, Copy)]
pub enum CompressionFormat {
    Zlib,
    Deflate,
}

impl std::fmt::Display for CompressionFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompressionFormat::Zlib => write!(f, "zlib"),
            CompressionFormat::Deflate => write!(f, "deflate"),
        }
    }
}

/// Type alias for `Result<T, HwpError>`
///
/// Note: For better clarity in function signatures, consider using `Result<T, HwpError>` directly
/// to make the error type explicit. This type alias is kept for backward compatibility.
pub type HwpResult<T> = Result<T, HwpError>;

impl HwpError {
    /// Create an `InsufficientData` error with field name
    pub fn insufficient_data(field: impl Into<String>, expected: usize, actual: usize) -> Self {
        Self::InsufficientData {
            field: field.into(),
            expected,
            actual,
        }
    }

    /// Create a `StreamNotFound` error
    pub fn stream_not_found(stream_name: impl Into<String>, path: impl Into<String>) -> Self {
        Self::StreamNotFound {
            stream_name: stream_name.into(),
            path: path.into(),
        }
    }

    /// Create a `StreamReadError` error
    pub fn stream_read_error(stream_name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::StreamReadError {
            stream_name: stream_name.into(),
            reason: reason.into(),
        }
    }

    /// Create a `RecordParseError` error
    pub fn record_parse(record_type: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::RecordParseError {
            record_type: record_type.into(),
            reason: reason.into(),
        }
    }

    /// Create a `DecompressError` error
    pub fn decompress_error(format: CompressionFormat, reason: impl Into<String>) -> Self {
        Self::DecompressError {
            format,
            reason: reason.into(),
        }
    }

    /// Create a parsing error with context
    pub fn parse_error_with_context(message: impl Into<String>, context: ParseContext) -> Self {
        Self::ParseErrorWithContext(Box::new(ParseErrorWithContextData {
            message: message.into(),
            context,
        }))
    }

    /// Create an attribute parse error
    pub fn attribute_parse_error(
        attribute: impl Into<String>,
        value: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::AttributeParseError(Box::new(AttributeParseErrorData {
            attribute: attribute.into(),
            value: value.into(),
            reason: reason.into(),
            context: None,
        }))
    }

    /// Create an attribute parse error with context
    pub fn attribute_parse_error_with_context(
        attribute: impl Into<String>,
        value: impl Into<String>,
        reason: impl Into<String>,
        context: ParseContext,
    ) -> Self {
        Self::AttributeParseError(Box::new(AttributeParseErrorData {
            attribute: attribute.into(),
            value: value.into(),
            reason: reason.into(),
            context: Some(context),
        }))
    }

    /// Create an XML parse error with context
    pub fn xml_parse_error_with_context(message: impl Into<String>, context: ParseContext) -> Self {
        Self::ParseErrorWithContext(Box::new(ParseErrorWithContextData {
            message: format!("XML parse error: {}", message.into()),
            context,
        }))
    }
}

/// Conversion from String to HwpError for backward compatibility
impl From<String> for HwpError {
    fn from(s: String) -> Self {
        HwpError::InternalError { message: s }
    }
}

/// Conversion from &str to HwpError for backward compatibility
impl From<&str> for HwpError {
    fn from(s: &str) -> Self {
        HwpError::InternalError {
            message: s.to_string(),
        }
    }
}

/// Conversion from std::io::Error to HwpError
impl From<std::io::Error> for HwpError {
    fn from(err: std::io::Error) -> Self {
        HwpError::Io(err.to_string())
    }
}

/// Conversion from serde_json::Error to HwpError
impl From<serde_json::Error> for HwpError {
    fn from(err: serde_json::Error) -> Self {
        HwpError::JsonError(err.to_string())
    }
}

/// Conversion from HwpError to String for NAPI and other integrations
/// This allows HwpError to be used with napi::Error::from_reason
impl From<HwpError> for String {
    fn from(err: HwpError) -> Self {
        err.to_string()
    }
}
