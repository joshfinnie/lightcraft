//! Parallel baseline JPEG encoder (ITU-T T.81 / JFIF).
//!
//! The image is cut into bands of MCU rows separated by restart markers (`DRI` + `RSTn`): DC
//! prediction restarts at every marker, so bands are entropy-coded independently on all cores and
//! concatenated. Quantization uses the example tables of T.81 Annex K scaled by the usual 1–100
//! quality convention. Huffman tables are made for each image from its own symbol counts (T.81
//! Annex K.2): a first pass over the bands counts the symbols, a second codes them. The file stays
//! interleaved baseline with the tables in its `DHT` segments, readable by every decoder, and is
//! smaller than with the Annex K example tables at the same pixels. Supports grayscale and YCbCr
//! 4:4:4 / 4:2:0.
#![expect(
    clippy::indexing_slicing,
    reason = "encodes buffers `encode` has checked: fixed tables indexed by masked symbols and zig-zag positions, planes and blocks sized from the checked dimensions"
)]

use crate::encode::ChromaSubsampling;

/// T.81 Annex K.1 / K.2 example quantization tables (natural order).
const Q_LUMA: [u16; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56, 14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56,
    68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113, 92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];
const Q_CHROMA: [u16; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99, 24, 26, 56, 99, 99, 99, 99, 99, 47, 66, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
];

/// Zig-zag position k → natural index.
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43,
    36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// T.81 Annex K.3 example Huffman tables: (code counts per length 1..16, symbols).
const DC_LUMA_BITS: [u8; 16] = [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
const DC_CHROMA_BITS: [u8; 16] = [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0];
const DC_VALS: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const AC_LUMA_BITS: [u8; 16] = [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d];
const AC_LUMA_VALS: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08,
    0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28,
    0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59,
    0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
    0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6,
    0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2,
    0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa,
];
const AC_CHROMA_BITS: [u8; 16] = [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77];
const AC_CHROMA_VALS: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71, 0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91,
    0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0, 0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18, 0x19, 0x1a, 0x26,
    0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58,
    0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87,
    0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4,
    0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda,
    0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa,
];

/// Huffman code table: symbol → (code, length).
struct Huff {
    code: [(u16, u8); 256],
}

impl Huff {
    /// Canonical codes from counts (T.81 Annex C).
    fn new(bits: &[u8; 16], vals: &[u8]) -> Huff {
        let mut code = [(0u16, 0u8); 256];
        let (mut c, mut k) = (0u16, 0usize);
        for (len, &n) in bits.iter().enumerate() {
            for _ in 0..n {
                if let Some(&v) = vals.get(k) {
                    code[v as usize] = (c, len as u8 + 1);
                }
                c = c.wrapping_add(1);
                k += 1;
            }
            c = c.wrapping_shl(1);
        }
        Huff { code }
    }
}

/// Symbol counts of one Huffman table: index = symbol (DC size category, or AC run/size byte).
type Freq = [u64; 256];

/// The code lengths (`BITS`, counts per length 1..16) and symbols (`HUFFVAL`) of an optimal
/// Huffman table for `freq`, limited to 16-bit codes and never assigning the all-ones code
/// (T.81 Annex K.2, figures K.1–K.4). Symbols with a zero count get no code. At least one symbol
/// is always present (a table nothing used gets symbol 0), so the result is a valid `DHT`.
fn optimal_table(freq: &Freq) -> ([u8; 16], Vec<u8>) {
    // 256 is the reserved pseudo-symbol: counted once, it takes the longest code, which is
    // then dropped so no real code is all ones (K.2)
    let mut f = [0u64; 257];
    f[..256].copy_from_slice(freq);
    if f[..256].iter().all(|&c| c == 0) {
        f[0] = 1;
    }
    f[256] = 1;
    let mut size = [0usize; 257];
    let mut next: [Option<usize>; 257] = [None; 257];
    loop {
        // the two smallest non-zero counts; on equal counts the larger symbol goes first (K.1)
        let mut c1: Option<usize> = None;
        for i in 0..257 {
            if f[i] > 0 && c1.is_none_or(|c| f[i] <= f[c]) {
                c1 = Some(i);
            }
        }
        let Some(c1) = c1 else { break };
        let mut c2: Option<usize> = None;
        for i in 0..257 {
            if i != c1 && f[i] > 0 && c2.is_none_or(|c| f[i] <= f[c]) {
                c2 = Some(i);
            }
        }
        let Some(c2) = c2 else { break };
        f[c1] = f[c1].saturating_add(f[c2]);
        f[c2] = 0;
        // every symbol in both trees goes one level deeper; c2's chain is appended to c1's
        let mut k = c1;
        size[k] += 1;
        while let Some(n) = next[k] {
            k = n;
            size[k] += 1;
        }
        next[k] = Some(c2);
        let mut k = c2;
        size[k] += 1;
        while let Some(n) = next[k] {
            k = n;
            size[k] += 1;
        }
    }
    // counts per code length; 257 symbols need at most 256 levels
    let mut bits = [0usize; 257];
    for &s in &size {
        if s > 0 {
            bits[s.min(256)] += 1;
        }
    }
    // limit to 16 bits (K.3): move pairs of the longest codes up, one level at a time (the
    // deepest level of a full tree always holds an even number of codes)
    for i in (17..257).rev() {
        while bits[i] >= 2 {
            let Some(j) = (1..i - 1).rev().find(|&j| bits[j] > 0) else { break };
            bits[i] -= 2;
            bits[i - 1] += 1;
            bits[j + 1] += 2;
            bits[j] -= 1;
        }
    }
    // drop the reserved symbol's code: it is one of the longest
    if let Some(i) = (1..=16).rev().find(|&i| bits[i] > 0) {
        bits[i] -= 1;
    }
    // symbols by code length, then by value (K.4); the reserved symbol is not listed
    let mut vals = Vec::new();
    for len in 1..257 {
        vals.extend((0..256).filter(|&s| size[s] == len).map(|s| s as u8));
    }
    let mut out = [0u8; 16];
    for (o, &b) in out.iter_mut().zip(&bits[1..=16]) {
        *o = b.min(255) as u8;
    }
    (out, vals)
}

