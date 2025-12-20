/// HWPX BinData parser
///
/// BinData folder contains binary files like images, OLE objects, etc.
use base64::{engine::general_purpose::STANDARD, Engine as _};

use crate::document::bindata::{BinData, BinaryDataItem};
use crate::error::HwpError;
use crate::types::WORD;

use super::container::HwpxContainer;

/// Parse BinData folder and create BinData structure
pub fn parse_bindata(container: &mut HwpxContainer) -> Result<BinData, HwpError> {
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
                // Log warning but continue parsing
                #[cfg(debug_assertions)]
                eprintln!("Warning: Failed to read BinData file {file_path}: {e}");
            }
        }
    }

    Ok(BinData { items })
}

/// Get the file extension from a BinData path
#[allow(dead_code)]
pub fn get_extension(path: &str) -> Option<&str> {
    path.rsplit('.').next()
}

/// Get the MIME type from file extension
#[allow(dead_code)]
pub fn get_mime_type(extension: &str) -> &'static str {
    match extension.to_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "tiff" | "tif" => "image/tiff",
        "ico" => "image/x-icon",
        "emf" => "image/emf",
        "wmf" => "image/wmf",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_extension() {
        assert_eq!(get_extension("BinData/image1.png"), Some("png"));
        assert_eq!(get_extension("BinData/photo.jpeg"), Some("jpeg"));
        assert_eq!(get_extension("noextension"), Some("noextension"));
    }

    #[test]
    fn test_get_mime_type() {
        assert_eq!(get_mime_type("png"), "image/png");
        assert_eq!(get_mime_type("JPG"), "image/jpeg");
        assert_eq!(get_mime_type("unknown"), "application/octet-stream");
    }
}
