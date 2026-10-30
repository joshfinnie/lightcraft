# Fuzzing

Coverage-guided fuzzing of the parsers that read files LightCraft didn't write, with
[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) (libFuzzer). This is tooling, not product:
the crate is its own workspace and needs nightly Rust; nothing in the app depends on it.

| Target | What it runs |
|---|---|
| `raw_decode` | `lightcraft_raw::decode`, the embedded preview, and developing images up to 64 K pixels |
| `raw_probe` | `lightcraft_raw::probe` and the headers-only `probe_info` used by import |
| `codecs_decode` | `lightcraft_codecs::decode` (JPEG, PNG, TIFF, WebP, PSD, HEIF, JPEG XL, and raws through it) |
| `meta_extract` | `lightcraft_meta::extract`, `read_exif`, `parse_iptc` and `parse_xmp` |

```text
cargo install cargo-fuzz --locked
cd fuzz
cargo +nightly run --example seeds   # small valid files of every kind into corpus/<target>/
cargo +nightly fuzz run raw_decode -O -- -max_total_time=300 -rss_limit_mb=4096 -timeout=10 -max_len=262144
```

From the repository root, `make seeds` writes the seeds and `make fuzz FUZZ_TARGET=codecs_decode FUZZ_SECONDS=600`
runs one target (`make smoke` decodes every sample and seed once).

`-rss_limit_mb` and `-timeout` make an allocation sized by a header, or a decode that never ends,
a finding like a panic. A finding is written to `artifacts/<target>/`; replay it with
`cargo +nightly fuzz run <target> artifacts/<target>/<file>`, then add a small synthetic
regression test next to the parser that failed (the never-crash rule in `CLAUDE.md`). `corpus/`,
`artifacts/` and `target/` are not committed: findings are turned into tests, and media never
enter the repository.
