//! Locating metadata blocks inside image containers.
//!
//! - JPEG (ITU-T T.81 marker structure): APP1 `Exif\0\0`, APP1 `http://ns.adobe.com/xap/1.0/\0` (XMP),
//!   APP1 `http://ns.adobe.com/xmp/extension/\0` (extended XMP, reassembled), APP2 `ICC_PROFILE\0`
//!   (multi-chunk, reassembled in sequence order), APP13 `Photoshop 3.0\0` 8BIM resource 0x0404 (IPTC-IIM).
//! - PNG (PNG spec 3rd ed.): `eXIf`, `iTXt` with keyword `XML:com.adobe.xmp` (uncompressed).
//! - WebP (RIFF container spec): `EXIF`, `XMP `, `ICCP` chunks.
//! - TIFF-based files (TIFF, DNG and most raws): the whole file is the Exif block; XMP (700), ICC (34675) and
//!   IPTC (33723) from IFD0.
//! - Canon CR3 (ISO base media file, see [`crate::cr3`]): `CMT1`/`CMT2`/`CMT4` merged into one Exif block; the
//!   XMP `uuid` box.

use lightcraft_tiff::{Tiff, tags};

/// Metadata blocks found in a file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Embedded {
    /// TIFF-structured Exif block (without the `Exif\0\0` prefix).
    pub exif: Option<Vec<u8>>,
    /// XMP packet (the main one; extended XMP is appended by [`Embedded::xmp_extended`]).
    pub xmp: Option<String>,
    /// Extended XMP (JPEG only), reassembled.
    pub xmp_extended: Option<String>,
    pub icc: Option<Vec<u8>>,
    /// IPTC-IIM stream.
    pub iptc: Option<Vec<u8>>,
}

/// Sniff the container and collect its metadata blocks. Unknown containers yield an empty result.
pub fn embedded(bytes: &[u8]) -> Embedded {
    if bytes.starts_with(b"FUJIFILMCCD-RAW") {
        // Fujifilm RAF: metadata lives in the embedded JPEG whose (offset, length) are big-endian u32s at byte 84
        let be = |i: usize| bytes.get(i..i + 4)?.try_into().ok().map(|a| u32::from_be_bytes(a) as usize);
        return match (be(84), be(88)) {
            (Some(o), Some(l)) if o > 0 => bytes.get(o..o.saturating_add(l).min(bytes.len())).map(jpeg_segments).unwrap_or_default(),
            _ => Embedded::default(),
        };
    }
    if let Some(c) = crate::cr3::parse_cr3(bytes) {
        return Embedded { exif: crate::cr3::merged_exif(&c), xmp: c.xmp.map(|b| String::from_utf8_lossy(b).into_owned()), ..Default::default() };
    }
    if bytes.starts_with(&[0xff, 0xd8]) {
        jpeg_segments(bytes)
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        png_chunks(bytes)
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        webp_chunks(bytes)
    } else if let Ok(t) = Tiff::parse(bytes)
        && let Some(ifd0) = t.ifds.first()
    {
        Embedded {
            exif: Some(bytes.to_vec()),
            xmp: ifd0.bytes(tags::XMP).map(|b| String::from_utf8_lossy(b).into_owned()),
            xmp_extended: None,
            icc: ifd0.bytes(tags::ICC_PROFILE).map(|b| b.to_vec()),
            iptc: ifd0.value(tags::IPTC_NAA).map(|v| match v {
                lightcraft_tiff::Value::Byte(b) | lightcraft_tiff::Value::Undefined(b) => b.clone(),
                other => lightcraft_tiff::writer::encode(t.order, other),
            }),
        }
    } else {
        Embedded::default()
    }
}

const XMP_SIG: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const XMP_EXT_SIG: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";

