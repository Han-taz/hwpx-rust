use std::collections::BTreeMap;

use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::Reader;

use crate::error::HwpError;

use super::container::{extract_section_number, HwpxContainer};
use super::xml_attr::{for_each_xml_attribute, parse_string_attr};
use super::xml_budget::XmlParseBudget;

const CONTENT_HPF_PATH: &str = "Contents/content.hpf";
pub(crate) const MAX_HWPX_CONTENT_HPF_SIZE: u64 = 4 * 1024 * 1024;
pub(crate) const MAX_HWPX_CONTENT_HPF_MANIFEST_ITEMS: u64 = 8_192;
pub(crate) const MAX_HWPX_CONTENT_HPF_SPINE_ITEMREFS: u64 = 8_192;
const MAX_HWPX_CONTENT_HPF_REF_BYTES: usize = 1024;

#[derive(Debug, Default)]
struct PackageStructureBudget {
    manifest_item_count: u64,
    spine_itemref_count: u64,
}

#[derive(Debug)]
struct ManifestItem {
    href: String,
    media_type: Option<String>,
}

impl PackageStructureBudget {
    fn add_manifest_item(&mut self) -> Result<(), HwpError> {
        self.manifest_item_count = self.manifest_item_count.saturating_add(1);
        if self.manifest_item_count > MAX_HWPX_CONTENT_HPF_MANIFEST_ITEMS {
            return Err(HwpError::ResourceLimitExceeded {
                resource: "HWPX content.hpf manifest item count",
                path: CONTENT_HPF_PATH.to_string(),
                limit: MAX_HWPX_CONTENT_HPF_MANIFEST_ITEMS,
                actual: self.manifest_item_count,
            });
        }

        Ok(())
    }

    fn add_spine_itemref(&mut self) -> Result<(), HwpError> {
        self.spine_itemref_count = self.spine_itemref_count.saturating_add(1);
        if self.spine_itemref_count > MAX_HWPX_CONTENT_HPF_SPINE_ITEMREFS {
            return Err(HwpError::ResourceLimitExceeded {
                resource: "HWPX content.hpf spine itemref count",
                path: CONTENT_HPF_PATH.to_string(),
                limit: MAX_HWPX_CONTENT_HPF_SPINE_ITEMREFS,
                actual: self.spine_itemref_count,
            });
        }

        Ok(())
    }
}

pub(crate) fn package_section_file_entries(
    container: &mut HwpxContainer<'_>,
) -> Result<Option<Vec<(usize, String)>>, HwpError> {
    if !container.file_exists(CONTENT_HPF_PATH) {
        return Ok(None);
    }

    let content = container.read_file_string_with_limit(
        CONTENT_HPF_PATH,
        MAX_HWPX_CONTENT_HPF_SIZE,
        "HWPX content.hpf byte size",
    )?;

    parse_content_hpf_section_file_entries(&content, container).map(Some)
}