fn scaled_quant(base: &[u16; 64], quality: u8) -> [u16; 64] {
    let q = quality.clamp(1, 100) as u32;
    let scale = if q < 50 { 5000 / q } else { 200 - 2 * q };
    base.map(|b| ((b as u32 * scale + 50) / 100).clamp(1, 255) as u16)
}

/// Entropy-coded segment writer with 0xFF byte stuffing.
struct Bits {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl Bits {
    fn new() -> Bits {
        Bits { out: Vec::new(), acc: 0, n: 0 }
    }
    #[inline]
    fn put(&mut self, code: u32, len: u32) {
        if len == 0 {
            return;
        }
        self.acc = (self.acc << len) | (code & ((1 << len) - 1));
        self.n += len;
        while self.n >= 8 {
            let b = (self.acc >> (self.n - 8)) as u8;
            self.out.push(b);
            if b == 0xFF {
                self.out.push(0);
            }
            self.n -= 8;
        }
        self.acc &= (1 << self.n) - 1;
    }
    /// Pad with 1-bits to a byte boundary.
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            let pad = 8 - self.n;
            self.put((1 << pad) - 1, pad);
        }
        self.out
    }
}

/// AAN output scale factors: `s[0] = 1`, `s[k] = cos(kπ/16)·√2`.
fn aan_scale() -> [f32; 8] {
    std::array::from_fn(|k| if k == 0 { 1.0 } else { ((k as f64 * std::f64::consts::PI / 16.0).cos() * std::f64::consts::SQRT_2) as f32 })
}

/// Quantization reciprocals with the AAN scaling folded in: coefficient = out / (8·s[u]·s[v]·q).
fn recip_table(q: &[u16; 64]) -> [f32; 64] {
    let s = aan_scale();
    std::array::from_fn(|i| 1.0 / (q[i] as f32 * s[i / 8] * s[i % 8] * 8.0))
}

/// One 8-point forward DCT (Arai–Agui–Nakajima flowgraph, 5 multiplies) on `d[o + k·stride]`.
#[inline(always)]
fn aan_1d(d: &mut [f32; 64], o: usize, stride: usize) {
    let g = |k: usize| o + k * stride;
    let (t0, t7) = (d[g(0)] + d[g(7)], d[g(0)] - d[g(7)]);
    let (t1, t6) = (d[g(1)] + d[g(6)], d[g(1)] - d[g(6)]);
    let (t2, t5) = (d[g(2)] + d[g(5)], d[g(2)] - d[g(5)]);
    let (t3, t4) = (d[g(3)] + d[g(4)], d[g(3)] - d[g(4)]);
    // even part
    let (t10, t13, t11, t12) = (t0 + t3, t0 - t3, t1 + t2, t1 - t2);
    d[g(0)] = t10 + t11;
    d[g(4)] = t10 - t11;
    let z1 = (t12 + t13) * 0.707_106_77;
    d[g(2)] = t13 + z1;
    d[g(6)] = t13 - z1;
    // odd part
    let (t10, t11, t12) = (t4 + t5, t5 + t6, t6 + t7);
    let z5 = (t10 - t12) * 0.382_683_43;
    let z2 = 0.541_196_1 * t10 + z5;
    let z4 = 1.306_563 * t12 + z5;
    let z3 = t11 * 0.707_106_77;
    let (z11, z13) = (t7 + z3, t7 - z3);
    d[g(5)] = z13 + z2;
    d[g(3)] = z13 - z2;
    d[g(1)] = z11 + z4;
    d[g(7)] = z11 - z4;
}

