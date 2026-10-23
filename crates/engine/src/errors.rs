//! What kind of error a command returned, for code that decides on it (#1046).
//!
//! A frontend that reacts to an error (offers a model download, says "cancelled" in its own words
//! instead of showing a failure) matches [`EngineError::kind`], never the message: messages are
//! for people, get reworded, and will be translated.

use serde_json::{Value, json};

use crate::EngineError;

/// A model a feature needs and LightCraft downloads only when the person agrees.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AiModel {
    /// YuNet, which finds faces (`faces.detect`; installed in Settings ▸ Faces).
    FaceDetector,
    /// SAM 3, behind Object and Describe masks (`segment.model.download`).
    Sam3,
}

impl AiModel {
    /// The model's id in an error reply's `model` field.
    pub fn id(self) -> &'static str {
        match self {
            Self::FaceDetector => "faceDetector",
            Self::Sam3 => "sam3",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        [Self::FaceDetector, Self::Sam3].into_iter().find(|m| m.id() == id)
    }
}

/// What went wrong with a file the work read or wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileProblem {
    /// The file (or its folder) is not there: moved, deleted, or on a disk that is not connected.
    NotFound,
    PermissionDenied,
    /// The disk (or the browser's storage quota) is full.
    DiskFull,
    /// The volume is mounted read-only.
    ReadOnly,
    /// A format, or a variant of one, LightCraft does not read.
    Unsupported,
    /// The file is damaged or truncated.
    Corrupt,
    /// Larger than LightCraft opens (pixels, or a size the file claims).
    TooLarge,
    /// Any other input/output failure.
    Io,
}

impl FileProblem {
    /// The problem's `code` in an error reply.
    pub fn code(self) -> &'static str {
        match self {
            Self::NotFound => "notFound",
            Self::PermissionDenied => "permissionDenied",
            Self::DiskFull => "diskFull",
            Self::ReadOnly => "readOnly",
            Self::Unsupported => "unsupportedFile",
            Self::Corrupt => "corruptFile",
            Self::TooLarge => "tooLarge",
            Self::Io => "io",
        }
    }

    const ALL: [Self; 8] =
        [Self::NotFound, Self::PermissionDenied, Self::DiskFull, Self::ReadOnly, Self::Unsupported, Self::Corrupt, Self::TooLarge, Self::Io];

    /// The problem an I/O error is.
    pub fn of_io(e: &std::io::Error) -> Self {
        use std::io::ErrorKind as K;
        match e.kind() {
            K::NotFound | K::NotADirectory => Self::NotFound,
            K::PermissionDenied => Self::PermissionDenied,
            K::StorageFull | K::QuotaExceeded => Self::DiskFull,
            K::ReadOnlyFilesystem => Self::ReadOnly,
            K::FileTooLarge => Self::TooLarge,
            K::InvalidData | K::UnexpectedEof => Self::Corrupt,
            _ => Self::Io,
        }
    }
}

/// The kind of an [`EngineError`] (or of an error a frontend raised itself: [`ErrorKind::Other`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    UnknownCommand,
    Disabled,
    BadParams,
    Catalog,
    NotSaved,
    LibraryInUse,
    LightroomCatalog,
    /// The work was cancelled: not a failure.
    Cancelled,
    /// The feature needs this model, which is not installed (or still downloading).
    ModelMissing(AiModel),
    /// A file could not be read, decoded or written.
    File(FileProblem),
    Other,
}

impl ErrorKind {
    /// The kind's stable name in an error reply's `code` field (control channel, MCP, CLI `--json`).
    pub fn code(self) -> &'static str {
        match self {
            Self::UnknownCommand => "unknownCommand",
            Self::Disabled => "disabled",
            Self::BadParams => "badParams",
            Self::Catalog => "catalog",
            Self::NotSaved => "notSaved",
            Self::LibraryInUse => "libraryInUse",
            Self::LightroomCatalog => "lightroomCatalog",
            Self::Cancelled => "cancelled",
            Self::ModelMissing(_) => "modelMissing",
            Self::File(problem) => problem.code(),
            Self::Other => "other",
        }
    }

    /// The kind an error reply names (`code`, and `model` for a missing model); [`ErrorKind::Other`]
    /// for a code this version doesn't know.
    pub fn from_reply(reply: &Value) -> Self {
        let code = reply.get("code").and_then(Value::as_str).unwrap_or_default();
        if code == "modelMissing" {
            return reply.get("model").and_then(Value::as_str).and_then(AiModel::from_id).map_or(Self::Other, Self::ModelMissing);
        }
        Self::PLAIN.iter().copied().chain(FileProblem::ALL.map(Self::File)).find(|k| k.code() == code).unwrap_or(Self::Other)
    }

    /// Every kind but [`ErrorKind::ModelMissing`] (one per model) and [`ErrorKind::File`] (one per problem).
    const PLAIN: &[Self] = &[
        Self::UnknownCommand,
        Self::Disabled,
        Self::BadParams,
        Self::Catalog,
        Self::NotSaved,
        Self::LibraryInUse,
        Self::LightroomCatalog,
        Self::Cancelled,
        Self::Other,
    ];
}

