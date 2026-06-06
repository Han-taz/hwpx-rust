//! Property-based robustness checks for untrusted HWPX input.

use std::io::{Cursor, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};

use hwp_core::HwpParser;
use proptest::prelude::*;
use std::collections::BTreeSet;
use zip::{write::SimpleFileOptions, ZipWriter};

fn zip_with_entries(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);

    let mut seen_paths = BTreeSet::new();
    for (path, data) in entries {
        if !seen_paths.insert(path) {
            continue;
        }

        zip.start_file(path, SimpleFileOptions::default())
            .expect("test ZIP entry should start");
        zip.write_all(data)
            .expect("test ZIP entry should be writable");
    }

    zip.finish().expect("test ZIP should finish").into_inner()
}

fn hwpx_entry_path() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("mimetype".to_string()),
        Just("version.xml".to_string()),
        Just("META-INF/container.xml".to_string()),
        Just("Contents/header.xml".to_string()),
        (0usize..64).prop_map(|index| format!("Contents/section{index}.xml")),
        (0usize..32).prop_map(|index| format!("BinData/image{index}.png")),
        (0usize..32).prop_map(|index| format!("Preview/PrvText{index}.txt")),
    ]
}

fn parse_does_not_panic(data: &[u8]) -> bool {
    let parser = HwpParser::new();
    catch_unwind(AssertUnwindSafe(|| parser.parse(data))).is_ok()
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        failure_persistence: None,
        max_shrink_iters: 2048,
        ..ProptestConfig::default()
    })]

    #[test]
    fn arbitrary_bytes_do_not_panic(data in prop::collection::vec(any::<u8>(), 0..4096)) {
        prop_assert!(
            parse_does_not_panic(&data),
            "parser panicked for arbitrary {} byte input",
            data.len(),
        );
    }

    #[test]
    fn hwpx_like_zip_entries_do_not_panic(
        entries in prop::collection::vec(
            (hwpx_entry_path(), prop::collection::vec(any::<u8>(), 0..2048)),
            0..32,
        )
    ) {
        let data = zip_with_entries(&entries);

        prop_assert!(
            parse_does_not_panic(&data),
            "parser panicked for generated HWPX-like ZIP with {} entries",
            entries.len(),
        );
    }
}
