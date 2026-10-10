use crate::{MoveParams, ParamsError, PoseParams, Vec3, World};

/// Lumps up to this share of the step height (raw units above the feet) are ridden over at
/// LUMP_RIDE_ANGLE degrees rather than stepped (推断: the values are ours, not Apex's).
const LOW_LUMP_FRACTION: f32 = 0.5;
const LUMP_RIDE_ANGLE: f32 = 40.0;
pub const FIXED_DT: f32 = 1.0 / 60.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pose {
    #[default]
    Standing,
    Crouching,
    Sliding,
    Airborne,
    Mantling,
}

/// Apex's duck state machine (spec §12): the crouched capsule and speeds only take effect at
/// the end of `Ducking` (a slide takes them at once).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Duck {
    #[default]
    Standing,
    Ducking,
    Ducked,
    Unducking,
}

#[derive(Clone, Copy, Debug)]
pub struct MoveState {
    /// Capsule feet, not center. Raw Apex units, Y-up.
    pub position: Vec3,
    pub velocity: Vec3,
    pub pose: Pose,
    /// Crouched capsule and speeds (Apex FL_DUCKING), independent of the animation pose.
    pub crouched: bool,
    pub duck: Duck,
    /// Eye above the feet, eased between the standing and crouched view heights.
    pub eye_height: f32,
    pub grounded: bool,
    pub ground_normal: Option<Vec3>,
    pub sprinting: bool,
    pub sliding: bool,
    pub ledge: Option<Ledge>,
    /// A streamed replacement/teleport could not be depenetrated within budget.
    pub stuck: bool,
    /// Apex's crouchFraction: 0 standing, 1 crouched, eased like the eye height (gun-motion spec
    /// §2.3). The viewmodel's crouch samples blend by it.
    pub duck_fraction: f32,
    /// Apex's visual sprint fraction, 0..1 (gun-motion spec §4.1).
    pub sprint_fraction: f32,
    /// The sprint's eye offset, raw units: `sprint_view_offset × sin(sprint_fraction·π/2)`. It is
    /// not in `eye_height` (the crouch easing only); the camera adds it.
    pub eye_sprint_offset: f32,
    /// The current slide got the boost and no slide jump has used it (Apex slideLongJumpAllowed;
    /// the slide FOV, gun-motion spec §4.4).
    pub slide_long_jump: bool,
}
impl MoveState {
    pub fn at(position: Vec3) -> Self {
        Self {
            position,
            velocity: Vec3::zeros(),
            pose: Pose::Standing,
            crouched: false,
            duck: Duck::Standing,
            eye_height: 0.0,
            grounded: false,
            ground_normal: None,
            sprinting: false,
            sliding: false,
            ledge: None,
            stuck: false,
            duck_fraction: 0.0,
            sprint_fraction: 0.0,
            eye_sprint_offset: 0.0,
            slide_long_jump: false,
        }
    }
}

/// A landing: how fast the player came down (raw units/s, the vertical speed just before the
/// contact) and whether crouched then (Apex's landing view kick is ×0.2 crouched, gun-motion
/// spec §4.2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Landing {
    pub speed: f32,
    pub crouched: bool,
}

/// What happened since the caller last took them (`Controller::take_events`). One `step` can run
/// several ticks, so the viewmodel animations and camera effects (gun-motion spec §2.2, §4.2)
/// start from these, not from the state at the end of the frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MoveEvents {
    pub jumped: bool,
    /// The fastest landing, if any.
    pub landed: Option<Landing>,
    /// A slide started; `true` if it got the speed boost.
    pub slide_started: Option<bool>,
    /// Sprinting started / ended (both can be set if it did both in one step: `state.sprinting`
    /// tells which came last).
    pub sprint_started: bool,
    pub sprint_ended: bool,
    /// The duck transition started down (`Duck::Ducking`) / up (`Duck::Unducking`).
    pub duck_started: bool,
    pub unduck_started: bool,
    /// The caller launched the player (`Controller::launch`; Octane's launch pad).
    pub launched: bool,
    /// The double jump a launch grants was used.
    pub double_jumped: bool,
}

/// The double jump Octane's launch pad grants (S3 `enable_doublejump`, the air branch of the jump
/// 0x140809CB0; plan-octane R3): `superjumpMinHeight` 150, only 0.25 of it
/// (`superjump_min_height_fraction`) while still rising fast, and no higher than
/// `verticalGainCutoff_doubleJump` 225 over this flight's predicted apex. The two heights are the
/// retail player settings (`pilot_survival_stim`); S3's own values are 待定.
const DOUBLE_JUMP_HEIGHT: f32 = 150.0;
const DOUBLE_JUMP_MIN_FRACTION: f32 = 0.25;
const DOUBLE_JUMP_CUTOFF: f32 = 225.0;
/// A launch's gravity scale ends on the ground, but not within this many seconds of the launch
/// (R5R's community `_jump_pads.gnut`: a frame and 0.1 s, then until on the ground).
const LAUNCH_GRAVITY_MIN_TIME: f64 = 0.1;