fn parse_content_hpf_section_file_entries(
    content: &str,
    container: &HwpxContainer<'_>,
) -> Result<Vec<(usize, String)>, HwpError> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);
    let mut xml_budget = XmlParseBudget::new(CONTENT_HPF_PATH);
    let mut manifest = BTreeMap::new();
    let mut spine_idrefs = Vec::new();
    let mut spine_seen_idrefs = std::collections::BTreeSet::new();
    let mut duplicate_spine_idref = None;
    let mut structure_budget = PackageStructureBudget::default();
    let mut manifest_depth = None;
    let mut spine_depth = None;
    let mut element_depth = 0usize;
    let mut package_depth = None;
    let mut package_seen = false;
    let mut manifest_seen = false;
    let mut spine_seen = false;

    loop {
        let event = reader
            .read_event()
            .map_err(|err| HwpError::XmlParseError(format!("{CONTENT_HPF_PATH}: {err}")))?;
        xml_budget.observe_event(&event)?;

        match event {
            Event::Start(ref e) if is_element(e, b"package") => {
                if package_seen {
                    return Err(HwpError::InvalidHwpxStructure {
                        reason: "content.hpf contains multiple package root elements".to_string(),
                    });
                }
                if element_depth != 0 {
                    return Err(HwpError::InvalidHwpxStructure {
                        reason: "content.hpf package element must be the document root".to_string(),
                    });
                }

                element_depth = element_depth.saturating_add(1);
                package_depth = Some(element_depth);
                package_seen = true;
            }
            Event::Start(ref e)
                if is_direct_child(package_depth, element_depth, e, b"manifest") =>
            {
                element_depth = element_depth.saturating_add(1);
                mark_package_child_seen(&mut manifest_seen, "manifest")?;
                manifest_depth = Some(element_depth);
            }
            Event::Start(ref e) if is_direct_child(package_depth, element_depth, e, b"spine") => {
                element_depth = element_depth.saturating_add(1);
                mark_package_child_seen(&mut spine_seen, "spine")?;
                spine_depth = Some(element_depth);
            }
            Event::Empty(ref e)
                if is_direct_child(package_depth, element_depth, e, b"manifest") =>
            {
                mark_package_child_seen(&mut manifest_seen, "manifest")?;
            }
            Event::Empty(ref e) if is_direct_child(package_depth, element_depth, e, b"spine") => {
                mark_package_child_seen(&mut spine_seen, "spine")?;
            }
            Event::Start(ref e) if is_direct_child(manifest_depth, element_depth, e, b"item") => {
                element_depth = element_depth.saturating_add(1);
                structure_budget.add_manifest_item()?;
                let (id, item) = parse_manifest_item(e)?;
                if manifest.insert(id.clone(), item).is_some() {
                    return Err(HwpError::InvalidHwpxStructure {
                        reason: format!("Duplicate content.hpf manifest item id: {id}"),
                    });
                }
            }
            Event::Empty(ref e) if is_direct_child(manifest_depth, element_depth, e, b"item") => {
                structure_budget.add_manifest_item()?;
                let (id, item) = parse_manifest_item(e)?;
                if manifest.insert(id.clone(), item).is_some() {
                    return Err(HwpError::InvalidHwpxStructure {
                        reason: format!("Duplicate content.hpf manifest item id: {id}"),
                    });
                }
            }
            Event::Start(ref e) if is_direct_child(spine_depth, element_depth, e, b"itemref") => {
                element_depth = element_depth.saturating_add(1);
                structure_budget.add_spine_itemref()?;
                if let Some(idref) = parse_spine_itemref(e)? {
                    record_spine_idref(&mut spine_seen_idrefs, &mut duplicate_spine_idref, &idref);
                    spine_idrefs.push(idref);
                }
            }
            Event::Empty(ref e) if is_direct_child(spine_depth, element_depth, e, b"itemref") => {
                structure_budget.add_spine_itemref()?;
                if let Some(idref) = parse_spine_itemref(e)? {
                    record_spine_idref(&mut spine_seen_idrefs, &mut duplicate_spine_idref, &idref);
                    spine_idrefs.push(idref);
                }
            }
            Event::Start(_) => {
                if element_depth == 0 {
                    return Err(HwpError::InvalidHwpxStructure {
                        reason: "content.hpf root element must be package".to_string(),
                    });
                }
                element_depth = element_depth.saturating_add(1);
            }
            Event::Empty(ref e) if is_element(e, b"package") => {
                if package_seen {
                    return Err(HwpError::InvalidHwpxStructure {
                        reason: "content.hpf contains multiple package root elements".to_string(),
                    });
                }
                if element_depth != 0 {
                    return Err(HwpError::InvalidHwpxStructure {
                        reason: "content.hpf package element must be the document root".to_string(),
                    });
                }

                package_seen = true;
            }
            Event::Empty(_) if element_depth == 0 => {
                return Err(HwpError::InvalidHwpxStructure {
                    reason: "content.hpf root element must be package".to_string(),
                });
            }
            Event::End(ref e) => {
                if is_end_element(e, b"manifest") && manifest_depth == Some(element_depth) {
                    manifest_depth = None;
                } else if is_end_element(e, b"spine") && spine_depth == Some(element_depth) {
                    spine_depth = None;
                } else if is_end_element(e, b"package") && package_depth == Some(element_depth) {
                    package_depth = None;
                }
                xml_budget.finish_end_event(e)?;
                element_depth = element_depth.saturating_sub(1);
            }
            Event::Eof => break,
            _ => {}
        }
    }

    if !package_seen {
        return Err(HwpError::InvalidHwpxStructure {
            reason: "content.hpf root element must be package".to_string(),
        });
    }

    if let Some(idref) = duplicate_spine_idref {
        return Err(HwpError::InvalidHwpxStructure {
            reason: format!("Duplicate content.hpf spine itemref idref: {idref}"),
        });
    }

    let mut section_files = Vec::new();
    let mut seen_section_paths = BTreeMap::new();
    for idref in spine_idrefs {
        let item = manifest
            .get(&idref)
            .ok_or_else(|| HwpError::InvalidHwpxStructure {
                reason: format!(
                    "content.hpf spine itemref references unknown manifest id: {idref}"
                ),
            })?;

        if let Some((index, path)) = package_section_entry_from_href(&item.href)? {
            if let Some(previous_idref) = seen_section_paths.insert(path.clone(), idref.clone()) {
                return Err(HwpError::InvalidHwpxStructure {
                    reason: format!(
                        "Duplicate content.hpf section href: {path} referenced by {previous_idref} and {idref}"
                    ),
                });
            }
            validate_section_media_type(&path, item.media_type.as_deref())?;
            if !container.file_exists(&path) {
                return Err(HwpError::InvalidHwpxStructure {
                    reason: format!("content.hpf references missing section file: {path}"),
                });
            }
            section_files.push((index, path));
        }
    }

    validate_package_lists_all_archive_sections(&section_files, container)?;

    Ok(section_files)
}

