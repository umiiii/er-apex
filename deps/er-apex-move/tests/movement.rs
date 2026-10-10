#[path = "../examples/support/mod.rs"]
mod support;
use er_apex_move::{Controller, LedgeKind, MoveInput, MoveParams, Pose, Triangle, Vec3, World, FIXED_DT};
use support::*;

fn forward() -> MoveInput {
    MoveInput {
        wish: Vec3::x(),
        ..Default::default()
    }
}
fn frames(c: &mut Controller, input: MoveInput, n: usize) {
    for _ in 0..n {
        assert_eq!(c.step(&input, FIXED_DT).unwrap(), 1);
    }
}

#[test]
fn unknown_scales_require_explicit_values() {
    assert!(Controller::new(World::default(), MoveParams::default(), Vec3::zeros()).is_err());
    assert!(MoveParams::default().to_meters(Vec3::x()).is_err());
    let p = demo_params();
    assert!(
        (p.from_meters(p.to_meters(Vec3::new(3.0, 4.0, 5.0)).unwrap())
            .unwrap()
            - Vec3::new(3.0, 4.0, 5.0))
        .norm()
            < 1.0e-5
    );
}

#[test]
fn flat_walk_sprint_time_and_terminal_speed() {
    let mut c = controller(&floor(0.0), Vec3::zeros());
    assert!(c.state.grounded);
    let mut time = 0;
    while c.state.velocity.x < 173.49 && time < 200 {
        frames(&mut c, forward(), 1);
        time += 1;
    }
    // lowAcceleration 2500 below lowSpeed 120 (3 ticks), then 450 (7 ticks): spec section 7
    assert_eq!(time, 10);
    assert!((c.state.velocity.x - 173.5).abs() < 0.01);
    let mut input = forward();
    input.sprint = true;
    frames(&mut c, input, 1);
    assert!(c.state.sprinting); // at once, no start delay (spec section 5)
    let mut acceleration_ticks = 1;
    while c.state.velocity.x < 259.99 && acceleration_ticks < 100 {
        frames(&mut c, input, 1);
        acceleration_ticks += 1;
    }
    assert_eq!(acceleration_ticks, 52); // ceil((260-173.5)/(100/60)).
    frames(&mut c, input, 60);
    assert!((c.state.velocity.x - 260.0).abs() < 0.01);
    frames(&mut c, MoveInput::default(), 13);
    assert!(c.state.velocity.norm() < 0.001);
    println!("walk reaches 173.5 at 0.167 s; sprint ramp 0.867 s; terminal 260");
}

/// A shield battery out (Apex `offhand_blocks_sprint`): no sprint while it is, the sticky sprint
/// is gone after it (a released key does not bring it back); held, the key sprints again at once.
#[test]
fn blocked_sprint_drops_the_sticky_sprint() {
    let mut c = controller(&floor(0.0), Vec3::zeros());
    frames(&mut c, forward(), 20);
    let mut press = forward();
    press.sprint = true;
    frames(&mut c, press, 1);
    frames(&mut c, forward(), 10);
    assert!(c.state.sprinting, "sticky sprint after a press");
    let mut blocked = forward();
    blocked.sprint_blocked = true;
    frames(&mut c, blocked, 30);
    assert!(!c.state.sprinting);
    frames(&mut c, forward(), 10);
    assert!(!c.state.sprinting, "the sticky sprint must not come back");
    let mut held = press;
    held.sprint_blocked = true;
    frames(&mut c, held, 5);
    assert!(!c.state.sprinting);
    held.sprint_blocked = false;
    frames(&mut c, held, 1);
    assert!(c.state.sprinting, "a held key sprints once unblocked");
}

