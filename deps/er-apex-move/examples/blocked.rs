//! Replays a spot where walking was blocked (er-apex's `kcc_blocked_<n>.txt`: the window, the state
//! before the step and the input): pushes with the recorded input for a second and says how far he
//! got. `cargo run --release --example blocked -- <kcc_blocked_n.txt> [ticks]`; BLOCKED_TRACE=1
//! prints every tick.
use er_apex_move::{Controller, MoveInput, MoveParams, Triangle, Vec3, World, FIXED_DT};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: blocked <kcc_blocked_n.txt> [ticks]");
    let ticks: usize = args.next().and_then(|t| t.parse().ok()).unwrap_or(60);
    let text = std::fs::read_to_string(&path).expect("read the dump");
    let nums = |l: &str| l.split_whitespace().filter_map(|x| x.parse::<f32>().ok()).collect::<Vec<_>>();
    let mut lines = text.lines();
    let head = nums(lines.next().unwrap());
    let state = nums(lines.next().unwrap());
    let input = nums(&lines.next().unwrap()["input".len()..]);
    let tris: Vec<Triangle> = lines
        .map(nums)
        .filter(|v| v.len() == 9)
        .map(|v| [Vec3::new(v[0], v[1], v[2]), Vec3::new(v[3], v[4], v[5]), Vec3::new(v[6], v[7], v[8])])
        .collect();
    let (mpu, gravity, slope, mult) = (head[0], head[1], head[2], head[3]);
    let params = MoveParams {
        base_gravity: Some(gravity / mpu),
        speed_multiplier: Some(mult),
        meters_per_unit: Some(mpu),
        max_slope_radians: Some(slope.to_radians()),
        collision_iterations: 5,
        ..MoveParams::default()
    };
    let start = Vec3::new(state[0], state[1], state[2]);
    let mut c = Controller::new(World::from_triangles(&tris).unwrap(), params, start).unwrap();
    let wish = Vec3::new(input[0], input[1], input[2]);
    let mi = MoveInput { wish, forward: wish, sprint: input[3] > 0.5, ..Default::default() };
    for _ in 0..5 {
        c.step(&MoveInput::default(), FIXED_DT).unwrap();
    }
    let from = c.state.position;
    for tick in 0..ticks {
        let before = c.state.position;
        c.step(&mi, FIXED_DT).unwrap();
        if std::env::var("BLOCKED_TRACE").is_ok() {
            println!(
                "{tick:3} at {:.2?} moved {:.2} v {:.1?} grounded {} normal {:.3?} {:?}",
                c.state.position,
                (c.state.position - before).norm(),
                c.state.velocity,
                c.state.grounded,
                c.state.ground_normal,
                c.state.pose
            );
        }
    }
    let moved = c.state.position - from;
    println!(
        "{path}: {} tris, start {:.1?}, pushing {:.2?} sprint {}: moved {:.1} units in {ticks} ticks ({:.2} m), ended {:.1?} grounded {}",
        tris.len(),
        from,
        wish,
        mi.sprint,
        Vec3::new(moved.x, 0.0, moved.z).norm(),
        Vec3::new(moved.x, 0.0, moved.z).norm() * mpu,
        c.state.position,
        c.state.grounded
    );
}
