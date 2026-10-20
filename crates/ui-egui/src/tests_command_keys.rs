//! Every call to a UI command written out in the repository passes only keys the command takes
//! (see `lightcraft_testkit::calls`): a key it doesn't is refused at run time.

#[test]
fn written_out_ui_command_calls_pass_only_keys_their_command_takes() {
    let (mut checked, mut bad) = (0, Vec::new());
    for (file, line, id, keys) in lightcraft_testkit::calls::repo_calls() {
        let Some(command) = crate::menus::ui_commands().find(|c| c.id == id) else { continue };
        let Some(takes) = (command.keys)() else { continue };
        checked += 1;
        for k in keys.iter().filter(|k| !takes.contains(&k.as_str())) {
            bad.push(format!("{file}:{line}: {id} doesn't take `{k}` (takes {takes:?})"));
        }
    }
    assert!(checked > 0, "no UI command calls found");
    assert!(bad.is_empty(), "{} call(s) pass keys their command doesn't take:\n{}", bad.len(), bad.join("\n"));
}
