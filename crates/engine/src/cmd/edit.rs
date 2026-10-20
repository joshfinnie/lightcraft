use serde_json::json;

use super::{CommandSpec, can_redo, can_undo, raw_cmd};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        raw_cmd!("edit.undo", "Undo", ["Edit"], Some("Cmd+Z"), "{}", can_undo, |s, _| {
            s.end_interaction()?;
            Ok(json!({"undone": s.undo_step()?}))
        }),
        raw_cmd!("edit.redo", "Redo", ["Edit"], Some("Cmd+Shift+Z"), "{}", can_redo, |s, _| Ok(json!({"redone": s.redo_step()?}))),
    ]
}