/// Walk JPEG markers up to SOS and collect APP1 Exif / XMP, APP2 ICC and APP13 IPTC.
pub fn jpeg_segments(b: &[u8]) -> Embedded {
    let mut out = Embedded::default();
    if !b.starts_with(&[0xff, 0xd8]) {
        return out;
    }
    let mut icc_parts: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut ext: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut i = 2usize;
    while let Some(&[0xff, marker, l0, l1]) = b.get(i..i + 4) {
        if marker == 0xff {
            i += 1; // fill byte
            continue;
        }
        if marker == 0xd8 || (0xd0..=0xd7).contains(&marker) || marker == 0x01 {
            i += 2;
            continue;
        }
        if marker == 0xd9 || marker == 0xda {
            break;
        }
        let len = u16::from_be_bytes([l0, l1]) as usize;
        if len < 2 {
            break;
        }
        let end = (i + 2 + len).min(b.len());
        let payload = b.get(i + 4..end).unwrap_or_default();
        match marker {
            0xe1 if payload.starts_with(b"Exif\0") && out.exif.is_none() => out.exif = Some(payload.get(6..).unwrap_or(&[]).to_vec()),
            0xe1 if payload.starts_with(XMP_SIG) && out.xmp.is_none() => {
                out.xmp = payload.strip_prefix(XMP_SIG).map(|x| String::from_utf8_lossy(x).into_owned());
            }
            0xe1 if payload.starts_with(XMP_EXT_SIG) => {
                // GUID (32) + full length (4) + offset (4) + data
                if let Some((head, data)) = payload.get(XMP_EXT_SIG.len()..).and_then(|p| p.split_at_checked(40))
                    && let Some(off) = head.get(36..40).and_then(|o| o.try_into().ok())
                {
                    ext.push((u32::from_be_bytes(off), data.to_vec()));
                }
            }
            0xe2 if payload.starts_with(b"ICC_PROFILE\0") => {
                if let (Some(&seq), Some(data)) = (payload.get(12), payload.get(14..)) {
                    icc_parts.push((seq, data.to_vec()));
                }
            }
            0xed if out.iptc.is_none() => {
                if let Some(res) = payload.strip_prefix(b"Photoshop 3.0\0") {
                    out.iptc = photoshop_iptc(res);
                }
            }
            _ => {}
        }
        i = end;
    }
    if !icc_parts.is_empty() {
        icc_parts.sort_by_key(|p| p.0);
        out.icc = Some(icc_parts.into_iter().flat_map(|p| p.1).collect());
    }
    if !ext.is_empty() {
        ext.sort_by_key(|p| p.0);
        out.xmp_extended = Some(String::from_utf8_lossy(&ext.into_iter().flat_map(|p| p.1).collect::<Vec<u8>>()).into_owned());
    }
    out
}

/// IPTC-IIM block (resource 0x0404) from a Photoshop image-resource stream (`8BIM` records).
fn photoshop_iptc(mut b: &[u8]) -> Option<Vec<u8>> {
    while let Some(&[b'8', b'B', b'I', b'M', i0, i1, name_len, ..]) = b.get(..12) {
        let id = u16::from_be_bytes([i0, i1]);
        let name_len = name_len as usize;
        let name_total = (1 + name_len).next_multiple_of(2);
        let size_at = 6 + name_total;
        let size = u32::from_be_bytes(b.get(size_at..size_at + 4)?.try_into().ok()?) as usize;
        let data_at = size_at + 4;
        let data = b.get(data_at..data_at.checked_add(size)?)?;
        if id == 0x0404 {
            return Some(data.to_vec());
        }
        b = b.get(data_at + size.next_multiple_of(2)..)?;
    }
    None
}

