/// HWPX BinData parser
///
/// BinData folder contains binary files like images, OLE objects, etc.
use base64::{engine::general_purpose::STANDARD, Engine as _};

use crate::document::bindata::{BinData, BinaryDataItem};
use crate::error::{HwpError, ParseWarning, ParseWarnings};
use crate::types::WORD;

use super::container::HwpxContainer;

/// Parse BinData folder and create BinData structure
pub fn parse_bindata(container: &mut HwpxContainer, warnings: &mut ParseWarnings) -> Result<BinData, HwpError> {
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
            }
        }
    }

    Ok(BinData { items })
}

