//! Typed command parameters. Each command reads its params into a struct declared with
//! [`params!`], so a key the command doesn't take (a typo such as `idz` for `ids`, at any depth)
//! is an error instead of being ignored while the command acts on something else.

use serde::de::{DeserializeOwned, Deserializer, Visitor};
use serde_json::Value;

use super::bad;
use crate::Result;

/// A command's parameter struct; [`params!`] implements it. Not for `Value`: params that take any
/// JSON name it as one field, so the keys around it are still checked.
pub trait Params: DeserializeOwned {}

/// `{}`: a command that takes no parameters.
#[derive(Debug, serde::Deserialize)]
pub struct NoParams {}
impl Params for NoParams {}

params! {
    /// `{name}`: commands on one named thing (a preset, a set).
    pub struct Named {
        name: Option<String>,
    }
}

impl Named {
    /// The name, trimmed; `missing `name`` when absent or blank.
    pub fn name(&self, cmd: &str) -> Result<String> {
        self.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).ok_or_else(|| bad(cmd, "missing `name`"))
    }
}

/// Params that are part of another type (`library.filter`: a partial `Filter`): any object here.
/// The command merges it and reads the result with [`from_value`], which checks its keys.
#[derive(Debug, serde::Deserialize)]
#[serde(transparent)]
pub struct Patch(pub serde_json::Map<String, Value>);
impl Params for Patch {}

impl Patch {
    pub fn value(&self) -> Value {
        Value::Object(self.0.clone())
    }
}

/// The keys of a command that reads its params itself (handing them on to code that parses
/// them): [`Keys`] refuses any other key, checking no value.
pub trait KeyList {
    const KEYS: &'static [&'static str];
}

/// Params checked for their keys only ([`KeyList`]); the command reads the raw JSON.
pub struct Keys<K: KeyList>(std::marker::PhantomData<K>);
impl<K: KeyList> Params for Keys<K> {}

impl<K: KeyList> std::fmt::Debug for Keys<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Keys{:?}", K::KEYS)
    }
}

impl<'de, K: KeyList> serde::Deserialize<'de> for Keys<K> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V<K>(std::marker::PhantomData<K>);
        impl<'de, K: KeyList> Visitor<'de> for V<K> {
            type Value = Keys<K>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> std::result::Result<Keys<K>, A::Error> {
                while let Some(k) = map.next_key::<String>()? {
                    if !K::KEYS.contains(&k.as_str()) {
                        let takes = K::KEYS.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", ");
                        return Err(serde::de::Error::custom(format!("unknown parameter `{k}` (takes {takes})")));
                    }
                    // read, not skipped: an `IgnoredAny` would count as an ignored key
                    map.next_value::<Value>()?;
                }
                Ok(Keys(std::marker::PhantomData))
            }
        }
        d.deserialize_struct("Keys", K::KEYS, V(std::marker::PhantomData))
    }
}

/// Declares a [`KeyList`]: `key_list! { pub struct DockKeys = ["panel", "operation"] }`.
#[macro_export]
macro_rules! key_list {
    ($(#[$m:meta])* $vis:vis struct $name:ident = [$($k:literal),* $(,)?]) => {
        $(#[$m])*
        $vis struct $name;
        impl $crate::cmd::params::KeyList for $name {
            const KEYS: &'static [&'static str] = &[$($k),*];
        }
    };
}

/// One string or a list of them (`"a"` is `["a"]`).
#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
pub enum Strings {
    One(String),
    Many(Vec<String>),
}

impl Strings {
    pub fn to_vec(&self) -> Vec<String> {
        match self {
            Strings::One(s) => vec![s.clone()],
            Strings::Many(v) => v.clone(),
        }
    }
}

/// Read `p` as `T`. Every key `T` would ignore is an error, and a value of the wrong type names
/// its key. The error's command is filled in by [`crate::Session::execute`].
pub fn parse<T: Params>(p: &Value) -> Result<T> {
    if !p.is_object() {
        return Err(bad("", "params must be a JSON object"));
    }
    from_value(p)
}

/// [`parse`] outside [`crate::Session::execute`] (the app's import task reads `library.import`'s
/// params itself): the error names `cmd`.
pub fn parse_as<T: Params>(cmd: &str, p: &Value) -> Result<T> {
    parse(p).map_err(|e| match e {
        crate::EngineError::BadParams { msg, .. } => bad(cmd, msg),
        e => e,
    })
}

/// `p` as `T`, checked like params: a key `T` would ignore (at any depth) is an error. For JSON a
/// command merges before reading it (a partial filter or settings).
pub fn from_value<T: DeserializeOwned>(p: &Value) -> Result<T> {
    let mut unknown = Vec::new();
    let mut note = |path: serde_ignored::Path| unknown.push(format!("`{}`", key_path(&path)));
    let de = serde_ignored::Deserializer::new(p, &mut note);
    let t = serde_path_to_error::deserialize(de).map_err(|e| {
        let path = e.path().to_string();
        let inner = e.into_inner();
        if path == "." { bad("", inner.to_string()) } else { bad("", format!("`{path}`: {inner}")) }
    })?;
    let takes = match keys_of::<T>() {
        Some([]) => " (takes none)".to_string(),
        Some(keys) => format!(" (takes {})", keys.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", ")),
        None => String::new(),
    };
    match unknown.len() {
        0 => Ok(t),
        1 => Err(bad("", format!("unknown parameter {}{takes}", unknown.join("")))),
        _ => Err(bad("", format!("unknown parameters {}{takes}", unknown.join(", ")))),
    }
}

