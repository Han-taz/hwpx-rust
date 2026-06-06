#![no_main]

use libfuzzer_sys::fuzz_target;

const MAX_FUZZ_INPUT_BYTES: usize = 2 * 1024 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_FUZZ_INPUT_BYTES {
        return;
    }

    let _ = hwp_core::parser::hwpx::parse(data);
});
