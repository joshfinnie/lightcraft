//! UI commands for the views, zoom, overlays, panels, sections and tools.

use serde_json::{Value, json};

use crate::LightcraftApp;
use crate::menus::*;
use crate::pick::{PickRequest, Picked};
use crate::state::{BeforeAfter, Dialog, RightPanel, ViewMode, Zoom};
use lightcraft_engine::cmd::params::{Keys, NoParams};

pub(crate) fn view_photo_grid(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.view = ViewMode::PhotoGrid;
    Ok(Value::Null)
}

pub(crate) fn view_square_grid(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.view = ViewMode::SquareGrid;
    Ok(Value::Null)
}

pub(crate) fn view_grid_info(app: &mut LightcraftApp, _: &Call, p: GridInfo) -> Result<Value, String> {
    // {info?: filename | exposure | date} (cycles when omitted)
    let next = match p.info.as_deref() {
        Some(i @ ("filename" | "exposure" | "date")) => i.to_string(),
        Some(other) => return Err(format!("view.gridInfo: unknown `{other}` (filename|exposure|date)")),
        None => match app.ui.grid_info.as_str() {
            "filename" => "exposure".into(),
            "exposure" => "date".into(),
            _ => "filename".into(),
        },
    };
    app.ui.grid_info = next;
    app.ui.show_filenames = true;
    // the caption is drawn on Square Grid cells only: from Photo Grid, show it there
    if app.ui.view == ViewMode::PhotoGrid {
        app.ui.view = ViewMode::SquareGrid;
    }
    Ok(json!({"info": app.ui.grid_info}))
}

pub(crate) fn tool_keyword_painter(app: &mut LightcraftApp, _: &Call, p: KeywordPainter) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    // {keyword?}: paint that keyword onto photos in the grid by clicking them; no keyword stops
    app.ui.keyword_painter = p.keyword.as_deref().map(str::trim).filter(|k| !k.is_empty()).map(str::to_string);
    if app.ui.keyword_painter.is_some() && !matches!(app.ui.view, ViewMode::PhotoGrid | ViewMode::SquareGrid) {
        app.ui.view = ViewMode::PhotoGrid;
    }
    if let Some(k) = app.ui.keyword_painter.clone() {
        app.toast(&ctx, crate::i18n::tr_format!("Painting “{k}”: click photos to add or remove it · Esc stops", k = k));
    }
    Ok(json!({"keyword": app.ui.keyword_painter}))
}

pub(crate) fn view_second_window(app: &mut LightcraftApp, _: &Call, p: Show) -> Result<Value, String> {
    app.ui.second_window = p.show.unwrap_or(!app.ui.second_window);
    Ok(json!({"show": app.ui.second_window}))
}

pub(crate) fn view_photo_counts(app: &mut LightcraftApp, _: &Call, p: Show) -> Result<Value, String> {
    app.ui.show_counts = p.show.unwrap_or(!app.ui.show_counts);
    Ok(json!({"show": app.ui.show_counts}))
}

pub(crate) fn view_sidebar_section(app: &mut LightcraftApp, c: &Call, p: SidebarSection) -> Result<Value, String> {
    let section = sidebar_section(c.id, p.section.as_deref())?;
    let layout = &mut app.ui.sidebar;
    match (p.show, p.collapsed) {
        (None, None) => layout.set_hidden(section, !layout.is_hidden(section)),
        (show, collapsed) => {
            if let Some(show) = show {
                layout.set_hidden(section, !show);
            }
            if let Some(collapsed) = collapsed {
                layout.set_collapsed(section, collapsed);
            }
        }
    }
    Ok(json!({"section": section.id(), "show": !layout.is_hidden(section), "collapsed": layout.is_collapsed(section)}))
}

/// A section of the left sidebar by id, one this build has.
fn sidebar_section(command: &str, id: Option<&str>) -> Result<crate::sidebar::SidebarSection, String> {
    use crate::sidebar::SidebarSection;
    let id = id.ok_or_else(|| format!("{command}: `section` is needed ({})", SidebarSection::ids()))?;
    let section = SidebarSection::from_id(id).ok_or_else(|| format!("{command}: unknown section `{id}` ({})", SidebarSection::ids()))?;
    if !section.available() {
        return Err(format!("{command}: there is no `{id}` section here"));
    }
    Ok(section)
}

pub(crate) fn view_sidebar_move_section(app: &mut LightcraftApp, c: &Call, p: SidebarMove) -> Result<Value, String> {
    // {section, direction: up | down} one place among the shown sections, or
    // {section, before: <section> | null} just above another section (null: last)
    let id = c.id;
    let section = sidebar_section(id, p.section.as_deref())?;
    let layout = &mut app.ui.sidebar;
    let moved = match (p.direction.as_deref(), &p.before) {
        (Some("up"), None) => layout.move_by(section, true),
        (Some("down"), None) => layout.move_by(section, false),
        (Some(d), None) => return Err(format!("{id}: `direction` must be \"up\" or \"down\", not {d:?}")),
        (None, Some(None)) => layout.move_before(section, None),
        (None, Some(Some(b))) => {
            let Some(before) = crate::sidebar::SidebarSection::from_id(b) else {
                return Err(format!("{id}: `before` must be a section ({}) or null, not {b:?}", crate::sidebar::SidebarSection::ids()));
            };
            layout.move_before(section, Some(before))
        }
        _ => return Err(format!("{id}: give either `direction` (up|down) or `before` (a section, or null for last)")),
    };
    let order: Vec<&str> = layout.sections().iter().map(|s| s.id.id()).collect();
    Ok(json!({"section": section.id(), "moved": moved, "order": order}))
}

