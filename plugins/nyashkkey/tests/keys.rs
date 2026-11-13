//! The finder must name the right key and tuning for chord progressions in every key,
//! at any tuning offset (a sped-up / slowed-down sample), and with typical voicings.
use nyashkkey::key::{evaluate, ref_a4, Analyzer, NOTE_SHARP};

fn xorshift(s: &mut u32) -> f64 {
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    *s as f64 / u32::MAX as f64
}

/// A progression in the key (tonic pc, minor) with bass + triad, harmonic tones, tuned `cents`.
fn progression(fs: f64, tonic: usize, minor: bool, cents: f64, secs: f64, seed: u32) -> Vec<f32> {
    // degrees (semitones above tonic) of chord roots and chord qualities (true = minor triad)
    let prog: &[(i32, bool)] = if minor {
        &[(0, true), (8, false), (3, false), (10, false), (0, true), (5, true), (7, false), (0, true)] // i VI III VII i iv V i
    } else {
        &[(0, false), (7, false), (9, true), (5, false), (0, false), (2, true), (7, false), (0, false)] // I V vi IV I ii V I
    };
    let mut s = seed;
    let n = (fs * secs) as usize;
    let mut out = vec![0.0f32; n];
    let chord_len = 0.75;
    let tune = 2f64.powf(cents / 1200.0);
    for i in 0..n {
        let t = i as f64 / fs;
        let ci = (t / chord_len) as usize % prog.len();
        let (root, mq) = prog[ci];
        let base = 36 + tonic as i32 + root; // bass in octave 2-3
        let third = if mq { 3 } else { 4 };
        let notes = [base, base + 12, base + 12 + third, base + 19, base + 24 + third];
        let env = (-(t % chord_len) * 3.0).exp();
        let mut v = 0.0;
        for (k, m) in notes.iter().enumerate() {
            let f = 440.0 * 2f64.powf((*m as f64 - 69.0) / 12.0) * tune;
            let w = 2.0 * std::f64::consts::PI * f * t;
            let a = if k == 0 { 0.25 } else { 0.10 };
            v += a * env * (w.sin() + 0.5 * (2.0 * w).sin() + 0.3 * (3.0 * w).sin() + 0.15 * (4.0 * w).sin());
        }
        out[i] = (v + 0.01 * (xorshift(&mut s) - 0.5)) as f32;
    }
    out
}

#[test]
fn every_key_and_any_tuning() {
    let fs = 48000.0;
    let mut seed = 7u32;
    let mut wrong = Vec::new();
    let mut worst_tune = 0.0f64;
    for tonic in 0..12 {
        for minor in [false, true] {
            let cents = (xorshift(&mut seed) - 0.5) * 90.0; // -45..45
            let x = progression(fs, tonic, minor, cents, 8.0, seed);
            let mut an = Analyzer::new(fs as f32);
            for &s in &x {
                an.push(s);
            }
            let r = evaluate(&an.snap.hist).expect("something analysed");
            let k = r.keys[0];
            let terr = (r.tuning_c - cents).abs();
            worst_tune = worst_tune.max(terr);
            let ok = k.tonic == tonic && k.minor == minor;
            println!(
                "{:>2}{} at {:+5.1} c -> {} ({:.2}), tuning {:+5.1} c (A4 {:.1} Hz), confidence {:.2}",
                NOTE_SHARP[tonic], if minor { "m" } else { " " }, cents, k.name(false), k.score, r.tuning_c, ref_a4(r.tuning_c), r.confidence
            );
            if !ok || terr > 3.0 {
                wrong.push(format!("{}{} {:+.1}c -> {} {:+.1}c", NOTE_SHARP[tonic], if minor { "m" } else { "" }, cents, k.name(false), r.tuning_c));
            }
        }
    }
    println!("worst tuning error {worst_tune:.2} c");
    assert!(wrong.is_empty(), "{:?}", wrong);
}

#[test]
fn silence_gives_nothing() {
    let mut an = Analyzer::new(44100.0);
    for _ in 0..44100 * 3 {
        an.push(0.0);
    }
    assert!(evaluate(&an.snap.hist).is_none());
    assert_eq!(an.snap.secs, 0.0);
}
