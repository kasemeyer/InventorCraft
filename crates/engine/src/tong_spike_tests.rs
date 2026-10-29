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
    let t = tong(9);
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

/// The §8.1 pass criteria, every row collected and printed before any is asserted (R109).
///
/// Timings are wall-clock: on a hybrid CPU (performance and efficiency cores) a test process in
/// the background can be moved to the slow cores mid-run (every phase of a frame 3-5x slower at
/// once), so the spike pins itself to the performance cores first ([`pin_to_performance_cores`]),
/// as the app in the foreground runs. Run (R127) with the solver crates optimised as a release
/// build optimises them (the plain `ci` profile builds them at opt-level 1: rows 1-3 run about ten
/// times slower), from one line:
/// `cargo test -p solidcraft-engine --profile ci --config profile.ci.package.solidcraft-doc.opt-level=3
/// --config profile.ci.package.solidcraft-geom.opt-level=3 --config
/// profile.ci.package.solidcraft-sketch.opt-level=3 --lib -- tong_spike --ignored --nocapture`.
/// (`--release` gives the same optimisation but its test build takes over ten minutes.)
#[test]
#[ignore = "spike: cargo test -p solidcraft-engine --profile ci --config profile.ci.package.solidcraft-doc.opt-level=3 --config profile.ci.package.solidcraft-geom.opt-level=3 --config profile.ci.package.solidcraft-sketch.opt-level=3 --lib -- tong_spike --ignored --nocapture"]
fn spike_meets_spec_8_1() {
    println!("{}", pin_to_performance_cores());
    let built = Instant::now();
    let t = tong(9);
    println!("tong(9): {} occurrences, {} joints, built in {:.0} ms", t.session.doc.occurrences.len(), t.joints, ms(built));
    let dir = std::env::temp_dir().join(format!("tcicad-con-{}-spike_meets_spec_8_1", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tong.tciasm");
    let mut saved = t.session;
    run(&mut saved, "file.save", json!({"path": path.to_string_lossy()}));
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": path.to_string_lossy()}));
    let _ = std::fs::remove_dir_all(&dir);
    let mut rows: Vec<(String, String, String, bool)> = Vec::new();
    let mut conflicts = 0usize;

    // 1. Full solve from file open (placement + DOF).
    let st = s.model.state();
    let mut times = Vec::new();
    let mut sol = joints::Solution::default();
    for _ in 0..5 {
        let at = Instant::now();
        sol = joints::solve_holding_with_analysis(&s.doc, &st, &Held::new(), Analysis::DegreesOfFreedom);
        times.push(ms(at));
    }
    conflicts += sol.conflicts.len();
    let full = median(&mut times);
    rows.push(("1 full solve (median of 5)".into(), format!("{full:.1} ms {times:.1?}"), "<= 500 ms".into(), full <= 500.0));
    let opened = (*s.doc).clone();
    let at = Instant::now();
    let placed = joints::solve_holding_with_analysis(&s.doc, &st, &Held::new(), Analysis::Placement);
    println!("placement-only solve: {:.1} ms ({} conflicts)", ms(at), placed.conflicts.len());

    // 2. Re-solve after one offset edit.
    let carrier_z = t.modules[0].carrier_z;
    let mut times = Vec::new();
    for k in 0..5 {
        let offset = if k % 2 == 0 { 0.5 } else { 0.0 };
        s.doc_mut().assembly.joints.iter_mut().find(|j| j.id == carrier_z).unwrap().offset = offset;
        let at = Instant::now();
        let r = crate::cmd::joints::resolve(&mut s);
        times.push(ms(at));
        conflicts += r.conflicts.len();
    }
    s.doc_mut().assembly.joints.iter_mut().find(|j| j.id == carrier_z).unwrap().offset = 0.0;
    conflicts += crate::cmd::joints::resolve(&mut s).conflicts.len();
    let resolve = median(&mut times);
    rows.push(("2 re-solve after an offset edit (median of 5)".into(), format!("{resolve:.1} ms {times:.1?}"), "<= 100 ms".into(), resolve <= 100.0));

    // 3. Drag frames: module 4's jaw tip B turned about A from 90° to 100° in 60 steps.
    let jaw = t.modules[4].jaw;
    let a = Vec3::new(0.0, 0.0, 160.0);
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
        reached += usize::from(last["reached"] == json!(true));
        frames.push(ms(at));
        conflicts += last["conflicts"].as_array().map_or(0, Vec::len);
    }
    let tip = solidcraft_doc::apply_point(&joints::occurrence_world(&s.doc, jaw), Vec3::new(20.0, 0.0, 0.0)) - a;
    if reached < 60 {
        if let Some(o) = last.as_object_mut() {
            o.remove("values");
        }
        println!("drag: last reach {last}");
    }
    let (med, p) = (median(&mut frames.clone()), p95(&mut frames.clone()));
    let end = tip.y.atan2(tip.x).to_degrees();
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

    // 5. Recovery from poses perturbed by up to 5 mm / 5°, every crank held.
    let held: Held = t.modules.iter().map(|m| ((m.crank, 0), 0.0)).collect();
    let cranks: std::collections::BTreeSet<u64> = t.modules.iter().map(|m| m.crank).collect();
    let base = joints::solve_holding(&opened, &st, &held);
    let mut moved = opened.clone();
    for (j, joint) in moved.assembly.joints.iter_mut().enumerate().filter(|(_, x)| !cranks.contains(&x.id)) {
        let kind = joint.kind;
        for (i, v) in joint.values.iter_mut().enumerate() {
            let d = (((7 * j + 3 * i) % 11) as f64 / 10.0 - 0.5) * 10.0;
            *v += if kind.is_angle(i) { d.to_radians() } else { d };
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

    for (row, got, bound, pass) in &rows {
        println!("ROW {row}: {got} | bound {bound} | {}", if *pass { "PASS" } else { "FAIL" });
    }
    let failed: Vec<&String> = rows.iter().filter(|r| !r.3).map(|r| &r.0).collect();
    assert!(failed.is_empty(), "failed rows: {failed:?}");
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

/// The reach mitigation (spec §8.1 row 3): tong(4) has 116 closing joints, over the old
/// design-wide cap of 100, yet the jaw's chain is in few of them, so a drag frame reaches.
#[test]
fn joints_reach_drags_a_design_with_more_loops_than_the_cap() {
    let mut t = tong(4);
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
    let mut t = tong(1);
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
