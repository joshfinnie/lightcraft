//! The launcher runs the program beside it with its arguments and output, and exits with its status.

/// A stand-in `lightcraft` (a shell script) beside a copy of the launcher.
#[cfg(unix)]
#[test]
fn arguments_output_and_status_pass_through() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("lc-console-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let launcher = dir.join("lightcraft.com");
    std::fs::copy(env!("CARGO_BIN_EXE_lightcraft-console"), &launcher).unwrap();
    let app = dir.join("lightcraft");
    std::fs::write(&app, "#!/bin/sh\nprintf '%s|' \"$@\"\nread line\necho \"in:$line\"\necho err >&2\nexit 3\n").unwrap();
    std::fs::set_permissions(&app, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut child = std::process::Command::new(&launcher)
        .args(["render", "a photo.jpg", "-o", "out.jpg"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), b"hello\n").unwrap();
    let o = child.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(String::from_utf8_lossy(&o.stdout), "render|a photo.jpg|-o|out.jpg|in:hello\n");
    assert_eq!(String::from_utf8_lossy(&o.stderr), "err\n");
    assert_eq!(o.status.code(), Some(3));
}

#[cfg(unix)]
#[test]
fn a_missing_app_is_an_error() {
    let dir = std::env::temp_dir().join(format!("lc-console-missing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let launcher = dir.join("lightcraft.com");
    std::fs::copy(env!("CARGO_BIN_EXE_lightcraft-console"), &launcher).unwrap();
    let o = std::process::Command::new(&launcher).arg("commands").output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("can't start"), "{}", String::from_utf8_lossy(&o.stderr));
}
