//! Navigation preferences, composed from the shared settings controls and figures.
use super::*;

/// A navigation checkbox whose default is the interface profile's (`PROFILE_NAV_PREFS`): it shows
/// the setting in force, and a change makes the setting the user's, over the profile's. Its hover
/// text says so after the setting's own help.
fn profile_check(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    key: &str,
    label: &str,
    help: Option<&str>,
    get: fn(&SolveApp) -> bool,
    set: fn(&mut Prefs, bool),
) -> egui::Response {
    let mut value = get(app);
    let response = ui_kit::checkbox(ui, label, &mut value);
    if response.changed() {
        set(&mut app.preferences, value);
        app.preferences.navigation_set.insert(key.to_string());
    }
    let precedence = crate::i18n::tr("prefs.nav_follows_profile");
    response.on_hover_text(match help {
        Some(help) => format!("{help}\n\n{precedence}"),
        None => precedence.to_string(),
    })
}

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui, t: &Tokens) {
    ui_kit::settings_heading(ui, app.tr("prefs.navigation"), app.tr("prefs.navigation_note"));
    ui_kit::settings_section(ui, "mouse", app.tr("prefs.mouse_heading"), |ui| {
        let mut gestures: Vec<_> = crate::navigation::bindings(app)
            .iter()
            .filter(|binding| !(crate::mouse_gesture::active(app) && binding.button == NavButton::Right))
            .map(|binding| {
                let button = match binding.button {
                    NavButton::Left => "prefs.button_left",
                    NavButton::Middle => "prefs.button_middle",
                    NavButton::Right => "prefs.button_right",
                };
                let mut keys = Vec::new();
                if binding.keys.shift {
                    keys.push("Shift");
                }
                if binding.keys.ctrl {
                    keys.push("Ctrl/Cmd");
                }
                if binding.keys.alt {
                    keys.push("Alt/Option");
                }
                keys.push(crate::i18n::tr(button));
                let action = match binding.mode {
                    NavMode::Orbit => "prefs.gesture_orbit",
                    NavMode::Pan => "prefs.gesture_pan",
                    NavMode::Zoom => "prefs.gesture_zoom",
                };
                (keys.into_iter().map(str::to_owned).collect::<Vec<_>>(), crate::i18n::tr(action).to_string())
            })
            .collect();
        gestures.extend([
            (
                vec![app.tr("prefs.wheel").to_owned()],
                app.tr(if crate::navigation::zoom_to_cursor(app) { "prefs.zoom_toward_cursor" } else { "prefs.gesture_zoom" }).to_owned(),
            ),
            (vec![app.tr("prefs.left_click").to_owned()], app.tr("prefs.select").to_owned()),
            (
                vec![app.tr("prefs.right_click").to_owned()],
                app.tr(if app.preferences.mouse_gestures.enabled && app.preferences.nav == crate::mouse_gesture::MOUSE_MODE {
                    "prefs.context_menu_ring"
                } else {
                    "prefs.context_menu"
                })
                .to_owned(),
            ),
        ]);
        ui_kit::settings_mouse_mapping(ui, &gestures, |ui| {
            ui_kit::settings_form(ui, 110.0, |ui| {
                ui_kit::row(ui, app.tr("prefs.mouse_mapping"), |ui| {
                    let picker = ui_kit::dropdown_menu(ui, "pref_nav", nav_preset(&app.preferences.nav).translated_label(), |ui| {
                        for preset in &NAV_PRESETS {
                            if ui_kit::list_picker(ui, preset.id, preset.translated_label(), app.preferences.nav == preset.id).clicked() {
                                crate::prefs::choose_preset(app, preset.id);
                            }
                        }
                    });
                    crate::scenario::publish_target("dialog:Pan, zoom, orbit", picker.rect);
                    let help = crate::i18n::tr_label(nav_preset(&app.preferences.nav).help);
                    picker.on_hover_text(format!("{help}\n\n{}", crate::i18n::tr("prefs.nav_follows_profile")));
                });
            });
        });
        // These are real navigation options absent from the static board; retain them below
        // the illustrated mapping rather than enlarging the diagram or repeating its legend.
        let free_orbit =
            profile_check(app, ui, "free_orbit", crate::i18n::tr("nav.free_orbit.label"), None, crate::navigation::free_orbit, |p, v| {
                p.free_orbit = v
            });
        crate::scenario::publish_target("prefs:free_orbit", free_orbit.rect);
        let free = crate::navigation::free_orbit(app);
        if free {
            ui_kit::text(ui, crate::i18n::tr("nav.free_orbit.help"), ui_kit::Text::Hint("free_orbit"));
        }
        let help = if app.preferences.nav == TOUCHPAD_NAV {
            app.tr("nav.touchpad.help").to_owned()
        } else if !free && app.preferences.nav != OPENSCAD_NAV {
            format!("{} {}", app.tr("prefs.extra_navigation"), app.tr("prefs.orbit_snap"))
        } else {
            app.tr("prefs.extra_navigation").to_owned()
        };
        ui_kit::text(ui, &help, ui_kit::Text::Hint("navigation"));
    });
    ui_kit::settings_section(ui, "orbit_pivot", app.tr_label("Orbit around"), |ui| {
        // The card in force: the user's choice, else the profile's pivot (the model point under the
        // cursor shows as Under the cursor; a selection pivot has no card).
        let shown = match crate::navigation::pivot(app) {
            crate::navigation::Pivot::Origin => Some(OrbitAround::Origin),
            crate::navigation::Pivot::ViewCentre => Some(OrbitAround::ViewCentre),
            crate::navigation::Pivot::Cursor | crate::navigation::Pivot::CursorElseCentre => Some(OrbitAround::Cursor),
            crate::navigation::Pivot::Selection => None,
        };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = t.space_8;
            for (index, (mode, label)) in
                [(OrbitAround::Origin, "Origin"), (OrbitAround::ViewCentre, "View centre"), (OrbitAround::Cursor, "prefs.under_cursor")]
                    .into_iter()
                    .enumerate()
            {
                if ui_kit::option_card(ui, app.tr_label(label), None, shown == Some(mode), true, ui_kit::PreferenceFigure::Pivot(index))
                    .on_hover_text(crate::i18n::tr("prefs.nav_follows_profile"))
                    .clicked()
                {
                    app.preferences.orbit_around = mode;
                    app.preferences.navigation_set.insert("orbit_around".into());
                }
            }
        });
    });
    ui_kit::settings_section(ui, "wheel_trackpad", app.tr("prefs.wheel_trackpad"), |ui| {
        // Board order is column-major in meaning: zoom behavior at left, gesture behavior at
        // right. Each checkbox keeps its original shared-control label and scenario target.
        let width = (ui.available_width() - (t.space_16 + t.space_4)) * 0.5;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = t.space_16 + t.space_4;
            ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                profile_check(
                    app,
                    ui,
                    "zoom_to_cursor",
                    crate::i18n::tr_label("Zoom to the cursor"),
                    Some(crate::i18n::tr_label("The wheel zooms toward the point under the cursor (off: toward the view's centre)")),
                    crate::navigation::zoom_to_cursor,
                    |p, v| p.zoom_to_cursor = v,
                );
                profile_check(
                    app,
                    ui,
                    "zoom_reverse",
                    crate::i18n::tr_label("Reverse zoom direction"),
                    None,
                    crate::navigation::wheel_zooms_out,
                    |p, v| p.zoom_reverse = v,
                );
                setting::pan_reverse_x(app, ui);
            });
            ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                profile_check(
                    app,
                    ui,
                    "smooth_wheel_zoom",
                    crate::i18n::tr_label("Smooth wheel zoom"),
                    Some(crate::i18n::tr_label("Turn off to apply each wheel event immediately, without a smoothing tail")),
                    crate::navigation::smooth_wheel,
                    |p, v| p.smooth_wheel_zoom = v,
                );
                let touchpad = app.preferences.nav == TOUCHPAD_NAV;
                ui.add_enabled_ui(!touchpad, |ui| {
                    ui_kit::checkbox(ui, crate::i18n::tr_label("Two-finger scroll pans"), &mut app.preferences.trackpad_pan)
                        .on_hover_text(if touchpad { crate::i18n::tr("nav.touchpad.scroll") } else { crate::i18n::tr("ui.trackpad_pan_tip") });
                });
                setting::radial_context_menu(app, ui);
                setting::direct_interaction(app, ui);
            });
        });
        ui_kit::divider(ui, "navigation_view_controls");
        ui_kit::settings_form(ui, 150.0, |ui| {
            ui_kit::row(ui, app.tr_label("View navigator"), |ui| {
                let mut selected = usize::from(app.preferences.navigator_style == NavigatorStyle::Cube);
                if ui_kit::segmented(
                    ui,
                    &mut selected,
                    &[app.tr("prefs.axes"), crate::i18n::tr_label("Cube")],
                    ui_kit::Segments { intrinsic: true, ..Default::default() },
                ) {
                    app.preferences.navigator_style = if selected == 0 { NavigatorStyle::Axes } else { NavigatorStyle::Cube };
                }
                if let Some(rect) = crate::scenario::rect_of(&format!("dialog:segments:{}", crate::i18n::tr_label("View navigator"))) {
                    crate::scenario::publish_target("prefs:navigator", rect);
                    for (label, key) in [(app.tr("prefs.axes"), "Signed axes"), (crate::i18n::tr_label("Cube"), "Cube")] {
                        if let Some(at) = crate::scenario::handle_at(&format!("dialog:{}:{label}", crate::i18n::tr_label("View navigator"))) {
                            crate::scenario::publish_target(
                                &format!("prefs:navigator:{key}"),
                                egui::Rect::from_center_size(at, egui::vec2(t.space_24, t.control_height)),
                            );
                        }
                    }
                }
            });
            ui_kit::row(ui, app.tr_label("Middle double-click"), |ui| {
                let current = MiddleDoubleClick::of(app);
                ui_kit::settings_control_width(ui, t.form_label_width * 2.0 + t.space_24 * 2.0 + t.space_12, |ui| {
                    ui_kit::dropdown_menu(ui, "pref_middle_double_click", crate::i18n::tr_label(current.label()), |ui| {
                    for (mode, id, label) in MiddleDoubleClick::ALL {
                        if ui_kit::list_picker(ui, id, crate::i18n::tr_label(label), current == mode).clicked() {
                            app.preferences.middle_double_click = id.into();
                        }
                    }
                    }).on_hover_text(crate::i18n::tr_label("Double-clicking the middle button in the view: Home (the home view, framing what is shown), Fit (frame what is shown, keeping the orientation) or nothing"));
                });
            });
            setting::look_at_anim(app, ui);
            setting::look_at_speed(app, ui);
        });
        // The existing ring editor is part of the real product and remains available.
        crate::mouse_gesture::nav_rows(app, ui, t);
    });
    // The 3D mouse is a section like the ones above: the driver, calibration, axes, picture and buttons
    // (`prefs::space_mouse_section`).
    space_mouse_section(app, ui);
}