pub(crate) fn view_sidebar_reset(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    // every section shown and open, in the usual order
    app.ui.sidebar = Default::default();
    Ok(json!({"sidebar": app.ui.sidebar}))
}

pub(crate) fn view_appearance(app: &mut LightcraftApp, _: &Call, p: Mode) -> Result<Value, String> {
    appearance_command(app, p.mode.as_deref())
}

pub(crate) fn view_theme(app: &mut LightcraftApp, _: &Call, p: Theme) -> Result<Value, String> {
    theme_command(app, p.theme.as_deref())
}

pub(crate) fn view_grid_toggle(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.view = if app.ui.view == ViewMode::PhotoGrid { ViewMode::SquareGrid } else { ViewMode::PhotoGrid };
    Ok(Value::Null)
}

pub(crate) fn view_detail(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.view = ViewMode::Detail;
    Ok(Value::Null)
}

pub(crate) fn view_compare(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    crate::panels::compare::enter_compare(app)
}

pub(crate) fn view_focus_point(app: &mut LightcraftApp, _: &Call, p: On) -> Result<Value, String> {
    // The camera's focus point (Sony, from the file) over the photo.
    app.ui.settings.focus_point = p.on.unwrap_or(!app.ui.settings.focus_point);
    Ok(json!({"on": app.ui.settings.focus_point}))
}

pub(crate) fn view_zoom_to_focus(app: &mut LightcraftApp, _: &Call, p: On) -> Result<Value, String> {
    // Toggle Zoom (Z) centres on the focus point when there is one.
    app.ui.settings.zoom_to_focus = p.on.unwrap_or(!app.ui.settings.zoom_to_focus);
    Ok(json!({"on": app.ui.settings.zoom_to_focus}))
}

pub(crate) fn view_face_boxes(app: &mut LightcraftApp, _: &Call, p: Show) -> Result<Value, String> {
    app.ui.face_boxes = p.show.unwrap_or(!app.ui.face_boxes);
    Ok(json!({"show": app.ui.face_boxes}))
}

pub(crate) fn view_people(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    // Everyone; the page just left stays one click away (the chip next to the title)
    if let Some(name) = app.ui.people.person_page.take() {
        app.ui.people.last_person = Some(name);
    }
    app.ui.people.person_from = None;
    app.ui.view = ViewMode::People;
    Ok(json!({"people": app.session.catalog.people().len()}))
}

pub(crate) fn view_person(app: &mut LightcraftApp, _: &Call, p: Person) -> Result<Value, String> {
    // {name}: one person's page in the People view: their faces, and the faces that look like them
    let Some(name) = p.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) else {
        return Err("view.person: missing `name`".into());
    };
    app.ui.view = ViewMode::People;
    app.ui.people.person_from = None;
    app.ui.people.person_page = Some(name.to_string());
    Ok(json!({"person": name}))
}

pub(crate) fn view_survey(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.view = ViewMode::Survey;
    Ok(json!({"photos": crate::panels::compare::survey_photos(app).len()}))
}

pub(crate) fn view_reference(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    // the reference: the one set before, else the active photo (the next one becomes active)
    let vis = app.session.visible_cloned();
    let reference = app
        .ui
        .reference
        .filter(|r| app.session.catalog.photo(lightcraft_catalog::PhotoId(*r)).is_some())
        .or_else(|| app.session.active().map(|a| a.0));
    let Some(r) = reference else { return Err("select a photo to use as the reference".into()) };
    app.ui.reference = Some(r);
    if app.session.active().is_none_or(|a| a.0 == r)
        && let Some(i) = vis.iter().position(|x| x.0 == r)
        && let Some(next) = vis.get(i + 1).or(i.checked_sub(1).and_then(|j| vis.get(j)))
    {
        let _ = app.session.execute("library.select", &json!({"ids": [next.0]}));
    }
    app.ui.view = ViewMode::Reference;
    Ok(json!({"reference": r, "active": app.session.active().map(|a| a.0)}))
}

pub(crate) fn compare_swap(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    crate::panels::compare::swap(app)
}

pub(crate) fn compare_set(app: &mut LightcraftApp, _: &Call, p: CompareSet) -> Result<Value, String> {
    crate::panels::compare::set_photo(app, p.id, p.side.as_deref())
}

pub(crate) fn compare_make_select(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    crate::panels::compare::make_select(app)
}

pub(crate) fn view_auto_advance(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    app.ui.auto_advance = !app.ui.auto_advance;
    app.toast(&ctx, crate::i18n::tr(if app.ui.auto_advance { "Auto Advance On" } else { "Auto Advance Off" }));
    Ok(json!({"autoAdvance": app.ui.auto_advance}))
}

