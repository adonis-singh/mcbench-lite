// The creative-mode player's movement, ported from the 1.21.10 client's code:
// LocalPlayer.tick and aiStep (the keys, double-tapped jump, sprinting,
// flying), Player.tick, aiStep and travel, LivingEntity.tick, aiStep and
// travel, and Entity.baseTick and move with their collision (collision.rs),
// step-up, sneaking at an edge, fluids, climbing and the blocks a player's
// movement meets. The player is a client: it reads the level (a `&Level`) and
// changes only itself, as the client's prediction does; a block that reacts
// to a player on the server (a pressure plate, a tripwire, a sculk sensor)
// does not hear this one. What this port does not model refuses
// ("unported: ...").
//
// The numbers are Java's: doubles and floats in Java's order and widths.
// scripts/movement-parity.js checks every tick against the client's own
// classes.

mod autojump;
mod inside;
mod motion;
pub mod read;
mod vanilla;
pub mod script;

use crate::dir::Pos;
use crate::level::Level;
use crate::mth;
use crate::phys::{self, Aabb, Vec3};
use crate::registry::{Fluid, State};
use std::collections::VecDeque;

// The options the player reads (Options): auto-jump, off by default.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub auto_jump: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { auto_jump: false }
    }
}

// Options.sprintWindow's default: the ticks a double-tapped forward key counts.
const SPRINT_WINDOW: i32 = 7;

// Input: the keys held (net.minecraft.world.entity.player.Input).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keys {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub shift: bool,
    pub sprint: bool,
}

impl Keys {
    // The bits of the protocol's input flags: forward, backward, left, right,
    // jump, shift, sprint.
    pub fn from_bits(bits: u32) -> Keys {
        Keys {
            forward: bits & 1 != 0,
            backward: bits & 2 != 0,
            left: bits & 4 != 0,
            right: bits & 8 != 0,
            jump: bits & 16 != 0,
            shift: bits & 32 != 0,
            sprint: bits & 64 != 0,
        }
    }
}

// What one tick takes: the keys held and where the player looks.
#[derive(Clone, Copy, Debug)]
pub struct Input {
    pub keys: Keys,
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pose {
    Standing,
    Crouching,
    Swimming,
}

impl Pose {
    // The pose's dimensions (Avatar.POSES): width, height, eye height.
    fn dimensions(self) -> (f32, f32, f32) {
        match self {
            Pose::Standing => (0.6f32, 1.8f32, 1.62f32),
            Pose::Crouching => (0.6f32, 1.5f32, 1.27f32),
            Pose::Swimming => (0.6f32, 0.6f32, 0.4f32),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Pose::Standing => "STANDING",
            Pose::Crouching => "CROUCHING",
            Pose::Swimming => "SWIMMING",
        }
    }
}

// Entity.Movement.
#[derive(Clone, Copy)]
struct Movement {
    from: Vec3,
    to: Vec3,
    axis_dependent_original: Option<Vec3>,
}

// The index of a fluid tag in Entity.fluidHeight and fluidOnEyes.
const WATER: usize = 0;
const LAVA: usize = 1;

pub struct Player {
    pub settings: Settings,
    // Entity.
    pub pos: Vec3,
    block_pos: Pos,
    pub bb: Aabb,
    pub delta: Vec3,
    pub y_rot: f32,
    pub x_rot: f32,
    pub y_rot_o: f32,
    pub x_rot_o: f32,
    pub xo: f64,
    pub yo: f64,
    pub zo: f64,
    pub on_ground: bool,
    pub horizontal_collision: bool,
    pub vertical_collision: bool,
    pub vertical_collision_below: bool,
    minor_horizontal_collision: bool,
    pub fall_distance: f64,
    stuck_speed_multiplier: Vec3,
    tick_count: i32,
    first_tick: bool,
    pub pose: Pose,
    width: f32,
    height: f32,
    eye_height: f32,
    sprinting: bool,
    swimming: bool,
    was_touching_water: bool,
    fluid_height: [f64; 2],
    fluid_on_eyes: [bool; 2],
    was_eye_in_water: bool,
    is_in_powder_snow: bool,
    was_in_powder_snow: bool,
    main_supporting_block: Option<Pos>,
    on_ground_no_blocks: bool,
    movements_this_tick: VecDeque<Movement>,
    in_block_state: Option<State>,
    has_impulse: bool,
    // Entity.pistonDeltas and pistonDeltasGameTime: how far pistons have moved
    // the player along each axis in the game time they last pushed it.
    piston_deltas: [f64; 3],
    piston_deltas_game_time: i64,
    // LivingEntity.
    jumping: bool,
    xxa: f32,
    zza: f32,
    no_jump_delay: i32,
    // Player: the abilities, jumpTriggerTime and wasUnderwater.
    pub flying: bool,
    jump_trigger_time: i32,
    was_underwater: bool,
    // LocalPlayer.
    keys: Keys,
    move_vector: (f32, f32),
    crouching: bool,
    sprint_trigger_time: i32,
    auto_jump_time: i32,
    auto_jump_enabled: bool,
}

// Creative mode's abilities (GameType.updatePlayerAbilities and the defaults).
const FLYING_SPEED: f32 = 0.05f32;

// Math.min for floats.
fn jminf(a: f32, b: f32) -> f32 {
    if a != a {
        return a;
    }
    if a == 0.0 && b == 0.0 && b.is_sign_negative() {
        return b;
    }
    if a <= b {
        a
    } else {
        b
    }
}

// Math.signum(double).
fn signum(d: f64) -> f64 {
    if d == 0.0 || d != d {
        d
    } else if d > 0.0 {
        1.0
    } else {
        -1.0
    }
}

// What a view of the player reads after a tick: where it is and was (a
// frame between two ticks draws the position and the look between the old
// and the new), how it looks and moves. `encode` is the same as numbers, in
// this order.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub pos: Vec3,
    pub old_pos: Vec3,
    pub delta: Vec3,
    pub y_rot: f32,
    pub x_rot: f32,
    pub y_rot_o: f32,
    pub x_rot_o: f32,
    pub width: f32,
    pub height: f32,
    pub eye_height: f32,
    pub pose: Pose,
    pub flying: bool,
    pub sprinting: bool,
    pub crouching: bool,
    pub swimming: bool,
    pub on_ground: bool,
    pub in_water: bool,
    pub in_lava: bool,
    pub eye_in_water: bool,
    pub eye_in_lava: bool,
    pub horizontal_collision: bool,
    pub fall_distance: f64,
}

