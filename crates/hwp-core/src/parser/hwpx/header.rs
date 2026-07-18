/// HWPX header.xml parser
///
/// header.xml contains document settings like character shapes, paragraph shapes,
/// styles, fonts, and other document-wide properties.
use std::str::FromStr;

use quick_xml::escape::unescape;
use quick_xml::events::attributes::Attribute;
use quick_xml::events::{BytesCData, BytesRef, BytesStart, BytesText, Event};
use quick_xml::{Reader, XmlVersion};

use crate::diagnostics::{
    DiagnosticCategory, DiagnosticContext, DiagnosticItem, DiagnosticReport, DiagnosticSeverity,
};
use crate::document::docinfo::border_fill::{
    BorderFill, BorderFillAttributes, BorderLine, DiagonalLine, FillInfo, GradientFill, ImageFill,
    SolidFill,
};
use crate::document::docinfo::char_shape::{
    CharShape, CharShapeAttributes, LanguageCharAttributesI8, LanguageCharAttributesU8,
    LanguageFontInfo,
};
use crate::document::docinfo::numbering::{
    DistanceType, ExtendedNumberingLevel, Numbering, NumberingHeaderAttributes, NumberingLevelInfo,
    ParagraphAlignType,
};
use crate::document::docinfo::para_shape::{
    HeaderShapeType, LineDivideUnit, LineSpacingType, LineSpacingTypeOld, ParaShape,
    ParaShapeAttributes1, ParaShapeAttributes2, ParaShapeAttributes3, ParagraphAlignment,
    VerticalAlignment,
};
use crate::document::docinfo::style::StyleType;
use crate::document::docinfo::tab_def::{TabDef, TabDefAttributes, TabItem, TabType};
use crate::document::{DocInfo, FaceName, FileHeader, Style};
use crate::error::{HwpError, ParseWarning, ParseWarnings};
use crate::types::{COLORREF, DWORD, HWPUNIT};

use super::bindata::normalize_hwpx_binary_item_ref;
use super::container::HwpxContainer;
use super::xml_attr::{
    for_each_xml_attribute, parse_numeric_attr, parse_string_attr, XmlAttributeValueError,
};
use super::xml_budget::XmlParseBudget;

pub(crate) const MAX_HWPX_HEADER_CHAR_SHAPES: u64 = 65_536;
pub(crate) const MAX_HWPX_HEADER_PARA_SHAPES: u64 = 65_536;
pub(crate) const MAX_HWPX_HEADER_STYLES: u64 = 65_536;
pub(crate) const MAX_HWPX_HEADER_TAB_DEFS: u64 = 65_536;
pub(crate) const MAX_HWPX_HEADER_NUMBERINGS: u64 = 65_536;
pub(crate) const MAX_HWPX_HEADER_BORDER_FILLS: u64 = 65_536;
pub(crate) const MAX_HWPX_HEADER_XML_SIZE: u64 = 16 * 1024 * 1024;
pub(crate) const MAX_HWPX_VERSION_XML_SIZE: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy)]
struct HeaderStructureLimits {
    max_char_shapes: u64,
    max_para_shapes: u64,
    max_styles: u64,
    max_tab_defs: u64,
    max_numberings: u64,
    max_border_fills: u64,
}

impl Default for HeaderStructureLimits {
    fn default() -> Self {
        Self {
            max_char_shapes: MAX_HWPX_HEADER_CHAR_SHAPES,
            max_para_shapes: MAX_HWPX_HEADER_PARA_SHAPES,
            max_styles: MAX_HWPX_HEADER_STYLES,
            max_tab_defs: MAX_HWPX_HEADER_TAB_DEFS,
            max_numberings: MAX_HWPX_HEADER_NUMBERINGS,
            max_border_fills: MAX_HWPX_HEADER_BORDER_FILLS,
        }
    }
}

struct HeaderStructureBudget<'a> {
    source: &'a str,
    limits: HeaderStructureLimits,
    char_shapes: u64,
    para_shapes: u64,
    styles: u64,
    tab_defs: u64,
    numberings: u64,
    border_fills: u64,
}

impl<'a> HeaderStructureBudget<'a> {
    fn new(source: &'a str, limits: HeaderStructureLimits) -> Self {
        Self {
            source,
            limits,
            char_shapes: 0,
            para_shapes: 0,
            styles: 0,
            tab_defs: 0,
            numberings: 0,
            border_fills: 0,
        }
    }

    fn add_char_shape(&mut self) -> Result<(), HwpError> {
        self.char_shapes = self.char_shapes.saturating_add(1);
        if self.char_shapes > self.limits.max_char_shapes {
            return Err(HwpError::ResourceLimitExceeded {
                resource: "HWPX header char shape count",
                path: self.source.to_string(),
                limit: self.limits.max_char_shapes,
                actual: self.char_shapes,
            });
        }

        Ok(())
    }

    fn add_para_shape(&mut self) -> Result<(), HwpError> {
        self.para_shapes = self.para_shapes.saturating_add(1);
        if self.para_shapes > self.limits.max_para_shapes {
            return Err(HwpError::ResourceLimitExceeded {
                resource: "HWPX header paragraph shape count",
                path: self.source.to_string(),
                limit: self.limits.max_para_shapes,
                actual: self.para_shapes,
            });
        }

        Ok(())
    }

    fn add_style(&mut self) -> Result<(), HwpError> {
        self.styles = self.styles.saturating_add(1);
        if self.styles > self.limits.max_styles {
            return Err(HwpError::ResourceLimitExceeded {
                resource: "HWPX header style count",
                path: self.source.to_string(),
                limit: self.limits.max_styles,
                actual: self.styles,
            });
        }

        Ok(())
    }

    fn add_tab_def(&mut self) -> Result<(), HwpError> {
        self.tab_defs = self.tab_defs.saturating_add(1);
        if self.tab_defs > self.limits.max_tab_defs {
            return Err(HwpError::ResourceLimitExceeded {
                resource: "HWPX header tab definition count",
                path: self.source.to_string(),
                limit: self.limits.max_tab_defs,
                actual: self.tab_defs,
            });
        }

        Ok(())
    }

    fn add_numbering(&mut self) -> Result<(), HwpError> {
        self.numberings = self.numberings.saturating_add(1);
        if self.numberings > self.limits.max_numberings {
            return Err(HwpError::ResourceLimitExceeded {
                resource: "HWPX header numbering count",
                path: self.source.to_string(),
                limit: self.limits.max_numberings,
                actual: self.numberings,
            });
        }

        Ok(())
    }

    fn add_border_fill(&mut self) -> Result<(), HwpError> {
        self.border_fills = self.border_fills.saturating_add(1);
        if self.border_fills > self.limits.max_border_fills {
            return Err(HwpError::ResourceLimitExceeded {
                resource: "HWPX header border fill count",
                path: self.source.to_string(),
                limit: self.limits.max_border_fills,
                actual: self.border_fills,
            });
        }

        Ok(())
    }
}

/// Parse header.xml and create FileHeader
pub fn parse_file_header(container: &mut HwpxContainer) -> Result<FileHeader, HwpError> {
    // Try to read version.xml first for version info
    let version = match parse_version_xml(container) {
        Ok(version) => version,
        Err(HwpError::HwpxFileNotFound { path }) if path == "version.xml" => 0x05010000,
        Err(err) => return Err(err),
    };

    // Create a FileHeader compatible with HWP 5.0 structure
    Ok(FileHeader {
        signature: "HWP Document File".to_string(),
        version,
        document_flags: 0, // HWPX is not compressed in the same way as HWP
        license_flags: 0,
        encrypt_version: 0,
        kogl_country: 0,
        reserved: vec![0; 207],
    })
}

/// Parse version.xml for OWPML version info
fn parse_version_xml(container: &mut HwpxContainer) -> Result<DWORD, HwpError> {
    let content = container.read_file_string_with_limit(
        "version.xml",
        MAX_HWPX_VERSION_XML_SIZE,
        "HWPX version XML byte size",
    )?;

    let mut reader = Reader::from_str(&content);
    reader.config_mut().trim_text(true);
    let mut xml_budget = XmlParseBudget::new("version.xml");

    let mut version: DWORD = 0x05010000; // Default version
    let mut xml_depth = 0usize;
    let mut version_root_seen = false;

    loop {
        let event = reader.read_event();
        if let Ok(ref event) = event {
            xml_budget.observe_event(event)?;
        }

        match &event {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let is_start = matches!(&event, Ok(Event::Start(_)));
                let local_name = e.name();
                let local_name = local_name.as_ref();
                validate_version_root_element(local_name, xml_depth, &mut version_root_seen)?;
                if is_start {
                    xml_depth = xml_depth.saturating_add(1);
                }

                if is_version_root_element(local_name) {
                    for_each_xml_attribute("version.xml", e, |attr| {
                        if attr.key.as_ref() == b"major"
                            || attr.key.as_ref() == b"oversion"
                            || attr.key.as_ref() == b"app-version"
                        {
                            let attribute = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
                            let v = match parse_numeric_attr::<u32>(
                                "version.xml",
                                "opf:version",
                                &attribute,
                                &attr,
                            ) {
                                Ok(v) => v,
                                Err(XmlAttributeValueError::InvalidValue(value)) => {
                                    return Err(HwpError::InvalidHwpxStructure {
                                        reason: format!(
                                            "version.xml opf:version {attribute} value {value} is not a valid unsigned integer"
                                        ),
                                    });
                                }
                                Err(XmlAttributeValueError::XmlParse(err)) => return Err(err),
                            };
                            if v > u8::MAX as u32 {
                                return Err(HwpError::InvalidHwpxStructure {
                                    reason: format!(
                                        "version.xml opf:version {attribute} value {v} exceeds BYTE range"
                                    ),
                                });
                            }

                            // Convert to HWP version format (major.minor.build.revision)
                            version = (v << 24) | 0x00010000;
                        }
                        Ok(())
                    })?;
                }
            }
            Ok(Event::End(e)) => {
                xml_budget.finish_end_event(e)?;
                xml_depth = xml_depth.saturating_sub(1);
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(HwpError::XmlParseError(format!(
                    "Error parsing version.xml: {e}"
                )))
            }
            _ => {}
        }
    }

    if !version_root_seen {
        return Err(HwpError::InvalidHwpxStructure {
            reason: "version.xml root element must be hv:HCFVersion or opf:version".to_string(),
        });
    }

    Ok(version)
}

fn validate_version_root_element(
    name: &[u8],
    current_depth: usize,
    version_root_seen: &mut bool,
) -> Result<(), HwpError> {
    if current_depth != 0 {
        return Ok(());
    }

    if *version_root_seen {
        return Err(HwpError::InvalidHwpxStructure {
            reason: "version.xml contains multiple root elements".to_string(),
        });
    }

    if !is_version_root_element(name) {
        return Err(HwpError::InvalidHwpxStructure {
            reason: "version.xml root element must be hv:HCFVersion or opf:version".to_string(),
        });
    }

    *version_root_seen = true;
    Ok(())
}

fn is_version_root_element(name: &[u8]) -> bool {
    header_has_local_name(name, b"HCFVersion") || header_has_local_name(name, b"version")
}

/// Parse header.xml and create DocInfo
pub fn parse_doc_info(
    container: &mut HwpxContainer,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<DocInfo, HwpError> {
    let content = container.read_file_string_with_limit(
        "Contents/header.xml",
        MAX_HWPX_HEADER_XML_SIZE,
        "HWPX header XML byte size",
    )?;

    let mut reader = Reader::from_str(&content);
    reader.config_mut().trim_text(true);

    // Create a basic DocInfo structure
    // In a full implementation, we would parse character shapes, paragraph shapes, etc.
    let mut doc_info = DocInfo::default();

    // Parse the XML and extract relevant information
    // For now, we create a minimal DocInfo that allows the document to be processed
    parse_header_xml_content(&mut reader, &mut doc_info, warnings, diagnostics)?;

    Ok(doc_info)
}

/// Parse hex color string (#RRGGBB or #AARRGGBB) to COLORREF (BGR format)
fn parse_color(color_str: &str) -> Result<COLORREF, String> {
    let color_str = color_str.trim();
    let mut hex = color_str.strip_prefix('#').unwrap_or(color_str);
    if hex.eq_ignore_ascii_case("none") || hex.is_empty() {
        return Ok(COLORREF(0));
    }
    if hex.len() == 8 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        hex = &hex[2..];
    }
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(color_str.to_string());
    }

    // Parse as RGB and convert to BGR (COLORREF format)
    let rgb = u32::from_str_radix(hex, 16).map_err(|_| color_str.to_string())?;
    let r = ((rgb >> 16) & 0xFF) as u8;
    let g = ((rgb >> 8) & 0xFF) as u8;
    let b = (rgb & 0xFF) as u8;
    Ok(COLORREF::rgb(r, g, b))
}

fn parse_header_numeric_attr_or_default<T>(
    attr: &Attribute<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
    element: &str,
    attribute: &str,
    message_prefix: &str,
    default: T,
) -> Result<T, HwpError>
where
    T: Copy + FromStr,
{
    match parse_numeric_attr("Contents/header.xml", element, attribute, attr) {
        Ok(value) => Ok(value),
        Err(XmlAttributeValueError::InvalidValue(value)) => {
            let message = format!("{message_prefix}: {value}");
            warnings.push(ParseWarning::warning(message.clone()));
            record_header_invalid_value(diagnostics, element, attribute, &value, message);
            Ok(default)
        }
        Err(XmlAttributeValueError::XmlParse(err)) => Err(err),
    }
}

fn parse_header_optional_numeric_attr<T>(
    attr: &Attribute<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
    element: &str,
    attribute: &str,
    message_prefix: &str,
) -> Result<Option<T>, HwpError>
where
    T: FromStr,
{
    match parse_numeric_attr("Contents/header.xml", element, attribute, attr) {
        Ok(value) => Ok(Some(value)),
        Err(XmlAttributeValueError::InvalidValue(value)) => {
            let message = format!("{message_prefix}: {value}");
            warnings.push(ParseWarning::warning(message.clone()));
            record_header_invalid_value(diagnostics, element, attribute, &value, message);
            Ok(None)
        }
        Err(XmlAttributeValueError::XmlParse(err)) => Err(err),
    }
}

