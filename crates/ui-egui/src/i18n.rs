//! Presentation-only text. The UI is English only: every label still passes through [`tr`] and
//! [`tr_format!`] (and the helpers below), which return the English text, so there is one place
//! user-visible strings go through. Command ids, user names and catalog data are never touched.

/// A built-in display label. Never call this on editable user text, filenames or command identifiers.
pub fn tr(source: &str) -> &str {
    source
}

/// A built-in format string: `format!`, kept apart so user-visible messages are easy to find.
macro_rules! tr_format {
    ($($args:tt)*) => {
        format!($($args)*)
    };
}

pub(crate) use tr_format;

/// The title and text of the message box shown when the desktop app's window can't start
/// (issue #260): the error and, when there is one, the log file to look in.
pub fn startup_failed_message(error: &str, log_file: Option<&str>) -> (String, String) {
    let text = match log_file {
        Some(path) => tr_format!("LightCraft could not open its window: {e}\n\nThe log file has the details: {path}", e = error, path = path),
        None => tr_format!("LightCraft could not open its window: {e}", e = error),
    };
    (tr("LightCraft could not start").to_string(), text)
}

/// The title and text of the message box shown when LightCraft stopped while its window was up
/// (issue #1337: the window's renderer after the graphics driver reset the GPU): what happened and, when there is one, the log file to look in.
pub fn stopped_message(error: &str, log_file: Option<&str>) -> (String, String) {
    let text = match log_file {
        Some(path) => tr_format!("LightCraft stopped unexpectedly: {e}\n\nThe log file has the details: {path}", e = error, path = path),
        None => tr_format!("LightCraft stopped unexpectedly: {e}", e = error),
    };
    (tr("LightCraft stopped").to_string(), text)
}

/// Whether sentence case may upper-case `c`. Georgian has no case: upper-casing Mkhedruli would
/// turn it into Mtavruli, a different (all-caps) script, so Georgian letters stay as written.
pub fn capitalizes(c: char) -> bool {
    !matches!(c, '\u{10A0}'..='\u{10FF}' | '\u{1C90}'..='\u{1CBF}' | '\u{2D00}'..='\u{2D2F}')
}

/// `text` with its first letter upper-cased ("its input" → "Its input"), unless [`capitalizes`]
/// says that letter has no capital to take.
pub fn upper_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) if capitalizes(first) => first.to_uppercase().chain(chars).collect(),
        _ => text.to_string(),
    }
}

pub fn builtin_label(source: &str, builtin: bool) -> &str {
    if builtin { tr(source) } else { source }
}

/// A profile's name for display: a built-in profile's is translated, an imported LUT's is shown as
/// its file named it (it may happen to match a message, like "Vivid").
pub fn profile_label<'a>(id: &str, name: &'a str) -> &'a str {
    builtin_label(name, lightcraft_engine::presets::profile(id).is_some())
}

/// An Activity (history) step for display. The step's stored label stays English; generated steps
/// are translated around the names they carry, and a user preset's name is kept verbatim.
pub fn history_label(source: &str, presets: &[lightcraft_develop::Preset]) -> String {
    if let Some(name) = source.strip_prefix("Preset: ") {
        // A user preset may share a stock preset's name: then the step could be either, so it
        // keeps the name as written.
        let builtin = presets.iter().any(|p| p.builtin && p.name == name) && !presets.iter().any(|p| !p.builtin && p.name == name);
        return tr_format!("Preset: {name}", name = builtin_label(name, builtin));
    }
    if let Some(name) = source.strip_prefix("Reset ") {
        return tr_format!("Reset {name}", name = tr(name));
    }
    if let Some(name) = source.strip_prefix("Quick Develop: ") {
        return tr_format!("Quick Develop: {name}", name = tr(name));
    }
    tr(source).to_string()
}

/// A library source's heading (All Photos, Recently Deleted…) in the UI language; an album's
/// name is the user's and stays verbatim.
pub fn source_label(source: lightcraft_engine::LibrarySource, catalog: &lightcraft_catalog::Catalog) -> String {
    let label = source.label(catalog);
    if matches!(source, lightcraft_engine::LibrarySource::Album(id) if catalog.album(id).is_some()) { label } else { tr(&label).to_string() }
}

