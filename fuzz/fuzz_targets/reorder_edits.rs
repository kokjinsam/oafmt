#![no_main]

use libfuzzer_sys::fuzz_target;
use oafmt_fuzz::check_reordering;

fuzz_target!(|data: &[u8]| {
    check_reordering(data);
});