impl View {
    // x, y, z, old x, old y, old z, dx, dy, dz, yaw, pitch, old yaw, old pitch,
    // width, height, eye height, pose (0 standing, 1 crouching, 2 swimming),
    // then flying, sprinting, crouching, swimming, on ground, in water, in
    // lava, eye in water, eye in lava and horizontal collision as 0 or 1,
    // and the fall distance.
    pub fn encode(&self) -> [f64; 28] {
        let flag = |b: bool| if b { 1.0 } else { 0.0 };
        [
            self.pos.x,
            self.pos.y,
            self.pos.z,
            self.old_pos.x,
            self.old_pos.y,
            self.old_pos.z,
            self.delta.x,
            self.delta.y,
            self.delta.z,
            self.y_rot as f64,
            self.x_rot as f64,
            self.y_rot_o as f64,
            self.x_rot_o as f64,
            self.width as f64,
            self.height as f64,
            self.eye_height as f64,
            match self.pose {
                Pose::Standing => 0.0,
                Pose::Crouching => 1.0,
                Pose::Swimming => 2.0,
            },
            flag(self.flying),
            flag(self.sprinting),
            flag(self.crouching),
            flag(self.swimming),
            flag(self.on_ground),
            flag(self.in_water),
            flag(self.in_lava),
            flag(self.eye_in_water),
            flag(self.eye_in_lava),
            flag(self.horizontal_collision),
            self.fall_distance,
        ]
    }
}

impl Player {
    pub fn view(&self) -> View {
        View {
            pos: self.pos,
            old_pos: Vec3::new(self.xo, self.yo, self.zo),
            delta: self.delta,
            y_rot: self.y_rot,
            x_rot: self.x_rot,
            y_rot_o: self.y_rot_o,
            x_rot_o: self.x_rot_o,
            width: self.width,
            height: self.height,
            eye_height: self.eye_height,
            pose: self.pose,
            flying: self.flying,
            sprinting: self.sprinting,
            crouching: self.crouching,
            swimming: self.is_swimming(),
            on_ground: self.on_ground,
            in_water: self.was_touching_water,
            in_lava: self.is_in_lava(),
            eye_in_water: self.fluid_on_eyes[WATER],
            eye_in_lava: self.fluid_on_eyes[LAVA],
            horizontal_collision: self.horizontal_collision,
            fall_distance: self.fall_distance,
        }
    }

    // A creative player standing at `pos`, in the state a LocalPlayer made
    // for the case has before its first tick.
    pub fn new(pos: Vec3, yaw: f32, pitch: f32, flying: bool, settings: Settings) -> Player {
        let (width, height, eye_height) = Pose::Standing.dimensions();
        let mut p = Player {
            settings,
            pos: Vec3::ZERO,
            block_pos: Pos::new(0, 0, 0),
            bb: Aabb::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
            delta: Vec3::ZERO,
            y_rot: 0.0,
            x_rot: 0.0,
            y_rot_o: 0.0,
            x_rot_o: 0.0,
            xo: 0.0,
            yo: 0.0,
            zo: 0.0,
            on_ground: false,
            horizontal_collision: false,
            vertical_collision: false,
            vertical_collision_below: false,
            minor_horizontal_collision: false,
            fall_distance: 0.0,
            stuck_speed_multiplier: Vec3::ZERO,
            tick_count: 0,
            first_tick: true,
            pose: Pose::Standing,
            width,
            height,
            eye_height,
            sprinting: false,
            swimming: false,
            was_touching_water: false,
            fluid_height: [0.0; 2],
            fluid_on_eyes: [false; 2],
            was_eye_in_water: false,
            is_in_powder_snow: false,
            was_in_powder_snow: false,
            main_supporting_block: None,
            on_ground_no_blocks: false,
            movements_this_tick: VecDeque::new(),
            in_block_state: None,
            has_impulse: false,
            piston_deltas: [0.0; 3],
            piston_deltas_game_time: 0,
            jumping: false,
            xxa: 0.0,
            zza: 0.0,
            no_jump_delay: 0,
            flying,
            jump_trigger_time: 0,
            was_underwater: false,
            keys: Keys::default(),
            move_vector: (0.0, 0.0),
            crouching: false,
            sprint_trigger_time: 0,
            auto_jump_time: 0,
            auto_jump_enabled: true,
        };
        p.set_pos(pos.x, pos.y, pos.z);
        p.set_y_rot(yaw);
        p.set_x_rot(pitch);
        p.set_old_pos_and_rot();
        p
    }

    pub fn eye_height(&self) -> f32 {
        self.eye_height
    }

    pub fn is_sprinting(&self) -> bool {
        self.sprinting
    }

    pub fn is_crouching(&self) -> bool {
        self.crouching
    }

    pub fn is_swimming(&self) -> bool {
        !self.flying && self.swimming
    }

    // What the parity traces print of the player's state (see script.rs).
    pub fn timers(&self) -> (f32, f32, i32, i32, i32, i32) {
        (self.xxa, self.zza, self.jump_trigger_time, self.sprint_trigger_time, self.no_jump_delay, self.auto_jump_time)
    }

    pub fn fluid_heights(&self) -> (f64, f64) {
        (self.fluid_height[WATER], self.fluid_height[LAVA])
    }

    pub fn fluids_on_eyes(&self) -> (bool, bool) {
        (self.fluid_on_eyes[WATER], self.fluid_on_eyes[LAVA])
    }

    pub fn is_touching_water(&self) -> bool {
        self.was_touching_water
    }

    pub fn supporting_block(&self) -> Option<Pos> {
        self.main_supporting_block
    }

    pub fn block_position(&self) -> Pos {
        self.block_pos
    }

    pub fn stuck(&self) -> Vec3 {
        self.stuck_speed_multiplier
    }

    pub fn in_block(&self) -> Option<State> {
        self.in_block_state
    }

    pub fn minor_horizontal_collision(&self) -> bool {
        self.minor_horizontal_collision
    }

    pub fn width(&self) -> f32 {
        self.width
    }

    pub fn height(&self) -> f32 {
        self.height
    }

    // Entity.setPosRaw.
    fn set_pos_raw(&mut self, x: f64, y: f64, z: f64) {
        if self.pos.x != x || self.pos.y != y || self.pos.z != z {
            self.pos = Vec3::new(x, y, z);
            let (fx, fy, fz) = (phys::floor(x), phys::floor(y), phys::floor(z));
            if fx != self.block_pos.x() || fy != self.block_pos.y() || fz != self.block_pos.z() {
                self.block_pos = Pos::new(fx, fy, fz);
                self.in_block_state = None;
            }
        }
    }