/// The top-level keys `T` takes, as serde names them; `None` when it isn't a struct (any keys,
/// [`Patch`]). Read from what its `Deserialize` asks for, without deserializing anything.
pub fn keys_of<T: DeserializeOwned>() -> Option<&'static [&'static str]> {
    let mut fields = None;
    let _ = T::deserialize(FieldProbe(&mut fields));
    fields
}

/// A deserializer that only notes the field names a struct asks for, then stops.
struct FieldProbe<'a>(&'a mut Option<&'static [&'static str]>);

#[derive(Debug)]
struct Stop;
impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("field probe")
    }
}
impl std::error::Error for Stop {}
impl serde::de::Error for Stop {
    fn custom<M: std::fmt::Display>(_: M) -> Self {
        Stop
    }
}

impl<'de> Deserializer<'de> for FieldProbe<'_> {
    type Error = Stop;
    fn deserialize_any<V: Visitor<'de>>(self, _: V) -> std::result::Result<V::Value, Stop> {
        Err(Stop)
    }
    fn deserialize_struct<V: Visitor<'de>>(self, _: &'static str, fields: &'static [&'static str], _: V) -> std::result::Result<V::Value, Stop> {
        *self.0 = Some(fields);
        Err(Stop)
    }
    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf option unit
        unit_struct newtype_struct seq tuple tuple_struct map enum identifier ignored_any
    }
}

/// `patch` merged onto `base` reads back as a `T` with no key ignored: a partial value (develop
/// settings, a filter) names only keys `T` has, at any depth.
pub(crate) fn check_merge<T: serde::Serialize + DeserializeOwned>(base: &T, patch: &Value) -> Result<()> {
    let mut v = serde_json::to_value(base).map_err(|e| bad("", e.to_string()))?;
    lightcraft_develop::presets::deep_merge(&mut v, patch);
    from_value::<T>(&v).map(|_| ())
}

/// `settings.light.exposure`, `ids[1]`: a key's place in the params (an `Option` or newtype
/// around it adds nothing).
fn key_path(p: &serde_ignored::Path) -> String {
    use serde_ignored::Path;
    match p {
        Path::Root => String::new(),
        Path::Seq { parent, index } => format!("{}[{index}]", key_path(parent)),
        Path::Map { parent, key } => match key_path(parent) {
            at if at.is_empty() => key.clone(),
            at => format!("{at}.{key}"),
        },
        Path::Some { parent } | Path::NewtypeStruct { parent } | Path::NewtypeVariant { parent } => key_path(parent),
    }
}

