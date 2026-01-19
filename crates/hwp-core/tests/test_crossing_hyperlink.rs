//! Test for cross-paragraph hyperlinks

use hwp_core::document::{CtrlHeaderData, ParagraphRecord};
use hwp_core::HwpParser;

#[test]
#[ignore = "HWP support frozen - focusing on HWPX only"]
fn test_issue144_crossing_hyperlink() {
    let file_path = "tests/fixtures/issue144-fields-crossing-lineseg-boundary.hwp";
    let data = std::fs::read(file_path).expect("Failed to read file");

    let parser = HwpParser::new();
    let document = parser.parse(&data).expect("Failed to parse");

    // Check paragraphs
    let section = &document.body_text.sections[0];

    println!("\n=== Paragraph Analysis ===");
    for (i, para) in section.paragraphs.iter().enumerate() {
        println!("\n--- Paragraph {} ---", i + 1);
        println!("Control mask value: {:?}", para.para_header.control_mask.value);

        for record in &para.records {
            match record {
                ParagraphRecord::ParaText { text, control_char_positions, .. } => {
                    println!("ParaText: {:?}", text);
                    println!("Control positions: {:?}", control_char_positions);
                }
                ParagraphRecord::CtrlHeader { header, .. } => {
                    if let CtrlHeaderData::Field { field_type, command, .. } = &header.data {
                        if field_type == "%hlk" || field_type == "hlk" {
                            println!("Hyperlink CtrlHeader: field_type={}, command={}", field_type, command);
                        }
                    }
                    println!("CtrlHeader ctrl_id: {:?}", header.ctrl_id);
                }
                _ => {}
            }
        }
    }

    // Generate markdown
    let options = hwp_core::viewer::markdown::MarkdownOptions {
        image_output_dir: None,
        use_html: Some(false),
        include_version: Some(true),
        include_page_info: Some(false),
    };
    let markdown = document.to_markdown(&options);

    println!("\n=== Generated Markdown ===");
    println!("{}", markdown);

    // Assertions
    assert!(markdown.contains("[google google google google google google](http"),
        "First paragraph hyperlink should work");

    // Check for cross-paragraph hyperlink
    // Paragraph 2 should have: gmail g[mail gmail gmail gmail gmail gmail](http://gmail.com)
    // Paragraph 3 should have: [gmai](http://gmail.com)le
    assert!(markdown.contains("gmail.com"), "Should contain gmail.com hyperlink");
}
