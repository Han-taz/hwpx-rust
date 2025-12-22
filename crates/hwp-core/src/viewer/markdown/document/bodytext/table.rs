/// Table conversion to Markdown/HTML
/// 테이블을 마크다운/HTML로 변환하는 모듈
///
/// 스펙 문서 매핑: 표 57 - 본문의 데이터 레코드, TABLE (HWPTAG_BEGIN + 61)
/// Spec mapping: Table 57 - BodyText data records, TABLE (HWPTAG_BEGIN + 61)
use crate::document::{bodytext::Table, HwpDocument, ParagraphRecord};

/// Convert table to markdown/HTML format
/// 테이블을 마크다운/HTML 형식으로 변환
/// 복잡한 테이블(colspan/rowspan)은 HTML로, 단순한 테이블은 마크다운으로 변환
pub fn convert_table_to_markdown(
    table: &Table,
    document: &HwpDocument,
    options: &crate::viewer::markdown::MarkdownOptions,
    tracker: &mut crate::viewer::markdown::utils::OutlineNumberTracker,
) -> String {
    let row_count = table.attributes.row_count as usize;
    let col_count = table.attributes.col_count as usize;

    if row_count == 0 || col_count == 0 {
        return format!("\n\n[Table: {row_count}x{col_count}]\n\n");
    }

    // 셀이 비어있어도 표 형식으로 출력 / Output table format even if cells are empty
    if table.cells.is_empty() {
        return format!("\n\n[Empty Table: {row_count}x{col_count}]\n\n");
    }

    // 복잡한 테이블인지 확인 (colspan > 1 또는 rowspan > 1인 셀이 있는지)
    // Check if table is complex (has cells with colspan > 1 or rowspan > 1)
    let has_merged_cells = table
        .cells
        .iter()
        .any(|cell| cell.cell_attributes.col_span > 1 || cell.cell_attributes.row_span > 1);

    // HTML 모드이거나 병합된 셀이 있으면 HTML 테이블로 출력
    // Use HTML table if in HTML mode or has merged cells
    if options.use_html == Some(true) || has_merged_cells {
        convert_table_to_html(table, document, options, tracker)
    } else {
        convert_table_to_markdown_simple(table, document, options, tracker)
    }
}

