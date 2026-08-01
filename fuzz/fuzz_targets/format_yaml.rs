#![no_main]

use libfuzzer_sys::fuzz_target;
use oafmt_core::InputFormat;
use oafmt_fuzz::check_format;

fuzz_target!(|data: &[u8]| {
    check_format(data, InputFormat::Yaml);
});