/// Octane's stim (S3): the `speed_boost` severity 52/255 scales the move speeds by 1 + 2 x it
/// (sprint terminal 260 -> 366) and slows the slide decay (exponent 1 - 0.75 x it).
#[test]
fn speed_boost_scales_speed_and_slows_slide_decay() {
    let boost = 52.0 / 255.0;
    let mut c = controller(&floor(0.0), Vec3::zeros());
    let stim = MoveInput {
        sprint: true,
        speed_boost: boost,
        ..forward()
    };
    frames(&mut c, stim, 240);
    let want = 260.0 * (1.0 + 2.0 * boost);
    assert!(
        (c.state.velocity.x - want).abs() < 0.05,
        "{} vs {want}",
        c.state.velocity.x
    );
    // back to normal: the sprint settles to 260 again
    frames(
        &mut c,
        MoveInput {
            sprint: true,
            ..forward()
        },
        240,
    );
    assert!(
        (c.state.velocity.x - 260.0).abs() < 0.05,
        "{}",
        c.state.velocity.x
    );
    let slide = |boost: f32| {
        let mut c = controller(&floor(0.0), Vec3::zeros());
        c.state.velocity = Vec3::x() * 260.0;
        frames(
            &mut c,
            MoveInput {
                crouch: true,
                speed_boost: boost,
                ..forward()
            },
            45,
        );
        c.state.velocity.x
    };
    let (plain, stimmed) = (slide(0.0), slide(boost));
    assert!(stimmed > plain + 5.0, "stimmed slide {stimmed} vs {plain}");
    assert!(MoveInput {
        speed_boost: f32::NAN,
        ..forward()
    }
    .speed_boost
    .is_nan());
    assert!(c
        .step(
            &MoveInput {
                speed_boost: f32::NAN,
                ..forward()
            },
            FIXED_DT
        )
        .is_err());
}

/// Octane's launch pad (S3, plan-octane R1/R3): a launch sets the velocity and leaves the ground;
/// the granted double jump at the apex adds sqrt(2 g 150), only once; pressed early while still
/// rising fast it adds a quarter of the height on top; landing takes it away.
#[test]
fn launch_and_double_jump() {
    let g = demo_params().gravity();
    let launch = Vec3::new(507.0, 862.0, 0.0);
    let mut c = controller(&floor(0.0), Vec3::zeros());
    c.launch(launch, true, 1.0).unwrap();
    assert!(c.take_events().launched);
    frames(&mut c, MoveInput::default(), 1);
    assert!(!c.state.grounded);
    // up to the apex
    let mut n = 0;
    while c.state.velocity.y > 0.0 && n < 200 {
        frames(&mut c, MoveInput::default(), 1);
        n += 1;
    }
    let apex = c.state.position.y;
    assert!(
        (apex - 862.0f32.powi(2) / (2.0 * g)).abs() < 10.0,
        "apex {apex}"
    );
    let vy = c.state.velocity.y;
    let jump = MoveInput {
        jump: true,
        ..Default::default()
    };
    frames(&mut c, jump, 1);
    let want = (2.0 * g * 150.0).sqrt() - g * FIXED_DT;
    assert!(
        (c.state.velocity.y - want).abs() < 2.0,
        "{} vs {want} (was {vy})",
        c.state.velocity.y
    );
    assert!(c.take_events().double_jumped);
    // only once
    frames(&mut c, MoveInput::default(), 1);
    let before = c.state.velocity.y;
    frames(&mut c, jump, 1);
    assert!(c.state.velocity.y < before && !c.take_events().double_jumped);

    // early, still rising fast: a quarter of the height on top of the speed
    let mut c = controller(&floor(0.0), Vec3::zeros());
    c.launch(launch, true, 1.0).unwrap();
    frames(&mut c, MoveInput::default(), 2);
    let vy = c.state.velocity.y;
    frames(&mut c, jump, 1);
    let want = vy + (2.0 * g * 37.5).sqrt() - g * FIXED_DT;
    assert!(
        (c.state.velocity.y - want).abs() < 2.0,
        "{} vs {want}",
        c.state.velocity.y
    );

    // landing takes the double jump away; no launch, no double jump
    let mut c = controller(&floor(0.0), Vec3::zeros());
    c.launch(Vec3::new(0.0, 200.0, 0.0), true, 1.0).unwrap();
    frames(&mut c, MoveInput::default(), 90);
    assert!(c.state.grounded);
    frames(&mut c, jump, 1);
    frames(&mut c, MoveInput::default(), 20);
    let before = c.state.velocity.y;
    frames(&mut c, jump, 1);
    assert!(
        c.state.velocity.y < before,
        "a second jump in the air without a launch"
    );
    assert!(c.launch(Vec3::new(f32::NAN, 0.0, 0.0), true, 1.0).is_err());
}

