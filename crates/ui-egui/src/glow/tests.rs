use super::*;
use egui::vec2;
use mareader_appearance::Appearance;

fn light() -> Light {
    Light::new(&Appearance::default().glow_palette())
}

fn look(head: f32) -> Look {
    Look {
        head,
        drift: 0.0,
        strength: 1.0,
        whole: 0.0,
        light: light(),
        accent: false,
    }
}

#[test]
fn the_palette_turns_without_a_seam() {
    let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.02);
    let l = light();
    assert!(close(l.hue(0.0), l.hue(1.0)));
    assert!(close(l.hue(0.999), l.hue(0.0)));
    assert!(close(l.hue(-0.25), l.hue(0.75)));
}

#[test]
fn a_ring_closes_and_its_shares_climb_once() {
    let sizes = [
        (vec2(8.0, 14.0), 0.0),
        (vec2(60.0, 18.0), 3.0),
        (vec2(300.0, 40.0), 20.0),
    ];
    for (size, radius) in sizes {
        let ring = Ring::new(Rect::from_min_size(Pos2::ZERO, size), radius);
        assert!(ring.shares.windows(2).all(|w| w[0] < w[1]), "{size:?}");
        assert!(ring.shares.last().is_some_and(|s| *s < 1.0));
        let first = ring.points.first().map(|p| p.0);
        assert_eq!(first, Some(pos2(radius, 0.0)), "{size:?}");
    }
}

#[test]
fn a_small_ring_lights_all_round_and_a_long_one_an_arc() {
    let small = (0..20).map(|i| look(0.0).at(i as f32 / 20.0, 60.0).1);
    assert!(small.fold(1.0_f32, f32::min) > 0.5);
    let far = look(0.0).at(0.5, 800.0).1;
    let near = look(0.0).at(0.0, 800.0).1;
    assert!(far < 0.15 && near > 0.99, "{far} {near}");
}

#[test]
fn a_finish_lights_the_whole_ring() {
    let done = Look {
        whole: 1.0,
        ..look(0.0)
    };
    for i in 0..40 {
        let share = i as f32 / 40.0;
        assert!((done.at(share, 800.0).1 - 0.85).abs() < 1e-4);
    }
    let later = Look {
        drift: 0.04,
        ..done
    };
    assert_ne!(done.at(0.5, 800.0).0, later.at(0.5, 800.0).0);
    assert_eq!(done.at(0.5, 800.0).1, later.at(0.5, 800.0).1);
}

#[test]
fn reduced_motion_holds_the_flare_and_snaps_focus_and_close() {
    let ctx = Context::default();
    crate::motion::set(&ctx, &mareader_settings::MotionSettings::default(), true);
    let tokens = Tokens::new(&Appearance::default());
    let id = Id::new("reduced-glow-test");
    let mut looks = Vec::new();
    for time in [0.0, 0.5] {
        let mut out = ctx.run_ui(
            egui::RawInput {
                time: Some(time),
                ..Default::default()
            },
            |ui| {
                let ctx = ui.ctx();
                let playing = retained_state(ctx, id, true, false).unwrap();
                assert_eq!(
                    playing,
                    Glow {
                        strength: 1.0,
                        whole: 0.0
                    }
                );
                let look = look_at(ctx, &tokens, playing, 500.0, time);
                looks.push((look.head, look.drift, look.strength));
            },
        );
        out.textures_delta.clear();
    }
    assert_eq!(looks[0].0, looks[1].0);
    assert_eq!(looks[0].1, 0.0);
    assert_eq!(looks[1].1, 0.0);
    assert_ne!(looks[0].2, looks[1].2, "reduced motion breathes");
    let mut out = ctx.run_ui(
        egui::RawInput {
            time: Some(1.0),
            ..Default::default()
        },
        |ui| {
            let ctx = ui.ctx();
            let focused = retained_state(ctx, id, false, true).unwrap();
            assert_eq!(
                focused,
                Glow {
                    strength: 1.0,
                    whole: 1.0
                }
            );
            assert!(retained_state(ctx, id, false, false).is_none());
        },
    );
    out.textures_delta.clear();
}

#[test]
fn accent_arc_moves_even_around_short_words_and_has_no_solid_ring() {
    let look = Look {
        accent: true,
        ..look(0.0)
    };
    assert!(look.at(0.0, 60.0).1 > 0.99);
    assert_eq!(look.at(0.5, 60.0).1, 0.0);
    let moved = Look { head: 0.5, ..look };
    assert!(moved.at(0.5, 60.0).1 > 0.99);
    assert_eq!(moved.at(0.0, 60.0).1, 0.0);
}