    // EntityDimensions.makeBoundingBox.
    fn make_bounding_box(&self, p: Vec3, width: f32, height: f32) -> Aabb {
        let half = width / 2.0f32;
        Aabb::new(p.x - half as f64, p.y, p.z - half as f64, p.x + half as f64, p.y + height as f64, p.z + half as f64)
    }

    // Entity.setPos.
    fn set_pos(&mut self, x: f64, y: f64, z: f64) {
        self.set_pos_raw(x, y, z);
        self.bb = self.make_bounding_box(self.pos, self.width, self.height);
    }

    fn set_pos_vec(&mut self, p: Vec3) {
        self.set_pos(p.x, p.y, p.z);
    }

    // Entity.setYRot and setXRot (a rotation that is not finite is discarded).
    fn set_y_rot(&mut self, yaw: f32) {
        if yaw.is_finite() {
            self.y_rot = yaw;
        }
    }

    fn set_x_rot(&mut self, pitch: f32) {
        if pitch.is_finite() {
            // Math.clamp(x % 360.0F, -90.0F, 90.0F).
            self.x_rot = (pitch % 360.0f32).clamp(-90.0f32, 90.0f32);
        }
    }

    fn set_old_pos_and_rot(&mut self) {
        self.xo = self.pos.x;
        self.yo = self.pos.y;
        self.zo = self.pos.z;
        self.y_rot_o = self.y_rot;
        self.x_rot_o = self.x_rot;
    }

    // Entity.reapplyPosition.
    fn reapply_position(&mut self) {
        self.set_pos(self.pos.x, self.pos.y, self.pos.z);
    }

    // Entity.setPose with the refreshDimensions it triggers on a change.
    fn set_pose(&mut self, pose: Pose) {
        if self.pose != pose {
            self.pose = pose;
            let (width, height, eye_height) = pose.dimensions();
            self.width = width;
            self.height = height;
            self.eye_height = eye_height;
            self.reapply_position();
        }
    }

    // Entity.getEyeY.
    fn eye_y(&self) -> f64 {
        self.pos.y + self.eye_height as f64
    }

    // Entity.getInBlockState: the block at the player's block position, read
    // once and kept until the player's block position changes or its next tick.
    fn in_block_state(&mut self, l: &Level) -> State {
        if let Some(state) = self.in_block_state {
            return state;
        }
        let state = read::state(l, self.block_pos);
        self.in_block_state = Some(state);
        state
    }

    // The attribute values of a player (movement speed with the sprinting
    // modifier), as AttributeInstance.calculateValue computes them.
    fn movement_speed(&self) -> f32 {
        // The base 0.1F as a double, times 1 + 0.3F (ADD_MULTIPLIED_TOTAL).
        let mut value = 0.1f32 as f64;
        if self.sprinting {
            value *= 1.0 + 0.3f32 as f64;
        }
        value as f32
    }

    // Player.getSpeed.
    fn speed(&self) -> f32 {
        self.movement_speed()
    }

    // LivingEntity.setSprinting (the shared flag, and the speed modifier that
    // movement_speed derives from it).
    fn set_sprinting(&mut self, sprinting: bool) {
        self.sprinting = sprinting;
    }

    // Player.getFlyingSpeed.
    fn flying_speed(&self) -> f32 {
        if self.flying {
            if self.sprinting {
                FLYING_SPEED * 2.0f32
            } else {
                FLYING_SPEED
            }
        } else if self.sprinting {
            0.025999999f32
        } else {
            0.02f32
        }
    }

    // LocalPlayer.isShiftKeyDown.
    fn is_shift_key_down(&self) -> bool {
        self.keys.shift
    }

    // Player.isAffectedByFluids and isPushedByFluid.
    fn is_affected_by_fluids(&self) -> bool {
        !self.flying
    }

    fn is_in_water(&self) -> bool {
        self.was_touching_water
    }

    // Entity.isInLava.
    fn is_in_lava(&self) -> bool {
        !self.first_tick && self.fluid_height[LAVA] > 0.0
    }

    // LocalPlayer.isUnderWater.
    fn is_under_water(&self) -> bool {
        self.was_underwater
    }

    fn is_eye_in_fluid(&self, fluid: usize) -> bool {
        self.fluid_on_eyes[fluid]
    }

    // Entity.getFluidJumpThreshold.
    fn fluid_jump_threshold(&self) -> f64 {
        if (self.eye_height as f64) < 0.4 {
            0.0
        } else {
            0.4
        }
    }

    // LocalPlayer.isMovingSlowly.
    fn is_moving_slowly(&self) -> bool {
        self.crouching || self.is_visually_crawling()
    }

    // Entity.isVisuallyCrawling.
    fn is_visually_crawling(&self) -> bool {
        self.pose == Pose::Swimming && !self.is_in_water()
    }

    // ClientInput.hasForwardImpulse.
    fn has_forward_impulse(&self) -> bool {
        self.move_vector.1 > 1.0E-5f32
    }

    // The position and look an observer reads.
    pub fn look_angle(&self) -> Vec3 {
        self.calculate_view_vector(self.x_rot, self.y_rot)
    }

    // Entity.calculateViewVector.
    fn calculate_view_vector(&self, x_rot: f32, y_rot: f32) -> Vec3 {
        let f = x_rot * (std::f64::consts::PI / 180.0) as f32;
        let f1 = -y_rot * (std::f64::consts::PI / 180.0) as f32;
        let f2 = mth::cos(f1);
        let f3 = mth::sin(f1);
        let f4 = mth::cos(f);
        let f5 = mth::sin(f);
        Vec3::new((f3 * f4) as f64, (-f5) as f64, (f2 * f4) as f64)
    }

    // ClientLevel.tickNonPassenger and LocalPlayer.tick for the keys held and
    // the look the mouse left: one client tick.
    pub fn tick(&mut self, l: &Level, input: Input) {
        self.set_y_rot(input.yaw);
        self.set_x_rot(input.pitch);
        // The keys the next input.tick() reads.
        let next = input.keys;
        self.set_old_pos_and_rot();
        self.tick_count += 1;
        self.player_tick(l, next);
        // LocalPlayer.sendPosition: the option is read at the end of a tick.
        self.auto_jump_enabled = self.settings.auto_jump;
    }