/// R5R's community launch pad (D-029, not Season 3's code): gravity x0.75 from the launch until
/// landing, so the flight rises 1/0.75 as high and the double jump keeps its height; on the ground
/// again, a plain jump has full gravity.
#[test]
fn launch_gravity_scale_lasts_until_landing() {
    let g = demo_params().gravity();
    let rise = |scale: f32| {
        let mut c = controller(&floor(0.0), Vec3::zeros());
        c.launch(Vec3::new(0.0, 600.0, 0.0), false, scale).unwrap();
        let mut top = 0.0f32;
        for _ in 0..400 {
            frames(&mut c, MoveInput::default(), 1);
            top = top.max(c.state.position.y);
            if c.state.grounded {
                break;
            }
        }
        assert!(c.state.grounded, "landed");
        (top, c)
    };
    let (full, _) = rise(1.0);
    let (light, mut c) = rise(0.75);
    let want = 600.0f32.powi(2) / (2.0 * g);
    assert!((full - want).abs() < 10.0, "apex {full} vs {want}");
    assert!(
        (light - want / 0.75).abs() < 12.0,
        "apex {light} vs {}",
        want / 0.75
    );
    // after landing: a plain jump climbs the jump height with full gravity
    frames(&mut c, MoveInput::default(), 30);
    let jump = MoveInput {
        jump: true,
        ..Default::default()
    };
    frames(&mut c, jump, 1);
    let mut top = 0.0f32;
    for _ in 0..120 {
        frames(&mut c, MoveInput::default(), 1);
        top = top.max(c.state.position.y);
    }
    let height = demo_params().jump_height;
    assert!(
        (top - height).abs() < 3.0,
        "jump {top} vs {height} after the light launch"
    );
    // the double jump keeps its height under the lighter gravity: sqrt(2 x 0.75 g x 150)
    let mut c = controller(&floor(0.0), Vec3::zeros());
    c.launch(Vec3::new(0.0, 600.0, 0.0), true, 0.75).unwrap();
    while c.state.velocity.y > 0.0 {
        frames(&mut c, MoveInput::default(), 1);
    }
    frames(&mut c, jump, 1);
    let want = (2.0 * 0.75 * g * 150.0).sqrt() - 0.75 * g * FIXED_DT;
    assert!(
        (c.state.velocity.y - want).abs() < 2.0,
        "{} vs {want}",
        c.state.velocity.y
    );
    let mut c = controller(&floor(0.0), Vec3::zeros());
    assert!(c.launch(Vec3::new(0.0, 600.0, 0.0), true, 0.0).is_err());
    assert!(c
        .launch(Vec3::new(0.0, 600.0, 0.0), true, f32::NAN)
        .is_err());
}

#[test]
fn crouch_speed_and_ceiling_prevent_standing() {
    let mut terrain = floor(0.0);
    terrain.extend(floor(55.0));
    let mut c = controller(&floor(0.0), Vec3::zeros());
    let input = MoveInput {
        crouch: true,
        ..forward()
    };
    frames(&mut c, input, 30);
    assert!((c.state.velocity.x - 80.0).abs() < 0.01);
    c.world_mut().replace_triangles(&terrain).unwrap();
    frames(&mut c, MoveInput::default(), 1);
    assert!(c.state.crouched);
    assert_eq!(c.state.pose, Pose::Crouching);
}

#[test]
fn step_22_works_30_blocks() {
    for (height, can_climb) in [(22.0, true), (30.0, false)] {
        let mut c = controller(&step(height), Vec3::new(50.0, 0.0, 0.0));
        frames(&mut c, forward(), 120);
        if can_climb {
            assert!(c.state.position.x > 180.0, "22 step: {:?}", c.state);
            assert!((c.state.position.y - height).abs() < 0.1);
            assert!(c.state.grounded);
        } else {
            assert!(c.state.position.x < 100.0, "30 step: {:?}", c.state);
            assert!(
                c.state.position.y < 1.0,
                "30 step unexpectedly climbed: {:?}",
                c.state
            );
        }
    }
}