pub(crate) fn view_back(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    if app.ui.dialog.is_some() {
        app.ui.dialog = None;
    } else if app.ui.lights_out != crate::state::LightsOut::On {
        app.ui.lights_out = crate::state::LightsOut::On;
    } else if app.ui.keyword_painter.is_some() {
        app.ui.keyword_painter = None;
    } else if app.ui.fullscreen {
        app.ui.fullscreen = false;
        app.ui.slideshow = None;
    } else if !app.ui.tool.is_empty() {
        app.ui.tool.clear();
    } else if app.ui.right == RightPanel::Crop && app.ui.view == ViewMode::Detail {
        // Esc leaves the crop tool (the crop stays), as C does; the next Esc goes back
        panel(app, &ctx, RightPanel::Crop, "Crop, Rotate, Geometry");
    } else if app.ui.view == ViewMode::People && app.ui.people.person_page.is_some() {
        app.ui.people.person_page = None;
    } else if matches!(app.ui.view, ViewMode::Compare | ViewMode::Survey) {
        app.ui.view = ViewMode::Detail;
    } else if app.ui.view == ViewMode::Detail {
        // a photo opened from a person's page (or the unnamed faces) goes back there, any other to the grid
        match app.ui.people.person_from.take() {
            Some((name, photo)) if app.session.active().is_some_and(|a| a.0 == photo) => {
                app.ui.view = ViewMode::People;
                app.ui.people.person_page = name;
            }
            _ => app.ui.view = ViewMode::PhotoGrid,
        }
    }
    Ok(Value::Null)
}

pub(crate) fn view_slideshow(app: &mut LightcraftApp, c: &Call, _: Keys<SlideshowKeys>) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    // the photos in view, full screen, with the playback settings (see panels::slideshow)
    let now = ctx.input(|i| i.time);
    let r = crate::panels::slideshow::start(app, c.params, now);
    if r.is_ok() {
        app.toast(&ctx, crate::i18n::tr("Slideshow · Space pauses · Esc ends"));
    }
    r
}

pub(crate) fn slideshow_toggle_pause(app: &mut LightcraftApp, c: &Call, _: NoParams) -> Result<Value, String> {
    let id = c.id;
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    let now = ctx.input(|i| i.time);
    crate::panels::slideshow::command(app, id, now)
}

pub(crate) fn view_slideshow_options(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.dialog = Some(Dialog::Settings { tab: "slideshow".into() });
    Ok(Value::Null)
}

pub(crate) fn view_full_screen_preview(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.fullscreen = !app.ui.fullscreen;
    if !app.ui.fullscreen {
        app.ui.slideshow = None;
    }
    if app.ui.fullscreen {
        app.ui.zoom = Zoom::Fit;
        app.ui.tool.clear();
        let _ = app.session.end_interaction();
    }
    Ok(json!({"fullscreen": app.ui.fullscreen}))
}

pub(crate) fn view_lights_out(app: &mut LightcraftApp, _: &Call, p: Mode) -> Result<Value, String> {
    // {mode?: on | dim | off} (cycles when omitted)
    let mode = match p.mode.as_deref() {
        Some(m) => serde_json::from_value(json!(m)).map_err(|_| format!("unknown lights-out mode: {m} (on|dim|off)"))?,
        None => app.ui.lights_out.next(),
    };
    app.ui.lights_out = mode;
    Ok(json!({"lightsOut": mode}))
}

pub(crate) fn view_enter_full_screen(app: &mut LightcraftApp, _: &Call, p: On) -> Result<Value, String> {
    // applied by the host's frame logic (it knows the window's current state)
    app.ui.window_fullscreen = Some(p.on.unwrap_or(!app.window_is_fullscreen));
    Ok(json!({"windowFullscreen": app.ui.window_fullscreen}))
}

pub(crate) fn view_info_overlay(app: &mut LightcraftApp, _: &Call, p: Mode) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    app.ui.info_overlay = match p.mode.as_deref() {
        Some(m) => serde_json::from_value(json!(m)).map_err(|_| format!("unknown info overlay `{m}` (off|basic|exposure)"))?,
        None => app.ui.info_overlay.next(),
    };
    let label = match app.ui.info_overlay {
        crate::state::InfoOverlay::Off => "Info Overlay Off",
        crate::state::InfoOverlay::Basic => "Info Overlay: File",
        crate::state::InfoOverlay::Exposure => "Info Overlay: Exposure",
    };
    app.toast(&ctx, crate::i18n::tr(label));
    Ok(json!({"infoOverlay": app.ui.info_overlay}))
}

pub(crate) fn view_choose_layout_image(app: &mut LightcraftApp, c: &Call, p: PathParam) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    let path = match p.path {
        Some(path) => path.trim().to_string(),
        None => {
            let req = PickRequest::file(crate::i18n::tr("Choose Layout Image…"), "PNG", &["png"]);
            match crate::pick::ask(app, c.id, c.params, "path", req, |s| s.pick_files.as_mut().map(|f| f())) {
                Picked::Now(paths) => match paths.first() {
                    Some(path) => path.trim().to_string(),
                    None => return Ok(Value::Null),
                },
                Picked::Later => return Ok(Value::Null),
                Picked::Unavailable => return Err("no file dialog on this platform; supply path".into()),
            }
        }
    };
    if path.is_empty() {
        return Err("view.chooseLayoutImage: path is empty".into());
    }
    let li = &mut app.ui.layout_image;
    li.path = path;
    li.show = true;
    li.revision = li.revision.wrapping_add(1);
    app.toast(&ctx, "Layout Image On");
    Ok(json!({"layoutImage": app.ui.layout_image}))
}

pub(crate) fn view_layout_image(app: &mut LightcraftApp, _: &Call, p: Show) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    let show = p.show.unwrap_or(!app.ui.layout_image.show);
    // nothing chosen yet: showing it chooses one first
    if show && app.ui.layout_image.path.trim().is_empty() {
        return view_choose_layout_image(app, &Call { id: "view.chooseLayoutImage", params: &json!({}) }, PathParam { path: None });
    }
    app.ui.layout_image.show = show;
    app.toast(&ctx, if show { "Layout Image On" } else { "Layout Image Off" });
    Ok(json!({"layoutImage": app.ui.layout_image}))
}

