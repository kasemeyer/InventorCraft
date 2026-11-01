# Interface profiles: the navigation block

An interface profile (`crates/ui-egui/src/profile.rs`) is data: the ribbon of each document kind,
the keyboard aliases, the browser layout, the ViewCube style, the preference defaults and, since
F-2 (R210), the **navigation scheme**. SolidCraft's profile is built in code from the `workspace`
tables (`profile::solidcraft`); the Inventor profile is `assets/profiles/inventor.json`, read by
`profile::parse` and checked by `profile::validate`.

## The `navigation` block

`crates/ui-egui/src/profile_navigation.rs` reads it. When a profile has the block, every key is
written out (unknown keys are errors); a profile without one navigates as SolidCraft
(`Navigation::solidcraft`). `navigation::handle` is the only place the scheme moves the camera.

| Key | Values | SolidCraft | Inventor |
|---|---|---|---|
| `wheel_direction` | `forward_zooms_in`, `forward_zooms_out` | `forward_zooms_in` | `forward_zooms_out` |
| `wheel_step` | fraction of the view distance one notch (40 points) zooms in by, above 0 and at most 0.5 | 0.0582 (`1 - exp(-40 × 0.0015)`) | 0.15 |
| `wheel_smoothing` | spread a notch over frames | `true` | `false` |
| `zoom_to_cursor` | the wheel zooms about the point under the cursor | `true` | `true` |
| `drag_zoom` | `up_zooms_in`, `up_zooms_out` (Ctrl+middle, F3 held) | `up_zooms_in` | `up_zooms_out` |
| `orbit_style` | `turntable`, `free` | `turntable` | `free` |
| `orbit_pivot` | `view_centre`, `model_point_under_cursor` (else the view centre), `selection` (else the view centre) | `view_centre` | `model_point_under_cursor` |
| `pivot_glyph` | mark the pivot while orbiting | `false` | `true` |
| `buttons` | `{pan, orbit, zoom}`: `"middle"`, `"shift+middle"`, `"ctrl+middle"`… or `null` | middle / shift+middle / none | middle / shift+middle / ctrl+middle |
| `right_drag_orbit` | a right drag orbits | `true` | `false` (the right button is the marking menu's) |
| `ctrl_left_drag_orbit` | a Ctrl+left drag from empty space orbits | `true` | `false` |
| `hold_keys` | `F1`–`F12` → `pan`, `zoom` or `orbit` while held (a left drag navigates) | none | F2 pan, F3 zoom, F4 orbit |
| `keys` | `[{key, action, scope}]`: a key chord (`"Home"`, `"Shift+F5"`) → a UI action where the alias `scope` is live | none | Home, End, Page Up, Shift+F5 (below) |
| `cursors` | the pan, zoom and orbit cursors while navigating | `false` | `true` |

SolidCraft's values are its behaviour before the block existed; the test
`navigation::profile_tests::solidcraft_navigation_is_unchanged` replays the same wheel, drag,
hold-key and navigation-key inputs and compares the camera with numbers recorded at 6a7a74e8.
Inventor's values are its documented defaults (sources in the planning repository's
`docs/evidence/navigation-fidelity-audit.md`).

## Keys

Inventor's navigation keys, as the profile lays them out (`navigation::key_action`,
`navigation::hold_key_press`, dispatched by the shortcut loop in `toolbar.rs` before the function
keys):

| Key | Does | Where |
|---|---|---|
| F2 (held) | a left drag pans | over the graphics window; elsewhere F2 renames the selection |
| F3 (held) | a left drag zooms (the profile's `drag_zoom`) | everywhere |
| F4 (held) | a left drag orbits (the profile's orbit style and pivot) | where the view can orbit |
| Home | Zoom All (`ui.view_fit_all`): everything shown, whatever is selected; a drawing's sheet | everywhere |
| End | Zoom Selected (`ui.zoom_selected`): the selection; nothing when nothing is selected | a part or assembly |
| Page Up | Look At (`ui.look_at`): the selected face, plane or sketch | a part or assembly |
| Shift+F5 | Next View (`ui.view_next`): forward through the views F5 stepped back from | everywhere |
| F5, F6 | Previous View, Home View (`function_keys`, unchanged) | everywhere |

While a hold key is held, a drag pans, zooms or orbits, or Space pans, a profile with `cursors`
shows that navigation's cursor in place of the pointer: original glyphs
`assets/icons/ui/design/svg/flat/cursor_{pan,zoom,orbit}.svg` (monochrome navigation family, left
Open on the icon review page for the owner), drawn in the text token on a panel-token halo. A text field, or a dimension
being edited, keeps its keys. A held Space still pans in both profiles.

## Precedence over Preferences

Six preferences have a profile default: the mouse mapping (`nav`), Reverse zoom direction
(`zoom_reverse`), Free orbit (`free_orbit`), Orbit around (`orbit_around`), Zoom to the cursor
(`zoom_to_cursor`) and Smooth wheel zoom (`smooth_wheel_zoom`) (`prefs::PROFILE_NAV_PREFS`).

- **The user's setting wins.** A preference changed on the Navigation page or by `ui.prefs`
  goes into `Prefs::navigation_set` and from then on rules over the profile, even when it holds
  the default value. A preference saved away from its default before the block existed counts as
  set too. Picking a mouse mapping sets the mapping and its wheel direction.
- **Otherwise the profile's value applies**, read live (`navigation::wheel_zooms_out`,
  `free_orbit`, `pivot`, `zoom_to_cursor`, `smooth_wheel`, `bindings`). The page shows the value
  in force, and its hover text says so.
- **Restore Defaults** on the Navigation page empties `navigation_set` and gives the mouse
  mapping back to the profile's own.
- Choosing a profile sets its mouse mapping, and its `buttons` rule until the user picks one.
- The trackpad pinch and the 3D mouse's zoom follow only the user's Reverse zoom direction, never
  a profile's wheel direction.