/// Stairs with treads narrower than the capsule (Elden Ring's: ~0.22 m rise, ~0.3 m tread, in raw
/// units 9 and 12 against a radius of 16): the capsule's round bottom meets each stair's nose, a
/// slanted contact that must be stepped over, not ridden up (the feet caught on every step,
/// 2026-10-10).
fn stairs(rise: f32, tread: f32, n: usize) -> Vec<Triangle> {
    let x0 = 100.0;
    let mut t = quad(
        Vec3::new(-1000.0, 0.0, -5000.0),
        Vec3::new(x0, 0.0, -5000.0),
        Vec3::new(x0, 0.0, 5000.0),
        Vec3::new(-1000.0, 0.0, 5000.0),
    );
    for i in 0..n {
        let (x, y) = (x0 + i as f32 * tread, i as f32 * rise);
        t.extend(quad(
            Vec3::new(x, y, -5000.0),
            Vec3::new(x, y + rise, -5000.0),
            Vec3::new(x, y + rise, 5000.0),
            Vec3::new(x, y, 5000.0),
        ));
        let end = if i + 1 == n { x + 10000.0 } else { x + tread };
        t.extend(quad(
            Vec3::new(x, y + rise, -5000.0),
            Vec3::new(end, y + rise, -5000.0),
            Vec3::new(end, y + rise, 5000.0),
            Vec3::new(x, y + rise, 5000.0),
        ));
    }
    t
}

#[test]
fn narrow_stairs_climb_without_catching() {
    // Elden Ring-like stairs (0.22-0.3 m rise, 0.3 m tread), walking and sprinting, straight on and
    // at an angle: up to the top and on (all of these stopped on a step before the edge fix)
    for (rise, tread) in [(9.0, 12.0), (12.0, 18.0), (9.0, 6.0), (15.0, 12.0)] {
        for (sprint, angle) in [(false, 0.0f32), (true, 0.0), (true, 30.0)] {
            let n = 10;
            let mut c = controller(&stairs(rise, tread, n), Vec3::new(50.0, 0.0, 0.0));
            let a = angle.to_radians();
            let dir = Vec3::new(a.cos(), 0.0, a.sin());
            let input = MoveInput { wish: dir, forward: dir, sprint, ..Default::default() };
            frames(&mut c, input, 600);
            let top = rise * n as f32;
            assert!((c.state.position.y - top).abs() < 0.5, "stairs {rise}x{tread} sprint {sprint} angle {angle}: {:?}", c.state);
            assert!(c.state.position.x > 100.0 + tread * n as f32 + 50.0, "stairs {rise}x{tread}: {:?}", c.state);
        }
    }
}

#[test]
fn stepping_down_stays_grounded() {
    let mut c = controller(&step(22.0), Vec3::new(200.0, 22.0, 0.0));
    let input = MoveInput {
        wish: -Vec3::x(),
        ..Default::default()
    };
    for _ in 0..100 {
        frames(&mut c, input, 1);
        assert!(c.state.grounded, "lost grounding on step: {:?}", c.state);
    }
    assert!(c.state.position.x < 60.0);
    assert!(c.state.position.y < 0.1);
}

#[test]
fn walkable_slope_and_steep_slope_slide() {
    let mut walkable = controller(&ramp(30.0), ramp_feet(30.0, 0.0));
    frames(&mut walkable, forward(), 60);
    assert!(walkable.state.grounded, "{:?}", walkable.state);
    assert!(walkable.state.position.x > 100.0);
    assert!(
        walkable
            .state
            .velocity
            .dot(&walkable.state.ground_normal.unwrap())
            .abs()
            < 0.001
    );
    assert!((walkable.state.ground_normal.unwrap().y - 30.0_f32.to_radians().cos()).abs() < 0.001);
    let mut steep = controller(&ramp(60.0), ramp_feet(60.0, 0.0));
    assert!(!steep.state.grounded);
    frames(&mut steep, MoveInput::default(), 60);
    assert!(steep.state.position.x < -50.0, "{:?}", steep.state);
    assert!(!steep.state.grounded);
}

