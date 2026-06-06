use super::CtrlHeaderResult;
use crate::document::bodytext::ParagraphRecord;
use crate::document::{CtrlHeader, Paragraph};

/// 꼬리말 처리 / Process footer
pub fn process_footer<'a>(
    _header: &'a CtrlHeader,
    _children: &'a [ParagraphRecord],
    _paragraphs: &[Paragraph],
) -> CtrlHeaderResult<'a> {
    // Footer paragraphs are rendered by the shared body control path.
    CtrlHeaderResult::new()
}
