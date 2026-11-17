//! Harder material for the key finder: a sung melody alone, a full mix with drums, and songs that
//! lean on the relative key.
use nyashkkey::key::{evaluate, Analyzer, NOTE_SHARP};

fn rng(s: &mut u32) -> f64 {
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    *s as f64 / u32::MAX as f64
}

fn tone(f: f64, t: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * f * t;
    w.sin() + 0.4 * (2.0 * w).sin() + 0.2 * (3.0 * w).sin()
}

fn hz(m: f64) -> f64 {
    440.0 * 2f64.powf((m - 69.0) / 12.0)
}

/// a voice-like melody over the scale that keeps returning to the tonic, with vibrato and scoops
fn melody(fs: f64, tonic: usize, minor: bool, secs: f64, seed: u32) -> Vec<f32> {
    let st: [i32; 7] = if minor { [0, 2, 3, 5, 7, 8, 10] } else { [0, 2, 4, 5, 7, 9, 11] };
    let mut s = seed;
    let n = (fs * secs) as usize;
    let mut out = vec![0.0f32; n];
    let mut deg = 0i32;
    let mut next = 0.0;
    let mut cur = 60.0 + tonic as f64;
    let mut prev = cur;
    let mut t0 = 0.0;
    let mut ph = 0.0f64;
    for i in 0..n {
        let t = i as f64 / fs;
        if t >= next {
            // walk the scale, falling back to the tonic or the fifth often
            let r = rng(&mut s);
            deg = if r < 0.25 { 0 } else if r < 0.4 { 4 } else { (deg + [-2, -1, 1, 2][(rng(&mut s) * 4.0) as usize % 4]).clamp(-3, 9) };
            let oct = deg.div_euclid(7);
            prev = cur;
            cur = 60.0 + tonic as f64 + 12.0 * oct as f64 + st[deg.rem_euclid(7) as usize] as f64;
            if cur > 76.0 {
                cur -= 12.0;
            }
            t0 = t;
            next = t + [0.25, 0.4, 0.6, 0.9][(rng(&mut s) * 4.0) as usize % 4];
        }
        let dt = t - t0;
        let m = cur + (prev - cur) * (-dt / 0.03).exp() + 0.3 * (dt - 0.2).max(0.0).min(0.3) / 0.3 * (2.0 * std::f64::consts::PI * 5.5 * t).sin();
        // integrate the phase (the pitch moves continuously)
        ph += 2.0 * std::f64::consts::PI * hz(m) / fs;
        out[i] = (0.3 * (ph.sin() + 0.4 * (2.0 * ph).sin() + 0.2 * (3.0 * ph).sin())) as f32;
    }
    out
}

/// chords + bass + noise drums, with the progression leaning on the relative key
fn mix(fs: f64, tonic: usize, minor: bool, secs: f64, seed: u32) -> Vec<f32> {
    // minor: i VI III VII (lots of relative-major chords); major: I vi IV V
    let prog: &[(i32, bool)] = if minor { &[(0, true), (8, false), (3, false), (10, false)] } else { &[(0, false), (9, true), (5, false), (7, false)] };
    let mut s = seed;
    let n = (fs * secs) as usize;
    let mut out = vec![0.0f32; n];
    for i in 0..n {
        let t = i as f64 / fs;
        let (root, mq) = prog[(t / 1.0) as usize % prog.len()];
        let b = 36.0 + tonic as f64 + root as f64;
        let third = if mq { 3.0 } else { 4.0 };
        let mut v = 0.3 * tone(hz(b), t);
        for k in [12.0, 12.0 + third, 19.0] {
            v += 0.08 * tone(hz(b + k), t);
        }
        // kick + hats as noise bursts
        let beat = (t * 2.0).fract();
        v += 0.5 * (-beat * 30.0).exp() * (2.0 * std::f64::consts::PI * 55.0 * t).sin();
        v += 0.15 * (rng(&mut s) - 0.5) * (-(t * 4.0).fract() * 40.0).exp();
        out[i] = v as f32;
    }
    out
}

/// (key found first, tuning, rank of the right key among the candidates)
fn analyse(x: &[f32], fs: f32, tonic: usize, minor: bool) -> ((usize, bool), f64, usize) {
    let mut an = Analyzer::new(fs);
    for &v in x {
        an.push(v);
    }
    let r = evaluate(&an.snap.hist).expect("analysed");
    let rank = r.keys.iter().position(|k| k.tonic == tonic && k.minor == minor).unwrap();
    ((r.keys[0].tonic, r.keys[0].minor), r.tuning_c, rank)
}

fn name(t: usize, m: bool) -> String {
    format!("{}{}", NOTE_SHARP[t], if m { "m" } else { "" })
}

#[test]
fn melody_alone_in_every_key() {
    let fs = 44100.0;
    let mut not_first = vec![];
    for tonic in 0..12 {
        for minor in [false, true] {
            let x = melody(fs as f64, tonic, minor, 20.0, 11 + tonic as u32 * 3 + minor as u32);
            let ((t, m), tc, rank) = analyse(&x, fs, tonic, minor);
            assert!(tc.abs() < 5.0, "{}: tuning {tc}", name(tonic, minor));
            if rank > 0 {
                not_first.push(format!("{} -> {} (right one #{})", name(tonic, minor), name(t, m), rank + 1));
            }
            // a lone melody can be ambiguous (tonic / dominant): the right key must be offered
            assert!(rank < 3, "{}: right key only #{}", name(tonic, minor), rank + 1);
        }
    }
    println!("melody alone: {} of 24 not first {:?}", not_first.len(), not_first);
    assert!(not_first.len() <= 6);
}

#[test]
fn full_mix_with_drums_in_every_key() {
    let fs = 48000.0;
    let mut outside = vec![];
    let mut relative = 0;
    for tonic in 0..12 {
        for minor in [false, true] {
            let x = mix(fs as f64, tonic, minor, 16.0, 5 + tonic as u32);
            let ((t, m), tc, _) = analyse(&x, fs, tonic, minor);
            assert!(tc.abs() < 5.0, "{}: tuning {tc}", name(tonic, minor));
            // i-VI-III-VII leans on the relative major: the relative key (same notes) is fine
            let rel = if minor { ((tonic + 3) % 12, false) } else { ((tonic + 9) % 12, true) };
            if (t, m) == rel {
                relative += 1;
            } else if (t, m) != (tonic, minor) {
                outside.push(format!("{} -> {}", name(tonic, minor), name(t, m)));
            }
        }
    }
    println!("full mix: {relative} of 24 read as the relative key, outside the key signature {outside:?}");
    // (G♯m read as E major: its VI chord, one note apart - the one miss of this loop)
    assert!(outside.len() <= 1, "{outside:?}");
}