#[test]
fn jump_apex_height_and_button_edge() {
    let mut c = controller(&floor(0.0), Vec3::zeros());
    let initial = c.state.position.y;
    let input = MoveInput {
        jump: true,
        ..Default::default()
    };
    let mut max_y = initial;
    for _ in 0..120 {
        frames(&mut c, input, 1);
        max_y = max_y.max(c.state.position.y);
    }
    let height = max_y - initial;
    // Apex launches half a gravity step lower than sqrt(2gh) (spec section 15)
    let g = demo_params().gravity();
    let launch = (2.0 * g * 56.0).sqrt() - 0.5 * g * FIXED_DT;
    let expected = launch * launch / (2.0 * g);
    assert!(
        (height - expected).abs() / expected < 0.01,
        "apex {height}, expected {expected}"
    );
    assert!(c.state.grounded);
    assert!(c.state.position.y < 0.1); // Held jump did not trigger a second jump.
    println!("jump apex={height:.5} raw units (target 56)");
}

#[test]
fn slide_boost_cap_cooldown_and_jump() {
    let mut c = controller(&floor(0.0), Vec3::zeros());
    c.state.velocity = Vec3::x() * 260.0;
    let slide = MoveInput {
        crouch: true,
        ..forward()
    };
    frames(&mut c, slide, 1);
    assert_eq!(c.state.pose, Pose::Sliding);
    assert!(c.state.velocity.x > 390.0 && c.state.velocity.x <= 400.0);
    frames(&mut c, forward(), 1);
    let before = c.state.velocity.x;
    frames(&mut c, slide, 1);
    assert!(c.state.sliding);
    assert!(
        c.state.velocity.x < before,
        "boost repeated during cooldown"
    );
    let jump = MoveInput {
        jump: true,
        ..slide
    };
    frames(&mut c, jump, 1);
    assert!(!c.state.grounded);
    assert!(c.state.velocity.x <= 350.01);
    let base = 0.02;
    let mut apex = c.state.position.y;
    for _ in 0..60 {
        frames(&mut c, MoveInput::default(), 1);
        apex = apex.max(c.state.position.y);
    }
    let g = demo_params().gravity();
    let launch = (2.0 * g * 50.0).sqrt() - 0.5 * g * FIXED_DT;
    assert!(
        (apex - base - launch * launch / (2.0 * g)).abs() < 0.5,
        "apex {apex}"
    );
    frames(&mut c, MoveInput::default(), 120);
    assert_eq!(c.slide_boost_remaining(), 0.0);
    c.state.velocity = Vec3::x() * 260.0;
    frames(&mut c, slide, 1);
    assert!(c.state.velocity.x > 390.0);
}

#[test]
fn slide_boost_does_not_reduce_above_cap_momentum() {
    let mut c = controller(&floor(0.0), Vec3::zeros());
    c.state.velocity.x = 500.0;
    frames(
        &mut c,
        MoveInput {
            crouch: true,
            ..forward()
        },
        1,
    );
    assert!(c.state.velocity.x > 490.0);
}

#[test]
fn downhill_slide_accelerates_and_stops_on_flat() {
    let mut downhill = controller(&ramp(-30.0), ramp_feet(-30.0, 0.0));
    downhill.state.velocity.x = 260.0;
    let slide = MoveInput {
        crouch: true,
        ..forward()
    };
    frames(&mut downhill, slide, 60);
    assert!(downhill.state.velocity.x > 400.0, "{:?}", downhill.state);
    let mut flat = controller(&floor(0.0), Vec3::zeros());
    flat.state.velocity.x = 260.0;
    frames(&mut flat, slide, 180);
    assert!(!flat.state.sliding);
    assert_eq!(flat.state.pose, Pose::Crouching);
}

