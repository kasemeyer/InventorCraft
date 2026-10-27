# Vendored third-party crates

Copies of third-party crates that SolidCraft builds with local fixes, wired in with
`[patch.crates-io]` in the workspace `Cargo.toml`. They keep their own licence; changed lines
are marked `SolidCraft:` in the source.

## truck-shapeops 0.4.0 (Apache-2.0, © RICOS Co. Ltd., https://github.com/ricosjp/truck)

Changes:
- `transversal/intersection_curve/mod.rs`: reject empty/single-point polylines and
  collapsed or non-finite segments before endpoint indexing and normalization. A hidden
  constructor re-export lets the kernel workspace tests exercise the actual vendored code.
- `transversal/integrate/mod.rs`: mesh both shells with `robust_triangulation` (faces made by
  earlier booleans have boundaries that lie on their surface only within tolerance).
- `transversal/divide_face/mod.rs`: when projecting a loop onto a face's surface, fall back to
  the nearest parameter when the exact search fails (intersection curves are approximations).
- `transversal/integrate/mod.rs`: the result's shells and faces keep the order of the shell
  they come from (truck's connected components followed hash order of face addresses, so a
  boolean's faces came out in a different order on every run).
- `transversal/loops_store/mod.rs`: a point at an end of a boundary curve (within the point
  tolerance) is that end, whatever its parameter: a polyline's parameter counts segments, so
  an intersection curve ending 2e-7 mm from another's end could read as inside it, cut it
  into a sliver with a vertex of its own, and leave the result's shell open (#391).
- `transversal/loops_store/mod.rs`: an intersection edge added to a face at a vertex that more
  than one wire visits (several curves end on it: the touching point of two equal cylinders) goes
  into the corner that holds its direction, swept counterclockwise about the face's normal from
  the wire's outgoing edge to the way back along its incoming one (`add_edge_by_angle`, told on
  the polygon loops and followed by the geometry loops). Taking the first visit of each end
  joined wires that are different regions of a face and left a wire visiting a vertex twice.
  With no direction to go by, the edge goes where both of its ends are in one wire.
- `transversal/divide_face/mod.rs`: a face piece whose wire is not simple makes the operation
  give up (`Face::try_new`) instead of panicking.
- `transversal/divide_face/mod.rs`: a loop whose polygon in the face's parameters has no area
  (a small hole's rim divided at the tolerance of a much larger operand: one chord per half
  circle) is divided again, finer, before its area's sign tells a hole from a face piece. The
  sign of a zero area was random: the hole became a face of its own, inside out, and the face it
  was cut in stayed whole.
- `lib.rs`: `tr!` reports where an operation gives up when `SHAPEOPS_TRACE` is set; compiler
  warnings no longer fail the build.
- Face and edge history (`transversal/history.rs`, new): `and_with_history` / `or_with_history`
  also return, for each face of the result (in `face_iter` order), the operand face it is a
  region of and whether it is that face unchanged, and for each edge (distinct edges in order
  of first use along the faces) the operand edge it is (a piece of) or the two operand faces
  whose intersection made it. The code that creates faces and edges records them as it goes:
  `divide_face` (face pieces), `loops_store` (intersection edges, split edges, edges rebuilt on
  a moved vertex), across the passes that fold in the operands' extra shells. `and` / `or` are
  wrappers that drop the history; the `*_with_history` forms return the same solid with the
  history as an `Option` (`None` where a face or edge went unrecorded).
- `transversal/integrate/mod.rs`: `meshes` and `and_with_history_meshed` take the first
  operand's shell meshes made beforehand (copied for each use, since the loops stores cut their
  edges): a large body cut down to the regions of many small ones is meshed once, which was most
  of each operation's time.
