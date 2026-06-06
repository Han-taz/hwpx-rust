use quick_xml::events::{BytesEnd, Event};

use crate::error::HwpError;

pub(crate) const MAX_HWPX_XML_EVENTS: u64 = 1_000_000;
pub(crate) const MAX_HWPX_XML_DEPTH: usize = 256;

pub(crate) struct XmlParseBudget<'a> {
    source: &'a str,
    event_count: u64,
    depth: usize,
    element_stack: Vec<Vec<u8>>,
}

impl<'a> XmlParseBudget<'a> {
    pub(crate) fn new(source: &'a str) -> Self {
        Self {
            source,
            event_count: 0,
            depth: 0,
            element_stack: Vec::new(),
        }
    }

    pub(crate) fn observe_event(&mut self, event: &Event<'_>) -> Result<(), HwpError> {
        reject_unsupported_xml_event(self.source, event)?;

        if !matches!(event, Event::Eof) {
            self.event_count = self.event_count.saturating_add(1);
            validate_xml_event_count(self.source, self.event_count)?;
        }

        match event {
            Event::Start(e) => {
                self.depth = self.depth.saturating_add(1);
                validate_xml_depth(self.source, self.depth)?;
                self.element_stack.push(e.name().as_ref().to_vec());
            }
            Event::Empty(_) => {
                validate_xml_depth(self.source, self.depth.saturating_add(1))?;
            }
            Event::Eof => {
                if let Some(unclosed_name) = self.element_stack.last() {
                    return Err(HwpError::InvalidHwpxStructure {
                        reason: format!(
                            "Unclosed HWPX XML start tag <{}> in {}",
                            xml_name_for_display(unclosed_name),
                            self.source
                        ),
                    });
                }
            }
            _ => {}
        }

        Ok(())
    }

    pub(crate) fn finish_end_event(&mut self, end: &BytesEnd<'_>) -> Result<(), HwpError> {
        let actual = end.name();
        let actual = actual.as_ref();

        let Some(expected) = self.element_stack.pop() else {
            return Err(HwpError::InvalidHwpxStructure {
                reason: format!(
                    "unexpected XML end tag </{}> in {}",
                    xml_name_for_display(actual),
                    self.source
                ),
            });
        };

        if expected.as_slice() != actual {
            return Err(HwpError::InvalidHwpxStructure {
                reason: format!(
                    "mismatched XML end tag in {}: expected </{}>, got </{}>",
                    self.source,
                    xml_name_for_display(&expected),
                    xml_name_for_display(actual),
                ),
            });
        }

        self.depth = self.depth.saturating_sub(1);
        Ok(())
    }
}

fn xml_name_for_display(name: &[u8]) -> String {
    String::from_utf8_lossy(name).into_owned()
}

fn reject_unsupported_xml_event(path: &str, event: &Event<'_>) -> Result<(), HwpError> {
    match event {
        Event::DocType(_) => Err(HwpError::InvalidHwpxStructure {
            reason: format!("Unsupported HWPX XML DOCTYPE declaration in {path}"),
        }),
        Event::PI(_) => Err(HwpError::InvalidHwpxStructure {
            reason: format!("Unsupported HWPX XML processing instruction in {path}"),
        }),
        _ => Ok(()),
    }
}

pub(crate) fn validate_xml_event_count(path: &str, event_count: u64) -> Result<(), HwpError> {
    if event_count > MAX_HWPX_XML_EVENTS {
        return Err(HwpError::ResourceLimitExceeded {
            resource: "HWPX XML event count",
            path: path.to_string(),
            limit: MAX_HWPX_XML_EVENTS,
            actual: event_count,
        });
    }

    Ok(())
}

pub(crate) fn validate_xml_depth(path: &str, depth: usize) -> Result<(), HwpError> {
    if depth > MAX_HWPX_XML_DEPTH {
        return Err(HwpError::ResourceLimitExceeded {
            resource: "HWPX XML nesting depth",
            path: path.to_string(),
            limit: MAX_HWPX_XML_DEPTH as u64,
            actual: depth as u64,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use quick_xml::events::{BytesEnd, BytesStart};
    use quick_xml::Reader;

    fn observe_all_events(xml: &str) -> Result<(), HwpError> {
        let mut reader = Reader::from_str(xml);
        let mut budget = XmlParseBudget::new("Contents/section0.xml");

        loop {
            let event = reader
                .read_event()
                .map_err(|err| HwpError::XmlParseError(err.to_string()))?;
            budget.observe_event(&event)?;
            if let Event::End(ref e) = event {
                budget.finish_end_event(e)?;
            }
            if matches!(event, Event::Eof) {
                break;
            }
        }

        Ok(())
    }

    #[test]
    fn xml_declaration_is_allowed() {
        observe_all_events(r#"<?xml version="1.0" encoding="UTF-8"?><root/>"#)
            .expect("XML declaration should be allowed");
    }

    #[test]
    fn doctype_is_rejected() {
        let err = observe_all_events(r#"<!DOCTYPE root [<!ENTITY x "y">]><root/>"#)
            .expect_err("HWPX XML DTDs should be rejected");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("DOCTYPE")
                    && reason.contains("Contents/section0.xml")
        ));
    }

    #[test]
    fn processing_instruction_is_rejected() {
        let err = observe_all_events(r#"<?xml-stylesheet href="x" type="text/xsl"?><root/>"#)
            .expect_err("HWPX XML processing instructions should be rejected");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("processing instruction")
                    && reason.contains("Contents/section0.xml")
        ));
    }

    #[test]
    fn mismatched_end_tag_is_rejected() {
        let mut budget = XmlParseBudget::new("Contents/section0.xml");
        let root = Event::Start(BytesStart::new("root"));
        let paragraph = Event::Start(BytesStart::new("paragraph"));
        budget
            .observe_event(&root)
            .expect("root start should be observed");
        budget
            .observe_event(&paragraph)
            .expect("paragraph start should be observed");

        let err = budget
            .finish_end_event(&BytesEnd::new("root"))
            .expect_err("mismatched HWPX XML end tags should be rejected");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("mismatched XML end tag")
                    && reason.contains("Contents/section0.xml")
        ));
    }

    #[test]
    fn unopened_end_tag_is_rejected() {
        let mut budget = XmlParseBudget::new("Contents/section0.xml");
        let err = budget
            .finish_end_event(&BytesEnd::new("root"))
            .expect_err("HWPX XML end tags without start tags should be rejected");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("unexpected XML end tag")
                    && reason.contains("Contents/section0.xml")
        ));
    }
}
