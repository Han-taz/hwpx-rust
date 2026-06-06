use super::CtrlHeaderResult;
use crate::document::bodytext::ctrl_header::VertRelTo;
use crate::document::bodytext::ParagraphRecord;
use crate::document::{CtrlHeader, CtrlHeaderData, Paragraph};
use crate::viewer::html::common;
use crate::viewer::html::line_segment::ImageInfo;
use crate::viewer::shared::{BinDataIndex, BinDataItemLookup};
use crate::viewer::HtmlOptions;

#[derive(Debug, Clone, Copy)]
struct ParentShapeSize {
    width: Option<u32>,
    height: Option<u32>,
}

struct ImageCollectionContext<'a> {
    options: &'a HtmlOptions,
    bindata_index: &'a BinDataIndex,
    bindata_lookup: &'a BinDataItemLookup<'a>,
    like_letters: bool,
    vert_rel_to: Option<VertRelTo>,
    images: &'a mut Vec<ImageInfo>,
}

/// 그리기 개체 처리 / Process shape object
pub fn process_shape_object<'a>(
    header: &'a CtrlHeader,
    children: &'a [ParagraphRecord],
    paragraphs: &'a [Paragraph],
    options: &'a HtmlOptions,
    bindata_index: &'a BinDataIndex,
    bindata_lookup: &'a BinDataItemLookup<'a>,
) -> CtrlHeaderResult<'a> {
    let mut result = CtrlHeaderResult::new();

    // object_common 속성 추출 / Extract object_common attributes
    let (like_letters, vert_rel_to) = match &header.data {
        CtrlHeaderData::ObjectCommon { attribute, .. } => {
            (attribute.like_letters, Some(attribute.vert_rel_to))
        }
        _ => (false, None),
    };

    // children과 paragraphs에서 첫 번째 ShapeComponent 찾기 (크기 정보 추출용) / Find first ShapeComponent in children and paragraphs (for size extraction)
    let mut initial_width = None;
    let mut initial_height = None;

    // children에서 찾기 / Search in children
    for record in children {
        if let ParagraphRecord::ShapeComponent { data: sc_data } = record {
            initial_width = Some(sc_data.shape_component.width);
            initial_height = Some(sc_data.shape_component.height);
            break;
        }
    }

    // children에서 찾지 못했으면 paragraphs에서 찾기 / If not found in children, search in paragraphs
    if initial_width.is_none() {
        for para in paragraphs {
            for record in &para.records {
                match record {
                    ParagraphRecord::ShapeComponent { data: sc_data } => {
                        initial_width = Some(sc_data.shape_component.width);
                        initial_height = Some(sc_data.shape_component.height);
                        break;
                    }
                    ParagraphRecord::CtrlHeader { data: ch_data } => {
                        // 중첩된 CtrlHeader의 children에서도 찾기 / Also search in nested CtrlHeader's children
                        for nested_record in &ch_data.children {
                            if let ParagraphRecord::ShapeComponent { data: sc_data } = nested_record
                            {
                                initial_width = Some(sc_data.shape_component.width);
                                initial_height = Some(sc_data.shape_component.height);
                                break;
                            }
                        }
                        if initial_width.is_some() {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if initial_width.is_some() {
                break;
            }
        }
    }

    // children과 paragraphs에서 재귀적으로 이미지 수집 / Recursively collect images from children and paragraphs
    // JSON 구조: CtrlHeader의 children에 ShapeComponent가 있고, 그 children에 ShapeComponentPicture가 있음
    // JSON structure: CtrlHeader's children contains ShapeComponent, and its children contains ShapeComponentPicture

    let parent_size = ParentShapeSize {
        width: initial_width,
        height: initial_height,
    };
    let mut image_context = ImageCollectionContext {
        options,
        bindata_index,
        bindata_lookup,
        like_letters,
        vert_rel_to,
        images: &mut result.images,
    };

    // 1. children이 있으면 children을 먼저 처리 (가장 일반적인 경우) / If children exists, process children first (most common case)
    if !children.is_empty() {
        collect_images_from_records(children, &mut image_context, parent_size);
    } else if initial_width.is_some() && initial_height.is_some() {
        // 2. children이 비어있고 paragraphs에 ShapeComponent가 있으면, paragraphs의 records에서 ShapeComponent를 찾아서 처리
        // If children is empty and paragraphs has ShapeComponent, find ShapeComponent in paragraphs' records and process
        for para in paragraphs {
            // paragraphs의 records를 재귀적으로 탐색하여 이미지 수집
            // Recursively search paragraphs' records to collect images
            collect_images_from_records(&para.records, &mut image_context, parent_size);
        }
    }

    result
}

/// ParagraphRecord 배열에서 재귀적으로 이미지 수집 / Recursively collect images from ParagraphRecord array
fn collect_images_from_records(
    records: &[ParagraphRecord],
    context: &mut ImageCollectionContext<'_>,
    parent_size: ParentShapeSize,
) {
    for record in records {
        match record {
            ParagraphRecord::ShapeComponentPicture {
                shape_component_picture,
            } => {
                let bindata_id = shape_component_picture.picture_info.bindata_id;
                let image_url = common::get_image_url_with_lookup(
                    context.bindata_index,
                    context.bindata_lookup,
                    bindata_id,
                    context.options.image_output_dir.as_deref(),
                    context.options.html_output_dir.as_deref(),
                );
                if !image_url.is_empty() {
                    // shape_component.width/height를 우선 사용 / Prioritize shape_component.width/height
                    let width = parent_size.width.unwrap_or(0);
                    let height = parent_size.height.unwrap_or(0);

                    if width > 0 && height > 0 {
                        context.images.push(ImageInfo {
                            width,
                            height,
                            url: image_url,
                            like_letters: context.like_letters,
                            vert_rel_to: context.vert_rel_to,
                        });
                    }
                }
            }
            ParagraphRecord::ShapeComponent { data: sc_data } => {
                // 재귀적으로 children에서 이미지 찾기 (shape_component.width/height 전달)
                collect_images_from_records(
                    &sc_data.children,
                    context,
                    ParentShapeSize {
                        width: Some(sc_data.shape_component.width),
                        height: Some(sc_data.shape_component.height),
                    },
                );
            }
            ParagraphRecord::CtrlHeader { data: ch_data } => {
                // 중첩된 CtrlHeader도 처리 (속성은 상위에서 상속, shape_component 크기는 유지)
                // Process nested CtrlHeader (attributes inherited from parent, shape_component size maintained)
                collect_images_from_records(&ch_data.children, context, parent_size);
            }
            _ => {}
        }
    }
}
