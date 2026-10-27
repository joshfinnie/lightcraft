//! The UI decides on what kind of error a command returned, not on its words (#1046): messages get
//! reworded and translated, and a decision that reads them breaks silently when they do.

use lightcraft_engine::segment::{DownloadStatus, NOT_INSTALLED, Segmenter};
use lightcraft_engine::{AiModel, ErrorKind};
use serde_json::json;

use crate::headless::Headless;
use crate::state::Dialog;
use crate::{CommandError, LightcraftApp, Services};

fn demo() -> Headless {
    let app = LightcraftApp::new(lightcraft_engine::Session::with_demo(), Services { png: None, ..Default::default() });
    Headless::new(app, [1200.0, 800.0], 1.0)
}

/// An AI mask error offers the model on `ModelMissing(Sam3)`, whatever the message says, and a
/// message that merely starts like the engine's is a toast.
#[test]
fn ai_mask_errors_offer_the_model_by_kind() {
    let mut h = demo();
    let ctx = h.view.ctx.clone();
    let missing = CommandError { message: "Das SAM-3-Modell ist nicht installiert".into(), kind: ErrorKind::ModelMissing(AiModel::Sam3) };
    h.app.ai_error(&ctx, missing, Some(("object", "new")));
    if Segmenter::AVAILABLE {
        assert_eq!(h.app.ui.dialog, Some(Dialog::SamModel { then: Some(("object".into(), "new".into())), error: None }));
    } else {
        assert_eq!(h.app.ui.dialog, None, "a build without AI masks has nothing to offer");
    }
    h.app.ui.dialog = None;
    h.app.ai_error(&ctx, format!("{NOT_INSTALLED}: but this is some other failure"), None);
    assert_eq!(h.app.ui.dialog, None, "decided on the kind, not the words");
    // another model missing is not SAM 3's
    let face = CommandError { message: "x".into(), kind: ErrorKind::ModelMissing(AiModel::FaceDetector) };
    h.app.ai_error(&ctx, face, None);
    assert_eq!(h.app.ui.dialog, None);
}

/// `run` keeps the engine's kind for the code that decides on it.
#[test]
fn run_keeps_the_engines_error_kind() {
    let mut h = demo();
    let dir = std::env::temp_dir().join(format!("lc-ui-error-kinds-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    h.app.session.face_models_dir = Some(dir.join("models"));
    let typed = h.app.run("faces.detect", json!({})).unwrap_err();
    assert_eq!(typed.kind, ErrorKind::ModelMissing(AiModel::FaceDetector), "{typed}");
    let unknown = h.app.run("no.such.command", json!({})).unwrap_err();
    assert_eq!(unknown.kind, ErrorKind::UnknownCommand, "{unknown}");
    // an error the UI raised itself is `Other`
    assert_eq!(CommandError::from("Tick the box to accept the model's terms first".to_string()).kind, ErrorKind::Other);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A UI command that runs an engine command keeps that command's kind: Export Keywords into a
/// folder that is a file is a file problem (`code: "io"`…), not `other`, as it was while UI
/// commands returned their errors as text.
#[test]
fn ui_commands_keep_the_kind_of_the_engine_error_they_pass_on() {
    let mut h = demo();
    let not_a_folder = std::env::temp_dir().join(format!("lc-ui-kinds-file-{}", std::process::id()));
    std::fs::write(&not_a_folder, b"a file").unwrap();
    let e = h.app.run("file.exportKeywords", json!({"path": not_a_folder.join("k.txt").to_string_lossy()})).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::File(_)), "{:?}: {e}", e.kind);
    let e = h.app.run("file.exportKeywords", json!({"path": 3})).unwrap_err();
    assert_eq!(e.kind, ErrorKind::BadParams, "{e}");
    let _ = std::fs::remove_file(&not_a_folder);
}

/// A control-channel error reply names its kind in `code` (and the model in `model`), so an agent
/// decides on it like the UI does.
#[test]
fn control_replies_carry_the_kind() {
    let mut h = demo();
    let t = std::time::Duration::from_secs(30);
    let r = h.request("engine.execute", json!({"command": "no.such"}), t);
    assert_eq!((r["ok"].as_bool(), r["code"].as_str()), (Some(false), Some("unknownCommand")), "{r}");
    let r = h.request("engine.execute", json!({"command": "photo.rate", "params": {"rating": "five"}}), t);
    assert_eq!(r["code"], "badParams", "{r}");
    let dir = std::env::temp_dir().join(format!("lc-ui-error-codes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    h.app.session.face_models_dir = Some(dir.join("models"));
    let r = h.request("engine.execute", json!({"command": "faces.detect"}), t);
    assert_eq!((r["code"].as_str(), r["model"].as_str()), (Some("modelMissing"), Some("faceDetector")), "{r}");
    let r = h.request("ui.zoom", json!({"factor": -1}), t);
    assert_eq!(r["code"], "other", "an error the UI raised itself: {r}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A UI command's error is logged and shown on the status line like an engine command's (B5: the
/// UI commands returned before either).
#[test]
fn a_ui_command_error_reaches_the_status_line() {
    let mut h = demo();
    h.app.ui.status.clear();
    let e = h.app.run("app.exportPrevious", json!({})).unwrap_err();
    assert_eq!(h.app.ui.status, e.message, "the UI command's error is on the status line");
    h.app.ui.status.clear();
    let e = h.app.run("photo.rate", json!({"rating": 9})).unwrap_err();
    assert_eq!(h.app.ui.status, e.message);
}

/// A SAM 3 download that ended while its dialog was closed: a cancel is told by the status's
/// `cancelled`, not by the error's words.
#[test]
fn a_cancelled_sam_download_is_told_by_its_flag() {
    let ended = |error: &str, cancelled: bool| {
        crate::sam_download_ended(&DownloadStatus { error: Some(error.into()), cancelled, ..DownloadStatus::default() }).unwrap()
    };
    let (text, error) = ended("Download abgebrochen", true);
    assert!(!error && text.starts_with("SAM 3 download stopped"), "{text}");
    let (text, error) = ended("could not download model.safetensors: the mirror cancelled the transfer", false);
    assert!(error && text.starts_with("The SAM 3 download failed: "), "{text}");
    assert_eq!(crate::sam_download_ended(&DownloadStatus::default()), None);
}
