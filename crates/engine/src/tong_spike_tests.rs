//! The casing-tong solver spike (assembly-constraints plan, Task 0; spec §8.1): does today's
//! joint/loop solver carry Inventor-density constraints, compiled by hand to joints, on a
//! tong-sized assembly?
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Instant;

use serde_json::{Value, json};
use solidcraft_doc::joints::{self, Analysis, Held};
use solidcraft_geom::Vec3;

use crate::Session;
use crate::test_support::tong;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// Every occurrence's degrees of freedom that differ from the hand count: (occurrence, solved, hand).
fn dof_misses(sol: &joints::Solution, hand: &std::collections::BTreeMap<u64, usize>) -> Vec<(u64, Option<usize>, usize)> {
    hand.iter().filter(|(o, d)| sol.dof.get(o) != Some(d)).map(|(o, d)| (*o, sol.dof.get(o).copied(), *d)).collect()
}

#[test]
fn the_tong_is_what_the_hand_count_says() {
    let t = tong(9, false);
    let doc = &t.session.doc;
    assert_eq!(doc.occurrences.len(), 154);
    assert_eq!(t.joints, 414);
    assert_eq!(doc.assembly.joints.len(), 414);
    let grounded: Vec<u64> = doc.occurrences.iter().filter(|o| o.grounded).map(|o| o.id).collect();
    assert_eq!(grounded, vec![t.body]);
    assert!(t.session.model.results.iter().all(|r| r.error.is_none()), "{:?}", t.session.model.results.iter().find(|r| r.error.is_some()));
    assert_eq!(t.hand_dof.len(), 154);
    let sol = joints::solve(doc, &t.session.model.state());
    assert_eq!(sol.conflicts, vec![]);
    assert_eq!(sol.errors, vec![]);
    assert_eq!(dof_misses(&sol, &t.hand_dof), vec![]);
    // The four-bar's one motion moves every member; the pins at A and D stay with the body.
    for m in &t.modules {
        let dof = |o: u64| sol.dof.get(&o).copied();
        assert_eq!([m.jaw, m.link, m.lever, m.pins[1], m.pins[2]].map(dof), [Some(1); 5]);
        assert_eq!([m.pins[0], m.pins[3]].map(dof), [Some(0); 2]);
    }
}

