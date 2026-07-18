use std::str::FromStr;

use quick_xml::events::{attributes::Attribute, BytesStart};
use quick_xml::XmlVersion;

use crate::error::HwpError;

pub(crate) const MAX_HWPX_XML_ATTRIBUTE_VALUE_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
pub(crate) enum XmlAttributeValueError {
    InvalidValue(String),
    XmlParse(HwpError),
}

pub(crate) fn for_each_xml_attribute<'a, F>(
    source: &str,
    element: &'a BytesStart<'_>,
    mut f: F,
) -> Result<(), HwpError>
where
    F: FnMut(Attribute<'a>) -> Result<(), HwpError>,
{
    for attr in element.attributes() {
        let attr = attr.map_err(|err| {
            let element_name = String::from_utf8_lossy(element.name().as_ref()).into_owned();
            HwpError::XmlParseError(format!(
                "Error parsing XML attribute in {source} element <{element_name}>: {err}"
            ))
        })?;
        if attr.value.as_ref().len() > MAX_HWPX_XML_ATTRIBUTE_VALUE_BYTES {
            return Err(HwpError::ResourceLimitExceeded {
                resource: "HWPX XML attribute value bytes",
                path: source.to_string(),
                limit: MAX_HWPX_XML_ATTRIBUTE_VALUE_BYTES as u64,
                actual: attr.value.as_ref().len() as u64,
            });
        }
        f(attr)?;
    }

    Ok(())
}

pub(crate) fn parse_numeric_attr<T>(
    source: &str,
    element: &str,
    attribute: &str,
    attr: &Attribute<'_>,
) -> Result<T, XmlAttributeValueError>
where
    T: FromStr,
{
    let raw = attr.value.as_ref();
    if let Ok(raw_value) = std::str::from_utf8(raw) {
        if let Ok(value) = raw_value.parse::<T>() {
            return Ok(value);
        }

        if raw.contains(&b'&') {
            let decoded_value = parse_string_attr(source, element, attribute, attr)
                .map_err(XmlAttributeValueError::XmlParse)?;
            if let Ok(value) = decoded_value.parse::<T>() {
                return Ok(value);
            }

            return Err(XmlAttributeValueError::InvalidValue(decoded_value));
        }

        return Err(XmlAttributeValueError::InvalidValue(raw_value.to_string()));
    }

    let decoded_value = parse_string_attr(source, element, attribute, attr)
        .map_err(XmlAttributeValueError::XmlParse)?;
    Err(XmlAttributeValueError::InvalidValue(decoded_value))
}

pub(crate) fn parse_string_attr(
    source: &str,
    element: &str,
    attribute: &str,
    attr: &Attribute<'_>,
) -> Result<String, HwpError> {
    attr.normalized_value(XmlVersion::Implicit1_0)
        .map(|value| value.into_owned())
        .map_err(|err| {
            HwpError::XmlParseError(format!(
                "Error decoding XML attribute {attribute} in {source} element <{element}>: {err}"
            ))
        })
}

#[cfg(test)]
mod tests {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    use super::*;

    #[test]
    fn rejects_oversized_xml_attribute_value() {
        let oversized_value = "x".repeat(MAX_HWPX_XML_ATTRIBUTE_VALUE_BYTES + 1);
        let xml = format!(r#"<root value="{oversized_value}"/>"#);
        let mut reader = Reader::from_str(&xml);

        let err = loop {
            match reader.read_event() {
                Ok(Event::Empty(ref e)) => {
                    break for_each_xml_attribute("test.xml", e, |_| Ok(()))
                        .expect_err("oversized XML attribute values should be rejected");
                }
                Ok(Event::Eof) => panic!("test XML did not contain an empty element"),
                Ok(_) => {}
                Err(err) => panic!("test XML should parse: {err}"),
            }
        };

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX XML attribute value bytes"
                && path == "test.xml"
                && limit == MAX_HWPX_XML_ATTRIBUTE_VALUE_BYTES as u64
                && actual == MAX_HWPX_XML_ATTRIBUTE_VALUE_BYTES as u64 + 1
        ));
    }
}