/// Declares a parameter struct: `Deserialize` with camelCase keys, [`Params`]. An `Option` field
/// may be left out or `null`; give others `#[serde(default)]` to make them optional.
/// `struct X with targets { … }` adds the photo targets `ids` and `id` (see [`Session::targets_of`]).
///
/// [`Session::targets_of`]: crate::Session::targets_of
#[macro_export]
macro_rules! params {
    ($(#[$m:meta])* $vis:vis struct $name:ident { $($(#[$fm:meta])* $f:ident : $t:ty),* $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        $vis struct $name { $($(#[$fm])* pub $f: $t),* }
        $(#[$m])*
        impl $crate::cmd::params::Params for $name {}
    };
    ($(#[$m:meta])* $vis:vis struct $name:ident with targets { $($(#[$fm:meta])* $f:ident : $t:ty),* $(,)? }) => {
        $crate::cmd::params::params! {
            $(#[$m])*
            $vis struct $name {
                ids: Option<Vec<$crate::catalog::PhotoId>>,
                id: Option<$crate::catalog::PhotoId>,
                $($(#[$fm])* $f: $t),*
            }
        }
        $(#[$m])*
        impl $name {
            /// The photos the call names (`ids`, else `id`), else the selection.
            #[allow(dead_code)]
            pub fn targets(&self, s: &$crate::Session) -> Vec<$crate::catalog::PhotoId> {
                s.targets_of(self.ids.as_deref(), self.id)
            }
        }
    };
}
pub use crate::params;

/// A whole number, also written as an integral float (`2.0`), as the float-reading params before
/// typing accepted. Out of range for `T`, fractional or not a number is an error.
struct Whole<T>(T);

impl<'de, T: TryFrom<i128>> serde::Deserialize<'de> for Whole<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V<T>(std::marker::PhantomData<T>);
        impl<T: TryFrom<i128>> V<T> {
            fn fit<E: serde::de::Error>(n: i128) -> std::result::Result<T, E> {
                T::try_from(n).map_err(|_| E::custom(format!("{n} is out of range")))
            }
        }
        impl<T: TryFrom<i128>> Visitor<'_> for V<T> {
            type Value = T;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a whole number")
            }
            fn visit_i64<E: serde::de::Error>(self, n: i64) -> std::result::Result<T, E> {
                Self::fit(i128::from(n))
            }
            fn visit_u64<E: serde::de::Error>(self, n: u64) -> std::result::Result<T, E> {
                Self::fit(i128::from(n))
            }
            fn visit_f64<E: serde::de::Error>(self, n: f64) -> std::result::Result<T, E> {
                // the bounds are exact in f64; the cast can't saturate inside them
                if n.is_finite() && n.fract() == 0.0 && n.abs() < 2f64.powi(64) {
                    Self::fit(n as i128)
                } else {
                    Err(E::custom(format!("{n} is not a whole number")))
                }
            }
        }
        d.deserialize_any(V(std::marker::PhantomData)).map(Whole)
    }
}

/// `#[serde(deserialize_with = "whole")]`: see [`Whole`].
pub fn whole<'de, D: Deserializer<'de>, T: TryFrom<i128>>(d: D) -> std::result::Result<T, D::Error> {
    <Whole<T> as serde::Deserialize>::deserialize(d).map(|w| w.0)
}

/// `#[serde(default, deserialize_with = "present")]` on an `Option<Option<T>>`: `None` when the key
/// is left out, `Some(None)` for `null` (which then means "none", not "unchanged").
pub fn present<'de, D: Deserializer<'de>, T: serde::Deserialize<'de>>(d: D) -> std::result::Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

use serde::Deserialize as _;

#[cfg(test)]
mod tests {
    use lightcraft_catalog::PhotoId;
    use serde_json::json;

    use super::{parse, whole};

    params! {
        struct Probe {
            #[serde(deserialize_with = "whole")]
            rating: u8,
            rows: Option<usize>,
            ids: Option<Vec<PhotoId>>,
            #[serde(default)]
            advance: bool,
            settings: Option<Inner>,
        }
    }
    #[derive(Debug, serde::Deserialize)]
    struct Inner {
        exposure: Option<f64>,
    }

    fn msg(p: serde_json::Value) -> String {
        parse::<Probe>(&p).unwrap_err().to_string()
    }

    #[test]
    fn a_misspelled_or_unknown_key_is_an_error_at_any_depth() {
        let m = msg(json!({"rating": 2, "idz": [1]}));
        assert!(m.contains("unknown parameter `idz` (takes `rating`, `rows`, `ids`, `advance`, `settings`)"), "{m}");
        let m = msg(json!({"rating": 2, "settings": {"exposre": 1}}));
        assert!(m.contains("`settings.exposre`"), "{m}");
        assert!(msg(json!({"rating": 2, "a": 1, "b": 2})).contains("unknown parameters `a`, `b`"));
        let ok = parse::<Probe>(&json!({"rating": 2, "ids": [1, 2], "advance": true, "settings": {"exposure": 1}})).unwrap();
        assert_eq!((ok.rating, ok.ids, ok.advance), (2, Some(vec![PhotoId(1), PhotoId(2)]), true));
        assert_eq!(ok.settings.and_then(|s| s.exposure), Some(1.0));
    }

    #[test]
    fn a_wrong_type_names_its_key_and_non_objects_are_refused() {
        assert!(msg(json!({"rating": 2, "ids": [1, "x"]})).contains("`ids[1]`"));
        assert!(msg(json!({"rating": "two"})).contains("`rating`"));
        assert!(msg(json!({})).contains("missing field `rating`"));
        assert!(msg(json!([1])).contains("must be a JSON object"));
    }

    params! {
        struct Index {
            #[serde(deserialize_with = "whole")]
            index: usize,
        }
    }

    #[test]
    fn indices_keep_integer_precision_and_check_usize_bounds() {
        let index = |v: serde_json::Value| parse::<Index>(&json!({"index": v})).map(|p| p.index);
        for i in [0, 1, usize::MAX] {
            assert_eq!(index(json!(i)).unwrap(), i);
        }
        #[cfg(target_pointer_width = "64")]
        assert_eq!(index(json!(9_007_199_254_740_993usize)).unwrap(), 9_007_199_254_740_993, "integer JSON must not round through f64");
        assert!(index(json!(2.0_f64.powi(usize::BITS as i32))).is_err(), "2^usize::BITS must not saturate");
        #[cfg(target_pointer_width = "32")]
        {
            assert!(index(json!(u64::from(u32::MAX) + 1)).is_err());
            assert_eq!(index(json!(f64::from(u32::MAX))).unwrap(), usize::MAX);
        }
    }

    crate::key_list! {
        struct Two = ["a", "b"]
    }

    #[test]
    fn a_key_list_refuses_other_keys_and_checks_no_values() {
        assert!(parse::<super::Keys<Two>>(&json!({"a": 1, "b": [true]})).is_ok());
        let m = parse::<super::Keys<Two>>(&json!({"a": 1, "c": 2})).unwrap_err().to_string();
        assert!(m.contains("`c`") && m.contains("`a`, `b`"), "{m}");
        assert_eq!(super::keys_of::<super::Keys<Two>>(), Some(&["a", "b"][..]));
    }

    #[test]
    fn keys_are_read_from_the_struct() {
        assert_eq!(super::keys_of::<Probe>(), Some(&["rating", "rows", "ids", "advance", "settings"][..]));
        assert_eq!(super::keys_of::<super::NoParams>(), Some(&[][..]));
        assert_eq!(super::keys_of::<super::Patch>(), None);
    }

    #[test]
    fn whole_numbers_take_integral_floats_and_refuse_the_rest() {
        assert_eq!(parse::<Probe>(&json!({"rating": 3.0, "rows": 4})).unwrap().rows, Some(4));
        assert!(msg(json!({"rating": 2.5})).contains("not a whole number"));
        assert!(msg(json!({"rating": 256})).contains("out of range"));
        assert!(msg(json!({"rating": -1})).contains("out of range"));
        assert!(msg(json!({"rating": 1e300})).contains("not a whole number"));
        assert_eq!(parse::<Probe>(&json!({"rating": 0, "rows": null})).unwrap().rows, None);
    }
}

#[cfg(test)]
mod command_tests {
    use serde_json::json;

    use crate::Session;

    /// The bug typed params fix: `idz` for `ids` was ignored and the active photo got the rating.
    #[test]
    fn a_misspelled_key_changes_nothing_and_says_what_the_command_takes() {
        let mut s = Session::with_demo();
        let ids = s.visible_cloned();
        s.execute("library.select", &json!({"ids": [ids[0].0]})).unwrap();
        let ratings = |s: &Session| ids.iter().map(|id| s.catalog.photo(*id).map(|p| p.rating)).collect::<Vec<_>>();
        let (before, undo) = (ratings(&s), s.undo.len());
        let typo = json!({"rating": 2, "idz": [ids[1].0]});
        let e = s.execute("photo.rate", &typo).unwrap_err().to_string();
        assert!(e.contains("photo.rate") && e.contains("unknown parameter `idz`") && e.contains("`ids`"), "{e}");
        assert_eq!((ratings(&s), s.undo.len()), (before, undo));
        s.execute("photo.rate", &json!({"rating": 2, "ids": [ids[1].0]})).unwrap();
        assert_eq!(s.catalog.photo(ids[1]).map(|p| p.rating), Some(2));
    }

    #[test]
    fn a_misspelled_setting_in_a_partial_is_refused() {
        let mut s = Session::with_demo();
        let e = s.execute("develop.merge", &json!({"settings": {"light": {"exposre": 1}}})).unwrap_err().to_string();
        assert!(e.contains("`light.exposre`"), "{e}");
        let e = s.execute("library.filter", &json!({"ratng": 3})).unwrap_err().to_string();
        assert!(e.contains("`ratng`"), "{e}");
        s.execute("develop.merge", &json!({"settings": {"light": {"exposure": 1}}})).unwrap();
    }
}