pub(crate) fn view_layout_image_opacity(app: &mut LightcraftApp, c: &Call, p: LayoutImageLevel) -> Result<Value, String> {
    let id = c.id;
    let (key, given, other) = if id == "view.layoutImageOpacity" { ("opacity", p.opacity, p.matte) } else { ("matte", p.matte, p.opacity) };
    if other.is_some() {
        return Err(format!("{id}: takes `{key}` only"));
    }
    let Some(v) = given.filter(|v| v.is_finite()) else {
        return Err(format!("{id}: {key} (0..1) is required"));
    };
    let v = v.clamp(0.0, 1.0) as f32;
    if key == "opacity" {
        app.ui.layout_image.opacity = v;
    } else {
        app.ui.layout_image.matte = v;
    }
    Ok(json!({"layoutImage": app.ui.layout_image}))
}

pub(crate) fn view_navigator(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.navigator = !app.ui.navigator;
    Ok(json!({"navigator": app.ui.navigator}))
}

pub(crate) fn view_filmstrip(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.filmstrip = !app.ui.filmstrip;
    Ok(Value::Null)
}

lightcraft_engine::params! {
    /// `{show?}`: show or hide; toggles when omitted.
    pub(crate) struct Show {
        show: Option<bool>,
    }
}

pub(crate) fn view_left_panel(app: &mut LightcraftApp, _: &Call, p: Show) -> Result<Value, String> {
    app.ui.left_panel = p.show.unwrap_or(!app.ui.left_panel);
    Ok(json!({"show": app.ui.left_panel}))
}

pub(crate) fn view_before_after(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.before_after = if app.ui.before_after == BeforeAfter::SideBySide { BeforeAfter::Off } else { BeforeAfter::SideBySide };
    Ok(Value::Null)
}

pub(crate) fn view_before_after_split(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.before_after = if app.ui.before_after == BeforeAfter::Split { BeforeAfter::Off } else { BeforeAfter::Split };
    Ok(Value::Null)
}

pub(crate) fn view_before_after_top_bottom(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.before_after = if app.ui.before_after == BeforeAfter::TopBottom { BeforeAfter::Off } else { BeforeAfter::TopBottom };
    Ok(Value::Null)
}

pub(crate) fn view_before_after_split_top_bottom(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.before_after = if app.ui.before_after == BeforeAfter::SplitTopBottom { BeforeAfter::Off } else { BeforeAfter::SplitTopBottom };
    Ok(Value::Null)
}

pub(crate) fn view_show_original(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.before_after = if app.ui.before_after == BeforeAfter::Original { BeforeAfter::Off } else { BeforeAfter::Original };
    Ok(Value::Null)
}

pub(crate) fn view_zoom_fit(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.zoom = Zoom::Fit;
    Ok(Value::Null)
}

pub(crate) fn view_zoom100(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.zoom = Zoom::Percent(100.0);
    Ok(Value::Null)
}

pub(crate) fn view_zoom_toggle(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    // the same ratio a click on the photo zooms to; zooming in centres on the camera's focus
    // point when the photo has one (View ▸ Zoom to Focus Point)
    let zoom_in = app.ui.zoom == Zoom::Fit;
    app.ui.zoom = if zoom_in { Zoom::Percent(app.ui.click_zoom as f32) } else { Zoom::Fit };
    app.ui.zoom_anim = true;
    if zoom_in
        && app.ui.settings.zoom_to_focus
        && let Some(pan) = app.session.active().and_then(|id| crate::panels::detail::focus_pan(app, id))
    {
        app.ui.pan = pan;
        return Ok(json!({"focus": [pan.0, pan.1]}));
    }
    Ok(Value::Null)
}

pub(crate) fn view_click_zoom(app: &mut LightcraftApp, _: &Call, p: ClickZoom) -> Result<Value, String> {
    // {ratio?: 1|2|3|4|8} → {ratio}
    if let Some(r) = p.ratio {
        let pct = (r * 100.0).round() as u32;
        if !crate::state::CLICK_ZOOMS.contains(&pct) {
            return Err(format!("view.clickZoom: ratio {r} (1, 2, 3, 4 or 8)"));
        }
        app.ui.click_zoom = pct;
    }
    Ok(json!({"ratio": app.ui.click_zoom / 100}))
}

pub(crate) fn view_zoom_in(app: &mut LightcraftApp, c: &Call, _: NoParams) -> Result<Value, String> {
    let steps = [25.0, 50.0, 100.0, 200.0, 400.0, 800.0];
    let cur = match app.ui.zoom {
        Zoom::Percent(p) => p,
        _ => 25.0,
    };
    let next = if c.id == "view.zoomIn" {
        steps.iter().find(|s| **s > cur).copied().unwrap_or(800.0)
    } else {
        steps.iter().rev().find(|s| **s < cur).copied().unwrap_or(0.0)
    };
    app.ui.zoom = if next == 0.0 { Zoom::Fit } else { Zoom::Percent(next) };
    Ok(Value::Null)
}

pub(crate) fn view_thumb_size_up(app: &mut LightcraftApp, c: &Call, _: NoParams) -> Result<Value, String> {
    let id = c.id;
    // the bottom-bar slider's range, one `THUMB_SIZE_STEP` per call; {} → {size}
    if !thumb_size_view(app) {
        return Err(format!("{id}: only in Photo Grid, Square Grid or People"));
    }
    let step = if id == "view.thumbSizeUp" { crate::state::THUMB_SIZE_STEP } else { -crate::state::THUMB_SIZE_STEP };
    app.ui.thumb_size = (app.ui.thumb_size + step).clamp(crate::state::THUMB_SIZE_MIN, crate::state::THUMB_SIZE_MAX);
    Ok(json!({"size": app.ui.thumb_size}))
}

