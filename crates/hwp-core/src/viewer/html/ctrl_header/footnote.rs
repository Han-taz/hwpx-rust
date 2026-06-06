use super::CtrlHeaderResult;
use crate::document::bodytext::ParagraphRecord;
use crate::document::{CtrlHeader, Paragraph};

/// 각주 처리 / Process footnote
pub fn process_footnote<'a>(
    _header: &'a CtrlHeader,
    _children: &'a [ParagraphRecord],
    _paragraphs: &[Paragraph],
) -> CtrlHeaderResult<'a> {
    // Footnote content is rendered by the shared body control path.
    CtrlHeaderResult::new()
}
