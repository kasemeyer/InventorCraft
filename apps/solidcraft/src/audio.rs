//! The constraint pop's player (R175): the samples `solidcraft_ui_egui::audio` makes, played on
//! the default output device through rodio. The device opens on the first pop, on a thread of its
//! own; with no device (or none that opens) every pop is silently dropped.

use std::sync::mpsc;

#[cfg(all(feature = "audio-linux", target_os = "linux"))]
use rodio_linux as rodio;

/// The `Services::play_samples` hook, or None when the thread can't start.
pub fn player() -> Option<Box<dyn Fn(Vec<f32>, u32)>> {
    let (tx, rx) = mpsc::channel::<(Vec<f32>, u32)>();
    std::thread::Builder::new()
        .name("audio".into())
        .spawn(move || {
            let Ok((samples, rate)) = rx.recv() else { return };
            let Ok(mut stream) = rodio::OutputStreamBuilder::open_default_stream() else {
                log::info!("no audio output: the constraint pop stays silent");
                return;
            };
            stream.log_on_drop(false);
            stream.mixer().add(rodio::buffer::SamplesBuffer::new(1, rate, samples));
            while let Ok((samples, rate)) = rx.recv() {
                stream.mixer().add(rodio::buffer::SamplesBuffer::new(1, rate, samples));
            }
        })
        .ok()?;
    Some(Box::new(move |samples, rate| {
        let _ = tx.send((samples, rate));
    }))
}
