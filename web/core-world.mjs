// The Rust engine (core/) as a WebAssembly module, one world per
// instance: the engine's requests (a placement, ticks, a player's input,
// the state dump) with the bytes the native program writes to files, byte
// for byte, returned instead. It runs in any JavaScript host with
// WebAssembly (core-worker.mjs puts it in a Web Worker; the parity script
// runs it in Node). The module has no imports and no clock: the same
// requests give the same bytes in every host.
//
// A failure Minecraft reported rejects with "The engine failed: <reason>",
// as server/engine.js's Engine does. A request the port does not model
// ("unported: ...") traps the module, which is spent after: the world
// rejects every request but a fresh placement, which starts a new instance.

const ENCODER = new TextEncoder();
const DECODER = new TextDecoder();

// What the native program answers a request after a refusal (core/src/main.rs).
const AFTER_REFUSAL = 'unported: an earlier request reached what this port does not model';

export class CoreWorld {
  // `module` is the compiled WebAssembly.Module of mcbench_core.wasm
  // (compile it once and share it between worlds).
  constructor(module) {
    this.module = module;
    this.exports = null;
    this.refused = null;
    this.start();
  }

  static async compile(bytes) {
    return WebAssembly.compile(bytes);
  }

  start() {
    this.exports = new WebAssembly.Instance(this.module, {}).exports;
    this.exports.mcb_init();
    this.refused = null;
  }

  // A copy of a result buffer (the module reuses them).
  result(name) {
    const { exports } = this;
    const [pointer, length] = [exports[`mcb_${name}_ptr`](), exports[`mcb_${name}_len`]()];
    return new Uint8Array(exports.memory.buffer, pointer, length).slice();
  }

  // Writes text to the module's input buffer and returns its length.
  write(text) {
    const bytes = ENCODER.encode(text);
    // Growing the buffer can grow (and so detach) the module's memory: ask for it first.
    const pointer = this.exports.mcb_input_buffer(bytes.length);
    new Uint8Array(this.exports.memory.buffer, pointer, bytes.length).set(bytes);
    return bytes.length;
  }

  // Calls an export that can trap. A trap, or any other error out of the
  // module (an exhausted call stack), spends the instance: the panic hook has
  // kept the reason of a refusal or a bug of the port; with none, the trap
  // is something else (memory, the stack), worded as the host words it.
  call(name, ...args) {
    let code;
    try {
      code = this.exports[name](...args);
    } catch (error) {
      const failure = DECODER.decode(this.result('failure')) || `mcbench-core failed: ${error.message}`;
      this.refused = failure;
      throw new Error(`The engine failed: ${failure}`);
    }
    if (code < 0) throw new Error(`The engine failed: ${DECODER.decode(this.result('message'))}`);
    return code;
  }

  // A request on a world that can still take it.
  guard(fresh = false) {
    if (fresh && this.refused !== null) this.start();
    if (this.refused !== null) throw new Error(`The engine failed: ${AFTER_REFUSAL}`);
  }

  // Places the commands (the live import's setblock and fill lines, one per
  // line) on an empty world (`fresh`) or on the world as it is, and returns
  // the blueprint.
  place(commands, { fresh = true } = {}) {
    this.guard(fresh);
    this.call('mcb_place', this.write(commands), fresh ? 1 : 0);
    return this.result('data');
  }

  // Places the commands on the world as it is, as place(commands, { fresh:
  // false }) does, and returns { bytes, entities } as tick does: the patch of
  // the blocks changed since the last request, on the base it holds.
  edit(commands) {
    this.guard();
    this.call('mcb_edit', this.write(commands));
    return { bytes: this.result('data'), entities: DECODER.decode(this.result('entities')) };
  }

  // Ticks the world `ticks` times. Returns { bytes, entities }: the patch of
  // the blocks that changed since the last request (web/live-patch.mjs:
  // palette additions, the bounds and built columns after it, then x y z
  // and a palette index for each block); entities is the list as JSON text.
  tick(ticks = 1) {
    this.guard();
    this.call('mcb_tick', ticks);
    return { bytes: this.result('data'), entities: DECODER.decode(this.result('entities')) };
  }

  // A player's input at x, y, z by a player looking with `yaw` (degrees, a
  // 32-bit float): 'toggle' a lever, 'press' a button, or 'use' a door,
  // trapdoor, fence gate, repeater, comparator, note block, daylight detector,
  // redstone dust or decorated pot.
  input(type, x, y, z, yaw) {
    this.guard();
    this.call('mcb_input', this.write(type), x, y, z, yaw);
  }

  // What the world holds besides its blocks, as text (McbenchEngine.state).
  state() {
    this.guard();
    this.call('mcb_state');
    return DECODER.decode(this.result('data'));
  }

  // The world as a Minecraft save holds it: every chunk that has a block as
  // NbtIo writes it (see export in core/src/chunk_save.rs). Returns
  // { gameTime (a BigInt), chunks: [{ x, z, nbt }] }.
  export() {
    this.guard();
    this.call('mcb_export');
    const bytes = this.result('data');
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const gameTime = view.getBigInt64(0);
    const chunks = [];
    let at = 12;
    for (let i = view.getInt32(8); i > 0; i -= 1) {
      const length = view.getInt32(at + 8);
      chunks.push({ x: view.getInt32(at), z: view.getInt32(at + 4), nbt: bytes.subarray(at + 12, at + 12 + length) });
      at += 12 + length;
    }
    return { gameTime, chunks };
  }

  // The creative player (core/src/player: the client's own movement): one
  // stands at x, y, z looking as yaw and pitch (degrees) say, flying or not.
  // Returns its view, 28 numbers (core/src/player/mod.rs View.encode: x, y, z,
  // the old x, y, z, the motion, yaw, pitch, the old yaw and pitch, width,
  // height, eye height, pose, then flags and the fall distance).
  spawnPlayer(x, y, z, yaw, pitch, flying) {
    this.guard();
    this.call('mcb_player_spawn', x, y, z, yaw, pitch, flying ? 1 : 0);
    return this.view();
  }

  removePlayer() {
    this.guard();
    this.call('mcb_player_remove');
  }

  // One tick of the world with its player: `keys` the keys held as flags (1
  // forward, 2 backward, 4 left, 8 right, 16 jump, 32 shift, 64 sprint) and
  // the look. Returns what tick does, and the player's view after it.
  playerTick(keys, yaw, pitch) {
    this.guard();
    this.call('mcb_player_tick', keys, yaw, pitch);
    return { bytes: this.result('data'), entities: DECODER.decode(this.result('entities')), view: this.view() };
  }

  view() {
    return new Float64Array(this.result('view').buffer);
  }
}
