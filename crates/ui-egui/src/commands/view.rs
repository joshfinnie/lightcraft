//! UI commands for the views, zoom, overlays, panels, sections and tools.

use serde_json::{Value, json};

use crate::LightcraftApp;
use crate::menus::*;
use crate::pick::{PickRequest, Picked};
use crate::state::{BeforeAfter, Dialog, RightPanel, ViewMode, Zoom};

pub(crate) fn view_photo_grid(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.view = ViewMode::PhotoGrid;
    Ok(Value::Null)
}

pub(crate) fn view_square_grid(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.view = ViewMode::SquareGrid;
    Ok(Value::Null)
}

pub(crate) fn view_grid_info(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    // {info?: filename | exposure | date} (cycles when omitted)
    let next = match p.get("info").and_then(Value::as_str) {
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

pub(crate) fn tool_keyword_painter(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    // {keyword?}: paint that keyword onto photos in the grid by clicking them; no keyword stops
    app.ui.keyword_painter = p.get("keyword").and_then(Value::as_str).map(str::trim).filter(|k| !k.is_empty()).map(str::to_string);
    if app.ui.keyword_painter.is_some() && !matches!(app.ui.view, ViewMode::PhotoGrid | ViewMode::SquareGrid) {
        app.ui.view = ViewMode::PhotoGrid;
    }
    if let Some(k) = app.ui.keyword_painter.clone() {
        app.toast(&ctx, crate::i18n::tr_format!("Painting “{k}”: click photos to add or remove it · Esc stops", k = k));
    }
    Ok(json!({"keyword": app.ui.keyword_painter}))
}

pub(crate) fn view_second_window(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    app.ui.second_window = p.get("show").and_then(Value::as_bool).unwrap_or(!app.ui.second_window);
    Ok(json!({"show": app.ui.second_window}))
}

pub(crate) fn view_photo_counts(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    app.ui.show_counts = p.get("show").and_then(Value::as_bool).unwrap_or(!app.ui.show_counts);
    Ok(json!({"show": app.ui.show_counts}))
}

pub(crate) fn view_sidebar_section(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let p = c.params;
    // {section, show?, collapsed?}: show or hide a section of the left sidebar, fold or
    // unfold it; with neither, a shown section is hidden and a hidden one shown
    let args = sidebar_section_param(id, p).and_then(|s| Ok((s, bool_param(id, p, "show")?, bool_param(id, p, "collapsed")?)));
    let (section, show, collapsed) = args?;
    let layout = &mut app.ui.sidebar;
    match (show, collapsed) {
        (None, None) => layout.set_hidden(section, !layout.is_hidden(section)),
        _ => {
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

pub(crate) fn view_sidebar_move_section(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let p = c.params;
    // {section, direction: up | down} one place among the shown sections, or
    // {section, before: <section> | null} just above another section (null: last)
    let section = sidebar_section_param(id, p)?;
    let layout = &mut app.ui.sidebar;
    let moved = match (p.get("direction"), p.get("before")) {
        (Some(d), None) => match d.as_str() {
            Some("up") => layout.move_by(section, true),
            Some("down") => layout.move_by(section, false),
            _ => return Err(format!("{id}: `direction` must be \"up\" or \"down\", not {d}")),
        },
        (None, Some(Value::Null)) => layout.move_before(section, None),
        (None, Some(b)) => {
            let before = b.as_str().and_then(crate::sidebar::SidebarSection::from_id);
            let Some(before) = before else {
                return Err(format!("{id}: `before` must be a section ({}) or null, not {b}", crate::sidebar::SidebarSection::ids()));
            };
            layout.move_before(section, Some(before))
        }
        _ => return Err(format!("{id}: give either `direction` (up|down) or `before` (a section, or null for last)")),
    };
    let order: Vec<&str> = layout.sections().iter().map(|s| s.id.id()).collect();
    Ok(json!({"section": section.id(), "moved": moved, "order": order}))
}

pub(crate) fn view_sidebar_reset(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    // every section shown and open, in the usual order
    app.ui.sidebar = Default::default();
    Ok(json!({"sidebar": app.ui.sidebar}))
}

pub(crate) fn view_appearance(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    appearance_command(app, p)
}

pub(crate) fn view_theme(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    theme_command(app, p)
}

pub(crate) fn view_grid_toggle(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.view = if app.ui.view == ViewMode::PhotoGrid { ViewMode::SquareGrid } else { ViewMode::PhotoGrid };
    Ok(Value::Null)
}

pub(crate) fn view_detail(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.view = ViewMode::Detail;
    Ok(Value::Null)
}

pub(crate) fn view_compare(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    crate::panels::compare::enter_compare(app)
}

pub(crate) fn view_focus_point(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    // {on?}; no params toggles. The camera's focus point (Sony, from the file) over the photo.
    app.ui.settings.focus_point = p.get("on").and_then(Value::as_bool).unwrap_or(!app.ui.settings.focus_point);
    Ok(json!({"on": app.ui.settings.focus_point}))
}

pub(crate) fn view_zoom_to_focus(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    // {on?}; no params toggles. Toggle Zoom (Z) centres on the focus point when there is one.
    app.ui.settings.zoom_to_focus = p.get("on").and_then(Value::as_bool).unwrap_or(!app.ui.settings.zoom_to_focus);
    Ok(json!({"on": app.ui.settings.zoom_to_focus}))
}

pub(crate) fn view_face_boxes(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    app.ui.face_boxes = p.get("show").and_then(Value::as_bool).unwrap_or(!app.ui.face_boxes);
    Ok(json!({"show": app.ui.face_boxes}))
}

pub(crate) fn view_people(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    // Everyone; the page just left stays one click away (the chip next to the title)
    if let Some(name) = app.ui.person_page.take() {
        app.ui.last_person = Some(name);
    }
    app.ui.person_from = None;
    app.ui.view = ViewMode::People;
    Ok(json!({"people": app.session.catalog.people().len()}))
}

pub(crate) fn view_person(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    // {name}: one person's page in the People view: their faces, and the faces that look like them
    let Some(name) = p.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()) else {
        return Err("view.person: missing `name`".into());
    };
    app.ui.view = ViewMode::People;
    app.ui.person_from = None;
    app.ui.person_page = Some(name.to_string());
    Ok(json!({"person": name}))
}

pub(crate) fn view_survey(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.view = ViewMode::Survey;
    Ok(json!({"photos": crate::panels::compare::survey_photos(app).len()}))
}

pub(crate) fn view_reference(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
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

pub(crate) fn compare_swap(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    crate::panels::compare::swap(app)
}

pub(crate) fn compare_set(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    let Some(id) = p.get("id").and_then(Value::as_u64) else { return Err("compare.set needs `id`".into()) };
    crate::panels::compare::set_photo(app, lightcraft_catalog::PhotoId(id), p.get("side").and_then(Value::as_str))
}

pub(crate) fn compare_make_select(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    crate::panels::compare::make_select(app)
}

pub(crate) fn view_auto_advance(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    app.ui.auto_advance = !app.ui.auto_advance;
    app.toast(&ctx, crate::i18n::tr(if app.ui.auto_advance { "Auto Advance On" } else { "Auto Advance Off" }));
    Ok(json!({"autoAdvance": app.ui.auto_advance}))
}

pub(crate) fn view_back(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
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
    } else if app.ui.view == ViewMode::People && app.ui.person_page.is_some() {
        app.ui.person_page = None;
    } else if matches!(app.ui.view, ViewMode::Compare | ViewMode::Survey) {
        app.ui.view = ViewMode::Detail;
    } else if app.ui.view == ViewMode::Detail {
        // a photo opened from a person's page (or the unnamed faces) goes back there, any other to the grid
        match app.ui.person_from.take() {
            Some((name, photo)) if app.session.active().is_some_and(|a| a.0 == photo) => {
                app.ui.view = ViewMode::People;
                app.ui.person_page = name;
            }
            _ => app.ui.view = ViewMode::PhotoGrid,
        }
    }
    Ok(Value::Null)
}

pub(crate) fn view_slideshow(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    // the photos in view, full screen, with the playback settings (see panels::slideshow)
    let now = ctx.input(|i| i.time);
    let r = crate::panels::slideshow::start(app, p, now);
    if r.is_ok() {
        app.toast(&ctx, crate::i18n::tr("Slideshow · Space pauses · Esc ends"));
    }
    r
}

pub(crate) fn slideshow_toggle_pause(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    let now = ctx.input(|i| i.time);
    crate::panels::slideshow::command(app, id, now)
}

pub(crate) fn view_slideshow_options(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.dialog = Some(Dialog::Settings { tab: "slideshow".into() });
    Ok(Value::Null)
}

pub(crate) fn view_full_screen_preview(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
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

pub(crate) fn view_lights_out(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    // {mode?: on | dim | off} (cycles when omitted)
    match p.get("mode").and_then(Value::as_str) {
        Some(m) => serde_json::from_value(json!(m)).map_err(|_| format!("unknown lights-out mode: {m} (on|dim|off)")),
        None => Ok(app.ui.lights_out.next()),
    }
    .map(|mode| {
        app.ui.lights_out = mode;
        json!({"lightsOut": mode})
    })
}

pub(crate) fn view_enter_full_screen(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    // applied by the host's frame logic (it knows the window's current state)
    let on = p.get("on").and_then(Value::as_bool);
    app.ui.window_fullscreen = Some(on.unwrap_or(!app.window_is_fullscreen));
    Ok(json!({"windowFullscreen": app.ui.window_fullscreen}))
}

pub(crate) fn view_info_overlay(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    app.ui.info_overlay = match p.get("mode").and_then(Value::as_str) {
        Some(m) => match serde_json::from_value(json!(m)) {
            Ok(v) => v,
            Err(_) => return Err(format!("unknown info overlay `{m}` (off|basic|exposure)")),
        },
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

pub(crate) fn view_choose_layout_image(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let p = c.params;
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    let path = match p.get("path").and_then(Value::as_str) {
        Some(path) => path.trim().to_string(),
        None => {
            let req = PickRequest::file(crate::i18n::tr("Choose Layout Image…"), "PNG", &["png"]);
            match crate::pick::ask(app, id, p, "path", req, |s| s.pick_files.as_mut().map(|f| f())) {
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

pub(crate) fn view_layout_image(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    let show = p.get("show").and_then(Value::as_bool).unwrap_or(!app.ui.layout_image.show);
    // nothing chosen yet: showing it chooses one first
    if show && app.ui.layout_image.path.trim().is_empty() {
        return view_choose_layout_image(app, &Call { id: "view.chooseLayoutImage", params: &json!({}) });
    }
    app.ui.layout_image.show = show;
    app.toast(&ctx, if show { "Layout Image On" } else { "Layout Image Off" });
    Ok(json!({"layoutImage": app.ui.layout_image}))
}

pub(crate) fn view_layout_image_opacity(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let p = c.params;
    let key = if id == "view.layoutImageOpacity" { "opacity" } else { "matte" };
    let Some(v) = p.get(key).and_then(Value::as_f64).filter(|v| v.is_finite()) else {
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

pub(crate) fn view_navigator(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.navigator = !app.ui.navigator;
    Ok(json!({"navigator": app.ui.navigator}))
}

pub(crate) fn view_filmstrip(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.filmstrip = !app.ui.filmstrip;
    Ok(Value::Null)
}

pub(crate) fn view_left_panel(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.left_panel = !app.ui.left_panel;
    Ok(Value::Null)
}

pub(crate) fn view_before_after(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.before_after = if app.ui.before_after == BeforeAfter::SideBySide { BeforeAfter::Off } else { BeforeAfter::SideBySide };
    Ok(Value::Null)
}

pub(crate) fn view_before_after_split(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.before_after = if app.ui.before_after == BeforeAfter::Split { BeforeAfter::Off } else { BeforeAfter::Split };
    Ok(Value::Null)
}

pub(crate) fn view_before_after_top_bottom(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.before_after = if app.ui.before_after == BeforeAfter::TopBottom { BeforeAfter::Off } else { BeforeAfter::TopBottom };
    Ok(Value::Null)
}

pub(crate) fn view_before_after_split_top_bottom(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.before_after = if app.ui.before_after == BeforeAfter::SplitTopBottom { BeforeAfter::Off } else { BeforeAfter::SplitTopBottom };
    Ok(Value::Null)
}

pub(crate) fn view_show_original(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.before_after = if app.ui.before_after == BeforeAfter::Original { BeforeAfter::Off } else { BeforeAfter::Original };
    Ok(Value::Null)
}

pub(crate) fn view_zoom_fit(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.zoom = Zoom::Fit;
    Ok(Value::Null)
}

pub(crate) fn view_zoom100(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.zoom = Zoom::Percent(100.0);
    Ok(Value::Null)
}

pub(crate) fn view_zoom_toggle(app: &mut LightcraftApp, _c: &Call) -> Result<Value, String> {
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

pub(crate) fn view_click_zoom(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    // {ratio?: 1|2|3|4|8} → {ratio}
    if let Some(r) = p.get("ratio").and_then(Value::as_f64) {
        let pct = (r * 100.0).round() as u32;
        if !crate::state::CLICK_ZOOMS.contains(&pct) {
            return Err(format!("view.clickZoom: ratio {r} (1, 2, 3, 4 or 8)"));
        }
        app.ui.click_zoom = pct;
    }
    Ok(json!({"ratio": app.ui.click_zoom / 100}))
}

pub(crate) fn view_zoom_in(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let steps = [25.0, 50.0, 100.0, 200.0, 400.0, 800.0];
    let cur = match app.ui.zoom {
        Zoom::Percent(p) => p,
        _ => 25.0,
    };
    let next = if id == "view.zoomIn" {
        steps.iter().find(|s| **s > cur).copied().unwrap_or(800.0)
    } else {
        steps.iter().rev().find(|s| **s < cur).copied().unwrap_or(0.0)
    };
    app.ui.zoom = if next == 0.0 { Zoom::Fit } else { Zoom::Percent(next) };
    Ok(Value::Null)
}

pub(crate) fn view_thumb_size_up(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    // the bottom-bar slider's range, one `THUMB_SIZE_STEP` per call; {} → {size}
    if !thumb_size_view(app) {
        return Err(format!("{id}: only in Photo Grid, Square Grid or People"));
    }
    let step = if id == "view.thumbSizeUp" { crate::state::THUMB_SIZE_STEP } else { -crate::state::THUMB_SIZE_STEP };
    app.ui.thumb_size = (app.ui.thumb_size + step).clamp(crate::state::THUMB_SIZE_MIN, crate::state::THUMB_SIZE_MAX);
    Ok(json!({"size": app.ui.thumb_size}))
}

pub(crate) fn view_navigate(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    // {zoom?: "fit"|"fill"|{percent: number}, pan?: [x, y]} (normalized image centre).
    // Validate the complete request before changing either part of the viewport.
    let zoom = match p.get("zoom") {
        Some(v) => match serde_json::from_value::<Zoom>(v.clone()) {
            Ok(Zoom::Percent(p)) if !p.is_finite() || p <= 0.0 || p > 800.0 => {
                return Err("view.navigate: zoom percent must be greater than 0 and at most 800".into());
            }
            Ok(z) => z,
            Err(e) => return Err(format!("view.navigate: {e}")),
        },
        None => app.ui.zoom,
    };
    let pan = match p.get("pan") {
        Some(v) => match serde_json::from_value::<(f32, f32)>(v.clone()) {
            Ok((x, y)) if x.is_finite() && y.is_finite() && (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y) => (x, y),
            _ => return Err("view.navigate: pan must be [x, y] with finite coordinates from 0 to 1".into()),
        },
        None => app.ui.pan,
    };
    app.ui.zoom = zoom;
    app.ui.pan = pan;
    app.ui.zoom_anim = false;
    Ok(json!({"zoom": zoom, "pan": pan}))
}

pub(crate) fn view_clipping(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.show_clipping = !app.ui.show_clipping;
    Ok(Value::Null)
}

pub(crate) fn view_visualize_hdr(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    // {on?}; no params toggles. Shown on photos edited in HDR (see `develop.hdr`).
    app.ui.hdr_visualize = p.get("on").and_then(Value::as_bool).unwrap_or(!app.ui.hdr_visualize);
    Ok(json!({"on": app.ui.hdr_visualize}))
}

pub(crate) fn view_soft_proof(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    // {on?, space?, profile?: ICC path | null, intent?: perceptual|relative, paperInk?,
    // destWarning?, displayWarning?}; no params toggles
    let has = |k: &str| p.get(k).is_some();
    if let Some(s) = p.get("space").and_then(Value::as_str) {
        match lightcraft_engine::pipeline::OutputSpace::parse(s) {
            Some(sp) => {
                app.ui.proof.space = sp;
                // a standard space replaces a device profile (unless one is given too)
                app.ui.proof_profile = None;
            }
            None => return Err(format!("view.softProof: unknown space {s:?} (srgb|displayP3|adobeRgb|proPhoto|rec2020)")),
        }
    }
    if let Some(i) = p.get("intent") {
        match i.as_str().and_then(lightcraft_engine::proof::ProofIntent::parse) {
            Some(i) => app.ui.proof_intent = i,
            None => return Err("view.softProof: intent must be perceptual or relative".into()),
        }
    }
    if let Some(b) = p.get("paperInk").and_then(Value::as_bool) {
        app.ui.proof_paper_ink = b;
    }
    let path = match p.get("profile") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Null) => {
            app.ui.proof_profile = None;
            None
        }
        Some(_) => return Err("view.softProof: profile must be an ICC file path or null".into()),
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
    if let Some(b) = p.get("destWarning").and_then(Value::as_bool) {
        app.ui.proof.dest_warning = b;
    }
    if let Some(b) = p.get("displayWarning").and_then(Value::as_bool) {
        app.ui.proof.display_warning = b;
    }
    app.ui.soft_proof = match p.get("on").and_then(Value::as_bool) {
        Some(b) => b,
        None if ["space", "destWarning", "displayWarning", "profile", "intent", "paperInk"].iter().any(|k| has(k)) => app.ui.soft_proof,
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

pub(crate) fn view_histogram(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.histogram = !app.ui.histogram;
    Ok(Value::Null)
}

pub(crate) fn view_mask_overlay(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    app.ui.mask_overlay = p.get("show").and_then(Value::as_bool).unwrap_or(!app.ui.mask_overlay);
    Ok(json!({"maskOverlay": app.ui.mask_overlay}))
}

pub(crate) fn view_mask_overlay_all(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.mask_overlay_all = !app.ui.mask_overlay_all;
    Ok(json!({"maskOverlayAll": app.ui.mask_overlay_all}))
}

pub(crate) fn view_mask_overlay_mode(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    use lightcraft_engine::pipeline::MaskView;
    let cur = MaskView::parse(&app.ui.mask_overlay_mode).unwrap_or_default();
    let next = match p.get("mode").and_then(Value::as_str) {
        Some(m) => match MaskView::parse(m) {
            Some(v) => v,
            None => {
                let names: Vec<&str> = MaskView::ALL.iter().map(|v| v.name()).collect();
                return Err(format!("view.maskOverlayMode: unknown mode `{m}` ({})", names.join("|")));
            }
        },
        None => cur.next(),
    };
    app.ui.mask_overlay_mode = next.name().into();
    app.ui.mask_overlay = true;
    app.toast(&ctx, next.label());
    Ok(json!({"mode": next.name()}))
}

pub(crate) fn view_mask_overlay_color(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    // no params: the next of the panel's swatch colours
    if p.get("color").is_none() && p.get("opacity").is_none() {
        let all = crate::panels::masking::OVERLAY_COLORS;
        let i = all.iter().position(|c| *c == app.ui.mask_overlay_color).map_or(0, |i| (i + 1) % all.len());
        app.ui.mask_overlay_color = all[i];
    }
    if let Some(c) = p.get("color") {
        match parse_rgb(c) {
            Some(rgb) => app.ui.mask_overlay_color = rgb,
            None => return Err("view.maskOverlayColor: `color` is \"#rrggbb\" or [r, g, b]".into()),
        }
    }
    if let Some(o) = p.get("opacity").and_then(Value::as_f64) {
        app.ui.mask_overlay_opacity = o.clamp(0.0, 100.0) as f32;
    }
    let [r, g, b] = app.ui.mask_overlay_color;
    Ok(json!({"color": format!("#{r:02x}{g:02x}{b:02x}"), "opacity": app.ui.mask_overlay_opacity}))
}

pub(crate) fn brush_smaller(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let (k, df) = match id {
        "brush.smaller" => (1.0 / 1.2, 0.0),
        "brush.larger" => (1.2, 0.0),
        "brush.featherLess" => (1.0, -10.0),
        _ => (1.0, 10.0),
    };
    Ok(adjust_brush(app, k, df))
}

pub(crate) fn view_mask_pins(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    app.ui.mask_pins = p.get("show").and_then(Value::as_bool).unwrap_or(!app.ui.mask_pins);
    Ok(json!({"maskPins": app.ui.mask_pins}))
}

pub(crate) fn view_tool_overlay(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    use crate::state::ToolOverlay;
    let mode = match p.get("mode") {
        None | Some(Value::Null) => app.ui.tool_overlay.next(),
        Some(v) => match v.as_str().and_then(ToolOverlay::parse) {
            Some(m) => m,
            None => return Err(format!("view.toolOverlay: unknown mode {v} (auto|always|selected|never)")),
        },
    };
    app.ui.tool_overlay = mode;
    app.toast(&ctx, tool_overlay_toast(mode));
    Ok(json!({"mode": mode.name()}))
}

pub(crate) fn view_visualize_spots(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    // like Lightroom's A: opens the Remove tool with the view on, or toggles it there
    if app.ui.right == RightPanel::Remove {
        app.ui.visualize_spots = !app.ui.visualize_spots;
    } else {
        app.ui.right = RightPanel::Remove;
        app.ui.visualize_spots = true;
    }
    Ok(Value::Null)
}

pub(crate) fn view_crop_overlay(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
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

pub(crate) fn view_crop_overlay_orientation(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.crop_overlay_orient = (app.ui.crop_overlay_orient + 1) % 4;
    Ok(json!({"orientation": app.ui.crop_overlay_orient}))
}

pub(crate) fn local_add_root(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    // a folder kept in Local's sidebar (saved with the UI state); nothing on disk changes
    let Some(path) = p.get("path").and_then(Value::as_str) else { return Err("local.addRoot needs a path".into()) };
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

pub(crate) fn local_hide(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    match p.get("path").and_then(Value::as_str) {
        Some(path) => {
            if !app.ui.hidden_locations.iter().any(|h| crate::panels::left::same_folder(h, path)) {
                app.ui.hidden_locations.push(path.to_string());
            }
            Ok(json!({"hidden": app.ui.hidden_locations}))
        }
        None => Err("local.hide needs a path".into()),
    }
}

pub(crate) fn local_restore_hidden(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    // one path, or (no path) every hidden location
    match p.get("path").and_then(Value::as_str) {
        Some(path) => app.ui.hidden_locations.retain(|h| !crate::panels::left::same_folder(h, path)),
        None => app.ui.hidden_locations.clear(),
    }
    Ok(json!({"hidden": app.ui.hidden_locations}))
}

pub(crate) fn view_solo_sections(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let p = c.params;
    bool_param(id, p, "on").map(|on| {
        app.ui.set_single_panel(on.unwrap_or(!app.ui.single_panel));
        json!({"soloSections": app.ui.single_panel})
    })
}

pub(crate) fn view_filter_bar(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.filter_bar = !app.ui.filter_bar;
    if app.ui.filter_bar && !matches!(app.ui.view, ViewMode::PhotoGrid | ViewMode::SquareGrid) {
        app.ui.view = ViewMode::PhotoGrid;
    }
    Ok(json!({"filterBar": app.ui.filter_bar}))
}

pub(crate) fn panel_edit(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    panel(app, &ctx, RightPanel::Edit, "Edit");
    Ok(Value::Null)
}

pub(crate) fn panel_profiles(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
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

pub(crate) fn panel_crop(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    panel(app, &ctx, RightPanel::Crop, "Crop, Rotate, Geometry");
    Ok(Value::Null)
}

pub(crate) fn panel_remove(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    panel(app, &ctx, RightPanel::Remove, "Remove");
    if app.ui.right == RightPanel::Remove && app.ui.tool.is_empty() {
        app.ui.tool = "remove".into();
    }
    Ok(Value::Null)
}

pub(crate) fn panel_masking(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    panel(app, &ctx, RightPanel::Masking, "Masking");
    Ok(Value::Null)
}

pub(crate) fn panel_redeye(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    panel(app, &ctx, RightPanel::RedEye, "Red Eye");
    Ok(Value::Null)
}

pub(crate) fn panel_info(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    panel(app, &ctx, RightPanel::Info, "Info");
    Ok(Value::Null)
}

pub(crate) fn panel_keywords(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    panel(app, &ctx, RightPanel::Keywords, "Keywords");
    Ok(Value::Null)
}

pub(crate) fn panel_versions(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    panel(app, &ctx, RightPanel::Versions, "Versions");
    Ok(Value::Null)
}

pub(crate) fn panel_activity(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    panel(app, &ctx, RightPanel::Activity, "History");
    Ok(Value::Null)
}

pub(crate) fn panel_presets(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.presets = !app.ui.presets || !crate::docking::exposed(app, crate::docking::Panel::Presets);
    if app.ui.presets && app.ui.view != ViewMode::Detail {
        app.ui.view = ViewMode::Detail;
    }
    Ok(Value::Null)
}

pub(crate) fn tool_done(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    // Return commits a tool panel (crop, remove, red eye, masking): back to Edit
    use RightPanel::*;
    if app.ui.dialog.is_none() && matches!(app.ui.right, Crop | Remove | RedEye | Masking) {
        let _ = app.session.end_interaction();
        app.ui.tool.clear();
        app.ui.right = Edit;
    }
    Ok(Value::Null)
}

pub(crate) fn panel_close(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.right = RightPanel::None;
    app.ui.presets = false;
    Ok(Value::Null)
}

pub(crate) fn section(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let s = c.id;
    let p = c.params;
    bool_param(s, p, "solo").and_then(|solo| {
        // only the Edit sections listed above: an unknown one would close the real ones
        if !UI_COMMANDS.iter().any(|c| c.id == s) {
            return Err(format!("{s}: no such Edit section"));
        }
        let sec = &s["section.".len()..];
        if app.ui.right != RightPanel::Edit {
            app.ui.right = RightPanel::Edit;
        }
        if solo == Some(true) {
            app.ui.solo_section(sec);
        } else {
            app.ui.toggle_section(sec);
        }
        Ok(Value::Null)
    })
}

pub(crate) fn tool(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let s = c.id;
    let p = c.params;
    let tool = &s["tool.".len()..];
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
            if !guided && let Err(e) = app.run("geometry.upright", json!({"mode": "guided"})) {
                return Err(e);
            }
            app.ui.tool = "guidedUpright".into();
        }
        "brush" => {
            // A new mask and Add/Subtract are selection operations, not the brush's
            // Erase mode. Paint positively into a component with the requested op.
            let create = if p.get("new").and_then(Value::as_bool) == Some(true) {
                Some(app.run("mask.add", json!({"kind": "brush"})))
            } else {
                p.get("op").map(|op| app.run("mask.addComponent", json!({"kind": "brush", "op": op})))
            };
            if let Some(result) = create {
                result?;
                app.ui.brush_erase = false;
                app.ui.mask_overlay = true;
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

pub(crate) fn view_focus_search(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    // the search field lives in the top bar of the library and detail views alike
    app.ui.focus_search = true;
    Ok(Value::Null)
}

/// `ui.dock` / `ui.dock.reset`: arrange the docked panels.
pub(crate) fn dock(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    crate::docking::command(app, c.id == "ui.dock.reset", c.params)
}
