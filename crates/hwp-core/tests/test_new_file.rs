use hwp_core::HwpParser;
use std::fs;

#[test]
fn test_parse_new_hwpx_file() {
    let path = "../../tests/251216 조간 (보도) 국가인공지능전략위원회, 대한민국 인공지능행동계획(안) 주요 내용 소개 및 각계각층 의견 수렴 착수.hwpx";

    let data = fs::read(path).expect("Failed to read file");
    let parser = HwpParser::new();
    let result = parser.parse(&data);

    match result {
        Ok(doc) => {
            println!("\n=== Document Info ===");
            println!("Version: {:?}", doc.file_header.version);
            println!("Sections: {}", doc.body_text.sections.len());

            // Output paragraphs from first section
            if let Some(section) = doc.body_text.sections.first() {
                println!("\n=== Section 0 - {} paragraphs ===", section.paragraphs.len());

                for (i, para) in section.paragraphs.iter().take(50).enumerate() {
                    for record in &para.records {
                        if let hwp_core::document::bodytext::ParagraphRecord::ParaText { text, .. } = record {
                            if !text.trim().is_empty() {
                                println!("[{}] {}", i, text.trim());
                            }
                        }
                    }
                }
            }

            // Convert to markdown
            let options = hwp_core::viewer::markdown::MarkdownOptions {
                image_output_dir: None,
                use_html: Some(true),
                include_version: None,
                include_page_info: None,
            };
            let md = hwp_core::viewer::markdown::to_markdown(&doc, &options);
            println!("\n=== Full Markdown Output ===");
            println!("{}", md);

            // Also save to file for comparison
            let output_path = "../../tests/parsed_output_hwpx.md";
            std::fs::write(output_path, &md).expect("Failed to write output");
            println!("\n=== Saved to {} ===", output_path);
        }
        Err(e) => {
            panic!("Parse error: {:?}", e);
        }
    }
}