fn parse_char_height_attr_or_default(
    attr: &Attribute<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<i32, HwpError> {
    let value = parse_header_numeric_attr_or_default::<i32>(
        attr,
        warnings,
        diagnostics,
        "hh:charPr",
        "height",
        "Invalid charPr height value",
        1000,
    )?;

    if value <= 0 {
        let value_text = value.to_string();
        let message = format!("Invalid charPr height value: {value_text}");
        warnings.push(ParseWarning::warning(message.clone()));
        record_header_invalid_value(diagnostics, "hh:charPr", "height", &value_text, message);
        return Ok(1000);
    }

    Ok(value)
}

fn parse_header_color_attr_or_default(
    attr: &Attribute<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
    element: &str,
    attribute: &str,
    default: COLORREF,
) -> Result<COLORREF, HwpError> {
    let value = parse_string_attr("Contents/header.xml", element, attribute, attr)?;
    match parse_color(&value) {
        Ok(color) => Ok(color),
        Err(value) => {
            let message = format!("Invalid {element} {attribute} color value: {value}");
            warnings.push(ParseWarning::warning(message.clone()));
            record_header_invalid_value(diagnostics, element, attribute, &value, message);
            Ok(default)
        }
    }
}

fn parse_hwpx_border_line_type_attr_or_default(
    attr: &Attribute<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
    element: &str,
) -> Result<u8, HwpError> {
    parse_header_enum_attr_or_default(attr, warnings, diagnostics, element, "type", 0, |value| {
        if value.eq_ignore_ascii_case("NONE") {
            Some(0)
        } else if value.eq_ignore_ascii_case("SOLID") {
            Some(1)
        } else if value.eq_ignore_ascii_case("DASH") {
            Some(2)
        } else if value.eq_ignore_ascii_case("DOT") {
            Some(3)
        } else if value.eq_ignore_ascii_case("DASH_DOT") {
            Some(4)
        } else if value.eq_ignore_ascii_case("DASH_DOT_DOT") {
            Some(5)
        } else if value.eq_ignore_ascii_case("LONG_DASH") {
            Some(6)
        } else {
            None
        }
    })
}

fn parse_hwpx_border_width_attr_or_default(
    attr: &Attribute<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
    element: &str,
) -> Result<u8, HwpError> {
    let value = parse_string_attr("Contents/header.xml", element, "width", attr)?;
    let normalized = value.trim().trim_end_matches("mm").trim();
    let parsed = normalized.parse::<f64>().ok().and_then(|width| {
        const WIDTHS: &[(f64, u8)] = &[
            (0.10, 0),
            (0.12, 1),
            (0.15, 2),
            (0.20, 3),
            (0.25, 4),
            (0.30, 5),
            (0.40, 6),
            (0.50, 7),
            (0.60, 8),
            (0.70, 9),
            (1.00, 10),
            (1.50, 11),
            (2.00, 12),
            (3.00, 13),
            (4.00, 14),
            (5.00, 15),
        ];
        WIDTHS
            .iter()
            .find(|(known, _)| (width - *known).abs() < 0.005)
            .map(|(_, code)| *code)
    });

    match parsed {
        Some(width) => Ok(width),
        None => {
            let message = format!("Invalid {element} width value: {value}");
            warnings.push(ParseWarning::warning(message.clone()));
            record_header_invalid_value(diagnostics, element, "width", &value, message);
            Ok(0)
        }
    }
}

fn parse_hwpx_border_line_element(
    element: &BytesStart<'_>,
    element_name: &str,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<BorderLine, HwpError> {
    let mut line = default_hwpx_border_line();
    for_each_xml_attribute("Contents/header.xml", element, |attr| {
        match attr.key.as_ref() {
            b"type" => {
                line.line_type = parse_hwpx_border_line_type_attr_or_default(
                    &attr,
                    warnings,
                    diagnostics,
                    element_name,
                )?;
            }
            b"width" => {
                line.width = parse_hwpx_border_width_attr_or_default(
                    &attr,
                    warnings,
                    diagnostics,
                    element_name,
                )?;
            }
            b"color" => {
                line.color = parse_header_color_attr_or_default(
                    &attr,
                    warnings,
                    diagnostics,
                    element_name,
                    "color",
                    COLORREF(0),
                )?;
            }
            _ => {}
        }
        Ok(())
    })?;
    Ok(line)
}

fn parse_hwpx_diagonal_line_element(
    element: &BytesStart<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<DiagonalLine, HwpError> {
    let line = parse_hwpx_border_line_element(element, "hh:diagonal", warnings, diagnostics)?;
    Ok(DiagonalLine {
        line_type: line.line_type,
        thickness: line.width,
        color: line.color,
    })
}

fn parse_hwpx_slash_element(
    element: &BytesStart<'_>,
    attributes: &mut BorderFillAttributes,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<(), HwpError> {
    for_each_xml_attribute("Contents/header.xml", element, |attr| {
        match attr.key.as_ref() {
            b"type" => {
                attributes.slash_shape = parse_hwpx_border_line_type_attr_or_default(
                    &attr,
                    warnings,
                    diagnostics,
                    "hh:slash",
                )?;
            }
            b"Crooked" => {
                attributes.slash_broken_line = u8::from(parse_header_bool_attr_or_default(
                    &attr,
                    warnings,
                    diagnostics,
                    "hh:slash",
                    "Crooked",
                    false,
                )?);
            }
            b"isCounter" => {
                attributes.slash_rotated_180 = parse_header_bool_attr_or_default(
                    &attr,
                    warnings,
                    diagnostics,
                    "hh:slash",
                    "isCounter",
                    false,
                )?;
            }
            _ => {}
        }
        Ok(())
    })
}

fn parse_hwpx_backslash_element(
    element: &BytesStart<'_>,
    attributes: &mut BorderFillAttributes,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<(), HwpError> {
    for_each_xml_attribute("Contents/header.xml", element, |attr| {
        match attr.key.as_ref() {
            b"type" => {
                attributes.backslash_shape = parse_hwpx_border_line_type_attr_or_default(
                    &attr,
                    warnings,
                    diagnostics,
                    "hh:backSlash",
                )?;
            }
            b"Crooked" => {
                attributes.backslash_broken_line = parse_header_bool_attr_or_default(
                    &attr,
                    warnings,
                    diagnostics,
                    "hh:backSlash",
                    "Crooked",
                    false,
                )?;
            }
            b"isCounter" => {
                attributes.backslash_rotated_180 = parse_header_bool_attr_or_default(
                    &attr,
                    warnings,
                    diagnostics,
                    "hh:backSlash",
                    "isCounter",
                    false,
                )?;
            }
            _ => {}
        }
        Ok(())
    })
}

fn parse_hwpx_gradient_type_attr_or_default(
    attr: &Attribute<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<i16, HwpError> {
    parse_header_enum_attr_or_default(
        attr,
        warnings,
        diagnostics,
        "hc:gradation",
        "type",
        0,
        |value| {
            if value.eq_ignore_ascii_case("NONE") {
                Some(0)
            } else if value.eq_ignore_ascii_case("LINEAR") {
                Some(1)
            } else if value.eq_ignore_ascii_case("RADIAL") {
                Some(2)
            } else if value.eq_ignore_ascii_case("CONICAL") {
                Some(3)
            } else if value.eq_ignore_ascii_case("SQUARE") {
                Some(4)
            } else {
                None
            }
        },
    )
}

fn parse_hwpx_gradation_element(
    element: &BytesStart<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<GradientFill, HwpError> {
    let mut gradient = GradientFill {
        gradient_type: 0,
        angle: 0,
        horizontal_center: 0,
        vertical_center: 0,
        spread: 0,
        step_center: None,
        alpha: None,
        color_count: 0,
        positions: None,
        colors: Vec::new(),
    };

    for_each_xml_attribute("Contents/header.xml", element, |attr| {
        match attr.key.as_ref() {
            b"type" => {
                gradient.gradient_type =
                    parse_hwpx_gradient_type_attr_or_default(&attr, warnings, diagnostics)?;
            }
            b"angle" => {
                gradient.angle = parse_header_numeric_attr_or_default::<i16>(
                    &attr,
                    warnings,
                    diagnostics,
                    "hc:gradation",
                    "angle",
                    "Invalid hc:gradation angle value",
                    0,
                )?;
            }
            b"centerX" => {
                gradient.horizontal_center = parse_header_numeric_attr_or_default::<i16>(
                    &attr,
                    warnings,
                    diagnostics,
                    "hc:gradation",
                    "centerX",
                    "Invalid hc:gradation centerX value",
                    0,
                )?;
            }
            b"centerY" => {
                gradient.vertical_center = parse_header_numeric_attr_or_default::<i16>(
                    &attr,
                    warnings,
                    diagnostics,
                    "hc:gradation",
                    "centerY",
                    "Invalid hc:gradation centerY value",
                    0,
                )?;
            }
            b"step" => {
                gradient.spread = parse_header_numeric_attr_or_default::<i16>(
                    &attr,
                    warnings,
                    diagnostics,
                    "hc:gradation",
                    "step",
                    "Invalid hc:gradation step value",
                    0,
                )?;
            }
            b"stepCenter" => {
                gradient.step_center = parse_header_optional_numeric_attr::<u8>(
                    &attr,
                    warnings,
                    diagnostics,
                    "hc:gradation",
                    "stepCenter",
                    "Invalid hc:gradation stepCenter value",
                )?;
            }
            b"alpha" => {
                gradient.alpha = parse_header_optional_numeric_attr::<u8>(
                    &attr,
                    warnings,
                    diagnostics,
                    "hc:gradation",
                    "alpha",
                    "Invalid hc:gradation alpha value",
                )?;
            }
            b"colorNum" => {
                let value = parse_header_numeric_attr_or_default::<i16>(
                    &attr,
                    warnings,
                    diagnostics,
                    "hc:gradation",
                    "colorNum",
                    "Invalid hc:gradation colorNum value",
                    0,
                )?;
                if value < 0 {
                    let value_text = value.to_string();
                    let message = format!("Invalid hc:gradation colorNum value: {value_text}");
                    warnings.push(ParseWarning::warning(message.clone()));
                    record_header_invalid_value(
                        diagnostics,
                        "hc:gradation",
                        "colorNum",
                        &value_text,
                        message,
                    );
                    gradient.color_count = 0;
                } else {
                    gradient.color_count = value;
                }
            }
            _ => {}
        }
        Ok(())
    })?;

    Ok(gradient)
}

fn append_hwpx_gradation_color(
    border_fill: &mut BorderFill,
    element: &BytesStart<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<(), HwpError> {
    let mut color = None;
    for_each_xml_attribute("Contents/header.xml", element, |attr| {
        if attr.key.as_ref() == b"value" {
            color = Some(parse_header_color_attr_or_default(
                &attr,
                warnings,
                diagnostics,
                "hc:color",
                "value",
                COLORREF(0),
            )?);
        }
        Ok(())
    })?;

    let Some(color) = color else {
        return Ok(());
    };

    if let FillInfo::Gradient(gradient) = &mut border_fill.fill {
        if gradient.colors.len() >= i16::MAX as usize {
            return Err(HwpError::ResourceLimitExceeded {
                resource: "HWPX header gradient color count",
                path: "Contents/header.xml".to_string(),
                limit: i16::MAX as u64,
                actual: gradient.colors.len() as u64 + 1,
            });
        }
        gradient.colors.push(color);
        gradient.color_count = gradient
            .color_count
            .max(i16::try_from(gradient.colors.len()).unwrap_or(i16::MAX));
    }

    Ok(())
}

fn default_hwpx_image_fill() -> ImageFill {
    ImageFill {
        image_fill_type: 0,
        image_info: Vec::new(),
        mode: None,
        binary_item_ref: None,
        brightness: None,
        contrast: None,
        effect: None,
        alpha: None,
        gradient_spread_center: None,
        additional_attributes_length: None,
        additional_attributes: None,
    }
}

fn parse_hwpx_img_brush_element(element: &BytesStart<'_>) -> Result<ImageFill, HwpError> {
    let mut image_fill = default_hwpx_image_fill();

    for_each_xml_attribute("Contents/header.xml", element, |attr| {
        if attr.key.as_ref() == b"mode" {
            image_fill.mode = Some(parse_string_attr(
                "Contents/header.xml",
                "hc:imgBrush",
                "mode",
                &attr,
            )?);
        }
        Ok(())
    })?;

    Ok(image_fill)
}

fn append_hwpx_image_fill_element(
    border_fill: &mut BorderFill,
    element: &BytesStart<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<(), HwpError> {
    let FillInfo::Image(image_fill) = &mut border_fill.fill else {
        return Ok(());
    };

    for_each_xml_attribute("Contents/header.xml", element, |attr| {
        match attr.key.as_ref() {
            b"binaryItemIDRef" => {
                let value =
                    parse_string_attr("Contents/header.xml", "hc:img", "binaryItemIDRef", &attr)?;
                match normalize_hwpx_binary_item_ref(&value) {
                    Some(normalized) => image_fill.binary_item_ref = Some(normalized),
                    None => record_skipped_header_image_ref(warnings, diagnostics, &value),
                }
            }
            b"bright" => {
                image_fill.brightness = parse_header_optional_numeric_attr::<i16>(
                    &attr,
                    warnings,
                    diagnostics,
                    "hc:img",
                    "bright",
                    "Invalid hc:img bright value",
                )?;
            }
            b"contrast" => {
                image_fill.contrast = parse_header_optional_numeric_attr::<i16>(
                    &attr,
                    warnings,
                    diagnostics,
                    "hc:img",
                    "contrast",
                    "Invalid hc:img contrast value",
                )?;
            }
            b"effect" => {
                image_fill.effect = Some(parse_string_attr(
                    "Contents/header.xml",
                    "hc:img",
                    "effect",
                    &attr,
                )?);
            }
            b"alpha" => {
                image_fill.alpha = parse_header_optional_numeric_attr::<u8>(
                    &attr,
                    warnings,
                    diagnostics,
                    "hc:img",
                    "alpha",
                    "Invalid hc:img alpha value",
                )?;
            }
            _ => {}
        }
        Ok(())
    })
}

fn finalize_hwpx_border_fill(
    border_fill: &mut BorderFill,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) {
    if let FillInfo::Gradient(gradient) = &mut border_fill.fill {
        let actual_color_count = i16::try_from(gradient.colors.len()).unwrap_or(i16::MAX);
        if gradient.color_count != actual_color_count {
            let value_text = gradient.color_count.to_string();
            let message = format!(
                "Invalid hc:gradation colorNum value: declared {}, actual {}",
                gradient.color_count, actual_color_count
            );
            warnings.push(ParseWarning::warning(message.clone()));
            record_header_invalid_value(
                diagnostics,
                "hc:gradation",
                "colorNum",
                &value_text,
                message,
            );
            gradient.color_count = actual_color_count;
        }
    }
}

fn parse_header_enum_attr_or_default<T>(
    attr: &Attribute<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
    element: &str,
    attribute: &str,
    default: T,
    parse: impl FnOnce(&str) -> Option<T>,
) -> Result<T, HwpError>
where
    T: Copy,
{
    let value = parse_string_attr("Contents/header.xml", element, attribute, attr)?;
    match parse(&value) {
        Some(parsed) => Ok(parsed),
        None => {
            let message = format!("Invalid {element} {attribute} value: {value}");
            warnings.push(ParseWarning::warning(message.clone()));
            record_header_invalid_value(diagnostics, element, attribute, &value, message);
            Ok(default)
        }
    }
}

fn parse_header_bool_attr_or_default(
    attr: &Attribute<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
    element: &str,
    attribute: &str,
    default: bool,
) -> Result<bool, HwpError> {
    parse_header_enum_attr_or_default(
        attr,
        warnings,
        diagnostics,
        element,
        attribute,
        default,
        |value| {
            let value = value.trim();
            if value.eq_ignore_ascii_case("true") || value == "1" {
                Some(true)
            } else if value.eq_ignore_ascii_case("false") || value == "0" {
                Some(false)
            } else {
                None
            }
        },
    )
}

fn parse_hwpx_tab_type_attr_or_default(
    attr: &Attribute<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<TabType, HwpError> {
    parse_header_enum_attr_or_default(
        attr,
        warnings,
        diagnostics,
        "hh:tabItem",
        "type",
        TabType::Left,
        |value| {
            if value.eq_ignore_ascii_case("LEFT") {
                Some(TabType::Left)
            } else if value.eq_ignore_ascii_case("RIGHT") {
                Some(TabType::Right)
            } else if value.eq_ignore_ascii_case("CENTER") {
                Some(TabType::Center)
            } else if value.eq_ignore_ascii_case("DECIMAL") {
                Some(TabType::Decimal)
            } else {
                None
            }
        },
    )
}

fn parse_hwpx_tab_leader_attr_or_default(
    attr: &Attribute<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<u8, HwpError> {
    let value = parse_string_attr("Contents/header.xml", "hh:tabItem", "leader", attr)?;
    if let Ok(leader) = value.parse::<u8>() {
        return Ok(leader);
    }

    let parsed = if value.eq_ignore_ascii_case("NONE") {
        Some(0)
    } else if value.eq_ignore_ascii_case("SOLID") {
        Some(1)
    } else if value.eq_ignore_ascii_case("DASH") {
        Some(2)
    } else if value.eq_ignore_ascii_case("DOT") {
        Some(3)
    } else if value.eq_ignore_ascii_case("DASH_DOT") {
        Some(4)
    } else if value.eq_ignore_ascii_case("DASH_DOT_DOT") {
        Some(5)
    } else {
        None
    };

    match parsed {
        Some(leader) => Ok(leader),
        None => {
            let message = format!("Invalid hh:tabItem leader value: {value}");
            warnings.push(ParseWarning::warning(message.clone()));
            record_header_invalid_value(diagnostics, "hh:tabItem", "leader", &value, message);
            Ok(0)
        }
    }
}

fn parse_hwpx_numbering_align_attr_or_default(
    attr: &Attribute<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<ParagraphAlignType, HwpError> {
    parse_header_enum_attr_or_default(
        attr,
        warnings,
        diagnostics,
        "hh:paraHead",
        "align",
        ParagraphAlignType::Left,
        |value| {
            if value.eq_ignore_ascii_case("LEFT") {
                Some(ParagraphAlignType::Left)
            } else if value.eq_ignore_ascii_case("CENTER") {
                Some(ParagraphAlignType::Center)
            } else if value.eq_ignore_ascii_case("RIGHT") {
                Some(ParagraphAlignType::Right)
            } else {
                None
            }
        },
    )
}

fn parse_hwpx_numbering_distance_type_attr_or_default(
    attr: &Attribute<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<DistanceType, HwpError> {
    parse_header_enum_attr_or_default(
        attr,
        warnings,
        diagnostics,
        "hh:paraHead",
        "textOffsetType",
        DistanceType::Ratio,
        |value| {
            if value.eq_ignore_ascii_case("PERCENT") || value.eq_ignore_ascii_case("RATIO") {
                Some(DistanceType::Ratio)
            } else if value.eq_ignore_ascii_case("HWPUNIT") || value.eq_ignore_ascii_case("VALUE") {
                Some(DistanceType::Value)
            } else {
                None
            }
        },
    )
}

fn parse_hwpx_heading_type_attr_or_default(
    attr: &Attribute<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<HeaderShapeType, HwpError> {
    parse_header_enum_attr_or_default(
        attr,
        warnings,
        diagnostics,
        "hh:heading",
        "type",
        HeaderShapeType::None,
        |value| {
            if value.eq_ignore_ascii_case("NONE") {
                Some(HeaderShapeType::None)
            } else if value.eq_ignore_ascii_case("OUTLINE") {
                Some(HeaderShapeType::Outline)
            } else if value.eq_ignore_ascii_case("NUMBER") {
                Some(HeaderShapeType::Number)
            } else if value.eq_ignore_ascii_case("BULLET") {
                Some(HeaderShapeType::Bullet)
            } else {
                None
            }
        },
    )
}

fn parse_hwpx_line_spacing_type_attr_or_default(
    attr: &Attribute<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<LineSpacingType, HwpError> {
    parse_header_enum_attr_or_default(
        attr,
        warnings,
        diagnostics,
        "hh:lineSpacing",
        "type",
        LineSpacingType::ByCharacter,
        |value| {
            if value.eq_ignore_ascii_case("PERCENT")
                || value.eq_ignore_ascii_case("BY_CHARACTER")
                || value.eq_ignore_ascii_case("BYCHARACTER")
            {
                Some(LineSpacingType::ByCharacter)
            } else if value.eq_ignore_ascii_case("FIXED") {
                Some(LineSpacingType::Fixed)
            } else if value.eq_ignore_ascii_case("BETWEEN_LINES")
                || value.eq_ignore_ascii_case("MARGIN_ONLY")
                || value.eq_ignore_ascii_case("MARGINONLY")
            {
                Some(LineSpacingType::MarginOnly)
            } else if value.eq_ignore_ascii_case("MINIMUM") {
                Some(LineSpacingType::Minimum)
            } else {
                None
            }
        },
    )
}

fn parse_hwpx_line_divide_attr_or_default(
    attr: &Attribute<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
    attribute: &str,
) -> Result<LineDivideUnit, HwpError> {
    parse_header_enum_attr_or_default(
        attr,
        warnings,
        diagnostics,
        "hh:breakSetting",
        attribute,
        LineDivideUnit::Word,
        |value| {
            if value.eq_ignore_ascii_case("KEEP_WORD") || value.eq_ignore_ascii_case("WORD") {
                Some(LineDivideUnit::Word)
            } else if value.eq_ignore_ascii_case("BREAK_WORD")
                || value.eq_ignore_ascii_case("CHAR")
                || value.eq_ignore_ascii_case("CHARACTER")
            {
                Some(LineDivideUnit::Character)
            } else if value.eq_ignore_ascii_case("HYPHEN")
                || value.eq_ignore_ascii_case("HYPHENATION")
            {
                Some(LineDivideUnit::Hyphen)
            } else {
                None
            }
        },
    )
}

fn parse_hwpx_condense_attr_or_default(
    attr: &Attribute<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<u8, HwpError> {
    let value = parse_header_numeric_attr_or_default::<u8>(
        attr,
        warnings,
        diagnostics,
        "hh:paraPr",
        "condense",
        "Invalid paraPr condense value",
        0,
    )?;

    if value > 75 {
        let value_text = value.to_string();
        let message = format!("Invalid paraPr condense value: {value_text}");
        warnings.push(ParseWarning::warning(message.clone()));
        record_header_invalid_value(diagnostics, "hh:paraPr", "condense", &value_text, message);
        return Ok(0);
    }

    Ok(value)
}

fn unescape_header_text(text: &BytesText<'_>) -> Result<String, HwpError> {
    let decoded = text.xml_content(XmlVersion::Implicit1_0).map_err(|err| {
        HwpError::XmlParseError(format!("Error decoding text in Contents/header.xml: {err}"))
    })?;

    unescape(&decoded)
        .map(|value| value.into_owned())
        .map_err(|err| {
            HwpError::XmlParseError(format!("Error decoding text in Contents/header.xml: {err}"))
        })
}

fn unescape_header_reference(reference: &BytesRef<'_>) -> Result<String, HwpError> {
    let decoded = reference.decode().map_err(|err| {
        HwpError::XmlParseError(format!("Error decoding text in Contents/header.xml: {err}"))
    })?;
    let escaped = format!("&{decoded};");

    unescape(&escaped)
        .map(|value| value.into_owned())
        .map_err(|err| {
            HwpError::XmlParseError(format!("Error decoding text in Contents/header.xml: {err}"))
        })
}

fn decode_header_cdata(cdata: &BytesCData<'_>) -> Result<String, HwpError> {
    cdata
        .xml_content(XmlVersion::Implicit1_0)
        .map(|value| value.into_owned())
        .map_err(|err| {
            HwpError::XmlParseError(format!(
                "Error decoding CDATA in Contents/header.xml: {err}"
            ))
        })
}

fn hwpx_format_length(format_string: &str) -> Result<u16, HwpError> {
    let length = format_string.chars().count();
    u16::try_from(length).map_err(|_| HwpError::ResourceLimitExceeded {
        resource: "HWPX header numbering format length",
        path: "Contents/header.xml".to_string(),
        limit: u16::MAX as u64,
        actual: length as u64,
    })
}

fn default_hwpx_char_shape() -> CharShape {
    hwpx_char_shape_from_attrs(1000, COLORREF(0))
}

fn default_hwpx_face_name() -> FaceName {
    FaceName {
        name: "함초롬바탕".to_string(),
        alternative_font_type: None,
        alternative_font_name: None,
        font_type_info: None,
        default_font_name: None,
    }
}

fn hwpx_face_name_from_name(name: String) -> FaceName {
    FaceName {
        name,
        alternative_font_type: None,
        alternative_font_name: None,
        font_type_info: None,
        default_font_name: None,
    }
}

fn store_hwpx_face_name(doc_info: &mut DocInfo, id: u16, name: String) {
    let index = usize::from(id);
    if index >= doc_info.face_names.len() {
        doc_info
            .face_names
            .resize_with(index + 1, default_hwpx_face_name);
    }
    doc_info.face_names[index] = hwpx_face_name_from_name(name);
}

fn default_hwpx_border_line() -> BorderLine {
    BorderLine {
        line_type: 0,
        width: 0,
        color: COLORREF(0),
    }
}

fn default_hwpx_border_fill() -> BorderFill {
    BorderFill {
        attributes: BorderFillAttributes {
            has_3d_effect: false,
            has_shadow: false,
            slash_shape: 0,
            backslash_shape: 0,
            slash_broken_line: 0,
            backslash_broken_line: false,
            slash_rotated_180: false,
            backslash_rotated_180: false,
            has_center_line: false,
        },
        borders: [
            default_hwpx_border_line(),
            default_hwpx_border_line(),
            default_hwpx_border_line(),
            default_hwpx_border_line(),
        ],
        diagonal: DiagonalLine {
            line_type: 0,
            thickness: 0,
            color: COLORREF(0),
        },
        fill: FillInfo::None,
    }
}

fn store_completed_border_fill(
    doc_info: &mut DocInfo,
    structure_budget: &mut HeaderStructureBudget<'_>,
    defined_border_fill_ids: &mut std::collections::BTreeSet<usize>,
    border_fill: BorderFill,
    explicit_id: Option<u16>,
) -> Result<(), HwpError> {
    let index = match explicit_id {
        Some(0) => {
            return Err(HwpError::InvalidHwpxStructure {
                reason: "HWPX borderFill id must be greater than zero".to_string(),
            });
        }
        Some(id) => usize::from(id - 1),
        None => doc_info.border_fill.len(),
    };

    if !defined_border_fill_ids.insert(index) {
        return Err(HwpError::InvalidHwpxStructure {
            reason: format!("Duplicate HWPX borderFill id: {}", index + 1),
        });
    }

    let required_slots = index as u64 + 1;
    if required_slots > structure_budget.limits.max_border_fills {
        return Err(HwpError::ResourceLimitExceeded {
            resource: "HWPX header border fill count",
            path: structure_budget.source.to_string(),
            limit: structure_budget.limits.max_border_fills,
            actual: required_slots,
        });
    }

    structure_budget.add_border_fill()?;
    if index >= doc_info.border_fill.len() {
        doc_info
            .border_fill
            .resize_with(index + 1, default_hwpx_border_fill);
    }
    doc_info.border_fill[index] = border_fill;
    Ok(())
}

fn hwpx_char_shape_from_attrs(height: i32, text_color: COLORREF) -> CharShape {
    CharShape {
        font_ids: LanguageFontInfo {
            korean: 0,
            english: 0,
            chinese: 0,
            japanese: 0,
            other: 0,
            symbol: 0,
            user: 0,
        },
        font_stretch: LanguageCharAttributesU8 {
            korean: 100,
            english: 100,
            chinese: 100,
            japanese: 100,
            other: 100,
            symbol: 100,
            user: 100,
        },
        letter_spacing: LanguageCharAttributesI8 {
            korean: 0,
            english: 0,
            chinese: 0,
            japanese: 0,
            other: 0,
            symbol: 0,
            user: 0,
        },
        relative_size: LanguageCharAttributesU8 {
            korean: 100,
            english: 100,
            chinese: 100,
            japanese: 100,
            other: 100,
            symbol: 100,
            user: 100,
        },
        text_position: LanguageCharAttributesI8 {
            korean: 0,
            english: 0,
            chinese: 0,
            japanese: 0,
            other: 0,
            symbol: 0,
            user: 0,
        },
        base_size: height,
        attributes: CharShapeAttributes {
            italic: false,
            bold: false,
            underline_type: 0,
            underline_style: 0,
            outline_type: 0,
            shadow_type: 0,
            emboss: false,
            engrave: false,
            superscript: false,
            subscript: false,
            strikethrough: 0,
            emphasis_mark: 0,
            use_font_spacing: false,
            strikethrough_style: 0,
            kerning: false,
        },
        shadow_spacing_x: 0,
        shadow_spacing_y: 0,
        text_color,
        underline_color: COLORREF(0),
        shading_color: COLORREF(0),
        shadow_color: COLORREF(0),
        border_fill_id: None,
        strikethrough_color: None,
    }
}

fn store_completed_char_shape(
    doc_info: &mut DocInfo,
    structure_budget: &mut HeaderStructureBudget<'_>,
    defined_char_shape_ids: &mut std::collections::BTreeSet<usize>,
    char_shape: CharShape,
    explicit_id: Option<u16>,
) -> Result<(), HwpError> {
    let index = explicit_id
        .map(usize::from)
        .unwrap_or(doc_info.char_shapes.len());

    if !defined_char_shape_ids.insert(index) {
        return Err(HwpError::InvalidHwpxStructure {
            reason: format!("Duplicate HWPX charPr id: {index}"),
        });
    }

    let required_slots = index as u64 + 1;
    if required_slots > structure_budget.limits.max_char_shapes {
        return Err(HwpError::ResourceLimitExceeded {
            resource: "HWPX header char shape count",
            path: structure_budget.source.to_string(),
            limit: structure_budget.limits.max_char_shapes,
            actual: required_slots,
        });
    }

    structure_budget.add_char_shape()?;
    if index >= doc_info.char_shapes.len() {
        doc_info
            .char_shapes
            .resize_with(index + 1, default_hwpx_char_shape);
    }
    doc_info.char_shapes[index] = char_shape;
    Ok(())
}

fn default_hwpx_para_shape() -> ParaShape {
    ParaShape {
        attributes1: ParaShapeAttributes1 {
            line_spacing_type_old: LineSpacingTypeOld::ByCharacter,
            align: ParagraphAlignment::Justify,
            line_divide_en: LineDivideUnit::Word,
            line_divide_ko: LineDivideUnit::Word,
            use_line_grid: false,
            blank_min_value: 0,
            protect_orphan_line: false,
            with_next_paragraph: false,
            protect_paragraph: false,
            always_page_break_before: false,
            vertical_align: VerticalAlignment::Baseline,
            line_height_matches_font: false,
            header_shape_type: HeaderShapeType::None,
            paragraph_level: 0,
            connect_border: false,
            ignore_margin: false,
            tail_shape: false,
        },
        left_margin: 0,
        right_margin: 0,
        indent: 0,
        outdent: 0,
        top_spacing: 0,
        bottom_spacing: 0,
        line_spacing_old: 0,
        tab_def_id: 0,
        number_bullet_id: 0,
        border_fill_id: 0,
        border_spacing_left: 0,
        border_spacing_right: 0,
        border_spacing_top: 0,
        border_spacing_bottom: 0,
        attributes2: None,
        attributes3: None,
        line_spacing: None,
    }
}

fn store_completed_para_shape(
    doc_info: &mut DocInfo,
    structure_budget: &mut HeaderStructureBudget<'_>,
    defined_para_shape_ids: &mut std::collections::BTreeSet<usize>,
    para_shape: ParaShape,
    explicit_id: Option<u16>,
) -> Result<(), HwpError> {
    let index = explicit_id
        .map(usize::from)
        .unwrap_or(doc_info.para_shapes.len());

    if !defined_para_shape_ids.insert(index) {
        return Err(HwpError::InvalidHwpxStructure {
            reason: format!("Duplicate HWPX paraPr id: {index}"),
        });
    }

    let required_slots = index as u64 + 1;
    if required_slots > structure_budget.limits.max_para_shapes {
        return Err(HwpError::ResourceLimitExceeded {
            resource: "HWPX header paragraph shape count",
            path: structure_budget.source.to_string(),
            limit: structure_budget.limits.max_para_shapes,
            actual: required_slots,
        });
    }

    structure_budget.add_para_shape()?;
    if index >= doc_info.para_shapes.len() {
        doc_info
            .para_shapes
            .resize_with(index + 1, default_hwpx_para_shape);
    }
    doc_info.para_shapes[index] = para_shape;
    Ok(())
}

fn default_hwpx_style() -> Style {
    Style {
        local_name: String::new(),
        english_name: String::new(),
        style_type: StyleType::Paragraph,
        next_style_id: 0,
        lang_id: 0,
        para_shape_id: None,
        char_shape_id: None,
    }
}

fn store_completed_style(
    doc_info: &mut DocInfo,
    structure_budget: &mut HeaderStructureBudget<'_>,
    defined_style_ids: &mut std::collections::BTreeSet<usize>,
    style: Style,
    explicit_id: Option<u16>,
) -> Result<(), HwpError> {
    let index = explicit_id
        .map(usize::from)
        .unwrap_or(doc_info.styles.len());

    if !defined_style_ids.insert(index) {
        return Err(HwpError::InvalidHwpxStructure {
            reason: format!("Duplicate HWPX style id: {index}"),
        });
    }

    let required_slots = index as u64 + 1;
    if required_slots > structure_budget.limits.max_styles {
        return Err(HwpError::ResourceLimitExceeded {
            resource: "HWPX header style count",
            path: structure_budget.source.to_string(),
            limit: structure_budget.limits.max_styles,
            actual: required_slots,
        });
    }

    structure_budget.add_style()?;
    if index >= doc_info.styles.len() {
        doc_info.styles.resize_with(index + 1, default_hwpx_style);
    }
    doc_info.styles[index] = style;
    Ok(())
}

fn default_hwpx_tab_def() -> TabDef {
    TabDef {
        attributes: TabDefAttributes {
            has_left_auto_tab: false,
            has_right_auto_tab: false,
        },
        count: 0,
        tabs: Vec::new(),
    }
}

fn store_completed_tab_def(
    doc_info: &mut DocInfo,
    structure_budget: &mut HeaderStructureBudget<'_>,
    defined_tab_def_ids: &mut std::collections::BTreeSet<usize>,
    tab_def: TabDef,
    explicit_id: Option<u16>,
) -> Result<(), HwpError> {
    let index = explicit_id
        .map(usize::from)
        .unwrap_or(doc_info.tab_defs.len());

    if !defined_tab_def_ids.insert(index) {
        return Err(HwpError::InvalidHwpxStructure {
            reason: format!("Duplicate HWPX tabPr id: {index}"),
        });
    }

    let required_slots = index as u64 + 1;
    if required_slots > structure_budget.limits.max_tab_defs {
        return Err(HwpError::ResourceLimitExceeded {
            resource: "HWPX header tab definition count",
            path: structure_budget.source.to_string(),
            limit: structure_budget.limits.max_tab_defs,
            actual: required_slots,
        });
    }

    structure_budget.add_tab_def()?;
    if index >= doc_info.tab_defs.len() {
        doc_info
            .tab_defs
            .resize_with(index + 1, default_hwpx_tab_def);
    }
    doc_info.tab_defs[index] = tab_def;
    Ok(())
}

fn push_hwpx_tab_item(
    tab_def: &mut TabDef,
    source: &str,
    tab_item: TabItem,
) -> Result<(), HwpError> {
    let next_count = tab_def.tabs.len() + 1;
    if next_count > i16::MAX as usize {
        return Err(HwpError::ResourceLimitExceeded {
            resource: "HWPX header tab item count per tabPr",
            path: source.to_string(),
            limit: i16::MAX as u64,
            actual: next_count as u64,
        });
    }

    tab_def.tabs.push(tab_item);
    tab_def.count = next_count as i16;
    Ok(())
}

fn default_hwpx_numbering() -> Numbering {
    Numbering {
        levels: Vec::new(),
        extended_levels: Vec::new(),
    }
}

fn default_hwpx_numbering_level() -> NumberingLevelInfo {
    NumberingLevelInfo {
        attributes: NumberingHeaderAttributes {
            align_type: ParagraphAlignType::Left,
            instance_like: false,
            auto_outdent: false,
            distance_type: DistanceType::Ratio,
        },
        width: 0,
        distance: 0,
        char_shape_id: 0,
        format_length: 0,
        format_string: String::new(),
        start_number: 0,
        level_start_number: None,
    }
}

fn default_hwpx_extended_numbering_level() -> ExtendedNumberingLevel {
    ExtendedNumberingLevel {
        format_length: 0,
        format_string: String::new(),
    }
}

#[derive(Debug, Clone)]
struct HwpxNumberingParaHead {
    level: u8,
    info: NumberingLevelInfo,
}

fn default_hwpx_numbering_para_head() -> HwpxNumberingParaHead {
    HwpxNumberingParaHead {
        level: 1,
        info: default_hwpx_numbering_level(),
    }
}

fn push_hwpx_numbering_para_head(
    numbering: &mut Numbering,
    mut para_head: HwpxNumberingParaHead,
) -> Result<(), HwpError> {
    para_head.info.format_string = para_head.info.format_string.trim().to_string();
    para_head.info.format_length = hwpx_format_length(&para_head.info.format_string)?;

    match para_head.level {
        1..=7 => {
            let index = usize::from(para_head.level - 1);
            if index >= numbering.levels.len() {
                numbering
                    .levels
                    .resize_with(index + 1, default_hwpx_numbering_level);
            }
            numbering.levels[index] = para_head.info;
        }
        8..=10 => {
            let index = usize::from(para_head.level - 8);
            if index >= numbering.extended_levels.len() {
                numbering
                    .extended_levels
                    .resize_with(index + 1, default_hwpx_extended_numbering_level);
            }
            numbering.extended_levels[index] = ExtendedNumberingLevel {
                format_length: para_head.info.format_length,
                format_string: para_head.info.format_string,
            };
        }
        _ => {
            return Err(HwpError::InvalidHwpxStructure {
                reason: format!(
                    "HWPX numbering paraHead level out of range: {}",
                    para_head.level
                ),
            });
        }
    }

    Ok(())
}

fn store_completed_numbering(
    doc_info: &mut DocInfo,
    structure_budget: &mut HeaderStructureBudget<'_>,
    defined_numbering_ids: &mut std::collections::BTreeSet<usize>,
    numbering: Numbering,
    explicit_id: Option<u16>,
) -> Result<(), HwpError> {
    let index = explicit_id
        .map(usize::from)
        .unwrap_or(doc_info.numbering.len());

    if !defined_numbering_ids.insert(index) {
        return Err(HwpError::InvalidHwpxStructure {
            reason: format!("Duplicate HWPX numbering id: {index}"),
        });
    }

    let required_slots = index as u64 + 1;
    if required_slots > structure_budget.limits.max_numberings {
        return Err(HwpError::ResourceLimitExceeded {
            resource: "HWPX header numbering count",
            path: structure_budget.source.to_string(),
            limit: structure_budget.limits.max_numberings,
            actual: required_slots,
        });
    }

    structure_budget.add_numbering()?;
    if index >= doc_info.numbering.len() {
        doc_info
            .numbering
            .resize_with(index + 1, default_hwpx_numbering);
    }
    doc_info.numbering[index] = numbering;
    Ok(())
}

/// Parse header.xml content
fn parse_header_xml_content(
    reader: &mut Reader<&[u8]>,
    doc_info: &mut DocInfo,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<(), HwpError> {
    parse_header_xml_content_with_limits(
        reader,
        doc_info,
        warnings,
        diagnostics,
        HeaderStructureLimits::default(),
    )
}

fn parse_header_xml_content_with_limits(
    reader: &mut Reader<&[u8]>,
    doc_info: &mut DocInfo,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
    limits: HeaderStructureLimits,
) -> Result<(), HwpError> {
    reader.config_mut().trim_text(false);
    let mut in_char_properties = false;
    let mut in_para_shapes = false;
    let mut in_face_names = false;
    let mut in_hangul_fontface = false;
    let mut in_styles = false;
    let mut in_tab_properties = false;
    let mut in_numberings = false;
    let mut in_border_fills = false;
    let mut in_margin = false; // Track if inside <hh:margin> element

    // Current charPr being parsed
    let mut current_char_shape: Option<CharShape> = None;
    let mut current_char_shape_id: Option<u16> = None;
    let mut defined_char_shape_ids = std::collections::BTreeSet::new();

    // Current paraPr being parsed
    let mut current_para_shape: Option<ParaShape> = None;
    let mut current_para_shape_id: Option<u16> = None;
    let mut defined_para_shape_ids = std::collections::BTreeSet::new();
    let mut defined_style_ids = std::collections::BTreeSet::new();
    let mut current_tab_def: Option<TabDef> = None;
    let mut current_tab_def_id: Option<u16> = None;
    let mut defined_tab_def_ids = std::collections::BTreeSet::new();
    let mut current_numbering: Option<Numbering> = None;
    let mut current_numbering_id: Option<u16> = None;
    let mut current_numbering_para_head: Option<HwpxNumberingParaHead> = None;
    let mut defined_numbering_ids = std::collections::BTreeSet::new();
    let mut current_border_fill: Option<BorderFill> = None;
    let mut current_border_fill_id: Option<u16> = None;
    let mut defined_border_fill_ids = std::collections::BTreeSet::new();
    let mut xml_budget = XmlParseBudget::new("Contents/header.xml");
    let mut structure_budget = HeaderStructureBudget::new("Contents/header.xml", limits);
    let mut xml_depth = 0usize;
    let mut header_root_seen = false;

    loop {
        let event = reader.read_event();
        if let Ok(ref event) = event {
            xml_budget.observe_event(event)?;
        }

        match &event {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let is_start = matches!(&event, Ok(Event::Start(_)));
                let local_name = e.name();
                let local_name = local_name.as_ref();
                validate_header_root_element(local_name, xml_depth, &mut header_root_seen)?;
                if is_start {
                    xml_depth = xml_depth.saturating_add(1);
                }

                match local_name {
                    // HWPX uses charProperties/charPr instead of charShapes/charShape
                    s if header_has_local_name(s, b"charProperties") && is_start => {
                        in_char_properties = true
                    }
                    s if (header_has_local_name(s, b"paraShapes")
                        || header_has_local_name(s, b"paraProperties"))
                        && is_start =>
                    {
                        in_para_shapes = true
                    }
                    s if header_has_local_name(s, b"faceNames")
                        || header_has_local_name(s, b"fontfaces") =>
                    {
                        in_face_names = true
                    }
                    s if header_has_local_name(s, b"styles") && is_start => in_styles = true,
                    s if header_has_local_name(s, b"tabProperties") && is_start => {
                        in_tab_properties = true
                    }
                    s if header_has_local_name(s, b"numberings") && is_start => {
                        in_numberings = true
                    }
                    s if header_has_local_name(s, b"borderFills") && is_start => {
                        in_border_fills = true
                    }
                    s if header_has_local_name(s, b"borderFill") && in_border_fills => {
                        current_border_fill_id = None;
                        let mut border_fill = default_hwpx_border_fill();
                        for_each_xml_attribute("Contents/header.xml", e, |attr| {
                            match attr.key.as_ref() {
                                b"id" => {
                                    current_border_fill_id =
                                        parse_header_optional_numeric_attr::<u16>(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:borderFill",
                                            "id",
                                            "Invalid borderFill id value",
                                        )?;
                                }
                                b"threeD" => {
                                    border_fill.attributes.has_3d_effect =
                                        parse_header_bool_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:borderFill",
                                            "threeD",
                                            false,
                                        )?;
                                }
                                b"shadow" => {
                                    border_fill.attributes.has_shadow =
                                        parse_header_bool_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:borderFill",
                                            "shadow",
                                            false,
                                        )?;
                                }
                                b"centerLine" => {
                                    border_fill.attributes.has_center_line =
                                        parse_header_enum_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:borderFill",
                                            "centerLine",
                                            false,
                                            |value| {
                                                if value.eq_ignore_ascii_case("NONE") {
                                                    Some(false)
                                                } else if value.eq_ignore_ascii_case("SOLID")
                                                    || value.eq_ignore_ascii_case("DASH")
                                                    || value.eq_ignore_ascii_case("DOT")
                                                    || value.eq_ignore_ascii_case("DASH_DOT")
                                                    || value.eq_ignore_ascii_case("DASH_DOT_DOT")
                                                {
                                                    Some(true)
                                                } else {
                                                    None
                                                }
                                            },
                                        )?;
                                }
                                _ => {}
                            }
                            Ok(())
                        })?;
                        current_border_fill = Some(border_fill);
                    }
                    s if header_has_local_name(s, b"slash") && current_border_fill.is_some() => {
                        if let Some(ref mut border_fill) = current_border_fill {
                            parse_hwpx_slash_element(
                                e,
                                &mut border_fill.attributes,
                                warnings,
                                diagnostics,
                            )?;
                        }
                    }
                    s if header_has_local_name(s, b"backSlash")
                        && current_border_fill.is_some() =>
                    {
                        if let Some(ref mut border_fill) = current_border_fill {
                            parse_hwpx_backslash_element(
                                e,
                                &mut border_fill.attributes,
                                warnings,
                                diagnostics,
                            )?;
                        }
                    }
                    s if header_has_local_name(s, b"leftBorder")
                        && current_border_fill.is_some() =>
                    {
                        if let Some(ref mut border_fill) = current_border_fill {
                            border_fill.borders[0] = parse_hwpx_border_line_element(
                                e,
                                "hh:leftBorder",
                                warnings,
                                diagnostics,
                            )?;
                        }
                    }
                    s if header_has_local_name(s, b"rightBorder")
                        && current_border_fill.is_some() =>
                    {
                        if let Some(ref mut border_fill) = current_border_fill {
                            border_fill.borders[1] = parse_hwpx_border_line_element(
                                e,
                                "hh:rightBorder",
                                warnings,
                                diagnostics,
                            )?;
                        }
                    }
                    s if header_has_local_name(s, b"topBorder")
                        && current_border_fill.is_some() =>
                    {
                        if let Some(ref mut border_fill) = current_border_fill {
                            border_fill.borders[2] = parse_hwpx_border_line_element(
                                e,
                                "hh:topBorder",
                                warnings,
                                diagnostics,
                            )?;
                        }
                    }
                    s if header_has_local_name(s, b"bottomBorder")
                        && current_border_fill.is_some() =>
                    {
                        if let Some(ref mut border_fill) = current_border_fill {
                            border_fill.borders[3] = parse_hwpx_border_line_element(
                                e,
                                "hh:bottomBorder",
                                warnings,
                                diagnostics,
                            )?;
                        }
                    }
                    s if header_has_local_name(s, b"diagonal") && current_border_fill.is_some() => {
                        if let Some(ref mut border_fill) = current_border_fill {
                            border_fill.diagonal =
                                parse_hwpx_diagonal_line_element(e, warnings, diagnostics)?;
                        }
                    }
                    s if header_has_local_name(s, b"winBrush") && current_border_fill.is_some() => {
                        let mut background_color = None;
                        let mut pattern_color = None;
                        let mut alpha = None;
                        for_each_xml_attribute("Contents/header.xml", e, |attr| {
                            match attr.key.as_ref() {
                                b"faceColor" => {
                                    background_color = Some(parse_header_color_attr_or_default(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hc:winBrush",
                                        "faceColor",
                                        COLORREF(0),
                                    )?);
                                }
                                b"hatchColor" => {
                                    pattern_color = Some(parse_header_color_attr_or_default(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hc:winBrush",
                                        "hatchColor",
                                        COLORREF(0),
                                    )?);
                                }
                                b"alpha" => {
                                    alpha = parse_header_optional_numeric_attr::<u8>(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hc:winBrush",
                                        "alpha",
                                        "Invalid hc:winBrush alpha value",
                                    )?;
                                }
                                _ => {}
                            }
                            Ok(())
                        })?;
                        if let Some(ref mut border_fill) = current_border_fill {
                            let (mut background, mut pattern, pattern_type, mut solid_alpha) =
                                match &border_fill.fill {
                                    FillInfo::Solid(solid) => (
                                        solid.background_color,
                                        solid.pattern_color,
                                        solid.pattern_type,
                                        solid.alpha,
                                    ),
                                    _ => (COLORREF(0), COLORREF(0), 0, None),
                                };
                            if let Some(color) = background_color {
                                background = color;
                            }
                            if let Some(color) = pattern_color {
                                pattern = color;
                            }
                            if alpha.is_some() {
                                solid_alpha = alpha;
                            }
                            border_fill.fill = FillInfo::Solid(SolidFill {
                                background_color: background,
                                pattern_color: pattern,
                                pattern_type,
                                alpha: solid_alpha,
                            });
                        }
                    }
                    s if header_has_local_name(s, b"gradation")
                        && current_border_fill.is_some() =>
                    {
                        if let Some(ref mut border_fill) = current_border_fill {
                            border_fill.fill = FillInfo::Gradient(parse_hwpx_gradation_element(
                                e,
                                warnings,
                                diagnostics,
                            )?);
                        }
                    }
                    s if header_has_local_name(s, b"imgBrush") && current_border_fill.is_some() => {
                        if let Some(ref mut border_fill) = current_border_fill {
                            border_fill.fill = FillInfo::Image(parse_hwpx_img_brush_element(e)?);
                        }
                    }
                    s if header_has_local_name(s, b"img") && current_border_fill.is_some() => {
                        if let Some(ref mut border_fill) = current_border_fill {
                            append_hwpx_image_fill_element(border_fill, e, warnings, diagnostics)?;
                        }
                    }
                    s if header_has_local_name(s, b"color") && current_border_fill.is_some() => {
                        if let Some(ref mut border_fill) = current_border_fill {
                            append_hwpx_gradation_color(border_fill, e, warnings, diagnostics)?;
                        }
                    }
                    s if header_has_local_name(s, b"style") && in_styles => {
                        let mut style_id: Option<u16> = None;
                        let mut style = default_hwpx_style();

                        for_each_xml_attribute("Contents/header.xml", e, |attr| {
                            match attr.key.as_ref() {
                                b"id" => {
                                    style_id = parse_header_optional_numeric_attr::<u16>(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hh:style",
                                        "id",
                                        "Invalid style id value",
                                    )?;
                                }
                                b"type" => {
                                    style.style_type = parse_header_enum_attr_or_default(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hh:style",
                                        "type",
                                        StyleType::Paragraph,
                                        |value| match value {
                                            "PARA" | "PARAGRAPH" => Some(StyleType::Paragraph),
                                            "CHAR" | "CHARACTER" => Some(StyleType::Character),
                                            _ => None,
                                        },
                                    )?;
                                }
                                b"name" => {
                                    style.local_name = parse_string_attr(
                                        "Contents/header.xml",
                                        "hh:style",
                                        "name",
                                        &attr,
                                    )?;
                                }
                                b"engName" => {
                                    style.english_name = parse_string_attr(
                                        "Contents/header.xml",
                                        "hh:style",
                                        "engName",
                                        &attr,
                                    )?;
                                }
                                b"paraPrIDRef" => {
                                    style.para_shape_id = parse_header_optional_numeric_attr::<u16>(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hh:style",
                                        "paraPrIDRef",
                                        "Invalid style paraPrIDRef value",
                                    )?;
                                }
                                b"charPrIDRef" => {
                                    style.char_shape_id = parse_header_optional_numeric_attr::<u16>(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hh:style",
                                        "charPrIDRef",
                                        "Invalid style charPrIDRef value",
                                    )?;
                                }
                                b"nextStyleIDRef" => {
                                    style.next_style_id = parse_header_numeric_attr_or_default(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hh:style",
                                        "nextStyleIDRef",
                                        "Invalid style nextStyleIDRef value",
                                        0,
                                    )?;
                                }
                                b"langID" => {
                                    style.lang_id = parse_header_numeric_attr_or_default(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hh:style",
                                        "langID",
                                        "Invalid style langID value",
                                        0,
                                    )?;
                                }
                                _ => {}
                            }
                            Ok(())
                        })?;

                        store_completed_style(
                            doc_info,
                            &mut structure_budget,
                            &mut defined_style_ids,
                            style,
                            style_id,
                        )?;
                    }
                    s if header_has_local_name(s, b"numbering") && in_numberings => {
                        current_numbering_id = None;
                        current_numbering = Some(default_hwpx_numbering());

                        for_each_xml_attribute("Contents/header.xml", e, |attr| {
                            if attr.key.as_ref() == b"id" {
                                current_numbering_id = parse_header_optional_numeric_attr::<u16>(
                                    &attr,
                                    warnings,
                                    diagnostics,
                                    "hh:numbering",
                                    "id",
                                    "Invalid numbering id value",
                                )?;
                            }
                            Ok(())
                        })?;
                    }
                    s if header_has_local_name(s, b"paraHead") && current_numbering.is_some() => {
                        let mut para_head = default_hwpx_numbering_para_head();

                        for_each_xml_attribute("Contents/header.xml", e, |attr| {
                            match attr.key.as_ref() {
                                b"level" => {
                                    para_head.level = parse_header_numeric_attr_or_default(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hh:paraHead",
                                        "level",
                                        "Invalid numbering level value",
                                        1,
                                    )?;
                                }
                                b"align" => {
                                    para_head.info.attributes.align_type =
                                        parse_hwpx_numbering_align_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                        )?;
                                }
                                b"useInstWidth" => {
                                    para_head.info.attributes.instance_like =
                                        parse_header_bool_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:paraHead",
                                            "useInstWidth",
                                            false,
                                        )?;
                                }
                                b"autoIndent" => {
                                    para_head.info.attributes.auto_outdent =
                                        parse_header_bool_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:paraHead",
                                            "autoIndent",
                                            false,
                                        )?;
                                }
                                b"textOffsetType" => {
                                    para_head.info.attributes.distance_type =
                                        parse_hwpx_numbering_distance_type_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                        )?;
                                }
                                b"widthAdjust" => {
                                    para_head.info.width = parse_header_numeric_attr_or_default(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hh:paraHead",
                                        "widthAdjust",
                                        "Invalid numbering widthAdjust value",
                                        0,
                                    )?;
                                }
                                b"textOffset" => {
                                    para_head.info.distance = parse_header_numeric_attr_or_default(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hh:paraHead",
                                        "textOffset",
                                        "Invalid numbering textOffset value",
                                        0,
                                    )?;
                                }
                                b"charPrIDRef" => {
                                    para_head.info.char_shape_id =
                                        parse_header_numeric_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:paraHead",
                                            "charPrIDRef",
                                            "Invalid numbering charPrIDRef value",
                                            0,
                                        )?;
                                }
                                b"start" => {
                                    para_head.info.start_number =
                                        parse_header_numeric_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:paraHead",
                                            "start",
                                            "Invalid numbering start value",
                                            0,
                                        )?;
                                }
                                _ => {}
                            }
                            Ok(())
                        })?;

                        if is_start {
                            current_numbering_para_head = Some(para_head);
                        } else if let Some(ref mut numbering) = current_numbering {
                            push_hwpx_numbering_para_head(numbering, para_head)?;
                        }
                    }
                    s if header_has_local_name(s, b"tabPr") && in_tab_properties => {
                        current_tab_def_id = None;
                        let mut tab_def = default_hwpx_tab_def();

                        for_each_xml_attribute("Contents/header.xml", e, |attr| {
                            match attr.key.as_ref() {
                                b"id" => {
                                    current_tab_def_id = parse_header_optional_numeric_attr::<u16>(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hh:tabPr",
                                        "id",
                                        "Invalid tabPr id value",
                                    )?;
                                }
                                b"autoTabLeft" | b"AutoTabLeft" => {
                                    tab_def.attributes.has_left_auto_tab =
                                        parse_header_bool_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:tabPr",
                                            "autoTabLeft",
                                            false,
                                        )?;
                                }
                                b"autoTabRight" | b"AutoTabRight" => {
                                    tab_def.attributes.has_right_auto_tab =
                                        parse_header_bool_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:tabPr",
                                            "autoTabRight",
                                            false,
                                        )?;
                                }
                                _ => {}
                            }
                            Ok(())
                        })?;

                        current_tab_def = Some(tab_def);
                    }
                    s if header_has_local_name(s, b"tabItem") && current_tab_def.is_some() => {
                        let mut position = HWPUNIT(0);
                        let mut tab_type = TabType::Left;
                        let mut fill_type = 0;

                        for_each_xml_attribute("Contents/header.xml", e, |attr| {
                            match attr.key.as_ref() {
                                b"pos" | b"Pos" => {
                                    let value: u32 = parse_header_numeric_attr_or_default(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hh:tabItem",
                                        "pos",
                                        "Invalid tabItem pos value",
                                        0,
                                    )?;
                                    position = HWPUNIT(value);
                                }
                                b"type" | b"Type" => {
                                    tab_type = parse_hwpx_tab_type_attr_or_default(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                    )?;
                                }
                                b"leader" | b"Leader" => {
                                    fill_type = parse_hwpx_tab_leader_attr_or_default(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                    )?;
                                }
                                _ => {}
                            }
                            Ok(())
                        })?;

                        if let Some(ref mut tab_def) = current_tab_def {
                            push_hwpx_tab_item(
                                tab_def,
                                structure_budget.source,
                                TabItem {
                                    position,
                                    tab_type,
                                    fill_type,
                                },
                            )?;
                        }
                    }
                    s if header_has_local_name(s, b"fontface") && in_face_names && is_start => {
                        in_hangul_fontface = false;
                        for_each_xml_attribute("Contents/header.xml", e, |attr| {
                            if attr.key.as_ref() == b"lang" {
                                let lang = parse_string_attr(
                                    "Contents/header.xml",
                                    "hh:fontface",
                                    "lang",
                                    &attr,
                                )?;
                                in_hangul_fontface = lang.eq_ignore_ascii_case("HANGUL");
                            }
                            Ok(())
                        })?;
                    }
                    s if header_has_local_name(s, b"charPr") && in_char_properties => {
                        // Parse charPr element attributes
                        current_char_shape_id = None;
                        let mut height: i32 = 1000; // Default 10pt
                        let mut text_color: COLORREF = COLORREF(0);

                        for_each_xml_attribute("Contents/header.xml", e, |attr| {
                            match attr.key.as_ref() {
                                b"id" => {
                                    current_char_shape_id =
                                        parse_header_optional_numeric_attr::<u16>(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:charPr",
                                            "id",
                                            "Invalid charPr id value",
                                        )?;
                                }
                                b"height" => {
                                    height = parse_char_height_attr_or_default(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                    )?;
                                }
                                b"textColor" => {
                                    text_color = parse_header_color_attr_or_default(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hh:charPr",
                                        "textColor",
                                        COLORREF(0),
                                    )?;
                                }
                                _ => {}
                            }
                            Ok(())
                        })?;

                        // Create new CharShape with parsed values
                        current_char_shape = Some(hwpx_char_shape_from_attrs(height, text_color));
                    }
                    s if header_has_local_name(s, b"fontRef") => {
                        // Parse font references for current char shape
                        if let Some(ref mut cs) = current_char_shape {
                            for_each_xml_attribute("Contents/header.xml", e, |attr| {
                                match attr.key.as_ref() {
                                    b"hangul" => {
                                        cs.font_ids.korean = parse_header_numeric_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:fontRef",
                                            "hangul",
                                            "Invalid fontRef attribute value",
                                            0,
                                        )?;
                                    }
                                    b"latin" => {
                                        cs.font_ids.english = parse_header_numeric_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:fontRef",
                                            "latin",
                                            "Invalid fontRef attribute value",
                                            0,
                                        )?;
                                    }
                                    b"hanja" => {
                                        cs.font_ids.chinese = parse_header_numeric_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:fontRef",
                                            "hanja",
                                            "Invalid fontRef attribute value",
                                            0,
                                        )?;
                                    }
                                    b"japanese" => {
                                        cs.font_ids.japanese =
                                            parse_header_numeric_attr_or_default(
                                                &attr,
                                                warnings,
                                                diagnostics,
                                                "hh:fontRef",
                                                "japanese",
                                                "Invalid fontRef attribute value",
                                                0,
                                            )?;
                                    }
                                    b"other" => {
                                        cs.font_ids.other = parse_header_numeric_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:fontRef",
                                            "other",
                                            "Invalid fontRef attribute value",
                                            0,
                                        )?;
                                    }
                                    b"symbol" => {
                                        cs.font_ids.symbol = parse_header_numeric_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:fontRef",
                                            "symbol",
                                            "Invalid fontRef attribute value",
                                            0,
                                        )?;
                                    }
                                    b"user" => {
                                        cs.font_ids.user = parse_header_numeric_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:fontRef",
                                            "user",
                                            "Invalid fontRef attribute value",
                                            0,
                                        )?;
                                    }
                                    _ => {}
                                }
                                Ok(())
                            })?;
                        }
                    }
                    s if header_has_local_name(s, b"underline") => {
                        // Parse underline for current char shape
                        if let Some(ref mut cs) = current_char_shape {
                            for_each_xml_attribute("Contents/header.xml", e, |attr| {
                                match attr.key.as_ref() {
                                    b"type" => {
                                        cs.attributes.underline_type =
                                            parse_header_enum_attr_or_default(
                                                &attr,
                                                warnings,
                                                diagnostics,
                                                "hh:underline",
                                                "type",
                                                0,
                                                |value| match value {
                                                    "BOTTOM" => Some(1),
                                                    "TOP" => Some(2),
                                                    "NONE" => Some(0),
                                                    _ => None,
                                                },
                                            )?;
                                    }
                                    b"color" => {
                                        cs.underline_color = parse_header_color_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:underline",
                                            "color",
                                            COLORREF(0),
                                        )?;
                                    }
                                    _ => {}
                                }
                                Ok(())
                            })?;
                        }
                    }
                    s if header_has_local_name(s, b"strikeout") => {
                        // Parse strikethrough for current char shape
                        if let Some(ref mut cs) = current_char_shape {
                            for_each_xml_attribute("Contents/header.xml", e, |attr| {
                                match attr.key.as_ref() {
                                    b"shape" => {
                                        // Unknown values keep the previous recovery behavior:
                                        // enabled strikethrough, but now with diagnostics.
                                        cs.attributes.strikethrough =
                                            parse_header_enum_attr_or_default(
                                                &attr,
                                                warnings,
                                                diagnostics,
                                                "hh:strikeout",
                                                "shape",
                                                1,
                                                |value| match value {
                                                    "NONE" => Some(0),
                                                    "CONTINUOUS" | "DASH" | "DOT" | "DASH_DOT"
                                                    | "DASH_DOT_DOT" | "3D" => Some(1),
                                                    _ => None,
                                                },
                                            )?;
                                    }
                                    b"color" => {
                                        cs.strikethrough_color =
                                            Some(parse_header_color_attr_or_default(
                                                &attr,
                                                warnings,
                                                diagnostics,
                                                "hh:strikeout",
                                                "color",
                                                COLORREF(0),
                                            )?);
                                    }
                                    _ => {}
                                }
                                Ok(())
                            })?;
                        }
                    }
                    s if header_has_local_name(s, b"bold") => {
                        // Parse bold for current char shape
                        // HWPX uses <hh:bold/> self-closing element to indicate bold
                        if let Some(ref mut cs) = current_char_shape {
                            cs.attributes.bold = true;
                        }
                    }
                    s if header_has_local_name(s, b"italic") => {
                        // Parse italic for current char shape
                        // HWPX uses <hh:italic/> self-closing element to indicate italic
                        if let Some(ref mut cs) = current_char_shape {
                            cs.attributes.italic = true;
                        }
                    }
                    s if (header_has_local_name(s, b"paraPr")
                        || header_has_local_name(s, b"paraShape"))
                        && in_para_shapes =>
                    {
                        current_para_shape_id = None;
                        let mut para_shape = default_hwpx_para_shape();
                        for_each_xml_attribute("Contents/header.xml", e, |attr| {
                            match attr.key.as_ref() {
                                b"id" => {
                                    current_para_shape_id =
                                        parse_header_optional_numeric_attr::<u16>(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:paraPr",
                                            "id",
                                            "Invalid paraPr id value",
                                        )?;
                                }
                                b"tabPrIDRef" => {
                                    para_shape.tab_def_id = parse_header_numeric_attr_or_default(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hh:paraPr",
                                        "tabPrIDRef",
                                        "Invalid paraPr tabPrIDRef value",
                                        0,
                                    )?;
                                }
                                b"condense" => {
                                    para_shape.attributes1.blank_min_value =
                                        parse_hwpx_condense_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                        )?;
                                }
                                b"fontLineHeight" => {
                                    para_shape.attributes1.line_height_matches_font =
                                        parse_header_bool_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:paraPr",
                                            "fontLineHeight",
                                            false,
                                        )?;
                                }
                                b"snapToGrid" => {
                                    para_shape.attributes1.use_line_grid =
                                        parse_header_bool_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:paraPr",
                                            "snapToGrid",
                                            false,
                                        )?;
                                }
                                _ => {}
                            }
                            Ok(())
                        })?;
                        current_para_shape = Some(para_shape);
                    }
                    s if header_has_local_name(s, b"align") && current_para_shape.is_some() => {
                        // Parse <hh:align horizontal="JUSTIFY|LEFT|RIGHT|CENTER" />
                        for_each_xml_attribute("Contents/header.xml", e, |attr| {
                            if attr.key.as_ref() == b"horizontal" {
                                if let Some(ref mut ps) = current_para_shape {
                                    ps.attributes1.align = parse_header_enum_attr_or_default(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hh:align",
                                        "horizontal",
                                        ParagraphAlignment::Justify,
                                        |value| match value {
                                            "LEFT" => Some(ParagraphAlignment::Left),
                                            "RIGHT" => Some(ParagraphAlignment::Right),
                                            "CENTER" => Some(ParagraphAlignment::Center),
                                            "DISTRIBUTE" => Some(ParagraphAlignment::Distribute),
                                            "JUSTIFY" => Some(ParagraphAlignment::Justify),
                                            _ => None,
                                        },
                                    )?;
                                }
                            }
                            Ok(())
                        })?;
                    }
                    s if header_has_local_name(s, b"heading") && current_para_shape.is_some() => {
                        for_each_xml_attribute("Contents/header.xml", e, |attr| {
                            if let Some(ref mut ps) = current_para_shape {
                                match attr.key.as_ref() {
                                    b"type" => {
                                        ps.attributes1.header_shape_type =
                                            parse_hwpx_heading_type_attr_or_default(
                                                &attr,
                                                warnings,
                                                diagnostics,
                                            )?;
                                    }
                                    b"idRef" => {
                                        ps.number_bullet_id = parse_header_numeric_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:heading",
                                            "idRef",
                                            "Invalid heading idRef value",
                                            0,
                                        )?;
                                    }
                                    b"level" => {
                                        ps.attributes1.paragraph_level =
                                            parse_header_numeric_attr_or_default(
                                                &attr,
                                                warnings,
                                                diagnostics,
                                                "hh:heading",
                                                "level",
                                                "Invalid heading level value",
                                                0,
                                            )?;
                                    }
                                    _ => {}
                                }
                            }
                            Ok(())
                        })?;
                    }
                    s if header_has_local_name(s, b"lineSpacing")
                        && current_para_shape.is_some() =>
                    {
                        let mut line_spacing_type = None;
                        let mut line_spacing_value = None;
                        for_each_xml_attribute("Contents/header.xml", e, |attr| {
                            match attr.key.as_ref() {
                                b"type" => {
                                    line_spacing_type =
                                        Some(parse_hwpx_line_spacing_type_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                        )?);
                                }
                                b"value" => {
                                    line_spacing_value =
                                        Some(parse_header_numeric_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:lineSpacing",
                                            "value",
                                            "Invalid lineSpacing value",
                                            0,
                                        )?);
                                }
                                _ => {}
                            }
                            Ok(())
                        })?;
                        if let Some(ref mut ps) = current_para_shape {
                            if let Some(value) = line_spacing_value {
                                ps.line_spacing = Some(value);
                            }
                            if line_spacing_type.is_some() || line_spacing_value.is_some() {
                                ps.attributes3 = Some(ParaShapeAttributes3 {
                                    line_spacing_type: line_spacing_type
                                        .unwrap_or(LineSpacingType::ByCharacter),
                                });
                            }
                        }
                    }
                    s if header_has_local_name(s, b"border") && current_para_shape.is_some() => {
                        for_each_xml_attribute("Contents/header.xml", e, |attr| {
                            if let Some(ref mut ps) = current_para_shape {
                                match attr.key.as_ref() {
                                    b"borderFillIDRef" => {
                                        ps.border_fill_id = parse_header_numeric_attr_or_default(
                                            &attr,
                                            warnings,
                                            diagnostics,
                                            "hh:border",
                                            "borderFillIDRef",
                                            "Invalid borderFillIDRef value",
                                            0,
                                        )?;
                                    }
                                    b"offsetLeft" => {
                                        ps.border_spacing_left =
                                            parse_header_numeric_attr_or_default(
                                                &attr,
                                                warnings,
                                                diagnostics,
                                                "hh:border",
                                                "offsetLeft",
                                                "Invalid border offsetLeft value",
                                                0,
                                            )?;
                                    }
                                    b"offsetRight" => {
                                        ps.border_spacing_right =
                                            parse_header_numeric_attr_or_default(
                                                &attr,
                                                warnings,
                                                diagnostics,
                                                "hh:border",
                                                "offsetRight",
                                                "Invalid border offsetRight value",
                                                0,
                                            )?;
                                    }
                                    b"offsetTop" => {
                                        ps.border_spacing_top =
                                            parse_header_numeric_attr_or_default(
                                                &attr,
                                                warnings,
                                                diagnostics,
                                                "hh:border",
                                                "offsetTop",
                                                "Invalid border offsetTop value",
                                                0,
                                            )?;
                                    }
                                    b"offsetBottom" => {
                                        ps.border_spacing_bottom =
                                            parse_header_numeric_attr_or_default(
                                                &attr,
                                                warnings,
                                                diagnostics,
                                                "hh:border",
                                                "offsetBottom",
                                                "Invalid border offsetBottom value",
                                                0,
                                            )?;
                                    }
                                    b"connect" => {
                                        ps.attributes1.connect_border =
                                            parse_header_bool_attr_or_default(
                                                &attr,
                                                warnings,
                                                diagnostics,
                                                "hh:border",
                                                "connect",
                                                false,
                                            )?;
                                    }
                                    b"ignoreMargin" => {
                                        ps.attributes1.ignore_margin =
                                            parse_header_bool_attr_or_default(
                                                &attr,
                                                warnings,
                                                diagnostics,
                                                "hh:border",
                                                "ignoreMargin",
                                                false,
                                            )?;
                                    }
                                    _ => {}
                                }
                            }
                            Ok(())
                        })?;
                    }
                    s if header_has_local_name(s, b"breakSetting")
                        && current_para_shape.is_some() =>
                    {
                        for_each_xml_attribute("Contents/header.xml", e, |attr| {
                            if let Some(ref mut ps) = current_para_shape {
                                match attr.key.as_ref() {
                                    b"breakLatinWord" => {
                                        ps.attributes1.line_divide_en =
                                            parse_hwpx_line_divide_attr_or_default(
                                                &attr,
                                                warnings,
                                                diagnostics,
                                                "breakLatinWord",
                                            )?;
                                    }
                                    b"breakNonLatinWord" => {
                                        ps.attributes1.line_divide_ko =
                                            parse_hwpx_line_divide_attr_or_default(
                                                &attr,
                                                warnings,
                                                diagnostics,
                                                "breakNonLatinWord",
                                            )?;
                                    }
                                    b"widowOrphan" => {
                                        ps.attributes1.protect_orphan_line =
                                            parse_header_bool_attr_or_default(
                                                &attr,
                                                warnings,
                                                diagnostics,
                                                "hh:breakSetting",
                                                "widowOrphan",
                                                false,
                                            )?;
                                    }
                                    b"keepWithNext" => {
                                        ps.attributes1.with_next_paragraph =
                                            parse_header_bool_attr_or_default(
                                                &attr,
                                                warnings,
                                                diagnostics,
                                                "hh:breakSetting",
                                                "keepWithNext",
                                                false,
                                            )?;
                                    }
                                    b"keepLines" => {
                                        ps.attributes1.protect_paragraph =
                                            parse_header_bool_attr_or_default(
                                                &attr,
                                                warnings,
                                                diagnostics,
                                                "hh:breakSetting",
                                                "keepLines",
                                                false,
                                            )?;
                                    }
                                    b"pageBreakBefore" => {
                                        ps.attributes1.always_page_break_before =
                                            parse_header_bool_attr_or_default(
                                                &attr,
                                                warnings,
                                                diagnostics,
                                                "hh:breakSetting",
                                                "pageBreakBefore",
                                                false,
                                            )?;
                                    }
                                    _ => {}
                                }
                            }
                            Ok(())
                        })?;
                    }
                    s if header_has_local_name(s, b"autoSpacing")
                        && current_para_shape.is_some() =>
                    {
                        for_each_xml_attribute("Contents/header.xml", e, |attr| {
                            if let Some(ref mut ps) = current_para_shape {
                                let attributes2 =
                                    ps.attributes2.get_or_insert(ParaShapeAttributes2 {
                                        single_line_input: 0,
                                        auto_spacing_ko_en: false,
                                        auto_spacing_ko_num: false,
                                    });
                                match attr.key.as_ref() {
                                    b"eAsianEng" => {
                                        attributes2.auto_spacing_ko_en =
                                            parse_header_bool_attr_or_default(
                                                &attr,
                                                warnings,
                                                diagnostics,
                                                "hh:autoSpacing",
                                                "eAsianEng",
                                                false,
                                            )?;
                                    }
                                    b"eAsianNum" => {
                                        attributes2.auto_spacing_ko_num =
                                            parse_header_bool_attr_or_default(
                                                &attr,
                                                warnings,
                                                diagnostics,
                                                "hh:autoSpacing",
                                                "eAsianNum",
                                                false,
                                            )?;
                                    }
                                    _ => {}
                                }
                            }
                            Ok(())
                        })?;
                    }
                    s if header_has_local_name(s, b"margin") && current_para_shape.is_some() => {
                        // Enter <hh:margin> element
                        in_margin = true;
                    }
                    s if header_has_local_name(s, b"intent")
                        && in_margin
                        && current_para_shape.is_some() =>
                    {
                        // Parse <hc:intent value="N" unit="HWPUNIT" />
                        for_each_xml_attribute("Contents/header.xml", e, |attr| {
                            if attr.key.as_ref() == b"value" {
                                if let Some(ref mut ps) = current_para_shape {
                                    // HWPUNIT = 1/7200 inch, HWP INT32 = 1/1800 inch
                                    // Convert: value / 4
                                    let hwpunit_value: i32 = parse_header_numeric_attr_or_default(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hc:intent",
                                        "value",
                                        "Invalid margin intent value",
                                        0,
                                    )?;
                                    ps.indent = hwpunit_value / 4;
                                }
                            }
                            Ok(())
                        })?;
                    }
                    s if header_has_local_name(s, b"left")
                        && in_margin
                        && current_para_shape.is_some() =>
                    {
                        // Parse <hc:left value="N" unit="HWPUNIT" />
                        for_each_xml_attribute("Contents/header.xml", e, |attr| {
                            if attr.key.as_ref() == b"value" {
                                if let Some(ref mut ps) = current_para_shape {
                                    let hwpunit_value: i32 = parse_header_numeric_attr_or_default(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hc:left",
                                        "value",
                                        "Invalid margin left value",
                                        0,
                                    )?;
                                    ps.left_margin = hwpunit_value / 4;
                                }
                            }
                            Ok(())
                        })?;
                    }
                    s if header_has_local_name(s, b"right")
                        && in_margin
                        && current_para_shape.is_some() =>
                    {
                        // Parse <hc:right value="N" unit="HWPUNIT" />
                        for_each_xml_attribute("Contents/header.xml", e, |attr| {
                            if attr.key.as_ref() == b"value" {
                                if let Some(ref mut ps) = current_para_shape {
                                    let hwpunit_value: i32 = parse_header_numeric_attr_or_default(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hc:right",
                                        "value",
                                        "Invalid margin right value",
                                        0,
                                    )?;
                                    ps.right_margin = hwpunit_value / 4;
                                }
                            }
                            Ok(())
                        })?;
                    }
                    s if header_has_local_name(s, b"prev")
                        && in_margin
                        && current_para_shape.is_some() =>
                    {
                        // Parse <hc:prev value="N" unit="HWPUNIT" />
                        for_each_xml_attribute("Contents/header.xml", e, |attr| {
                            if attr.key.as_ref() == b"value" {
                                if let Some(ref mut ps) = current_para_shape {
                                    let hwpunit_value: i32 = parse_header_numeric_attr_or_default(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hc:prev",
                                        "value",
                                        "Invalid margin prev value",
                                        0,
                                    )?;
                                    ps.top_spacing = hwpunit_value / 4;
                                }
                            }
                            Ok(())
                        })?;
                    }
                    s if header_has_local_name(s, b"next")
                        && in_margin
                        && current_para_shape.is_some() =>
                    {
                        // Parse <hc:next value="N" unit="HWPUNIT" />
                        for_each_xml_attribute("Contents/header.xml", e, |attr| {
                            if attr.key.as_ref() == b"value" {
                                if let Some(ref mut ps) = current_para_shape {
                                    let hwpunit_value: i32 = parse_header_numeric_attr_or_default(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hc:next",
                                        "value",
                                        "Invalid margin next value",
                                        0,
                                    )?;
                                    ps.bottom_spacing = hwpunit_value / 4;
                                }
                            }
                            Ok(())
                        })?;
                    }
                    s if (header_has_local_name(s, b"font")
                        || header_has_local_name(s, b"faceName"))
                        && in_face_names
                        && in_hangul_fontface =>
                    {
                        let mut font_id: Option<u16> = None;
                        let mut face_name: Option<String> = None;
                        for_each_xml_attribute("Contents/header.xml", e, |attr| {
                            match attr.key.as_ref() {
                                b"id" => {
                                    font_id = parse_header_optional_numeric_attr::<u16>(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        "hh:font",
                                        "id",
                                        "Invalid font id value",
                                    )?;
                                }
                                b"face" | b"name" => {
                                    let attribute = if attr.key.as_ref() == b"face" {
                                        "face"
                                    } else {
                                        "name"
                                    };
                                    face_name = Some(parse_string_attr(
                                        "Contents/header.xml",
                                        "hh:font",
                                        attribute,
                                        &attr,
                                    )?);
                                }
                                _ => {}
                            }
                            Ok(())
                        })?;

                        if let (Some(font_id), Some(face_name)) = (font_id, face_name) {
                            store_hwpx_face_name(doc_info, font_id, face_name);
                        }
                    }
                    _ => {}
                }

                if !is_start && header_has_local_name(local_name, b"charPr") && in_char_properties {
                    if let Some(cs) = current_char_shape.take() {
                        store_completed_char_shape(
                            doc_info,
                            &mut structure_budget,
                            &mut defined_char_shape_ids,
                            cs,
                            current_char_shape_id.take(),
                        )?;
                    }
                }

                if !is_start
                    && (header_has_local_name(local_name, b"paraPr")
                        || header_has_local_name(local_name, b"paraShape"))
                    && in_para_shapes
                {
                    if let Some(ps) = current_para_shape.take() {
                        store_completed_para_shape(
                            doc_info,
                            &mut structure_budget,
                            &mut defined_para_shape_ids,
                            ps,
                            current_para_shape_id.take(),
                        )?;
                    }
                }

                if !is_start && header_has_local_name(local_name, b"tabPr") && in_tab_properties {
                    if let Some(tab_def) = current_tab_def.take() {
                        store_completed_tab_def(
                            doc_info,
                            &mut structure_budget,
                            &mut defined_tab_def_ids,
                            tab_def,
                            current_tab_def_id.take(),
                        )?;
                    }
                }

                if !is_start && header_has_local_name(local_name, b"numbering") && in_numberings {
                    if let Some(numbering) = current_numbering.take() {
                        store_completed_numbering(
                            doc_info,
                            &mut structure_budget,
                            &mut defined_numbering_ids,
                            numbering,
                            current_numbering_id.take(),
                        )?;
                    }
                }

                if !is_start && header_has_local_name(local_name, b"borderFill") && in_border_fills
                {
                    if let Some(mut border_fill) = current_border_fill.take() {
                        finalize_hwpx_border_fill(&mut border_fill, warnings, diagnostics);
                        store_completed_border_fill(
                            doc_info,
                            &mut structure_budget,
                            &mut defined_border_fill_ids,
                            border_fill,
                            current_border_fill_id.take(),
                        )?;
                    }
                }
            }
            Ok(Event::Text(e)) => {
                if let Some(ref mut para_head) = current_numbering_para_head {
                    para_head
                        .info
                        .format_string
                        .push_str(&unescape_header_text(e)?);
                }
            }
            Ok(Event::GeneralRef(e)) => {
                if let Some(ref mut para_head) = current_numbering_para_head {
                    para_head
                        .info
                        .format_string
                        .push_str(&unescape_header_reference(e)?);
                }
            }
            Ok(Event::CData(e)) => {
                if let Some(ref mut para_head) = current_numbering_para_head {
                    para_head
                        .info
                        .format_string
                        .push_str(&decode_header_cdata(e)?);
                }
            }
            Ok(Event::End(e)) => {
                let local_name = e.name();
                let local_name = local_name.as_ref();

                match local_name {
                    s if header_has_local_name(s, b"charProperties") => in_char_properties = false,
                    s if header_has_local_name(s, b"paraShapes")
                        || header_has_local_name(s, b"paraProperties") =>
                    {
                        in_para_shapes = false
                    }
                    s if header_has_local_name(s, b"faceNames")
                        || header_has_local_name(s, b"fontfaces") =>
                    {
                        in_face_names = false;
                        in_hangul_fontface = false;
                    }
                    s if header_has_local_name(s, b"styles") => in_styles = false,
                    s if header_has_local_name(s, b"tabProperties") => in_tab_properties = false,
                    s if header_has_local_name(s, b"numberings") => in_numberings = false,
                    s if header_has_local_name(s, b"borderFills") => in_border_fills = false,
                    s if header_has_local_name(s, b"fontface") => in_hangul_fontface = false,
                    s if header_has_local_name(s, b"charPr") => {
                        // Save completed char shape
                        if let Some(cs) = current_char_shape.take() {
                            store_completed_char_shape(
                                doc_info,
                                &mut structure_budget,
                                &mut defined_char_shape_ids,
                                cs,
                                current_char_shape_id.take(),
                            )?;
                        }
                    }
                    s if header_has_local_name(s, b"paraPr")
                        || header_has_local_name(s, b"paraShape") =>
                    {
                        // Save completed para shape
                        if let Some(ps) = current_para_shape.take() {
                            store_completed_para_shape(
                                doc_info,
                                &mut structure_budget,
                                &mut defined_para_shape_ids,
                                ps,
                                current_para_shape_id.take(),
                            )?;
                        }
                    }
                    s if header_has_local_name(s, b"tabPr") => {
                        if let Some(tab_def) = current_tab_def.take() {
                            store_completed_tab_def(
                                doc_info,
                                &mut structure_budget,
                                &mut defined_tab_def_ids,
                                tab_def,
                                current_tab_def_id.take(),
                            )?;
                        }
                    }
                    s if header_has_local_name(s, b"paraHead") => {
                        if let (Some(numbering), Some(para_head)) = (
                            current_numbering.as_mut(),
                            current_numbering_para_head.take(),
                        ) {
                            push_hwpx_numbering_para_head(numbering, para_head)?;
                        }
                    }
                    s if header_has_local_name(s, b"numbering") => {
                        if let Some(numbering) = current_numbering.take() {
                            store_completed_numbering(
                                doc_info,
                                &mut structure_budget,
                                &mut defined_numbering_ids,
                                numbering,
                                current_numbering_id.take(),
                            )?;
                        }
                    }
                    s if header_has_local_name(s, b"borderFill") => {
                        if let Some(mut border_fill) = current_border_fill.take() {
                            finalize_hwpx_border_fill(&mut border_fill, warnings, diagnostics);
                            store_completed_border_fill(
                                doc_info,
                                &mut structure_budget,
                                &mut defined_border_fill_ids,
                                border_fill,
                                current_border_fill_id.take(),
                            )?;
                        }
                    }
                    s if header_has_local_name(s, b"margin") => {
                        // Exit <hh:margin> element
                        in_margin = false;
                    }
                    _ => {}
                }
                xml_budget.finish_end_event(e)?;
                xml_depth = xml_depth.saturating_sub(1);
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(HwpError::XmlParseError(format!(
                    "Error parsing header.xml: {e}"
                )))
            }
            _ => {}
        }
    }

    if !header_root_seen {
        return Err(HwpError::InvalidHwpxStructure {
            reason: "HWPX header XML root element must be hh:head".to_string(),
        });
    }

    // Create default document properties if not set
    if doc_info.document_properties.is_none() {
        doc_info.document_properties = Some(crate::document::DocumentProperties {
            area_count: 1,
            start_number_info: 0,
            page_start_number: 1,
            footnote_start_number: 1,
            endnote_start_number: 1,
            image_start_number: 1,
            table_start_number: 1,
            formula_start_number: 1,
            list_id: 0,
            paragraph_id: 0,
            character_position: 0,
        });
    }

    Ok(())
}