    // Player.tick (LocalPlayer.tick and AbstractClientPlayer.tick only add what
    // reaches the connection).
    fn player_tick(&mut self, l: &Level, next: Keys) {
        // updateIsUnderwater: the eyes' fluid of the last tick.
        self.was_underwater = self.is_eye_in_fluid(WATER);
        // LivingEntity.tick.
        self.base_tick(l);
        self.ai_step(l, next);
        // Player.tick: the position is kept within the world's reach.
        let x = phys::clamp(self.pos.x, -2.9999999E7, 2.9999999E7);
        let z = phys::clamp(self.pos.z, -2.9999999E7, 2.9999999E7);
        if x != self.pos.x || z != self.pos.z {
            self.set_pos(x, self.pos.y, z);
        }
        self.update_player_pose(l);
    }

    // Entity.baseTick for a player on the client (LivingEntity.baseTick adds
    // only what the server does).
    fn base_tick(&mut self, l: &Level) {
        self.in_block_state = None;
        self.was_in_powder_snow = self.is_in_powder_snow;
        self.is_in_powder_snow = false;
        self.update_in_water_state_and_do_fluid_pushing(l);
        self.update_fluid_on_eyes(l);
        self.update_swimming(l);
        if self.is_in_lava() {
            self.fall_distance *= 0.5;
        }
        self.first_tick = false;
    }

    // Player.updateSwimming and Entity.updateSwimming.
    fn update_swimming(&mut self, l: &Level) {
        if self.flying {
            self.swimming = false;
        } else if self.is_swimming() {
            self.swimming = self.sprinting && self.is_in_water();
        } else {
            self.swimming = self.sprinting && self.is_under_water() && read::fluid_state_is_water(l, self.block_pos);
        }
    }

    // Entity.updateFluidOnEyes.
    fn update_fluid_on_eyes(&mut self, l: &Level) {
        self.was_eye_in_water = self.is_eye_in_fluid(WATER);
        self.fluid_on_eyes = [false; 2];
        let eye_y = self.eye_y();
        let at = Pos::new(phys::floor(self.pos.x), phys::floor(eye_y), phys::floor(self.pos.z));
        let fluid = read::fluid_state(l, at);
        let height = read::fluid_height(l, fluid, at);
        let d = (at.y() as f32 + height) as f64;
        if d > eye_y {
            match l.fluid_type(fluid) {
                Fluid::FlowingWater | Fluid::Water => self.fluid_on_eyes[WATER] = true,
                Fluid::FlowingLava | Fluid::Lava => self.fluid_on_eyes[LAVA] = true,
                Fluid::Empty => {}
            }
        }
    }

    // Entity.updateInWaterStateAndDoFluidPushing.
    fn update_in_water_state_and_do_fluid_pushing(&mut self, l: &Level) {
        self.fluid_height = [0.0; 2];
        self.update_in_water_state_and_do_water_current_pushing(l);
        // The dimension is not ultra warm.
        self.update_fluid_height_and_do_fluid_pushing(l, LAVA, 0.0023333333333333335);
    }

    // Entity.updateInWaterStateAndDoWaterCurrentPushing.
    pub(super) fn update_in_water_state_and_do_water_current_pushing(&mut self, l: &Level) {
        if self.update_fluid_height_and_do_fluid_pushing(l, WATER, 0.014) {
            // (doWaterSplashEffect plays sounds and makes particles.)
            self.fall_distance = 0.0;
            self.was_touching_water = true;
        } else {
            self.was_touching_water = false;
        }
    }

    // Entity.updateFluidHeightAndDoFluidPushing for a player.
    fn update_fluid_height_and_do_fluid_pushing(&mut self, l: &Level, tag: usize, flow_scale: f64) -> bool {
        // (touchingUnloadedChunk: every chunk is there.)
        let bb = self.bb.deflate(0.001);
        let min_x = phys::floor(bb.min_x);
        let max_x = phys::ceil(bb.max_x);
        let min_y = phys::floor(bb.min_y);
        let max_y = phys::ceil(bb.max_y);
        let min_z = phys::floor(bb.min_z);
        let max_z = phys::ceil(bb.max_z);
        let mut max_height = 0.0f64;
        let pushed = self.is_affected_by_fluids();
        let mut in_fluid = false;
        let mut push = Vec3::ZERO;
        let mut count = 0;
        for x in min_x..max_x {
            for y in min_y..max_y {
                for z in min_z..max_z {
                    let pos = Pos::new(x, y, z);
                    let fluid = read::fluid_state(l, pos);
                    let kind = l.fluid_type(fluid);
                    if !(if tag == WATER { kind.is_water() } else { kind.is_lava() }) {
                        continue;
                    }
                    let height = (y as f32 + read::fluid_height(l, fluid, pos)) as f64;
                    if height >= bb.min_y {
                        in_fluid = true;
                        max_height = phys::jmax(height - bb.min_y, max_height);
                        if pushed {
                            let mut flow = read::fluid_flow(l, pos, fluid);
                            if max_height < 0.4 {
                                flow = flow.scale(max_height);
                            }
                            push = push.add_vec(flow);
                            count += 1;
                        }
                    }
                }
            }
        }
        if push.length() > 0.0 {
            if count > 0 {
                push = push.scale(1.0 / count as f64);
            }
            // (A player's push is not normalized.)
            let current = self.delta;
            push = push.scale(flow_scale);
            if current.x.abs() < 0.003 && current.z.abs() < 0.003 && push.length() < 0.0045000000000000005 {
                push = push.normalize().scale(0.0045000000000000005);
            }
            self.delta = self.delta.add_vec(push);
        }
        self.fluid_height[tag] = max_height;
        in_fluid
    }

    // LocalPlayer.modifyInput.
    fn modify_input(&self, v: (f32, f32)) -> (f32, f32) {
        if v.0 * v.0 + v.1 * v.1 == 0.0f32 {
            return v;
        }
        // scale(0.98F).
        let mut v = (v.0 * 0.98f32, v.1 * 0.98f32);
        if self.is_moving_slowly() {
            // Attributes.SNEAKING_SPEED's default, 0.3.
            let factor = 0.3f64 as f32;
            v = (v.0 * factor, v.1 * factor);
        }
        // modifyInputSpeedForSquareMovement.
        let length = mth_sqrt(v.0 * v.0 + v.1 * v.1);
        if length <= 0.0f32 {
            return v;
        }
        let unit = (v.0 * (1.0f32 / length), v.1 * (1.0f32 / length));
        let (x, y) = (unit.0.abs(), unit.1.abs());
        let t = if y > x { x / y } else { y / x };
        let distance = mth_sqrt(1.0f32 + t * t);
        let scaled = jminf(length * distance, 1.0f32);
        (unit.0 * scaled, unit.1 * scaled)
    }

