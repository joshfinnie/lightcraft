//! Serves SAM 3 on this Mac's Metal GPU to one LightCraft editor over a loopback socket (an SSH
//! tunnel); see docs/ai-masks.md → Remote Metal inference.
#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::disallowed_methods))]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // args_os: std::env::args panics on an argument that isn't valid Unicode
    let mut args = std::env::args_os().skip(1);
    let dir = std::path::PathBuf::from(args.next().ok_or("usage: lightcraft-sam3-worker MODEL_DIR [127.0.0.1:8793]")?);
    let address = match args.next() {
        Some(a) => a.to_str().ok_or("the address isn't valid text")?.parse()?,
        None => "127.0.0.1:8793".parse()?,
    };
    let device = candle_core::Device::new_metal(0)?;
    lightcraft_segment::remote::serve(address, &dir, device)?;
    Ok(())
}

/// The worker serves a local GPU over a socket; there is nothing to serve in a browser.
#[cfg(target_arch = "wasm32")]
fn main() {}