fn validate_header_root_element(
    name: &[u8],
    current_depth: usize,
    header_root_seen: &mut bool,
) -> Result<(), HwpError> {
    if current_depth != 0 {
        return Ok(());
    }

    if *header_root_seen {
        return Err(HwpError::InvalidHwpxStructure {
            reason: "HWPX header XML contains multiple root elements".to_string(),
        });
    }

    if !is_header_root_element(name) {
        return Err(HwpError::InvalidHwpxStructure {
            reason: "HWPX header XML root element must be hh:head".to_string(),
        });
    }

    *header_root_seen = true;
    Ok(())
}

fn is_header_root_element(name: &[u8]) -> bool {
    header_has_local_name(name, b"head")
}

fn header_has_local_name(name: &[u8], local_name: &[u8]) -> bool {
    if name == local_name {
        return true;
    }

    name.iter()
        .rposition(|byte| *byte == b':')
        .map(|position| &name[position + 1..])
        == Some(local_name)
}

fn record_header_invalid_value(
    diagnostics: &mut DiagnosticReport,
    element: &str,
    attribute: &str,
    value: &str,
    message: impl Into<String>,
) {
    diagnostics.push(
        DiagnosticItem::new(
            DiagnosticSeverity::RecoveredError,
            DiagnosticCategory::InvalidValue,
            message,
        )
        .with_context(
            DiagnosticContext::new()
                .with_source("Contents/header.xml")
                .with_element(element)
                .with_attribute(attribute)
                .with_value(value)
                .with_component("hwpx::header"),
        ),
    );
}

