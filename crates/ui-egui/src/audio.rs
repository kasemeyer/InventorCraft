//! The constraint audio notification (R175): a short synthesised "wood block" pop when a
//! constraint is made (the Constrain panel's OK or Apply ran `constraint.create`), not on a
//! refusal and not on an edit, behind the "Constraint audio notification" preference (on by
//! default, as Inventor's Application Options). The sound is generated here, no asset: a ~60 ms
//! decaying sine near 1 kHz with a sharp attack. The host plays it (`Services::play_samples`:
//! the desktop app with its `audio` feature); without one (the browser, tests, CI, a machine with
//! no output device) nothing is heard. Each request is counted, so tests see it without a device.

use std::cell::Cell;

use crate::SolveApp;

/// Samples per second of the pop.
pub const RATE: u32 = 44_100;
/// How long the pop lasts, seconds.
const LENGTH_S: f32 = 0.06;
/// Its pitch, Hz, and how fast it dies away (1/s).
const PITCH_HZ: f32 = 1_000.0;
const DECAY: f32 = 70.0;
/// The attack ramp, seconds.
const ATTACK_S: f32 = 0.001;
/// Its loudness (of full scale).
const LEVEL: f32 = 0.5;

thread_local! {
    static REQUESTS: Cell<usize> = const { Cell::new(0) };
}

/// The pop's mono samples at `rate`.
pub fn pop_samples(rate: u32) -> Vec<f32> {
    let rate = rate.clamp(8_000, 192_000) as f32;
    let n = (LENGTH_S * rate) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / rate;
            let attack = (t / ATTACK_S).min(1.0);
            // A second partial an octave and a fifth up gives it the wooden knock.
            let tone = (std::f32::consts::TAU * PITCH_HZ * t).sin() + 0.35 * (std::f32::consts::TAU * PITCH_HZ * 2.9 * t).sin();
            LEVEL * attack * (-DECAY * t).exp() * tone / 1.35
        })
        .collect()
}

/// A constraint was made: the pop, when the preference has it.
pub(crate) fn constraint_made(app: &SolveApp) {
    if !app.preferences.constraint_audio {
        return;
    }
    REQUESTS.with(|r| r.set(r.get() + 1));
    if let Some(play) = &app.services.play_samples {
        play(pop_samples(RATE), RATE);
    }
}

/// Pops asked for on this thread so far (tests).
#[cfg(test)]
pub(crate) fn requests() -> usize {
    REQUESTS.with(Cell::get)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pop_is_a_short_decaying_click() {
        let s = pop_samples(RATE);
        assert_eq!(s.len(), (0.06 * RATE as f32) as usize, "about 60 ms");
        let peak = s.iter().copied().map(f32::abs).fold(0.0, f32::max);
        assert!(peak > 0.2 && peak <= 1.0, "audible and never clipped: {peak}");
        let at = s.iter().position(|v| v.abs() == peak).unwrap();
        assert!((at as f32) < 0.004 * RATE as f32, "a sharp attack: the peak at sample {at}");
        let tail = s[s.len() - 50..].iter().copied().map(f32::abs).fold(0.0, f32::max);
        assert!(tail < 0.05 * peak, "it dies away: {tail}");
        assert_eq!(s.first().copied(), Some(0.0), "it starts from silence");
    }
}
