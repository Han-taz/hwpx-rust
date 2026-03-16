use crate::error::HwpError;
use crate::types::{HWPUNIT, HWPUNIT16, UINT32};

use super::types::{Caption, CaptionAlign, CaptionVAlign};

/// LIST_HEADER 레코드에서 캡션 파싱 (hwplib 방식) / Parse caption from LIST_HEADER record (hwplib approach)
pub fn parse_caption_from_list_header(data: &[u8]) -> Result<Option<Caption>, HwpError> {
    if data.len() < 22 {
        return Ok(None);
    }

    let mut offset = 0usize;

    // paraCount (SInt4) - 4 bytes (read but not used)
    offset += 4;

    let list_header_property = UINT32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]);
    offset += 4;

    let vertical_align = match (list_header_property >> 5) & 0x03 {
        0 => CaptionVAlign::Top,
        1 => CaptionVAlign::Middle,
        2 => CaptionVAlign::Bottom,
        _ => CaptionVAlign::Middle,
    };

    let caption_property_value = UINT32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]);
    offset += 4;

    let align = match caption_property_value & 0x03 {
        0 => CaptionAlign::Left,
        1 => CaptionAlign::Right,
        2 => CaptionAlign::Top,
        3 => CaptionAlign::Bottom,
        _ => CaptionAlign::Bottom,
    };

    let include_margin = (caption_property_value & 0x04) != 0;

    let width = HWPUNIT::from(UINT32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]));
    offset += 4;

    let gap = HWPUNIT16::from_le_bytes([data[offset], data[offset + 1]]);
    offset += 2;

    let last_width = HWPUNIT::from(UINT32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]));

    Ok(Some(Caption {
        align,
        include_margin,
        width,
        gap,
        last_width,
        vertical_align: Some(vertical_align),
    }))
}