/// PNG chunks: `eXIf`, uncompressed `iTXt` XMP.
pub fn png_chunks(b: &[u8]) -> Embedded {
    let mut out = Embedded::default();
    let mut i = 8usize;
    while let Some(&[l0, l1, l2, l3, t0, t1, t2, t3, ..]) = b.get(i..i.saturating_add(12)) {
        let len = u32::from_be_bytes([l0, l1, l2, l3]) as usize;
        let typ = &[t0, t1, t2, t3];
        let Some(data) = b.get(i + 8..(i + 8).saturating_add(len)) else { break };
        match typ {
            b"eXIf" => out.exif = Some(crate::strip_exif_header(data).to_vec()),
            b"iTXt" if data.starts_with(b"XML:com.adobe.xmp\0") => {
                // keyword\0 compression-flag compression-method language\0 translated\0 text
                if let Some(rest) = data.get(18..).and_then(|r| r.strip_prefix(&[0])).and_then(|r| r.get(1..)) {
                    let mut parts = rest.splitn(3, |&c| c == 0);
                    let (_, _, text) = (parts.next(), parts.next(), parts.next());
                    if let Some(t) = text {
                        out.xmp = Some(String::from_utf8_lossy(t).into_owned());
                    }
                }
            }
            b"IEND" => break,
            _ => {}
        }
        i = i.saturating_add(12).saturating_add(len);
    }
    out
}