- `transversal/integrate/mod.rs`: in an AND, a face no intersection divides (a shell that meets
  nothing of the other) is placed by the crossings of a ray with the other shell counted as each
  face is oriented, not by its surface alone, and an inward shell (a void, a cut's inverted tool)
  holds what lies outside it. Faces with reversed surfaces (a revolved body's) counted the wrong
  way, and a void dropped every face outside it: a cut beside a sealed void came out empty or
  with the body gone. An OR keeps truck's count.

## truck-geometry 0.5.0 (Apache-2.0, © RICOS Co. Ltd., https://github.com/ricosjp/truck)

Changes:
- `decorators/intersection_curve.rs`: `IntersectionCurve::parameter_division` divides as
  `algo::curve::parameter_division` does (the same split points, in the same order), but each
  point's projection onto the two surfaces starts from the parameters of the point that begins
  its piece, instead of a search of the whole surfaces (a grid pre-search of a B-spline surface
  for every point: a small hole on a large curved face took seconds, #1038). A hinted result
  farther from the leader's point than the curve's tolerance (the projection settled on another
  part of a surface, such as the far side of a small hole) is dropped for the search from
  nothing, as before. `crates/kernel/src/issue_1038_tests.rs` holds it to that search.
- `decorators/revolved_curve.rs`: a point on a revolved surface's seam (angle 0, the same place
  as 2π) takes the end of the period nearer the hinted angle. The angle was decided by the sign of
  a cross product that is rounding noise there, so a boundary followed point by point could come
  back to the seam at the other end of the period and fold its parameter polygon over itself (no
  intersection loop then lay inside the face). `crates/kernel/src/seam_faces_tests.rs` holds it
  to a cone read from STEP.
- `nurbs/bspsurface.rs`: `uuder` and `vvder` multiply by p·(p − 1), as `BSplineCurve::der2`
  does. They multiplied by p only, so the second derivatives of every B-spline and NURBS surface
  of degree p ≥ 3 were 1/(p − 1) of their value (a cubic surface's curvature read as half, #763).
  `crates/kernel/src/surface_second_derivative_tests.rs` holds them to finite differences.
- `decorators/intersection_curve.rs`: `IntersectionCurve::parameter_division` with a tolerance
  below truck's `TOLERANCE` (1e-6), or one that is not a number, divides at `TOLERANCE` instead
  of panicking in truck's tolerance assertion; a join failed on that panic (#4623).
  `crates/kernel/src/issue_1038_tests.rs` holds it to the division at 1e-6.

## truck-stepio 0.3.0 (Apache-2.0, © RICOS Co. Ltd., https://github.com/ricosjp/truck)

Only the writer (`out`) is used; SolidCraft reads STEP with its own reader.

Changes:
- `out/geometry.rs`: an `INTERSECTION_CURVE`'s second surface was written at the first
  surface's entity index, so every boolean-made edge produced duplicate entity ids.
- `out/geometry.rs`: surfaces of revolution carry the right `same_sense` for inverted and
  mirrored revolutions (ISO 10303-42 parameterises them by angle then profile, so their normal
  is the opposite of truck's; a mirroring transform reverses it again), and the modeling-surface
  wrapper passes the revolution's sense through instead of always `true`.
- `out/geometry.rs`: a surface of revolution under a transform that commutes with turning about
  its axis (a scale across the axis that differs from the scale along it, as a shell or offset
  makes of a revolved face) is written as the transformed profile revolved about the
  transformed axis. Only uniform scales were taken. Other transforms are still refused: the
  writer stops with a formatting error, which SolidCraft's `step.rs` now reports as an export
  error (`to_string` panicked on it).
- `lib.rs`: compiler warnings no longer fail the build. Example and test targets are dropped
  (their sources are not vendored).

## truck-meshalgo 0.4.0 (Apache-2.0, © RICOS Co. Ltd., https://github.com/ricosjp/truck)

Changes:
- `tessellation/triangulation.rs`, `closed.rs`: retry nearest trim projections outside a
  spline's knot domain without the preceding point's hint, preferring a bounded fit within
  the caller's chord tolerance, with a bounded damped least-squares fallback where
  Newton's method diverges. Affine plane charts remain unbounded; modelling surfaces may
  retain genuine continuation when no bounded fit is accurate enough. Geometrically closed,
  smooth spline seams get a local periodic chart during meshing, with wrapped evaluation,
  so a trim crossing the seam stays a small patch. The stored geometry is unchanged (#1974).
- `tessellation/mod.rs`, `triangulation.rs`: a separate provisional `preview_face`
  path accepts at most 4,096 already sampled boundary points and a fixed grid of
  at most 16 subdivisions per direction, including periodic seam connections.
  It does not run adaptive subdivision or grid balancing. The kernel caller bounds
  projection complexity and total allocation before using it; ordinary modelling
  tessellation keeps its existing accuracy policy.
- `tessellation/triangulation.rs`: exact, robust and provisional-preview projections prefer an
  equivalent in-domain parameter at collapsed surface edges, and snap to exact bounded
  endpoints only at collapsed edges when that improves the spatial fit within the existing
  search tolerance. Closed spline seams retain their distinct parameter endpoints.
  This prevents huge arbitrary pole parameters from corrupting trimmed meshes (#796),
  while retaining genuine extrapolation for unbounded planes. Work is bounded to four
  endpoint evaluations per projected point; no search or tessellation tolerance is relaxed.
- `tessellation/triangulation.rs`: a boundary loop is projected onto its surface point by point,
  each searched from the one before; the first has no hint. When that walk fails, it is tried
  once more from the first point an unhinted search finds. An unhinted search can miss a point
  that lies on the surface, such as the corner of a fillet strip on a surface extended past it,
  and the face was then left unmeshed, which also failed every boolean with the body (#882). A
  walk that succeeds is kept as it was: where a loop starts decides how it is laid out across a
  periodic surface's seam.
- `tessellation/mod.rs`, `triangulation.rs`: edges are sampled through a new `TrySubs` trait
  (`try_subs`, `approx_subs`), a bound of `PolylineableCurve`, instead of `subs`. truck's
  `IntersectionCurve::subs` unwraps a projection onto its two surfaces that can fail to
  converge, which panicked while meshing (#1252). A failed sample at an interval's end uses the
  intersection curve's leader point, so the polyline stays finite; a failed interior probe is
  NaN, which already counts as flat. `truck-shapeops`' `ShapeOpsCurve` carries the same bound,
  and `truck-modeling` is now a regular dependency (it was a dev-dependency).
- `tessellation/triangulation.rs`: the inside test of a face's boundary skips loops whose
  parameter box is clear of the point (it tested every segment of every loop, so a face with
  hundreds of holes meshed in quadratic time).
- `tessellation/triangulation.rs`: meshed faces and edges are built unchecked: faces whose loop
  runs along a seam edge both ways and closed edges (a full circle on one vertex) are valid,
  but the debug-build checks rejected them (debug builds could not mesh them).
- `tessellation/triangulation.rs`: on a periodic surface, a boundary point's projected parameter
  is moved by whole periods to within half a period of the point before it, however many periods
  away the search put it (it was moved by two periods at most). A hinted search on a revolved
  surface can return an angle millions of turns from its hint; the loop then spanned those turns,
  the polyline closing it across the seam had millions of points, and meshing never finished,
  which hung the STEP export that reads its own file back (#1985).
- `tessellation/triangulation.rs`: parameter searches that land outside a surface's bounded,
  non-periodic domain are rejected (a hinted B-spline search can run off and extrapolate to
  thousands of parameter units, and meshing that grid never finishes); the last-resort nearest
  search is clamped into the domain.
- `tessellation/triangulation.rs`, bounded work (hostile files must not hang): edge polylines
  and surface grids come from our own bounded divisions (at most 20 000 points per edge, a
  grid budget per face shared out of 10 million points per shell, 24 refinement rounds; a
  point that does not evaluate counts as flat instead of recursing). Surface grids refine only
  the direction that bends, and a ruling left undivided gets cells comparable to the other
  direction (long triangles from a trim loop to the grid's far ends cut through cylinders).
- `tessellation/triangulation.rs`: an edge's division evaluates each parameter once. A piece
  carries the points of its ends and its midpoint is an end of both halves: 2 evaluations per
  piece instead of 4, and the same polyline (a point of an intersection curve costs two grid
  searches of its surfaces). `bounded_polyline` and `Deflection::turn2`/`turns` are public but
  hidden, for the kernel's regression against the old division.
- `tessellation/mod.rs`, `tessellation/triangulation.rs`, for display: a `Deflection` is the
  chord tolerance plus an optional bound on the angle one facet spans, and
  `RoundMeshableShape::robust_triangulation_within` divides by it: an edge or a grid direction
  within the chord tolerance is still halved while a piece turns more than the angle, until its
  sag is under a floor (a radius that is small beside the tolerance was one flat facet).
  Bounded work: a face that would be bounded by more than 4096 segments has its curved edges
  divided at twice the angle, and again, down to the chord tolerance alone, and the angle
  stops refining a face's grid at 16 384 points. Each edge is still divided once for both its
  faces. `triangulation` and `robust_triangulation` divide by the chord tolerance alone, as
  before.
- `analyzers/collision.rs`: the collision search builds each triangle's box once, filters the
  sweep's open list by box overlap as it goes (it cloned the list for every triangle and built
  both boxes for every pair), and runs the exact triangle tests on the rayon pool, in order
  (sequential on wasm). Same collisions in the same order. The old search is kept as
  `analyzers::collision_test_hooks`, behind a `test-hooks` feature that only
  `solidcraft-kernel`'s tests turn on, and `crates/kernel/tests/mesh_collision_equivalence.rs`
  compares the two.
- `tessellation/triangulation.rs`: the inside test of a face's boundary also skips runs of 32
  segments whose box, grown by the tolerance, the test's ray cannot meet. Same answers. The
  old test is kept as `tessellation::include_test_hooks`, behind a `test-hooks` feature that
  only `solidcraft-kernel`'s tests turn on, and `crates/kernel/tests/tess_include_equivalence.rs`
  compares the two.
- `tessellation/triangulation.rs`: the constrained Delaunay triangulation of a trimmed face
  works in (u, v) scaled by the surface's mean speed along each (a power of two, so exact both
  ways). In raw parameters, a loft side ruled across (about 30 mm) and long along the loft
  (about 600 mm) is a strip stretched twentyfold, and a trimming node was fanned to boundary
  nodes hundreds of millimetres away: a flat sheet through a joined tube.
- `tessellation/triangulation.rs`: every segment of a face's boundary becomes a constraint; one
  that crosses or overlaps a constraint already added (an intersection edge's polyline can hook
  back a few micrometres past its vertex) is split where they meet. truck dropped such a
  segment and joined its start to the first later node it could reach, which on a joined tube
  left a trimming curve's 130 nodes behind one constraint across the face: slivers from it
  sagged millimetres off the surface, and a face pinched to a point (#1648) got no triangles.
  All meshes; faces whose segments all go in mesh as before.
- `tessellation/mod.rs`: `Deflection::rulings` is now `Deflection::display`: a mesh to display
  or export (the kernel's `tessellate` and display meshes), which gets the ruling grid lines,
  and the speed-scaled triangulation above. The meshes a Boolean operation intersects
  (`Deflection::chord`) keep truck's (u, v) and insertion order.
- `lib.rs`: compiler warnings no longer fail the build. Example, test and bench targets are
  dropped (their sources are not vendored).
- `tessellation/triangulation.rs`: adaptive division also splits a cell that bows off its
  corners' plane by more than the chord tolerance (a quarter of its twist `p00 - p01 - p10 +
  p11`, across that plane, at the middle). A bilinear patch equals the blend of its corners the
  division compared against, so a twisted ruled face (a loft between polylines) went out as two
  triangles per face (#1666); a flat cell such as a cone's trapezoid between rulings is not split.
- `tessellation/triangulation.rs`: a shell's face meshes share their edges' points exactly. A
  face's mesh puts each point at its parameters on the face's own surface, so a point of an edge
  that lies off a surface within tolerance (an intersection-curve sample whose projection did not
  converge, an imported edge) came out in a different place on each of the edge's faces, and the
  exported mesh had a crack there. In display and export meshes (`Deflection::display`), a
  vertex at a point of a face's boundary loops that is farther than `TOLERANCE` from the nearest
  point of the face's edges now goes to that point, when it is within the chord tolerance
  (`onto_edges`). Points within `TOLERANCE` stay on the surface, so exact geometry meshes as
  before. The meshes Boolean operations intersect keep truck's points.
- `tessellation/triangulation.rs`: in display and export meshes (`Deflection::display`), a
  direction left undivided along the rulings of a face ruled one way also gets a grid line
  through each point of the face's boundary that lies inside the face along it
  (`with_boundary_lines`), when there are at most 16 such points: corners standing into the
  face, not a boundary curve running through it, which the triangulation already follows. A hole in a cone or cylinder smaller than one cell (a notch into a
  chamfer) or a trim crossing the rulings of a narrow band (an oblique plane through a bore's
  chamfer) had no grid point beside it, and the triangulation fanned from it to the far ends of
  the face, many degrees round: those triangles cut through the surface and folded over each
  other (a cone face with a small hole meshed several mm² too large, and the boolean that made
  it was refused for its volume).
- `tessellation/triangulation.rs`: a face that comes out of the triangulation with no triangle
  at all, though the points of its one boundary loop enclose an area in space, is given the
  triangles of that loop (ears clipped on the plane of the loop, no point added, so the faces on
  either side of its edges still meet it). Its two edges can be different curves that, sampled
  as they are, run along the same line in the face's parameters (a lens thinner than the
  tolerance), and the loop then has no area there. The shell's mesh had a hole. Faces that
  mesh are untouched, and so are preview meshes (they have their own step grid). Only a face
  with one loop of at most 256 points is filled, and not one whose loop crosses itself, whose
  ears would overlap (the pinched face of #1648 is one): it stays as it was.
  `crates/kernel/src/thin_face_tests.rs` holds it, `crates/kernel/tests/tess_loop_fill.rs` the
  ear clipping (through the `test-hooks` feature).

## matext4cgmath 0.1.0 (Apache-2.0, © RICOS Co. Ltd., https://github.com/ricosjp/matext4cgmath)

truck-geometry uses it for a matrix's operator norm (an intersection curve scales its tolerance
by it whenever it is moved).

Changes:
- `solver.rs`: the Newton refinement of cubic and quartic roots stops after 100 steps. Near a
  double root it could cycle forever: moving a body whose edges a boolean made (the retries
  move both bodies and the result back) hung the app.
- `lib.rs`: compiler warnings no longer fail the build. Test targets are dropped (their sources
  are not vendored).

## egui-wgpu 0.36.2 (MIT OR Apache-2.0, Emil Ernerfeldt and contributors)

Original package sources from https://github.com/emilk/egui/tree/0.36.2/crates/egui-wgpu,
with both original licence texts retained. No images or bundled documentation are copied.

SolidCraft changes:
- `src/lib.rs`: use 1x MSAA on the selected OpenGL adapter before creating pipelines,
  exposing the effective sample count so host paint callbacks can match it.
- `src/winit.rs`: use that same effective count for depth and resolve attachments.
- `src/renderer.rs`: when the GPU refuses a staging buffer for a frame's meshes (device lost
  or out of memory), log a warning and draw nothing that frame instead of panicking (#1469).
- Other backends retain requested MSAA, without another adapter or device probe.
- `src/lib.rs`: clamp MSAA to the largest sample count the device supports for the surface and
  depth formats (`supported_msaa_samples`, `RenderState::msaa_supported`): a saved 8x on a
  device without it failed pipeline validation, a panic at start.
- `src/setup.rs`: the default device requests `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES` when
  the adapter has it, so 2x and 8x work where the adapter supports them (WebGPU only
  guarantees 1x and 4x).
- `src/setup.rs`: the default device asks for the adapter's largest 2D texture (at least 8192)
  instead of exactly 8192.
- `src/lib.rs`, `src/winit.rs`: `set_startup_hook` is told before each step of getting the
  device and the window's surface up (instance, surface, adapter request, the chosen adapter,
  device, renderer, surface configure, first present), so the app's crash journal can say which
  step a crash inside the graphics driver happened in.
- `src/lib.rs`, `src/winit.rs`: a window larger than the device's largest texture gets a surface
  clamped to it (`clamp_surface_extent`, aspect ratio kept) and egui is drawn at the matching
  smaller scale, instead of a `Surface::configure` validation panic (#869). The resize path and
  an unsupported surface no longer unwrap.
- `src/winit.rs`: acquire a drawable before preparing paint callbacks, so a skipped
  surface frame cannot discard initialization commands for cached viewport meshes.
- `src/lib.rs`, `src/winit.rs`: surfaces are configured under `capture_errors` (error scopes):
  a configuration the device refuses (D3D12 "Failed to wait for GPU to come idle before
  reconfiguring the Surface", #1935) is logged, the frame is skipped and the configure is tried
  again, instead of the device's uncaptured-error handler panicking. A streak of refusals warns
  once (debug after), waits 16 ms doubling up to 1 s between tries and recreates the surface
  every `SURFACE_RECREATE_AFTER` refusals (`ConfigureRetry`). Each surface keeps the deadline of
  its next try: until it is due, no configure or recreate happens, whether the repaint comes
  from input, an animation, a resize (its size is kept for the retry) or an outdated surface,
  and the frame is skipped with a repaint asked for when it is due. The streak and the deadline
  survive a recreate; a configure the device takes ends the streak, a recreate it asked for
  included. A recreate for a lost surface goes ahead as before.
- `src/lib.rs`, `src/winit.rs`: the device's lost callback marks `RenderState::device_lost`
  (`DeviceLost`), and the painter draws nothing on a lost device. Before a frame's uploads it
  submits nothing (`DeviceLost::probe`), so a device the driver lost is found by that
  submission rather than by the first queue write, which panicked inside wgpu-core
  (`PendingWrites::activate`, #4172).

## CADCraft drawing crates (`crates/cadcraft-*`)

TCI-CAD's drawing environment uses seven crates of CADCraft (https://github.com/storytold/cadcraft,
MIT OR Apache-2.0), taken at **`d864a08b013353143086bd8bb568c26cb6256796`**, and the PDF writer
they pin from craft-libs (https://github.com/storytold/craft-libs), taken at
**`9f89fe8f6d72ab4db4cc06f186381a6a254463ab`**. Re-sync crate by crate, deliberately, at a new
pinned sha; never track CADCraft `main`. Every edited vendored line is marked `TCI-CAD:` (`//` in
Rust, `#` in TOML), so a re-sync can find and re-apply them.

Crates (package names are upstream's, `cadcraft-*`; they are workspace members under `crates/`
and sit in `xtask/src/layers.rs` `TABLE` below the engine):

| Directory | Upstream at the CADCraft pin | Layer |
|---|---|---|
| `crates/cadcraft-color` | `crates/color` | L0 |
| `crates/cadcraft-geom` | `crates/geom` | L0 |
| `crates/cadcraft-dxf` | `crates/dxf` | L0 |
| `crates/cadcraft-doc` | `crates/doc` | L1 |
| `crates/cadcraft-fonts` | `crates/fonts` | L1 |
| `crates/cadcraft-render` | `crates/render` | L2 |
| `crates/cadcraft-io` | `crates/io` | L3 |

Not taken: CADCraft's `dwg` (acadrust, MPL-2.0), `engine`, `ui-egui`, `mcp`, `constraints`. Each
crate directory also holds CADCraft's root `LICENSE-MIT` and `LICENSE-APACHE` (the crates ship
none of their own).

From craft-libs (outside the workspace, `exclude = ["vendor"]`):

| Directory | Upstream at the craft-libs pin | Wired by |
|---|---|---|
| `vendor/craft-pdf` | `crates/pdf` (craft-pdf 0.1.0), plus the repository's `LICENSE-MIT`/`LICENSE-APACHE` | path dependency of `cadcraft-io` (`write`; `read` for its tests) |
| `vendor/krilla` | `vendor/krilla` (krilla 0.8.2 with craft-libs' patches, `README.craft.md`), plus `LICENSE_MIT`/`LICENSE_APACHE` from craft-libs `licenses/pdf-writer` | root `[patch.crates-io]` |
| `vendor/hayro-syntax` | `vendor/hayro-syntax` (0.7.2) | root `[patch.crates-io]` |
| `vendor/hayro-jbig2` | `vendor/hayro-jbig2` (0.3.0) | root `[patch.crates-io]` |

The three patches are CADCraft's root `[patch.crates-io]` entries at its pin, by path instead of
craft-libs git (`deny.toml` forbids unknown git sources).

Changes:
- Manifests de-workspaced: `version = "0.13.0"`, `edition = "2024"`, `license = "MIT OR
  Apache-2.0"` and every `{ workspace = true }` dependency written out from CADCraft's root
  `Cargo.toml` at the pin; the seven crates depend on each other by path; `[lints] workspace = true`
  dropped. `vendor/craft-pdf/Cargo.toml` likewise from craft-libs' root `Cargo.toml` (version,
  edition, licence, repository, rust-version and its six optional dependencies).
- `cadcraft-fonts`: unchanged; its `build.rs` still reads `CRAFT_FONTS_DIR`, which stays unset (no
  OFL fonts embedded).
- `cadcraft-io` DWG stub: the `cadcraft-dwg` dependency and the `acadrust` dev-dependency
  (MPL-2.0) are removed. `src/lib.rs`: `read` refuses bytes that start with the DWG version prefix
  `AC1` (was `cadcraft_dwg::is_dwg` and the conversion), and `write_with` to `.dwg` (so `write` to
  `.dwg` and `.dwt`) returns the same error (was the bridge), both
  `IoError::Format("DWG is not supported in TCI-CAD (PDF and DXF only)")` (`DWG_UNSUPPORTED`).
  `src/dxf_write.rs`: `dwg_bridge`, now uncalled, carries `#[allow(dead_code)]`.
  `tests/tci_dwg_stub.rs` (ours) holds the stub to that text.
- DWG tests removed (they need the bridge); each removal leaves a `TCI-CAD:` line where it was:
  - files: `tests/dwg_active_layout.rs`, `tests/dwg_polyline_vertex_order.rs`,
    `tests/dwg_required_dictionaries.rs`, `tests/it/dwg_header_vars.rs`,
    `tests/it/dwg_layout_owner.rs`, `tests/it/dwg_roundtrip_losses.rs`,
    `tests/it/groups_roundtrip.rs` (and their `mod` lines in `tests/it/main.rs`);
  - `src/tests.rs`: `viewport_frozen_layers_survive_dwg`, `many_entities`, `ac1024_dwg`,
    `ac1024_dwg_with_many_entities_opens`, `damaged_dwg_opens_partially_and_says_what_it_left_out`,
    `dwg_roundtrip_keeps_non_ascii_text`, `dwg_roundtrip_keeps_extension_data`,
    `gradient_hatch_roundtrips_through_dwg`; the DWG legs of
    `generated_arrowhead_blocks_are_not_kept_as_user_blocks`,
    `file_arrowhead_blocks_with_standard_names_are_kept`, `oversized_dwg_and_dxf_are_rejected_early`
    (its DXF case stays), `layer_states_roundtrip_next_to_constraints` and
    `arc_length_dimension_keeps_its_kind_and_centre_on_roundtrip`;
  - `tests/it/acad_interop.rs`: `dwg_roundtrip`, `a_rotated_linear_dimension_stores_its_projected_length`,
    `stored_measurement`, `an_angular_dimension_stores_its_angle_in_radians`, `polar`, and the DWG
    legs of `justified_text_is_written_with_its_baseline_start_as_insertion_point` and
    `the_current_dimension_style_and_its_variables_reach_autocad` (its `use` line trimmed);
  - one test each: `acis_data_store::dwg_keeps_the_solids_geometry`,
    `codepages::pre_2007_dwg_text_decodes_through_the_bridge`,
    `dim_block_resave_roundtrip::dwg_reopen_and_save_keeps_one_block_per_dimension`,
    `dim_block_text::dwg_angular_block_text_keeps_the_decimal_separator`,
    `dwt_template::dwt_is_written_as_dwg_and_read_back`,
    `dxf_versions::{dwg_at_every_version_reads_back, dwg_default_is_at_least_2004}` (and the helper
    `first_table`), `followups_626::multileader_in_dwg_follows_the_dwg_version`,
    `helix_roundtrip::helix_survives_dwg_save_and_reopen`, `mleader_roundtrip::multileader_through_dwg`,
    `mline_roundtrip::mline_survives_dwg_save_and_reopen`,
    `model_layout_plot_type::model_layout_plot_type_survives_the_dwg_bridge`,
    `passthrough_image_roundtrip::image_survives_dwg_save_and_reopen`,
    `roundtrip_audit::dwg_keeps_extension_dictionaries_xrecords_and_entity_xdata`;
  - DWG legs only: `attrib_modes::modes_survive_dxf_and_dwg`,
    `dim_block_resave_roundtrip::an_unedited_dimension_keeps_the_block_it_was_read_with`,
    `dynblock_read::a_saved_drawing_keeps_the_dynamic_block_as_read`,
    `hatch_assoc_roundtrip::a_hatch_in_a_block_keeps_its_boundary_and_the_boundary_its_reactor`,
    `low_handseed::faces_made_while_reading_do_not_take_the_files_handles`, and
    `tests/dimassoc_angular_radial.rs::standard_dimassoc_associates_in_dxf_and_dwg` (its `use` line
    trimmed).
- Crate-root clippy allows (`cargo clippy --workspace --all-targets -- -D warnings`; CADCraft's own
  workspace lints allowed `type_complexity`, which ours do not), each marked `TCI-CAD:`:
  - `#![allow(clippy::type_complexity)]` in `crates/cadcraft-doc/src/lib.rs`,
    `crates/cadcraft-render/src/lib.rs`, `crates/cadcraft-io/src/lib.rs` and
    `crates/cadcraft-io/tests/it/main.rs`;
  - `#![allow(clippy::single_element_loop)]` in `crates/cadcraft-io/src/lib.rs` (for `src/tests.rs`)
    and `crates/cadcraft-io/tests/it/main.rs`: the loops that kept only their DXF leg.
