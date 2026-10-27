# Set up a machine to build, test and fuzz LightCraft, then build it: run `make`.
# Every step is safe to repeat; `make help` lists the targets. Windows: GNU make in Git Bash.

RUST_MIN := 1.95
VERSION := $(shell sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
# extra raw samples for local smoke tests and fuzz seeds; their licence says only "Creative
# Commons", so they stay outside the repository and CI never pins them
SAMPLES_DIR ?= ../lightcraft-samples
SAMPLES_URL ?= https://github.com/f-spot/raw-samples
SAMPLES_REV ?= 54425e2cd53ec17d5fd88ee3277d03f54bf1fe84
WASM_BINDGEN := $(shell sed -n '/^name = "wasm-bindgen"$$/{n;s/^version = "\(.*\)"/\1/p;}' Cargo.lock)
# (a rustup installed by `make rust` is usable at once)
export PATH := $(HOME)/.cargo/bin:$(PATH)

.DEFAULT_GOAL := all
.PHONY: all setup rust tools corpus samples seeds build run web check test slow-tests smoke fuzz coverage release publish help

all: setup build ## everything below, then a release build of the app and the CLI

setup: rust tools corpus samples seeds ## toolchains, tools, test corpora and fuzz seeds

rust: ## Rust (stable >= 1.95 and nightly) and the wasm32 target, through rustup
	@command -v rustup >/dev/null 2>&1 || { \
	  case "$$(uname -s)" in \
	    MINGW*|MSYS*|CYGWIN*) echo "Install Rust from https://rustup.rs (rustup-init.exe), then run make again."; exit 1;; \
	    *) curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal;; \
	  esac; }
	@rustup toolchain install stable --profile minimal --component rustfmt,clippy && \
	rustup toolchain install nightly --profile minimal && \
	rustup target add wasm32-unknown-unknown --toolchain stable && \
	have=$$(rustc +stable --version | cut -d' ' -f2); \
	if [ "$$(printf '%s\n%s\n' "$(RUST_MIN)" "$$have" | sort -V | head -1)" != "$(RUST_MIN)" ]; then \
	  rustup update stable; fi; \
	echo "Rust: $$(rustc +stable --version)"

tools: rust ## wasm-bindgen (the version in Cargo.lock), cargo-fuzz, cargo-llvm-cov
	@if [ "$$(wasm-bindgen --version 2>/dev/null | cut -d' ' -f2)" != "$(WASM_BINDGEN)" ]; then \
	  cargo install wasm-bindgen-cli --version $(WASM_BINDGEN) --locked; fi
	@command -v cargo-fuzz >/dev/null 2>&1 || cargo install cargo-fuzz --locked
	@command -v cargo-llvm-cov >/dev/null 2>&1 || cargo install cargo-llvm-cov --locked
	@rustup component add llvm-tools-preview --toolchain stable >/dev/null
	@echo "Tools: wasm-bindgen $(WASM_BINDGEN), cargo-fuzz, cargo-llvm-cov"

# best effort: raw.pixls.us is sometimes unreachable, and tests skip what is missing
corpus: ## PngSuite and CC0 sample raws into corpus/ (checked by sha256)
	@cargo xtask corpus --download || echo "corpus: download incomplete; the tests that need it skip. Run 'make corpus' again later."

# best effort, like the corpus: only the pinned commit is fetched, and a failed download says so
samples: ## extra raw samples (f-spot/raw-samples, pinned) next to this repo
	@if [ "$$(git -C "$(SAMPLES_DIR)" rev-parse HEAD 2>/dev/null)" = "$(SAMPLES_REV)" ]; then \
	  echo "Samples: $(SAMPLES_DIR) at $(SAMPLES_REV)"; \
	elif mkdir -p "$(SAMPLES_DIR)" && git -C "$(SAMPLES_DIR)" init --quiet && \
	  git -C "$(SAMPLES_DIR)" fetch --quiet --depth 1 "$(SAMPLES_URL)" $(SAMPLES_REV) && \
	  git -C "$(SAMPLES_DIR)" checkout --quiet --detach FETCH_HEAD; then \
	  echo "Samples: $(SAMPLES_DIR) at $(SAMPLES_REV)"; \
	else echo "samples: download incomplete; run 'make samples' again later."; fi

