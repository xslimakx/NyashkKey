//! NyashkKey: finds the key (and how far the tuning is off 440 Hz) of whatever plays through it.
//! The audio passes through untouched.
use nih_plug::prelude::*;
use nih_plug_egui::EguiState;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

pub mod key;
mod editor;

pub struct Shared {
    pub snap: Mutex<key::Snapshot>,
    pub reset: AtomicBool,
    /// bumps on every published analysis frame (the editor repaints on change)
    pub frames: AtomicU32,
}

impl Shared {
    fn new() -> Self {
        Self { snap: Mutex::new(key::Snapshot::new()), reset: AtomicBool::new(false), frames: AtomicU32::new(0) }
    }
}

#[derive(Params)]
pub struct KeyParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,
    /// pause the analysis (keeps what was found)
    #[id = "hold"]
    pub hold: BoolParam,
    /// how to spell the notes: as the found key is written, or always sharps / flats
    #[id = "accid"]
    pub accidentals: EnumParam<Accidentals>,
}

#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum Accidentals {
    #[name = "Auto"]
    Auto,
    #[name = "Sharps"]
    Sharps,
    #[name = "Flats"]
    Flats,
}

impl Default for KeyParams {
    fn default() -> Self {
        Self { editor_state: EguiState::from_size(820, 480), hold: BoolParam::new("Hold", false), accidentals: EnumParam::new("Accidentals", Accidentals::Auto) }
    }
}

pub struct NyashkKey {
    params: Arc<KeyParams>,
    shared: Arc<Shared>,
    an: Option<key::Analyzer>,
}

impl Default for NyashkKey {
    fn default() -> Self {
        Self { params: Arc::new(KeyParams::default()), shared: Arc::new(Shared::new()), an: None }
    }
}

impl Plugin for NyashkKey {
    const NAME: &'static str = "NyashkKey";
    const VENDOR: &'static str = "Nyashk";
    const URL: &'static str = "https://github.com/xslimakx";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout { main_input_channels: NonZeroU32::new(2), main_output_channels: NonZeroU32::new(2), ..AudioIOLayout::const_default() },
        AudioIOLayout { main_input_channels: NonZeroU32::new(1), main_output_channels: NonZeroU32::new(1), ..AudioIOLayout::const_default() },
    ];
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _e: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone(), self.shared.clone())
    }

    fn initialize(&mut self, _l: &AudioIOLayout, config: &BufferConfig, _c: &mut impl InitContext<Self>) -> bool {
        let mut an = key::Analyzer::new(config.sample_rate);
        // keep what was analysed before (sample-rate change / re-activation) when possible
        if let Ok(s) = self.shared.snap.lock() {
            an.snap = s.clone();
        }
        self.an = Some(an);
        true
    }

    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, _c: &mut impl ProcessContext<Self>) -> ProcessStatus {
        nyashk_common::demo::sanitize(buffer);
        #[cfg(feature = "standalone")]
        demo_fill(buffer);
        let Some(an) = self.an.as_mut() else { return ProcessStatus::Normal };
        let sh = &self.shared;
        if sh.reset.swap(false, Ordering::Relaxed) {
            an.reset();
            if let Ok(mut s) = sh.snap.try_lock() {
                s.clone_from(&an.snap);
            }
            sh.frames.fetch_add(1, Ordering::Relaxed);
        }
        if self.params.hold.value() {
            return ProcessStatus::Normal;
        }
        let mut fresh = false;
        for frame in buffer.iter_samples() {
            let n = frame.len().max(1);
            let mut m = 0.0f32;
            for s in frame {
                m += *s;
            }
            fresh |= an.push(m / n as f32);
        }
        if fresh {
            // never block the audio thread: if the editor is reading right now, publish next time
            if let Ok(mut s) = sh.snap.try_lock() {
                s.hist.copy_from_slice(&an.snap.hist);
                s.recent.copy_from_slice(&an.snap.recent);
                s.secs = an.snap.secs;
                sh.frames.fetch_add(1, Ordering::Relaxed);
            }
        }
        ProcessStatus::Normal
    }
}

/// Standalone screenshots (`NYASHK_DEMO=1`): a chord loop in F minor (i VI iv V), tuned +39 c.
#[cfg(feature = "standalone")]
fn demo_fill(buffer: &mut Buffer) {
    use std::sync::atomic::AtomicU64;
    static N: AtomicU64 = AtomicU64::new(0);
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var("NYASHK_DEMO").is_ok()) {
        return;
    }
    let fs = 48000.0f64;
    let tune = 2f64.powf(39.0 / 1200.0);
    // Fm - Db - Bbm - C (i VI iv V), one chord per second, bass + triad
    let chords: [[f64; 4]; 4] = [[41.0, 53.0, 56.0, 60.0], [37.0, 53.0, 56.0, 61.0], [46.0, 53.0, 58.0, 61.0], [36.0, 52.0, 55.0, 60.0]];
    for mut frame in buffer.iter_samples() {
        let n = N.fetch_add(1, Ordering::Relaxed);
        let t = n as f64 / fs;
        let c = &chords[(t as usize) % 4];
        let env = (-(t % 1.0) * 2.5).exp();
        let mut v = 0.0;
        for (i, m) in c.iter().enumerate() {
            let f = 440.0 * 2f64.powf((m - 69.0) / 12.0) * tune;
            let w = 2.0 * std::f64::consts::PI * f * t;
            let a = if i == 0 { 0.30 } else { 0.12 };
            v += a * env * (w.sin() + 0.4 * (2.0 * w).sin() + 0.2 * (3.0 * w).sin());
        }
        for s in frame.iter_mut() {
            *s = v as f32 * 0.8;
        }
    }
}

impl Vst3Plugin for NyashkKey {
    const VST3_CLASS_ID: [u8; 16] = *b"NyashkKeyFindVST";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx, Vst3SubCategory::Analyzer];
}

nih_export_vst3!(NyashkKey);