pub(crate) fn view_navigate(app: &mut LightcraftApp, _: &Call, p: Navigate) -> Result<Value, String> {
    // Validate the complete request before changing either part of the viewport.
    let zoom = match p.zoom {
        Some(Zoom::Percent(z)) if !z.is_finite() || z <= 0.0 || z > 800.0 => {
            return Err("view.navigate: zoom percent must be greater than 0 and at most 800".into());
        }
        Some(z) => z,
        None => app.ui.zoom,
    };
    let pan = match p.pan {
        Some([x, y]) if x.is_finite() && y.is_finite() && (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y) => (x, y),
        Some(_) => return Err("view.navigate: pan must be [x, y] with finite coordinates from 0 to 1".into()),
        None => app.ui.pan,
    };
    app.ui.zoom = zoom;
    app.ui.pan = pan;
    app.ui.zoom_anim = false;
    Ok(json!({"zoom": zoom, "pan": pan}))
}

pub(crate) fn view_clipping(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.show_clipping = !app.ui.show_clipping;
    Ok(Value::Null)
}

pub(crate) fn view_visualize_hdr(app: &mut LightcraftApp, _: &Call, p: On) -> Result<Value, String> {
    // Shown on photos edited in HDR (see `develop.hdr`).
    app.ui.hdr_visualize = p.on.unwrap_or(!app.ui.hdr_visualize);
    Ok(json!({"on": app.ui.hdr_visualize}))
}

pub(crate) fn view_soft_proof(app: &mut LightcraftApp, _: &Call, p: SoftProof) -> Result<Value, String> {
    if let Some(s) = p.space.as_deref() {
        match lightcraft_engine::pipeline::OutputSpace::parse(s) {
            Some(sp) => {
                app.ui.proof.space = sp;
                // a standard space replaces a device profile (unless one is given too)
                app.ui.proof_profile = None;
            }
            None => return Err(format!("view.softProof: unknown space {s:?} (srgb|displayP3|adobeRgb|proPhoto|rec2020)")),
        }
    }
    if let Some(i) = p.intent.as_deref() {
        app.ui.proof_intent = lightcraft_engine::proof::ProofIntent::parse(i).ok_or("view.softProof: intent must be perceptual or relative")?;
    }
    if let Some(b) = p.paper_ink {
        app.ui.proof_paper_ink = b;
    }
    let path = match &p.profile {
        Some(Some(s)) => Some(s.clone()),
        Some(None) => {
            app.ui.proof_profile = None;
            None
        }
        None => app.ui.proof_profile.as_ref().map(|(f, _)| f.clone()),
    };
    match path {
        Some(path) => match lightcraft_engine::proof::load(&path, app.ui.proof_intent, app.ui.proof_paper_ink) {
            Ok(pp) => {
                app.ui.proof.profile = pp.id;
                app.ui.proof_profile = Some((path, pp.name));
            }
            Err(e) => return Err(format!("view.softProof: {e}")),
        },
        None => app.ui.proof.profile = 0,
    }
    if let Some(b) = p.dest_warning {
        app.ui.proof.dest_warning = b;
    }
    if let Some(b) = p.display_warning {
        app.ui.proof.display_warning = b;
    }
    let settings = p.space.is_some()
        || p.dest_warning.is_some()
        || p.display_warning.is_some()
        || p.profile.is_some()
        || p.intent.is_some()
        || p.paper_ink.is_some();
    app.ui.soft_proof = match p.on {
        Some(b) => b,
        None if settings => app.ui.soft_proof,
        None => !app.ui.soft_proof,
    };
    if app.ui.soft_proof && !matches!(app.ui.view, ViewMode::Detail | ViewMode::Reference) {
        app.ui.view = ViewMode::Detail;
    }
    let pr = app.ui.proof;
    let profile = app.ui.proof_profile.as_ref().map(|(f, n)| json!({"path": f, "name": n}));
    Ok(json!({"on": app.ui.soft_proof, "space": pr.space, "profile": profile, "intent": app.ui.proof_intent.name(),
        "paperInk": app.ui.proof_paper_ink, "destWarning": pr.dest_warning, "displayWarning": pr.display_warning}))
}

pub(crate) fn view_histogram(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.histogram = !app.ui.histogram;
    Ok(Value::Null)
}

pub(crate) fn view_mask_overlay(app: &mut LightcraftApp, _: &Call, p: Show) -> Result<Value, String> {
    app.ui.overlay.on = p.show.unwrap_or(!app.ui.overlay.on);
    Ok(json!({"maskOverlay": app.ui.overlay.on}))
}

pub(crate) fn view_mask_overlay_all(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.overlay.all = !app.ui.overlay.all;
    Ok(json!({"maskOverlayAll": app.ui.overlay.all}))
}

pub(crate) fn view_mask_overlay_mode(app: &mut LightcraftApp, _: &Call, p: Mode) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    use lightcraft_engine::pipeline::MaskView;
    let cur = MaskView::parse(&app.ui.overlay.mode).unwrap_or_default();
    let next = match p.mode.as_deref() {
        Some(m) => MaskView::parse(m).ok_or_else(|| {
            let names: Vec<&str> = MaskView::ALL.iter().map(|v| v.name()).collect();
            format!("view.maskOverlayMode: unknown mode `{m}` ({})", names.join("|"))
        })?,
        None => cur.next(),
    };
    app.ui.overlay.mode = next.name().into();
    app.ui.overlay.on = true;
    app.toast(&ctx, next.label());
    Ok(json!({"mode": next.name()}))
}

