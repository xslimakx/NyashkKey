//! Key and tuning finder.
//!
//! The input is low-passed and decimated to about 12 kHz. Every hop a Hann-windowed FFT of 8192
//! points (~0.7 s) is taken, spectral peaks are found and refined (parabolic interpolation in dB),
//! and every peak is added - weighted by its level - to a fine pitch histogram: 1200 bins of one
//! cent each over one octave (pitch class + cents, relative to C).
//!
//! From that histogram:
//! - the tuning is the circular mean of all peaks' offsets from the 440 Hz grid (mod 100 cents),
//!   so a sped-up or slowed-down sample is recognised (e.g. +39 c = A4 at 449.9 Hz);
//! - the 12-note profile ("chroma") is read off the histogram around the *tuned* note centres;
//! - the key is the major / minor key whose profile correlates best with the chroma.
//!
//! Everything is accumulated since the last reset, so the longer you play, the surer it gets.

use nyashk_common::fft::Fft;

pub const N: usize = 8192;
pub const HOP: usize = 2048;
pub const BINS: usize = 1200;

pub const NOTE_SHARP: [&str; 12] = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
pub const NOTE_FLAT: [&str; 12] = ["C", "D♭", "D", "E♭", "E", "F", "G♭", "G", "A♭", "A", "B♭", "B"];

/// Key profiles (Temperley 2007, Kostka-Payne corpus), index 0 = tonic.
const MAJOR: [f64; 12] = [0.748, 0.060, 0.488, 0.082, 0.670, 0.460, 0.096, 0.715, 0.104, 0.366, 0.057, 0.400];
const MINOR: [f64; 12] = [0.712, 0.084, 0.474, 0.618, 0.049, 0.460, 0.105, 0.747, 0.404, 0.067, 0.133, 0.330];

