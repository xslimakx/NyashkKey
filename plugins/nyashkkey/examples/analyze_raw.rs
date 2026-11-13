//! usage: analyze_raw file.f32 sample_rate [every_secs]  (raw mono f32 little-endian)
//! prints the running result every few seconds and the final one
use nyashkkey::key::{evaluate, ref_a4, Analyzer};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let raw = std::fs::read(&a[1]).unwrap();
    let fs: f32 = a[2].parse().unwrap();
    let every: f64 = a.get(3).map(|s| s.parse().unwrap()).unwrap_or(1e9);
    let x: Vec<f32> = raw.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
    let mut an = Analyzer::new(fs);
    let mut next = every;
    let show = |an: &Analyzer, t: f64| {
        if let Some(r) = evaluate(&an.snap.hist) {
            let ks: Vec<String> = r.keys.iter().take(3).map(|k| format!("{} {:.2}", k.name(false), k.score)).collect();
            println!("{:6.1}s  {:<40} tuning {:+5.1} c (A4 {:.1}) conf {:.2}", t, ks.join(" | "), r.tuning_c, ref_a4(r.tuning_c), r.confidence);
        }
    };
    for (i, &s) in x.iter().enumerate() {
        an.push(s);
        let t = i as f64 / fs as f64;
        if t >= next {
            show(&an, t);
            next += every;
        }
    }
    show(&an, x.len() as f64 / fs as f64);
}
