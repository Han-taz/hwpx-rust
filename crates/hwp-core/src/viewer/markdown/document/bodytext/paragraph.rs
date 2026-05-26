/// Paragraph conversion to Markdown
/// 문단을 마크다운으로 변환하는 모듈
///
/// 스펙 문서 매핑: 표 57 - 본문의 데이터 레코드
/// Spec mapping: Table 57 - BodyText data records
use crate::document::bodytext::CharShapeInfo;
use crate::document::CharShape;
use crate::document::{HwpDocument, Paragraph, ParagraphRecord};
use crate::viewer::markdown::collect::collect_text_and_images_from_paragraph;
use crate::viewer::markdown::document::bodytext::para_text::{
    convert_para_text_to_markdown, convert_para_text_to_markdown_with_char_shapes,
    convert_para_text_to_markdown_with_crossing_hyperlinks,
    convert_para_text_to_markdown_with_hyperlinks, convert_runs_to_markdown,
    runs_have_char_shape_id, CrossingHyperlinkState, HyperlinkRegion,
};
use crate::viewer::markdown::document::bodytext::shape_component::convert_shape_component_children_to_markdown;
use crate::viewer::markdown::document::bodytext::shape_component_picture::{
    convert_hwpx_image_to_markdown, convert_shape_component_picture_to_markdown,
};
use crate::viewer::markdown::document::bodytext::table::convert_table_to_markdown;
use crate::viewer::markdown::utils::{
    convert_to_outline_with_number, is_text_part, OutlineNumberTracker,
};
use crate::viewer::markdown::MarkdownOptions;
use crate::viewer::shared::BinDataIndex;

