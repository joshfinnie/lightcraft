#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let _ = lightcraft_meta::extract(data);
    let _ = lightcraft_meta::read_exif(data);
    let _ = lightcraft_meta::parse_iptc(data);
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = lightcraft_meta::parse_xmp(s);
    }
});