    // KeyboardInput.tick: the keys and the move vector they make.
    fn input_tick(&mut self, keys: Keys) {
        self.keys = keys;
        let impulse = |positive: bool, negative: bool| if positive == negative { 0.0f32 } else if positive { 1.0f32 } else { -1.0f32 };
        let forward = impulse(keys.forward, keys.backward);
        let left = impulse(keys.left, keys.right);
        // Vec2.normalized.
        let length = mth_sqrt(left * left + forward * forward);
        self.move_vector = if length < 1.0E-4f32 { (0.0, 0.0) } else { (left / length, forward / length) };
    }

    // LocalPlayer.aiStep.
    fn ai_step(&mut self, l: &Level, next: Keys) {
        if self.sprint_trigger_time > 0 {
            self.sprint_trigger_time -= 1;
        }
        // (The portal effect and the portal cooldown reach no one here.)
        let was_jump = self.keys.jump;
        let was_shift = self.keys.shift;
        let had_forward_impulse = self.has_forward_impulse();
        self.crouching = !self.flying
            && !self.is_swimming()
            && self.can_player_fit_within_blocks_and_entities_when(l, Pose::Crouching)
            && (self.is_shift_key_down() || !self.can_player_fit_within_blocks_and_entities_when(l, Pose::Standing));
        self.input_tick(next);
        let mut auto_jumping = false;
        if self.auto_jump_time > 0 {
            self.auto_jump_time -= 1;
            auto_jumping = true;
            // ClientInput.makeJump.
            self.keys.jump = true;
        }
        self.move_towards_closest_space_all(l);
        if was_shift || self.keys.backward {
            self.sprint_trigger_time = 0;
        }
        if self.can_start_sprinting() {
            if !had_forward_impulse {
                if self.sprint_trigger_time > 0 {
                    self.set_sprinting(true);
                } else {
                    self.sprint_trigger_time = SPRINT_WINDOW;
                }
            }
            if self.keys.sprint {
                self.set_sprinting(true);
            }
        }
        if self.sprinting {
            if self.is_swimming() {
                if self.should_stop_swim_sprinting() {
                    self.set_sprinting(false);
                }
            } else if self.should_stop_run_sprinting() {
                self.set_sprinting(false);
            }
        }
        let mut toggled = false;
        // (mayfly: a creative player.)
        if !was_jump && self.keys.jump && !auto_jumping {
            if self.jump_trigger_time == 0 {
                self.jump_trigger_time = 7;
            } else if !self.is_swimming() {
                self.flying = !self.flying;
                if self.flying && self.on_ground {
                    self.jump_from_ground(l);
                }
                toggled = true;
                self.jump_trigger_time = 0;
            }
        }
        // tryToStartFallFlying needs an elytra; the check reads the block the
        // player is in first (onClimbable), which the later reads keep.
        if self.keys.jump && !toggled && !was_jump {
            self.on_climbable(l);
        }
        if self.is_in_water() && self.keys.shift && self.is_affected_by_fluids() {
            // goDownInWater.
            self.delta = self.delta.add(0.0, -0.04f32 as f64, 0.0);
        }
        if self.flying {
            let mut up = 0;
            if self.keys.shift {
                up -= 1;
            }
            if self.keys.jump {
                up += 1;
            }
            if up != 0 {
                self.delta = self.delta.add(0.0, ((up as f32 * self.flying_speed_ability()) * 3.0f32) as f64, 0.0);
            }
        }
        self.player_ai_step(l);
        if self.on_ground && self.flying {
            self.flying = false;
        }
    }

    // Abilities.getFlyingSpeed.
    fn flying_speed_ability(&self) -> f32 {
        FLYING_SPEED
    }

    // LocalPlayer.shouldStopRunSprinting.
    fn should_stop_run_sprinting(&self) -> bool {
        !self.is_sprinting_possible(self.flying) || !self.has_forward_impulse() || self.horizontal_collision && !self.minor_horizontal_collision
    }

    // LocalPlayer.shouldStopSwimSprinting.
    fn should_stop_swim_sprinting(&self) -> bool {
        !self.is_sprinting_possible(true) || !self.is_in_water() || !self.has_forward_impulse() && !self.on_ground && !self.keys.shift
    }

    // LocalPlayer.isSprintingPossible: a creative player has food enough.
    fn is_sprinting_possible(&self, flying: bool) -> bool {
        flying || !self.is_in_shallow_water()
    }

    // Entity.isInShallowWater.
    fn is_in_shallow_water(&self) -> bool {
        self.is_in_water() && !self.is_under_water()
    }

    // LocalPlayer.canStartSprinting.
    fn can_start_sprinting(&self) -> bool {
        !self.sprinting && self.has_forward_impulse() && self.is_sprinting_possible(self.flying) && (!self.is_moving_slowly() || self.is_under_water())
    }

    // Player.aiStep.
    fn player_ai_step(&mut self, l: &Level) {
        if self.jump_trigger_time > 0 {
            self.jump_trigger_time -= 1;
        }
        if self.flying {
            self.fall_distance = 0.0;
        }
        self.living_ai_step(l);
        // (The entities the player touches are picked up by the server.)
    }

    // LivingEntity.aiStep for a player the client moves.
    fn living_ai_step(&mut self, l: &Level) {
        if self.no_jump_delay > 0 {
            self.no_jump_delay -= 1;
        }
        let (mut x, mut y, mut z) = (self.delta.x, self.delta.y, self.delta.z);
        if self.delta.horizontal_distance_sqr() < 9.0E-6 {
            x = 0.0;
            z = 0.0;
        }
        if self.delta.y.abs() < 0.003 {
            y = 0.0;
        }
        self.delta = Vec3::new(x, y, z);
        // applyInput (LocalPlayer's).
        let v = self.modify_input(self.move_vector);
        self.xxa = v.0;
        self.zza = v.1;
        self.jumping = self.keys.jump;
        // The jump.
        if self.jumping && self.is_affected_by_fluids() {
            let fluid_height = if self.is_in_lava() { self.fluid_height[LAVA] } else { self.fluid_height[WATER] };
            let in_water = self.is_in_water() && fluid_height > 0.0;
            let threshold = self.fluid_jump_threshold();
            if !in_water || self.on_ground && !(fluid_height > threshold) {
                if !self.is_in_lava() || self.on_ground && !(fluid_height > threshold) {
                    if (self.on_ground || in_water && fluid_height <= threshold) && self.no_jump_delay == 0 {
                        self.jump_from_ground(l);
                        self.no_jump_delay = 10;
                    }
                } else {
                    self.delta = self.delta.add(0.0, 0.04f32 as f64, 0.0);
                }
            } else {
                self.delta = self.delta.add(0.0, 0.04f32 as f64, 0.0);
            }
        } else {
            self.no_jump_delay = 0;
        }
        // travel.
        let input = Vec3::new(self.xxa as f64, 0.0, self.zza as f64);
        self.player_travel(l, input);
        self.apply_effects_from_blocks(l);
        // (pushEntities: no entity is near.)
        self.check_no_entities(l);
    }

