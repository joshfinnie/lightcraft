//! [`DevelopSettings::hash64`](crate::DevelopSettings::hash64): the settings' serde data model
//! hashed word by word. Floats go in as their bits, not as text: a painted mask is hundreds of
//! thousands of numbers, and a slider drag hashes new settings on every frame.
//!
//! Field and variant names, lengths and `None`/`Some` are hashed too, so two different values
//! never feed the same words; a field skipped when it holds its default adds nothing, so a new
//! field left at its default keeps every older hash (and the previews cached under it). A number
//! is only its bits, with nothing to say what kind it is: the type a value has follows from the
//! names and variants before it, and a point is four words (two names, two numbers).

use serde::Serialize;
use serde::ser;

const K: u64 = 0x9e37_79b9_7f4a_7c15;

/// What a value's shape is where the schema leaves it open (an `Option`, an enum, a length).
#[derive(Clone, Copy)]
enum Tag {
    None = 1,
    Some,
    Unit,
    Seq,
    Map,
    Variant,
    End,
}

pub(crate) fn hash<T: Serialize + ?Sized>(value: &T) -> u64 {
    let mut h = Hasher(0x243f_6a88_85a3_08d3);
    // (the settings' types only fail through a custom `Serialize`: the words so far still differ)
    let _ = value.serialize(&mut h);
    h.0
}

struct Hasher(u64);

impl Hasher {
    fn word(&mut self, w: u64) {
        let p = u128::from(self.0 ^ w) * u128::from(K);
        self.0 = p as u64 ^ (p >> 64) as u64;
    }

    fn tag(&mut self, t: Tag) {
        self.word(t as u64);
    }

    /// Up to 7 bytes are one word, their length in its top byte (a length word of a longer run
    /// is below 2^56, so the two never meet).
    fn bytes(&mut self, b: &[u8]) {
        if b.len() < 8 {
            let mut w = [0u8; 8];
            w[..b.len()].copy_from_slice(b);
            w[7] = b.len() as u8;
            self.word(u64::from_le_bytes(w));
            return;
        }
        self.word(b.len() as u64);
        let (chunks, rest) = b.as_chunks::<8>();
        for c in chunks {
            self.word(u64::from_le_bytes(*c));
        }
        if !rest.is_empty() {
            let mut w = [0u8; 8];
            w[..rest.len()].copy_from_slice(rest);
            self.word(u64::from_le_bytes(w));
        }
    }

    fn name(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
}

#[derive(Debug)]
pub(crate) struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl ser::Error for Error {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        Error(msg.to_string())
    }
}

type R = Result<(), Error>;

