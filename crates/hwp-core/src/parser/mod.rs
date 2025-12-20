/// Parser module for HWP/HWPX file format detection and parsing
///
/// This module provides format detection and parsing for both HWP 5.0 (CFB-based)
/// and HWPX (ZIP-based) file formats.
pub mod detect;
pub mod hwpx;

pub use detect::{detect_format, FileFormat};