    // LivingEntity.jumpFromGround.
    fn jump_from_ground(&mut self, l: &Level) {
        let power = self.jump_power(l);
        if !(power <= 1.0E-5f32) {
            let d = self.delta;
            self.delta = Vec3::new(d.x, phys::jmax(power as f64, d.y), d.z);
            if self.sprinting {
                let f = self.y_rot * (std::f64::consts::PI / 180.0) as f32;
                self.delta = self.delta.add_vec(Vec3::new((-mth::sin(f)) as f64 * 0.2, 0.0, mth::cos(f) as f64 * 0.2));
            }
            self.has_impulse = true;
        }
    }

    // LivingEntity.getJumpPower: the jump strength attribute's 0.42F, the
    // block's jump factor, no jump boost.
    fn jump_power(&self, l: &Level) -> f32 {
        0.42f32 * 1.0f32 * self.block_jump_factor(l) + 0.0f32
    }

    // Player.travel.
    fn player_travel(&mut self, l: &Level, input: Vec3) {
        if self.is_swimming() {
            let y = self.look_angle().y;
            let f = if y < -0.2 { 0.085 } else { 0.06 };
            let below = Pos::new(phys::floor(self.pos.x), phys::floor(self.pos.y + 1.0 - 0.1), phys::floor(self.pos.z));
            if y <= 0.0 || self.jumping || read::fluid_state(l, below) != crate::registry::EMPTY_FLUID {
                let d = self.delta;
                self.delta = d.add(0.0, (y - d.y) * f, 0.0);
            }
        }
        if self.flying {
            let y = self.delta.y;
            self.living_travel(l, input);
            self.delta = self.delta.with(1, y * 0.6);
        } else {
            self.living_travel(l, input);
        }
    }

    // LivingEntity.travel.
    fn living_travel(&mut self, l: &Level, input: Vec3) {
        let at = self.block_pos;
        let fluid = read::fluid_state(l, at);
        let _ = fluid;
        // (canStandOnFluid is false for a player.)
        if (self.is_in_water() || self.is_in_lava()) && self.is_affected_by_fluids() {
            self.travel_in_fluid(l, input);
        } else {
            self.travel_in_air(l, input);
        }
    }

    // Entity.moveRelative.
    fn move_relative(&mut self, speed: f32, input: Vec3) {
        let v = get_input_vector(input, speed, self.y_rot);
        self.delta = self.delta.add_vec(v);
    }

    // LivingEntity.getEffectiveGravity: no slow falling.
    fn effective_gravity(&self) -> f64 {
        0.08
    }

    // LivingEntity.travelInAir.
    fn travel_in_air(&mut self, l: &Level, input: Vec3) {
        let below = self.block_pos_below_that_affects_my_movement(l);
        let friction = if self.on_ground { l.reg.blocks[l.block(read::state(l, below)) as usize].friction } else { 1.0f32 };
        let f3 = friction * 0.91f32;
        let d = self.handle_relative_friction_and_calculate_movement(l, input, friction);
        let mut y = d.y;
        // (No levitation; the chunk below is there.)
        y -= self.effective_gravity();
        // (shouldDiscardFriction is false; not a flying animal.)
        self.delta = Vec3::new(d.x * f3 as f64, y * 0.98f32 as f64, d.z * f3 as f64);
    }

    // LivingEntity.handleRelativeFrictionAndCalculateMovement.
    fn handle_relative_friction_and_calculate_movement(&mut self, l: &Level, input: Vec3, friction: f32) -> Vec3 {
        let speed = self.friction_influenced_speed(friction);
        self.move_relative(speed, input);
        self.delta = self.handle_on_climbable(l, self.delta);
        self.move_entity(l, self.delta);
        let mut d = self.delta;
        if (self.horizontal_collision || self.jumping) && (self.on_climbable(l) || self.was_in_powder_snow && false) {
            d = Vec3::new(d.x, 0.2, d.z);
        }
        d
    }

    // LivingEntity.getFrictionInfluencedSpeed.
    fn friction_influenced_speed(&self, friction: f32) -> f32 {
        if self.on_ground {
            self.speed() * (0.21600002f32 / (friction * friction * friction))
        } else {
            self.flying_speed()
        }
    }

    // LivingEntity.handleOnClimbable.
    fn handle_on_climbable(&mut self, l: &Level, d: Vec3) -> Vec3 {
        if self.on_climbable(l) {
            self.fall_distance = 0.0;
            let x = phys::clamp(d.x, -0.15f32 as f64, 0.15f32 as f64);
            let z = phys::clamp(d.z, -0.15f32 as f64, 0.15f32 as f64);
            let mut y = phys::jmax(d.y, -0.15f32 as f64);
            // LocalPlayer.isSuppressingSlidingDownLadder: shift, not flying.
            if y < 0.0 && l.reg.blocks[l.block(self.in_block_state(l)) as usize].key != "minecraft:scaffolding" && !self.flying && self.is_shift_key_down() {
                y = 0.0;
            }
            Vec3::new(x, y, z)
        } else {
            d
        }
    }

    // Player.onClimbable (a flying player climbs nothing) and
    // LivingEntity.onClimbable.
    fn on_climbable(&mut self, l: &Level) -> bool {
        if self.flying {
            return false;
        }
        let pos = self.block_pos;
        let state = self.in_block_state(l);
        if l.reg.in_tag(state, "minecraft:climbable") {
            return true;
        }
        if l.reg.blocks[l.block(state) as usize].class == "TrapDoorBlock" && self.trapdoor_usable_as_ladder(l, pos, state) {
            return true;
        }
        false
    }

    // LivingEntity.trapdoorUsableAsLadder.
    fn trapdoor_usable_as_ladder(&self, l: &Level, pos: Pos, state: State) -> bool {
        let reg = l.reg;
        if !reg.get_bool(state, reg.property_of(state, "open")) {
            return false;
        }
        let below = read::state(l, pos.below());
        reg.blocks[l.block(below) as usize].key == "minecraft:ladder" && reg.get_dir(below, reg.property_of(below, "facing")) == reg.get_dir(state, reg.property_of(state, "facing"))
    }

