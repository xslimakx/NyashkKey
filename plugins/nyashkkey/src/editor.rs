//! NyashkKey GUI: the key, how sure it is, the tuning offset, the notes that sound, and the values
//! to type into NyashkTune.
use nih_plug::prelude::*;
use nih_plug_egui::{
    create_egui_editor,
    egui::{self, epaint::PathStroke, Align2, Color32, Pos2, Rect, Stroke, Vec2},
};
use nyashk_common::ui::*;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use crate::key::{self, Key, NOTE_FLAT, NOTE_SHARP};
use crate::{Accidentals, KeyParams, Shared};

const BASE_W: f32 = 820.0;
const BASE_H: f32 = 480.0;

struct Gui {
    hist: Vec<f64>,
    recent: Vec<f64>,
    secs: f64,
    needle: f64,
}

fn note(pc: usize, flats: bool) -> &'static str {
    if flats {
        NOTE_FLAT[pc % 12]
    } else {
        NOTE_SHARP[pc % 12]
    }
}

fn key_name(k: &Key, flats: bool) -> String {
    format!("{} {}", note(k.tonic, flats), if k.minor { "minor" } else { "major" })
}

/// Which NyashkTune scale fits: harmonic minor when the raised 7th clearly beats the natural one.
fn tune_scale(k: &Key, ch: &[f64; 12]) -> &'static str {
    if !k.minor {
        return "Major";
    }
    let nat7 = ch[(k.tonic + 10) % 12];
    let raised7 = ch[(k.tonic + 11) % 12];
    if raised7 > nat7 * 1.25 && raised7 > 0.35 {
        "Harmonic Minor"
    } else {
        "Natural Minor"
    }
}

/// ♯ and ♭ are not in egui's built-in fonts: DejaVu Sans as a fall-back after the primary font.
fn install_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontFamily};
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("dejavu".into(), Arc::new(FontData::from_static(include_bytes!("fonts/DejaVuSans.ttf"))));
    for fam in [FontFamily::Monospace, FontFamily::Proportional] {
        if let Some(list) = fonts.families.get_mut(&fam) {
            let at = list.len().min(1);
            list.insert(at, "dejavu".into());
        }
    }
    ctx.set_fonts(fonts);
}

