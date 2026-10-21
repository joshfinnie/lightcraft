#![no_main]

// what opening a raw does: read it, its embedded preview, and develop it (kept small, so a run
// tries many files rather than developing one large one)
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let _ = lightcraft_raw::embedded_preview(data);
    if let Ok(img) = lightcraft_raw::decode(data)
        && img.width * img.height <= 1 << 16
    {
        let _ = img.develop(lightcraft_raw::Method::Ahd);
        let xy = lightcraft_raw::color::as_shot_white_xy(&img);
        let _ = lightcraft_raw::color::camera_transform(&img, xy);
    }
});