    // LivingEntity.travelInFluid.
    fn travel_in_fluid(&mut self, l: &Level, input: Vec3) {
        let falling = self.delta.y <= 0.0;
        let y = self.pos.y;
        let gravity = self.effective_gravity();
        if self.is_in_water() {
            let mut slow_down = if self.sprinting { 0.9f32 } else { 0.8f32 };
            let speed = 0.02f32;
            // (Water movement efficiency is 0.)
            self.move_relative(speed, input);
            self.move_entity(l, self.delta);
            let mut d = self.delta;
            if self.horizontal_collision && self.on_climbable(l) {
                d = Vec3::new(d.x, 0.2, d.z);
            }
            // (No dolphin's grace.)
            let _ = &mut slow_down;
            d = d.multiply(slow_down as f64, 0.8f32 as f64, slow_down as f64);
            self.delta = self.fluid_falling_adjusted_movement(gravity, falling, d);
        } else {
            self.move_relative(0.02f32, input);
            self.move_entity(l, self.delta);
            if self.fluid_height[LAVA] <= self.fluid_jump_threshold() {
                self.delta = self.delta.multiply(0.5, 0.8f32 as f64, 0.5);
                self.delta = self.fluid_falling_adjusted_movement(gravity, falling, self.delta);
            } else {
                self.delta = self.delta.scale(0.5);
            }
            if gravity != 0.0 {
                self.delta = self.delta.add(0.0, -gravity / 4.0, 0.0);
            }
        }
        let d = self.delta;
        if self.horizontal_collision && self.is_free(l, d.x, d.y + 0.6f32 as f64 - self.pos.y + y, d.z) {
            self.delta = Vec3::new(d.x, 0.3f32 as f64, d.z);
        }
    }

    // LivingEntity.getFluidFallingAdjustedMovement.
    fn fluid_falling_adjusted_movement(&self, gravity: f64, falling: bool, d: Vec3) -> Vec3 {
        if gravity != 0.0 && !self.sprinting {
            let y = if falling && (d.y - 0.005).abs() >= 0.003 && (d.y - gravity / 16.0).abs() < 0.003 { -0.003 } else { d.y - gravity / 16.0 };
            Vec3::new(d.x, y, d.z)
        } else {
            d
        }
    }

    // Player.updatePlayerPose.
    fn update_player_pose(&mut self, l: &Level) {
        if self.can_player_fit_within_blocks_and_entities_when(l, Pose::Swimming) {
            let desired = self.desired_pose();
            let pose = if self.can_player_fit_within_blocks_and_entities_when(l, desired) {
                desired
            } else if self.can_player_fit_within_blocks_and_entities_when(l, Pose::Crouching) {
                Pose::Crouching
            } else {
                Pose::Swimming
            };
            self.set_pose(pose);
        }
    }

    // Player.getDesiredPose.
    fn desired_pose(&self) -> Pose {
        if self.is_swimming() {
            Pose::Swimming
        } else if self.is_shift_key_down() && !self.flying {
            Pose::Crouching
        } else {
            Pose::Standing
        }
    }

    // Player.canPlayerFitWithinBlocksAndEntitiesWhen.
    fn can_player_fit_within_blocks_and_entities_when(&self, l: &Level, pose: Pose) -> bool {
        let (width, height, _) = pose.dimensions();
        let aabb = self.make_bounding_box(self.pos, width, height).deflate(1.0E-7);
        self.no_collision(l, &aabb)
    }

    // Entity.isFree(x, y, z).
    fn is_free(&self, l: &Level, x: f64, y: f64, z: f64) -> bool {
        let aabb = self.bb.mv(x, y, z);
        self.no_collision(l, &aabb) && !contains_any_liquid(l, &aabb)
    }

    // LivingEntity.pushEntities: a client pushes no entity and none pushes it
    // (ClientLevel.getPushableEntities holds only the local player).
    fn check_no_entities(&self, _l: &Level) {}

    // LocalPlayer.moveTowardsClosestSpace for the four corners.
    fn move_towards_closest_space_all(&mut self, l: &Level) {
        let w = self.width;
        let x = self.pos.x;
        let z = self.pos.z;
        self.move_towards_closest_space(l, x - (w as f64) * 0.35, z + (w as f64) * 0.35);
        self.move_towards_closest_space(l, x - (w as f64) * 0.35, z - (w as f64) * 0.35);
        self.move_towards_closest_space(l, x + (w as f64) * 0.35, z - (w as f64) * 0.35);
        self.move_towards_closest_space(l, x + (w as f64) * 0.35, z + (w as f64) * 0.35);
    }

    // LocalPlayer.moveTowardsClosestSpace.
    fn move_towards_closest_space(&mut self, l: &Level, x: f64, z: f64) {
        use crate::dir::Dir;
        let block = Pos::new(phys::floor(x), phys::floor(self.pos.y), phys::floor(z));
        if self.suffocates_at(l, block) {
            let dx = x - block.x() as f64;
            let dz = z - block.z() as f64;
            let mut best: Option<Dir> = None;
            let mut best_distance = f64::MAX;
            for dir in [Dir::West, Dir::East, Dir::North, Dir::South] {
                let along = match dir.axis() {
                    crate::dir::Axis::X => dx,
                    crate::dir::Axis::Y => 0.0,
                    crate::dir::Axis::Z => dz,
                };
                let positive = matches!(dir, Dir::East | Dir::South);
                let distance = if positive { 1.0 - along } else { along };
                if distance < best_distance && !self.suffocates_at(l, block.relative(dir)) {
                    best_distance = distance;
                    best = Some(dir);
                }
            }
            if let Some(dir) = best {
                let d = self.delta;
                let (sx, _, sz) = dir.offset();
                if dir.axis() == crate::dir::Axis::X {
                    self.delta = Vec3::new(0.1 * sx as f64, d.y, d.z);
                } else {
                    self.delta = Vec3::new(d.x, d.y, 0.1 * sz as f64);
                }
            }
        }
    }

    // LocalPlayer.suffocatesAt.
    fn suffocates_at(&self, l: &Level, pos: Pos) -> bool {
        let b = self.bb;
        let aabb = Aabb::new(pos.x() as f64, b.min_y, pos.z() as f64, pos.x() as f64 + 1.0, b.max_y, pos.z() as f64 + 1.0).deflate(1.0E-7);
        self.collides_with_suffocating_block(l, &aabb)
    }
}

