//! Dependency layering rules (plan/architecture.md §3).
//!
//! The rule engine works on a small, metadata-independent model so it can be
//! unit-tested; `from_metadata` builds that model from `cargo metadata`.

use serde_json::Value;

/// Where a workspace crate sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Regular layered crate.
    Layer(u8),
    /// Layer 0, and additionally may depend on no workspace crate at all.
    Standalone,
    /// Test tooling: may depend on anything up to L5 (it sits at L6 for rule
    /// purposes); other crates may use it only as a dev-dependency.
    Testkit,
    /// Binaries and build tooling: exempt from the rules.
    Exempt,
}

impl Class {
    fn layer(self) -> Option<u8> {
        match self {
            Class::Layer(l) => Some(l),
            Class::Standalone => Some(0),
            Class::Testkit => Some(5),
            Class::Exempt => None,
        }
    }
}

/// The layering table. Names are package names without the `lightcraft-`
/// prefix.
pub const TABLE: &[(&str, Class)] = &[
    ("geom", Class::Layer(0)),
    ("color", Class::Layer(0)),
    ("raster", Class::Layer(0)),
    ("tiff", Class::Layer(0)),
    ("sysmem", Class::Layer(0)),
    ("fetch", Class::Layer(0)),
    ("denoise-core", Class::Layer(1)),
    ("denoise", Class::Layer(1)),
    ("heif", Class::Layer(0)),
    ("raw", Class::Layer(1)),
    ("codecs", Class::Layer(1)),
    ("meta", Class::Layer(1)),
    ("develop", Class::Layer(1)),
    ("lensdb", Class::Layer(2)),
    ("inpaint", Class::Layer(1)),
    ("faces", Class::Layer(1)),
    ("geocode", Class::Layer(1)),
    ("scenes", Class::Layer(1)),
    ("slideshow", Class::Layer(1)),
    ("pipeline", Class::Layer(2)),
    ("gpu", Class::Layer(3)),
    ("catalog", Class::Layer(3)),
    ("preview", Class::Layer(3)),
    ("export", Class::Layer(3)),
    ("merge", Class::Layer(3)),
    ("segment", Class::Layer(3)),
    ("engine", Class::Layer(4)),
    ("ui-egui", Class::Layer(5)),
    ("mcp", Class::Layer(5)),
    ("testkit", Class::Testkit),
    // L6 apps and tooling
    ("lightcraft", Class::Exempt),
    ("cli", Class::Exempt),
    ("web", Class::Exempt),
    ("xtask", Class::Exempt),
];

/// Explicit orderings *within* a layer (earlier may be used by later).
/// L0: `raster` builds on `color` and `geom`; `color` uses `geom` for matrices.
/// L1: `raw` and `codecs` read metadata through `meta`; `develop` uses `meta` for XMP.
pub const INTRA_LAYER_ORDER: &[&[&str]] = &[
    &["geom", "color", "raster"],
    &["tiff", "raster"],
    &["denoise-core", "denoise"],
    &["meta", "raw"],
    &["meta", "codecs"],
    &["meta", "develop"],
    &["develop", "scenes"],
    &["codecs", "raw"],
];

fn intra_layer_allowed(from: &str, to: &str) -> bool {
    let (from, to) = (short_name(from), short_name(to));
    INTRA_LAYER_ORDER.iter().any(|chain| match (chain.iter().position(|n| *n == from), chain.iter().position(|n| *n == to)) {
        (Some(f), Some(t)) => t < f,
        _ => false,
    })
}

/// External crates that constitute a UI toolkit / windowing dependency.
/// Entries ending in `*` are prefixes.
pub const UI_CRATES: &[&str] = &["craft-ui", "egui", "eframe", "winit", "egui_kittest", "rfd", "bevy*"];

/// First layer allowed to use UI crates.
pub const UI_MIN_LAYER: u8 = 5;

pub fn short_name(pkg: &str) -> &str {
    pkg.strip_prefix("lightcraft-").unwrap_or(pkg)
}

pub fn classify(pkg: &str) -> Option<Class> {
    let s = short_name(pkg);
    TABLE.iter().find(|(n, _)| *n == s).map(|(_, c)| *c)
}

fn is_ui_crate(name: &str) -> bool {
    UI_CRATES.iter().any(|p| match p.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => name == *p,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepKind {
    Normal,
    Dev,
    Build,
}

#[derive(Debug, Clone)]
pub struct Dep {
    pub name: String,
    pub kind: DepKind,
    /// `true` if the dependency is a workspace member.
    pub workspace: bool,
}

#[derive(Debug, Clone)]
pub struct Crate {
    pub name: String,
    pub deps: Vec<Dep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    Unregistered { krate: String },
    Upward { krate: String, dep: String, from: u8, to: u8, kind: DepKind },
    StandaloneHasWorkspaceDep { krate: String, dep: String },
    TestkitAsNormalDep { krate: String },
    UiBelowL5 { krate: String, dep: String, layer: u8 },
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Violation::Unregistered { krate } => {
                write!(f, "{krate}: unknown workspace crate; register it in xtask/src/layers.rs TABLE (see plan/architecture.md §3)")
            }
            Violation::Upward { krate, dep, from, to, kind } => {
                write!(f, "{krate} (L{from}) -> {dep} (L{to}) [{kind:?}]: may only depend on strictly lower layers")
            }
            Violation::StandaloneHasWorkspaceDep { krate, dep } => {
                write!(f, "{krate}: standalone crate must not depend on workspace crate {dep}")
            }
            Violation::TestkitAsNormalDep { krate } => {
                write!(f, "{krate}: lightcraft-testkit may only be a dev-dependency")
            }
            Violation::UiBelowL5 { krate, dep, layer } => {
                write!(f, "{krate} (L{layer}) depends on UI crate `{dep}`; UI toolkits are only allowed in L5+")
            }
        }
    }
}