/// Forward DCT of a level-shifted 8×8 block and quantization.
#[inline]
fn fdct_quant(block: &[f32; 64], recip: &[f32; 64], out: &mut [i16; 64]) {
    let mut d = *block;
    for r in 0..8 {
        aan_1d(&mut d, r * 8, 1);
    }
    for c in 0..8 {
        aan_1d(&mut d, c, 8);
    }
    for i in 0..64 {
        out[i] = (d[i] * recip[i]).round() as i16;
    }
}

#[inline]
fn magnitude(v: i32) -> (u32, u32) {
    let a = v.unsigned_abs();
    let size = 32 - a.leading_zeros();
    let bits = if v < 0 { (v - 1) as u32 } else { v as u32 };
    (size, bits & ((1u32 << size) - 1))
}

/// The Huffman symbols of one quantized block in coding order: `sym(ac, symbol, bits, n)` for the
/// DC size category (`ac` false) and each AC run/size, ZRL and EOB (`ac` true), each followed by
/// its `n` extra bits `bits`.
#[inline(always)]
fn block_symbols(q: &[i16; 64], pred: &mut i32, mut sym: impl FnMut(bool, usize, u32, u32)) {
    let d = q[0] as i32 - *pred;
    *pred = q[0] as i32;
    let (s, b) = magnitude(d);
    sym(false, s as usize, b, s);
    let mut run = 0u32;
    for &zi in &ZIGZAG[1..] {
        let v = q[zi] as i32;
        if v == 0 {
            run += 1;
            continue;
        }
        while run >= 16 {
            sym(true, 0xF0, 0, 0);
            run -= 16;
        }
        let (s, b) = magnitude(v);
        sym(true, ((run << 4) | s) as usize, b, s);
        run = 0;
    }
    if run > 0 {
        sym(true, 0x00, 0, 0);
    }
}

/// One coded symbol of the entropy-coded data: bits 0–7 the symbol, 8–9 its table (0 DC luma,
/// 1 AC luma, 2 DC chroma, 3 AC chroma), 10–13 the number of extra bits, 14–29 those bits (a
/// baseline coefficient needs at most 11).
type Token = u32;

/// Append one block's symbols to `out` and count them in `freq` (by table, as in [`Token`]).
fn tokenize_block(q: &[i16; 64], pred: &mut i32, chroma: bool, out: &mut Vec<Token>, freq: &mut [Freq; 4]) {
    let base = if chroma { 2 } else { 0 };
    block_symbols(q, pred, |is_ac, s, bits, n| {
        let table = base + is_ac as usize;
        freq[table][s & 0xFF] += 1;
        out.push((s as u32 & 0xFF) | (table as u32) << 8 | (n & 0xF) << 10 | (bits & 0xFFFF) << 14);
    });
}

/// Code a band's symbols with `tables` (indexed as in [`Token`]).
fn write_tokens(tokens: &[Token], tables: &[Huff; 4]) -> Vec<u8> {
    let mut w = Bits::new();
    for &t in tokens {
        let (s, table, n, bits) = (t & 0xFF, (t >> 8) & 3, (t >> 10) & 0xF, t >> 14);
        let (c, l) = tables[table as usize].code[s as usize];
        w.put(c as u32, l as u32);
        w.put(bits, n);
    }
    w.finish()
}

/// The image being coded, read a block at a time: its samples are converted (JFIF YCbCr, full
/// range, level-shifted to −128..127) as the blocks are coded, so no plane of the whole image is
/// ever held (three float planes are three times a 24 MP image). Reads past the right and bottom
/// edges repeat the last column and row.
struct Source<'a> {
    data: &'a [u8],
    w: usize,
    h: usize,
    ch: usize,
    gray: bool,
}

