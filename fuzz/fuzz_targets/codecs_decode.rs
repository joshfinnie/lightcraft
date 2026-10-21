#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let _ = lightcraft_codecs::decode(data, Default::default());
});