fn validate_package_lists_all_archive_sections(
    package_sections: &[(usize, String)],
    container: &HwpxContainer<'_>,
) -> Result<(), HwpError> {
    let listed_sections = package_sections
        .iter()
        .map(|(_, path)| path.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    for (_, archive_section) in container.get_section_file_entries() {
        if !listed_sections.contains(archive_section.as_str()) {
            return Err(HwpError::InvalidHwpxStructure {
                reason: format!("content.hpf does not list HWPX section file: {archive_section}"),
            });
        }
    }

    Ok(())
}

fn record_spine_idref(
    seen_idrefs: &mut std::collections::BTreeSet<String>,
    duplicate_idref: &mut Option<String>,
    idref: &str,
) {
    if !seen_idrefs.insert(idref.to_string()) && duplicate_idref.is_none() {
        *duplicate_idref = Some(idref.to_string());
    }
}

fn mark_package_child_seen(seen: &mut bool, child_name: &str) -> Result<(), HwpError> {
    if *seen {
        return Err(HwpError::InvalidHwpxStructure {
            reason: format!("content.hpf contains multiple {child_name} elements"),
        });
    }

    *seen = true;
    Ok(())
}

fn package_section_entry_from_href(href: &str) -> Result<Option<(usize, String)>, HwpError> {
    if let Some(index) = extract_section_number(href) {
        return Ok(Some((index, href.to_string())));
    }

    if href.contains('/') || href.contains('\\') {
        if is_section_like_href(href) {
            return Err(HwpError::InvalidHwpxStructure {
                reason: format!("Unsafe content.hpf section href: {href}"),
            });
        }
        return Ok(None);
    }

    let path = format!("Contents/{href}");
    if let Some(index) = extract_section_number(&path) {
        return Ok(Some((index, path)));
    }

    if is_section_like_href(&path) {
        return Err(HwpError::InvalidHwpxStructure {
            reason: format!("Invalid content.hpf section href: {href}"),
        });
    }

    Ok(None)
}

fn is_section_like_href(href: &str) -> bool {
    let filename = href.rsplit(['/', '\\']).next().unwrap_or(href);
    filename.starts_with("section") && filename.ends_with(".xml")
}

fn parse_manifest_item(e: &BytesStart<'_>) -> Result<(String, ManifestItem), HwpError> {
    let mut id = None;
    let mut href = None;
    let mut media_type = None;

    for_each_xml_attribute(CONTENT_HPF_PATH, e, |attr| {
        match attr.key.as_ref() {
            b"id" => {
                id = Some(parse_string_attr(
                    CONTENT_HPF_PATH,
                    "opf:item",
                    "id",
                    &attr,
                )?);
            }
            b"href" => {
                href = Some(parse_string_attr(
                    CONTENT_HPF_PATH,
                    "opf:item",
                    "href",
                    &attr,
                )?);
            }
            b"media-type" => {
                media_type = Some(parse_string_attr(
                    CONTENT_HPF_PATH,
                    "opf:item",
                    "media-type",
                    &attr,
                )?);
            }
            _ => {}
        }
        Ok(())
    })?;

    let id = id.ok_or_else(|| HwpError::InvalidHwpxStructure {
        reason: "content.hpf manifest item missing required id".to_string(),
    })?;
    let href = href.ok_or_else(|| HwpError::InvalidHwpxStructure {
        reason: "content.hpf manifest item missing required href".to_string(),
    })?;
    validate_package_ref("content.hpf manifest item id", &id)?;
    validate_package_ref("content.hpf manifest item href", &href)?;
    validate_manifest_href(&href)?;

    Ok((id, ManifestItem { href, media_type }))
}

fn validate_package_ref(context: &str, value: &str) -> Result<(), HwpError> {
    if value.is_empty() {
        return Err(HwpError::InvalidHwpxStructure {
            reason: format!("Invalid {context}: empty value"),
        });
    }

    if value.len() > MAX_HWPX_CONTENT_HPF_REF_BYTES {
        return Err(HwpError::ResourceLimitExceeded {
            resource: "HWPX content.hpf reference bytes",
            path: CONTENT_HPF_PATH.to_string(),
            limit: MAX_HWPX_CONTENT_HPF_REF_BYTES as u64,
            actual: value.len() as u64,
        });
    }

    if value.chars().any(char::is_whitespace) {
        return Err(HwpError::InvalidHwpxStructure {
            reason: format!("Invalid {context}: whitespace is not allowed"),
        });
    }

    if value.chars().any(char::is_control) {
        return Err(HwpError::InvalidHwpxStructure {
            reason: format!("Invalid {context}: control characters are not allowed"),
        });
    }

    Ok(())
}

fn validate_manifest_href(href: &str) -> Result<(), HwpError> {
    let is_safe = !href.is_empty()
        && !href.starts_with('/')
        && !href.contains('\\')
        && !href.contains(':')
        && !href.chars().any(char::is_control)
        && href
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..");

    if !is_safe {
        return Err(HwpError::InvalidHwpxStructure {
            reason: format!("Unsafe content.hpf manifest href: {}", href.escape_debug()),
        });
    }

    Ok(())
}

fn validate_section_media_type(path: &str, media_type: Option<&str>) -> Result<(), HwpError> {
    let Some(media_type) = media_type else {
        return Err(HwpError::InvalidHwpxStructure {
            reason: format!("content.hpf section item missing required media-type for {path}"),
        });
    };

    validate_media_type_value(media_type)?;

    if is_xml_media_type(media_type) {
        return Ok(());
    }

    Err(HwpError::InvalidHwpxStructure {
        reason: format!(
            "Invalid content.hpf section media-type for {path}: {}",
            media_type.escape_debug()
        ),
    })
}

fn validate_media_type_value(media_type: &str) -> Result<(), HwpError> {
    if media_type.len() > MAX_HWPX_CONTENT_HPF_REF_BYTES {
        return Err(HwpError::ResourceLimitExceeded {
            resource: "HWPX content.hpf media-type bytes",
            path: CONTENT_HPF_PATH.to_string(),
            limit: MAX_HWPX_CONTENT_HPF_REF_BYTES as u64,
            actual: media_type.len() as u64,
        });
    }

    if media_type.trim().is_empty() {
        return Err(HwpError::InvalidHwpxStructure {
            reason: "Invalid content.hpf section media-type: empty value".to_string(),
        });
    }

    if media_type.chars().any(char::is_control) {
        return Err(HwpError::InvalidHwpxStructure {
            reason: "Invalid content.hpf section media-type: control characters are not allowed"
                .to_string(),
        });
    }

    Ok(())
}

fn is_xml_media_type(media_type: &str) -> bool {
    let mime = media_type
        .split_once(';')
        .map_or(media_type, |(mime, _)| mime)
        .trim();

    mime.eq_ignore_ascii_case("application/xml")
        || mime.eq_ignore_ascii_case("text/xml")
        || mime.to_ascii_lowercase().ends_with("+xml")
}

fn parse_spine_itemref(e: &BytesStart<'_>) -> Result<Option<String>, HwpError> {
    let mut idref = None;
    let mut linear = None;

    for_each_xml_attribute(CONTENT_HPF_PATH, e, |attr| {
        match attr.key.as_ref() {
            b"idref" => {
                idref = Some(parse_string_attr(
                    CONTENT_HPF_PATH,
                    "opf:itemref",
                    "idref",
                    &attr,
                )?);
            }
            b"linear" => {
                linear = Some(parse_string_attr(
                    CONTENT_HPF_PATH,
                    "opf:itemref",
                    "linear",
                    &attr,
                )?);
            }
            _ => {}
        }
        Ok(())
    })?;

    let idref = idref.ok_or_else(|| HwpError::InvalidHwpxStructure {
        reason: "content.hpf spine itemref missing required idref".to_string(),
    })?;
    validate_package_ref("content.hpf spine itemref idref", &idref)?;

    match linear.as_deref() {
        Some("no") => return Ok(None),
        Some("yes") | None => {}
        Some(value) => {
            return Err(HwpError::InvalidHwpxStructure {
                reason: format!(
                    "Invalid content.hpf spine itemref linear value: {}",
                    value.escape_debug()
                ),
            });
        }
    }

    Ok(Some(idref))
}

fn is_element(e: &BytesStart<'_>, local_name: &[u8]) -> bool {
    has_local_name(e.name().as_ref(), local_name)
}

fn is_direct_child(
    parent_scope_depth: Option<usize>,
    parent_depth: usize,
    e: &BytesStart<'_>,
    local_name: &[u8],
) -> bool {
    parent_scope_depth == Some(parent_depth) && is_element(e, local_name)
}

fn is_end_element(e: &BytesEnd<'_>, local_name: &[u8]) -> bool {
    has_local_name(e.name().as_ref(), local_name)
}

fn has_local_name(name: &[u8], local_name: &[u8]) -> bool {
    if name == local_name {
        return true;
    }

    name.iter()
        .rposition(|byte| *byte == b':')
        .map(|position| &name[position + 1..])
        == Some(local_name)
}