#[test]
fn wall_slide_and_400_speed_thin_wall_no_tunneling() {
    let mut terrain = floor(0.0);
    terrain.extend(wall_x(100.0));
    let mut c = controller(&terrain, Vec3::zeros());
    let diagonal = MoveInput {
        wish: Vec3::new(1.0, 0.0, 1.0),
        ..Default::default()
    };
    frames(&mut c, diagonal, 120);
    assert!(
        c.state.position.x <= 84.05 && c.state.position.z > 100.0,
        "{:?}",
        c.state
    );
    let mut fast = controller(&terrain, Vec3::new(80.0, 0.0, 0.0));
    fast.state.velocity.x = 400.0;
    frames(
        &mut fast,
        MoveInput {
            crouch: true,
            ..forward()
        },
        1,
    );
    assert!(fast.state.position.x < 84.05, "tunneled: {:?}", fast.state);
    frames(
        &mut fast,
        MoveInput {
            crouch: true,
            ..forward()
        },
        120,
    );
    assert!(fast.state.position.x < 84.05);
}

#[test]
fn concave_corner_can_back_out() {
    let mut terrain = floor(0.0);
    terrain.extend(wall_x(100.0));
    terrain.extend(wall_z(100.0));
    let mut c = controller(&terrain, Vec3::zeros());
    frames(
        &mut c,
        MoveInput {
            wish: Vec3::new(1.0, 0.0, 1.0),
            ..Default::default()
        },
        120,
    );
    assert!(c.state.position.x <= 84.05 && c.state.position.z <= 84.05);
    let corner = c.state.position;
    frames(
        &mut c,
        MoveInput {
            wish: Vec3::new(-1.0, 0.0, -1.0),
            ..Default::default()
        },
        60,
    );
    assert!(
        c.state.position.x < corner.x - 80.0 && c.state.position.z < corner.z - 80.0,
        "corner stuck: {:?}",
        c.state
    );
    assert!(!c.state.stuck);
}

#[test]
fn air_wish_speed_caps_component_without_erasing_momentum() {
    let mut c = controller(&[], Vec3::new(0.0, 1000.0, 0.0));
    c.state.velocity.x = 300.0;
    frames(
        &mut c,
        MoveInput {
            wish: Vec3::z(),
            ..Default::default()
        },
        60,
    );
    // Air acceleration reaches the 60 wish speed sideways, then player_extraairaccelleration
    // only turns the motion: the speed stays (spec section 20).
    let h = Vec3::new(c.state.velocity.x, 0.0, c.state.velocity.z);
    assert!(
        (h.norm() - (300.0f32 * 300.0 + 60.0 * 60.0).sqrt()).abs() < 0.01,
        "{h:?}"
    );
    assert!(
        c.state.velocity.z > 60.0 && c.state.velocity.z < 62.0,
        "{h:?}"
    );
}

#[test]
fn mantle_climb_detection_and_safe_execution() {
    for (height, kind) in [(70.0, LedgeKind::Mantle), (90.0, LedgeKind::Climb)] {
        let mut c = controller(&step(height), Vec3::new(80.0, 0.0, 0.0));
        let ledge = c.detect_ledge(Vec3::x()).expect("ledge should be found");
        assert_eq!(ledge.kind, kind);
        assert!((ledge.edge_position.y - height).abs() < 0.1);
        assert!((ledge.edge_position.x - 100.0).abs() < 0.1);
        assert!(c.execute_ledge(Vec3::x()));
        assert_eq!(c.state.pose, Pose::Mantling);
        assert!(c.state.position.x > 100.0 && (c.state.position.y - height).abs() < 0.1);
        frames(&mut c, MoveInput::default(), 1);
        assert_eq!(c.state.pose, Pose::Standing);
    }
    let c = controller(&step(110.0), Vec3::new(80.0, 0.0, 0.0));
    assert!(c.detect_ledge(Vec3::x()).is_none());
    let mut blocked = step(70.0);
    blocked.extend(floor(120.0));
    let c = controller(&blocked, Vec3::new(80.0, 0.0, 0.0));
    assert!(c.detect_ledge(Vec3::x()).is_none());
}