pub(crate) fn view_mask_overlay_color(app: &mut LightcraftApp, _: &Call, p: MaskOverlayColor) -> Result<Value, String> {
    // no params: the next of the panel's swatch colours
    if p.color.is_none() && p.opacity.is_none() {
        let all = crate::panels::masking::OVERLAY_COLORS;
        let i = all.iter().position(|c| *c == app.ui.overlay.color).map_or(0, |i| (i + 1) % all.len());
        app.ui.overlay.color = all[i];
    }
    if let Some(c) = &p.color {
        app.ui.overlay.color = parse_rgb(c).ok_or("view.maskOverlayColor: `color` is \"#rrggbb\" or [r, g, b]")?;
    }
    if let Some(o) = p.opacity {
        app.ui.overlay.opacity = o.clamp(0.0, 100.0) as f32;
    }
    let [r, g, b] = app.ui.overlay.color;
    Ok(json!({"color": format!("#{r:02x}{g:02x}{b:02x}"), "opacity": app.ui.overlay.opacity}))
}

pub(crate) fn brush_smaller(app: &mut LightcraftApp, c: &Call, _: NoParams) -> Result<Value, String> {
    let id = c.id;
    let (k, df) = match id {
        "brush.smaller" => (1.0 / 1.2, 0.0),
        "brush.larger" => (1.2, 0.0),
        "brush.featherLess" => (1.0, -10.0),
        _ => (1.0, 10.0),
    };
    Ok(adjust_brush(app, k, df))
}

pub(crate) fn view_mask_pins(app: &mut LightcraftApp, _: &Call, p: Show) -> Result<Value, String> {
    app.ui.overlay.pins = p.show.unwrap_or(!app.ui.overlay.pins);
    Ok(json!({"maskPins": app.ui.overlay.pins}))
}

pub(crate) fn view_tool_overlay(app: &mut LightcraftApp, _: &Call, p: Mode) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    use crate::state::ToolOverlay;
    let mode = match p.mode.as_deref() {
        None => app.ui.tool_overlay.next(),
        Some(m) => ToolOverlay::parse(m).ok_or_else(|| format!("view.toolOverlay: unknown mode {m:?} (auto|always|selected|never)"))?,
    };
    app.ui.tool_overlay = mode;
    app.toast(&ctx, tool_overlay_toast(mode));
    Ok(json!({"mode": mode.name()}))
}

pub(crate) fn view_visualize_spots(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    // like Lightroom's A: opens the Remove tool with the view on, or toggles it there
    if app.ui.right == RightPanel::Remove {
        app.ui.visualize_spots = !app.ui.visualize_spots;
    } else {
        app.ui.right = RightPanel::Remove;
        app.ui.visualize_spots = true;
    }
    Ok(Value::Null)
}

pub(crate) fn view_crop_overlay(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    use crate::state::CropOverlay::*;
    app.ui.crop_overlay = match app.ui.crop_overlay {
        Thirds => Grid,
        Grid => Golden,
        Golden => Diagonal,
        Diagonal => Triangle,
        Triangle => Spiral,
        Spiral => None,
        None => Thirds,
    };
    Ok(json!({"overlay": app.ui.crop_overlay}))
}

pub(crate) fn view_crop_overlay_orientation(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.crop_overlay_orient = (app.ui.crop_overlay_orient + 1) % 4;
    Ok(json!({"orientation": app.ui.crop_overlay_orient}))
}

pub(crate) fn local_add_root(app: &mut LightcraftApp, _: &Call, p: PathParam) -> Result<Value, String> {
    // a folder kept in Local's sidebar (saved with the UI state); nothing on disk changes
    let Some(path) = p.path.as_deref() else { return Err("local.addRoot needs a path".into()) };
    let abs = std::path::absolute(path).map(|a| a.to_string_lossy().trim_end_matches(['/', '\\']).to_string()).unwrap_or(path.into());
    let abs = if abs.is_empty() { path.to_string() } else { abs };
    if !std::path::Path::new(&abs).is_dir() {
        return Err(format!("{path}: not a folder"));
    }
    use crate::panels::left::same_folder;
    if !app.ui.local_roots.iter().any(|r| same_folder(r, &abs)) {
        app.ui.local_roots.push(abs.clone());
    }
    app.ui.hidden_locations.retain(|h| !same_folder(h, &abs));
    Ok(json!({"roots": app.ui.local_roots}))
}

pub(crate) fn local_hide(app: &mut LightcraftApp, _: &Call, p: PathParam) -> Result<Value, String> {
    let path = p.path.ok_or("local.hide needs a path")?;
    if !app.ui.hidden_locations.iter().any(|h| crate::panels::left::same_folder(h, &path)) {
        app.ui.hidden_locations.push(path);
    }
    Ok(json!({"hidden": app.ui.hidden_locations}))
}

pub(crate) fn local_restore_hidden(app: &mut LightcraftApp, _: &Call, p: PathParam) -> Result<Value, String> {
    // one path, or (no path) every hidden location
    match p.path.as_deref() {
        Some(path) => app.ui.hidden_locations.retain(|h| !crate::panels::left::same_folder(h, path)),
        None => app.ui.hidden_locations.clear(),
    }
    Ok(json!({"hidden": app.ui.hidden_locations}))
}