/// Second-order section (direct form I), used for the anti-alias low-pass.
#[derive(Clone, Copy, Default)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Biquad {
    fn lowpass(fs: f64, fc: f64, q: f64) -> Self {
        let w = 2.0 * std::f64::consts::PI * fc / fs;
        let (s, c) = w.sin_cos();
        let al = s / (2.0 * q);
        let a0 = 1.0 + al;
        Biquad { b: [(1.0 - c) / 2.0 / a0, (1.0 - c) / a0, (1.0 - c) / 2.0 / a0], a: [-2.0 * c / a0, (1.0 - al) / a0], x: [0.0; 2], y: [0.0; 2] }
    }
    #[inline]
    fn run(&mut self, v: f64) -> f64 {
        let y = self.b[0] * v + self.b[1] * self.x[0] + self.b[2] * self.x[1] - self.a[0] * self.y[0] - self.a[1] * self.y[1];
        self.x = [v, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// What the analysis has gathered so far (copied to the editor).
#[derive(Clone)]
pub struct Snapshot {
    /// pitch histogram: bin b = (b + 0.5) cents above C, any octave
    pub hist: Vec<f64>,
    /// seconds of (non-silent) audio analysed
    pub secs: f64,
    /// short-term chroma of the latest frames (what is sounding now), untuned bins
    pub recent: Vec<f64>,
}

impl Snapshot {
    pub fn new() -> Self {
        Snapshot { hist: vec![0.0; BINS], secs: 0.0, recent: vec![0.0; BINS] }
    }
}

impl Default for Snapshot {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Analyzer {
    decim: usize,
    fs_d: f64,
    lp: [Biquad; 2],
    phase: usize,
    ring: Vec<f32>,
    pos: usize,
    filled: usize,
    since_hop: usize,
    fft: Fft,
    frame: Vec<f32>,
    mags: Vec<f32>,
    pub snap: Snapshot,
}

impl Analyzer {
    pub fn new(fs: f32) -> Self {
        let fs = fs as f64;
        let decim = ((fs / 12000.0).round() as usize).max(1);
        let fs_d = fs / decim as f64;
        // 4th-order Butterworth low-pass at ~0.38 of the decimated rate
        let fc = 0.38 * fs_d;
        let lp = [Biquad::lowpass(fs, fc, 0.5412), Biquad::lowpass(fs, fc, 1.3066)];
        Analyzer {
            decim,
            fs_d,
            lp,
            phase: 0,
            ring: vec![0.0; N],
            pos: 0,
            filled: 0,
            since_hop: 0,
            fft: Fft::new(N),
            frame: vec![0.0; N],
            mags: vec![0.0; N / 2],
            snap: Snapshot::new(),
        }
    }

    pub fn reset(&mut self) {
        self.snap.hist.iter_mut().for_each(|v| *v = 0.0);
        self.snap.recent.iter_mut().for_each(|v| *v = 0.0);
        self.snap.secs = 0.0;
    }

    /// Feed one (mono) input sample. Returns true when a new analysis frame was added.
    #[inline]
    pub fn push(&mut self, x: f32) -> bool {
        let mut v = x as f64;
        for f in self.lp.iter_mut() {
            v = f.run(v);
        }
        self.phase += 1;
        if self.phase < self.decim {
            return false;
        }
        self.phase = 0;
        self.ring[self.pos] = v as f32;
        self.pos = (self.pos + 1) % N;
        self.filled = (self.filled + 1).min(N);
        self.since_hop += 1;
        if self.filled == N && self.since_hop >= HOP {
            self.since_hop = 0;
            self.analyse();
            return true;
        }
        false
    }

    fn analyse(&mut self) {
        // newest N samples in order
        let mut e = 0.0f64;
        for i in 0..N {
            let s = self.ring[(self.pos + i) % N];
            self.frame[i] = s;
            e += (s as f64) * (s as f64);
        }
        let hop_secs = HOP as f64 / self.fs_d;
        // recent view decays with a ~3 s memory
        let k = (-hop_secs / 3.0).exp();
        self.snap.recent.iter_mut().for_each(|v| *v *= k);
        let rms_db = 10.0 * (e / N as f64 + 1e-20).log10();
        if rms_db < -65.0 {
            return; // silence: nothing to learn
        }
        self.fft.magnitude_db(&self.frame, &mut self.mags);
        let bin_hz = self.fs_d / N as f64;
        let k_lo = (50.0 / bin_hz).floor() as usize + 1;
        let k_hi = ((4200.0f64).min(self.fs_d * 0.42) / bin_hz) as usize;
        let mut top = -200.0f32;
        for k in k_lo..k_hi {
            top = top.max(self.mags[k]);
        }
        let floor = (top - 50.0).max(-100.0);
        let mut added = 0.0;
        for k in k_lo..k_hi {
            let b = self.mags[k];
            if b < floor || b <= self.mags[k - 1] || b < self.mags[k + 1] {
                continue;
            }
            let (a, c) = (self.mags[k - 1] as f64, self.mags[k + 1] as f64);
            let b = b as f64;
            let den = a - 2.0 * b + c;
            let d = if den.abs() > 1e-9 { (0.5 * (a - c) / den).clamp(-0.5, 0.5) } else { 0.0 };
            let f = (k as f64 + d) * bin_hz;
            let level_db = b - 0.25 * (a - c) * d;
            // weight: level (compressed), less for high partials that are mostly overtones
            let lin = 10f64.powf((level_db - top as f64) / 20.0); // 0..1 relative to the frame's peak
            let fw = if f <= 1000.0 { 1.0 } else { (1000.0 / f).powf(0.8) };
            let w = lin.sqrt() * fw * (rms_db + 90.0).max(0.0) / 90.0;
            // cents above C (any octave)
            let cents_a = 1200.0 * (f / 440.0).log2();
            let c_c = (cents_a + 900.0).rem_euclid(1200.0);
            let bi = (c_c as usize).min(BINS - 1);
            self.snap.hist[bi] += w * hop_secs;
            self.snap.recent[bi] += w * hop_secs;
            added += w;
        }
        if added > 0.0 {
            self.snap.secs += hop_secs;
        }
    }
}

/// Tuning offset in cents (-50..50) from a pitch histogram, and how consistent it is (0..1).
pub fn tuning(hist: &[f64]) -> (f64, f64) {
    let (mut cs, mut sn, mut tot) = (0.0f64, 0.0f64, 0.0f64);
    for (b, &w) in hist.iter().enumerate() {
        if w <= 0.0 {
            continue;
        }
        let ang = 2.0 * std::f64::consts::PI * ((b as f64 + 0.5) % 100.0) / 100.0;
        cs += w * ang.cos();
        sn += w * ang.sin();
        tot += w;
    }
    if tot <= 0.0 {
        return (0.0, 0.0);
    }
    let mut t = sn.atan2(cs) * 100.0 / (2.0 * std::f64::consts::PI);
    if t >= 50.0 {
        t -= 100.0;
    }
    (t, (cs * cs + sn * sn).sqrt() / tot)
}

/// 12-note profile around the tuned note centres (C = 0), normalised to max 1.
pub fn chroma(hist: &[f64], tuning_c: f64) -> [f64; 12] {
    let mut ch = [0.0f64; 12];
    for (b, &w) in hist.iter().enumerate() {
        if w <= 0.0 {
            continue;
        }
        let rel = b as f64 + 0.5 - tuning_c;
        let pc = ((rel / 100.0).round() as i64).rem_euclid(12) as usize;
        // within +-50 c of the centre; the very edges count less
        let dist = (rel - 100.0 * (rel / 100.0).round()).abs();
        ch[pc] += w * (1.0 - 0.5 * (dist / 50.0).powi(2));
    }
    let m = ch.iter().cloned().fold(0.0, f64::max);
    if m > 0.0 {
        ch.iter_mut().for_each(|v| *v /= m);
    }
    ch
}

fn pearson(a: &[f64; 12], b: &[f64; 12]) -> f64 {
    let ma = a.iter().sum::<f64>() / 12.0;
    let mb = b.iter().sum::<f64>() / 12.0;
    let (mut num, mut da, mut db) = (0.0, 0.0, 0.0);
    for i in 0..12 {
        num += (a[i] - ma) * (b[i] - mb);
        da += (a[i] - ma).powi(2);
        db += (b[i] - mb).powi(2);
    }
    if da <= 0.0 || db <= 0.0 {
        return 0.0;
    }
    num / (da * db).sqrt()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
    pub tonic: usize, // 0 = C
    pub minor: bool,
    pub score: f64,
}

impl Key {
    pub fn name(&self, flats: bool) -> String {
        let n = if flats { NOTE_FLAT[self.tonic] } else { NOTE_SHARP[self.tonic] };
        format!("{} {}", n, if self.minor { "minor" } else { "major" })
    }
    /// relative major / minor
    pub fn relative(&self) -> Key {
        if self.minor {
            Key { tonic: (self.tonic + 3) % 12, minor: false, score: self.score }
        } else {
            Key { tonic: (self.tonic + 9) % 12, minor: true, score: self.score }
        }
    }
    /// Camelot wheel code (8B = C major, 5A = C minor)
    pub fn camelot(&self) -> String {
        // circle of fifths position of the tonic, C = 0
        let fifths = (self.tonic * 7) % 12;
        if self.minor {
            // A minor = 8A
            let n = ((fifths + 12 - 3) % 12 + 8 - 1) % 12 + 1;
            format!("{}A", n)
        } else {
            let n = (fifths + 8 - 1) % 12 + 1;
            format!("{}B", n)
        }
    }
    /// Whether this key is written with flats (F, B♭, E♭, A♭, D♭, G♭ major and their relative minors).
    pub fn uses_flats(&self) -> bool {
        let major_tonic = if self.minor { (self.tonic + 3) % 12 } else { self.tonic };
        matches!(major_tonic, 5 | 10 | 3 | 8 | 1 | 6)
    }
    /// pitch classes of the (natural) scale
    pub fn scale(&self) -> [usize; 7] {
        let st: [usize; 7] = if self.minor { [0, 2, 3, 5, 7, 8, 10] } else { [0, 2, 4, 5, 7, 9, 11] };
        st.map(|s| (self.tonic + s) % 12)
    }
}

/// All 24 keys, best first.
pub fn keys(ch: &[f64; 12]) -> Vec<Key> {
    let mut out = Vec::with_capacity(24);
    for tonic in 0..12 {
        for minor in [false, true] {
            let prof = if minor { &MINOR } else { &MAJOR };
            let mut rot = [0.0f64; 12];
            for i in 0..12 {
                rot[(tonic + i) % 12] = prof[i];
            }
            out.push(Key { tonic, minor, score: pearson(ch, &rot) });
        }
    }
    out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    out
}

/// Everything the editor shows.
#[derive(Clone, Debug)]
pub struct Result {
    pub tuning_c: f64,
    pub tuning_conf: f64,
    pub chroma: [f64; 12],
    pub keys: Vec<Key>,
    /// 0..1: how clearly the best key wins
    pub confidence: f64,
}

pub fn evaluate(hist: &[f64]) -> Option<Result> {
    let tot: f64 = hist.iter().sum();
    if tot <= 0.0 {
        return None;
    }
    let (t, tc) = tuning(hist);
    let ch = chroma(hist, t);
    let ks = keys(&ch);
    // a clear winner over the runner-up that is not its own relative key counts most
    let best = ks[0];
    let rel = best.relative();
    let second = ks.iter().skip(1).find(|k| !(k.tonic == rel.tonic && k.minor == rel.minor)).map(|k| k.score).unwrap_or(0.0);
    let rel_score = ks.iter().find(|k| k.tonic == rel.tonic && k.minor == rel.minor).map(|k| k.score).unwrap_or(0.0);
    let gap = (best.score - second).max(0.0);
    let gap_rel = (best.score - rel_score).max(0.0);
    let confidence = ((gap / 0.15).min(1.0) * 0.6 + (gap_rel / 0.10).min(1.0) * 0.4) * best.score.clamp(0.0, 1.0).sqrt();
    Some(Result { tuning_c: t, tuning_conf: tc, chroma: ch, keys: ks, confidence })
}

/// A4 frequency for a tuning offset in cents.
pub fn ref_a4(tuning_c: f64) -> f64 {
    440.0 * 2f64.powf(tuning_c / 1200.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camelot_codes() {
        let k = |t, m| Key { tonic: t, minor: m, score: 0.0 };
        assert_eq!(k(0, false).camelot(), "8B"); // C major
        assert_eq!(k(9, true).camelot(), "8A"); // A minor
        assert_eq!(k(7, false).camelot(), "9B"); // G major
        assert_eq!(k(4, true).camelot(), "9A"); // E minor
        assert_eq!(k(6, true).camelot(), "11A"); // F# minor
        assert_eq!(k(0, true).camelot(), "5A"); // C minor
        assert_eq!(k(5, false).camelot(), "7B"); // F major
        assert_eq!(k(11, false).camelot(), "1B"); // B major
    }

    #[test]
    fn spelling() {
        let k = |t, m| Key { tonic: t, minor: m, score: 0.0 };
        assert!(k(5, true).uses_flats()); // F minor
        assert!(k(3, false).uses_flats()); // Eb major
        assert!(k(2, true).uses_flats()); // D minor (1 flat)
        assert!(!k(4, true).uses_flats()); // E minor
        assert!(!k(9, true).uses_flats()); // A minor
        assert!(!k(0, false).uses_flats()); // C major
        assert!(!k(6, true).uses_flats()); // F# minor
    }

    #[test]
    fn relative_keys() {
        let am = Key { tonic: 9, minor: true, score: 0.0 };
        assert_eq!(am.relative(), Key { tonic: 0, minor: false, score: 0.0 });
        assert_eq!(am.relative().relative(), am);
    }
}