/// What the grid is titled for the session's source: [`source_label`], or a library folder's last
/// two names (`photos/travel`; the folder's name is the user's and stays verbatim).
pub fn source_title(session: &lightcraft_engine::Session) -> String {
    match (session.source, session.library_folder.as_deref()) {
        (lightcraft_engine::LibrarySource::LibraryFolder, Some(path)) => lightcraft_catalog::folders::folder_label(path),
        (source, _) => source_label(source, &session.catalog),
    }
}

/// A date group heading (`2026-09-20`, `2026-09`, `2026`): the grid's full form, or the date
/// sidebar's `short` one ("Sunday, 20" / "September" / "2026"). The grouping keys never change.
pub fn date_group_label(key: &str, short: bool) -> String {
    let label = lightcraft_catalog::dates::group_label(key);
    if !short {
        return label;
    }
    match key.len() {
        // "September 2026" → "September"
        7 => label.split(' ').next().unwrap_or(&label).to_string(),
        // "Sunday, 20 September 2026" → "Sunday, 20"
        10 => label.rsplitn(3, ' ').nth(2).unwrap_or(&label).to_string(),
        _ => label,
    }
}

/// A capture time for display ("March 30, 2022 at 10:11:11 PM"); the metadata itself is never
/// rewritten, and a value that doesn't parse is shown as it is.
pub fn display_time(iso: &str) -> String {
    lightcraft_catalog::dates::display_time(iso)
}

/// Built-in colour names are translated; custom label names are user data.
pub fn color_label(catalog: &lightcraft_catalog::Catalog, label: lightcraft_catalog::ColorLabel) -> String {
    catalog.custom_label_name(label).map_or_else(|| tr(&catalog.label_name(label)).to_string(), str::to_string)
}

/// A legacy smart-album filter summary, using the same display labels as filter chips.
pub fn filter_label(filter: &lightcraft_catalog::Filter, catalog: &lightcraft_catalog::Catalog) -> String {
    filter.describe_with(catalog)
}

/// What people read for a smart-album rule's problem, under its row: the field's name and the
/// issue ("Title: needs something to look for").
pub fn problem_text(problem: &lightcraft_catalog::rules::Problem) -> String {
    let issue = problem.issue.text();
    match problem.field.as_deref() {
        Some(field) => format!("{}: {issue}", lightcraft_catalog::rules::field_label(field).unwrap_or(field)),
        None => issue.to_string(),
    }
}

/// [`problem_text`] with the rule's position first ("#2.1 Rating: …"), for lists away from the
/// editor (the sidebar's tooltip).
pub fn problem_line(problem: &lightcraft_catalog::rules::Problem) -> String {
    let at: Vec<String> = problem.path.iter().map(|i| i.saturating_add(1).to_string()).collect();
    // a problem with the album filter around the rules has no rule to point at
    if at.is_empty() {
        return problem_text(problem);
    }
    format!("#{} {}", at.join("."), problem_text(problem))
}

/// What people read for smart-album choice `id` of `field`; an id the field doesn't know, as written.
pub fn choice_text<'a>(field: &str, id: &'a str) -> &'a str {
    lightcraft_catalog::rules::choice_label(field, id).unwrap_or(id)
}

/// What people read for a smart-album yes/no value.
pub fn bool_text(b: bool) -> &'static str {
    lightcraft_catalog::rules::bool_label(b)
}

/// A rule summary for display, keeping free-text rule values verbatim.
pub fn rules_label(rules: &lightcraft_catalog::RuleSet, catalog: &lightcraft_catalog::Catalog) -> String {
    rules.describe_with(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Settings written when the UI had a language setting still read; the key is not written back.
    #[test]
    fn a_saved_language_is_ignored() {
        for saved in [r#"{"language":"ja","leftWidth":300.0}"#, r#"{"language":42,"leftWidth":300.0}"#] {
            let ui: crate::state::UiState = serde_json::from_str(saved).unwrap();
            assert_eq!(ui.left_width, 300.0, "{saved}");
            assert!(serde_json::to_value(&ui).unwrap().get("language").is_none());
        }
        assert_eq!(tr("Export"), "Export");
        assert_eq!(tr_format!("{} photos", 3), "3 photos");
    }
}