pub(crate) fn view_solo_sections(app: &mut LightcraftApp, _: &Call, p: On) -> Result<Value, String> {
    app.ui.set_single_panel(p.on.unwrap_or(!app.ui.single_panel));
    Ok(json!({"soloSections": app.ui.single_panel}))
}

pub(crate) fn view_filter_bar(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.filter_bar = !app.ui.filter_bar;
    if app.ui.filter_bar && !matches!(app.ui.view, ViewMode::PhotoGrid | ViewMode::SquareGrid) {
        app.ui.view = ViewMode::PhotoGrid;
    }
    Ok(json!({"filterBar": app.ui.filter_bar}))
}

pub(crate) fn panel_edit(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    panel(app, &ctx, RightPanel::Edit, "Edit");
    Ok(Value::Null)
}

pub(crate) fn panel_profiles(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    // toggles between the profile browser and the Edit panel it belongs to
    app.ui.right = if app.ui.right == RightPanel::Profiles && crate::docking::exposed(app, crate::docking::Panel::Tools) {
        RightPanel::Edit
    } else {
        RightPanel::Profiles
    };
    if !matches!(app.ui.view, ViewMode::Detail) {
        app.ui.view = ViewMode::Detail;
    }
    Ok(json!({"open": app.ui.right == RightPanel::Profiles}))
}

pub(crate) fn panel_crop(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    panel(app, &ctx, RightPanel::Crop, "Crop, Rotate, Geometry");
    Ok(Value::Null)
}

pub(crate) fn panel_remove(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    panel(app, &ctx, RightPanel::Remove, "Remove");
    if app.ui.right == RightPanel::Remove && app.ui.tool.is_empty() {
        app.ui.tool = "remove".into();
    }
    Ok(Value::Null)
}

pub(crate) fn panel_masking(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    panel(app, &ctx, RightPanel::Masking, "Masking");
    Ok(Value::Null)
}

pub(crate) fn panel_redeye(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    panel(app, &ctx, RightPanel::RedEye, "Red Eye");
    Ok(Value::Null)
}

pub(crate) fn panel_info(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    panel(app, &ctx, RightPanel::Info, "Info");
    Ok(Value::Null)
}

pub(crate) fn panel_keywords(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    panel(app, &ctx, RightPanel::Keywords, "Keywords");
    Ok(Value::Null)
}

pub(crate) fn panel_versions(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    panel(app, &ctx, RightPanel::Versions, "Versions");
    Ok(Value::Null)
}

pub(crate) fn panel_activity(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    panel(app, &ctx, RightPanel::Activity, "History");
    Ok(Value::Null)
}

pub(crate) fn panel_presets(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.presets = !app.ui.presets || !crate::docking::exposed(app, crate::docking::Panel::Presets);
    if app.ui.presets && app.ui.view != ViewMode::Detail {
        app.ui.view = ViewMode::Detail;
    }
    Ok(Value::Null)
}

pub(crate) fn tool_done(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    // Return commits a tool panel (crop, remove, red eye, masking): back to Edit
    use RightPanel::*;
    if app.ui.dialog.is_none() && matches!(app.ui.right, Crop | Remove | RedEye | Masking) {
        let _ = app.session.end_interaction();
        app.ui.tool.clear();
        app.ui.right = Edit;
    }
    Ok(Value::Null)
}

pub(crate) fn panel_close(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.right = RightPanel::None;
    app.ui.presets = false;
    Ok(Value::Null)
}

pub(crate) fn section(app: &mut LightcraftApp, c: &Call, p: Section) -> Result<Value, String> {
    let s = c.id;
    // only the Edit sections listed above: an unknown one would close the real ones
    if !UI_COMMANDS.iter().any(|c| c.id == s) {
        return Err(format!("{s}: no such Edit section"));
    }
    let sec = &s["section.".len()..];
    if app.ui.right != RightPanel::Edit {
        app.ui.right = RightPanel::Edit;
    }
    if p.solo == Some(true) {
        app.ui.solo_section(sec);
    } else {
        app.ui.toggle_section(sec);
    }
    Ok(Value::Null)
}

pub(crate) fn tool(app: &mut LightcraftApp, c: &Call, p: Tool) -> Result<Value, String> {
    let s = c.id;
    let tool = &s["tool.".len()..];
    if tool != "brush" && (p.new || p.op.is_some()) {
        return Err(format!("{s}: `new` and `op` are for tool.brush"));
    }
    match tool {
        "none" => app.ui.tool.clear(),
        "guidedUpright" => {
            // Crop & Geometry with Guided Upright on, ready to draw guides
            app.ui.right = RightPanel::Crop;
            app.ui.view = ViewMode::Detail;
            let guided = app
                .session
                .active()
                .and_then(|id| app.session.develop_of(id))
                .is_some_and(|d| d.geometry.upright == lightcraft_develop::Upright::Guided);
            if !guided {
                app.run("geometry.upright", json!({"mode": "guided"}))?;
            }
            app.ui.tool = "guidedUpright".into();
        }
        "brush" => {
            // A new mask and Add/Subtract are selection operations, not the brush's
            // Erase mode. Paint positively into a component with the requested op.
            let create = if p.new {
                Some(app.run("mask.add", json!({"kind": "brush"})))
            } else {
                p.op.map(|op| app.run("mask.addComponent", json!({"kind": "brush", "op": op})))
            };
            if let Some(result) = create {
                result?;
                app.ui.brush.erase = false;
                app.ui.overlay.on = true;
            }
            app.ui.right = RightPanel::Masking;
            app.ui.view = ViewMode::Detail;
            app.ui.tool = "brush".into();
        }
        "linear" | "radial" => {
            app.ui.right = RightPanel::Masking;
            app.ui.view = ViewMode::Detail;
            app.ui.tool = tool.into();
            return app.session.execute("mask.add", &json!({"kind": tool})).map_err(|e| e.to_string());
        }
        "wbPicker" => {
            app.ui.right = RightPanel::Edit;
            app.ui.view = ViewMode::Detail;
            app.ui.tool = "wbPicker".into();
        }
        other => return Err(format!("unknown tool `{other}`")),
    }
    Ok(Value::Null)
}