/// A refused or failed command as a frontend handles it (#1046): the message for people, and what
/// kind of error it is for code that decides on it. Decide on [`CommandError::kind`], never on the
/// message: messages get reworded and will be translated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandError {
    pub message: String,
    pub kind: ErrorKind,
}

impl CommandError {
    /// The error fields of a reply: `error` (the message), `code` ([`ErrorKind::code`]) and, for a
    /// missing model, `model` ([`AiModel::id`]).
    pub fn fields(&self) -> serde_json::Map<String, Value> {
        let mut m = serde_json::Map::new();
        m.insert("error".into(), json!(self.message));
        m.insert("code".into(), json!(self.kind.code()));
        if let ErrorKind::ModelMissing(model) = self.kind {
            m.insert("model".into(), json!(model.id()));
        }
        m
    }

    /// A control-channel reply refusing the request: `{ok: false, error, code, model?}`.
    pub fn reply(&self) -> Value {
        let mut m = self.fields();
        m.insert("ok".into(), json!(false));
        Value::Object(m)
    }

    /// The error a reply carries ([`CommandError::fields`]); `None` when it has no `error` text.
    pub fn from_reply(reply: &Value) -> Option<Self> {
        let message = reply.get("error")?.as_str()?.to_string();
        Some(Self { message, kind: ErrorKind::from_reply(reply) })
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CommandError {}

/// An error a frontend raised itself (its own commands, dialogs): [`ErrorKind::Other`].
impl From<String> for CommandError {
    fn from(message: String) -> Self {
        Self { message, kind: ErrorKind::Other }
    }
}

impl From<&str> for CommandError {
    fn from(message: &str) -> Self {
        message.to_string().into()
    }
}

impl From<EngineError> for CommandError {
    fn from(e: EngineError) -> Self {
        Self { kind: e.kind(), message: e.to_string() }
    }
}

impl EngineError {
    /// A file problem, `msg` saying which file and what happened.
    pub fn file(problem: FileProblem, msg: impl Into<String>) -> Self {
        Self::File { problem, msg: msg.into() }
    }

    /// This error said differently: a file problem stays that problem, anything else becomes [`EngineError::Other`].
    pub fn reworded(&self, msg: impl Into<String>) -> Self {
        match self {
            Self::File { problem, .. } => Self::file(*problem, msg),
            _ => Self::Other(msg.into()),
        }
    }

    /// This error about `what` (a file, a photo): `"{what}: {self}"`, of the same kind when it is a file problem.
    pub fn about(&self, what: impl std::fmt::Display) -> Self {
        self.reworded(format!("{what}: {self}"))
    }

    /// `e`, from reading or writing `what` (a path, or what the file is).
    pub fn io(what: impl std::fmt::Display, e: &std::io::Error) -> Self {
        Self::file(FileProblem::of_io(e), format!("{what}: {e}"))
    }
}

impl From<lightcraft_codecs::Error> for EngineError {
    fn from(e: lightcraft_codecs::Error) -> Self {
        use lightcraft_codecs::Error as E;
        let problem = match &e {
            E::UnknownFormat | E::Unsupported(..) => FileProblem::Unsupported,
            E::Malformed(..) => FileProblem::Corrupt,
            E::TooLarge(..) => FileProblem::TooLarge,
            E::Encode(_) => return Self::Other(e.to_string()),
        };
        Self::file(problem, e.to_string())
    }
}

impl From<lightcraft_raw::RawError> for EngineError {
    fn from(e: lightcraft_raw::RawError) -> Self {
        use lightcraft_raw::RawError as E;
        let problem = match &e {
            E::NotRaw | E::Unsupported(_) => FileProblem::Unsupported,
            E::Corrupt(_) | E::Tiff(_) => FileProblem::Corrupt,
            E::Limit(_) | E::TooLarge(..) => FileProblem::TooLarge,
        };
        Self::file(problem, e.to_string())
    }
}

/// An error reported as text (code that does not say what kind it is): [`EngineError::Other`].
impl From<String> for EngineError {
    fn from(msg: String) -> Self {
        Self::Other(msg)
    }
}

impl From<&str> for EngineError {
    fn from(msg: &str) -> Self {
        Self::Other(msg.into())
    }
}

/// For code that still reports errors as text: the message.
impl From<EngineError> for String {
    fn from(e: EngineError) -> Self {
        e.to_string()
    }
}

impl From<CommandError> for String {
    fn from(e: CommandError) -> Self {
        e.message
    }
}

impl EngineError {
    /// What kind of error this is: decide on this, not on the message.
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::UnknownCommand(_) => ErrorKind::UnknownCommand,
            Self::Disabled(..) => ErrorKind::Disabled,
            Self::BadParams { .. } => ErrorKind::BadParams,
            Self::Catalog(_) => ErrorKind::Catalog,
            Self::NotSaved(_) => ErrorKind::NotSaved,
            Self::LibraryInUse(_) => ErrorKind::LibraryInUse,
            Self::LightroomCatalog { .. } => ErrorKind::LightroomCatalog,
            Self::Cancelled(_) => ErrorKind::Cancelled,
            Self::ModelMissing { model, .. } => ErrorKind::ModelMissing(*model),
            Self::File { problem, .. } => ErrorKind::File(*problem),
            Self::Other(_) => ErrorKind::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kind_does_not_depend_on_the_message() {
        let cancelled = EngineError::Cancelled("Lightroom-Import abgebrochen".into());
        assert_eq!(cancelled.kind(), ErrorKind::Cancelled);
        assert_eq!(cancelled.to_string(), "Lightroom-Import abgebrochen", "the message is shown as is");
        let missing = EngineError::ModelMissing { model: AiModel::Sam3, msg: "any wording".into() };
        assert_eq!(missing.kind(), ErrorKind::ModelMissing(AiModel::Sam3));
        assert_eq!(missing.to_string(), "any wording");
        // an `Other` whose text reads like a cancellation or a missing model is still `Other`
        assert_eq!(EngineError::Other("The SAM 3 model is not installed; cancelled".into()).kind(), ErrorKind::Other);
    }

    #[test]
    fn a_reply_names_the_kind_and_reads_back_as_it() {
        let plain = ErrorKind::PLAIN.iter().map(|k| k.code()).collect::<std::collections::HashSet<_>>();
        assert_eq!(plain.len(), ErrorKind::PLAIN.len(), "one code per kind");
        let models = [ErrorKind::ModelMissing(AiModel::FaceDetector), ErrorKind::ModelMissing(AiModel::Sam3)];
        let files = FileProblem::ALL.map(ErrorKind::File);
        let all = plain.iter().copied().chain(files.iter().map(|k| k.code())).collect::<std::collections::HashSet<_>>();
        assert_eq!(all.len(), plain.len() + files.len(), "a file problem's code is no other kind's");
        for kind in ErrorKind::PLAIN.iter().copied().chain(models).chain(files) {
            let reply = CommandError { message: "why".into(), kind }.reply();
            assert_eq!(reply["ok"], false);
            assert_eq!(reply["error"], "why");
            assert_eq!(CommandError::from_reply(&reply), Some(CommandError { message: "why".into(), kind }), "{reply}");
        }
        assert_eq!(CommandError::from_reply(&json!({"ok": false, "error": "old app"})).map(|e| e.kind), Some(ErrorKind::Other));
        assert_eq!(ErrorKind::from_reply(&json!({"code": "fromTheFuture"})), ErrorKind::Other);
        assert_eq!(ErrorKind::from_reply(&json!({"code": "modelMissing", "model": "gpt"})), ErrorKind::Other);
        assert_eq!(CommandError::from_reply(&json!({"ok": false})), None);
    }

    #[test]
    fn file_errors_say_what_went_wrong() {
        use std::io::{Error, ErrorKind as K};
        let kind = |k: K| EngineError::io("/x/a.jpg", &Error::from(k)).kind();
        assert_eq!(kind(K::NotFound), ErrorKind::File(FileProblem::NotFound));
        assert_eq!(kind(K::PermissionDenied), ErrorKind::File(FileProblem::PermissionDenied));
        assert_eq!(kind(K::StorageFull), ErrorKind::File(FileProblem::DiskFull));
        assert_eq!(kind(K::ReadOnlyFilesystem), ErrorKind::File(FileProblem::ReadOnly));
        assert_eq!(kind(K::Interrupted), ErrorKind::File(FileProblem::Io));
        #[cfg(unix)]
        assert_eq!(EngineError::io("x", &Error::from_raw_os_error(28)).kind(), ErrorKind::File(FileProblem::DiskFull), "ENOSPC");
        assert!(EngineError::io("/x/a.jpg", &Error::from(K::NotFound)).to_string().starts_with("/x/a.jpg: "));
        let codec = |e: lightcraft_codecs::Error| EngineError::from(e).kind();
        assert_eq!(codec(lightcraft_codecs::Error::UnknownFormat), ErrorKind::File(FileProblem::Unsupported));
        assert_eq!(codec(lightcraft_codecs::Error::Malformed(lightcraft_codecs::Format::Jpeg, "x".into())), ErrorKind::File(FileProblem::Corrupt));
        assert_eq!(codec(lightcraft_codecs::Error::TooLarge(1, 1)), ErrorKind::File(FileProblem::TooLarge));
        assert_eq!(codec(lightcraft_codecs::Error::Encode("x".into())), ErrorKind::Other, "encoding is not a file problem");
        let raw = |e: lightcraft_raw::RawError| EngineError::from(e).kind();
        assert_eq!(raw(lightcraft_raw::RawError::Corrupt("x".into())), ErrorKind::File(FileProblem::Corrupt));
        assert_eq!(raw(lightcraft_raw::RawError::Unsupported("x".into())), ErrorKind::File(FileProblem::Unsupported));
        assert_eq!(raw(lightcraft_raw::RawError::Limit("x")), ErrorKind::File(FileProblem::TooLarge));
    }
}