/// Convert table to HTML format with colspan/rowspan support
/// colspan/rowspan을 지원하는 HTML 테이블 형식으로 변환
fn convert_table_to_html(
    table: &Table,
    document: &HwpDocument,
    options: &crate::viewer::markdown::MarkdownOptions,
    tracker: &mut crate::viewer::markdown::utils::OutlineNumberTracker,
) -> String {
    let row_count = table.attributes.row_count as usize;
    let col_count = table.attributes.col_count as usize;

    // 셀을 row_address 기준으로 그룹화
    // Group cells by row_address
    let min_row = table
        .cells
        .iter()
        .map(|c| c.cell_attributes.row_address)
        .min()
        .unwrap_or(0);
    let min_col = table
        .cells
        .iter()
        .map(|c| c.cell_attributes.col_address)
        .min()
        .unwrap_or(0);

    // 병합된 셀을 추적하기 위한 그리드 (이미 다른 셀에 의해 커버된 위치)
    // Grid to track merged cells (positions already covered by other cells)
    let mut covered: Vec<Vec<bool>> = vec![vec![false; col_count]; row_count];

    // 셀을 row, col 순서로 정렬
    // Sort cells by row, col order
    let mut sorted_cells: Vec<_> = table.cells.iter().collect();
    sorted_cells.sort_by_key(|cell| {
        (
            cell.cell_attributes.row_address,
            cell.cell_attributes.col_address,
        )
    });

    // HTML 테이블 생성
    let mut html = String::new();
    html.push_str("\n<table border=\"1\" cellpadding=\"5\" cellspacing=\"0\" style=\"border-collapse: collapse;\">\n");

    for row_idx in 0..row_count {
        // 이 행에 속한 셀들 찾기
        // Find cells belonging to this row
        let row_cells: Vec<_> = sorted_cells
            .iter()
            .filter(|cell| {
                (cell.cell_attributes.row_address.saturating_sub(min_row)) as usize == row_idx
            })
            .collect();

        // 행의 셀 내용을 먼저 수집
        // First collect cell contents for this row
        let mut row_cell_contents: Vec<(String, usize, usize, usize)> = Vec::new(); // (content, col_idx, col_span, row_span)
        let mut row_has_content = false;

        for cell in &row_cells {
            let col_idx = (cell.cell_attributes.col_address.saturating_sub(min_col)) as usize;

            // 이미 다른 셀에 의해 커버된 위치는 건너뛰기
            // Skip positions already covered by other cells
            if col_idx < col_count && covered[row_idx][col_idx] {
                continue;
            }

            let col_span = cell.cell_attributes.col_span as usize;
            let row_span = cell.cell_attributes.row_span as usize;

            // 커버된 위치 표시
            // Mark covered positions
            for r in row_idx..(row_idx + row_span).min(row_count) {
                for c in col_idx..(col_idx + col_span).min(col_count) {
                    covered[r][c] = true;
                }
            }

            // 셀 내용 추출
            // Extract cell content
            let cell_content = get_cell_content(cell, document, options, tracker);

            // 빈 행 필터링: 셀에 실제 내용이 있는지 확인
            // Empty row filtering: check if cell has actual content
            if !cell_content.trim().is_empty() {
                row_has_content = true;
            }

            row_cell_contents.push((cell_content, col_idx, col_span, row_span));
        }

        // 행에 내용이 없으면 건너뛰기 (레이아웃용 빈 행 필터링)
        // Skip row if it has no content (filter out layout-only empty rows)
        if !row_has_content {
            continue;
        }

        html.push_str("  <tr>\n");

        for (cell_content, _col_idx, col_span, row_span) in row_cell_contents {
            // td 태그 생성
            // Generate td tag
            let mut td_attrs = Vec::new();
            if col_span > 1 {
                td_attrs.push(format!("colspan=\"{col_span}\""));
            }
            if row_span > 1 {
                td_attrs.push(format!("rowspan=\"{row_span}\""));
            }

            let attrs_str = if td_attrs.is_empty() {
                String::new()
            } else {
                format!(" {}", td_attrs.join(" "))
            };

            // 셀 내용에 줄바꿈이 있으면 <br>로 변환
            // Convert newlines to <br> in cell content
            let cell_html = cell_content.replace('\n', "<br>");

            html.push_str(&format!("    <td{attrs_str}>{cell_html}</td>\n"));
        }

        html.push_str("  </tr>\n");
    }

    html.push_str("</table>\n");
    html
}

/// Convert table to simple markdown format (no colspan/rowspan support)
/// 단순 마크다운 형식으로 변환 (colspan/rowspan 미지원)
fn convert_table_to_markdown_simple(
    table: &Table,
    document: &HwpDocument,
    options: &crate::viewer::markdown::MarkdownOptions,
    tracker: &mut crate::viewer::markdown::utils::OutlineNumberTracker,
) -> String {
    let row_count = table.attributes.row_count as usize;
    let col_count = table.attributes.col_count as usize;

    // 2D 배열로 셀 정렬 (행/열 위치 기준) / Arrange cells in 2D array (by row/column position)
    let mut grid: Vec<Vec<Option<String>>> = vec![vec![None; col_count]; row_count];

    let min_row = table
        .cells
        .iter()
        .map(|c| c.cell_attributes.row_address)
        .min()
        .unwrap_or(1);
    let min_col = table
        .cells
        .iter()
        .map(|c| c.cell_attributes.col_address)
        .min()
        .unwrap_or(1);

    let all_same_row = table
        .cells
        .iter()
        .all(|c| c.cell_attributes.row_address == min_row);

    let mut sorted_cells: Vec<_> = table.cells.iter().enumerate().collect();
    sorted_cells.sort_by_key(|(_, cell)| {
        (
            cell.cell_attributes.row_address,
            cell.cell_attributes.col_address,
        )
    });

    if all_same_row {
        let mut row_index = 0;
        let mut last_col = u16::MAX;

        for (_original_idx, cell) in sorted_cells {
            let col = (cell.cell_attributes.col_address.saturating_sub(min_col)) as usize;

            if cell.cell_attributes.col_address <= last_col && last_col != u16::MAX {
                row_index += 1;
                if row_index >= row_count {
                    row_index = row_count - 1;
                }
            }
            last_col = cell.cell_attributes.col_address;

            let row = row_index;

            if col < col_count {
                fill_cell_content(
                    &mut grid, cell, row, col, row_count, col_count, document, options, tracker,
                );
            }
        }
    } else {
        for cell in &table.cells {
            let row = (cell.cell_attributes.row_address.saturating_sub(min_row)) as usize;
            let col = (cell.cell_attributes.col_address.saturating_sub(min_col)) as usize;

            if row < row_count && col < col_count {
                fill_cell_content(
                    &mut grid, cell, row, col, row_count, col_count, document, options, tracker,
                );
            }
        }
    }

    // 마크다운 표 형식으로 변환 / Convert to markdown table format
    let mut lines = Vec::new();
    lines.push(String::new());

    for row_idx in 0..row_count {
        let row_data: Vec<String> = (0..col_count)
            .map(|col| {
                grid[row_idx][col]
                    .clone()
                    .unwrap_or_else(|| " ".to_string())
            })
            .collect();
        lines.push(format!("| {} |", row_data.join(" | ")));

        if row_idx == 0 {
            lines.push(format!(
                "|{}|",
                (0..col_count).map(|_| "---").collect::<Vec<_>>().join("|")
            ));
        }
    }

    lines.push(String::new());
    lines.join("\n")
}

