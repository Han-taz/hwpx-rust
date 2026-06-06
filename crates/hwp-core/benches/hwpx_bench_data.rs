use std::io::{Cursor, Write};
use zip::{write::SimpleFileOptions, ZipWriter};

pub fn synthetic_hwpx_document(
    section_count: usize,
    paragraphs_per_section: usize,
    runs_per_paragraph: usize,
) -> Vec<u8> {
    build_synthetic_hwpx(section_count, |section_index| {
        let mut section_xml = section_xml_start();

        for paragraph_index in 0..paragraphs_per_section {
            section_xml.push_str(r#"<hp:p>"#);
            for run_index in 0..runs_per_paragraph {
                section_xml.push_str("<hp:run><hp:t>");
                section_xml.push_str(&format!(
                    "section {section_index} paragraph {paragraph_index} run {run_index} "
                ));
                section_xml.push_str("</hp:t></hp:run>");
            }
            section_xml.push_str("</hp:p>\n");
        }

        section_xml.push_str("</hs:sec>");
        section_xml
    })
}

pub fn synthetic_hwpx_table_document(
    section_count: usize,
    rows_per_table: usize,
    cols_per_table: usize,
) -> Vec<u8> {
    build_synthetic_hwpx(section_count, |section_index| {
        let mut section_xml = section_xml_start();
        section_xml.push_str("<hp:tbl>\n");

        for row_index in 0..rows_per_table {
            section_xml.push_str("<hp:tr>\n");
            for col_index in 0..cols_per_table {
                section_xml.push_str("<hp:tc>");
                section_xml.push_str(&format!(
                    r#"<hp:cellAddr colAddr="{col_index}" rowAddr="{row_index}"/>"#
                ));
                section_xml.push_str(r#"<hp:cellSpan colSpan="1" rowSpan="1"/>"#);
                section_xml.push_str("<hp:subList><hp:p><hp:run><hp:t>");
                section_xml.push_str(&format!("cell {section_index} {row_index} {col_index}"));
                section_xml.push_str("</hp:t></hp:run></hp:p></hp:subList>");
                section_xml.push_str("</hp:tc>\n");
            }
            section_xml.push_str("</hp:tr>\n");
        }

        section_xml.push_str("</hp:tbl>\n</hs:sec>");
        section_xml
    })
}

fn build_synthetic_hwpx(
    section_count: usize,
    mut section_xml: impl FnMut(usize) -> String,
) -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("mimetype", options).unwrap();
    zip.write_all(b"application/hwp+zip").unwrap();

    zip.start_file("version.xml", options).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?><opf:version xmlns:opf="http://www.idpf.org/2007/opf" major="5"/>"#,
    )
    .unwrap();

    zip.start_file("Contents/header.xml", options).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?><hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head"/>"#,
    )
    .unwrap();

    for section_index in 0..section_count {
        zip.start_file(format!("Contents/section{section_index}.xml"), options)
            .unwrap();
        zip.write_all(section_xml(section_index).as_bytes())
            .unwrap();
    }

    zip.finish().unwrap().into_inner()
}

fn section_xml_start() -> String {
    String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section"
        xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
"#,
    )
}