/// The records build (Task 9): one module as 46 constraint records, every one met, the hand
/// count's DOF; and `constraint.create` with `predict` makes the same records on the built
/// pose, moving nothing (the builder makes them with the command's own `build`, solving once).
#[test]
fn the_record_tong_is_what_the_hand_count_says() {
    let t = tong(1, true);
    let doc = &t.session.doc;
    assert_eq!((doc.occurrences.len(), t.joints, t.records.len()), (18, 46, 46));
    assert_eq!((doc.assembly.joints.len(), doc.assembly.constraints.len()), (0, 46));
    let kinds = |k: &str| doc.assembly.constraints.iter().filter(|c| c.name.starts_with(k)).count();
    // Rigid: 4 pins + 6 fasteners locked Inserts, 2 dies XZ Flush; Cylindrical: 10 axis Mates;
    // Planar: 8 + 6 + 6 + 4 Flushes.
    assert_eq!((kinds("Insert:"), kinds("Mate:")), (10, 36));
    assert!(doc.assembly.constraints.iter().filter(|c| c.name.starts_with("Insert:")).all(|c| c.lock));
    let sol = joints::solve(doc, &t.session.model.state());
    assert_eq!(sol.conflicts, vec![]);
    assert_eq!(sol.errors, vec![]);
    assert_eq!(dof_misses(&sol, &t.hand_dof), vec![]);
    let m = &t.modules[0];
    assert_eq!(m.parts.len(), 17);
    let dof = |o: u64| sol.dof.get(&o).copied();
    assert_eq!([m.jaw, m.link, m.lever, m.pins[1], m.pins[2]].map(dof), [Some(1); 5]);
    assert_eq!([m.pins[0], m.pins[3]].map(dof), [Some(0); 2]);

    // By command, on the same parts (the joints build's, its joints taken out): the same records.
    // The evidence covers each record's id, name, kind, solution, picks, offset and lock, on
    // tong(1) only (tong(9)'s records are built with `build`, over the command cap); limits,
    // expressions, suppression, the Directed axis and the saved state are not compared.
    let mut parts = (*tong(1, false).session.doc).clone();
    parts.assembly.joints.clear();
    let mut s = Session::new(parts);
    let before: Vec<_> = s.doc.occurrences.iter().map(|o| joints::occurrence_world(&s.doc, o.id)).collect();
    for p in &t.records {
        let r = run(&mut s, "constraint.create", p.clone());
        assert_eq!(r["status"], "ok", "{p} -> {r}");
    }
    let key = |c: &solidcraft_doc::constraints::Constraint| (c.id, c.name.clone(), c.kind, c.solution, c.picks.clone(), c.offset, c.lock);
    let by_command: Vec<_> = s.doc.assembly.constraints.iter().map(key).collect();
    let built: Vec<_> = doc.assembly.constraints.iter().map(key).collect();
    assert_eq!(by_command, built);
    for (o, was) in s.doc.occurrences.iter().zip(&before) {
        let now = joints::occurrence_world(&s.doc, o.id);
        for q in [Vec3::ZERO, Vec3::X * 100.0, Vec3::Y * 100.0] {
            let d = solidcraft_doc::apply_point(&now, q).dist(solidcraft_doc::apply_point(was, q));
            assert!(d <= 1e-6, "{} moved {d} mm", o.name);
        }
    }
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// The 95th percentile (nearest rank).
fn p95(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    v[(v.len() * 95).div_ceil(100) - 1]
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// The spike's build command (R127, R130; Task 9 records it): the solver crates optimised as a
/// release build optimises them (the plain `ci` profile builds them at opt-level 1: rows 1-3 run
/// about ten times slower), and the engine split into 256 codegen units so its test build fits
/// the 10-minute window (`--release` gives the same optimisation but its test build takes longer).
const SPIKE: &str = "cargo test -p solidcraft-engine --profile ci --config profile.ci.package.solidcraft-doc.opt-level=3 --config profile.ci.package.solidcraft-geom.opt-level=3 --config profile.ci.package.solidcraft-sketch.opt-level=3 --config profile.ci.package.solidcraft-engine.codegen-units=256 --lib -- tong_spike_tests::spike_meets_spec_8_1 --ignored --nocapture";

/// The §8.1 pass criteria on both builds of the tong: the hand joints (Task 0) and the same
/// mechanism as 414 constraint records (Task 9), every row of each collected and printed before
/// any is asserted (R109).
///
/// Timings are wall-clock: on a hybrid CPU (performance and efficiency cores) a test process in
/// the background can be moved to the slow cores mid-run (every phase of a frame 3-5x slower at
/// once), so the spike pins itself to the performance cores first ([`pin_to_performance_cores`]),
/// as the app in the foreground runs. Run from one line: [`SPIKE`].
#[test]
#[ignore = "spike: cargo test -p solidcraft-engine --profile ci --config profile.ci.package.solidcraft-doc.opt-level=3 --config profile.ci.package.solidcraft-geom.opt-level=3 --config profile.ci.package.solidcraft-sketch.opt-level=3 --config profile.ci.package.solidcraft-engine.codegen-units=256 --lib -- tong_spike_tests::spike_meets_spec_8_1 --ignored --nocapture"]
fn spike_meets_spec_8_1() {
    println!("{}", pin_to_performance_cores());
    println!("command: {SPIKE}");
    let mut failed = Vec::new();
    for compiled in [false, true] {
        let build = if compiled { "RECORDS" } else { "JOINTS" };
        for (row, got, bound, pass) in spike(compiled) {
            println!("{build} ROW {row}: {got} | bound {bound} | {}", if pass { "PASS" } else { "FAIL" });
            if !pass {
                failed.push(format!("{build} {row}"));
            }
        }
    }
    assert!(failed.is_empty(), "failed rows: {failed:?}");
}

/// The held turn of a records build's crank: its axis Mate's Cylindrical, value 0 (the angle).
fn crank_turn(crank: u64) -> (u64, usize) {
    (solidcraft_doc::constraints::compiled_id(crank, 0), 0)
}

type Row = (String, String, String, bool);

/// The seven rows on one build of tong(9).
fn spike(compiled: bool) -> Vec<Row> {
    let built = Instant::now();
    let t = tong(9, compiled);
    let what = if compiled { "constraint records" } else { "joints" };
    println!("tong(9): {} occurrences, {} {what}, built in {:.0} ms", t.session.doc.occurrences.len(), t.joints, ms(built));
    let dir = std::env::temp_dir().join(format!("tcicad-con-{}-spike_meets_spec_8_1", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tong.tciasm");
    let mut saved = t.session;
    run(&mut saved, "file.save", json!({"path": path.to_string_lossy()}));
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": path.to_string_lossy()}));
    let _ = std::fs::remove_dir_all(&dir);
    if compiled {
        assert!(s.doc.assembly.joints.is_empty() && s.doc.assembly.constraints.len() == t.joints, "the records build saves records only");
    }
    let mut rows: Vec<Row> = Vec::new();
    let mut conflicts = 0usize;

    // 1. Full solve from file open (placement + DOF, no memo), each with an empty pick cache
    // (R156: what the first solve of a session pays; later ones read the session's cache).
    let st = s.model.state();
    let mut times = Vec::new();
    let mut sol = joints::Solution::default();
    for _ in 0..5 {
        let cache = solidcraft_doc::constraints::PickCache::default();
        let at = Instant::now();
        sol = joints::solve_cached(&s.doc, &st, &Held::new(), Analysis::DegreesOfFreedom, None, &cache);
        times.push(ms(at));
    }
    conflicts += sol.conflicts.len();
    let full = median(&mut times);
    rows.push(("1 full solve (median of 5)".into(), format!("{full:.1} ms {times:.1?}"), "<= 500 ms (§8.1)".into(), full <= 500.0));
    // The records build answers to R156's target too: the joints build's ×1.5 (180 ms).
    if compiled {
        rows.push(("1 full solve, R156 target".into(), format!("{full:.1} ms"), "<= 180 ms (R156)".into(), full <= 180.0));
    }
    let opened = (*s.doc).clone();
    let at = Instant::now();
    let placed = joints::solve_holding_with_analysis(&s.doc, &st, &Held::new(), Analysis::Placement);
    println!("placement-only solve: {:.1} ms ({} conflicts)", ms(at), placed.conflicts.len());
    if compiled {
        compile_cost(&s);
    }

    // 2. Re-solve after one offset edit (R131): COLD is the open's own DOF solve, which warms the
    // session's memo (recorded, not decisive); WARM are the edits after it, each <= 50 ms (R130).
    assert!(s.dof_memo.lock().is_ok_and(|m| !m.is_empty()), "doc.open warms the DOF memo");
    *s.dof_memo.lock().unwrap() = Default::default();
    let at = Instant::now();
    crate::cmd::joints::warm_dof(&s);
    let cold = ms(at);
    let carrier_z = t.modules[0].carrier_z;
    // The carrier's +Z offset: a joint's, or its Flush record's (measured when it was made).
    let set_offset = |s: &mut Session, v: Option<f64>| -> f64 {
        let doc = s.doc_mut();
        let slot = if compiled {
            &mut doc.assembly.constraints.iter_mut().find(|c| c.id == carrier_z).unwrap().offset
        } else {
            &mut doc.assembly.joints.iter_mut().find(|j| j.id == carrier_z).unwrap().offset
        };
        let was = *slot;
        if let Some(v) = v {
            *slot = v;
        }
        was
    };
    let home = set_offset(&mut s, None);
    let mut times = Vec::new();
    for k in 0..5 {
        set_offset(&mut s, Some(if k % 2 == 0 { home + 0.5 } else { home }));
        let at = Instant::now();
        let r = crate::cmd::joints::resolve(&mut s);
        times.push(ms(at));
        conflicts += r.conflicts.len();
    }
    set_offset(&mut s, Some(home));
    conflicts += crate::cmd::joints::resolve(&mut s).conflicts.len();
    let warm = times.iter().copied().fold(0.0, f64::max);
    let in_order = format!("{times:.1?}");
    let resolve = median(&mut times);
    rows.push((
        "2 re-solve after an offset edit".into(),
        format!("COLD {cold:.1} ms (the open's DOF solve, recorded) | WARM median {resolve:.1} ms, max {warm:.1} ms {in_order}"),
        "every WARM <= 50 ms (R130, R131)".into(),
        warm <= 50.0,
    ));

    // 3. Drag frames: module 4's jaw tip B turned about A from 90° to 100° in 60 steps, by
    // `joint.reach` on both builds (R157: it chains through the joints constraints compile to).
    let jaw = t.modules[4].jaw;
    let a = Vec3::new(0.0, 0.0, 160.0);
    let tip_deg = |s: &Session| {
        let tip = solidcraft_doc::apply_point(&joints::occurrence_world(&s.doc, jaw), Vec3::new(20.0, 0.0, 0.0)) - a;
        tip.y.atan2(tip.x).to_degrees()
    };
    let mut frames = Vec::new();
    let mut last = json!(null);
    let mut reached = 0usize;
    for k in 1..=60 {
        let tip = solidcraft_doc::apply_point(&joints::occurrence_world(&s.doc, jaw), Vec3::new(20.0, 0.0, 0.0));
        let deg = (90.0 + 10.0 * k as f64 / 60.0).to_radians();
        let to = a + Vec3::new(20.0 * deg.cos(), 20.0 * deg.sin(), 0.0);
        let p = json!({"occurrence": jaw, "at": [tip.x, tip.y, tip.z], "point": [to.x, to.y, to.z], "continuous": true, "quiet": true});
        let at = Instant::now();
        last = run(&mut s, "joint.reach", p);
        frames.push(ms(at));
        reached += usize::from(last["reached"] == json!(true));
        conflicts += last["conflicts"].as_array().map_or(0, Vec::len);
    }
    if reached < 60 {
        if let Some(o) = last.as_object_mut() {
            o.remove("values");
        }
        println!("drag: {reached}/60 reached; last reach {last}");
    }
    let (med, p) = (median(&mut frames.clone()), p95(&mut frames.clone()));
    let end = tip_deg(&s);
    rows.push((
        "3 drag frame (60 joint.reach)".into(),
        format!(
            "median {med:.1} ms, p95 {p:.1} ms, max {:.1} ms; {reached}/60 frames reached, tip ends at {end:.4}°",
            frames.iter().fold(0.0f64, |m, v| m.max(*v))
        ),
        "median <= 33 ms, p95 <= 50 ms, every frame reached".into(),
        med <= 33.0 && p <= 50.0 && reached == 60,
    ));

    // 4. Placement error at every closing joint (the solver's conflict test: 1e-6 mm, 1e-8 rad).
    rows.push((
        "4 placement error (conflicts after 1-3)".into(),
        format!("{conflicts} conflicts"),
        "0 (<= 1e-6 mm, 1e-8 rad)".into(),
        conflicts == 0,
    ));

    // 5. Recovery from poses perturbed by up to 5 mm / 5°, every crank held (a joint's values, or
    // a record's state: its solved values; a locked Insert's turn is left as it is locked).
    let held: Held = if compiled {
        t.modules.iter().map(|m| (crank_turn(m.crank), opened.assembly.constraints.iter().find(|c| c.id == m.crank).unwrap().state[0])).collect()
    } else {
        t.modules.iter().map(|m| ((m.crank, 0), 0.0)).collect()
    };
    let cranks: std::collections::BTreeSet<u64> = t.modules.iter().map(|m| m.crank).collect();
    let base = joints::solve_holding(&opened, &st, &held);
    let mut moved = opened.clone();
    let nudge = |j: usize, i: usize, angle: bool| {
        let d = (((7 * j + 3 * i) % 11) as f64 / 10.0 - 0.5) * 10.0;
        if angle { d.to_radians() } else { d }
    };
    for (j, joint) in moved.assembly.joints.iter_mut().enumerate().filter(|(_, x)| !cranks.contains(&x.id)) {
        let kind = joint.kind;
        for (i, v) in joint.values.iter_mut().enumerate() {
            *v += nudge(j, i, kind.is_angle(i));
        }
    }
    for (j, c) in moved.assembly.constraints.iter_mut().enumerate().filter(|(_, x)| !cranks.contains(&x.id)) {
        // One member each on the tong (no equations): its signature says what each value is.
        let Some(sig) = c.signatures.first().copied() else { continue };
        if sig.kind == joints::JointKind::Rigid {
            continue;
        }
        for (i, v) in c.state.iter_mut().enumerate() {
            *v += nudge(j, i, sig.kind.is_angle(i));
        }
    }
    let at = Instant::now();
    let back = joints::solve_holding(&moved, &st, &held);
    let recover_ms = ms(at);
    // The whole placement, turn included: the occurrence's origin and points 100 mm along its
    // own x and y.
    let probes = [Vec3::ZERO, Vec3::X * 100.0, Vec3::Y * 100.0];
    let mut worst = (0.0f64, 0u64);
    for (o, m) in &base.world {
        let d = back.world.get(o).map_or(f64::INFINITY, |n| {
            probes.iter().map(|p| solidcraft_doc::apply_point(n, *p).dist(solidcraft_doc::apply_point(m, *p))).fold(0.0, f64::max)
        });
        if d > worst.0 || d.is_nan() {
            worst = (d, *o);
        }
    }
    rows.push((
        "5 recovery from 5 mm / 5° (cranks held)".into(),
        format!(
            "worst {:.3e} mm (occurrence {}), {} occurrences, {} conflicts, {recover_ms:.0} ms",
            worst.0,
            worst.1,
            base.world.len(),
            back.conflicts.len()
        ),
        "<= 1e-4 mm (origin and points 100 mm along x, y), 0 conflicts".into(),
        worst.0 <= 1e-4 && back.world.len() == base.world.len() && back.conflicts.is_empty(),
    ));

    // 6. Groups checked, not solved (over a cap).
    let capped = sol.groups.iter().filter(|g| g.capped).count();
    let largest = sol.groups.iter().map(|g| g.values).max().unwrap_or(0);
    let most = sol.groups.iter().map(|g| g.members.len()).max().unwrap_or(0);
    rows.push((
        "6 capped groups".into(),
        format!("{capped} of {} groups; largest {largest} values, most {most} closing joints", sol.groups.len()),
        "0".into(),
        capped == 0,
    ));

    // 7. DOF per occurrence against the hand count.
    let misses = dof_misses(&sol, &t.hand_dof);
    rows.push((
        "7 DOF = hand count".into(),
        format!("{} of {} differ {:?}", misses.len(), t.hand_dof.len(), misses.iter().take(5).collect::<Vec<_>>()),
        "0 differ".into(),
        misses.is_empty(),
    ));

    if compiled {
        command_costs(&mut s, t.modules[0].carrier_z, &t.records[0]);
    }
    rows
}

/// What the commands cost on the 414-record design (recorded, not rows): `constraint.edit` of
/// the carrier's offset (its over-constraint check, solve and refresh around the re-solve of row
/// 2), and `constraint.create` of one more record (module 0's first row again, redundant and met).
fn command_costs(s: &mut Session, carrier_z: u64, record: &Value) {
    let home = s.doc.assembly.constraints.iter().find(|c| c.id == carrier_z).unwrap().offset;
    let mut times = Vec::new();
    for k in 0..2 {
        let at = Instant::now();
        run(s, "constraint.edit", json!({"constraint": carrier_z, "offset": if k % 2 == 0 { home + 0.5 } else { home }}));
        times.push(ms(at));
    }
    println!("constraint.edit (offset) on {} records: {times:.1?} ms", s.doc.assembly.constraints.len());
    let at = Instant::now();
    let r = run(s, "constraint.create", record.clone());
    println!("constraint.create (predict) on {} records: {:.1} ms, status {}", s.doc.assembly.constraints.len() - 1, ms(at), r["status"]);
    run(s, "constraint.delete", json!({"constraint": r["id"]}));
}

/// Where a solve of the records build spends its time: `compile` of the 414 records with no
/// cache (every pick found by name, indexed), then twice through one pick cache (R156: cold,
/// then warm), and the session's own (warmed by the open); the world resolution of their picks
/// by kind, uncached (recorded, not rows).
fn compile_cost(s: &Session) {
    use solidcraft_doc::constraints::{Geo, PickCache, compile, compile_with, resolve_world};
    let st = s.model.state();
    let at = Instant::now();
    let compiled = compile(&s.doc, &st, &s.doc.assembly);
    println!(
        "compile (no cache): {:.1} ms ({} joints, {} equations, {} lost)",
        ms(at),
        compiled.joints.len(),
        compiled.equations.len(),
        compiled.lost.len()
    );
    let cache = PickCache::default();
    for pass in ["cold", "warm"] {
        let at = Instant::now();
        let c = compile_with(&cache.picks(&s.doc, &st), &s.doc.assembly);
        println!(
            "compile through a pick cache ({pass}): {:.2} ms ({} joints, {} lost; {} entries, {} misses)",
            ms(at),
            c.joints.len(),
            c.lost.len(),
            cache.len(),
            cache.misses()
        );
    }
    let at = Instant::now();
    compile_with(&s.pick_cache.picks(&s.doc, &st), &s.doc.assembly);
    println!("compile through the session's cache: {:.2} ms", ms(at));
    let mut by_kind: std::collections::BTreeMap<&str, (usize, f64)> = Default::default();
    for g in s.doc.assembly.constraints.iter().flat_map(|c| &c.picks) {
        let kind = match g.geo {
            Geo::Face { .. } => "face",
            Geo::Edge { .. } => "edge",
            Geo::Vertex { .. } => "vertex",
            Geo::Origin { .. } => "origin",
        };
        let at = Instant::now();
        resolve_world(&s.doc, &st, g).unwrap();
        let e = by_kind.entry(kind).or_default();
        *e = (e.0 + 1, e.1 + ms(at));
    }
    for (kind, (n, total)) in by_kind {
        println!("resolve_world (uncached) {kind}: {n} picks in {total:.1} ms ({:.3} ms each)", total / n as f64);
    }
}

/// R155: what `stop_on_contact` costs a drive on the tong's records build. Module 0's carrier
/// lifted 10 mm off its jaw in 100 frames (its +Z Flush driven) with a contact set of the
/// module's 17 parts, then 10 frames without contact; ms per frame of each (recorded).
#[test]
#[ignore = "spike (R155), as spike_meets_spec_8_1: cargo test -p solidcraft-engine --profile ci --config profile.ci.package.solidcraft-doc.opt-level=3 --config profile.ci.package.solidcraft-geom.opt-level=3 --config profile.ci.package.solidcraft-sketch.opt-level=3 --config profile.ci.package.solidcraft-engine.codegen-units=256 --lib -- tong_spike_tests::spike_drive --ignored --nocapture"]
fn spike_drive_stops_on_contact_r155() {
    println!("{}", pin_to_performance_cores());
    let mut t = tong(9, true);
    let s = &mut t.session;
    let m = &t.modules[0];
    run(s, "contact.create", json!({"occurrences": m.parts, "name": "Module 0"}));
    let home = s.doc.assembly.constraints.iter().find(|c| c.id == m.carrier_z).unwrap().offset;
    // R158: what the drive's contact check costs on its own: made once (the meshes), the first
    // test of every pair, then again with nothing moved (every pair's answer kept).
    let at = Instant::now();
    let check = crate::cmd::motion::ContactCheck::new(s);
    let made = ms(at);
    let at = Instant::now();
    let again = check.hit(s);
    println!(
        "R158 contact check (cached geometry: {}): made with its start test in {made:.1} ms ({} member pairs pass through each other at the start); a check with nothing moved {:.2} ms ({again:?})",
        check.cached(),
        check.at_start().len(),
        ms(at)
    );
    // Up 10 mm off the jaw (no stop), down into it (a contact stop at the first frame, then the
    // bisection to 1e-4 mm), and up without contact.
    for (contact, end, steps) in [(true, 10.0, 99), (true, -10.0, 99), (false, 10.0, 9)] {
        let p = json!({"constraint": m.carrier_z, "start": home, "end": home + end, "steps": steps, "stop_on_contact": contact});
        let at = Instant::now();
        let r = run(s, "constraint.drive", p);
        let total = ms(at);
        let frames = r["frames"].as_array().map_or(0, Vec::len);
        let stops: Vec<&str> =
            ["stopped_by_contact", "stopped_by_limit", "stopped_by_loop"].into_iter().filter(|k| r.get(*k).is_some_and(|v| !v.is_null())).collect();
        println!(
            "R155 drive {end:+} mm (stop_on_contact {contact}, contact set of {} parts): {frames} frames in {total:.0} ms = {:.1} ms/frame; stops {stops:?} {}",
            m.parts.len(),
            total / frames.max(1) as f64,
            r.get("stopped_by_contact").map_or(String::new(), |v| v.to_string())
        );
    }
    // A stop: module 0's jaw die (moving with its carrier) against module 1's jaw 33 mm above it,
    // a second contact set; up and down 40 mm (the stop, then the bisection to 1e-4 mm).
    let die = s.doc.find_component("Die 0.0").and_then(|c| s.doc.occurrence_of(c)).unwrap().id;
    run(s, "contact.create", json!({"occurrences": [die, t.modules[1].jaw], "name": "Die on the next jaw"}));
    for end in [40.0, -40.0] {
        let p = json!({"constraint": m.carrier_z, "start": home, "end": home + end, "steps": 99, "stop_on_contact": true});
        let at = Instant::now();
        let r = run(s, "constraint.drive", p);
        let total = ms(at);
        let frames = r["frames"].as_array().map_or(0, Vec::len);
        println!(
            "R158 drive {end:+} mm into the next jaw: {frames} frames in {total:.0} ms = {:.1} ms/frame; stopped_by_contact {}",
            total / frames.max(1) as f64,
            r["stopped_by_contact"]
        );
    }
}

/// Pin this process to the performance cores at high priority (Windows; elsewhere nothing), and
/// say which mask it used. On a hybrid Intel CPU Windows numbers the performance cores' threads
/// first, two per core, so they are the first `2 * (logical - cores)` logical processors; a CPU
/// with no efficiency cores (logical = 2 * cores, or no hyper-threading) keeps every processor.
/// High priority too: at normal priority a background process pinned to those cores still ran
/// every row about 3.5x slower (Windows power throttling). `unsafe` is forbidden in the
/// workspace, so both are set through PowerShell on this process id.
fn pin_to_performance_cores() -> String {
    if !cfg!(windows) {
        return "affinity: unchanged (not Windows)".into();
    }
    let script = [
        "$c = Get-CimInstance Win32_Processor",
        "$l = ($c | Measure-Object NumberOfLogicalProcessors -Sum).Sum",
        "$n = ($c | Measure-Object NumberOfCores -Sum).Sum",
        "$p = $l - $n",
        &format!("$me = Get-Process -Id {}", std::process::id()),
        "$me.PriorityClass = 'High'",
        "if ($p -le 0 -or 2 * $p -ge $l -or $l -gt 62) { 'affinity: unchanged, priority high ({0} logical, {1} cores)' -f $l, $n }          else { $m = [long][math]::Pow(2, 2 * $p) - 1; $me.ProcessorAffinity = [IntPtr]$m; \
         'affinity: 0x{0:X}, priority high (performance cores; {1} logical, {2} cores)' -f $m, $l, $n }",
    ]
    .join("; ");
    match std::process::Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", &script]).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Ok(o) => format!("affinity: unchanged (powershell: {})", String::from_utf8_lossy(&o.stderr).trim()),
        Err(e) => format!("affinity: unchanged ({e})"),
    }
}

/// R130: a session re-solve takes the loop degrees of freedom its last solve counted where an
/// occurrence's loops and values are unchanged, and counts again where an edit touched them; the
/// counts equal a fresh solve's either way.
#[test]
fn a_memo_re_solve_counts_as_a_fresh_one() {
    let mut t = tong(3, false);
    let fresh = |s: &Session| joints::solve(&s.doc, &s.model.state()).dof;
    let first = crate::cmd::joints::resolve(&mut t.session);
    assert_eq!(dof_misses(&first, &t.hand_dof), vec![]);
    // An offset edit in module 1: its loops move, the others' are taken from the memo.
    let carrier_z = t.modules[1].carrier_z;
    t.session.doc_mut().assembly.joints.iter_mut().find(|j| j.id == carrier_z).unwrap().offset = 0.5;
    let edited = crate::cmd::joints::resolve(&mut t.session);
    assert_eq!(edited.conflicts, vec![]);
    assert_eq!(edited.dof, fresh(&t.session));
    assert_eq!(dof_misses(&edited, &t.hand_dof), vec![]);
    // A joint that locks module 0's four-bar: its members' counts change and are not taken
    // from the memo.
    let crank = t.session.doc.assembly.joints.iter().find(|j| j.id == t.modules[0].crank).unwrap().clone();
    let doc = t.session.doc_mut();
    let lock = joints::Joint { id: doc.allocate_id().unwrap(), name: "Lock".into(), kind: joints::JointKind::Rigid, values: Vec::new(), ..crank };
    doc.assembly.joints.push(lock);
    let locked = crate::cmd::joints::resolve(&mut t.session);
    assert_eq!(locked.dof, fresh(&t.session));
    assert_eq!(locked.dof.get(&t.modules[0].jaw), Some(&0));
    assert_eq!(locked.dof.get(&t.modules[1].jaw), Some(&1));
}

/// The reach mitigation (spec §8.1 row 3): tong(4) has 116 closing joints, over the old
/// design-wide cap of 100, yet the jaw's chain is in few of them, so a drag frame reaches.
#[test]
fn joints_reach_drags_a_design_with_more_loops_than_the_cap() {
    let mut t = tong(4, false);
    let sol = joints::solve(&t.session.doc, &t.session.model.state());
    assert_eq!(sol.groups.iter().map(|g| g.members.len()).sum::<usize>(), 116);
    let jaw = t.modules[2].jaw;
    let a = Vec3::new(0.0, 0.0, 80.0);
    let tip = solidcraft_doc::apply_point(&joints::occurrence_world(&t.session.doc, jaw), Vec3::new(20.0, 0.0, 0.0));
    let deg = 91.0f64.to_radians();
    let to = a + Vec3::new(20.0 * deg.cos(), 20.0 * deg.sin(), 0.0);
    let r = run(
        &mut t.session,
        "joint.reach",
        json!({"occurrence": jaw, "at": [tip.x, tip.y, tip.z], "point": [to.x, to.y, to.z], "continuous": true, "quiet": true}),
    );
    assert_eq!(r["reason"], json!(null));
    assert_eq!(r["reached"], json!(true), "{r}");
    assert_eq!(r["conflicts"], json!([]));
}

/// The per-chain cap still holds: 101 redundant Rigid copies of module 0's crank close loops on
/// the jaw's chain (no values of their own), and the reach is refused, saying why.
#[test]
fn joints_reach_refuses_a_chain_in_more_than_100_loops() {
    let mut t = tong(1, false);
    let jaw = t.modules[0].jaw;
    let crank = t.session.doc.assembly.joints.iter().find(|j| j.id == t.modules[0].crank).unwrap().clone();
    let doc = t.session.doc_mut();
    for k in 0..101 {
        let mut j = crank.clone();
        j.id = doc.allocate_id().unwrap();
        j.name = format!("Extra{k}");
        j.kind = solidcraft_doc::joints::JointKind::Rigid;
        j.values = Vec::new();
        doc.assembly.joints.push(j);
    }
    let tip = solidcraft_doc::apply_point(&joints::occurrence_world(&t.session.doc, jaw), Vec3::new(20.0, 0.0, 0.0));
    let r = run(
        &mut t.session,
        "joint.reach",
        json!({"occurrence": jaw, "at": [tip.x, tip.y, tip.z], "point": [tip.x - 1.0, tip.y, tip.z], "quiet": true}),
    );
    assert_eq!(r["reason"], json!("its chain is in more than 100 closed loops"), "{r}");
    assert_eq!(r["moved"], json!(false));
}