// Mth.sqrt(float).
fn mth_sqrt(f: f32) -> f32 {
    (f as f64).sqrt() as f32
}

// Entity.getInputVector.
fn get_input_vector(input: Vec3, speed: f32, y_rot: f32) -> Vec3 {
    let d = input.length_sqr();
    if d < 1.0E-7 {
        return Vec3::ZERO;
    }
    let v = (if d > 1.0 { input.normalize() } else { input }).scale(speed as f64);
    let f = mth::sin(y_rot * (std::f64::consts::PI / 180.0) as f32);
    let f1 = mth::cos(y_rot * (std::f64::consts::PI / 180.0) as f32);
    Vec3::new(v.x * f1 as f64 - v.z * f as f64, v.y, v.z * f1 as f64 + v.x * f as f64)
}

// LevelReader.containsAnyLiquid.
fn contains_any_liquid(l: &Level, aabb: &Aabb) -> bool {
    let (min_x, max_x) = (phys::floor(aabb.min_x), phys::ceil(aabb.max_x));
    let (min_y, max_y) = (phys::floor(aabb.min_y), phys::ceil(aabb.max_y));
    let (min_z, max_z) = (phys::floor(aabb.min_z), phys::ceil(aabb.max_z));
    for x in min_x..max_x {
        for y in min_y..max_y {
            for z in min_z..max_z {
                if read::fluid_state(l, Pos::new(x, y, z)) != crate::registry::EMPTY_FLUID {
                    return true;
                }
            }
        }
    }
    false
}

// What the client's blocks decide that the registry does not hold, as runs
// of state ids (first, last) of 1.21.10 (`MovementReference --dump` writes
// them from the client; scripts/movement-parity.js checks the port against
// it). States whose isSuffocating differs from blocksMotion and a full
// collision shape, with the value, in the order the dump gives them:
const SUFFOCATING: &[(usize, usize, bool)] = &[
    (163, 164, false), (252, 559, false), (562, 562, false), (5118, 5125, true),
    (6797, 6797, true), (6897, 6912, false), (14613, 14613, true), (24486, 24486, false),
    (26845, 26860, false), (27720, 27720, true),
];
// States whose collision shape is Shapes.block() itself:
const BLOCK_SHAPE: &[(usize, usize)] = &[
    (1, 28), (85, 85), (118, 1730), (2041, 2046), (2063, 2068), (2093, 2108), (2137, 2398), (3167, 3168),
    (3687, 3705), (5106, 5109), (5126, 5133), (6680, 6683), (6726, 6727), (6745, 6745), (6762, 6763), (6796, 6796),
    (6798, 6804), (6815, 6815), (6818, 6825), (6897, 6912), (7553, 7756), (8131, 8132), (8717, 8718), (8720, 8721),
    (8806, 8807), (9132, 9133), (9276, 9276), (9278, 9279), (9372, 9373), (9526, 9526), (9767, 9779), (11109, 11110),
    (11121, 11125), (11230, 11257), (12330, 12332), (12429, 12431), (12676, 12677), (12682, 12683), (12688, 12693), (12710, 12712),
    (13045, 13047), (13132, 13133), (13138, 13139), (13144, 13145), (13150, 13151), (13156, 13157), (13162, 13163), (13168, 13169),
    (13174, 13175), (13180, 13181), (13186, 13187), (13192, 13193), (13198, 13199), (13204, 13205), (13210, 13211), (13216, 13217),
    (13222, 13223), (13228, 13229), (13234, 13235), (13240, 13241), (13246, 13247), (13252, 13253), (13258, 13259), (13264, 13265),
    (13270, 13271), (13276, 13281), (14504, 14513), (14594, 14594), (14615, 14648), (14650, 14859), (14887, 14887), (14935, 14944),
    (15073, 15073), (16218, 16219), (16224, 16225), (16230, 16231), (16236, 16237), (16242, 16243), (16248, 16249), (16254, 16255),
    (16260, 16261), (16266, 16267), (16272, 16273), (16278, 16279), (16284, 16285), (16290, 16291), (20536, 20569), (20598, 20598),
    (20743, 20755), (20757, 20757), (20760, 20772), (20774, 20774), (20830, 20831), (20836, 20837), (20842, 20843), (21520, 21540),
    (21550, 21613), (21615, 21623), (21628, 21629), (22038, 22043), (22048, 22049), (22454, 22454), (22539, 22540), (22891, 22893),
    (23200, 23201), (23250, 23250), (23255, 23256), (23661, 23661), (23666, 23667), (24072, 24073), (24078, 24079), (24484, 24486),
    (24968, 24968), (25097, 25098), (25107, 25124), (25449, 25450), (25455, 25456), (25461, 25462), (25467, 25476), (25801, 25802),
    (25807, 25808), (25813, 25814), (25819, 25820), (26845, 26892), (27553, 27553), (27660, 27660), (27719, 27719), (27721, 27724),
    (27809, 27810), (28135, 28135), (28220, 28221), (28546, 28546), (28631, 28632), (28957, 28957), (29042, 29043), (29368, 29377),
    (29380, 29388), (29390, 29390), (29407, 29498), (29501, 29501),
];

// SUFFOCATING and BLOCK_SHAPE by state.
struct Tables {
    // isSuffocating where it differs from blocksMotion and a full collision shape.
    suffocating: Vec<Option<bool>>,
    // The collision shape is Shapes.block() itself.
    block: Vec<bool>,
}

fn tables(l: &Level) -> &'static Tables {
    use std::sync::OnceLock;
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let count = l.reg.states.len();
        let mut tables = Tables { suffocating: vec![None; count], block: vec![false; count] };
        for &(first, last, value) in SUFFOCATING {
            for id in first..=last {
                tables.suffocating[id] = Some(value);
            }
        }
        for &(first, last) in BLOCK_SHAPE {
            for id in first..=last {
                tables.block[id] = true;
            }
        }
        tables
    })
}

// BlockStateBase.isSuffocating: a block that blocks motion and has a full
// collision shape, but for the states the client's blocks decide otherwise
// (leaves, glass...).
pub fn is_suffocating(l: &Level, state: State) -> bool {
    if let Some(value) = tables(l).suffocating[state as usize] {
        return value;
    }
    let flags = l.reg.flags(state);
    flags & crate::registry::BLOCKS_MOTION != 0 && flags & crate::registry::COLLISION_FULL != 0
}

// Whether the state's collision shape is Shapes.block() itself (BlockCollisions
// tests such a block with AABB.intersects).
pub fn is_block_shape(l: &Level, state: State) -> bool {
    tables(l).block[state as usize]
}