fn record_skipped_header_image_ref(
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
    value: &str,
) {
    let message = format!("Skipped unsafe HWPX header image binaryItemIDRef: {value}");
    warnings.push(ParseWarning::recovered_error(message.clone()));
    diagnostics.push(
        DiagnosticItem::new(
            DiagnosticSeverity::DataLoss,
            DiagnosticCategory::SkippedBinary,
            message,
        )
        .with_context(
            DiagnosticContext::new()
                .with_source("Contents/header.xml")
                .with_element("hc:img")
                .with_attribute("binaryItemIDRef")
                .with_value(value)
                .with_component("hwpx::header"),
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{DiagnosticCategory, DiagnosticReport, DiagnosticSeverity};
    use std::io::{Cursor, Write};
    use zip::{write::SimpleFileOptions, ZipWriter};

    fn zip_with_file(path: &str, content: &str) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file(path, options).unwrap();
        zip.write_all(content.as_bytes()).unwrap();
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn invalid_char_height_records_diagnostic() {
        let xml = r##"
            <hh:head>
              <hh:charProperties>
                <hh:charPr height="huge" textColor="#000000"></hh:charPr>
              </hh:charProperties>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert!(diagnostics.items.iter().any(|item| {
            item.severity == DiagnosticSeverity::RecoveredError
                && item.category == DiagnosticCategory::InvalidValue
                && item.context.attribute.as_deref() == Some("height")
        }));
    }

    #[test]
    fn negative_char_height_records_diagnostic_and_defaults() {
        let xml = r##"
            <hh:head>
              <hh:charProperties>
                <hh:charPr height="-1" textColor="#000000"></hh:charPr>
              </hh:charProperties>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.char_shapes[0].base_size, 1000);
        assert!(diagnostics.items.iter().any(|item| {
            item.severity == DiagnosticSeverity::RecoveredError
                && item.category == DiagnosticCategory::InvalidValue
                && item.context.element.as_deref() == Some("hh:charPr")
                && item.context.attribute.as_deref() == Some("height")
                && item.context.value.as_deref() == Some("-1")
        }));
    }

    #[test]
    fn invalid_char_text_color_records_diagnostic() {
        let xml = r##"
            <hh:head>
              <hh:charProperties>
                <hh:charPr height="1000" textColor="#GGGGGG"></hh:charPr>
              </hh:charProperties>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert!(diagnostics.items.iter().any(|item| {
            item.severity == DiagnosticSeverity::RecoveredError
                && item.category == DiagnosticCategory::InvalidValue
                && item.context.element.as_deref() == Some("hh:charPr")
                && item.context.attribute.as_deref() == Some("textColor")
                && item.context.value.as_deref() == Some("#GGGGGG")
        }));
    }

    #[test]
    fn duplicate_header_attribute_is_rejected() {
        let xml = r##"
            <hh:head>
              <hh:charProperties>
                <hh:charPr height="1000" height="2000" textColor="#000000"></hh:charPr>
              </hh:charProperties>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err =
            parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
                .expect_err("duplicate header XML attributes should be rejected");

        assert!(matches!(
            err,
            HwpError::XmlParseError(message)
                if message.contains("attribute")
                    && message.contains("Contents/header.xml")
                    && message.contains("hh:charPr")
        ));
    }

    #[test]
    fn parse_header_rejects_non_head_root_element() {
        let xml = r##"
            <root>
              <hh:charProperties>
                <hh:charPr height="1000" textColor="#000000"></hh:charPr>
              </hh:charProperties>
            </root>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err =
            parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
                .expect_err("header.xml root element should be hh:head");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("HWPX header XML root element must be hh:head")
        ));
    }

    #[test]
    fn parse_header_rejects_multiple_head_roots() {
        let xml = r##"
            <hh:head>
              <hh:charProperties>
                <hh:charPr height="1000" textColor="#000000"></hh:charPr>
              </hh:charProperties>
            </hh:head>
            <hh:head>
              <hh:paraProperties>
                <hh:paraPr></hh:paraPr>
              </hh:paraProperties>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err =
            parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
                .expect_err("header.xml should contain exactly one hh:head root");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("HWPX header XML contains multiple root elements")
        ));
    }

    #[test]
    fn header_element_matching_uses_exact_local_names_not_suffixes() {
        let xml = r##"
            <hh:head>
              <hh:notcharProperties>
                <hh:notcharPr height="1000" textColor="#FF0000"></hh:notcharPr>
              </hh:notcharProperties>
              <hh:notparaProperties>
                <hh:notparaPr>
                  <hh:notalign horizontal="CENTER"/>
                </hh:notparaPr>
              </hh:notparaProperties>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .expect("unknown header elements should be ignored, not parsed by suffix");

        assert!(doc_info.char_shapes.is_empty());
        assert!(doc_info.para_shapes.is_empty());
        assert!(warnings.is_empty());
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn self_closing_char_properties_does_not_open_scope_for_sibling_char_pr() {
        let xml = r##"
            <hh:head>
              <hh:charProperties/>
              <hh:charPr height="1200" textColor="#000000"/>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert!(doc_info.char_shapes.is_empty());
        assert!(warnings.is_empty());
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn self_closing_para_properties_does_not_open_scope_for_sibling_para_pr() {
        let xml = r##"
            <hh:head>
              <hh:paraProperties/>
              <hh:paraPr/>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert!(doc_info.para_shapes.is_empty());
        assert!(warnings.is_empty());
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn rejects_header_xml_when_char_shape_count_exceeds_limit() {
        let xml = r##"
            <hh:head>
              <hh:charProperties>
                <hh:charPr height="1000" textColor="#000000"></hh:charPr>
                <hh:charPr height="1000" textColor="#000000"></hh:charPr>
              </hh:charProperties>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();
        let limits = HeaderStructureLimits {
            max_char_shapes: 1,
            ..Default::default()
        };

        let err = parse_header_xml_content_with_limits(
            &mut reader,
            &mut doc_info,
            &mut warnings,
            &mut diagnostics,
            limits,
        )
        .expect_err("header char shape count over budget should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX header char shape count"
                && path == "Contents/header.xml"
                && limit == 1
                && actual == 2
        ));
    }

    #[test]
    fn rejects_header_xml_when_para_shape_count_exceeds_limit() {
        let xml = r#"
            <hh:head>
              <hh:paraProperties>
                <hh:paraPr></hh:paraPr>
                <hh:paraPr></hh:paraPr>
              </hh:paraProperties>
            </hh:head>
        "#;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();
        let limits = HeaderStructureLimits {
            max_para_shapes: 1,
            ..Default::default()
        };

        let err = parse_header_xml_content_with_limits(
            &mut reader,
            &mut doc_info,
            &mut warnings,
            &mut diagnostics,
            limits,
        )
        .expect_err("header para shape count over budget should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX header paragraph shape count"
                && path == "Contents/header.xml"
                && limit == 1
                && actual == 2
        ));
    }

    #[test]
    fn char_height_numeric_attribute_entity_references_are_decoded() {
        let xml = r##"
            <hh:head>
              <hh:charProperties>
                <hh:charPr height="12&#48;0" textColor="#000000"></hh:charPr>
              </hh:charProperties>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.char_shapes[0].base_size, 1200);
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn self_closing_char_pr_is_stored() {
        let xml = r##"
            <hh:head>
              <hh:charProperties>
                <hh:charPr height="1200" textColor="#000000"/>
              </hh:charProperties>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.char_shapes.len(), 1);
        assert_eq!(doc_info.char_shapes[0].base_size, 1200);
    }

    #[test]
    fn char_pr_id_places_shape_at_referenced_index() {
        let xml = r##"
            <hh:head>
              <hh:charProperties>
                <hh:charPr id="3" height="1500" textColor="#000000"/>
              </hh:charProperties>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.char_shapes.len(), 4);
        assert_eq!(doc_info.char_shapes[3].base_size, 1500);
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn hangul_fontface_fonts_are_stored_by_id() {
        let xml = r##"
            <hh:head>
              <hh:refList>
                <hh:fontfaces>
                  <hh:fontface lang="LATIN">
                    <hh:font id="0" face="Latin Font"/>
                  </hh:fontface>
                  <hh:fontface lang="HANGUL">
                    <hh:font id="2" face="Custom Hangul"/>
                  </hh:fontface>
                </hh:fontfaces>
              </hh:refList>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.face_names.len(), 3);
        assert_eq!(doc_info.face_names[2].name, "Custom Hangul");
        assert!(!doc_info
            .face_names
            .iter()
            .any(|face_name| face_name.name == "Latin Font"));
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn style_id_places_style_at_referenced_index() {
        let xml = r##"
            <hh:head>
              <hh:styles>
                <hh:style id="3" type="PARA" name="개요 8" engName="Outline 8"
                  paraPrIDRef="12" charPrIDRef="5" nextStyleIDRef="3" langID="1042"/>
              </hh:styles>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.styles.len(), 4);
        assert_eq!(doc_info.styles[3].local_name, "개요 8");
        assert_eq!(doc_info.styles[3].english_name, "Outline 8");
        assert_eq!(doc_info.styles[3].para_shape_id, Some(12));
        assert_eq!(doc_info.styles[3].char_shape_id, Some(5));
        assert_eq!(doc_info.styles[3].next_style_id, 3);
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn tab_pr_id_places_tab_def_at_referenced_index() {
        let xml = r##"
            <hh:head>
              <hh:tabProperties>
                <hh:tabPr id="2" autoTabLeft="true" autoTabRight="false">
                  <hh:tabItem pos="7200" type="RIGHT" leader="DOT" unit="HWPUNIT"/>
                </hh:tabPr>
              </hh:tabProperties>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.tab_defs.len(), 3);
        assert!(doc_info.tab_defs[2].attributes.has_left_auto_tab);
        assert!(!doc_info.tab_defs[2].attributes.has_right_auto_tab);
        assert_eq!(doc_info.tab_defs[2].count, 1);
        assert_eq!(doc_info.tab_defs[2].tabs[0].position.value(), 7200);
        assert_eq!(
            doc_info.tab_defs[2].tabs[0].tab_type,
            crate::document::docinfo::tab_def::TabType::Right
        );
        assert_eq!(doc_info.tab_defs[2].tabs[0].fill_type, 3);
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn numbering_id_places_numbering_at_referenced_index() {
        let xml = r##"
            <hh:head>
              <hh:numberings>
                <hh:numbering id="2" start="0">
                  <hh:paraHead start="3" level="1" align="CENTER" useInstWidth="1"
                    autoIndent="0" widthAdjust="12" textOffsetType="HWPUNIT"
                    textOffset="50" charPrIDRef="7">^1.</hh:paraHead>
                  <hh:paraHead start="8" level="8" align="LEFT" useInstWidth="0"
                    autoIndent="0" widthAdjust="0" textOffsetType="PERCENT"
                    textOffset="0" charPrIDRef="0">^8</hh:paraHead>
                </hh:numbering>
              </hh:numberings>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.numbering.len(), 3);
        assert_eq!(doc_info.numbering[2].levels.len(), 1);
        let level = &doc_info.numbering[2].levels[0];
        assert_eq!(level.format_string, "^1.");
        assert_eq!(level.format_length, 3);
        assert_eq!(level.start_number, 3);
        assert_eq!(level.width, 12);
        assert_eq!(level.distance, 50);
        assert_eq!(level.char_shape_id, 7);
        assert_eq!(
            level.attributes.align_type,
            crate::document::docinfo::numbering::ParagraphAlignType::Center
        );
        assert!(level.attributes.instance_like);
        assert!(!level.attributes.auto_outdent);
        assert_eq!(
            level.attributes.distance_type,
            crate::document::docinfo::numbering::DistanceType::Value
        );
        assert_eq!(doc_info.numbering[2].extended_levels.len(), 1);
        assert_eq!(doc_info.numbering[2].extended_levels[0].format_string, "^8");
        assert_eq!(doc_info.numbering[2].extended_levels[0].format_length, 2);
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn numbering_format_text_decodes_xml_entities() {
        let xml = r##"
            <hh:head>
              <hh:numberings>
                <hh:numbering id="0" start="0">
                  <hh:paraHead start="1" level="1" align="LEFT" useInstWidth="0"
                    autoIndent="0" widthAdjust="0" textOffsetType="PERCENT"
                    textOffset="0" charPrIDRef="0">A &amp; B</hh:paraHead>
                </hh:numbering>
              </hh:numberings>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.numbering[0].levels[0].format_string, "A & B");
    }

    #[test]
    fn numbering_format_text_normalizes_xml_line_endings() {
        let xml = "<hh:head><hh:numberings><hh:numbering id=\"0\" start=\"0\"><hh:paraHead \
                   start=\"1\" level=\"1\" align=\"LEFT\" useInstWidth=\"0\" autoIndent=\"0\" \
                   widthAdjust=\"0\" textOffsetType=\"PERCENT\" textOffset=\"0\" \
                   charPrIDRef=\"0\">A\r\nB\rC</hh:paraHead></hh:numbering></hh:numberings></hh:head>";
        let mut reader = Reader::from_str(xml);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.numbering[0].levels[0].format_string, "A\nB\nC");
    }

    #[test]
    fn para_pr_heading_sets_header_shape_level_and_numbering_ref() {
        let xml = r##"
            <hh:head>
              <hh:paraProperties>
                <hh:paraPr id="2">
                  <hh:heading type="OUTLINE" idRef="1" level="3"/>
                </hh:paraPr>
              </hh:paraProperties>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.para_shapes.len(), 3);
        assert_eq!(
            doc_info.para_shapes[2].attributes1.header_shape_type,
            HeaderShapeType::Outline
        );
        assert_eq!(doc_info.para_shapes[2].attributes1.paragraph_level, 3);
        assert_eq!(doc_info.para_shapes[2].number_bullet_id, 1);
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn para_pr_preserves_tab_line_spacing_and_border_fields() {
        let xml = r##"
            <hh:head>
              <hh:paraProperties>
                <hh:paraPr id="2" tabPrIDRef="3">
                  <hh:lineSpacing type="FIXED" value="2400" unit="HWPUNIT"/>
                  <hh:border borderFillIDRef="4" offsetLeft="5" offsetRight="6"
                    offsetTop="7" offsetBottom="8" connect="1" ignoreMargin="1"/>
                </hh:paraPr>
              </hh:paraProperties>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.para_shapes.len(), 3);
        let para_shape = &doc_info.para_shapes[2];
        assert_eq!(para_shape.tab_def_id, 3);
        assert_eq!(para_shape.line_spacing, Some(2400));
        assert_eq!(
            para_shape
                .attributes3
                .as_ref()
                .expect("line spacing attributes should be present")
                .line_spacing_type,
            crate::document::docinfo::para_shape::LineSpacingType::Fixed
        );
        assert_eq!(para_shape.border_fill_id, 4);
        assert_eq!(para_shape.border_spacing_left, 5);
        assert_eq!(para_shape.border_spacing_right, 6);
        assert_eq!(para_shape.border_spacing_top, 7);
        assert_eq!(para_shape.border_spacing_bottom, 8);
        assert!(para_shape.attributes1.connect_border);
        assert!(para_shape.attributes1.ignore_margin);
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn border_fill_id_places_border_fill_at_referenced_index() {
        let xml = r##"
            <hh:head>
              <hh:borderFills>
                <hh:borderFill id="2" threeD="1" shadow="1" centerLine="SOLID">
                  <hh:leftBorder type="SOLID" width="0.12 mm" color="#112233"/>
                  <hh:rightBorder type="NONE" width="0.1 mm" color="#000000"/>
                  <hh:topBorder type="DASH" width="0.2 mm" color="#445566"/>
                  <hh:bottomBorder type="DOT" width="0.3 mm" color="#778899"/>
                  <hh:diagonal type="SOLID" width="0.12 mm" color="#AABBCC"/>
                  <hc:fillBrush>
                    <hc:winBrush faceColor="#010203" hatchColor="#FF040506"/>
                  </hc:fillBrush>
                </hh:borderFill>
              </hh:borderFills>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.border_fill.len(), 2);
        let border_fill = &doc_info.border_fill[1];
        assert!(border_fill.attributes.has_3d_effect);
        assert!(border_fill.attributes.has_shadow);
        assert!(border_fill.attributes.has_center_line);
        assert_eq!(border_fill.borders[0].line_type, 1);
        assert_eq!(border_fill.borders[0].width, 1);
        assert_eq!(
            border_fill.borders[0].color,
            COLORREF::rgb(0x11, 0x22, 0x33)
        );
        assert_eq!(border_fill.borders[1].line_type, 0);
        assert_eq!(border_fill.borders[2].line_type, 2);
        assert_eq!(border_fill.borders[2].width, 3);
        assert_eq!(border_fill.borders[3].line_type, 3);
        assert_eq!(border_fill.borders[3].width, 5);
        assert_eq!(border_fill.diagonal.line_type, 1);
        assert_eq!(border_fill.diagonal.thickness, 1);
        assert_eq!(border_fill.diagonal.color, COLORREF::rgb(0xAA, 0xBB, 0xCC));
        match &border_fill.fill {
            crate::document::docinfo::border_fill::FillInfo::Solid(solid) => {
                assert_eq!(solid.background_color, COLORREF::rgb(0x01, 0x02, 0x03));
                assert_eq!(solid.pattern_color, COLORREF::rgb(0x04, 0x05, 0x06));
                assert_eq!(solid.pattern_type, 0);
            }
            fill => panic!("expected solid fill, got {fill:?}"),
        }
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn border_fill_preserves_solid_fill_alpha() {
        let xml = r##"
            <hh:head>
              <hh:borderFills>
                <hh:borderFill id="1">
                  <hc:fillBrush>
                    <hc:winBrush faceColor="#112233" hatchColor="#445566" alpha="128"/>
                  </hc:fillBrush>
                </hh:borderFill>
              </hh:borderFills>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.border_fill.len(), 1);
        let fill = serde_json::to_value(&doc_info.border_fill[0].fill)
            .expect("solid fill should serialize");
        assert_eq!(fill["alpha"], 128);
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn border_fill_preserves_slash_and_backslash_diagonal_attributes() {
        let xml = r##"
            <hh:head>
              <hh:borderFills>
                <hh:borderFill id="1">
                  <hh:slash type="DASH_DOT" Crooked="1" isCounter="1"/>
                  <hh:backSlash type="LONG_DASH" Crooked="1" isCounter="0"/>
                </hh:borderFill>
              </hh:borderFills>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.border_fill.len(), 1);
        let attributes = &doc_info.border_fill[0].attributes;
        assert_eq!(attributes.slash_shape, 4);
        assert_eq!(attributes.backslash_shape, 6);
        assert_eq!(attributes.slash_broken_line, 1);
        assert!(attributes.backslash_broken_line);
        assert!(attributes.slash_rotated_180);
        assert!(!attributes.backslash_rotated_180);
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn border_fill_preserves_gradient_fill_attributes_and_colors() {
        let xml = r##"
            <hh:head>
              <hh:borderFills>
                <hh:borderFill id="1">
                  <hc:fillBrush>
                    <hc:gradation type="LINEAR" angle="90" centerX="10" centerY="20"
                      step="30" colorNum="2">
                      <hc:color value="#112233"/>
                      <hc:color value="#AABBCC"/>
                    </hc:gradation>
                  </hc:fillBrush>
                </hh:borderFill>
              </hh:borderFills>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.border_fill.len(), 1);
        match &doc_info.border_fill[0].fill {
            crate::document::docinfo::border_fill::FillInfo::Gradient(gradient) => {
                assert_eq!(gradient.gradient_type, 1);
                assert_eq!(gradient.angle, 90);
                assert_eq!(gradient.horizontal_center, 10);
                assert_eq!(gradient.vertical_center, 20);
                assert_eq!(gradient.spread, 30);
                assert_eq!(gradient.color_count, 2);
                assert_eq!(gradient.positions, None);
                assert_eq!(
                    gradient.colors,
                    vec![
                        COLORREF::rgb(0x11, 0x22, 0x33),
                        COLORREF::rgb(0xAA, 0xBB, 0xCC)
                    ]
                );
            }
            fill => panic!("expected gradient fill, got {fill:?}"),
        }
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn border_fill_preserves_gradient_step_center_and_alpha() {
        let xml = r##"
            <hh:head>
              <hh:borderFills>
                <hh:borderFill id="1">
                  <hc:fillBrush>
                    <hc:gradation type="LINEAR" stepCenter="50" alpha="128" colorNum="2">
                      <hc:color value="#112233"/>
                      <hc:color value="#445566"/>
                    </hc:gradation>
                  </hc:fillBrush>
                </hh:borderFill>
              </hh:borderFills>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.border_fill.len(), 1);
        let fill = serde_json::to_value(&doc_info.border_fill[0].fill)
            .expect("gradient fill should serialize");
        assert_eq!(fill["step_center"], 50);
        assert_eq!(fill["alpha"], 128);
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn invalid_gradient_color_num_records_diagnostic_and_defaults() {
        let xml = r##"
            <hh:head>
              <hh:borderFills>
                <hh:borderFill id="1">
                  <hc:fillBrush>
                    <hc:gradation type="LINEAR" colorNum="-1"/>
                  </hc:fillBrush>
                </hh:borderFill>
              </hh:borderFills>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.border_fill.len(), 1);
        match &doc_info.border_fill[0].fill {
            crate::document::docinfo::border_fill::FillInfo::Gradient(gradient) => {
                assert_eq!(gradient.color_count, 0);
                assert!(gradient.colors.is_empty());
            }
            fill => panic!("expected gradient fill, got {fill:?}"),
        }
        assert!(diagnostics.items.iter().any(|item| {
            item.severity == DiagnosticSeverity::RecoveredError
                && item.category == DiagnosticCategory::InvalidValue
                && item.context.element.as_deref() == Some("hc:gradation")
                && item.context.attribute.as_deref() == Some("colorNum")
                && item.context.value.as_deref() == Some("-1")
        }));
    }

    #[test]
    fn gradient_color_count_mismatch_records_diagnostic_and_uses_actual_colors() {
        let xml = r##"
            <hh:head>
              <hh:borderFills>
                <hh:borderFill id="1">
                  <hc:fillBrush>
                    <hc:gradation type="LINEAR" colorNum="3">
                      <hc:color value="#112233"/>
                    </hc:gradation>
                  </hc:fillBrush>
                </hh:borderFill>
              </hh:borderFills>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.border_fill.len(), 1);
        match &doc_info.border_fill[0].fill {
            crate::document::docinfo::border_fill::FillInfo::Gradient(gradient) => {
                assert_eq!(gradient.color_count, 1);
                assert_eq!(gradient.colors, vec![COLORREF::rgb(0x11, 0x22, 0x33)]);
            }
            fill => panic!("expected gradient fill, got {fill:?}"),
        }
        assert!(diagnostics.items.iter().any(|item| {
            item.severity == DiagnosticSeverity::RecoveredError
                && item.category == DiagnosticCategory::InvalidValue
                && item.context.element.as_deref() == Some("hc:gradation")
                && item.context.attribute.as_deref() == Some("colorNum")
                && item.context.value.as_deref() == Some("3")
        }));
    }

    #[test]
    fn border_fill_preserves_image_fill_attributes_and_binary_ref() {
        let xml = r##"
            <hh:head>
              <hh:borderFills>
                <hh:borderFill id="1">
                  <hc:fillBrush>
                    <hc:imgBrush mode="TILE">
                      <hc:img binaryItemIDRef="BinData/image1.jpg" bright="-10"
                        contrast="20" effect="GRAY_SCALE" alpha="128"/>
                    </hc:imgBrush>
                  </hc:fillBrush>
                </hh:borderFill>
              </hh:borderFills>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.border_fill.len(), 1);
        match &doc_info.border_fill[0].fill {
            FillInfo::Image(image_fill) => {
                assert_eq!(image_fill.mode.as_deref(), Some("TILE"));
                assert_eq!(image_fill.binary_item_ref.as_deref(), Some("image1"));
                assert_eq!(image_fill.brightness, Some(-10));
                assert_eq!(image_fill.contrast, Some(20));
                assert_eq!(image_fill.effect.as_deref(), Some("GRAY_SCALE"));
                assert_eq!(image_fill.alpha, Some(128));
            }
            fill => panic!("expected image fill, got {fill:?}"),
        }
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn unsafe_border_fill_image_refs_are_skipped_with_diagnostic() {
        let xml = r##"
            <hh:head>
              <hh:borderFills>
                <hh:borderFill id="1">
                  <hc:fillBrush>
                    <hc:imgBrush mode="TILE">
                      <hc:img binaryItemIDRef="../secret.png"/>
                    </hc:imgBrush>
                  </hc:fillBrush>
                </hh:borderFill>
              </hh:borderFills>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.border_fill.len(), 1);
        match &doc_info.border_fill[0].fill {
            FillInfo::Image(image_fill) => {
                assert_eq!(image_fill.mode.as_deref(), Some("TILE"));
                assert_eq!(image_fill.binary_item_ref, None);
            }
            fill => panic!("expected image fill, got {fill:?}"),
        }
        assert!(diagnostics.items.iter().any(|item| {
            item.severity == DiagnosticSeverity::DataLoss
                && item.category == DiagnosticCategory::SkippedBinary
                && item.context.source.as_deref() == Some("Contents/header.xml")
                && item.context.element.as_deref() == Some("hc:img")
                && item.context.attribute.as_deref() == Some("binaryItemIDRef")
                && item.context.value.as_deref() == Some("../secret.png")
        }));
    }

    #[test]
    fn para_pr_preserves_break_setting_and_auto_spacing_attributes() {
        let xml = r##"
            <hh:head>
              <hh:paraProperties>
                <hh:paraPr id="1" condense="20" fontLineHeight="1" snapToGrid="1">
                  <hh:breakSetting breakLatinWord="BREAK_WORD" breakNonLatinWord="BREAK_WORD"
                    widowOrphan="1" keepWithNext="1" keepLines="1" pageBreakBefore="1"/>
                  <hh:autoSpacing eAsianEng="1" eAsianNum="1"/>
                </hh:paraPr>
              </hh:paraProperties>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.para_shapes.len(), 2);
        let para_shape = &doc_info.para_shapes[1];
        assert_eq!(para_shape.attributes1.blank_min_value, 20);
        assert!(para_shape.attributes1.line_height_matches_font);
        assert!(para_shape.attributes1.use_line_grid);
        assert_eq!(
            para_shape.attributes1.line_divide_en,
            LineDivideUnit::Character
        );
        assert_eq!(
            para_shape.attributes1.line_divide_ko,
            LineDivideUnit::Character
        );
        assert!(para_shape.attributes1.protect_orphan_line);
        assert!(para_shape.attributes1.with_next_paragraph);
        assert!(para_shape.attributes1.protect_paragraph);
        assert!(para_shape.attributes1.always_page_break_before);
        let attributes2 = para_shape
            .attributes2
            .as_ref()
            .expect("auto spacing attributes should be present");
        assert_eq!(attributes2.single_line_input, 0);
        assert!(attributes2.auto_spacing_ko_en);
        assert!(attributes2.auto_spacing_ko_num);
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn self_closing_para_pr_is_stored() {
        let xml = r##"
            <hh:head>
              <hh:paraProperties>
                <hh:paraPr/>
              </hh:paraProperties>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.para_shapes.len(), 1);
        assert_eq!(
            doc_info.para_shapes[0].attributes1.align,
            ParagraphAlignment::Justify
        );
    }

    #[test]
    fn para_pr_id_places_shape_at_referenced_index() {
        let xml = r##"
            <hh:head>
              <hh:paraProperties>
                <hh:paraPr id="3">
                  <hh:align horizontal="CENTER"/>
                </hh:paraPr>
              </hh:paraProperties>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.para_shapes.len(), 4);
        assert_eq!(
            doc_info.para_shapes[3].attributes1.align,
            ParagraphAlignment::Center
        );
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn malformed_header_numeric_attribute_entity_is_rejected() {
        let xml = r##"
            <hh:head>
              <hh:charProperties>
                <hh:charPr height="12&unknown_entity;0" textColor="#000000"></hh:charPr>
              </hh:charProperties>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err =
            parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
                .expect_err("malformed numeric attribute entities should be rejected");

        assert!(matches!(
            err,
            HwpError::XmlParseError(message)
                if message.contains("Error decoding XML attribute height")
                    && message.contains("Contents/header.xml")
                    && message.contains("hh:charPr")
                    && message.contains("unknown_entity")
        ));
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn char_text_color_attribute_entity_references_are_decoded() {
        let xml = r##"
            <hh:head>
              <hh:charProperties>
                <hh:charPr height="1000" textColor="#FF&#48;000"></hh:charPr>
              </hh:charProperties>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.char_shapes[0].text_color, COLORREF::rgb(255, 0, 0));
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn invalid_header_enum_values_record_diagnostics() {
        let xml = r##"
            <hh:head>
              <hh:charProperties>
                <hh:charPr height="1000" textColor="#000000">
                  <hh:underline type="SIDEWAYS"/>
                  <hh:strikeout shape="BLINK"/>
                </hh:charPr>
              </hh:charProperties>
              <hh:paraProperties>
                <hh:paraPr>
                  <hh:align horizontal="DIAGONAL"/>
                </hh:paraPr>
              </hh:paraProperties>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        for (element, attribute, value) in [
            ("hh:underline", "type", "SIDEWAYS"),
            ("hh:strikeout", "shape", "BLINK"),
            ("hh:align", "horizontal", "DIAGONAL"),
        ] {
            assert!(
                diagnostics.items.iter().any(|item| {
                    item.severity == DiagnosticSeverity::RecoveredError
                        && item.category == DiagnosticCategory::InvalidValue
                        && item.context.element.as_deref() == Some(element)
                        && item.context.attribute.as_deref() == Some(attribute)
                        && item.context.value.as_deref() == Some(value)
                }),
                "expected diagnostic for {element} {attribute}={value}"
            );
        }
    }

    #[test]
    fn strikeout_3d_shape_is_accepted_without_diagnostic() {
        let xml = r##"
            <hh:head>
              <hh:charProperties>
                <hh:charPr height="1000" textColor="#000000">
                  <hh:strikeout shape="3D"/>
                </hh:charPr>
              </hh:charProperties>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.char_shapes[0].attributes.strikethrough, 1);
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn header_enum_string_attribute_entity_references_are_decoded() {
        let xml = r##"
            <hh:head>
              <hh:charProperties>
                <hh:charPr height="1000" textColor="#000000">
                  <hh:underline type="BOT&#84;OM"/>
                  <hh:strikeout shape="NO&#78;E"/>
                </hh:charPr>
              </hh:charProperties>
              <hh:paraProperties>
                <hh:paraPr>
                  <hh:align horizontal="CENTE&#82;"/>
                </hh:paraPr>
              </hh:paraProperties>
            </hh:head>
        "##;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.char_shapes[0].attributes.underline_type, 1);
        assert_eq!(doc_info.char_shapes[0].attributes.strikethrough, 0);
        assert_eq!(
            doc_info.para_shapes[0].attributes1.align,
            ParagraphAlignment::Center
        );
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn margin_numeric_attribute_entity_references_are_decoded() {
        let xml = r#"
            <hh:head>
              <hh:paraProperties>
                <hh:paraPr>
                  <hh:margin>
                    <hc:left value="7&#50;00" unit="HWPUNIT"/>
                  </hh:margin>
                </hh:paraPr>
              </hh:paraProperties>
            </hh:head>
        "#;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.para_shapes[0].left_margin, 1800);
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn margin_prev_and_next_set_top_and_bottom_spacing() {
        let xml = r#"
            <hh:head>
              <hh:paraProperties>
                <hh:paraPr>
                  <hh:margin>
                    <hc:prev value="800" unit="HWPUNIT"/>
                    <hc:next value="1200" unit="HWPUNIT"/>
                  </hh:margin>
                </hh:paraPr>
              </hh:paraProperties>
            </hh:head>
        "#;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
            .unwrap();

        assert_eq!(doc_info.para_shapes[0].top_spacing, 200);
        assert_eq!(doc_info.para_shapes[0].bottom_spacing, 300);
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn rejects_header_xml_with_excessive_nesting_depth() {
        let mut xml = String::from("<hh:head>");
        for _ in 0..300 {
            xml.push_str("<hh:group>");
        }
        for _ in 0..300 {
            xml.push_str("</hh:group>");
        }
        xml.push_str("</hh:head>");

        let mut reader = Reader::from_str(&xml);
        reader.config_mut().trim_text(true);
        let mut doc_info = DocInfo::default();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err =
            parse_header_xml_content(&mut reader, &mut doc_info, &mut warnings, &mut diagnostics)
                .expect_err("header XML with excessive nesting should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX XML nesting depth"
                && path == "Contents/header.xml"
                && actual == limit + 1
        ));
    }

    #[test]
    fn file_header_propagates_version_xml_resource_limit() {
        let mut version_xml = String::from("<opf:version>");
        for _ in 0..300 {
            version_xml.push_str("<opf:group>");
        }
        for _ in 0..300 {
            version_xml.push_str("</opf:group>");
        }
        version_xml.push_str("</opf:version>");

        let data = zip_with_file("version.xml", &version_xml);
        let mut container = HwpxContainer::open(&data).unwrap();

        let err = parse_file_header(&mut container)
            .expect_err("version.xml resource limit errors should not be swallowed");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX XML nesting depth"
                && path == "version.xml"
                && actual == limit + 1
        ));
    }

    #[test]
    fn file_header_rejects_oversized_version_xml_before_parsing() {
        let version_xml = "x".repeat((1024 * 1024) + 1);
        let data = zip_with_file("version.xml", &version_xml);
        let mut container = HwpxContainer::open(&data).unwrap();

        let err = parse_file_header(&mut container)
            .expect_err("oversized version.xml should be rejected before XML parsing");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX version XML byte size"
                && path == "version.xml"
                && limit == 1024 * 1024
                && actual == (1024 * 1024) + 1
        ));
    }

    #[test]
    fn file_header_rejects_version_major_outside_byte_range() {
        let data = zip_with_file("version.xml", r#"<opf:version major="256"/>"#);
        let mut container = HwpxContainer::open(&data).unwrap();

        let err = parse_file_header(&mut container)
            .expect_err("version.xml major values outside one byte should be rejected");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("version.xml")
                    && reason.contains("major")
                    && reason.contains("256")
        ));
    }

    #[test]
    fn file_header_rejects_invalid_version_major_value() {
        let data = zip_with_file("version.xml", r#"<opf:version major="not-a-number"/>"#);
        let mut container = HwpxContainer::open(&data).unwrap();

        let err =
            parse_file_header(&mut container).expect_err("invalid version.xml major should fail");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("version.xml")
                    && reason.contains("major")
                    && reason.contains("not-a-number")
        ));
    }

    #[test]
    fn file_header_accepts_hcf_version_root() {
        let data = zip_with_file(
            "version.xml",
            r#"<hv:HCFVersion xmlns:hv="http://www.hancom.co.kr/hwpml/2011/version" major="5"/>"#,
        );
        let mut container = HwpxContainer::open(&data).unwrap();

        let header =
            parse_file_header(&mut container).expect("HWPX HCFVersion root should be accepted");

        assert_eq!(header.version, 0x05010000);
    }

    #[test]
    fn file_header_rejects_non_version_xml_root() {
        let data = zip_with_file("version.xml", r#"<root><opf:version major="5"/></root>"#);
        let mut container = HwpxContainer::open(&data).unwrap();

        let err = parse_file_header(&mut container)
            .expect_err("version.xml root element should be opf:version");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("version.xml root element must be")
        ));
    }

    #[test]
    fn file_header_rejects_multiple_version_roots() {
        let data = zip_with_file(
            "version.xml",
            r#"<opf:version major="5"/><opf:version major="6"/>"#,
        );
        let mut container = HwpxContainer::open(&data).unwrap();

        let err = parse_file_header(&mut container)
            .expect_err("version.xml should contain exactly one opf:version root");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("version.xml contains multiple root elements")
        ));
    }

    #[test]
    fn doc_info_rejects_oversized_header_xml_before_parsing() {
        let header_xml = "x".repeat((16 * 1024 * 1024) + 1);
        let data = zip_with_file("Contents/header.xml", &header_xml);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_doc_info(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("oversized header.xml should be rejected before XML parsing");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX header XML byte size"
                && path == "Contents/header.xml"
                && limit == 16 * 1024 * 1024
                && actual == (16 * 1024 * 1024) + 1
        ));
    }
}