impl Source<'_> {
    /// The red, green and blue of pixel (`x`, `y`) of a colour image, clamped to the image.
    #[inline]
    fn rgb(&self, x: usize, y: usize) -> [f32; 3] {
        let i = (y.min(self.h - 1) * self.w + x.min(self.w - 1)) * self.ch;
        match self.data.get(i..i + 3) {
            Some(&[r, g, b]) => [r as f32, g as f32, b as f32],
            _ => [0.0; 3],
        }
    }

    #[inline]
    fn luma(&self, x: usize, y: usize) -> f32 {
        if self.gray {
            let i = y.min(self.h - 1) * self.w + x.min(self.w - 1);
            return self.data.get(i).map_or(0.0, |v| *v as f32) - 128.0;
        }
        let [r, g, b] = self.rgb(x, y);
        0.299 * r + 0.587 * g + 0.114 * b - 128.0
    }

    /// Block (`bx`, `by`) of the luma.
    fn luma_block(&self, bx: usize, by: usize, out: &mut [f32; 64]) {
        for (k, o) in out.iter_mut().enumerate() {
            *o = self.luma(bx * 8 + k % 8, by * 8 + k / 8);
        }
    }

    /// Block (`bx`, `by`) of every component at full resolution (4:4:4; only the luma of a gray
    /// image).
    fn blocks(&self, bx: usize, by: usize, out: &mut [[f32; 64]; 3]) {
        if self.gray {
            return self.luma_block(bx, by, &mut out[0]);
        }
        for k in 0..64 {
            let [r, g, b] = self.rgb(bx * 8 + k % 8, by * 8 + k / 8);
            out[0][k] = 0.299 * r + 0.587 * g + 0.114 * b - 128.0;
            out[1][k] = -0.168_736 * r - 0.331_264 * g + 0.5 * b;
            out[2][k] = 0.5 * r - 0.418_688 * g - 0.081_312 * b;
        }
    }

    /// Block (`bx`, `by`) of the 4:2:0 chroma. The conversion is linear: the RGB of each 2×2 is
    /// averaged first, then converted once. The chroma is `w/2 × h/2` (rounded up) and repeats
    /// its own last column and row.
    fn chroma_blocks_420(&self, bx: usize, by: usize, cb: &mut [f32; 64], cr: &mut [f32; 64]) {
        let (cw, chh) = (self.w.div_ceil(2), self.h.div_ceil(2));
        for k in 0..64 {
            let (cx, cy) = ((bx * 8 + k % 8).min(cw - 1), (by * 8 + k / 8).min(chh - 1));
            let (x0, x1, y0, y1) = (2 * cx, 2 * cx + 1, 2 * cy, 2 * cy + 1);
            let (a, b, c, d) = (self.rgb(x0, y0), self.rgb(x1, y0), self.rgb(x0, y1), self.rgb(x1, y1));
            let s = |i: usize| (a[i] + b[i] + c[i] + d[i]) * 0.25;
            let (r, g, b) = (s(0), s(1), s(2));
            cb[k] = -0.168_736 * r - 0.331_264 * g + 0.5 * b;
            cr[k] = 0.5 * r - 0.418_688 * g - 0.081_312 * b;
        }
    }
}

fn segment(out: &mut Vec<u8>, marker: u8, payload: &[u8]) {
    out.extend_from_slice(&[0xFF, marker]);
    out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(payload);
}

fn dht(out: &mut Vec<u8>, class_id: u8, bits: &[u8; 16], vals: &[u8]) {
    let mut p = vec![class_id];
    p.extend_from_slice(bits);
    p.extend_from_slice(vals);
    segment(out, 0xC4, &p);
}

/// Encode interleaved 8-bit samples (`channels` 1, 3 or 4; alpha is dropped), with Huffman
/// tables made for the image. An error for no pixels, more than 65535 on a side, another channel
/// count, or fewer samples than `width × height × channels`.
pub fn encode(
    data: &[u8],
    width: usize,
    height: usize,
    channels: usize,
    quality: u8,
    sub: ChromaSubsampling,
    app_segments: &[(u8, Vec<u8>)],
) -> crate::Result<Vec<u8>> {
    let bad = |why: &str| Err(crate::Error::Encode(format!("JPEG: {why}")));
    if width == 0 || height == 0 || width > u16::MAX as usize || height > u16::MAX as usize {
        return bad("dimensions must be 1 to 65535");
    }
    if !matches!(channels, 1 | 3 | 4) {
        return bad("1, 3 or 4 channels");
    }
    if width.checked_mul(height).and_then(|n| n.checked_mul(channels)).is_none_or(|n| data.len() < n) {
        return bad("fewer samples than the dimensions need");
    }
    Ok(encode_with(data, width, height, channels, quality, sub, app_segments, true))
}

