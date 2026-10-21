//! Seed corpora for the fuzz targets: small valid files made with LightCraft's own writers (DNG in
//! every coding, vendor-style TIFF raws, PNG, JPEG, TIFF, WebP, PSD), written to
//! `corpus/<target>/`. Run from `fuzz/`: `cargo run --example seeds`.

use std::path::Path;

use lightcraft_codecs::{ChromaSubsampling, EncodeImage, EncodeMeta, Samples, TiffCompression};
use lightcraft_raw::{BlackLevel, Cfa, ColorData, DngCompression, DngWriteOptions, OpcodeLists, Orientation, RawData, RawFormat, RawImage, Rect};
use lightcraft_tiff::{IfdBuilder, ImageData, TiffWriter, Value, tags as t};

fn raw(w: usize, h: usize, cpp: usize) -> RawImage {
    let data: Vec<u16> = (0..w * h * cpp).map(|i| (i * 97 % 4000) as u16).collect();
    RawImage {
        format: RawFormat::Dng,
        width: w,
        height: h,
        cpp,
        data: RawData::U16(data),
        cfa: (cpp == 1).then(|| Cfa::bayer("RGGB").expect("a Bayer pattern")),
        bits: 12,
        black: BlackLevel::uniform(64.0),
        white: vec![4095.0],
        active_area: Rect::new(0, 0, w, h),
        crop: Rect::new(0, 0, w, h),
        orientation: Orientation::Normal,
        color: ColorData::default(),
        wb_multipliers: None,
        linearized: false,
        pixel_aspect: 1.0,
        opcodes: OpcodeLists::default(),
        metadata: Default::default(),
        lens_metadata: Default::default(),
    }
}

fn dngs() -> Vec<(String, Vec<u8>)> {
    let codings = [
        ("plain", DngCompression::Uncompressed),
        ("lj92", DngCompression::Lj92 { tile: 16 }),
        ("deflate", DngCompression::Deflate { tile: 16, half: false }),
        ("lossy", DngCompression::LossyJpeg { tile: 16, quality: 80, long_edge: None }),
    ];
    let mut out = Vec::new();
    for (name, c) in codings {
        for (kind, cpp) in [("cfa", 1), ("linear", 3)] {
            let opts = DngWriteOptions { compression: c.clone(), ..Default::default() };
            if let Ok(b) = lightcraft_raw::write_dng(&raw(32, 24, cpp), &opts) {
                out.push((format!("dng-{name}-{kind}.dng"), b));
            }
        }
    }
    out
}

/// A one-strip TIFF raw as the vendor decoders look for it: `make`/`model`, a CFA image of `w × h`
/// coded `compression`, `strip` as its data.
fn vendor(make: &str, model: &str, compression: u16, bits: u16, w: u32, h: u32, strip: Vec<u8>) -> Vec<u8> {
    let mut ifd = IfdBuilder::new();
    ifd.set(t::MAKE, Value::Ascii(make.into()));
    ifd.set(t::MODEL, Value::Ascii(model.into()));
    ifd.set(t::IMAGE_WIDTH, Value::Long(vec![w]));
    ifd.set(t::IMAGE_LENGTH, Value::Long(vec![h]));
    ifd.set(t::BITS_PER_SAMPLE, Value::Short(vec![bits]));
    ifd.set(t::SAMPLES_PER_PIXEL, Value::Short(vec![1]));
    ifd.set(t::PHOTOMETRIC, Value::Short(vec![32803]));
    ifd.set(t::COMPRESSION, Value::Short(vec![compression]));
    ifd.set(t::CFA_REPEAT_PATTERN_DIM, Value::Short(vec![2, 2]));
    ifd.set(t::CFA_PATTERN_EP, Value::Byte(vec![0, 1, 1, 2]));
    ifd.set_image(ImageData::Strips { rows_per_strip: h, strips: vec![strip] });
    TiffWriter::default().write(&[ifd]).unwrap_or_default()
}