#[derive(Clone, Copy, Debug)]
pub struct MoveInput {
    /// World-space horizontal wish vector; length 0..1 gives analog input.
    pub wish: Vec3,
    /// Held sprint button. A press keeps sprinting while moving on (Apex's sticky sprint).
    pub sprint: bool,
    /// Held crouch button (Apex +duck).
    pub crouch: bool,
    /// Toggle-crouch button (Apex +toggle_duck); a press flips the toggle.
    pub crouch_toggle: bool,
    /// Held button; the controller detects a rising edge. No auto-bunnyhop.
    pub jump: bool,
    /// Horizontal view direction: slides and sprints need the wish within reach of it; ledge
    /// detection looks along it. The wish direction is used if this is zero.
    pub forward: Vec3,
    /// Explicit request for the minimal ledge execution, not automatic animation.
    pub traverse: bool,
    /// Apex's `speed_boost` status effect severity, 0..1 (Octane's stim: 52/255): the move scale
    /// times 1 + 2 severity (S3 0x140815400) and a slower slide decay (0x140817C90). 0 normally.
    pub speed_boost: f32,
    /// An offhand that blocks sprinting is out (Apex `offhand_blocks_sprint`, e.g. a shield
    /// battery): no sprint, and the sticky sprint is dropped (a new press is needed after).
    pub sprint_blocked: bool,
}
impl Default for MoveInput {
    fn default() -> Self {
        Self {
            wish: Vec3::zeros(),
            sprint: false,
            crouch: false,
            crouch_toggle: false,
            jump: false,
            forward: Vec3::zeros(),
            traverse: false,
            speed_boost: 0.0,
            sprint_blocked: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgeKind {
    Mantle,
    Climb,
}
#[derive(Clone, Copy, Debug)]
pub struct Ledge {
    pub kind: LedgeKind,
    /// Actual surface point on the top, raw Apex units.
    pub edge_position: Vec3,
    /// Collision-safe capsule feet on the top, including skin offset.
    pub landing_position: Vec3,
    pub normal: Vec3,
}

/// Owns its local terrain snapshot. `world_mut()` permits streamed replacement.
/// Parameters are immutable after validation; construct a new controller to tune.
///
/// Movement rules follow Apex Season 3's own code (D-020, docs/research/apex-slide-spec.md; §
/// numbers below refer to it); collision is this crate's own.
pub struct Controller {
    world: World,
    params: MoveParams,
    pub state: MoveState,
    accumulator: f64,
    /// Simulated seconds since construction.
    time: f64,
    last_jump: bool,
    last_crouch: bool,
    last_crouch_toggle: bool,
    last_sprint: bool,
    last_traverse: bool,
    pending_jump: bool,
    /// Crouch pressed or released since the last tick (clears toggled crouch, §12).
    pending_crouch_change: bool,
    pending_crouch_toggle: bool,
    pending_sprint: bool,
    pending_traverse: bool,
    crouch_held: bool,
    world_revision: u64,
    /// Standing in a crease of unwalkable surfaces last tick (see `tick`).
    wedged: bool,
    /// Duck/unduck transition left, whole milliseconds as Apex counts them.
    duck_ms: i32,
    duck_total_ms: i32,
    duck_toggle: bool,
    /// Ducking began in the air: the feet tuck up instead of the head coming down.
    half_duck: bool,
    sticky_sprint: bool,
    sticky_sprint_until: f64,
    last_slide_time: f64,
    last_slide_boost: f32,
    /// The current slide got the boost: its jump may be raised to slide_max_jump_speed once.
    slide_long_jump: bool,
    last_ground_time: f64,
    last_land_time: f64,
    /// Height the last landing came from, for the landing slowdown.
    fall_height: f32,
    jumped_since_ground: bool,
    multi_jump_penalty: bool,
    /// The visual sprint fraction (gun-motion spec §4.1): when the sticky sprint took effect, when
    /// sprinting started and the fraction then, when it ended and the fraction then.
    sticky_since: f64,
    sprint_started_at: f64,
    sprint_fraction_start: f32,
    sprint_ended_at: f64,
    sprint_fraction_end: f32,
    events: MoveEvents,
    /// This tick's `MoveInput::speed_boost`, clamped to 0..1.
    speed_boost: f32,
    /// Apex's vertical gain reference (C_Player +0x1D00): this flight's predicted apex (a jump or a
    /// launch sets it to y + vy²/2g), the height reached if higher, the feet on the ground.
    apex_y: f32,
    /// A launch granted one double jump; landing takes it away.
    double_jump: bool,
    /// The airborne gravity scale a launch set, until landing at least
    /// `LAUNCH_GRAVITY_MIN_TIME` later; 1 otherwise.
    gravity_scale: f32,
    /// When the last launch happened (simulated seconds).
    launch_time: f64,
}

impl Controller {
    pub fn new(world: World, params: MoveParams, position: Vec3) -> Result<Self, ParamsError> {
        params.validate()?;
        if position.iter().any(|v| !v.is_finite()) {
            return Err(ParamsError("invalid position"));
        }
        let revision = world.revision();
        let mut controller = Self {
            world,
            params,
            state: MoveState::at(position),
            accumulator: 0.0,
            time: 0.0,
            last_jump: false,
            last_crouch: false,
            last_crouch_toggle: false,
            last_sprint: false,
            last_traverse: false,
            pending_jump: false,
            pending_crouch_change: false,
            pending_crouch_toggle: false,
            pending_sprint: false,
            pending_traverse: false,
            crouch_held: false,
            world_revision: revision,
            wedged: false,
            duck_ms: 0,
            duck_total_ms: 400,
            duck_toggle: false,
            half_duck: false,
            sticky_sprint: false,
            sticky_sprint_until: f64::NEG_INFINITY,
            last_slide_time: f64::NEG_INFINITY,
            last_slide_boost: 0.0,
            slide_long_jump: false,
            last_ground_time: f64::NEG_INFINITY,
            last_land_time: f64::NEG_INFINITY,
            fall_height: 0.0,
            jumped_since_ground: false,
            multi_jump_penalty: false,
            sticky_since: f64::NEG_INFINITY,
            sprint_started_at: f64::NEG_INFINITY,
            sprint_fraction_start: 0.0,
            sprint_ended_at: f64::NEG_INFINITY,
            sprint_fraction_end: 0.0,
            events: MoveEvents::default(),
            speed_boost: 0.0,
            apex_y: position.y,
            double_jump: false,
            gravity_scale: 1.0,
            launch_time: f64::NEG_INFINITY,
        };
        controller.refresh_view();
        controller.recover_overlap();
        controller.probe_ground(controller.params.skin * 4.0);
        controller.refresh_pose();
        Ok(controller)
    }

    pub fn world(&self) -> &World {
        &self.world
    }
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }
    /// Install a world prepared off-thread and re-check the current capsule.
    /// Prefer this over assigning through world_mut, whose revision checks are
    /// intended for chunk/window edits on the existing World.
    /// Returns the previous world, so a caller on a tight frame budget can free it elsewhere.
    pub fn replace_world(&mut self, world: World) -> World {
        let old = std::mem::replace(&mut self.world, world);
        self.world_revision = self.world.revision();
        self.recover_overlap();
        if self.state.grounded || self.state.velocity.y <= 0.0 {
            self.probe_ground(self.params.skin * 4.0);
        }
        self.refresh_pose();
        old
    }
    /// The caller's speed multiplier from now on (the holstered mode's faster run: er-apex weapons.rs).
    pub fn set_speed_multiplier(&mut self, m: f32) {
        self.params.speed_multiplier = Some(m);
    }

    pub fn params(&self) -> &MoveParams {
        &self.params
    }
    /// Seconds until a new slide gets the boost again.
    pub fn slide_boost_remaining(&self) -> f32 {
        (f64::from(self.params.slide_boost_cooldown) - (self.time - self.last_slide_time)).max(0.0)
            as f32
    }
    pub fn pending_time(&self) -> f64 {
        self.accumulator
    }
    /// Simulated seconds since construction (whole ticks).
    pub fn time(&self) -> f64 {
        self.time
    }
    /// The events since the last call, cleared.
    pub fn take_events(&mut self) -> MoveEvents {
        std::mem::take(&mut self.events)
    }

    /// Integrator teleport/spawn: reset motion and re-evaluate overlap/support.
    pub fn teleport(&mut self, position: Vec3) -> Result<(), ParamsError> {
        if position.iter().any(|v| !v.is_finite()) {
            return Err(ParamsError("invalid position"));
        }
        self.state = MoveState::at(position);
        self.accumulator = 0.0;
        self.pending_jump = false;
        self.pending_crouch_change = false;
        self.pending_crouch_toggle = false;
        self.pending_sprint = false;
        self.pending_traverse = false;
        self.duck_ms = 0;
        self.duck_toggle = false;
        self.sticky_sprint = false;
        self.sticky_sprint_until = f64::NEG_INFINITY;
        self.slide_long_jump = false;
        self.sticky_since = f64::NEG_INFINITY;
        self.sprint_ended_at = f64::NEG_INFINITY;
        self.sprint_fraction_end = 0.0;
        self.events = MoveEvents::default();
        self.apex_y = position.y;
        self.double_jump = false;
        self.gravity_scale = 1.0;
        self.launch_time = f64::NEG_INFINITY;
        self.refresh_view();
        self.recover_overlap();
        self.probe_ground(self.params.skin * 4.0);
        self.refresh_pose();
        Ok(())
    }

    /// Launches the player with this velocity (raw units/s; Octane's launch pad, S3 0x140B8CEB0
    /// sets the velocity outright): off the ground, any slide ended, no coyote jump; this flight's
    /// predicted apex is y + vy²/2g. `double_jump` grants one double jump until landing.
    /// `gravity_scale` scales gravity in the air until landing at least `LAUNCH_GRAVITY_MIN_TIME`
    /// later (1 for Season 3's native launch; R5R's community `_jump_pads.gnut` uses 0.75).
    pub fn launch(
        &mut self,
        velocity: Vec3,
        double_jump: bool,
        gravity_scale: f32,
    ) -> Result<(), ParamsError> {
        if velocity.iter().any(|v| !v.is_finite()) {
            return Err(ParamsError("invalid launch velocity"));
        }
        if !(gravity_scale.is_finite() && gravity_scale > 0.0) {
            return Err(ParamsError("invalid launch gravity scale"));
        }
        self.state.velocity = velocity;
        self.state.grounded = false;
        self.state.ground_normal = None;
        self.jumped_since_ground = true;
        self.end_slide();
        self.gravity_scale = gravity_scale;
        self.launch_time = self.time;
        let g = self.air_gravity();
        self.apex_y = self.state.position.y + velocity.y.max(0.0).powi(2) / (2.0 * g);
        self.double_jump = double_jump;
        self.events.launched = true;
        Ok(())
    }

    /// Sets the velocity for a pull (raw units/s; Pathfinder's grapple, the host steering it each
    /// frame): off the ground, any slide ended, gravity scaled as a launch's (until landing), but no
    /// launch event and no double jump.
    pub fn pull(&mut self, velocity: Vec3, gravity_scale: f32) -> Result<(), ParamsError> {
        if velocity.iter().any(|v| !v.is_finite()) || !(gravity_scale.is_finite() && gravity_scale > 0.0) {
            return Err(ParamsError("invalid pull"));
        }
        self.state.velocity = velocity;
        if self.state.grounded && velocity.y > 0.0 {
            self.state.grounded = false;
            self.state.ground_normal = None;
        }
        self.jumped_since_ground = true;
        self.end_slide();
        self.gravity_scale = gravity_scale;
        self.launch_time = self.time;
        let g = self.air_gravity();
        self.apex_y = self.state.position.y + velocity.y.max(0.0).powi(2) / (2.0 * g);
        self.double_jump = false;
        Ok(())
    }

    /// Gravity in the air: a launch's scale applies until landing.
    fn air_gravity(&self) -> f32 {
        self.params.gravity() * self.gravity_scale
    }

    /// Gravity in the air now (units/s²), a launch's scale included: for callers predicting a
    /// flight (the host's collision window).
    pub fn air_gravity_now(&self) -> f32 {
        self.air_gravity()
    }

    /// Accumulates caller time and only simulates whole 1/60-s substeps.
    /// Invalid dt/input is rejected without changing state. Button edges between
    /// substeps are latched. Continuous input is sampled from the latest call.
    pub fn step(&mut self, input: &MoveInput, dt: f32) -> Result<usize, ParamsError> {
        if !dt.is_finite()
            || dt < 0.0
            || !input.speed_boost.is_finite()
            || input
                .wish
                .iter()
                .chain(input.forward.iter())
                .any(|v| !v.is_finite())
            || self
                .state
                .position
                .iter()
                .chain(self.state.velocity.iter())
                .any(|v| !v.is_finite())
        {
            return Err(ParamsError("non-finite state/input or invalid dt"));
        }
        self.pending_jump |= input.jump && !self.last_jump;
        self.pending_crouch_change |= input.crouch != self.last_crouch;
        self.pending_crouch_toggle |= input.crouch_toggle && !self.last_crouch_toggle;
        self.pending_sprint |= input.sprint && !self.last_sprint;
        self.pending_traverse |= input.traverse && !self.last_traverse;
        self.last_jump = input.jump;
        self.last_crouch = input.crouch;
        self.last_crouch_toggle = input.crouch_toggle;
        self.last_sprint = input.sprint;
        self.last_traverse = input.traverse;
        self.accumulator += f64::from(dt);
        let mut count = 0;
        // Use the same representable f32 FIXED_DT supplied by integrations.
        while self.accumulator + 1.0e-9 >= f64::from(FIXED_DT) {
            self.tick(input);
            self.accumulator = (self.accumulator - f64::from(FIXED_DT)).max(0.0);
            count += 1;
        }
        Ok(count)
    }

    fn dimensions(&self) -> PoseParams {
        if self.state.crouched {
            self.params.crouching
        } else {
            self.params.standing
        }
    }
    fn slope_cos(&self) -> f32 {
        self.params.max_slope_radians.unwrap().cos()
    }
    /// Apex's m_cachedMoveScale (§3): the caller's multiplier times the landing slowdown, times
    /// 1 + 2 x the `speed_boost` severity (S3 0x140815400; Octane's stim gives x1.41).
    fn scale(&self) -> f32 {
        self.params.speed_multiplier.unwrap()
            * self.land_slowdown()
            * (1.0 + 2.0 * self.speed_boost)
    }
    fn ducking(&self) -> bool {
        matches!(self.state.duck, Duck::Ducking | Duck::Ducked)
    }

    fn recover_overlap(&mut self) {
        let pose = self.dimensions();
        for _ in 0..self.params.collision_iterations {
            let Some((normal, depth)) = self.world.deepest_overlap(
                self.state.position,
                pose.radius,
                pose.height,
                self.params.skin * 0.1,
            ) else {
                self.state.stuck = false;
                return;
            };
            self.state.position += normal * (depth + self.params.skin);
        }
        self.state.stuck = self.world.overlaps(
            self.state.position,
            pose.radius,
            pose.height,
            self.params.skin * 0.1,
        );
    }

    fn probe_ground(&mut self, distance: f32) {
        self.state.grounded = false;
        self.state.ground_normal = None;
        let pose = self.dimensions();
        if let Some(hit) = self.world.sweep_support(
            self.state.position,
            -Vec3::y() * distance,
            pose.radius,
            pose.height,
            self.params.skin,
            self.slope_cos(),
        ) {
            // A floor below a steep ramp must not pull the capsule through it.
            // At a stair edge, wall and top-face hits at the same TOI are allowed.
            if hit.fraction > 0.0 {
                if let Some(obstacle) = self.world.sweep(
                    self.state.position,
                    -Vec3::y() * distance,
                    pose.radius,
                    pose.height,
                    self.params.skin,
                ) {
                    if (hit.fraction - obstacle.fraction) * distance > self.params.skin * 2.0
                        && obstacle.surface_normal.y.abs() > 1.0e-4
                    {
                        return;
                    }
                }
            }
            if hit.surface_normal.y >= self.slope_cos() {
                let landing = self.state.position - Vec3::y() * (distance * hit.fraction);
                if self
                    .world
                    .overlaps(landing, pose.radius, pose.height, self.params.skin * 2.0)
                {
                    return;
                }
                self.state.position.y -= distance * hit.fraction;
                self.state.grounded = true;
                self.state.ground_normal = Some(hit.surface_normal);
            }
        }
    }

    fn tick(&mut self, input: &MoveInput) {
        let dt = FIXED_DT;
        self.time += f64::from(dt);
        let jump = std::mem::take(&mut self.pending_jump);
        let crouch_changed = std::mem::take(&mut self.pending_crouch_change);
        let crouch_toggled = std::mem::take(&mut self.pending_crouch_toggle);
        let sprint_pressed = std::mem::take(&mut self.pending_sprint);
        let traverse = std::mem::take(&mut self.pending_traverse);
        self.crouch_held = input.crouch;
        self.speed_boost = input.speed_boost.clamp(0.0, 1.0);
        if self.world_revision != self.world.revision() {
            self.recover_overlap();
            if self.state.grounded || self.state.velocity.y <= 0.0 {
                self.probe_ground(self.params.skin * 4.0);
            } else {
                self.state.grounded = false;
                self.state.ground_normal = None;
            }
            self.world_revision = self.world.revision();
        }
        if self.state.stuck {
            self.state.velocity = Vec3::zeros();
            return;
        }
        // The duck transition counts down in whole milliseconds (16 a tick at 60 Hz), §1.
        if self.duck_ms > 0 {
            self.duck_ms = (self.duck_ms - (dt * 1000.0) as i32).max(0);
        }
        let mut wish = horizontal(input.wish);
        if wish.norm_squared() > 1.0 {
            wish = wish.normalize();
        }
        let facing = horizontal(input.forward)
            .try_normalize(1.0e-6)
            .or_else(|| wish.try_normalize(1.0e-6));
        // Acceleration and deceleration use the stance this command began with (Apex picks the
        // pose settings before the duck code runs): the tick a slide ends standing up still
        // brakes with the crouched deceleration, as R5Reloaded does (spec §22, trial C).
        let start_pose = self.dimensions();

        self.update_duck(wish, facing, crouch_changed, crouch_toggled);
        if input.sprint_blocked {
            self.sticky_sprint = false;
            self.sticky_sprint_until = f64::NEG_INFINITY;
        }
        self.update_sprint(wish, facing, sprint_pressed && !input.sprint_blocked, input.sprint_blocked);
        let pose = self.dimensions();
        let max_speed = self.scale()
            * if self.state.sprinting {
                pose.sprint_speed
            } else {
                pose.speed
            };
        let jumped = jump && self.check_jump();
        self.events.jumped |= jumped;
        if self.state.grounded {
            self.slope_gravity(0.5);
            self.walk_move(wish, start_pose, max_speed);
            self.slope_gravity(0.5);
        } else {
            self.air_move(wish);
        }

        let pose = self.dimensions();
        let mut delta = self.state.velocity * dt;
        if self.state.grounded {
            let normal = self.state.ground_normal.unwrap();
            // Tangent motion with horizontal speed preserved on walkable slopes.
            delta.y = -(normal.x * delta.x + normal.z * delta.z) / normal.y;
            self.state.velocity.y = delta.y / dt;
        } else {
            // Exact constant-gravity displacement gives a <1% discrete jump apex.
            let g = self.air_gravity();
            delta.y -= 0.5 * g * dt * dt;
            self.state.velocity.y -= g * dt;
        }
        // what lands, before the contacts clip it (the landing slowdown and slide use it)
        let impact = self.state.velocity;
        let start = self.state.position;
        let grounded_for_motion = self.state.grounded;
        #[cfg(feature = "profile")]
        let t_slide = std::time::Instant::now();
        let (mut position, normals, lowest_block) =
            self.slide_move(start, delta, pose, grounded_for_motion);
        let mut did_step = false;
        #[cfg(feature = "profile")]
        crate::profile::add(&crate::profile::SLIDE_NS, t_slide);
        // Stepping up only where something low blocked the way: a wall touched only above the
        // step height can't be climbed, and trying cost up to 0.7 ms a tick against Elden Ring's
        // rubble (journal 2026-10-04).
        if grounded_for_motion
            && lowest_block <= self.params.step_height + self.params.skin * 3.0
            && horizontal(delta).norm_squared() > 1.0e-8
            && horizontal(position - start).norm_squared() + 1.0e-5
                < horizontal(delta).norm_squared()
        {
            #[cfg(feature = "profile")]
            let t_step = std::time::Instant::now();
            if let Some(step) = self.try_step(start, delta, pose, position) {
                position = step;
                did_step = true;
                // a sliding climb costs speed by the step's height (§18)
                let climbed = position.y - start.y;
                if self.state.sliding && climbed > 0.0 {
                    let h = horizontal(self.state.velocity);
                    let speed = h.norm();
                    if speed > 0.0 {
                        let kept = (speed - climbed * self.params.slide_step_velocity_reduction)
                            .max(0.0)
                            / speed;
                        self.state.velocity.x *= kept;
                        self.state.velocity.z *= kept;
                    }
                }
            }
            #[cfg(feature = "profile")]
            crate::profile::add(&crate::profile::STEP_UP_NS, t_step);
        }
        self.state.position = position;
        // Ground normals are re-evaluated after movement; downward snapping only
        // for a previously grounded walker, never for a rising jumper.
        let snap = if grounded_for_motion {
            self.params.step_height + self.params.skin * 4.0
        } else {
            self.params.skin * 4.0
        };
        #[cfg(feature = "profile")]
        let t_ground = std::time::Instant::now();
        if grounded_for_motion || self.state.velocity.y <= 0.0 {
            self.probe_ground(snap);
        } else {
            self.state.grounded = false;
            self.state.ground_normal = None;
        }
        #[cfg(feature = "profile")]
        crate::profile::add(&crate::profile::GROUND_NS, t_ground);
        // Successful stepping supersedes the blocked low path.
        let (skin, slope_cos) = (self.params.skin, self.slope_cos());
        // A step taken replaces the low path whole: none of its contacts clip the velocity. The
        // stair edge it ran into is a slanted contact (the capsule's round bottom on the edge),
        // walkable by its slope, and clipping against it threw him up and halved his speed on
        // every step: on stairs he hopped and stopped (2026-10-10).
        let stepped = |normal: &Vec3| {
            did_step
                || (grounded_for_motion && position.y > start.y + skin && normal.y.abs() < slope_cos)
        };
        // Clip the velocity against every contact plane the way the displacement was (a single
        // pass left gravity's share in a crease, where it grew every tick).
        for _ in 0..3 {
            for normal in normals.iter().filter(|n| !stepped(n)) {
                let into = self.state.velocity.dot(normal);
                if into < 0.0 {
                    self.state.velocity -= normal * into;
                }
            }
        }
        // What still points into a plane after that is in a crease: two planes leave the motion
        // along their edge; more than two stop it. Zeroing it with two planes lost ground speed
        // along walls in one tick (user test 2026-10-04: 6.4 -> 2.5 m/s).
        let threshold = -1.0e-3 * self.state.velocity.norm();
        let violated: Vec<Vec3> = normals
            .iter()
            .filter(|n| !stepped(n) && self.state.velocity.dot(n) < threshold)
            .copied()
            .collect();
        match violated.as_slice() {
            [] => {}
            [a, b] => {
                self.state.velocity = a
                    .cross(b)
                    .try_normalize(1.0e-6)
                    .map_or(Vec3::zeros(), |edge| edge * edge.dot(&self.state.velocity));
            }
            _ => self.state.velocity = Vec3::zeros(),
        }
        // Held in place entirely: the velocity is what was realized, none, as Source's
        // TryPlayerMove does when no part of the move happened (a hang must never build speed to
        // release at once; it reached 2,900 m/s in the Boss room, 2026-10-04). On the ground too:
        // keeping the speed there doubled the replay's speed losses in corners and its step-up
        // retries passed 1 ms (2026-10-04 20:58).
        if (position - start).norm_squared() < 1.0e-8 && delta.norm_squared() > 1.0e-6 {
            self.state.velocity = Vec3::zeros();
        }
        // Wedged: meant to fall but held where it is by surfaces none of which is walkable alone
        // (a wall and a steep slope): stand there as on flat ground, so the player can walk along
        // the crease or jump out instead of hanging in the air. A single steep slope still drops
        // the capsule at least half of the fall (it slides), so it is not taken for a wedge.
        let held = delta.y < 0.0 && start.y - position.y < -delta.y * 0.5;
        if !self.state.grounded && (held || (self.wedged && grounded_for_motion)) {
            let probe = self.params.skin * 4.0;
            let blocked = self
                .world
                .sweep(
                    position,
                    -Vec3::y() * probe,
                    pose.radius,
                    pose.height,
                    self.params.skin,
                )
                .is_some_and(|hit| hit.fraction < 0.5);
            if blocked {
                self.state.grounded = true;
                self.state.ground_normal = Some(Vec3::y());
                self.state.velocity.y = 0.0;
            }
            self.wedged = blocked;
        } else {
            self.wedged = false;
        }
        if self.state.grounded {
            let n = self.state.ground_normal.unwrap();
            self.state.velocity.y =
                -(n.x * self.state.velocity.x + n.z * self.state.velocity.z) / n.y;
        }
        // Physics may have clipped the upward velocity into a ceiling.
        if !self.state.grounded && self.state.velocity.y.abs() < 1.0e-4 {
            self.probe_ground(self.params.skin * 4.0);
        }
        if self.state.grounded && !grounded_for_motion {
            self.land(impact, wish, facing);
        }
        if self.state.grounded {
            self.last_ground_time = self.time;
        } else if grounded_for_motion && !jumped {
            // walked off: no multi-jump penalty for the next landing's jump
            self.multi_jump_penalty = false;
        }
        // the vertical gain reference (0x140870160): the feet on the ground, else the higher of
        // the predicted apex and the height reached; landing takes a granted double jump away
        if self.state.grounded {
            self.apex_y = self.state.position.y;
            self.double_jump = false;
            if self.time - self.launch_time >= LAUNCH_GRAVITY_MIN_TIME {
                self.gravity_scale = 1.0;
            }
        } else {
            self.apex_y = self.apex_y.max(self.state.position.y);
        }
        self.refresh_view();
        let forward = facing.unwrap_or_default();
        #[cfg(feature = "profile")]
        let t_ledge = std::time::Instant::now();
        // Ledges are probed on the traverse button only: its execution is the only user, and
        // probing every tick against a wall cost up to 0.8 ms in dense Elden Ring meshes (journal
        // 2026-10-04). Apex's automantle (airborne against a wall) is left for the integration's
        // M2 movement work, with its own time budget.
        self.state.ledge = if traverse {
            self.detect_ledge(forward)
        } else {
            None
        };
        #[cfg(feature = "profile")]
        crate::profile::add(&crate::profile::LEDGE_NS, t_ledge);
        if traverse && self.state.ledge.is_some() {
            self.execute_ledge(forward);
        } else {
            self.refresh_pose();
        }
    }

    /// The landing slowdown factor (§16): 1 unless the last landing came from high enough.
    fn land_slowdown(&self) -> f32 {
        let p = &self.params;
        let t = (self.time - self.last_land_time) as f32;
        if t >= p.land_slowdown_duration || self.fall_height <= p.land_slowdown_height_min {
            return 1.0;
        }
        let k = ((self.fall_height - p.land_slowdown_height_min)
            / (p.land_slowdown_height_max - p.land_slowdown_height_min))
            .min(1.0);
        1.0 - (1.0
            - (t / p.land_slowdown_duration)
                .max(0.0)
                .powf(p.land_slowdown_time_power))
            * k
            * (1.0 - p.land_slowdown_frac)
    }

    /// Crouch, stand up and the slide that a crouch starts (§12).
    fn update_duck(&mut self, wish: Vec3, facing: Option<Vec3>, changed: bool, toggled: bool) {
        let was_ducking = self.ducking();
        if changed {
            self.duck_toggle = false;
        } else if toggled {
            self.duck_toggle = !self.duck_toggle;
        }
        let mut want = self.crouch_held || self.duck_toggle;
        if self.state.sliding
            && horizontal(self.state.velocity).norm_squared()
                > self.params.slide_max_stop_speed * self.params.slide_max_stop_speed
        {
            want = true;
        }
        if !self.params.crouch_enabled {
            want = false;
            self.duck_toggle = false;
        }
        let mut fast = false;
        if !want && self.state.crouched && !self.can_stand() {
            want = true;
            fast = true;
        }
        if want {
            if !was_ducking {
                let normal = self.state.ground_normal.filter(|_| self.state.grounded);
                self.try_start_slide(normal, false, self.state.velocity.y, wish, facing);
            }
            let total = if fast || self.state.sliding { 200 } else { 400 };
            match self.state.duck {
                Duck::Standing => {
                    self.state.duck = Duck::Ducking;
                    self.duck_ms = total;
                    self.duck_total_ms = total;
                    self.half_duck = !self.state.grounded;
                    self.events.duck_started = true;
                }
                Duck::Unducking => {
                    let done = ((200 - self.duck_ms) as f32 / 200.0).clamp(0.0, 1.0);
                    self.state.duck = Duck::Ducking;
                    self.duck_ms = (total as f32 * done) as i32;
                    self.duck_total_ms = total;
                    self.events.duck_started = true;
                }
                _ => {}
            }
            if self.state.duck == Duck::Ducking && self.duck_ms <= 0 {
                if !self.state.crouched {
                    self.state.crouched = true;
                    if self.half_duck && !self.state.grounded {
                        // ducking in the air tucks the feet up by half the height difference
                        let lift =
                            0.5 * (self.params.standing.height - self.params.crouching.height);
                        let up = self.state.position + Vec3::y() * lift;
                        let c = self.params.crouching;
                        if !self
                            .world
                            .overlaps(up, c.radius, c.height, self.params.skin * 0.1)
                        {
                            self.state.position = up;
                        }
                    }
                }
                self.state.duck = Duck::Ducked;
            }
        } else {
            match self.state.duck {
                Duck::Ducked => {
                    self.state.duck = Duck::Unducking;
                    self.duck_ms = 200;
                    self.finish_unduck();
                    self.events.unduck_started = true;
                }
                Duck::Ducking => {
                    let total = if self.state.sliding { 200.0 } else { 400.0 };
                    let done = (self.duck_ms as f32 / total).clamp(0.0, 1.0);
                    self.state.duck = Duck::Unducking;
                    self.duck_ms = ((1.0 - done) * 200.0) as i32;
                    if self.state.crouched {
                        self.finish_unduck();
                    }
                    self.events.unduck_started = true;
                }
                _ => {}
            }
            self.duck_total_ms = 200;
            if self.state.duck == Duck::Unducking && self.duck_ms <= 0 {
                self.state.duck = Duck::Standing;
            }
        }
        if was_ducking && !self.ducking() {
            self.end_slide();
        }
    }

    fn can_stand(&self) -> bool {
        let s = self.params.standing;
        !self.world.overlaps(
            self.state.position,
            s.radius,
            s.height,
            self.params.skin * 0.1,
        )
    }

    /// Standing capsule again; in the air the feet come down by half the height difference.
    fn finish_unduck(&mut self) {
        self.state.crouched = false;
        if !self.state.grounded {
            let drop = 0.5 * (self.params.standing.height - self.params.crouching.height);
            let down = self.state.position - Vec3::y() * drop;
            let s = self.params.standing;
            if !self
                .world
                .overlaps(down, s.radius, s.height, self.params.skin * 0.1)
            {
                self.state.position = down;
            }
        }
    }

    /// Apex's crouchFraction (gun-motion spec §2.3): smoothstep over the duck or unduck transition.
    fn duck_fraction(&self) -> f32 {
        let smooth = |r: f32| {
            let r = r.clamp(0.0, 1.0);
            (3.0 - 2.0 * r) * r * r
        };
        match self.state.duck {
            Duck::Standing => 0.0,
            Duck::Ducked => 1.0,
            Duck::Ducking => 1.0 - smooth(self.duck_ms as f32 / self.duck_total_ms.max(1) as f32),
            Duck::Unducking => smooth(self.duck_ms as f32 / 200.0),
        }
    }

    /// Eye height (§12, §19): eased by the crouch fraction.
    fn eye_height(&self) -> f32 {
        let (s, c) = (
            self.params.standing.viewheight,
            self.params.crouching.viewheight,
        );
        s + (c - s) * self.duck_fraction()
    }

    /// Apex's visual sprint fraction at `now` (gun-motion spec §4.1). Sprinting: it rises from
    /// its value at the start, over `sprint_start_fast_duration` from the start but not before
    /// `sprint_start_delay` + `sprint_start_duration` from the sticky sprint taking effect (from a
    /// walk 0.2 s late and 1 s to the full; a sprint resumed on landing in 0.2 s). Otherwise it
    /// falls linearly over `sprint_end_duration`.
    fn sprint_fraction(&self, now: f64) -> f32 {
        let p = &self.params;
        let f = if self.state.sprinting {
            let fast = ((now - self.sprint_started_at) as f32 / p.sprint_start_fast_duration)
                .clamp(0.0, 1.0);
            let slow = ((now - (self.sticky_since + f64::from(p.sprint_start_delay))) as f32
                / p.sprint_start_duration)
                .clamp(0.0, 1.0);
            fast.min(slow) + self.sprint_fraction_start
        } else {
            self.sprint_fraction_end - (now - self.sprint_ended_at) as f32 / p.sprint_end_duration
        };
        f.clamp(0.0, 1.0)
    }

    /// The state's view outputs: eye height, crouch and sprint fractions, the slide jump flag.
    fn refresh_view(&mut self) {
        self.state.duck_fraction = self.duck_fraction();
        self.state.eye_height = self.eye_height();
        let f = self.sprint_fraction(self.time);
        self.state.sprint_fraction = f;
        self.state.eye_sprint_offset =
            self.params.sprint_view_offset * (f * std::f32::consts::FRAC_PI_2).sin();
        self.state.slide_long_jump = self.slide_long_jump;
    }

    /// Apex's TryStartSlide (§13). `normal` is the ground under the player (None in the air),
    /// `vertical` the speed along up at that moment (landing: still the impact speed).
    fn try_start_slide(
        &mut self,
        normal: Option<Vec3>,
        from_air: bool,
        vertical: f32,
        wish: Vec3,
        facing: Option<Vec3>,
    ) {
        let p = &self.params;
        if normal.map_or(0.0, |n| n.y) < 0.7 && !p.slide_while_in_air {
            return;
        }
        if !p.slide_enabled {
            return;
        }
        let h = horizontal(self.state.velocity);
        let h2 = h.norm_squared();
        let threshold = if from_air && vertical <= -200.0 {
            p.slide_required_start_speed_air
        } else {
            p.slide_required_start_speed
        };
        if h2 < threshold * threshold {
            return;
        }
        // On the ground the move input must be within reach of the view (a landing skips this:
        // Apex still has no ground entity at that point).
        if self.state.grounded
            && !from_air
            && facing.map_or(0.0, |f| wish.dot(&f)) < p.slide_max_angle_dot
        {
            return;
        }
        let (boost, cap, cooldown) = (
            p.slide_speed_boost,
            p.slide_speed_boost_cap,
            p.slide_boost_cooldown,
        );
        self.state.sliding = true;
        self.state.crouched = true;
        let boosted = self.time - self.last_slide_time > f64::from(cooldown);
        self.events.slide_started = Some(boosted);
        if boosted {
            let scale = self.scale();
            let speed = h2.sqrt();
            let target = (speed + scale * boost).min(scale * cap);
            if target > speed {
                self.state.velocity.x *= target / speed;
                self.state.velocity.z *= target / speed;
                self.last_slide_boost = target - speed;
            } else {
                self.last_slide_boost = 0.0;
            }
            self.slide_long_jump = true;
        }
        // every slide restarts the cooldown, boosted or not (R5Reloaded trial E)
        self.last_slide_time = self.time;
    }

    fn end_slide(&mut self) {
        if self.state.sliding {
            self.state.sliding = false;
            if self.params.slide_auto_stand {
                self.duck_toggle = false;
            }
        }
    }

    /// Apex's sticky sprint (§5): sprinting starts at once; a press keeps it on while the player
    /// keeps moving roughly forward.
    fn update_sprint(&mut self, wish: Vec3, facing: Option<Vec3>, pressed: bool, blocked: bool) {
        let now = self.time;
        if pressed {
            self.duck_toggle = false;
            self.sticky_sprint_until = self.sticky_sprint_until.max(now + 3.0);
        } else if self.last_sprint {
            self.sticky_sprint_until = self.sticky_sprint_until.max(now + 0.1);
        }
        let m2 = wish.norm_squared();
        let m = m2.sqrt();
        let along = facing.map_or(m, |f| wish.dot(&f));
        let can_sticky = m2 >= 0.64
            && (m == 0.0 || along / m >= -0.707)
            && (!self.ducking() || self.state.sliding);
        if self.sticky_sprint {
            if !can_sticky {
                self.sticky_sprint = false;
                self.sticky_sprint_until = f64::NEG_INFINITY;
            }
        } else if self.sticky_sprint_until >= now && can_sticky {
            self.sticky_sprint = true;
            self.sticky_since = now;
        }
        let h = horizontal(self.state.velocity);
        let moving_on = facing.map_or(h.norm(), |f| h.dot(&f)) > 0.0;
        // Off the ground up to 0.25 s, but not after a jump: R5Reloaded ends the sprint the tick
        // after a jump (gun-motion spec §2.2, trial T3; the code doing it is not found yet).
        // Nothing in the air depends on sprinting; the sprint view and animations do.
        let can_sprint = self.params.sprint_enabled
            && !blocked
            && (self.state.grounded
                || (now - self.last_ground_time <= 0.25 && !self.jumped_since_ground))
            && m2 >= 0.64
            && along / m >= 0.707
            && !self.ducking()
            && self.land_slowdown() >= self.params.land_slowdown_no_sprint_frac
            && moving_on;
        let sprinting = (self.last_sprint || self.sticky_sprint) && can_sprint;
        if sprinting != self.state.sprinting {
            // the fraction so far (by the old state's rule), as each new rule starts from it
            if sprinting {
                // Apex keeps the start value quantized to 1/1023 (spec §4.1)
                self.sprint_fraction_start = (self.sprint_fraction(now) * 1023.0).round() / 1023.0;
                self.sprint_started_at = now;
                self.events.sprint_started = true;
            } else {
                self.sprint_fraction_end = self.sprint_fraction(now);
                self.sprint_ended_at = now;
                self.events.sprint_ended = true;
            }
        }
        self.state.sprinting = sprinting;
    }

    /// Apex's CheckJumpButton (§15). Returns whether the player jumped.
    fn check_jump(&mut self) -> bool {
        let p = self.params.clone();
        let now = self.time;
        let near_ground = self.state.grounded
            || (now - self.last_ground_time < f64::from(p.jump_grace_period)
                && !self.jumped_since_ground);
        if !near_ground && self.double_jump {
            self.double_jump();
            return true;
        }
        let crouched_ok = self.state.sliding || p.can_jump_while_crouched;
        if self.duck_toggle {
            if !p.can_jump_while_crouched {
                self.duck_toggle = false;
            }
            if near_ground && !crouched_ok {
                return false;
            }
        }
        if !near_ground || (self.state.duck == Duck::Unducking && !crouched_ok) {
            return false;
        }
        let set_speed = |v: &mut Vec3, from: f32, to: f32| {
            if from > 0.0 {
                v.x *= to / from;
                v.z *= to / from;
            }
        };
        if self.state.sliding && self.slide_long_jump {
            let h2 = horizontal(self.state.velocity).norm_squared();
            if h2 > 0.0 {
                let h = h2.sqrt();
                let mut current = h;
                // jumping straight out of a fresh slide takes its boost back
                if self.state.duck != Duck::Ducked && self.last_slide_time > now - 0.4 {
                    current = (h - self.last_slide_boost).max(0.0);
                    set_speed(&mut self.state.velocity, h, current);
                }
                let top = p.slide_max_jump_speed;
                if h2 > (0.85 * top) * (0.85 * top) && h2 < top * top {
                    set_speed(&mut self.state.velocity, current, top);
                }
                self.slide_long_jump = false;
            }
        }
        let mut height = if self.state.sliding {
            p.slide_jump_height
        } else {
            p.jump_height
        };
        let since_land = (now - self.last_land_time) as f32;
        if since_land < p.skip_time {
            let h2 = horizontal(self.state.velocity).norm_squared();
            if h2 > p.skip_jump_height_speed * p.skip_jump_height_speed {
                height *= p.skip_jump_height_fraction;
            }
            let retain = if p.skip_speed_retain < 0.0 {
                self.scale() * p.standing.sprint_speed
            } else {
                p.skip_speed_retain
            };
            if h2 > retain * retain {
                let h = h2.sqrt();
                set_speed(
                    &mut self.state.velocity,
                    h,
                    retain.max(h - p.skip_speed_reduce),
                );
            }
        }
        if since_land < p.anti_multi_jump_time_max && self.multi_jump_penalty {
            let k = if p.anti_multi_jump_time_min <= since_land {
                (since_land - p.anti_multi_jump_time_max)
                    / (p.anti_multi_jump_time_min - p.anti_multi_jump_time_max)
            } else {
                1.0
            };
            height *= 1.0 - (1.0 - p.anti_multi_jump_height_frac) * k;
        }
        self.state.grounded = false;
        self.state.ground_normal = None;
        self.jumped_since_ground = true;
        // Apex adds the jump speed after a half-step of gravity and applies one more within the
        // jump: half a step lower than a plain launch (R5Reloaded: vz = v0 − 1.5·g·dt after the
        // jump tick).
        let g = p.gravity();
        self.state.velocity.y = (2.0 * g * height).sqrt() - 0.5 * g * FIXED_DT;
        // the predicted apex (0x1408095E0: y + v²/2g of the jump speed)
        self.apex_y = self.state.position.y + height;
        self.end_slide();
        self.multi_jump_penalty = true;
        true
    }

    /// The air branch of Apex's jump (0x140809CB0) with a granted double jump: height 150; if the
    /// rise left in the current upward speed already covers 0.75 of it, only 0.25 of it is added
    /// on top of that speed, else the vertical speed starts from 0; no more than 225 over this
    /// flight's predicted apex; then vy += sqrt(2 g height).
    fn double_jump(&mut self) {
        let g = self.air_gravity();
        let vy = self.state.velocity.y;
        let rise = if vy >= 0.0 { vy * vy / (2.0 * g) } else { 0.0 };
        let mut height = DOUBLE_JUMP_HEIGHT;
        if height <= height * DOUBLE_JUMP_MIN_FRACTION + rise {
            height *= DOUBLE_JUMP_MIN_FRACTION;
        } else {
            self.state.velocity.y = 0.0;
        }
        height = height.min((DOUBLE_JUMP_CUTOFF + self.apex_y - self.state.position.y).max(0.0));
        self.state.velocity.y += (2.0 * g * height).sqrt();
        self.double_jump = false;
        self.events.double_jumped = true;
    }

    /// Apex's GetAccel (§7).
    fn acceleration(&self, h: Vec3, pose: PoseParams) -> f32 {
        let scale = self.scale();
        if self.state.sliding {
            return self.params.slide_accel * scale;
        }
        let h2 = h.norm_squared();
        let walk = scale * pose.speed - 1.0;
        let mut a = pose.sprint_acceleration;
        if a < 0.0 || h2 <= walk * walk {
            let low = scale * pose.low_speed;
            a = if low * low <= h2 || pose.low_acceleration < 0.0 {
                pose.acceleration
            } else {
                pose.low_acceleration
            };
        }
        a * scale
    }

    /// Apex's Decelerate (§8) on the horizontal velocity `h`.
    fn decelerate(
        &self,
        h: Vec3,
        dir: Vec3,
        wish_speed: f32,
        wish: Vec3,
        pose: PoseParams,
    ) -> Vec3 {
        let p = &self.params;
        let mut rest = h;
        let mut keep = Vec3::zeros();
        let along = dir.dot(&rest);
        if along > 0.0 {
            // the part of the motion the player is pushing for is not braked
            let frac = (along * wish_speed / rest.norm_squared()).min(1.0);
            keep = dir * (frac * along);
            rest -= keep;
        }
        let speed = rest.norm();
        let mut decel = -1.0;
        if h.norm() > self.scale() * pose.speed {
            decel = if pose.sprint_deceleration >= 0.0 {
                pose.sprint_deceleration
            } else if pose.sprint_acceleration >= 0.0 {
                pose.sprint_acceleration
            } else {
                -1.0
            };
        }
        if self.state.sliding {
            decel = if (self.crouch_held || self.duck_toggle) && h.dot(&wish) >= 0.0 {
                p.slide_decel
            } else {
                p.slide_want_to_stop_decel
            };
        }
        if decel < 0.0 {
            decel = if pose.deceleration >= 0.0 {
                pose.deceleration
            } else {
                pose.acceleration * 0.6
            };
        }
        let mut reduced = (speed - decel * FIXED_DT).max(0.0);
        if self.state.sliding {
            // a speed boost slows the decay (S3 0x140817C90: exponent (1 - 0.75 severity) dt)
            reduced *= p
                .slide_velocity_decay
                .powf((1.0 - 0.75 * self.speed_boost) * FIXED_DT);
        }
        if reduced < speed {
            keep + rest * (reduced / speed)
        } else {
            h
        }
    }

    /// Apex's WalkMove velocity update (§10): wish, deceleration, acceleration, slide end.
    fn walk_move(&mut self, wish: Vec3, pose: PoseParams, max_speed: f32) {
        let mut h = horizontal(self.state.velocity);
        let mut w = wish * if self.state.sprinting { 1.25 } else { 1.0 };
        let m = w.norm();
        if m > 1.0 {
            w /= m;
        } else if m > 0.0 {
            w *= 2.0 - m;
        }
        if self.state.sliding {
            // forward input can't push a slide on, only steer it
            let d = h.dot(&w);
            if d > 0.0 {
                w -= h * (d / h.norm_squared());
            }
        }
        let len = w.norm();
        let dir = if len > 1.0e-6 { w / len } else { Vec3::zeros() };
        let wish_speed = len.min(1.0) * max_speed;
        let accel = self.acceleration(h, pose);
        h = self.decelerate(h, dir, wish_speed, wish, pose);
        accelerate(&mut h, dir, wish_speed, accel, FIXED_DT);
        self.state.velocity.x = h.x;
        self.state.velocity.z = h.z;
        if self.state.sliding && h.norm() < self.params.slide_stop_speed {
            self.end_slide();
        }
    }

    /// Half of the slide's gravity pull along the ground (§11); Apex applies it before and after
    /// the walk move. Walking feels no slope.
    fn slope_gravity(&mut self, part: f32) {
        if !self.state.sliding {
            return;
        }
        let Some(n) = self.state.ground_normal else {
            return;
        };
        let pull = part * self.params.gravity() * FIXED_DT * n.y;
        self.state.velocity.x += pull * n.x;
        self.state.velocity.z += pull * n.z;
    }

    /// Apex's air move (§20): friction, then wish acceleration that only turns once at speed.
    fn air_move(&mut self, wish: Vec3) {
        let p = &self.params;
        let scale = self.scale();
        let mut h = horizontal(self.state.velocity);
        let speed = h.norm();
        if speed > 0.0 {
            let friction = p.air_friction + p.air_drag * speed;
            h *= (speed - friction * speed * FIXED_DT).max(0.0) / speed;
        }
        let m = wish.norm();
        let dir = if m > 1.0e-6 { wish / m } else { Vec3::zeros() };
        let wish_speed = p.air_speed * scale * m.min(1.0);
        let add = wish_speed - dir.dot(&h);
        if add > 0.0 {
            h += dir * add.min(p.air_acceleration * scale * FIXED_DT);
        } else if m > 1.0e-6 {
            let cap2 = h.norm_squared().max(wish_speed * wish_speed);
            h += dir * (FIXED_DT * p.extra_air_acceleration);
            let n2 = h.norm_squared();
            if n2 > cap2 {
                h *= (cap2 / n2).sqrt();
            }
        }
        self.state.velocity.x = h.x;
        self.state.velocity.z = h.z;
    }

    /// Apex's SetGroundEntity landing (§16): fall height, landing slowdown, landing slide.
    fn land(&mut self, impact: Vec3, wish: Vec3, facing: Option<Vec3>) {
        let speed = (-impact.y).max(0.0);
        if self.events.landed.is_none_or(|l| l.speed < speed) {
            self.events.landed = Some(Landing {
                speed,
                crouched: self.state.crouched,
            });
        }
        let n = self.state.ground_normal.unwrap_or(Vec3::y());
        let p = &self.params;
        let g = p.base_gravity.unwrap();
        let fade = 1.0
            - ((self.time - self.last_land_time) as f32 / p.land_slowdown_duration).clamp(0.0, 1.0);
        let into = impact.dot(&n);
        self.fall_height = (fade * self.fall_height).max(into * into * 0.5 / g);
        self.last_land_time = self.time;
        let k = self.land_slowdown();
        self.state.velocity.x *= k;
        self.state.velocity.z *= k;
        self.jumped_since_ground = false;
        if self.ducking() {
            self.try_start_slide(Some(n), true, impact.y, wish, facing);
            if self.state.grounded {
                let n = self.state.ground_normal.unwrap();
                self.state.velocity.y =
                    -(n.x * self.state.velocity.x + n.z * self.state.velocity.z) / n.y;
            }
        }
    }

    fn refresh_pose(&mut self) {
        self.state.pose = if !self.state.grounded {
            Pose::Airborne
        } else if self.state.sliding {
            Pose::Sliding
        } else if self.state.crouched {
            Pose::Crouching
        } else {
            Pose::Standing
        };
    }

    fn slide_move(
        &self,
        start: Vec3,
        mut remaining: Vec3,
        pose: PoseParams,
        block_uphill_walls: bool,
    ) -> (Vec3, Vec<Vec3>, f32) {
        let mut position = start;
        let mut planes = Vec::with_capacity(self.params.collision_iterations);
        // the lowest point (above the feet) where something too steep to walk on blocked it
        let mut lowest_block = f32::MAX;
        for _ in 0..self.params.collision_iterations {
            let Some(hit) = self.world.sweep(
                position,
                remaining,
                pose.radius,
                pose.height,
                self.params.skin,
            ) else {
                position += remaining;
                break;
            };
            #[cfg(test)]
            if wedge_debug::TRACE.with(|t| t.get()) {
                eprintln!(
                    "    sweep {:?} -> fraction {:.4} normal {:.3?} surface {:.3?} point {:.2?}",
                    remaining, hit.fraction, hit.normal, hit.surface_normal, hit.point
                );
            }
            position += remaining * hit.fraction;
            remaining *= 1.0 - hit.fraction;
            // an edge or corner ahead and above the feet (the capsule's round bottom on a stair's
            // nose: the contact slants, the face does not): a wall to step over, not a slope to
            // ride up (on stairs he rode every edge, was thrown up and stopped, 2026-10-10)
            let edge = block_uphill_walls
                && hit.point.y - position.y > self.params.skin * 2.0
                && hit.normal.dot(&hit.surface_normal) < 0.99;
            if hit.surface_normal.y < self.slope_cos() || edge {
                lowest_block = lowest_block.min(hit.point.y - position.y);
            }
            let mut normal = hit.normal;
            // a low lump under the feet (a bone, a stone: Elden Ring's floors are full of them;
            // each one stopped him dead, 2026-10-10): ridden over as a walkable slope, not a wall
            let low_lump = block_uphill_walls
                && hit.point.y - position.y <= self.params.step_height * LOW_LUMP_FRACTION
                && horizontal(hit.normal).norm_squared() > 1.0e-8;
            if low_lump {
                let h = horizontal(hit.normal).normalize();
                let (sin, cos) = LUMP_RIDE_ANGLE.to_radians().sin_cos();
                normal = (h * sin + Vec3::y() * cos).normalize();
            } else if block_uphill_walls
                && normal.y > 0.0
                && (hit.surface_normal.y < self.slope_cos() || normal.y < self.slope_cos() || edge)
            {
                normal = horizontal(normal).try_normalize(1.0e-6).unwrap_or(normal);
            }
            if !planes.iter().any(|n: &Vec3| n.dot(&normal) > 0.999) {
                planes.push(normal);
            }
            // Resolve all planes; two walls leave motion along their crease.
            for _ in 0..3 {
                for n in &planes {
                    let into = remaining.dot(n);
                    if into < 0.0 {
                        remaining -= n * into;
                    }
                }
            }
            if planes.iter().any(|n| remaining.dot(n) < -1.0e-4) {
                remaining = Vec3::zeros();
            }
            if remaining.norm_squared() < 1.0e-10 {
                break;
            }
        }
        (position, planes, lowest_block)
    }

    fn try_step(&self, start: Vec3, delta: Vec3, pose: PoseParams, low: Vec3) -> Option<Vec3> {
        let rise = self.params.step_height + self.params.skin * 2.0;
        if self
            .world
            .sweep(
                start,
                Vec3::y() * rise,
                pose.radius,
                pose.height,
                self.params.skin,
            )
            .is_some()
        {
            return None;
        }
        let raised = start + Vec3::y() * rise;
        let (across, _, _) = self.slide_move(raised, horizontal(delta), pose, true);
        let drop = rise + self.params.step_height + self.params.skin * 2.0;
        // down onto a walkable face first, as `probe_ground` does: on stairs the capsule coming
        // down touched the next riser first (a wall face) and the step was refused, so the feet
        // caught on every step (Elden Ring's stairs, 2026-10-10); else onto whatever is there (a
        // stone's crest, a bone: no walkable face on top, each one stopped him); clear either way
        let max_rise = self.params.step_height + self.params.skin * 3.0;
        let clear = |hit: &crate::world::Hit| {
            let landing = across - Vec3::y() * (drop * hit.fraction);
            (hit.point.y - start.y <= max_rise
                && !self.world.overlaps(landing, pose.radius, pose.height, self.params.skin * 2.0))
            .then_some(landing)
        };
        let walkable = self
            .world
            .sweep_support(across, -Vec3::y() * drop, pose.radius, pose.height, self.params.skin, self.slope_cos())
            .filter(|hit| hit.surface_normal.y >= self.slope_cos())
            .and_then(|hit| clear(&hit));
        let landing = match walkable {
            Some(l) => l,
            None => self
                .world
                .sweep(across, -Vec3::y() * drop, pose.radius, pose.height, self.params.skin)
                .and_then(|hit| clear(&hit))?,
        };
        if landing.y - start.y > self.params.step_height + self.params.skin * 3.0
            || landing.y < start.y - self.params.step_height - self.params.skin * 3.0
            || horizontal(landing - start).norm_squared()
                <= horizontal(low - start).norm_squared() + 1.0e-5
        {
            return None;
        }
        Some(landing)
    }

    /// Probe an obstructing wall, then sweep over it from above, find its top,
    /// and validate the landing volume and full lift/across path.
    pub fn detect_ledge(&self, forward: Vec3) -> Option<Ledge> {
        if forward.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let direction = horizontal(forward).try_normalize(1.0e-6)?;
        let pose = self.dimensions();
        let start = self.state.position;
        let distance = self.params.ledge_probe_distance + pose.radius;
        let wall = self.world.sweep(
            start,
            direction * distance,
            pose.radius,
            pose.height,
            self.params.skin,
        )?;
        if wall.surface_normal.y >= self.slope_cos() {
            return None;
        }
        let reach = if self.params.climb_enabled {
            self.params.climb_height.max(self.params.mantle_height)
        } else {
            self.params.mantle_height
        };
        let rise = reach + self.params.skin * 4.0;
        let above = start + Vec3::y() * rise;
        if self
            .world
            .sweep(
                start,
                Vec3::y() * rise,
                pose.radius,
                pose.height,
                self.params.skin,
            )
            .is_some()
            || self
                .world
                .sweep(
                    above,
                    direction * distance,
                    pose.radius,
                    pose.height,
                    self.params.skin,
                )
                .is_some()
        {
            return None;
        }
        let target = above + direction * distance;
        let top = self.world.ray_down(target, rise)?;
        let edge_origin =
            Vec3::new(wall.point.x, above.y, wall.point.z) + direction * (self.params.skin * 2.0);
        let edge = self.world.ray_down(edge_origin, rise)?;
        let height = edge.point.y - start.y;
        if height <= self.params.step_height
            || height > reach + self.params.skin
            || top.normal.y < self.slope_cos()
            || edge.normal.y < self.slope_cos()
            || top.point.y - start.y > reach + self.params.skin
        {
            return None;
        }
        let kind = if height <= self.params.mantle_height {
            LedgeKind::Mantle
        } else if self.params.climb_enabled && height <= self.params.climb_height {
            LedgeKind::Climb
        } else {
            return None;
        };
        let landing_hit = self.world.sweep(
            target,
            -Vec3::y() * rise,
            pose.radius,
            pose.height,
            self.params.skin,
        )?;
        if landing_hit.surface_normal.y < self.slope_cos() {
            return None;
        }
        let landing = target - Vec3::y() * (rise * landing_hit.fraction);
        if self
            .world
            .overlaps(landing, pose.radius, pose.height, self.params.skin * 0.1)
        {
            return None;
        }
        Some(Ledge {
            kind,
            edge_position: edge.point,
            landing_position: landing,
            normal: top.normal,
        })
    }

    /// Minimal execution: re-probe the current world, then move to the validated
    /// top. No animation/climbing timing; Mantling is emitted for this substep.
    pub fn execute_ledge(&mut self, forward: Vec3) -> bool {
        let Some(ledge) = self.detect_ledge(forward) else {
            return false;
        };
        self.state.position = ledge.landing_position;
        self.state.velocity = Vec3::zeros();
        self.state.grounded = true;
        self.state.ground_normal = Some(ledge.normal);
        self.state.sliding = false;
        self.state.sprinting = false;
        self.state.pose = Pose::Mantling;
        self.state.ledge = Some(ledge);
        true
    }
}

fn horizontal(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}
/// Apex's Accelerate (§9): adds along `wish` up to `wish_speed`, never past
/// max(old speed, wish speed).
fn accelerate(v: &mut Vec3, wish: Vec3, wish_speed: f32, acceleration: f32, dt: f32) {
    let cap2 = v.norm_squared().max(wish_speed * wish_speed);
    let add = wish_speed - v.dot(&wish);
    if add > 0.0 {
        *v += wish * add.min(acceleration * dt);
        let n2 = v.norm_squared();
        if n2 > cap2 {
            *v *= (cap2 / n2).sqrt();
        }
    }
}

/// Replays the Boss-room wedge (2026-10-04, `kcc dump` in scratch, not in git):
/// `FUSE_WEDGE_DUMP=<file> cargo test --release wedge_debug -- --ignored --nocapture`.
#[cfg(test)]
mod wedge_debug {
    use super::*;
    thread_local!(pub static TRACE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });

    #[test]
    #[ignore]
    fn trace_dump() {
        let Ok(path) = std::env::var("FUSE_WEDGE_DUMP") else {
            return;
        };
        let text = std::fs::read_to_string(path).unwrap();
        let nums = |l: &str| {
            l.split_whitespace()
                .filter_map(|x| x.parse::<f32>().ok())
                .collect::<Vec<_>>()
        };
        let mut lines = text.lines();
        let head = nums(lines.next().unwrap());
        let state = nums(lines.next().unwrap());
        let tris: Vec<crate::Triangle> = lines
            .map(nums)
            .filter(|v| v.len() == 9)
            .map(|v| {
                [
                    Vec3::new(v[0], v[1], v[2]),
                    Vec3::new(v[3], v[4], v[5]),
                    Vec3::new(v[6], v[7], v[8]),
                ]
            })
            .collect();
        let params = MoveParams {
            base_gravity: Some(head[1] / head[0]),
            speed_multiplier: Some(head[3]),
            meters_per_unit: Some(head[0]),
            max_slope_radians: Some(head[2].to_radians()),
            ..MoveParams::default()
        };
        let mut c = Controller::new(
            World::from_triangles(&tris).unwrap(),
            params,
            Vec3::new(state[0], state[1], state[2]),
        )
        .unwrap();
        let a = std::f32::consts::FRAC_PI_4;
        let input = MoveInput {
            wish: Vec3::new(a.sin(), 0.0, a.cos()),
            ..Default::default()
        };
        for t in 0..40 {
            TRACE.with(|x| x.set(t >= 0));
            eprintln!(
                "tick {t}: at {:.3?} v {:.1?} {:?} grounded {} wedged {}",
                c.state.position, c.state.velocity, c.state.pose, c.state.grounded, c.wedged
            );
            c.tick(&input);
        }
    }
}
