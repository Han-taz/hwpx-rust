use crate::error::HwpError;
use crate::types::decode_utf16le;
use crate::types::{UINT16, UINT32, UINT8};

use super::types::CtrlHeaderData;

/// 필드 파싱 (표 152) / Parse field (Table 152)
/// FIELD_START (%%%%) 컨트롤의 경우 데이터에서 field_type을 읽음
/// For FIELD_START (%%%%) control, read field_type from data
pub(crate) fn parse_field(data: &[u8]) -> Result<CtrlHeaderData, HwpError> {
    parse_field_with_type(data, None)
}

/// 필드 타입을 지정하여 필드 파싱 / Parse field with specified field type
/// ctrl_id가 이미 필드 타입인 경우 (예: %hlk, %dte) field_type을 직접 전달
/// When ctrl_id is already field type (e.g., %hlk, %dte), pass field_type directly
pub(crate) fn parse_field_with_type(
    data: &[u8],
    known_field_type: Option<&str>,
) -> Result<CtrlHeaderData, HwpError> {
    let mut offset = 0usize;
    let field_type: String;

    // ctrl_id가 이미 필드 타입인 경우 데이터에서 field_type을 읽지 않음
    // If ctrl_id is already field type, don't read field_type from data
    if let Some(ft) = known_field_type {
        // 최소 11바이트 필요 (attribute 4 + other_attr 1 + command_len 2 + id 4)
        // Minimum 11 bytes needed (attribute 4 + other_attr 1 + command_len 2 + id 4)
        if data.len() < 7 {
            // 데이터가 부족해도 빈 필드로 반환 / Return empty field even if data is insufficient
            return Ok(CtrlHeaderData::Field {
                field_type: ft.to_string(),
                attribute: 0,
                other_attr: 0,
                command_len: 0,
                command: String::new(),
                id: 0,
            });
        }
        field_type = ft.to_string();
    } else {
        // FIELD_START (%%%%)의 경우: 데이터에서 field_type 읽음
        // For FIELD_START (%%%%): read field_type from data
        if data.len() < 15 {
            return Err(HwpError::insufficient_data("Field data", 15, data.len()));
        }
        let field_type_bytes = [
            data[offset + 3],
            data[offset + 2],
            data[offset + 1],
            data[offset],
        ];
        field_type = String::from_utf8_lossy(&field_type_bytes)
            .trim_end_matches('\0')
            .to_string();
        offset += 4;
    }

    // 데이터 경계 검사 / Data boundary check
    if offset + 7 > data.len() {
        return Ok(CtrlHeaderData::Field {
            field_type,
            attribute: 0,
            other_attr: 0,
            command_len: 0,
            command: String::new(),
            id: 0,
        });
    }

    let attribute = UINT32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]);
    offset += 4;

    let other_attr = UINT8::from_le_bytes([data[offset]]);
    offset += 1;

    let command_len = UINT16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
    offset += 2;

    let command = if command_len > 0 && offset + (command_len * 2) <= data.len() {
        let command_bytes = &data[offset..offset + (command_len * 2)];
        decode_utf16le(command_bytes).unwrap_or_default()
    } else {
        String::new()
    };
    offset += command_len * 2;

    let id = if offset + 4 <= data.len() {
        UINT32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ])
    } else {
        0
    };

    Ok(CtrlHeaderData::Field {
        field_type,
        attribute,
        other_attr,
        command_len: command_len as UINT16,
        command,
        id,
    })
}