pub(crate) fn view_focus_search(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    // the search field lives in the top bar of the library and detail views alike
    app.ui.focus_search = true;
    Ok(Value::Null)
}

/// `ui.dock` / `ui.dock.reset`: arrange the docked panels.
pub(crate) fn dock(app: &mut LightcraftApp, c: &Call, _: Keys<DockKeys>) -> Result<Value, String> {
    crate::docking::command(app, c.id == "ui.dock.reset", c.params)
}

lightcraft_engine::params! {
    /// `{on?}`: on or off; toggles when omitted.
    pub(crate) struct On {
        on: Option<bool>,
    }
}

lightcraft_engine::params! {
    /// `{mode?}`: a mode by name; the next one when omitted.
    pub(crate) struct Mode {
        mode: Option<String>,
    }
}

lightcraft_engine::params! {
    /// `{path}`: a file or folder.
    pub(crate) struct PathParam {
        path: Option<String>,
    }
}

lightcraft_engine::params! {
    /// `view.gridInfo`
    pub(crate) struct GridInfo {
        info: Option<String>,
    }
}

lightcraft_engine::params! {
    /// `tool.keywordPainter`
    pub(crate) struct KeywordPainter {
        keyword: Option<String>,
    }
}

lightcraft_engine::params! {
    /// `view.sidebarSection`: with neither `show` nor `collapsed`, a shown section is hidden and a hidden one shown.
    pub(crate) struct SidebarSection {
        section: Option<String>,
        show: Option<bool>,
        collapsed: Option<bool>,
    }
}

lightcraft_engine::params! {
    /// `view.sidebarMoveSection`: `direction` (up | down), or `before` a section (`null`: last).
    pub(crate) struct SidebarMove {
        section: Option<String>,
        direction: Option<String>,
        #[serde(default, deserialize_with = "lightcraft_engine::cmd::params::present")]
        before: Option<Option<String>>,
    }
}

lightcraft_engine::params! {
    /// `view.theme`
    pub(crate) struct Theme {
        theme: Option<String>,
    }
}

lightcraft_engine::params! {
    /// `view.person`
    pub(crate) struct Person {
        name: Option<String>,
    }
}

lightcraft_engine::params! {
    /// `compare.set`
    pub(crate) struct CompareSet {
        id: lightcraft_catalog::PhotoId,
        side: Option<String>,
    }
}

lightcraft_engine::params! {
    /// `view.layoutImageOpacity` (`opacity`) / `view.layoutImageMatte` (`matte`), 0..1.
    pub(crate) struct LayoutImageLevel {
        opacity: Option<f64>,
        matte: Option<f64>,
    }
}

lightcraft_engine::params! {
    /// `view.clickZoom`
    pub(crate) struct ClickZoom {
        ratio: Option<f64>,
    }
}

lightcraft_engine::params! {
    /// `view.navigate`: `zoom` is `"fit"`, `"fill"` or `{percent}`; `pan` the normalized image centre.
    pub(crate) struct Navigate {
        zoom: Option<Zoom>,
        pan: Option<[f32; 2]>,
    }
}

lightcraft_engine::params! {
    /// `view.softProof`: `profile` is an ICC file, `null` for none; the settings given keep the
    /// proof on or off, and no params toggle it.
    pub(crate) struct SoftProof {
        on: Option<bool>,
        space: Option<String>,
        #[serde(default, deserialize_with = "lightcraft_engine::cmd::params::present")]
        profile: Option<Option<String>>,
        intent: Option<String>,
        paper_ink: Option<bool>,
        dest_warning: Option<bool>,
        display_warning: Option<bool>,
    }
}

lightcraft_engine::params! {
    /// `view.maskOverlayColor`: no params steps to the next swatch.
    pub(crate) struct MaskOverlayColor {
        color: Option<Value>,
        opacity: Option<f64>,
    }
}

lightcraft_engine::params! {
    /// `section.*`: `solo` opens just that section; otherwise it opens or closes.
    pub(crate) struct Section {
        solo: Option<bool>,
    }
}

lightcraft_engine::params! {
    /// `tool.*`; the brush also takes `new` (a new mask) or `op` (a component added with that op).
    pub(crate) struct Tool {
        #[serde(default)]
        new: bool,
        op: Option<String>,
    }
}

lightcraft_engine::key_list! {
    /// `ui.dock`'s keys (`docking::command` reads them).
    pub(crate) struct DockKeys = ["operation", "panel", "action", "path", "size", "open", "height", "rect", "target", "zone", "before"]
}

/// `view.slideshow`'s keys: the playback settings (`slideshow.playback`) for this show, `interval`
/// for `slideDuration`, and `seed`.
pub(crate) struct SlideshowKeys;
impl lightcraft_engine::cmd::params::KeyList for SlideshowKeys {
    const KEYS: &'static [&'static str] =
        &["slideDuration", "fadeDuration", "fadeColor", "randomOrder", "repeat", "preparePreviews", "interval", "seed"];
}