/// [`encode`], with optimized Huffman tables or (`optimize` false) the Annex K example tables.
#[allow(clippy::too_many_arguments)]
fn encode_with(
    data: &[u8],
    width: usize,
    height: usize,
    channels: usize,
    quality: u8,
    sub: ChromaSubsampling,
    app_segments: &[(u8, Vec<u8>)],
    optimize: bool,
) -> Vec<u8> {
    let gray = channels == 1;
    let (hs, vs) = match (gray, sub) {
        (true, _) | (false, ChromaSubsampling::S444) => (1usize, 1usize),
        (false, _) => (2, 2),
    };
    #[cfg(feature = "parallel")]
    use rayon::prelude::*;
    let src = Source { data, w: width, h: height, ch: channels, gray };
    let ql = scaled_quant(&Q_LUMA, quality);
    let qc = scaled_quant(&Q_CHROMA, quality);
    let (rl, rc) = (recip_table(&ql), recip_table(&qc));
    let (mcu_w, mcu_h) = (8 * hs, 8 * vs);
    let (mx, my) = (width.div_ceil(mcu_w), height.div_ceil(mcu_h));
    // restart interval: whole MCU rows per band, ≤ 65535 MCUs, enough bands to keep all cores busy
    let rows_per_band = (my / 64).clamp(1, (65_535 / mx.max(1)).max(1));
    let bands: Vec<usize> = (0..my.div_ceil(rows_per_band)).collect();
    // first pass: each band's quantized blocks as symbols, with their counts per table (DC
    // prediction restarts per band, as the restart markers say)
    let tokenize_band = |band: &usize| -> (Vec<Token>, [Freq; 4]) {
        let mut q = [0i16; 64];
        let mut blk = [[0f32; 64]; 3];
        let (mut tokens, mut freq) = (Vec::new(), [[0u64; 256]; 4]);
        let mut pred = [0i32; 3];
        for mcu_y in band * rows_per_band..((band + 1) * rows_per_band).min(my) {
            for mcu_x in 0..mx {
                if hs == 1 {
                    // 4:4:4 (and gray): one block of each component, converted together
                    src.blocks(mcu_x, mcu_y, &mut blk);
                    fdct_quant(&blk[0], &rl, &mut q);
                    tokenize_block(&q, &mut pred[0], false, &mut tokens, &mut freq);
                } else {
                    for vy in 0..vs {
                        for hx in 0..hs {
                            src.luma_block(mcu_x * hs + hx, mcu_y * vs + vy, &mut blk[0]);
                            fdct_quant(&blk[0], &rl, &mut q);
                            tokenize_block(&q, &mut pred[0], false, &mut tokens, &mut freq);
                        }
                    }
                    let [_, cb, cr] = &mut blk;
                    src.chroma_blocks_420(mcu_x, mcu_y, cb, cr);
                }
                if !gray {
                    for (b, pred) in blk.iter().zip(pred.iter_mut()).skip(1) {
                        fdct_quant(b, &rc, &mut q);
                        tokenize_block(&q, pred, true, &mut tokens, &mut freq);
                    }
                }
            }
        }
        (tokens, freq)
    };
    #[cfg(feature = "parallel")]
    let tokenized: Vec<(Vec<Token>, [Freq; 4])> = bands.par_iter().map(tokenize_band).collect();
    #[cfg(not(feature = "parallel"))]
    let tokenized: Vec<(Vec<Token>, [Freq; 4])> = bands.iter().map(tokenize_band).collect();
    // the tables: DC luma, AC luma, DC chroma, AC chroma
    let tables: [([u8; 16], Vec<u8>); 4] = if optimize {
        let mut total = [[0u64; 256]; 4];
        for (_, f) in &tokenized {
            for (t, f) in total.iter_mut().zip(f) {
                for (t, f) in t.iter_mut().zip(f) {
                    *t = t.saturating_add(*f);
                }
            }
        }
        total.each_ref().map(optimal_table)
    } else {
        [
            (DC_LUMA_BITS, DC_VALS.to_vec()),
            (AC_LUMA_BITS, AC_LUMA_VALS.to_vec()),
            (DC_CHROMA_BITS, DC_VALS.to_vec()),
            (AC_CHROMA_BITS, AC_CHROMA_VALS.to_vec()),
        ]
    };
    let huff = tables.each_ref().map(|(bits, vals)| Huff::new(bits, vals));
    // second pass: code the symbols
    #[cfg(feature = "parallel")]
    let coded: Vec<Vec<u8>> = tokenized.into_par_iter().map(|(t, _)| write_tokens(&t, &huff)).collect();
    #[cfg(not(feature = "parallel"))]
    let coded: Vec<Vec<u8>> = tokenized.into_iter().map(|(t, _)| write_tokens(&t, &huff)).collect();

    // headers
    let mut out = Vec::with_capacity(coded.iter().map(Vec::len).sum::<usize>() + 4096);
    out.extend_from_slice(&[0xFF, 0xD8]);
    // JFIF APP0 first (callers may pass their own, e.g. with a print density)
    if !app_segments.iter().any(|(m, _)| *m == 0xE0) {
        segment(&mut out, 0xE0, &[b'J', b'F', b'I', b'F', 0, 1, 2, 0, 0, 1, 0, 1, 0, 0]);
    }
    for (marker, payload) in app_segments {
        segment(&mut out, *marker, payload);
    }
    let dqt = |id: u8, q: &[u16; 64]| -> Vec<u8> {
        let mut p = vec![id];
        p.extend(ZIGZAG.iter().map(|&i| q[i] as u8));
        p
    };
    segment(&mut out, 0xDB, &dqt(0, &ql));
    if !gray {
        segment(&mut out, 0xDB, &dqt(1, &qc));
    }
    let mut sof = vec![8];
    sof.extend_from_slice(&(height as u16).to_be_bytes());
    sof.extend_from_slice(&(width as u16).to_be_bytes());
    if gray {
        sof.extend_from_slice(&[1, 1, 0x11, 0]);
    } else {
        sof.extend_from_slice(&[3, 1, ((hs as u8) << 4) | vs as u8, 0, 2, 0x11, 1, 3, 0x11, 1]);
    }
    segment(&mut out, 0xC0, &sof);
    let [(dlb, dlv), (alb, alv), (dcb, dcv), (acb, acv)] = &tables;
    dht(&mut out, 0x00, dlb, dlv);
    dht(&mut out, 0x10, alb, alv);
    if !gray {
        dht(&mut out, 0x01, dcb, dcv);
        dht(&mut out, 0x11, acb, acv);
    }
    let interval = (rows_per_band * mx) as u16;
    if coded.len() > 1 {
        segment(&mut out, 0xDD, &interval.to_be_bytes());
    }
    if gray {
        segment(&mut out, 0xDA, &[1, 1, 0x00, 0, 63, 0]);
    } else {
        segment(&mut out, 0xDA, &[3, 1, 0x00, 2, 0x11, 3, 0x11, 0, 63, 0]);
    }
    let last = coded.len().saturating_sub(1);
    for (i, c) in coded.iter().enumerate() {
        out.extend_from_slice(c);
        if i < last {
            out.extend_from_slice(&[0xFF, 0xD0 + (i % 8) as u8]);
        }
    }
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Buffers that don't match their dimensions, no pixels, or an unsupported channel count are refused instead of
    /// panicking in the band loops.
    #[test]
    fn mismatched_buffers_are_refused() {
        let s444 = ChromaSubsampling::S444;
        assert!(encode(&[0; 12], 2, 2, 3, 90, s444, &[]).is_ok());
        assert!(encode(&[0; 11], 2, 2, 3, 90, s444, &[]).is_err(), "short buffer");
        assert!(encode(&[], 0, 2, 3, 90, s444, &[]).is_err(), "no width");
        assert!(encode(&[0; 8], 2, 2, 2, 90, s444, &[]).is_err(), "two channels");
        assert!(encode(&[0; 4], 2, 2, 0, 90, s444, &[]).is_err(), "no channels");
        assert!(encode(&[0; 3], 70_000, 1, 1, 90, s444, &[]).is_err(), "too wide");
        assert!(encode(&[0; 3], usize::MAX, 2, 3, 90, s444, &[]).is_err(), "size overflow");
    }

    fn test_rgb(w: usize, h: usize) -> Vec<u8> {
        let mut v = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                let t = ((x as f32 / 13.0).sin() * 0.5 + 0.5) * 255.0;
                v.extend_from_slice(&[(x * 255 / w) as u8, (y * 255 / h) as u8, t as u8]);
            }
        }
        v
    }

    fn psnr(a: &[u8], b: &[u8]) -> f64 {
        let mse = a.iter().zip(b).map(|(x, y)| (*x as f64 - *y as f64).powi(2)).sum::<f64>() / a.len() as f64;
        10.0 * (255.0f64.powi(2) / mse.max(1e-9)).log10()
    }

    /// The AAN path matches the direct DCT-II definition.
    #[test]
    fn aan_matches_reference_dct() {
        let block: [f32; 64] = std::array::from_fn(|i| ((i * 37 % 255) as f32) - 128.0);
        let q = [1u16; 64];
        let mut out = [0i16; 64];
        fdct_quant(&block, &recip_table(&q), &mut out);
        for v in 0..8 {
            for u in 0..8 {
                let c = |k: usize| if k == 0 { std::f64::consts::FRAC_1_SQRT_2 } else { 1.0 };
                let mut s = 0.0;
                for y in 0..8 {
                    for x in 0..8 {
                        s += block[y * 8 + x] as f64
                            * (((2 * x + 1) * u) as f64 * std::f64::consts::PI / 16.0).cos()
                            * (((2 * y + 1) * v) as f64 * std::f64::consts::PI / 16.0).cos();
                    }
                }
                let f = 0.25 * c(u) * c(v) * s;
                assert!((out[v * 8 + u] as f64 - f).abs() <= 0.51, "({u},{v}): {} vs {f}", out[v * 8 + u]);
            }
        }
    }

    #[test]
    fn huffman_tables_are_consistent() {
        for (bits, n) in [(&AC_LUMA_BITS, AC_LUMA_VALS.len()), (&AC_CHROMA_BITS, AC_CHROMA_VALS.len()), (&DC_LUMA_BITS, 12), (&DC_CHROMA_BITS, 12)] {
            assert_eq!(bits.iter().map(|&b| b as usize).sum::<usize>(), n);
        }
    }

    #[test]
    fn decodes_with_independent_decoders_at_odd_sizes() {
        for (w, h, sub) in [
            (1, 1, ChromaSubsampling::S444),
            (37, 23, ChromaSubsampling::S420),
            (640, 1031, ChromaSubsampling::S420),
            (803, 600, ChromaSubsampling::S444),
        ] {
            let src = test_rgb(w, h);
            let jpg = encode(&src, w, h, 3, 92, sub, &[]).unwrap();
            let mut d = jpeg_decoder::Decoder::new(&jpg[..]);
            let px = d.decode().expect("jpeg-decoder");
            let info = d.info().unwrap();
            assert_eq!((info.width as usize, info.height as usize), (w, h));
            if w * h > 64 {
                assert!(psnr(&src, &px) > 30.0, "{w}x{h} {sub:?}: psnr {}", psnr(&src, &px));
            }
            let z = zune_jpeg::JpegDecoder::new(zune_core::bytestream::ZCursor::new(&jpg[..])).decode().expect("zune-jpeg");
            assert_eq!(z.len(), px.len());
        }
    }

    /// The code lengths of `(bits, vals)`, checked: every listed symbol once, at most 16 bits,
    /// and room left in the code space (the all-ones code is never assigned, Kraft sum < 1).
    fn check_table(bits: &[u8; 16], vals: &[u8]) {
        assert_eq!(bits.iter().map(|&b| b as usize).sum::<usize>(), vals.len());
        let mut seen = [false; 256];
        for &v in vals {
            assert!(!seen[v as usize], "symbol {v} listed twice");
            seen[v as usize] = true;
        }
        let kraft: f64 = bits.iter().enumerate().map(|(i, &n)| n as f64 / 2f64.powi(i as i32 + 1)).sum();
        assert!(kraft < 1.0, "code space full: {kraft}");
        let h = Huff::new(bits, vals);
        for &v in vals {
            let (c, l) = h.code[v as usize];
            assert!((1..=16).contains(&l));
            assert_ne!(c as u32, (1u32 << l) - 1, "symbol {v} has the all-ones code");
        }
    }

    #[test]
    fn optimal_tables_are_valid_and_bounded() {
        // one symbol, two, a typical spread and every symbol
        let mut one = [0u64; 256];
        one[7] = 5;
        let mut two = [0u64; 256];
        two[0] = 1;
        two[0xF0] = 1_000_000;
        let typical: Freq = std::array::from_fn(|s| if s % 3 == 0 { (s as u64 * 7919) % 1000 + 1 } else { 0 });
        let all: Freq = std::array::from_fn(|s| s as u64 + 1);
        // Fibonacci counts make an unbounded Huffman tree ~40 levels deep: the 16-bit limit applies
        let mut fib = [0u64; 256];
        let (mut a, mut b) = (1u64, 1u64);
        for f in fib.iter_mut().take(40) {
            *f = a;
            (a, b) = (b, a + b);
        }
        for (name, f) in [("one", one), ("two", two), ("typical", typical), ("all", all), ("fib", fib), ("none", [0; 256])] {
            let (bits, vals) = optimal_table(&f);
            check_table(&bits, &vals);
            for s in 0..256 {
                assert_eq!(vals.contains(&(s as u8)), f[s] > 0 || (name == "none" && s == 0), "{name}: symbol {s}");
            }
        }
        // more frequent symbols never get longer codes
        let (bits, vals) = optimal_table(&typical);
        let h = Huff::new(&bits, &vals);
        for &x in &vals {
            for &y in &vals {
                if typical[x as usize] > typical[y as usize] {
                    assert!(h.code[x as usize].1 <= h.code[y as usize].1, "{x} vs {y}");
                }
            }
        }
        // the Annex K example tables pass the same checks
        check_table(&AC_LUMA_BITS, &AC_LUMA_VALS);
        check_table(&DC_CHROMA_BITS, &DC_VALS);
    }

    /// Issue #1373: tables made for the image give a smaller file with exactly the same pixels as
    /// the Annex K example tables (the quantized coefficients are the same), at every quality and
    /// subsampling, and both decoders read it.
    #[test]
    fn optimized_tables_shrink_files_without_changing_pixels() {
        let (w, h) = (517, 333);
        // gradients plus deterministic noise, like a photo's grain
        let mut seed = 0x2545_f491_u32;
        let src: Vec<u8> = test_rgb(w, h)
            .into_iter()
            .map(|v| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                v.saturating_add((seed % 23) as u8)
            })
            .collect();
        for (q, sub) in [(100, ChromaSubsampling::S444), (92, ChromaSubsampling::S444), (75, ChromaSubsampling::S420), (30, ChromaSubsampling::S420)]
        {
            let std = encode_with(&src, w, h, 3, q, sub, &[], false);
            let opt = encode(&src, w, h, 3, q, sub, &[]).unwrap();
            assert!(opt.len() < std.len(), "q{q} {sub:?}: {} ≥ {}", opt.len(), std.len());
            let a = jpeg_decoder::Decoder::new(&std[..]).decode().expect("standard tables");
            let b = jpeg_decoder::Decoder::new(&opt[..]).decode().expect("optimized tables");
            assert_eq!(a, b, "q{q} {sub:?}: pixels differ");
            let z = zune_jpeg::JpegDecoder::new(zune_core::bytestream::ZCursor::new(&opt[..])).decode().expect("zune-jpeg");
            assert_eq!(z.len(), b.len());
        }
        // grayscale too
        let gray: Vec<u8> = src.iter().step_by(3).copied().collect();
        let std = encode_with(&gray, w, h, 1, 90, ChromaSubsampling::S444, &[], false);
        let opt = encode(&gray, w, h, 1, 90, ChromaSubsampling::S444, &[]).unwrap();
        assert!(opt.len() < std.len());
        assert_eq!(jpeg_decoder::Decoder::new(&std[..]).decode().unwrap(), jpeg_decoder::Decoder::new(&opt[..]).decode().unwrap());
    }

    #[test]
    fn grayscale_and_quality_monotone() {
        let (w, h) = (300, 200);
        let src: Vec<u8> = (0..w * h).map(|i| ((i % w) * 255 / w) as u8 ^ ((i / w) as u8 & 0x10)).collect();
        let lo = encode(&src, w, h, 1, 30, ChromaSubsampling::S444, &[]).unwrap();
        let hi = encode(&src, w, h, 1, 95, ChromaSubsampling::S444, &[]).unwrap();
        assert!(lo.len() < hi.len());
        let px = jpeg_decoder::Decoder::new(&hi[..]).decode().unwrap();
        assert!(psnr(&src, &px) > 35.0);
    }

    /// The files are pinned byte for byte (FNV-1a): a change to how samples are read or converted
    /// that alters a single coefficient shows here. Odd sizes take the edge-replication paths.
    #[test]
    fn output_is_pinned() {
        let fnv = |b: &[u8]| b.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, x| (h ^ u64::from(*x)).wrapping_mul(0x0100_0000_01b3));
        let rgba = |w: usize, h: usize| test_rgb(w, h).as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 200]).collect::<Vec<u8>>();
        let gray = |w: usize, h: usize| (0..w * h).map(|i| ((i % w) * 255 / w) as u8 ^ ((i / w) as u8 & 0x10)).collect::<Vec<u8>>();
        let (s444, s420) = (ChromaSubsampling::S444, ChromaSubsampling::S420);
        let got: Vec<u64> = [
            (test_rgb(64, 48), 64, 48, 3, s444),
            (test_rgb(64, 48), 64, 48, 3, s420),
            (test_rgb(37, 29), 37, 29, 3, s444),
            (test_rgb(37, 29), 37, 29, 3, s420),
            (rgba(41, 17), 41, 17, 4, s420),
            (gray(33, 21), 33, 21, 1, s444),
            (test_rgb(1, 1), 1, 1, 3, s420),
        ]
        .iter()
        .map(|(px, w, h, c, sub)| fnv(&encode(px, *w, *h, *c, 85, *sub, &[]).unwrap()))
        .collect();
        assert_eq!(got, PINNED);
    }

    const PINNED: [u64; 7] = [
        14107411018555077366,
        17267204589457427831,
        10691470458064166444,
        13816826160406889593,
        14172702776067435247,
        17805419631319095519,
        3811093167143368540,
    ];
}