#[test]
fn fixed_tick_accumulator_and_latched_jump() {
    let mut a = controller(&floor(0.0), Vec3::zeros());
    let mut b = controller(&floor(0.0), Vec3::zeros());
    for _ in 0..30 {
        a.step(&forward(), 2.0 * FIXED_DT).unwrap();
    }
    frames(&mut b, forward(), 60);
    assert!((a.state.position - b.state.position).norm() < 1.0e-5);
    let mut c = controller(&floor(0.0), Vec3::zeros());
    assert_eq!(
        c.step(
            &MoveInput {
                jump: true,
                ..Default::default()
            },
            FIXED_DT * 0.25
        )
        .unwrap(),
        0
    );
    assert_eq!(c.step(&MoveInput::default(), FIXED_DT * 0.75).unwrap(), 1);
    assert!(!c.state.grounded);
    let previous = c.state;
    assert!(c.step(&MoveInput::default(), f32::NAN).is_err());
    assert_eq!(previous.position, c.state.position);
}

#[test]
fn streamed_replacement_removes_geometry_and_failed_update_is_atomic() {
    let mut w = World::from_triangles(&floor(0.0)).unwrap();
    w.update_chunks(&[(1, &wall_x(100.0))], &[]).unwrap();
    assert_eq!(w.triangle_count(), 4);
    let revision = w.revision();
    let invalid = [[Vec3::new(f32::NAN, 0.0, 0.0), Vec3::x(), Vec3::z()]];
    assert!(w.update_chunks(&[(2, &invalid)], &[0]).is_err());
    assert_eq!(w.revision(), revision);
    assert_eq!(w.triangle_count(), 4);
    w.update_chunks(&[], &[1]).unwrap();
    assert_eq!(w.triangle_count(), 2);
    let mut c = Controller::new(w, demo_params(), Vec3::zeros()).unwrap();
    c.world_mut().replace_triangles(&[]).unwrap();
    frames(&mut c, MoveInput::default(), 2);
    assert!(!c.state.grounded && c.state.velocity.y < 0.0);
    c.world_mut().replace_triangles(&floor(0.0)).unwrap();
    frames(&mut c, MoveInput::default(), 1);
    assert!(c.state.grounded && !c.state.stuck);
}

#[test]
fn exact_400_airborne_sweep_and_ceiling_collision() {
    let mut terrain = floor(0.0);
    terrain.extend(wall_x(100.0));
    terrain.extend(floor(150.0));
    let mut c = controller(&terrain, Vec3::new(80.0, 40.0, 0.0));
    c.state.velocity.x = 400.0;
    frames(&mut c, forward(), 1);
    assert!(c.state.position.x < 84.05, "{:?}", c.state);
    let mut jumping = controller(&floor(0.0), Vec3::zeros());
    jumping
        .world_mut()
        .replace_triangles(&[floor(0.0), floor(100.0)].concat())
        .unwrap();
    frames(
        &mut jumping,
        MoveInput {
            jump: true,
            ..Default::default()
        },
        10,
    );
    assert!(jumping.state.position.y <= 28.05);
    assert!(jumping.state.velocity.y <= 0.0);
}

#[test]
fn coplanar_triangle_seams_do_not_lose_ground_or_speed() {
    let mut triangles = Vec::new();
    for x in -2..30 {
        for z in -2..3 {
            let a = Vec3::new(x as f32 * 32.0, 0.0, z as f32 * 32.0);
            triangles.extend(quad(
                a,
                a + Vec3::x() * 32.0,
                a + (Vec3::x() + Vec3::z()) * 32.0,
                a + Vec3::z() * 32.0,
            ));
        }
    }
    let mut c = controller(&triangles, Vec3::zeros());
    for _ in 0..180 {
        frames(&mut c, forward(), 1);
        assert!(c.state.grounded, "{:?}", c.state);
    }
    assert!((c.state.velocity.x - 173.5).abs() < 0.01);
}

#[test]
fn support_probe_does_not_snap_through_steep_geometry() {
    let mut terrain = ramp(60.0);
    terrain.extend(floor(0.0));
    let c = controller(&terrain, ramp_feet(60.0, 0.0));
    assert!(!c.state.grounded, "{:?}", c.state);
}

#[test]
fn replacement_during_rising_jump_keeps_airborne_state() {
    let mut c = controller(&floor(0.0), Vec3::zeros());
    frames(
        &mut c,
        MoveInput {
            jump: true,
            ..Default::default()
        },
        1,
    );
    c.world_mut().replace_triangles(&floor(0.0)).unwrap();
    frames(&mut c, MoveInput::default(), 1);
    assert!(!c.state.grounded && c.state.velocity.y > 0.0);
}