fn vendors() -> Vec<(String, Vec<u8>)> {
    let (w, h) = (32u32, 24u32);
    let n = (w * h) as usize;
    vec![
        ("sony-a200.arw".into(), vendor("SONY", "DSLR-A200", 32767, 12, w, h, vec![7; n])),
        ("sony-a900.arw".into(), vendor("SONY", "DSLR-A900", 32767, 12, w, h, vec![7; n * 3 / 2])),
        ("sony-plain.arw".into(), vendor("SONY", "ILCE-7M3", 1, 14, w, h, vec![7; n * 2])),
        ("nikon.nef".into(), vendor("NIKON CORPORATION", "NIKON D850", 1, 14, w, h, vec![7; n * 2])),
        ("olympus.orf".into(), vendor("OLYMPUS IMAGING CORP.", "E-M5", 1, 12, w, h, vec![7; n * 2])),
        ("pentax.pef".into(), vendor("PENTAX", "PENTAX K-1", 1, 14, w, h, vec![7; n * 2])),
        ("samsung.srw".into(), vendor("SAMSUNG", "NX1", 32769, 12, w, h, vec![7; n * 2])),
        ("leaf.mef".into(), vendor("Mamiya-OP Co.,Ltd.", "ZD", 1, 12, w, h, vec![7; n * 3 / 2])),
        ("epson.erf".into(), vendor("SEIKO EPSON CORP.", "R-D1", 1, 12, w, h, vec![7; n * 3 / 2])),
    ]
}

fn images() -> Vec<(String, Vec<u8>)> {
    let (w, h) = (17u32, 13u32);
    let rgb8: Vec<u8> = (0..w * h * 3).map(|i| (i * 31 % 251) as u8).collect();
    let rgba16: Vec<u16> = (0..w * h * 4).map(|i| (i * 977 % 65521) as u16).collect();
    let gray8: Vec<u8> = (0..w * h).map(|i| (i * 7 % 255) as u8).collect();
    let rgb = EncodeImage::new(w, h, 3, Samples::U8(&rgb8));
    let rgba = EncodeImage::new(w, h, 4, Samples::U16(&rgba16));
    let gray = EncodeImage::new(w, h, 1, Samples::U8(&gray8));
    let meta = EncodeMeta::default();
    let mut out: Vec<(String, lightcraft_codecs::Result<Vec<u8>>)> = vec![
        ("rgb.png".into(), lightcraft_codecs::encode_png(&rgb, &meta)),
        ("rgba16.png".into(), lightcraft_codecs::encode_png(&rgba, &meta)),
        ("gray.png".into(), lightcraft_codecs::encode_png(&gray, &meta)),
        ("rgb444.jpg".into(), lightcraft_codecs::encode_jpeg(&rgb, 85, ChromaSubsampling::S444, &meta)),
        ("rgb420.jpg".into(), lightcraft_codecs::encode_jpeg(&rgb, 60, ChromaSubsampling::S420, &meta)),
        ("rgb.webp".into(), lightcraft_codecs::encode_webp_lossless(&rgb, &meta)),
        ("rgb.psd".into(), lightcraft_codecs::encode_psd(&rgb, &meta)),
    ];
    for (name, c) in [("none", TiffCompression::None), ("lzw", TiffCompression::Lzw), ("deflate", TiffCompression::Deflate), ("packbits", TiffCompression::PackBits)]
    {
        out.push((format!("rgb-{name}.tif"), lightcraft_codecs::encode_tiff(&rgb, c, &meta)));
        out.push((format!("rgba16-{name}.tif"), lightcraft_codecs::encode_tiff(&rgba, c, &meta)));
    }
    out.into_iter().filter_map(|(n, r)| r.ok().map(|b| (n, b))).collect()
}

fn write(target: &str, files: &[(String, Vec<u8>)]) {
    let dir = Path::new("corpus").join(target);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("{}: {e}", dir.display());
        return;
    }
    for (name, bytes) in files {
        if let Err(e) = std::fs::write(dir.join(name), bytes) {
            eprintln!("{name}: {e}");
        }
    }
    println!("{target}: {} seeds", files.len());
}

fn main() {
    let raws: Vec<_> = dngs().into_iter().chain(vendors()).collect();
    let imgs = images();
    write("raw_decode", &raws);
    write("raw_probe", &raws);
    let all: Vec<_> = imgs.iter().cloned().chain(raws.iter().cloned()).collect();
    write("codecs_decode", &all);
    write("meta_extract", &all);
}