seeds: ## starting files for the fuzzers (fuzz/corpus/), the samples among them
	@cd fuzz && cargo +nightly run --quiet --example seeds
	@if [ -d "$(SAMPLES_DIR)" ]; then for t in raw_decode raw_probe; do \
	  find "$(SAMPLES_DIR)" -path '*/.git' -prune -o -type f ! -name '*.md' -exec cp -f {} fuzz/corpus/$$t/ \; ; done; fi

build: ## release build of the app (its subcommands are the CLI)
	cargo build --release -p lightcraft

run: ## open the app (release build)
	cargo run --release -p lightcraft

web: ## the browser build, served at http://127.0.0.1:8080/
	cargo xtask web --serve

check: ## the full quality gate: fmt, clippy, tests, parity, layers, assets, wasm
	cargo xtask ci

test: ## the workspace tests only
	cargo test --workspace

slow-tests: ## the 20 slowest tests, to see where test time goes (make slow-tests N=40)
	cargo xtask slow-tests $(or $(N),20)

smoke: samples seeds ## decode and develop every sample and seed once: no panic, no runaway memory or time
	cd fuzz && cargo +nightly fuzz run raw_decode -O corpus/raw_decode -- -runs=0 -rss_limit_mb=4096 -timeout=30

FUZZ_TARGET ?= raw_decode
FUZZ_SECONDS ?= 300
fuzz: seeds ## fuzz one parser: make fuzz FUZZ_TARGET=codecs_decode FUZZ_SECONDS=600
	cd fuzz && cargo +nightly fuzz run $(FUZZ_TARGET) -O -- -max_total_time=$(FUZZ_SECONDS) -rss_limit_mb=4096 -timeout=10 -max_len=262144

# release: instrumented debug builds run some tests ~25x slower, past their deadlines;
# --no-fail-fast: one failing test binary must not leave the rest unmeasured
coverage: ## line coverage of the workspace tests, per file (HTML report: target/llvm-cov/html)
	cargo llvm-cov --workspace --release --no-fail-fast --html
	cargo llvm-cov report --release --summary-only

# Release: packages for the platform this runs on, into dist/release/ (see docs/releasing.md).
# macOS signs ad hoc unless MACOS_SIGN_IDENTITY is set, and notarizes with APPLE_ID,
# APPLE_PASSWORD and APPLE_TEAM_ID; Windows signs when sign.ps1's variables are set.
RELEASE_ARCH ?=
release: tools ## installers for this platform, the web zip and SHA256SUMS.txt in dist/release/
	@echo "Release $(VERSION)"
	@case "$$(uname -s)" in \
	  Darwin) packaging/macos/package.sh --arch $${RELEASE_ARCH:-universal};; \
	  Linux) packaging/linux/package.sh;; \
	  MINGW*|MSYS*|CYGWIN*) pwsh -NoProfile -File packaging/windows/package.ps1 -Arch $${RELEASE_ARCH:-x64};; \
	  *) echo "release: no packaging for $$(uname -s)"; exit 1;; \
	esac
	packaging/web/package.sh
	@cd dist/release && rm -f SHA256SUMS.txt && \
	  { command -v sha256sum >/dev/null 2>&1 && sha256sum * || shasum -a 256 *; } > SHA256SUMS.txt.tmp && \
	  mv SHA256SUMS.txt.tmp SHA256SUMS.txt && cat SHA256SUMS.txt

# a draft, so nothing is public until you publish it on GitHub; run once per platform's dist/
publish: ## upload dist/release/ to a draft GitHub release v<version> (gh CLI)
	@command -v gh >/dev/null 2>&1 || { echo "publish: install the GitHub CLI (gh) and run gh auth login"; exit 1; }
	@if gh release view "v$(VERSION)" >/dev/null 2>&1; then \
	  gh release upload "v$(VERSION)" dist/release/* --clobber; \
	else gh release create "v$(VERSION)" dist/release/* --draft --title "LightCraft v$(VERSION)" \
	  $$(case "$(VERSION)" in *-*) echo --prerelease;; esac) --generate-notes; fi

help: ## list the targets
	@grep -E '^[a-z]+:.*## ' $(MAKEFILE_LIST) | sed 's/:.*## /\t/' | expand -t 10