/// WebP RIFF chunks `EXIF`, `XMP `, `ICCP`.
pub fn webp_chunks(b: &[u8]) -> Embedded {
    let mut out = Embedded::default();
    let mut i = 12usize;
    while let Some(&[i0, i1, i2, i3, l0, l1, l2, l3]) = b.get(i..i.saturating_add(8)) {
        let id = &[i0, i1, i2, i3];
        let len = u32::from_le_bytes([l0, l1, l2, l3]) as usize;
        let Some(data) = b.get(i + 8..(i + 8).saturating_add(len)) else { break };
        match id {
            b"EXIF" => out.exif = Some(crate::strip_exif_header(data).to_vec()),
            b"XMP " => out.xmp = Some(String::from_utf8_lossy(data).into_owned()),
            b"ICCP" => out.icc = Some(data.to_vec()),
            _ => {}
        }
        i = i.saturating_add(8).saturating_add(len).saturating_add(len & 1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exif::tests::sample_exif;
    use lightcraft_tiff::ByteOrder;

    fn seg(marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![0xff, marker];
        v.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        v.extend_from_slice(payload);
        v
    }

    pub(crate) fn sample_jpeg() -> Vec<u8> {
        let mut j = vec![0xff, 0xd8];
        j.extend(seg(0xe0, b"JFIF\0\x01\x02\0\0\x01\0\x01\0\0"));
        let mut exif = b"Exif\0\0".to_vec();
        exif.extend(sample_exif(ByteOrder::Big));
        j.extend(seg(0xe1, &exif));
        let meta = crate::Metadata { title: Some("Sunset".into()), rating: Some(4), ..Default::default() };
        let mut xmp = XMP_SIG.to_vec();
        xmp.extend(crate::write_xmp(&meta, Some("{\"exposure\":1.5}")).into_bytes());
        j.extend(seg(0xe1, &xmp));
        // ICC in two chunks, out of order
        j.extend(seg(0xe2, b"ICC_PROFILE\0\x02\x02WORLD"));
        j.extend(seg(0xe2, b"ICC_PROFILE\0\x01\x02HELLO"));
        // IPTC
        let iim = crate::iptc::tests::sample_iim();
        let mut ps = b"Photoshop 3.0\0".to_vec();
        ps.extend_from_slice(b"8BIM\x04\x0c\0\0");
        ps.extend_from_slice(&2u32.to_be_bytes());
        ps.extend_from_slice(&[1, 2]);
        ps.extend_from_slice(b"8BIM\x04\x04\0\0");
        ps.extend_from_slice(&(iim.len() as u32).to_be_bytes());
        ps.extend_from_slice(&iim);
        j.extend(seg(0xed, &ps));
        j.extend_from_slice(&[0xff, 0xda, 0, 2, 1, 2, 3, 0xff, 0xd9]);
        j
    }

    #[test]
    fn jpeg_blocks() {
        let j = sample_jpeg();
        let e = jpeg_segments(&j);
        assert_eq!(e.icc.as_deref(), Some(&b"HELLOWORLD"[..]));
        assert!(e.xmp.as_deref().unwrap().contains("Sunset"));
        assert!(e.iptc.is_some());
        let m = crate::extract(&j);
        assert_eq!(m.iso, Some(400));
        assert_eq!(m.title.as_deref(), Some("Sunset"));
        assert_eq!(m.rating, Some(4)); // XMP overrides EXIF rating 3
        assert_eq!(m.keywords, vec!["alpha".to_string(), "beta".to_string()]); // from IPTC
        assert_eq!(m.caption.as_deref(), Some("A caption"));
    }

    #[test]
    fn jpeg_extended_xmp() {
        let mut j = vec![0xff, 0xd8];
        for (off, part) in [(5u32, &b"world"[..]), (0, b"hello")] {
            let mut p = XMP_EXT_SIG.to_vec();
            p.extend_from_slice(&[b'A'; 32]);
            p.extend_from_slice(&10u32.to_be_bytes());
            p.extend_from_slice(&off.to_be_bytes());
            p.extend_from_slice(part);
            j.extend(seg(0xe1, &p));
        }
        assert_eq!(jpeg_segments(&j).xmp_extended.as_deref(), Some("helloworld"));
    }

    #[test]
    fn png_and_webp() {
        let exif = sample_exif(ByteOrder::Little);
        let mut p = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut chunk = |typ: &[u8], data: &[u8]| {
            p.extend_from_slice(&(data.len() as u32).to_be_bytes());
            p.extend_from_slice(typ);
            p.extend_from_slice(data);
            p.extend_from_slice(&[0; 4]);
        };
        chunk(b"eXIf", &exif);
        let mut itxt = b"XML:com.adobe.xmp\0\0\0\0\0".to_vec();
        itxt.extend(crate::write_xmp(&crate::Metadata { label: Some("Red".into()), ..Default::default() }, None).into_bytes());
        chunk(b"iTXt", &itxt);
        chunk(b"IEND", &[]);
        let m = crate::extract(&p);
        assert_eq!(m.iso, Some(400));
        assert_eq!(m.label.as_deref(), Some("Red"));

        let mut w = b"RIFF\0\0\0\0WEBP".to_vec();
        let mut body = |id: &[u8], data: &[u8]| {
            w.extend_from_slice(id);
            w.extend_from_slice(&(data.len() as u32).to_le_bytes());
            w.extend_from_slice(data);
            if data.len() % 2 == 1 {
                w.push(0);
            }
        };
        body(b"VP8X", &[0; 10]);
        body(b"ICCP", b"abc");
        let mut ex = b"Exif\0\0".to_vec();
        ex.extend_from_slice(&exif);
        body(b"EXIF", &ex);
        let e = webp_chunks(&w);
        assert_eq!(e.icc.as_deref(), Some(&b"abc"[..]));
        assert_eq!(crate::extract(&w).f_number, Some(2.8));
    }

    #[test]
    fn tiff_file_is_its_own_exif() {
        let t = sample_exif(ByteOrder::Big);
        assert_eq!(crate::extract(&t).model.as_deref(), Some("Model X"));
        assert_eq!(embedded(b"not an image"), Embedded::default());
    }

    #[test]
    fn raf_reads_its_embedded_jpeg() {
        let j = sample_jpeg();
        let mut raf = b"FUJIFILMCCD-RAW 0201FF000000TEST".to_vec();
        raf.resize(120, 0);
        raf[84..88].copy_from_slice(&120u32.to_be_bytes());
        raf[88..92].copy_from_slice(&(j.len() as u32).to_be_bytes());
        raf.extend_from_slice(&j);
        assert_eq!(crate::extract(&raf).iso, Some(400));
        for n in 0..raf.len() {
            let _ = crate::extract(&raf[..n]);
        }
    }

    #[test]
    fn truncated_containers_do_not_panic() {
        let j = sample_jpeg();
        for n in 0..j.len() {
            let _ = crate::extract(&j[..n]);
        }
    }
}
