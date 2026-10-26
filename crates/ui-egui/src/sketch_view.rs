//! Sketch entry and exit (Plan B-2 Task 6): under a profile that asks for it, every way into a
//! sketch (a plane picked, Edit Sketch, the control channel's `sketch.create`) looks at its plane,
//! and leaving it (Finish, Cancel, a feature that consumes it) animates back to the view the
//! camera stood at before. All of it goes through `dialogs::look_at_sketch` and
//! [`SolveApp::animate_to`], the path every view command takes. SolidCraft's profile turns both
//! off: there only `start_sketch` and `edit_sketch` look at the sketch, as they always have.

use solidcraft_engine::render::Camera;

use crate::SolveApp;

/// One per design (it goes with the tab, see `documents::DocSlot`).
#[derive(Default)]
pub struct SketchView {
    /// The sketch open as of the last [`tick`].
    open: Option<u64>,
    /// The settled view from before the sketch was entered.
    return_to: Option<Camera>,
}

/// Once per frame, before `view_history::settle`: on None→Some remember the settled view (when
/// restoring) and look at the sketch unless a look-at is already animating (when looking);
/// on Some→Some look at the new sketch; on Some→None animate back to the remembered view.
pub(crate) fn tick(app: &mut SolveApp) {
    let now = app.session.active_sketch;
    let was = std::mem::replace(&mut app.sketch_view.open, now);
    if now == was {
        return;
    }
    let flow = &crate::profile::active(app).workflow;
    let (look, restore) = (flow.look_at_on_sketch, flow.restore_view_after_sketch);
    match (was, now) {
        (None, Some(_)) => {
            // The view from before: `settle` has not run since the sketch opened, so a look-at
            // that already landed (no animation) is not it yet.
            app.sketch_view.return_to = restore.then(|| crate::view_history::settled(app).unwrap_or(app.cam));
            if look {
                look_at(app);
            }
        }
        (Some(_), Some(_)) => {
            if look {
                look_at(app);
            }
        }
        (Some(_), None) => {
            let to = app.sketch_view.return_to.take();
            if let Some(to) = to
                && restore
                && !crate::render_workspace::active(app)
            {
                app.animate_to(to);
            }
        }
        (None, None) => {}
    }
}

/// Look at the open sketch, unless the camera already faces it or is on its way there
/// (`start_sketch` and `edit_sketch` look at it themselves, a face sketch at the picked point).
fn look_at(app: &mut SolveApp) {
    let st = app.session.world_state();
    let Some(normal) = app.session.active_sketch.and_then(|id| st.sketch(id)).map(|ss| ss.plane.normal()) else {
        return;
    };
    let heading = app.cam_anim.as_ref().map_or(app.cam, |anim| anim.to);
    if heading.back().dot(normal) > 0.999 {
        return;
    }
    crate::dialogs::look_at_sketch(app);
}

#[cfg(test)]
#[path = "sketch_view_tests.rs"]
mod tests;
