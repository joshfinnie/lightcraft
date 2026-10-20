//! Every call to an engine command written out in the repository passes only keys the command
//! takes (see `lightcraft_testkit::calls`): a key it doesn't is refused at run time, so a front end
//! or doc example that sends one is broken.

#[test]
fn written_out_command_calls_pass_only_keys_their_command_takes() {
    let (mut checked, mut bad) = (0, Vec::new());
    for (file, line, id, keys) in lightcraft_testkit::calls::repo_calls() {
        let Some(spec) = lightcraft_engine::cmd::find_command(&id) else { continue };
        let Some(takes) = (spec.keys)() else { continue };
        checked += 1;
        for k in keys.iter().filter(|k| !takes.contains(&k.as_str())) {
            bad.push(format!("{file}:{line}: {id} doesn't take `{k}` (takes {takes:?})"));
        }
    }
    assert!(checked > 0, "no engine command calls found");
    assert!(bad.is_empty(), "{} call(s) pass keys their command doesn't take:\n{}", bad.len(), bad.join("\n"));
}
