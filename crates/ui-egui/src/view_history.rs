//! Previous View (Inventor's F5): a short history of the views the camera stood still at.
//!
//! The history records *settled* views only: once per frame [`settle`] looks at the camera, and
//! only when no gesture is moving it (no button held, no scroll) and no view animation runs does a
//! camera different from the last settled one become settled, the old one going on the back stack.
//! [`previous`] pops that stack and animates there through [`SolveApp::animate_to`], the same path
//! every view command takes; the view it leaves is not pushed again, so repeated presses walk back.

use solidcraft_engine::render::Camera;

use crate::SolveApp;

/// The deepest the back stack goes; older views are dropped.
pub const MAX_VIEWS: usize = 32;

/// How long a 3D mouse's cap rests before the view it left counts as settled (seconds).
pub const DEVICE_REST_S: f64 = 0.3;

#[derive(Default)]
pub struct ViewHistory {
    /// The camera as it last stood still.
    settled: Option<Camera>,
    /// Earlier settled views, oldest first.
    back: Vec<Camera>,
    /// `previous` is moving to a popped view: settling there pushes nothing.
    restoring: bool,
}

impl ViewHistory {
    /// The view `previous` was moving to was taken over (`cancel_view_animation`): where the
    /// camera settles now is a new view, pushed like any other.
    pub(crate) fn end_restore(&mut self) {
        self.restoring = false;
    }
}

/// Whether a gesture is moving the camera this frame: a button held, a scroll, a spin, or 3D
/// mouse motion in the last [`DEVICE_REST_S`] (its every frame would otherwise settle a view).
/// While the cap is resting it asks for the frame after the rest, so the view settles without
/// waiting for other input.
pub(crate) fn busy(app: &SolveApp, ctx: &egui::Context) -> bool {
    if ctx.input(|i| i.pointer.any_down() || i.is_scrolling()) || app.viewport.spin {
        return true;
    }
    let Some(at) = app.space_mouse.moved_at else {
        return false;
    };
    let left = (DEVICE_REST_S - (app.now - at)).min(DEVICE_REST_S);
    if !(left > 0.0) {
        return false;
    }
    ctx.request_repaint_after(std::time::Duration::from_secs_f64(left + 0.01));
    true
}

/// Once per frame: when nothing moves the camera (`!busy`, no animation), a camera different from
/// the settled one becomes settled and the old one is pushed (unless it came from `previous`).
pub(crate) fn settle(app: &mut SolveApp, busy: bool) {
    if busy || app.cam_anim.is_some() {
        return;
    }
    let cam = app.cam;
    let views = &mut app.views;
    match views.settled {
        Some(old) if old == cam => {}
        Some(old) => {
            if !std::mem::take(&mut views.restoring) {
                views.back.push(old);
                let over = views.back.len().saturating_sub(MAX_VIEWS);
                views.back.drain(..over);
            }
            views.settled = Some(cam);
        }
        None => views.settled = Some(cam),
    }
}

/// The camera as it last stood still (`sketch_view` reads it before a sketch's look-at settles).
pub(crate) fn settled(app: &SolveApp) -> Option<Camera> {
    app.views.settled
}

/// Animate to the view before the last change; false when there is none.
pub(crate) fn previous(app: &mut SolveApp) -> bool {
    let Some(to) = app.views.back.pop() else {
        return false;
    };
    app.views.restoring = true;
    app.animate_to(to);
    true
}

/// Whether `previous` has a view to go to.
pub(crate) fn can_go_back(app: &SolveApp) -> bool {
    !app.views.back.is_empty()
}

#[cfg(test)]
#[path = "view_history_tests.rs"]
mod tests;
