//! `lightcraft.com`, beside `lightcraft.exe` in the Windows packages.
//!
//! A release `lightcraft.exe` is a GUI-subsystem program: started from a terminal it gets no
//! console, so `lightcraft render …` would print nothing. Windows runs `lightcraft.com` before
//! `lightcraft.exe` when a terminal names `lightcraft` (PATHEXT lists .COM first), and this console
//! program runs `lightcraft.exe` with the same arguments and the terminal's input and output (a
//! child given standard handles uses them, GUI program or not), then exits with its status.
//! Double-clicking `lightcraft.exe` still opens the app without a console.
#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::disallowed_methods))]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// The app beside this program (`lightcraft.exe`; `lightcraft` where executables have no suffix).
fn app_beside(me: &Path) -> PathBuf {
    me.with_file_name(format!("lightcraft{}", std::env::consts::EXE_SUFFIX))
}

fn main() {
    let app = match std::env::current_exe() {
        Ok(me) => app_beside(&me),
        Err(e) => {
            eprintln!("lightcraft: can't tell where this program is: {e}");
            std::process::exit(1);
        }
    };
    match Command::new(&app).args(std::env::args_os().skip(1)).status() {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("lightcraft: can't start {}: {e}", app.display());
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_is_the_program_beside_it() {
        let app = app_beside(Path::new("C:/Program Files/LightCraft/lightcraft.com"));
        assert_eq!(app, Path::new("C:/Program Files/LightCraft").join(format!("lightcraft{}", std::env::consts::EXE_SUFFIX)));
    }
}