/// Get cell content as string
/// 셀 내용을 문자열로 추출
fn get_cell_content(
    cell: &crate::document::bodytext::TableCell,
    document: &HwpDocument,
    options: &crate::viewer::markdown::MarkdownOptions,
    tracker: &mut crate::viewer::markdown::utils::OutlineNumberTracker,
) -> String {
    let mut cell_parts = Vec::new();

    for para in &cell.paragraphs {
        for record in &para.records {
            match record {
                ParagraphRecord::ParaText { text, .. } => {
                    if !text.trim().is_empty() {
                        cell_parts.push(text.clone());
                    }
                }
                ParagraphRecord::ShapeComponentPicture { shape_component_picture } => {
                    if let Some(image_md) =
                        crate::viewer::markdown::document::bodytext::shape_component_picture::convert_shape_component_picture_to_markdown(
                            shape_component_picture,
                            document,
                            options.image_output_dir.as_deref(),
                        )
                    {
                        cell_parts.push(image_md);
                    }
                }
                ParagraphRecord::ShapeComponent { children, .. } => {
                    let shape_parts =
                        crate::viewer::markdown::document::bodytext::shape_component::convert_shape_component_children_to_markdown(
                            children,
                            document,
                            options.image_output_dir.as_deref(),
                            tracker,
                        );
                    cell_parts.extend(shape_parts);
                }
                ParagraphRecord::HwpxImage { binary_item_ref } => {
                    // HWPX 이미지 참조 변환 / Convert HWPX image reference
                    if let Some(image_md) =
                        crate::viewer::markdown::document::bodytext::shape_component_picture::convert_hwpx_image_to_markdown(
                            binary_item_ref,
                            document,
                            options.image_output_dir.as_deref(),
                        )
                    {
                        cell_parts.push(image_md);
                    }
                }
                _ => {}
            }
        }
    }

    cell_parts.join(" ")
}