impl ser::Serializer for &mut Hasher {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    fn serialize_bool(self, v: bool) -> R {
        self.word(u64::from(v));
        Ok(())
    }
    fn serialize_i8(self, v: i8) -> R {
        self.serialize_i64(i64::from(v))
    }
    fn serialize_i16(self, v: i16) -> R {
        self.serialize_i64(i64::from(v))
    }
    fn serialize_i32(self, v: i32) -> R {
        self.serialize_i64(i64::from(v))
    }
    fn serialize_i64(self, v: i64) -> R {
        self.word(v as u64);
        Ok(())
    }
    fn serialize_i128(self, v: i128) -> R {
        self.word(v as u64);
        self.word((v >> 64) as u64);
        Ok(())
    }
    fn serialize_u8(self, v: u8) -> R {
        self.serialize_u64(u64::from(v))
    }
    fn serialize_u16(self, v: u16) -> R {
        self.serialize_u64(u64::from(v))
    }
    fn serialize_u32(self, v: u32) -> R {
        self.serialize_u64(u64::from(v))
    }
    fn serialize_u64(self, v: u64) -> R {
        self.word(v);
        Ok(())
    }
    fn serialize_u128(self, v: u128) -> R {
        self.word(v as u64);
        self.word((v >> 64) as u64);
        Ok(())
    }
    fn serialize_f32(self, v: f32) -> R {
        self.serialize_f64(f64::from(v))
    }
    fn serialize_f64(self, v: f64) -> R {
        self.word(v.to_bits());
        Ok(())
    }
    fn serialize_char(self, v: char) -> R {
        self.word(u64::from(v));
        Ok(())
    }
    fn serialize_str(self, v: &str) -> R {
        self.name(v);
        Ok(())
    }
    fn serialize_bytes(self, v: &[u8]) -> R {
        self.bytes(v);
        Ok(())
    }
    fn serialize_none(self) -> R {
        self.tag(Tag::None);
        Ok(())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> R {
        self.tag(Tag::Some);
        value.serialize(self)
    }
    fn serialize_unit(self) -> R {
        self.tag(Tag::Unit);
        Ok(())
    }
    fn serialize_unit_struct(self, _: &'static str) -> R {
        self.serialize_unit()
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, variant: &'static str) -> R {
        self.tag(Tag::Variant);
        self.name(variant);
        Ok(())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, value: &T) -> R {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(self, _: &'static str, _: u32, variant: &'static str, value: &T) -> R {
        self.tag(Tag::Variant);
        self.name(variant);
        value.serialize(self)
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<Self, Error> {
        self.tag(Tag::Seq);
        // (a length not known up front is told by the marker at the end)
        self.word(len.map_or(u64::MAX, |n| n as u64));
        Ok(self)
    }
    fn serialize_tuple(self, len: usize) -> Result<Self, Error> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(self, _: &'static str, len: usize) -> Result<Self, Error> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_variant(self, _: &'static str, _: u32, variant: &'static str, len: usize) -> Result<Self, Error> {
        self.tag(Tag::Variant);
        self.name(variant);
        self.serialize_seq(Some(len))
    }
    fn serialize_map(self, len: Option<usize>) -> Result<Self, Error> {
        self.tag(Tag::Map);
        self.word(len.map_or(u64::MAX, |n| n as u64));
        Ok(self)
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self, Error> {
        // not the field count: a field skipped at its default must leave the hash as it was
        Ok(self)
    }
    fn serialize_struct_variant(self, _: &'static str, _: u32, variant: &'static str, _: usize) -> Result<Self, Error> {
        self.tag(Tag::Variant);
        self.name(variant);
        self.serialize_struct("", 0)
    }
}

impl ser::SerializeSeq for &mut Hasher {
    type Ok = ();
    type Error = Error;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> R {
        value.serialize(&mut **self)
    }
    fn end(self) -> R {
        self.tag(Tag::End);
        Ok(())
    }
}

impl ser::SerializeTuple for &mut Hasher {
    type Ok = ();
    type Error = Error;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> R {
        value.serialize(&mut **self)
    }
    fn end(self) -> R {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleStruct for &mut Hasher {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> R {
        value.serialize(&mut **self)
    }
    fn end(self) -> R {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleVariant for &mut Hasher {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> R {
        value.serialize(&mut **self)
    }
    fn end(self) -> R {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeMap for &mut Hasher {
    type Ok = ();
    type Error = Error;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> R {
        key.serialize(&mut **self)
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> R {
        value.serialize(&mut **self)
    }
    fn end(self) -> R {
        self.tag(Tag::End);
        Ok(())
    }
}

impl ser::SerializeStruct for &mut Hasher {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, key: &'static str, value: &T) -> R {
        self.name(key);
        value.serialize(&mut **self)
    }
    fn end(self) -> R {
        Ok(())
    }
}

impl ser::SerializeStructVariant for &mut Hasher {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, key: &'static str, value: &T) -> R {
        ser::SerializeStruct::serialize_field(self, key, value)
    }
    fn end(self) -> R {
        ser::SerializeStruct::end(self)
    }
}

#[cfg(test)]
mod tests {
    use super::hash;
    use crate::{BrushStroke, DevelopSettings, MaskShape, controls};
    use lightcraft_geom::Point;
    use serde::Serialize;

    /// Previews cached on disk are found by this hash in later runs and other builds: a change to
    /// the hash (or to the default settings) is deliberate, and makes every cached preview render
    /// again.
    #[test]
    fn the_hash_of_the_default_settings_is_pinned() {
        assert_eq!(DevelopSettings::default().hash64(), 14_487_977_974_107_052_410);
    }

    #[test]
    fn equal_settings_hash_alike_and_any_change_differs() {
        let mut a = DevelopSettings::default();
        controls::set(&mut a, "light.exposure", 1.0);
        assert_eq!(a.hash64(), a.clone().hash64());
        let mut swapped = DevelopSettings::default();
        controls::set(&mut swapped, "light.contrast", 1.0);
        assert_ne!(a.hash64(), swapped.hash64(), "the same value in another field");
        let mut tiny = a.clone();
        controls::set(&mut tiny, "light.exposure", 1.0 + f64::EPSILON);
        assert_ne!(a.hash64(), tiny.hash64());
        let mut neg = DevelopSettings::default();
        controls::set(&mut neg, "light.exposure", -0.0);
        assert_ne!(neg.hash64(), DevelopSettings::default().hash64(), "-0 is saved as -0: another value");
    }

    /// Lengths are hashed: the same points split into strokes differently are another mask.
    #[test]
    fn the_same_points_in_other_strokes_differ() {
        let p = |x: f64| Point::new(x, x);
        let stroke = |points: Vec<Point>| BrushStroke { points, ..Default::default() };
        let one = MaskShape::Brush { strokes: vec![stroke(vec![p(0.1), p(0.2)]), stroke(vec![p(0.3)])] };
        let two = MaskShape::Brush { strokes: vec![stroke(vec![p(0.1)]), stroke(vec![p(0.2), p(0.3)])] };
        assert_ne!(hash(&one), hash(&two));
    }

    #[test]
    fn none_some_unit_and_names_differ() {
        #[derive(Serialize)]
        struct A {
            x: Option<u32>,
        }
        #[derive(Serialize)]
        struct B {
            y: Option<u32>,
        }
        let hashes = [hash(&A { x: None }), hash(&A { x: Some(0) }), hash(&B { y: None }), hash(&()), hash(""), hash("12345678")];
        for (i, a) in hashes.iter().enumerate() {
            for b in &hashes[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    /// A field skipped at its default (as `hdr` is) adds nothing: settings saved before it existed
    /// keep their hash.
    #[test]
    fn a_skipped_field_adds_nothing() {
        #[derive(Serialize)]
        struct Old {
            a: u32,
        }
        #[derive(Serialize)]
        struct New {
            a: u32,
            #[serde(skip_serializing_if = "Option::is_none")]
            b: Option<u32>,
        }
        assert_eq!(hash(&Old { a: 3 }), hash(&New { a: 3, b: None }));
        assert_ne!(hash(&Old { a: 3 }), hash(&New { a: 3, b: Some(0) }));
    }
}
