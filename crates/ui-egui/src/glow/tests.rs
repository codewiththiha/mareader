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
fn a_text_path_uses_the_same_outward_flare_as_a_ring() {
    let path = [
        pos2(0.0, 0.0),
        pos2(200.0, 0.0),
        pos2(200.0, 40.0),
        pos2(0.0, 40.0),
    ];
    let line = Rect::from_min_size(Pos2::ZERO, vec2(200.0, 14.0)).expand(0.5);
    let mark = Ring::new(line, 2.5);
    for p in [path.to_vec(), path.into_iter().rev().collect()] {
        let ring = Ring::path(&p, reach(line)).expect("a valid closed path");
        let centre = pos2(100.0, 20.0);
        assert!(ring.points.iter().all(|(p, n)| (*p - centre).dot(*n) > 0.0));
        let mesh = ring.mesh(&look(0.0));
        assert_eq!(mesh.vertices.len(), p.len() * (FALLOFF.len() + 1));
        assert_eq!(ring.reach, mark.reach, "the mark's light reach");
    }
    assert!(Ring::path(&[Pos2::ZERO, Pos2::ZERO, Pos2::ZERO], 4.0).is_none());
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
fn a_stepped_selection_never_folds_its_flare_triangles() {
    let line = |x, y, w| Rect::from_min_size(pos2(x, y), vec2(w, 18.0));
    for gap in [0.5, 3.0, 9.0] {
        for (x, w) in [(0.0, 120.0), (25.0, 70.0), (85.0, 60.0)] {
            let lines = [line(0.0, 0.0, 120.0), line(x, 18.0 + gap, w)];
            for seed in 0..16 {
                let points = crate::skill_outline::sample_outline(&lines, seed);
                for reverse in [false, true] {
                    let mut path = points.clone();
                    if reverse {
                        path.reverse();
                    }
                    let ring = Ring::path(&path, 12.0).unwrap();
                    for phase in [0.0, 0.25, 0.5, 0.75] {
                        let mesh = ring.mesh(&look(phase));
                        for triangle in mesh.indices.as_chunks::<3>().0 {
                            let a = mesh.vertices[triangle[0] as usize].pos;
                            let b = mesh.vertices[triangle[1] as usize].pos;
                            let c = mesh.vertices[triangle[2] as usize].pos;
                            let (u, v) = (b - a, c - a);
                            let area = (u.x * v.y - u.y * v.x) * if reverse { -1.0 } else { 1.0 };
                            assert!(
                                area >= -0.001,
                                "folded triangle: gap {gap} x {x} w {w} seed {seed} reverse {reverse} phase {phase} area {area}"
                            );
                        }
                    }
                }
            }
        }
    }
}