/// Fill cell content and handle cell merging
/// 셀 내용을 채우고 셀 병합을 처리
#[allow(unused_assignments)]
fn fill_cell_content(
    grid: &mut [Vec<Option<String>>],
    cell: &crate::document::bodytext::TableCell,
    row: usize,
    col: usize,
    row_count: usize,
    col_count: usize,
    document: &HwpDocument,
    options: &crate::viewer::markdown::MarkdownOptions,
    tracker: &mut crate::viewer::markdown::utils::OutlineNumberTracker,
) {
    // 셀 내용을 텍스트와 이미지로 변환 / Convert cell content to text and images
    let mut cell_parts = Vec::new();
    let mut has_image = false;

    // 먼저 이미지가 있는지 확인 / First check if image exists
    for para in &cell.paragraphs {
        for rec in &para.records {
            if matches!(rec, ParagraphRecord::ShapeComponentPicture { .. }) {
                has_image = true;
                break;
            }
        }
        if has_image {
            break;
        }
    }

    // 셀 내부의 문단들을 마크다운으로 변환 / Convert paragraphs inside cell to markdown
    for (idx, para) in cell.paragraphs.iter().enumerate() {
        // 테이블 셀 내부에서는 PARA_BREAK를 직접 처리해야 함
        // In table cells, we need to handle PARA_BREAK directly

        // 문단 내의 모든 ParaText 레코드를 먼저 수집하여 함께 처리
        // First collect all ParaText records in the paragraph to process together
        let mut para_text_records = Vec::new();
        let mut has_non_text_records = false;

        for record in &para.records {
            match record {
                ParagraphRecord::ParaText {
                    text,
                    control_char_positions,
                    ..
                } => {
                    para_text_records.push((text, control_char_positions));
                }
                _ => {
                    has_non_text_records = true;
                }
            }
        }

        // ParaText 레코드가 있으면 직접 처리 / If ParaText records exist, process directly
        if !para_text_records.is_empty() {
            let mut para_text_result = String::new();

            for (text, control_char_positions) in para_text_records {
                // PARA_BREAK나 LINE_BREAK를 직접 처리 / Handle PARA_BREAK or LINE_BREAK directly
                let has_breaks = control_char_positions.iter().any(|pos| {
                    use crate::document::bodytext::ControlChar;
                    pos.code == ControlChar::PARA_BREAK || pos.code == ControlChar::LINE_BREAK
                });

                if !has_breaks {
                    // 제어 문자가 없으면 텍스트만 추가 / If no control characters, just add text
                    para_text_result.push_str(text);
                    continue;
                }

                let mut last_char_pos = 0;

                // control_positions를 정렬하여 순서대로 처리 / Sort control_positions to process in order
                let mut sorted_positions: Vec<_> = control_char_positions
                    .iter()
                    .filter(|pos| {
                        use crate::document::bodytext::ControlChar;
                        pos.code == ControlChar::PARA_BREAK || pos.code == ControlChar::LINE_BREAK
                    })
                    .collect();
                sorted_positions.sort_by_key(|pos| pos.position);

                for pos in sorted_positions {
                    // position은 문자 인덱스이므로, 그 위치까지의 텍스트를 문자 단위로 추가
                    // position is character index, so add text up to that position by character
                    let text_len = text.chars().count();
                    if pos.position > last_char_pos && pos.position <= text_len {
                        // 문자 단위로 텍스트 추출 / Extract text by character
                        let text_before: String = text
                            .chars()
                            .skip(last_char_pos)
                            .take(pos.position - last_char_pos)
                            .collect();
                        // trim() 없이 그대로 추가 (정확한 위치 유지) / Add as-is without trim (maintain exact position)
                        para_text_result.push_str(&text_before);
                    }

                    // PARA_BREAK나 LINE_BREAK 위치에 <br> 추가 / Add <br> at PARA_BREAK or LINE_BREAK position
                    if options.use_html == Some(true) {
                        para_text_result.push_str("<br>");
                    } else {
                        para_text_result.push(' ');
                    }

                    // 제어 문자 다음 위치 / Position after control character
                    // position이 텍스트 끝이면 더 이상 텍스트가 없으므로 text_len으로 설정
                    // If position is at end of text, set to text_len as there's no more text
                    last_char_pos = if pos.position >= text_len {
                        text_len
                    } else {
                        pos.position + 1
                    };
                }

                // 마지막 부분의 텍스트 추가 (last_char_pos가 텍스트 길이보다 작을 때만)
                // Add remaining text (only if last_char_pos is less than text length)
                let text_len = text.chars().count();
                if last_char_pos < text_len {
                    let text_after: String = text.chars().skip(last_char_pos).collect();
                    // trim() 없이 그대로 추가 / Add as-is without trim
                    para_text_result.push_str(&text_after);
                }
            }

            if !para_text_result.trim().is_empty() {
                cell_parts.push(para_text_result);
            }
        }
        // ParaText가 아닌 레코드(이미지 등)가 있으면 직접 처리
        // Process non-ParaText records (images, etc.) directly if they exist
        if has_non_text_records {
            for record in &para.records {
                match record {
                    ParagraphRecord::ParaText { .. } => {
                        // ParaText는 이미 처리했으므로 건너뜀 / Skip ParaText as it's already processed
                        continue;
                    }
                    ParagraphRecord::ShapeComponentPicture {
                        shape_component_picture,
                    } => {
                        // ShapeComponentPicture 변환 / Convert ShapeComponentPicture
                        if let Some(image_md) =
                            crate::viewer::markdown::document::bodytext::shape_component_picture::convert_shape_component_picture_to_markdown(
                                shape_component_picture,
                                document,
                                options.image_output_dir.as_deref(),
                            )
                        {
                            cell_parts.push(image_md);
                            has_image = true;
                        }
                    }
                    ParagraphRecord::ShapeComponent {
                        shape_component: _,
                        children,
                    } => {
                        // SHAPE_COMPONENT의 children을 재귀적으로 처리 / Recursively process SHAPE_COMPONENT's children
                        let shape_parts =
                            crate::viewer::markdown::document::bodytext::shape_component::convert_shape_component_children_to_markdown(
                                children,
                                document,
                                options.image_output_dir.as_deref(),
                                tracker,
                            );
                        for shape_part in shape_parts {
                            if shape_part.contains("![이미지]") {
                                has_image = true;
                            }
                            cell_parts.push(shape_part);
                        }
                    }
                    ParagraphRecord::HwpxImage { binary_item_ref } => {
                        // HWPX 이미지 참조 변환 / Convert HWPX image reference
                        if let Some(image_md) =
                            crate::viewer::markdown::document::bodytext::shape_component_picture::convert_hwpx_image_to_markdown(
                                binary_item_ref,
                                document,
                                options.image_output_dir.as_deref(),
                            )
                        {
                            cell_parts.push(image_md);
                            has_image = true;
                        }
                    }
                    _ => {
                        // 기타 레코드는 서식 정보이므로 건너뜀 / Other records are formatting info, skip
                    }
                }
            }
        }

        // 마지막 문단이 아니면 문단 사이 공백 추가
        // If not last paragraph, add space between paragraphs
        if idx < cell.paragraphs.len() - 1 {
            cell_parts.push(" ".to_string());
        }
    }

    // 셀 내용을 하나의 문자열로 결합 / Combine cell parts into a single string
    // 표 셀 내부에서는 개행을 공백으로 변환 (마크다운 표는 한 줄로 표시)
    // In table cells, convert line breaks to spaces (markdown tables are displayed in one line)
    let cell_text = cell_parts.join("");

    // 마크다운 표에서 파이프 문자 이스케이프 처리 / Escape pipe characters in markdown table
    let cell_content = if cell_text.is_empty() {
        " ".to_string() // 빈 셀은 공백으로 표시 / Empty cell shows as space
    } else {
        cell_text.replace('|', "\\|") // 파이프 문자 이스케이프 / Escape pipe character
    };

    // 셀에 이미 내용이 있으면 덮어쓰지 않음 (병합 셀 처리)
    // Don't overwrite if cell already has content (handle merged cells)
    if grid[row][col].is_none() {
        grid[row][col] = Some(cell_content);

        // 셀 병합 처리: col_span과 row_span에 따라 병합된 셀을 빈 셀로 채움
        // Handle cell merging: fill merged cells with empty cells based on col_span and row_span
        let col_span = cell.cell_attributes.col_span as usize;
        let row_span = cell.cell_attributes.row_span as usize;

        // 병합된 열을 빈 셀로 채움 (마크다운에서는 병합을 직접 표현할 수 없으므로 빈 셀로 처리)
        // Fill merged columns with empty cells (markdown doesn't support cell merging directly)
        for c in (col + 1)..(col + col_span).min(col_count) {
            if grid[row][c].is_none() {
                grid[row][c] = Some(" ".to_string());
            }
        }

        // 병합된 행을 빈 셀로 채움
        // Fill merged rows with empty cells
        for r in (row + 1)..(row + row_span).min(row_count) {
            for c in col..(col + col_span).min(col_count) {
                if grid[r][c].is_none() {
                    grid[r][c] = Some(" ".to_string());
                }
            }
        }
    }
}