#[test]
fn installing_new_world_with_same_revision_rechecks_ground() {
    let mut c = controller(&floor(0.0), Vec3::zeros());
    let empty = World::from_triangles(&[]).unwrap();
    assert_eq!(empty.revision(), c.world().revision());
    c.replace_world(empty);
    assert!(!c.state.grounded);
    frames(&mut c, MoveInput::default(), 1);
    assert!(c.state.velocity.y < 0.0);
}

/// A crease between a wall and a slope too steep to stand on (found in Elden Ring: Fuse dropped
/// between a cell wall and a pile of sacks, 2026-10-04). Neither surface is walkable alone, but
/// together they hold the capsule up: it must rest there (no speed piling up from gravity) and be
/// able to walk along the crease and jump out.
#[test]
fn wedge_between_wall_and_steep_slope() {
    let tan = 65.0_f32.to_radians().tan();
    let mut t = wall_x(40.0);
    // the slope meets the wall's foot at x = 40 and rises towards -x
    t.extend(quad(
        Vec3::new(40.0, 0.0, -1000.0),
        Vec3::new(40.0, 0.0, 1000.0),
        Vec3::new(-200.0, 240.0 * tan, 1000.0),
        Vec3::new(-200.0, 240.0 * tan, -1000.0),
    ));
    let mut c = controller(&t, Vec3::new(24.0, 200.0, 0.0));
    frames(&mut c, MoveInput::default(), 180);
    assert!(
        c.state.velocity.norm() < 1.0,
        "speed piled up in the crease: {:?}",
        c.state.velocity
    );
    assert!(
        c.state.grounded,
        "wedged capsule should stand: {:?}",
        c.state
    );
    let rest = c.state.position;
    // along the crease
    frames(
        &mut c,
        MoveInput {
            wish: Vec3::z(),
            ..Default::default()
        },
        30,
    );
    assert!(
        c.state.position.z - rest.z > 50.0,
        "could not walk along the crease: {:?}",
        c.state.position
    );
    // and out by jumping
    let y0 = c.state.position.y;
    frames(
        &mut c,
        MoveInput {
            jump: true,
            ..Default::default()
        },
        10,
    );
    assert!(
        c.state.position.y > y0 + 20.0,
        "could not jump out: {:?}",
        c.state.position
    );
}


/// Small lumps on a floor (Elden Ring's bones and rocks): a ridge across the way, `h` high, its
/// sides `slope` degrees steep, with a rough top (a peak).
fn lump(h: f32, slope_deg: f32, x: f32) -> Vec<Triangle> {
    let run = h / slope_deg.to_radians().tan();
    let (a, b, c) = (x - run, x, x + run);
    let mut t = Vec::new();
    for z in [-5000.0f32] {
        let z2 = 5000.0;
        t.extend(quad(Vec3::new(a, 0.0, z), Vec3::new(b, h, z), Vec3::new(b, h, z2), Vec3::new(a, 0.0, z2)));
        t.extend(quad(Vec3::new(b, h, z), Vec3::new(c, 0.0, z), Vec3::new(c, 0.0, z2), Vec3::new(b, h, z2)));
    }
    t
}

#[test]
fn small_lumps_do_not_stop_the_feet() {
    // bones and stones, 5 cm to half a metre, steep-sided, crested: walked and sprinted over (all
    // of these stopped him dead before, 2026-10-10)
    for sprint in [false, true] {
        for h in [2.0f32, 6.0, 12.0, 20.0] {
            for slope in [55.0f32, 75.0, 85.0] {
                let mut t = floor(0.0);
                t.extend(lump(h, slope, 150.0));
                let mut c = controller(&t, Vec3::new(50.0, 0.0, 0.0));
                let input = MoveInput { wish: Vec3::x(), forward: Vec3::x(), sprint, ..Default::default() };
                frames(&mut c, input, 240);
                assert!(c.state.position.x > 250.0, "lump {h} at {slope} deg, sprint {sprint}: {:?}", c.state);
            }
        }
    }
}
