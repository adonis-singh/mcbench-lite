// One world of the engine: McbenchEngine's requests (a placement, ticks, a
// player's input, the state dump) on a level, with their results as bytes
// instead of files. The native program and the WebAssembly module are both
// a loop over this.
//
// An error Minecraft reported is the request's it happened in, as
// McbenchEngine clears McbenchLevel.serverException before each request: a
// request that checks for one first drops what an earlier request left
// (one raised by the light after a tick's last check, or by a STATE).

use crate::blocks::Blocks;
use crate::dir::Pos;
use crate::engine::{self, Stream};
use crate::level::Level;
use crate::registry::Registry;

pub use crate::engine::Ticked;

pub struct Session {
    reg: &'static Registry,
    blocks: &'static Blocks,
    level: Level,
    stream: Option<Stream>,
}

impl Session {
    pub fn new(reg: &'static Registry, blocks: &'static Blocks) -> Session {
        Session { reg, blocks, level: Level::new(reg, blocks, 0, true), stream: None }
    }

    // Places the commands (the live import's setblock and fill lines) on an
    // empty world, or on the world the last placement left, and returns the
    // blueprint. The blueprint is the stream's base. A fresh placement starts
    // a new world without a base (renew), so one that fails leaves a world
    // whose ticks fail until a placement writes a base; a continuing one that
    // fails keeps the base before it.
    pub fn place(&mut self, commands: &str, fresh: bool) -> Result<Vec<u8>, String> {
        if fresh {
            self.renew();
        }
        self.level.reported = None;
        let lines: Vec<String> = commands.lines().map(str::to_string).collect();
        engine::place_all(&mut self.level, &lines, false)?;
        let (stream, blueprint) = engine::write(&mut self.level)?;
        self.stream = Some(stream);
        Ok(blueprint)
    }

    // An empty world without a base: what a fresh placement places on, made
    // before anything else of the request is read, as McbenchEngine replaces
    // its level first.
    pub fn renew(&mut self) {
        self.level = Level::new(self.reg, self.blocks, 0, true);
        self.stream = None;
    }

    // Places the commands on the world as it is, as place does, and returns
    // the changes since the last request as a patch on the stream's base (a
    // tick of no ticks, unlit, as a continuing place leaves its light): the
    // world is the one place leaves, delivered without a new base.
    pub fn edit(&mut self, commands: &str) -> Result<Ticked, String> {
        self.level.reported = None;
        let lines: Vec<String> = commands.lines().map(str::to_string).collect();
        engine::place_all(&mut self.level, &lines, false)?;
        engine::tick(&mut self.level, &mut self.stream, 0, false)
    }

    // Ticks the world `ticks` times: the changes since the last request as a
    // patch, the bounds and built columns grown or shrunk with them, and the
    // entities. `light`: whether to light what the ticks changed (the stock
    // request; a settle, which reads only blocks after its ticks, asks for none).
    pub fn tick(&mut self, ticks: i32, light: bool) -> Result<Ticked, String> {
        self.level.reported = None;
        engine::tick(&mut self.level, &mut self.stream, ticks, light)
    }

    // A player's input: "toggle" a lever, "press" a button, or "use" a door,
    // trapdoor, fence gate, repeater, comparator, note block, daylight
    // detector, redstone dust or decorated pot as a player looking with `yaw`
    // (degrees).
    pub fn input(&mut self, kind: &str, x: i32, y: i32, z: i32, yaw: f32) -> Result<(), String> {
        self.level.reported = None;
        engine::input(&mut self.level, kind, Pos::new(x, y, z), yaw)
    }

    // The world as a Minecraft save holds it, chunk by chunk (Level::export_chunks).
    pub fn export(&mut self) -> Vec<u8> {
        self.level.export_chunks()
    }

    // A creative player standing at `pos` (crate::player::Player::new), the
    // one the world's ticks then move. It is a client: it reads the world and
    // changes only itself.
    pub fn spawn_player(&mut self, pos: [f64; 3], yaw: f32, pitch: f32, flying: bool) {
        let player = crate::player::Player::new(crate::phys::Vec3::new(pos[0], pos[1], pos[2]), yaw, pitch, flying, crate::player::Settings::default());
        self.level.player = Some(Box::new(player));
    }

    pub fn remove_player(&mut self) {
        self.level.player = None;
    }

    // The player as a frame draws it (crate::player::View::encode: 28 numbers).
    pub fn player_view(&self) -> [f64; 28] {
        self.level.player.as_ref().expect("a world without a player").view().encode()
    }

    // One tick of the world with its player: the client ticks its player
    // (the keys held as the protocol's flags: forward, backward, left, right,
    // jump, shift, sprint, and where it looks), then the world (as a tick
    // request does). The view is the player after it.
    pub fn player_tick(&mut self, keys: u32, yaw: f32, pitch: f32) -> Result<(Ticked, [f64; 28]), String> {
        if self.level.player.is_none() {
            return Err("java.lang.IllegalStateException: a world without a player (PTICK before PSPAWN)".into());
        }
        self.level.reported = None;
        let input = crate::player::Input { keys: crate::player::Keys::from_bits(keys), yaw, pitch };
        self.level.player_tick(input);
        let ticked = engine::tick(&mut self.level, &mut self.stream, 1, true)?;
        Ok((ticked, self.player_view()))
    }

    // What the world holds besides its blocks (McbenchEngine.state).
    pub fn state(&mut self) -> String {
        self.level.state_text()
    }
}
