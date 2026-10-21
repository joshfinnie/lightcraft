#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let _ = lightcraft_raw::probe(data);
    let _ = lightcraft_raw::probe_info(data);
});