/// Convert a paragraph to markdown
/// 문단을 마크다운으로 변환
pub fn convert_paragraph_to_markdown(
    paragraph: &Paragraph,
    document: &HwpDocument,
    bindata_index: &BinDataIndex,
    options: &MarkdownOptions,
    tracker: &mut OutlineNumberTracker,
) -> String {
    if paragraph.records.is_empty() {
        return String::new();
    }

    let mut parts = Vec::new();
    let mut text_parts = Vec::new(); // 같은 문단 내의 텍스트 레코드들을 모음 / Collect text records in the same paragraph
    let mut char_shapes: Vec<CharShapeInfo> = Vec::new(); // 문단의 글자 모양 정보 수집 / Collect character shape information for paragraph

    // 먼저 ParaCharShape 레코드를 수집 / First collect ParaCharShape records
    for record in &paragraph.records {
        if let ParagraphRecord::ParaCharShape { shapes } = record {
            char_shapes.extend(shapes.clone());
        }
    }

    // CharShape를 가져오는 클로저 / Closure to get CharShape
    let get_char_shape = |shape_id: u32| -> Option<&CharShape> {
        document.doc_info.char_shapes.get(shape_id as usize)
    };

    // paragraph.records에서 테이블 관련 이미지 ID를 먼저 수집
    // First collect image IDs related to tables in paragraph.records
    let mut para_table_cell_image_ids: std::collections::HashSet<u16> =
        std::collections::HashSet::new();
    for record in &paragraph.records {
        if let ParagraphRecord::CtrlHeader { data: ch_data } = record {
            let header = &ch_data.header;
            let children = &ch_data.children;
            if header.ctrl_id == crate::document::CtrlId::TABLE {
                // TABLE 컨트롤의 children에서 모든 이미지 ID 수집 (셀 내용 + children 직접)
                // Collect all image IDs from TABLE control's children (cell content + direct children)
                for child in children {
                    match child {
                        ParagraphRecord::Table { table } => {
                            // 테이블 셀 내용에서 이미지 ID 수집
                            for cell in &table.cells {
                                for para in &cell.paragraphs {
                                    let mut dummy_texts = std::collections::HashSet::new();
                                    collect_text_and_images_from_paragraph(
                                        para,
                                        &mut dummy_texts,
                                        &mut para_table_cell_image_ids,
                                    );
                                }
                            }
                        }
                        ParagraphRecord::ShapeComponent { data: sc_data } => {
                            let shape_children = &sc_data.children;
                            // CtrlHeader.children의 ShapeComponent에서 이미지 ID 수집
                            for shape_child in shape_children {
                                if let ParagraphRecord::ShapeComponentPicture {
                                    shape_component_picture,
                                } = shape_child
                                {
                                    para_table_cell_image_ids
                                        .insert(shape_component_picture.picture_info.bindata_id);
                                }
                            }
                        }
                        ParagraphRecord::ShapeComponentPicture {
                            shape_component_picture,
                        } => {
                            // CtrlHeader.children의 직접 ShapeComponentPicture에서 이미지 ID 수집
                            para_table_cell_image_ids
                                .insert(shape_component_picture.picture_info.bindata_id);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    // 하이퍼링크 CtrlHeader에서 URL 수집 (순서대로) / Collect URLs from hyperlink CtrlHeaders (in order)
    let mut hyperlinks: Vec<HyperlinkRegion> = Vec::new();
    for record in &paragraph.records {
        if let ParagraphRecord::CtrlHeader { data: ch_data } = record {
            let header = &ch_data.header;
            use crate::viewer::markdown::utils::{get_hyperlink_url, is_hyperlink_field};
            if is_hyperlink_field(header) {
                if let Some(url) = get_hyperlink_url(header) {
                    hyperlinks.push(HyperlinkRegion { url });
                }
            }
        }
    }

    // Process all records in order / 모든 레코드를 순서대로 처리
    for record in &paragraph.records {
        match record {
            ParagraphRecord::ParaText { data: pt_data } => {
                let text = &pt_data.text;
                let control_char_positions = &pt_data.control_char_positions;
                let runs = &pt_data.runs;
                // ParaText 변환 / Convert ParaText
                // 표 셀 내부의 텍스트는 이미 Table.cells에 포함되어 convert_table_to_markdown에서 처리되므로
                // 여기서는 표 앞뒤의 일반 텍스트도 정상적으로 처리됨
                // Text inside table cells is already included in Table.cells and processed in convert_table_to_markdown,
                // so regular text before/after tables is also processed normally here

                // HWPX runs에 char_shape_id가 있으면 우선 사용 / Use runs with char_shape_id first (HWPX)
                let text_md = if runs_have_char_shape_id(runs) {
                    convert_runs_to_markdown(runs, document)
                } else if !hyperlinks.is_empty() {
                    // 하이퍼링크가 있으면 하이퍼링크 변환 사용 / Use hyperlink conversion if hyperlinks exist
                    convert_para_text_to_markdown_with_hyperlinks(
                        text,
                        control_char_positions,
                        &hyperlinks,
                    )
                } else if !char_shapes.is_empty() {
                    convert_para_text_to_markdown_with_char_shapes(
                        text,
                        control_char_positions,
                        &char_shapes,
                        Some(&get_char_shape),
                    )
                } else {
                    convert_para_text_to_markdown(text, control_char_positions)
                };

                if let Some(text_md) = text_md {
                    // 같은 문단 내의 텍스트는 나중에 합침 / Text in the same paragraph will be combined later
                    text_parts.push(text_md);
                }
            }
            ParagraphRecord::ShapeComponent { data: sc_data } => {
                let children = &sc_data.children;
                // SHAPE_COMPONENT의 children을 재귀적으로 처리 / Recursively process SHAPE_COMPONENT's children
                // 테이블 셀 내부의 이미지는 테이블 변환 시 이미 포함되므로 건너뜀
                // Skip images inside table cells as they are already included in table conversion
                for child in children {
                    match child {
                        ParagraphRecord::ShapeComponentPicture {
                            shape_component_picture,
                        } => {
                            if para_table_cell_image_ids
                                .contains(&shape_component_picture.picture_info.bindata_id)
                            {
                                continue; // 테이블 셀 내부 이미지는 건너뜀
                            }
                            if let Some(image_md) = convert_shape_component_picture_to_markdown(
                                shape_component_picture,
                                document,
                                bindata_index,
                                options.image_output_dir.as_deref(),
                            ) {
                                parts.push(image_md);
                            }
                        }
                        _ => {
                            // 다른 타입은 기존 방식으로 처리
                            let shape_parts = convert_shape_component_children_to_markdown(
                                std::slice::from_ref(child),
                                document,
                                bindata_index,
                                options.image_output_dir.as_deref(),
                                tracker,
                            );
                            parts.extend(shape_parts);
                        }
                    }
                }
            }
            ParagraphRecord::ShapeComponentPicture {
                shape_component_picture,
            } => {
                // ShapeComponentPicture 변환 / Convert ShapeComponentPicture
                // 테이블 셀 내부의 이미지는 테이블 변환 시 이미 포함되므로 건너뜀
                if para_table_cell_image_ids
                    .contains(&shape_component_picture.picture_info.bindata_id)
                {
                    continue; // 테이블 셀 내부 이미지는 건너뜀
                }
                if let Some(image_md) = convert_shape_component_picture_to_markdown(
                    shape_component_picture,
                    document,
                    bindata_index,
                    options.image_output_dir.as_deref(),
                ) {
                    parts.push(image_md);
                }
            }
            ParagraphRecord::HwpxImage { binary_item_ref } => {
                // HWPX 이미지 참조 변환 / Convert HWPX image reference
                if let Some(image_md) = convert_hwpx_image_to_markdown(
                    binary_item_ref,
                    document,
                    bindata_index,
                    options.image_output_dir.as_deref(),
                ) {
                    parts.push(image_md);
                }
            }
            ParagraphRecord::CtrlHeader { data: ch_data } => {
                let header = &ch_data.header;
                let children = &ch_data.children;
                let ctrl_paragraphs = &ch_data.paragraphs;
                // 하이퍼링크 필드 처리 / Handle hyperlink field
                use crate::viewer::markdown::utils::{
                    get_hyperlink_url, is_hyperlink_field, should_process_control_header,
                };
                if is_hyperlink_field(header) {
                    // ctrl_paragraphs가 있는 경우에만 처리 / Only process if ctrl_paragraphs exist
                    // ctrl_paragraphs가 비어있으면 ParaText에서 이미 처리됨 / If empty, already processed in ParaText
                    if !ctrl_paragraphs.is_empty() {
                        // 하이퍼링크: 내부 문단에서 텍스트를 추출하고 URL과 결합
                        // Hyperlink: extract text from inner paragraphs and combine with URL
                        if let Some(url) = get_hyperlink_url(header) {
                            // 내부 문단들의 텍스트를 수집 / Collect text from inner paragraphs
                            let mut link_texts: Vec<String> = Vec::new();
                            for para in ctrl_paragraphs {
                                let para_md = convert_paragraph_to_markdown(
                                    para,
                                    document,
                                    bindata_index,
                                    options,
                                    tracker,
                                );
                                if !para_md.is_empty() {
                                    link_texts.push(para_md);
                                }
                            }
                            let link_text = link_texts.join(" ").trim().to_string();

                            if !link_text.is_empty() {
                                // [text](url) 형식으로 마크다운 링크 생성 / Create markdown link as [text](url)
                                parts.push(format!("[{}]({})", link_text, url));
                            } else {
                                // 텍스트가 없으면 URL만 표시 / If no text, show URL only
                                parts.push(format!("<{}>", url));
                            }
                        } else {
                            // URL이 없으면 내부 문단만 처리 / If no URL, process inner paragraphs only
                            for para in ctrl_paragraphs {
                                let para_md = convert_paragraph_to_markdown(
                                    para,
                                    document,
                                    bindata_index,
                                    options,
                                    tracker,
                                );
                                if !para_md.is_empty() {
                                    parts.push(para_md);
                                }
                            }
                        }
                    }
                    continue; // 하이퍼링크 처리 완료 / Hyperlink processing done
                }

                // 마크다운으로 표현할 수 없는 컨트롤 헤더는 건너뜀 / Skip control headers that cannot be expressed in markdown
                if !should_process_control_header(header) {
                    // CTRL_HEADER 내부의 직접 문단 처리 / Process direct paragraphs inside CTRL_HEADER
                    for para in ctrl_paragraphs {
                        let para_md = convert_paragraph_to_markdown(
                            para,
                            document,
                            bindata_index,
                            options,
                            tracker,
                        );
                        if !para_md.is_empty() {
                            parts.push(para_md);
                        }
                    }
                    // CTRL_HEADER의 children 처리 (예: SHAPE_COMPONENT_PICTURE) / Process CTRL_HEADER's children (e.g., SHAPE_COMPONENT_PICTURE)
                    for child in children {
                        match child {
                            ParagraphRecord::ShapeComponentPicture {
                                shape_component_picture,
                            } => {
                                if let Some(image_md) = convert_shape_component_picture_to_markdown(
                                    shape_component_picture,
                                    document,
                                    bindata_index,
                                    options.image_output_dir.as_deref(),
                                ) {
                                    parts.push(image_md);
                                }
                            }
                            _ => {
                                // 기타 자식 레코드는 무시 (이미 처리된 경우만 도달) / Ignore other child records (only reached if already processed)
                            }
                        }
                    }
                    continue; // 이 컨트롤 헤더는 더 이상 처리하지 않음 / Don't process this control header further
                }

                // 컨트롤 헤더의 자식 레코드 처리 (레벨 2, 예: 테이블, 그림 등) / Process control header children (level 2, e.g., table, images, etc.)
                let mut has_table = false;

                // 표 셀 내부의 텍스트와 이미지를 수집하여 셀 내부 문단인지 확인
                // Collect text and images from paragraphs in Table.cells to check if they are cell content
                // 포인터 비교는 복사된 객체에서 실패할 수 있으므로 텍스트 내용과 이미지 ID로 비교
                // Pointer comparison may fail with copied objects, so compare by text content and image ID
                let mut table_cell_texts: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                let mut table_cell_image_ids: std::collections::HashSet<u16> =
                    std::collections::HashSet::new();
                let mut table_cell_para_ids: std::collections::HashSet<u32> =
                    std::collections::HashSet::new(); // 표 셀 내부 문단의 instance_id 수집
                let mut table_opt: Option<&crate::document::bodytext::Table> = None;
                let mut table_index: Option<usize> = None;
                let is_table_control = header.ctrl_id == crate::document::CtrlId::TABLE;

                // 먼저 TABLE을 찾아서 table_opt에 저장 / First find TABLE and store in table_opt
                let children_slice: &[ParagraphRecord] = children;
                for (idx, child) in children_slice.iter().enumerate() {
                    if let ParagraphRecord::Table { table } = child {
                        table_opt = Some(table);
                        table_index = Some(idx);
                        break;
                    }
                }

                if let Some(table) = table_opt {
                    for cell in &table.cells {
                        for para in &cell.paragraphs {
                            // 문단의 instance_id 수집 (0이 아닌 경우만) / Collect paragraph instance_id (only if not 0)
                            if para.para_header.instance_id != 0 {
                                table_cell_para_ids.insert(para.para_header.instance_id);
                            }
                            // ParaText의 텍스트를 수집 / Collect text from ParaText
                            // 재귀적으로 모든 레코드를 확인하여 이미지 ID 수집 / Recursively check all records to collect image IDs
                            collect_text_and_images_from_paragraph(
                                para,
                                &mut table_cell_texts,
                                &mut table_cell_image_ids,
                            );
                        }
                    }
                }

                // libhwp 방식: 파서 단계에서 TABLE 이전/이후의 LIST_HEADER를 children에 추가하지 않으므로
                // 여기서는 처리하지 않음
                // libhwp approach: LIST_HEADERs (before/after TABLE) are not added to children in parser stage,
                // so we don't process them here
                // TABLE 이전의 LIST_HEADER는 캡션으로 별도 처리되고,
                // TABLE 이후의 LIST_HEADER는 셀로만 처리됨
                // LIST_HEADERs before TABLE are processed as captions separately,
                // and LIST_HEADERs after TABLE are only processed as cells

                // 컨트롤 헤더의 자식 레코드들을 순회하며 처리 / Iterate through control header's child records and process them
                let children_slice: &[ParagraphRecord] = children;
                let is_shape_object = header.ctrl_id == crate::document::CtrlId::SHAPE_OBJECT;

                // 먼저 Table을 처리 (표 안의 이미지가 표보다 앞에 배치되는 문제 방지)
                // First process Table (prevent images inside table from being placed before table)
                if let Some(table_idx) = table_index {
                    if let ParagraphRecord::Table { table } = &children_slice[table_idx] {
                        let table_md = convert_table_to_markdown(
                            table,
                            document,
                            bindata_index,
                            options,
                            tracker,
                        );
                        if !table_md.is_empty() {
                            parts.push(table_md);
                            has_table = true;
                        }
                    }
                }

                // 그 다음 나머지 children 처리 / Then process remaining children
                for (index, child) in children_slice.iter().enumerate() {
                    // Table은 이미 처리됨 / Table already processed
                    if Some(index) == table_index {
                        continue;
                    }

                    match child {
                        ParagraphRecord::ListHeader { data: lh_data } => {
                            let paragraphs = &lh_data.paragraphs;
                            // LIST_HEADER 처리 (표 아래 캡션 등) / Process LIST_HEADER (caption below, etc.)
                            // libhwp 방식: TABLE 이후의 LIST_HEADER는 Table.cells에 포함되어 있으므로 children에서 처리하지 않음
                            // libhwp approach: LIST_HEADERs after TABLE are included in Table.cells, so don't process in children
                            if let Some(table_idx) = table_index {
                                if index > table_idx && is_table_control {
                                    // TABLE 이후의 LIST_HEADER는 이미 Table.cells에 포함되어 있으므로 건너뜀
                                    // LIST_HEADERs after TABLE are already included in Table.cells, so skip
                                    // libhwp는 TABLE 이후의 LIST_HEADER를 children에서 처리하지 않고,
                                    // TABLE을 읽은 후 명시적으로 각 셀을 순회하며 처리함
                                    // libhwp doesn't process LIST_HEADERs after TABLE in children,
                                    // but processes them explicitly when iterating through cells after reading TABLE
                                } else {
                                    // 표 앞의 LIST_HEADER는 이미 처리됨 / LIST_HEADER before table is already processed
                                    // 표가 없거나 표 셀 내부가 아닌 경우 일반 처리 / General processing if no table or not inside table cell
                                    for para in paragraphs {
                                        let para_md = convert_paragraph_to_markdown(
                                            para,
                                            document,
                                            bindata_index,
                                            options,
                                            tracker,
                                        );
                                        if !para_md.is_empty() {
                                            parts.push(para_md);
                                        }
                                    }
                                }
                            } else {
                                // 표가 없는 경우 일반 처리 / General processing if no table
                                for para in paragraphs {
                                    let para_md = convert_paragraph_to_markdown(
                                        para,
                                        document,
                                        bindata_index,
                                        options,
                                        tracker,
                                    );
                                    if !para_md.is_empty() {
                                        parts.push(para_md);
                                    }
                                }
                            }
                        }
                        ParagraphRecord::ShapeComponent { data: sc_data } => {
                            let shape_children = &sc_data.children;
                            // SHAPE_COMPONENT의 children 처리 (SHAPE_COMPONENT_PICTURE 등) / Process SHAPE_COMPONENT's children (SHAPE_COMPONENT_PICTURE, etc.)
                            // 먼저 표 셀 내부의 이미지인지 확인 / First check if images are inside table cells
                            let mut shape_parts_to_output = Vec::new();
                            let mut has_image_in_shape = false;

                            for child in shape_children {
                                match child {
                                    ParagraphRecord::ShapeComponentPicture {
                                        shape_component_picture,
                                    } => {
                                        // 표 셀 내부의 이미지인지 확인 (bindata_id로 직접 비교) / Check if image is inside table cell (direct comparison by bindata_id)
                                        let is_table_cell_image = table_cell_image_ids.contains(
                                            &shape_component_picture.picture_info.bindata_id,
                                        );

                                        if is_table_cell_image {
                                            // 표 셀 내부의 이미지는 표 셀에 이미 포함되므로 여기서는 표시하지 않음
                                            // Images inside table cells are already included in table cells, so don't display here
                                            has_image_in_shape = true;
                                        } else {
                                            // 표 셀 내부가 아닌 경우 이미지 변환 / Convert image if not inside table cell
                                            if let Some(image_md) =
                                                convert_shape_component_picture_to_markdown(
                                                    shape_component_picture,
                                                    document,
                                                    bindata_index,
                                                    options.image_output_dir.as_deref(),
                                                )
                                            {
                                                shape_parts_to_output.push(image_md);
                                                has_image_in_shape = true;
                                            }
                                        }
                                    }
                                    ParagraphRecord::ListHeader { data: lh_data } => {
                                        let paragraphs = &lh_data.paragraphs;
                                        // LIST_HEADER의 paragraphs 처리 (글상자 텍스트) / Process LIST_HEADER's paragraphs (textbox text)
                                        for para in paragraphs {
                                            let para_md = convert_paragraph_to_markdown(
                                                para,
                                                document,
                                                bindata_index,
                                                options,
                                                tracker,
                                            );
                                            if !para_md.is_empty() {
                                                shape_parts_to_output.push(para_md);
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }

                            // 변환된 부분들을 출력 / Output converted parts
                            for shape_part in shape_parts_to_output {
                                if has_table && has_image_in_shape {
                                    // 표가 있고 이미지가 있는 경우, 이미지가 표 셀 내부가 아니므로 출력
                                    // If table exists and image exists, image is not inside table cell, so output
                                    parts.push(shape_part);
                                } else if is_shape_object {
                                    // SHAPE_OBJECT의 경우 표 밖에서도 이미지를 처리
                                    // For SHAPE_OBJECT, process image even outside table
                                    parts.push(shape_part);
                                } else {
                                    // 기타 경우에는 출력
                                    // For other cases, output
                                    parts.push(shape_part);
                                }
                            }
                        }
                        ParagraphRecord::ShapeComponentPicture {
                            shape_component_picture,
                        } => {
                            // ShapeComponentPicture 변환 / Convert ShapeComponentPicture
                            // 표 셀 내부의 이미지인지 확인 / Check if image is inside table cell
                            let is_table_cell_image = table_cell_image_ids
                                .contains(&shape_component_picture.picture_info.bindata_id);

                            // 표 셀 내부의 이미지는 표 셀에 이미 포함되므로 여기서는 표시하지 않음
                            // Images inside table cells are already included in table cells, so don't display here
                            if !is_table_cell_image {
                                if let Some(image_md) = convert_shape_component_picture_to_markdown(
                                    shape_component_picture,
                                    document,
                                    bindata_index,
                                    options.image_output_dir.as_deref(),
                                ) {
                                    if has_table {
                                        // 표가 있지만 셀 내부가 아닌 경우 (표 위/아래 이미지 등)
                                        // Table exists but not inside cell (e.g., image above/below table)
                                        parts.push(image_md);
                                    } else if is_shape_object {
                                        // SHAPE_OBJECT의 경우 표 밖에서도 이미지를 처리
                                        // For SHAPE_OBJECT, process image even outside table
                                        parts.push(image_md);
                                    } else {
                                        // 기타 경우에는 출력
                                        // For other cases, output
                                        parts.push(image_md);
                                    }
                                }
                            }
                        }
                        _ => {
                            // 기타 레코드는 무시 / Ignore other records
                            // CtrlHeader는 이미 위에서 처리됨 / CtrlHeader is already processed above
                        }
                    }
                }

                // CTRL_HEADER 내부의 직접 문단 처리 / Process direct paragraphs inside CTRL_HEADER
                for para in ctrl_paragraphs {
                    // 표 셀 내부의 문단인지 확인 / Check if paragraph is inside table cell
                    let is_table_cell = if is_table_control && table_opt.is_some() {
                        // 먼저 instance_id로 확인 (가장 정확, 0이 아닌 경우만) / First check by instance_id (most accurate, only if not 0)
                        let is_table_cell_by_id = !table_cell_para_ids.is_empty()
                            && para.para_header.instance_id != 0
                            && table_cell_para_ids.contains(&para.para_header.instance_id);

                        // instance_id로 확인되지 않으면 텍스트와 이미지로 확인 / If not found by instance_id, check by text and images
                        if is_table_cell_by_id {
                            true
                        } else {
                            // 재귀적으로 문단에서 텍스트와 이미지를 수집하여 표 셀 내부인지 확인
                            // Recursively collect text and images from paragraph to check if inside table cell
                            let mut para_texts = std::collections::HashSet::new();
                            let mut para_image_ids = std::collections::HashSet::new();
                            collect_text_and_images_from_paragraph(
                                para,
                                &mut para_texts,
                                &mut para_image_ids,
                            );

                            // 수집한 텍스트나 이미지가 표 셀 내부에 포함되어 있는지 확인
                            // Check if collected text or images are included in table cells
                            (!para_texts.is_empty()
                                && para_texts
                                    .iter()
                                    .any(|text| table_cell_texts.contains(text)))
                                || (!para_image_ids.is_empty()
                                    && para_image_ids
                                        .iter()
                                        .any(|id| table_cell_image_ids.contains(id)))
                        }
                    } else {
                        false
                    };

                    // 표 셀 내부가 아닌 경우에만 처리 / Only process if not inside table cell
                    if !is_table_cell {
                        let para_md = convert_paragraph_to_markdown(
                            para,
                            document,
                            bindata_index,
                            options,
                            tracker,
                        );
                        if !para_md.is_empty() {
                            parts.push(para_md);
                        }
                    }
                }
            }
            ParagraphRecord::Table { table } => {
                // HWPX 파서에서 직접 생성된 테이블 처리 / Handle tables directly created by HWPX parser
                // (CtrlHeader로 감싸지지 않은 테이블)
                let table_md =
                    convert_table_to_markdown(table, document, bindata_index, options, tracker);
                if !table_md.is_empty() {
                    parts.push(table_md);
                }
            }
            _ => {
                // 기타 레코드는 무시 / Ignore other records
            }
        }
    }

    // 같은 문단 내의 텍스트를 합침 / Combine text in the same paragraph
    if !text_parts.is_empty() {
        let combined_text = text_parts.join("");
        // 개요 레벨 확인 및 개요 번호 추가 / Check outline level and add outline number
        let outline_md = convert_to_outline_with_number(
            &combined_text,
            &paragraph.para_header,
            document,
            tracker,
        );
        parts.push(outline_md);
    }

    // 마크다운 문법에 맞게 개행 처리 / Handle line breaks according to markdown syntax
    // 텍스트가 연속으로 나올 때는 스페이스 2개 + 개행, 블록 요소 사이는 빈 줄
    // For consecutive text: two spaces + newline, for block elements: blank line
    if parts.is_empty() {
        return String::new();
    }

    if parts.len() == 1 {
        return parts[0].clone();
    }

    let mut result = String::new();
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            // 이전 part가 텍스트이고 현재 part도 텍스트인 경우: 스페이스 2개 + 개행
            // If previous part is text and current part is also text: two spaces + newline
            if is_text_part(&parts[i - 1]) && is_text_part(part) {
                result.push_str("  \n");
            }
            // 블록 요소가 포함된 경우: 빈 줄
            // When block elements are involved, use blank line
            else {
                result.push_str("\n\n");
            }
        }
        result.push_str(part);
    }

    result
}

/// 문단 경계 하이퍼링크 변환 결과
/// Result of paragraph conversion with crossing hyperlinks
pub struct ParagraphConversionResult {
    /// 마크다운 결과 / Markdown result
    pub markdown: String,
    /// 새로운 열린 하이퍼링크 상태 / New open hyperlink state
    pub new_open_state: Option<CrossingHyperlinkState>,
}

/// 문단 경계 하이퍼링크를 처리하여 문단을 마크다운으로 변환
/// Convert a paragraph to markdown handling cross-paragraph hyperlinks
///
/// # Arguments / 매개변수
/// * `paragraph` - 변환할 문단 / Paragraph to convert
/// * `document` - HWP 문서 / HWP document
/// * `options` - 마크다운 옵션 / Markdown options
/// * `tracker` - 개요 번호 추적기 / Outline number tracker
/// * `open_hyperlink` - 이전 문단에서 열린 하이퍼링크 상태 / Open hyperlink state from previous paragraph
///
/// # Returns / 반환값
/// 마크다운 문자열과 새로운 열린 하이퍼링크 상태 / Markdown string and new open hyperlink state
pub fn convert_paragraph_to_markdown_with_state(
    paragraph: &Paragraph,
    document: &HwpDocument,
    bindata_index: &BinDataIndex,
    options: &MarkdownOptions,
    tracker: &mut OutlineNumberTracker,
    open_hyperlink: Option<&CrossingHyperlinkState>,
) -> ParagraphConversionResult {
    if paragraph.records.is_empty() {
        return ParagraphConversionResult {
            markdown: String::new(),
            new_open_state: open_hyperlink.cloned(),
        };
    }

    let mut parts = Vec::new();
    let mut text_parts = Vec::new();
    let mut char_shapes: Vec<CharShapeInfo> = Vec::new();
    let mut new_open_state: Option<CrossingHyperlinkState> = None;

    // 먼저 ParaCharShape 레코드를 수집 / First collect ParaCharShape records
    for record in &paragraph.records {
        if let ParagraphRecord::ParaCharShape { shapes } = record {
            char_shapes.extend(shapes.clone());
        }
    }

    // CharShape를 가져오는 클로저 / Closure to get CharShape
    let get_char_shape = |shape_id: u32| -> Option<&CharShape> {
        document.doc_info.char_shapes.get(shape_id as usize)
    };

    // 테이블 관련 이미지 ID 수집 / Collect table-related image IDs
    let mut para_table_cell_image_ids: std::collections::HashSet<u16> =
        std::collections::HashSet::new();
    for record in &paragraph.records {
        if let ParagraphRecord::CtrlHeader { data: ch_data } = record {
            let header = &ch_data.header;
            let children = &ch_data.children;
            if header.ctrl_id == crate::document::CtrlId::TABLE {
                for child in children {
                    match child {
                        ParagraphRecord::Table { table } => {
                            for cell in &table.cells {
                                for para in &cell.paragraphs {
                                    let mut dummy_texts = std::collections::HashSet::new();
                                    collect_text_and_images_from_paragraph(
                                        para,
                                        &mut dummy_texts,
                                        &mut para_table_cell_image_ids,
                                    );
                                }
                            }
                        }
                        ParagraphRecord::ShapeComponent { data: sc_data } => {
                            let shape_children = &sc_data.children;
                            for shape_child in shape_children {
                                if let ParagraphRecord::ShapeComponentPicture {
                                    shape_component_picture,
                                } = shape_child
                                {
                                    para_table_cell_image_ids
                                        .insert(shape_component_picture.picture_info.bindata_id);
                                }
                            }
                        }
                        ParagraphRecord::ShapeComponentPicture {
                            shape_component_picture,
                        } => {
                            para_table_cell_image_ids
                                .insert(shape_component_picture.picture_info.bindata_id);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    // 하이퍼링크 CtrlHeader에서 URL 수집 / Collect URLs from hyperlink CtrlHeaders
    let mut hyperlinks: Vec<HyperlinkRegion> = Vec::new();
    for record in &paragraph.records {
        if let ParagraphRecord::CtrlHeader { data: ch_data } = record {
            let header = &ch_data.header;
            use crate::viewer::markdown::utils::{get_hyperlink_url, is_hyperlink_field};
            if is_hyperlink_field(header) {
                if let Some(url) = get_hyperlink_url(header) {
                    hyperlinks.push(HyperlinkRegion { url });
                }
            }
        }
    }

    // 열린 하이퍼링크가 있거나 이 문단에 하이퍼링크가 있으면 crossing hyperlink 처리 사용
    // Use crossing hyperlink processing if open hyperlink exists or this paragraph has hyperlinks
    let use_crossing = open_hyperlink.is_some() || !hyperlinks.is_empty();

    // Process all records in order
    for record in &paragraph.records {
        match record {
            ParagraphRecord::ParaText { data: pt_data } => {
                let text = &pt_data.text;
                let control_char_positions = &pt_data.control_char_positions;
                let runs = &pt_data.runs;
                if use_crossing {
                    // 문단 경계 하이퍼링크 처리 / Handle crossing hyperlinks
                    let result = convert_para_text_to_markdown_with_crossing_hyperlinks(
                        text,
                        control_char_positions,
                        &hyperlinks,
                        open_hyperlink,
                    );
                    if let Some(md) = result.markdown {
                        text_parts.push(md);
                    }
                    // 새 열린 상태 업데이트 / Update new open state
                    if result.new_open_state.is_some() {
                        new_open_state = result.new_open_state;
                    }
                } else if runs_have_char_shape_id(runs) {
                    // HWPX runs에 char_shape_id가 있으면 우선 사용
                    // Use runs with char_shape_id first (HWPX)
                    if let Some(text_md) = convert_runs_to_markdown(runs, document) {
                        text_parts.push(text_md);
                    }
                } else if !char_shapes.is_empty() {
                    let text_md = convert_para_text_to_markdown_with_char_shapes(
                        text,
                        control_char_positions,
                        &char_shapes,
                        Some(&get_char_shape),
                    );
                    if let Some(text_md) = text_md {
                        text_parts.push(text_md);
                    }
                } else {
                    let text_md = convert_para_text_to_markdown(text, control_char_positions);
                    if let Some(text_md) = text_md {
                        text_parts.push(text_md);
                    }
                }
            }
            ParagraphRecord::ShapeComponent { data: sc_data } => {
                let children = &sc_data.children;
                for child in children {
                    match child {
                        ParagraphRecord::ShapeComponentPicture {
                            shape_component_picture,
                        } => {
                            if para_table_cell_image_ids
                                .contains(&shape_component_picture.picture_info.bindata_id)
                            {
                                continue;
                            }
                            if let Some(image_md) = convert_shape_component_picture_to_markdown(
                                shape_component_picture,
                                document,
                                bindata_index,
                                options.image_output_dir.as_deref(),
                            ) {
                                parts.push(image_md);
                            }
                        }
                        _ => {
                            let shape_parts = convert_shape_component_children_to_markdown(
                                std::slice::from_ref(child),
                                document,
                                bindata_index,
                                options.image_output_dir.as_deref(),
                                tracker,
                            );
                            parts.extend(shape_parts);
                        }
                    }
                }
            }
            ParagraphRecord::ShapeComponentPicture {
                shape_component_picture,
            } => {
                if para_table_cell_image_ids
                    .contains(&shape_component_picture.picture_info.bindata_id)
                {
                    continue;
                }
                if let Some(image_md) = convert_shape_component_picture_to_markdown(
                    shape_component_picture,
                    document,
                    bindata_index,
                    options.image_output_dir.as_deref(),
                ) {
                    parts.push(image_md);
                }
            }
            ParagraphRecord::HwpxImage { binary_item_ref } => {
                if let Some(image_md) = convert_hwpx_image_to_markdown(
                    binary_item_ref,
                    document,
                    bindata_index,
                    options.image_output_dir.as_deref(),
                ) {
                    parts.push(image_md);
                }
            }
            ParagraphRecord::CtrlHeader { data: ch_data } => {
                let header = &ch_data.header;
                let children = &ch_data.children;
                let ctrl_paragraphs = &ch_data.paragraphs;
                use crate::viewer::markdown::utils::{
                    get_hyperlink_url, is_hyperlink_field, should_process_control_header,
                };

                // 하이퍼링크 필드는 ParaText에서 처리되므로 건너뜀 / Skip hyperlink fields (handled in ParaText)
                if is_hyperlink_field(header) {
                    // ctrl_paragraphs가 있는 경우에만 처리 / Only process if ctrl_paragraphs exist
                    if !ctrl_paragraphs.is_empty() {
                        if let Some(url) = get_hyperlink_url(header) {
                            let mut link_texts: Vec<String> = Vec::new();
                            for para in ctrl_paragraphs {
                                let para_md = convert_paragraph_to_markdown(
                                    para,
                                    document,
                                    bindata_index,
                                    options,
                                    tracker,
                                );
                                if !para_md.is_empty() {
                                    link_texts.push(para_md);
                                }
                            }
                            let link_text = link_texts.join(" ").trim().to_string();

                            if !link_text.is_empty() {
                                parts.push(format!("[{}]({})", link_text, url));
                            } else {
                                parts.push(format!("<{}>", url));
                            }
                        } else {
                            for para in ctrl_paragraphs {
                                let para_md = convert_paragraph_to_markdown(
                                    para,
                                    document,
                                    bindata_index,
                                    options,
                                    tracker,
                                );
                                if !para_md.is_empty() {
                                    parts.push(para_md);
                                }
                            }
                        }
                    }
                    continue;
                }

                if !should_process_control_header(header) {
                    for para in ctrl_paragraphs {
                        let para_md = convert_paragraph_to_markdown(
                            para,
                            document,
                            bindata_index,
                            options,
                            tracker,
                        );
                        if !para_md.is_empty() {
                            parts.push(para_md);
                        }
                    }
                    for child in children {
                        if let ParagraphRecord::ShapeComponentPicture {
                            shape_component_picture,
                        } = child
                        {
                            if let Some(image_md) = convert_shape_component_picture_to_markdown(
                                shape_component_picture,
                                document,
                                bindata_index,
                                options.image_output_dir.as_deref(),
                            ) {
                                parts.push(image_md);
                            }
                        }
                    }
                    continue;
                }

                // 테이블 처리 / Handle table
                let mut table_opt: Option<&crate::document::bodytext::Table> = None;
                let mut table_index: Option<usize> = None;

                for (idx, child) in children.iter().enumerate() {
                    if let ParagraphRecord::Table { table } = child {
                        table_opt = Some(table);
                        table_index = Some(idx);
                        break;
                    }
                }

                if let Some(table_idx) = table_index {
                    if let ParagraphRecord::Table { table } = &children[table_idx] {
                        let table_md = convert_table_to_markdown(
                            table,
                            document,
                            bindata_index,
                            options,
                            tracker,
                        );
                        if !table_md.is_empty() {
                            parts.push(table_md);
                        }
                    }
                }

                for (index, child) in children.iter().enumerate() {
                    if Some(index) == table_index {
                        continue;
                    }
                    match child {
                        ParagraphRecord::ListHeader { data: lh_data } => {
                            let paragraphs = &lh_data.paragraphs;
                            for para in paragraphs {
                                let para_md = convert_paragraph_to_markdown(
                                    para,
                                    document,
                                    bindata_index,
                                    options,
                                    tracker,
                                );
                                if !para_md.is_empty() {
                                    parts.push(para_md);
                                }
                            }
                        }
                        ParagraphRecord::ShapeComponentPicture {
                            shape_component_picture,
                        } => {
                            if let Some(image_md) = convert_shape_component_picture_to_markdown(
                                shape_component_picture,
                                document,
                                bindata_index,
                                options.image_output_dir.as_deref(),
                            ) {
                                parts.push(image_md);
                            }
                        }
                        _ => {}
                    }
                }

                // 표 셀 외부 문단 처리 / Handle paragraphs outside table cells
                let table_cell_para_ids: std::collections::HashSet<u32> =
                    if let Some(table) = table_opt {
                        table
                            .cells
                            .iter()
                            .flat_map(|cell| cell.paragraphs.iter())
                            .filter(|p| p.para_header.instance_id != 0)
                            .map(|p| p.para_header.instance_id)
                            .collect()
                    } else {
                        std::collections::HashSet::new()
                    };

                for para in ctrl_paragraphs {
                    let is_table_cell = !table_cell_para_ids.is_empty()
                        && para.para_header.instance_id != 0
                        && table_cell_para_ids.contains(&para.para_header.instance_id);

                    if !is_table_cell {
                        let para_md = convert_paragraph_to_markdown(
                            para,
                            document,
                            bindata_index,
                            options,
                            tracker,
                        );
                        if !para_md.is_empty() {
                            parts.push(para_md);
                        }
                    }
                }
            }
            ParagraphRecord::Table { table } => {
                let table_md =
                    convert_table_to_markdown(table, document, bindata_index, options, tracker);
                if !table_md.is_empty() {
                    parts.push(table_md);
                }
            }
            _ => {}
        }
    }

    // 텍스트 합치기 / Combine text
    if !text_parts.is_empty() {
        let combined_text = text_parts.join("");
        let outline_md = convert_to_outline_with_number(
            &combined_text,
            &paragraph.para_header,
            document,
            tracker,
        );
        parts.push(outline_md);
    }

    // 결과 조합 / Combine result
    if parts.is_empty() {
        return ParagraphConversionResult {
            markdown: String::new(),
            new_open_state,
        };
    }

    if parts.len() == 1 {
        return ParagraphConversionResult {
            markdown: parts[0].clone(),
            new_open_state,
        };
    }

    let mut result = String::new();
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            if is_text_part(&parts[i - 1]) && is_text_part(part) {
                result.push_str("  \n");
            } else {
                result.push_str("\n\n");
            }
        }
        result.push_str(part);
    }

    ParagraphConversionResult {
        markdown: result,
        new_open_state,
    }
}