pub fn create(params: Arc<KeyParams>, shared: Arc<Shared>) -> Option<Box<dyn Editor>> {
    let editor_state = params.editor_state.clone();
    let size_state = editor_state.clone();
    create_egui_editor(
        editor_state,
        Gui { hist: vec![0.0; key::BINS], recent: vec![0.0; key::BINS], secs: 0.0, needle: 0.0 },
        |ctx, _| {
            apply_theme(ctx);
            install_fonts(ctx);
        },
        move |ctx, setter, st| {
            ctx.request_repaint_after(Duration::from_millis(50));
            let p = &*params;
            if let Ok(s) = shared.snap.lock() {
                st.hist.copy_from_slice(&s.hist);
                st.recent.copy_from_slice(&s.recent);
                st.secs = s.secs;
            }
            let res = key::evaluate(&st.hist);
            let flats = match p.accidentals.value() {
                Accidentals::Sharps => false,
                Accidentals::Flats => true,
                Accidentals::Auto => res.as_ref().map(|r| r.keys[0].uses_flats()).unwrap_or(false),
            };
            let accid = p.accidentals.value();
            let hold = p.hold.value();
            if let Some(r) = &res {
                st.needle += (r.tuning_c - st.needle) * 0.25;
            }

            egui::CentralPanel::default().frame(egui::Frame::NONE.fill(BG)).show(ctx, |ui| {
                let full = ui.max_rect();
                let o = full.min;
                let rect = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(Pos2::new(o.x + x, o.y + y), Vec2::new(w, h));
                header(ui, o, BASE_W, "Key", "KEY & TUNING FINDER");
                let painter = ui.painter().clone();

                // ---- header buttons
                if pill_tip(ui, rect(BASE_W - 14.0 - 76.0, 22.0, 76.0, 24.0), "RESET", false, "Forget everything heard so far and start a new analysis.") {
                    shared.reset.store(true, Ordering::Relaxed);
                }
                if pill_tip(ui, rect(BASE_W - 14.0 - 76.0 - 8.0 - 70.0, 22.0, 70.0, 24.0), "HOLD", hold, "Pause the analysis and keep the result (the audio still passes through).") {
                    set_b(setter, &p.hold, !hold);
                }
                let acc = rect(BASE_W - 14.0 - 76.0 - 8.0 - 70.0 - 14.0 - 2.0 * 34.0 - 4.0 - 54.0, 22.0, 50.0, 24.0);
                if pill_tip(ui, acc, "AUTO", accid == Accidentals::Auto, "Spell the notes the way the found key is written (F minor: flats, E minor: sharps).") {
                    set_e(setter, &p.accidentals, Accidentals::Auto);
                }
                let acc = Rect::from_min_size(acc.min + Vec2::new(54.0, 0.0), Vec2::new(34.0, 24.0));
                if pill_tip(ui, acc, "♯", accid == Accidentals::Sharps, "Always sharps (C♯, F♯ ...).") {
                    set_e(setter, &p.accidentals, Accidentals::Sharps);
                }
                if pill_tip(ui, acc.translate(Vec2::new(38.0, 0.0)), "♭", accid == Accidentals::Flats, "Always flats (D♭, G♭ ...).") {
                    set_e(setter, &p.accidentals, Accidentals::Flats);
                }

                let listening = st.secs < 4.0;
                // ================= KEY
                let a = rect(14.0, 72.0, 392.0, 196.0);
                panel(ui, a);
                painter.text(Pos2::new(a.left() + 14.0, a.top() + 16.0), Align2::LEFT_CENTER, "KEY", mono(10.0), DIM);
                match &res {
                    None => {
                        painter.text(a.center() - Vec2::new(0.0, 8.0), Align2::CENTER_CENTER, "play something", mono(24.0), DIM);
                        painter.text(a.center() + Vec2::new(0.0, 24.0), Align2::CENTER_CENTER, "the key appears after a few seconds of music", mono(10.0), DIM);
                    }
                    Some(r) => {
                        let k = r.keys[0];
                        let col = if listening { DIM } else { WHITE };
                        painter.text(Pos2::new(a.center().x, a.top() + 72.0), Align2::CENTER_CENTER, key_name(&k, flats), mono(40.0), col);
                        let rel = k.relative();
                        painter.text(
                            Pos2::new(a.center().x, a.top() + 112.0),
                            Align2::CENTER_CENTER,
                            format!("relative {}   ·   Camelot {}", key_name(&rel, flats), k.camelot()),
                            mono(10.5),
                            DIM,
                        );
                        // confidence
                        let bar = Rect::from_min_size(Pos2::new(a.left() + 14.0, a.top() + 142.0), Vec2::new(a.width() - 28.0, 8.0));
                        painter.rect_filled(bar, 2.0, Color32::from_rgb(7, 7, 7));
                        let c = if listening { r.confidence * st.secs / 4.0 } else { r.confidence } as f32;
                        painter.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * c.clamp(0.0, 1.0), bar.height())), 2.0, Color32::from_gray(200));
                        painter.rect_stroke(bar, 2.0, Stroke::new(1.0, LINE), egui::StrokeKind::Inside);
                        let word = if listening {
                            "listening…"
                        } else if r.confidence > 0.7 {
                            "clear"
                        } else if r.confidence > 0.4 {
                            "likely"
                        } else {
                            "unsure — let more of the track play"
                        };
                        painter.text(Pos2::new(a.left() + 14.0, a.top() + 168.0), Align2::LEFT_CENTER, word, mono(10.5), if listening { DIM } else { WHITE });
                        painter.text(
                            Pos2::new(a.right() - 14.0, a.top() + 168.0),
                            Align2::RIGHT_CENTER,
                            format!("{:.0} s analysed{}", st.secs, if hold { "  ·  HOLD" } else { "" }),
                            mono(10.0),
                            DIM,
                        );
                    }
                }

                // ================= TUNING
                let b = rect(418.0, 72.0, 388.0, 196.0);
                panel(ui, b);
                painter.text(Pos2::new(b.left() + 14.0, b.top() + 16.0), Align2::LEFT_CENTER, "TUNING", mono(10.0), DIM);
                let c = Pos2::new(b.center().x, b.top() + 132.0);
                let rad = 92.0f32;
                let ang = |cents: f64| -> f32 { std::f32::consts::PI * (1.0 + (cents as f32 + 50.0) / 100.0) };
                let pt = |cents: f64, r: f32| c + Vec2::new(ang(cents).cos() * r, ang(cents).sin() * r);
                let arc: Vec<Pos2> = (0..=60).map(|i| pt(-50.0 + 100.0 * i as f64 / 60.0, rad)).collect();
                painter.add(egui::Shape::line(arc, PathStroke::new(2.0, LINE)));
                for t in [-50.0f64, -25.0, 0.0, 25.0, 50.0] {
                    painter.line_segment([pt(t, rad - 6.0), pt(t, rad + 4.0)], Stroke::new(1.0, if t == 0.0 { WHITE } else { DIM }));
                    painter.text(pt(t, rad + 16.0), Align2::CENTER_CENTER, if t == 0.0 { "0".to_string() } else { format!("{:+.0}", t) }, mono(9.0), DIM);
                }
                match &res {
                    None => {
                        painter.text(c - Vec2::new(0.0, 24.0), Align2::CENTER_CENTER, "--", mono(30.0), DIM);
                    }
                    Some(r) => {
                        let col = if listening { DIM } else { WHITE };
                        painter.line_segment([c, pt(st.needle, rad - 10.0)], Stroke::new(2.0, col));
                        painter.circle_filled(c, 4.0, col);
                        let t = r.tuning_c;
                        let ts = if t.abs() < 0.5 { "0 c".to_string() } else { format!("{:+.0} c", t) };
                        painter.text(c - Vec2::new(0.0, 40.0), Align2::CENTER_CENTER, ts, mono(30.0), col);
                        painter.text(c + Vec2::new(0.0, 22.0), Align2::CENTER_CENTER, format!("Ref A4 = {:.1} Hz", key::ref_a4(t)), mono(12.0), col);
                        let how = if t.abs() < 3.0 { "standard tuning (440 Hz)" } else if t > 0.0 { "tuned up (sped up?)" } else { "tuned down (slowed down?)" };
                        painter.text(c + Vec2::new(0.0, 42.0), Align2::CENTER_CENTER, how, mono(9.5), DIM);
                    }
                }

                // ================= NOTES
                let n = rect(14.0, 280.0, 528.0, 186.0);
                panel(ui, n);
                painter.text(Pos2::new(n.left() + 14.0, n.top() + 16.0), Align2::LEFT_CENTER, "NOTES", mono(10.0), DIM);
                painter.text(Pos2::new(n.right() - 14.0, n.top() + 16.0), Align2::RIGHT_CENTER, "bar = whole analysis   ·   tick = now", mono(9.0), DIM);
                let tun = res.as_ref().map(|r| r.tuning_c).unwrap_or(0.0);
                let now = key::chroma(&st.recent, tun);
                let has_now = st.recent.iter().any(|v| *v > 0.0);
                let scale: Vec<usize> = res.as_ref().map(|r| r.keys[0].scale().to_vec()).unwrap_or_default();
                let tonic = res.as_ref().map(|r| r.keys[0].tonic);
                let col_w = (n.width() - 28.0) / 12.0;
                let (bt, bb) = (n.top() + 34.0, n.bottom() - 30.0);
                for pc in 0..12 {
                    let x0 = n.left() + 14.0 + col_w * pc as f32;
                    let slot = Rect::from_min_max(Pos2::new(x0 + 5.0, bt), Pos2::new(x0 + col_w - 5.0, bb));
                    painter.rect_filled(slot, 2.0, Color32::from_rgb(7, 7, 7));
                    let in_scale = scale.contains(&pc);
                    if let Some(r) = &res {
                        let h = (r.chroma[pc] as f32).clamp(0.0, 1.0) * slot.height();
                        let fill = if in_scale { Color32::from_gray(if listening { 120 } else { 205 }) } else { Color32::from_gray(70) };
                        painter.rect_filled(Rect::from_min_max(Pos2::new(slot.left(), slot.bottom() - h), slot.max), 2.0, fill);
                    }
                    if has_now {
                        let y = slot.bottom() - (now[pc] as f32).clamp(0.0, 1.0) * slot.height();
                        painter.hline(slot.left() - 3.0..=slot.right() + 3.0, y, Stroke::new(2.0, Color32::from_rgb(0xe2, 0xb0, 0x58)));
                    }
                    painter.rect_stroke(slot, 2.0, Stroke::new(1.0, LINE), egui::StrokeKind::Inside);
                    let is_tonic = tonic == Some(pc);
                    painter.text(
                        Pos2::new(slot.center().x, bb + 14.0),
                        Align2::CENTER_CENTER,
                        note(pc, flats),
                        mono(if is_tonic { 12.0 } else { 10.5 }),
                        if is_tonic { WHITE } else if in_scale { Color32::from_gray(190) } else { DIM },
                    );
                    if is_tonic {
                        painter.circle_filled(Pos2::new(slot.center().x, bt - 6.0), 2.5, WHITE);
                    }
                }

                // ================= FOR NYASHKTUNE + alternatives
                let d = rect(554.0, 280.0, 252.0, 186.0);
                panel(ui, d);
                painter.text(Pos2::new(d.left() + 14.0, d.top() + 16.0), Align2::LEFT_CENTER, "FOR NYASHKTUNE", mono(10.0), DIM);
                let row = |i: usize, k: &str, v: String, col: Color32| {
                    let y = d.top() + 42.0 + 20.0 * i as f32;
                    painter.text(Pos2::new(d.left() + 14.0, y), Align2::LEFT_CENTER, k, mono(10.0), DIM);
                    painter.text(Pos2::new(d.right() - 14.0, y), Align2::RIGHT_CENTER, v, mono(11.5), col);
                };
                match &res {
                    None => {
                        row(0, "KEY", "--".into(), DIM);
                        row(1, "SCALE", "--".into(), DIM);
                        row(2, "REF A4", "--".into(), DIM);
                    }
                    Some(r) => {
                        let k = r.keys[0];
                        let col = if listening { DIM } else { WHITE };
                        row(0, "KEY", note(k.tonic, flats).to_string(), col);
                        row(1, "SCALE", tune_scale(&k, &r.chroma).to_string(), col);
                        row(2, "REF A4", format!("{:.1} Hz", key::ref_a4(r.tuning_c)), col);
                        painter.hline(d.left() + 14.0..=d.right() - 14.0, d.top() + 112.0, Stroke::new(1.0, LINE));
                        painter.text(Pos2::new(d.left() + 14.0, d.top() + 126.0), Align2::LEFT_CENTER, "ALSO POSSIBLE", mono(9.5), DIM);
                        for (i, alt) in r.keys.iter().skip(1).take(2).enumerate() {
                            let y = d.top() + 146.0 + 18.0 * i as f32;
                            painter.text(Pos2::new(d.left() + 14.0, y), Align2::LEFT_CENTER, key_name(alt, flats), mono(10.5), Color32::from_gray(190));
                            let m = (alt.score / k.score.max(1e-6) * 100.0).clamp(0.0, 100.0);
                            painter.text(Pos2::new(d.right() - 14.0, y), Align2::RIGHT_CENTER, format!("{:.0} %", m), mono(10.0), DIM);
                        }
                    }
                }

                resize_grip(ui, ctx, &size_state, full, BASE_W, BASE_H, 600.0);
            });
        },
    )
}