/// Check all rules. Returns violations sorted for stable output.
pub fn check(crates: &[Crate]) -> Vec<Violation> {
    let mut out = Vec::new();
    for c in crates {
        let Some(class) = classify(&c.name) else {
            out.push(Violation::Unregistered { krate: c.name.clone() });
            continue;
        };
        if class == Class::Exempt {
            continue;
        }
        let layer = class.layer().unwrap_or(0);
        for d in &c.deps {
            // Self dev-dependencies (e.g. to enable features in tests) are fine.
            if d.name == c.name {
                continue;
            }
            if d.workspace {
                if class == Class::Standalone {
                    out.push(Violation::StandaloneHasWorkspaceDep { krate: c.name.clone(), dep: d.name.clone() });
                    continue;
                }
                match classify(&d.name) {
                    // Unregistered deps are reported on their own entry.
                    None => {}
                    Some(Class::Testkit) if d.kind != DepKind::Dev => out.push(Violation::TestkitAsNormalDep { krate: c.name.clone() }),
                    Some(Class::Testkit) => {}
                    Some(dc) => {
                        let to = dc.layer().unwrap_or(u8::MAX);
                        if to >= layer && !(to == layer && intra_layer_allowed(&c.name, &d.name)) {
                            out.push(Violation::Upward {
                                krate: c.name.clone(),
                                dep: d.name.clone(),
                                from: layer,
                                to: if to == u8::MAX { 7 } else { to },
                                kind: d.kind,
                            });
                        }
                    }
                }
            } else if layer < UI_MIN_LAYER && is_ui_crate(&d.name) {
                out.push(Violation::UiBelowL5 { krate: c.name.clone(), dep: d.name.clone(), layer });
            }
        }
    }
    out.sort_by_key(|v| v.to_string());
    out.dedup();
    out
}

/// Build the model from `cargo metadata --format-version 1 --no-deps`.
pub fn from_metadata(meta: &Value) -> Result<Vec<Crate>, String> {
    let pkgs = meta["packages"].as_array().ok_or("metadata: no packages array")?;
    let members: Vec<&str> = pkgs.iter().filter_map(|p| p["name"].as_str()).collect();
    let mut out = Vec::new();
    for p in pkgs {
        let name = p["name"].as_str().ok_or("package without name")?.to_owned();
        let mut deps = Vec::new();
        for d in p["dependencies"].as_array().into_iter().flatten() {
            let dname = d["name"].as_str().unwrap_or_default().to_owned();
            let kind = match d["kind"].as_str() {
                Some("dev") => DepKind::Dev,
                Some("build") => DepKind::Build,
                _ => DepKind::Normal,
            };
            let workspace = members.contains(&dname.as_str()) || d["path"].is_string();
            deps.push(Dep { name: dname, kind, workspace });
        }
        out.push(Crate { name, deps });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub fn describe(class: Option<Class>) -> String {
    match class {
        Some(Class::Layer(l)) => format!("L{l}"),
        Some(Class::Standalone) => "L0 standalone".into(),
        Some(Class::Testkit) => "testkit".into(),
        Some(Class::Exempt) => "exempt".into(),
        None => "UNREGISTERED".into(),
    }
}

/// The no-panic lints every production crate root denies (never-crash rule, CLAUDE.md).
pub const ROOT_DENY: [&str; 6] =
    ["clippy::unwrap_used", "clippy::expect_used", "clippy::panic", "clippy::unimplemented", "clippy::todo", "clippy::unreachable"];

/// What crate root `src` lacks: `#![forbid(unsafe_code)]` (unless `may_use_unsafe`) and each of
/// [`ROOT_DENY`] in a `#![deny(…)]` attribute.
pub fn root_attr_problems(src: &str, may_use_unsafe: bool) -> Vec<String> {
    let inner: Vec<String> = src.lines().map(str::trim).filter(|l| l.starts_with("#![")).map(|l| l.replace(' ', "")).collect();
    let mut out = Vec::new();
    if !may_use_unsafe && !inner.iter().any(|l| l.starts_with("#![forbid(") && l.contains("unsafe_code")) {
        out.push("no #![forbid(unsafe_code)]".to_string());
    }
    let denied = |lint: &str| inner.iter().any(|l| l.starts_with("#![deny(") && l.contains(lint));
    let missing: Vec<&str> = ROOT_DENY.iter().copied().filter(|l| !denied(l)).collect();
    if !missing.is_empty() {
        out.push(format!("does not deny {}", missing.join(", ")));
    }
    out
}

/// The production crate roots (each package's lib and bin targets, outside tooling) that lack the
/// never-crash attributes ([`root_attr_problems`]).
pub fn root_attr_violations(meta: &Value) -> Result<Vec<String>, String> {
    let pkgs = meta["packages"].as_array().ok_or("metadata: no packages array")?;
    let mut out = Vec::new();
    for p in pkgs {
        let name = p["name"].as_str().unwrap_or_default();
        if matches!(name, "xtask" | "lightcraft-testkit") {
            continue;
        }
        for t in p["targets"].as_array().into_iter().flatten() {
            let kinds: Vec<&str> = t["kind"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            if !kinds.iter().any(|k| matches!(*k, "lib" | "rlib" | "cdylib" | "bin")) {
                continue;
            }
            let path = t["src_path"].as_str().ok_or("target without src_path")?;
            let src = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            for problem in root_attr_problems(&src, name == "lightcraft-sysmem") {
                out.push(format!("{path}: {problem}"));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_roots_need_forbid_unsafe_and_every_no_panic_lint() {
        let full = "//! x\n#![forbid(unsafe_code)]\n#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]\n";
        assert!(root_attr_problems(full, false).is_empty());
        assert_eq!(
            root_attr_problems(
                "#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]",
                true
            ),
            Vec::<String>::new()
        );
        assert_eq!(
            root_attr_problems("//! nothing\n", true),
            ["does not deny clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable"]
        );
        let partial = "#![forbid(unsafe_code)]\n#![deny(clippy::unwrap_used, clippy::panic)]\n// #![deny(clippy::todo)]\n";
        assert_eq!(
            root_attr_problems(partial, false),
            ["does not deny clippy::expect_used, clippy::unimplemented, clippy::todo, clippy::unreachable"]
        );
        assert_eq!(
            root_attr_problems(
                "#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]",
                false
            ),
            ["no #![forbid(unsafe_code)]"]
        );
    }

    fn c(name: &str, deps: &[(&str, DepKind, bool)]) -> Crate {
        Crate { name: name.into(), deps: deps.iter().map(|(n, k, w)| Dep { name: (*n).into(), kind: *k, workspace: *w }).collect() }
    }
    use DepKind::*;

    #[test]
    fn clean_downward_graph_passes() {
        let g = [
            c("lightcraft-geom", &[("serde", Normal, false)]),
            c("lightcraft-develop", &[("lightcraft-geom", Normal, true)]),
            c("lightcraft-engine", &[("lightcraft-develop", Normal, true), ("lightcraft-testkit", Dev, true)]),
            c("lightcraft-ui-egui", &[("lightcraft-engine", Normal, true), ("egui", Normal, false)]),
            c("lightcraft-cli", &[("lightcraft-ui-egui", Normal, true)]),
        ];
        assert!(check(&g).is_empty(), "{:?}", check(&g));
    }

    #[test]
    fn upward_dependency_flagged() {
        let v = check(&[c("lightcraft-develop", &[("lightcraft-engine", Normal, true)])]);
        assert!(matches!(v[..], [Violation::Upward { from: 1, to: 4, .. }]));
    }

    #[test]
    fn sideways_dependency_flagged() {
        let v = check(&[c("lightcraft-catalog", &[("lightcraft-preview", Normal, true)])]);
        assert!(matches!(v[..], [Violation::Upward { from: 3, to: 3, .. }]));
    }

    #[test]
    fn l0_chain_allowed_one_way() {
        assert!(check(&[c("lightcraft-raster", &[("lightcraft-color", Normal, true)])]).is_empty());
        assert!(!check(&[c("lightcraft-color", &[("lightcraft-raster", Normal, true)])]).is_empty());
    }

    #[test]
    fn ui_crates_forbidden_below_l5() {
        for dep in ["craft-ui", "egui", "eframe", "winit", "rfd"] {
            let v = check(&[c("lightcraft-engine", &[(dep, Normal, false)])]);
            assert!(matches!(v[..], [Violation::UiBelowL5 { .. }]), "{dep}");
        }
        assert!(check(&[c("lightcraft-mcp", &[("winit", Normal, false)])]).is_empty());
    }

    #[test]
    fn unregistered_and_testkit_rules() {
        let v = check(&[c("lightcraft-mystery", &[])]);
        assert!(matches!(&v[..], [Violation::Unregistered { krate }] if krate == "lightcraft-mystery"));
        assert!(!check(&[c("lightcraft-pipeline", &[("lightcraft-testkit", Normal, true)])]).is_empty());
        assert!(check(&[c("lightcraft-pipeline", &[("lightcraft-testkit", Dev, true)])]).is_empty());
    }

    #[test]
    fn apps_exempt() {
        for app in ["lightcraft", "lightcraft-cli", "lightcraft-web", "xtask"] {
            assert!(check(&[c(app, &[("egui", Normal, false), ("lightcraft-ui-egui", Normal, true)])]).is_empty());
        }
    }
}
