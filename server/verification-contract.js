'use strict';

const { MIN_MC_Y, MAX_MC_Y, Y_OFFSET } = require('./minecraft-dimension');

const BUDGET_ENV = {
  operations: 'MCBENCH_VERIFY_MAX_OPERATIONS',
  simulationTicks: 'MCBENCH_VERIFY_MAX_SIMULATION_TICKS',
  wallMs: 'MCBENCH_VERIFY_MAX_WALL_MS',
  columns: 'MCBENCH_VERIFY_MAX_COLUMNS',
  tests: 'MCBENCH_VERIFY_MAX_TESTS',
  requestBytes: 'MCBENCH_VERIFY_MAX_REQUEST_BYTES',
  reportBytes: 'MCBENCH_VERIFY_MAX_REPORT_BYTES',
};

// Operator budgets, not claims about Minecraft or the host's maximum capacity.
//
// The wall-clock budget is the limit that matters. The others are generous
// ceilings on what one test may ask for, set from what a test costs on the
// engines (scripts/benchmark-verification.js, 2026-10-23; per test; the Mac is
// an Apple M3 under load, the PC a Ryzen 9 9950X3D, about 1.3 times faster).
// A test runs on the Rust engine only; the Java figures are the reference
// engine's, which only the parity scripts run:
//
//   operations      0.6 microseconds each on Rust, 0.8 on Java: a million take
//                   under a second, inventory probes (54 operations each)
//                   included. Time does not bind; the report does: a million
//                   block observations are 39 MB when the states are short and
//                   about 130 MB when they are long, which is reportBytes. So
//                   both stay.
//   setup           0.5 microseconds per block on Rust, 0.9 on Java: the largest
//                   build (3,000,000 blocks) loads in 1.6 s and 2.7 s over 1,024
//                   columns.
//   columns         16,384 columns (a build 126 chunks, about 2,000 blocks,
//                   square, with the margin) of the 3,000,000 blocks load in 7
//                   to 62 s with a peak resident size of 0.5 GB on Rust and 1.8
//                   GB on Java (1,024 columns: 0.2 and 0.8 GB; 4,096: 0.3 and 1.0
//                   GB), under the 2.5 GiB a lane is given (LANE_BYTES in
//                   minecraft-verify.js).
//   simulationTicks 72,000 ticks, an hour of game time. Columns with nothing
//                   scheduled cost nothing per tick (stone, grass, leaves and
//                   crops over 1,024 columns: 3,000 ticks cost no more than 1);
//                   16 quiet columns run the 72,000 ticks in 0.06 s, a ring of
//                   observers in 2 to 8 s, the busy collectors build of the
//                   community corpus in 30 s on Rust and 66 s on Java. Memory
//                   stays flat over the run.
//
// Only dense, active circuits cost real time: the busiest community builds,
// repeated over every column, cost 65 to 170 microseconds per column and tick
// on Rust and 100 to 225 on Java, and a column of 2,048 always-firing observers
// 275 and 500. A test that asks for the most of columns, ticks and activity at
// once would take days, so no static budget could promise it fits; wallMs ends
// it instead, with a budget error that says it was too dense to finish and how
// many ticks it reached (WallBudgetError). A test that reaches what the Rust
// engine does not model fails with the engine's "unported" error.
//
// tests: a call's tests run on up to three lanes, so eight tests take at most
// three rounds.
function readBudgets(env = process.env) {
  const defaults = { operations: 1_000_000, simulationTicks: 72_000, wallMs: 900_000, columns: 16_384, tests: 8, requestBytes: 8 * 1024 * 1024, reportBytes: 128 * 1024 * 1024 };
  return Object.fromEntries(Object.entries(defaults).map(([key, fallback]) => {
    const name = BUDGET_ENV[key];
    const value = env[name] === undefined ? fallback : Number(env[name]);
    const maximum = key === 'reportBytes' ? Number.MAX_SAFE_INTEGER : 2_147_483_647;
    if (!Number.isSafeInteger(value) || value < 1 || value > maximum) throw new Error(`${name} must be an integer within 1..${maximum}`);
    return [key, value];
  }));
}

const BUDGETS = readBudgets();

// Fractional entity coordinates (imported entities and dropped-item inputs) in
// blueprint space. Shared by test_build and exported benchmark submissions.
function entityCoordinates(value, yOffset = Y_OFFSET) {
  if (!Array.isArray(value) || value.length !== 3) throw new Error('entity position requires 3..3 entries');
  return value.map((n, axis) => {
    const lo = axis === 1 ? MIN_MC_Y - yOffset : -4096, hi = axis === 1 ? MAX_MC_Y + 1 - yOffset : 4096;
    if (typeof n !== 'number' || !Number.isFinite(n) || n < lo || n >= hi) throw new Error('Entity coordinate outside supported world bounds');
    return n;
  });
}

// Entries {at, data_version, snbt}; entity families, UUIDs and Pos are checked
// by the native importer. Order is preserved: it is the native insertion order.
function validateEntities(value, yOffset = Y_OFFSET) {
  if (!Array.isArray(value)) throw new Error('entities requires 0..9007199254740991 entries');
  return value.map(entry => {
    if (!entry || typeof entry !== 'object' || Object.keys(entry).sort().join(',') !== 'at,data_version,snbt' || typeof entry.snbt !== 'string' || !entry.snbt.trim()) throw new Error('entities require at, data_version and nonempty snbt');
    const at = entityCoordinates(entry.at, yOffset);
    if (!Number.isInteger(entry.data_version) || entry.data_version < 0 || entry.data_version > 2147483647) throw new Error('data_version must be an integer within 0..2147483647');
    return { at, data_version: entry.data_version, snbt: entry.snbt };
  });
}
const DEFAULT_SAMPLES = [0, 20, 100];
const DEFAULT_SETTLE_TICKS = 40;
const MIB = 1024 * 1024;
const MAX_INLINE_BYTES = 20_000;
const positionSchema = { type: 'array', items: { type: 'integer' }, minItems: 3, maxItems: 3 };
// One experiment: a fresh copy of the blueprint, its inputs and its observations.
const TEST_SCHEMA = {
  type: 'object',
  properties: {
    probes: { type: 'array', items: positionSchema, minItems: 1, description: 'Positions to read in blueprint coordinates, in the same order in every observation. Probes and sample times share a total work budget.' },
    actions: {
      type: 'array',
      items: {
        type: 'object', properties: {
          tick: { type: 'integer', minimum: 0 },
          type: { type: 'string', enum: ['press', 'toggle', 'insert', 'extract', 'drain', 'use', 'drop', 'page', 'book'] },
          at: {type:'array',items:{type:'number'},minItems:3,maxItems:3,description:'Block coordinates for interactions; fractional coordinates allowed only for drop.'},
          velocity: {type:'array',items:{type:'number',minimum:-10,maximum:10},minItems:3,maxItems:3,description:'Required only for drop: native incoming item velocity in blocks per game tick.'},
          present: {type:'boolean',description:'Required only for book: false removes the lectern book through its native menu; true restores that same previously removed book, or places an explicit supplied book into an empty lectern. Explicit supply requires item, count:1 and stack; it cannot overwrite a present or previously removed book.'},
          page: {type:'integer',minimum:0,description:'Required only for page: zero-based page in the existing lectern book. Out-of-range requests fail without changing it.'},
          slot: { type: 'integer', minimum: 0, description: 'Required for insert/extract: zero-based slot in this block, not both halves of a double chest.' },
          count: { type: 'integer', minimum: 1, maximum: 64, description: 'Required for insert/extract/drop and explicit book supply (exactly1 for books). The entire amount must fit or be present; otherwise nothing changes.' },
          stack: {type:'object',description:'Optional for insert/drop or explicit book supply: complete native item SNBT and its data version, for potions, enchanted books or named items. Minecraft upgrades and decodes it; ID/count must match the action. Invalid data fails before insertion.',properties:{data_version:{type:'integer',minimum:0},snbt:{type:'string'}},required:['data_version','snbt'],additionalProperties:false},
          item: { type: 'string', description: 'Required for insert/drop or explicit book supply: Minecraft item ID, e.g. minecraft:iron_ingot.' },
          yaw: { type: 'number', description: 'Only for use: the direction the player faces in degrees (0 south, 90 west, 180 north, 270 east). Required to use a fence gate, which opens away from the player; no other block reads it.' },
        }, required: ['tick', 'type', 'at'], additionalProperties: false,
      },
      description: 'Inputs after settling: press a button or toggle a lever. Native button release is automatic. use is a survival player\'s right click with an empty hand, through Minecraft\'s own interaction, on a door or trapdoor (opens or closes it; iron ones cannot be opened by hand), fence gate (needs yaw), repeater (next delay), comparator (switches mode), note block (next note, played), daylight detector (inverts it), redstone dust (a dot becomes a cross and a cross a dot) or decorated pot (wobbles); a use that changes nothing fails. drain {tick,type,at} empties one container block (one chest half) through its native removeItem, slot by slot; its outcome lists every removed stack under removed, in the inventory observation format. drop supplies a native dropped-item stack at the specified position and velocity; the outcome records its UUID. It represents an external item input, not a circuit output. book removes or restores the same lectern book, or places an explicitly supplied written/writable book into an empty lectern; page selects an existing lectern book page through the native Minecraft menu interaction. Defaults to no inputs.',
    },
    block_data_version: {type:'integer',minimum:0,description:'Optional data version of all source blueprint block states. Minecraft upgrades their IDs/properties using its native block-state data fixer before placement and reports the migrations. Omit for current-version builds. Does not rewrite the blueprint.'},
    block_entities: { type: 'array', description: 'Optional initial block-entity data, loaded while frozen before settling. Not saved to the blueprint. Include the same data on every test. Minecraft upgrades older data using its native data fixer.', items: { type: 'object', properties: { at: positionSchema, data_version: { type: 'integer', minimum: 0 }, snbt: { type: 'string' } }, required: ['at', 'data_version', 'snbt'], additionalProperties: false } },
    scheduled_ticks: {type:'object',description:'Optional authoritative saved schematic tick queue, restored while frozen under strict placement. Replaces placement-generated block/fluid ticks inside bounds; empty entries means an empty saved queue. Preserve source delays, priorities and signed 64-bit sub-tick order. Does not persist in the blueprint.',properties:{bounds:{type:'array',items:positionSchema,minItems:2,maxItems:2},data_version:{type:'integer',minimum:0},entries:{type:'array',items:{type:'object',properties:{kind:{type:'string',enum:['block','fluid']},id:{type:'string'},at:positionSchema,delay:{type:'integer'},priority:{type:'integer',minimum:-3,maximum:3},sub_tick:{type:'string',description:'Original signed 64-bit order as decimal text.'}},required:['kind','id','at','delay','priority','sub_tick'],additionalProperties:false}}},required:['bounds','data_version','entries'],additionalProperties:false},
    entities: {type: 'array', description: 'Optional boat/raft, dropped-item, item-frame, painting, armor-stand or ordinary/container/furnace/TNT minecart entity NBT, imported natively while frozen. at uses fractional blueprint coordinates and must match NBT Pos. Preserve source UUID. An ordinary minecart may retain one original dolphin passenger, including its UUID, position and equipment. Other passenger trees and all leashes fail explicitly. Include again in every test; report records importedEntities.', items: {type: 'object', properties: {at: {type:'array',items:{type:'number'},minItems:3,maxItems:3},data_version:{type:'integer',minimum:0},snbt:{type:'string'}},required:['at','data_version','snbt'],additionalProperties:false}},
    inventory_missing: { type: 'string', enum: ['error', 'report'], description: 'Default error. Use report for containers that move or break: inventory is explicitly null when no container exists, alongside the actual block state. Null is not an empty inventory.' },
    world_inventories: {type:'boolean',description:'Read every loaded block-container inventory, container-entity inventory (including hopper/chest minecarts), and item-frame contents at each sample. Boats and minecarts also report vehicle_item using their native pick-item identity, separately from their cargo. Dolphin equipment is included so held input items remain observable. Reports kind, type, position, entity UUID where applicable, and full item stacks. Combine this census with item_entities for conservation; do not count the same block again from inventory_probes. Unopened loot tables fail explicitly instead of generating loot while observing.'},
    living_entities: {type:'boolean',description:'Read all loaded non-player living entities at each sample: native entity type, UUID, blueprint-relative position, health, max_health and dead flag. Read-only; use for mob routing and safety. An absent entity is not proof of delivery or survival. Keep the complete route loaded with build/probe bounds.'},
    item_entities: { type: 'boolean', description: 'Include all loaded dropped-item entities in the isolated world at each sample, with identity, position and full item SNBT. Read-only; useful for tracking items in transit.' },
    inventory_probes: { type: 'array', items: positionSchema, description: 'Subset of probes that are containers. Adds nonempty slots with item IDs, counts and complete item SNBT to every observation. A missing container is an experiment error unless inventory_missing is report.' },
    samples: { type: 'array', items: { type: 'integer', minimum: 0 }, minItems: 1, description: `Sample ticks after settling; default ${JSON.stringify(DEFAULT_SAMPLES)}. Samples follow inputs at the same tick. 20 ticks = 1 second.` },
    settle_ticks: { type: 'integer', minimum: 1, description: `Ticks to run before the initial observation and input timeline; default ${DEFAULT_SETTLE_TICKS}. Shares the simulation-time budget with the timeline.` },
  },
  required: ['probes'], additionalProperties: false,
};
const TEST_BUILD_SCHEMA = {
  type: 'function',
  function: {
    name: 'test_build',
    description: 'Run fresh copies of the current blueprint in Minecraft, one experiment per entry of tests. Each test imports its own copy, operates native buttons/levers and observes real block states at game ticks; tests do not affect one another, the blueprint or the user\'s live world in the browser. Each test has the full budgets on its own (they are not shared across the call); a test too dense to finish within its wall-clock budget is stopped with an error that says how many ticks it reached. Tests run side by side when the machine has room; results come back per test in the order given. A test that fails does not stop the others. ok means the experiment completed, not that the design passed. Full observations of each test are saved under /work/verification; small results are also returned inline. Every result includes a bounded preview of initial states, sampled changes, and input failures.',
    parameters: {
      type: 'object',
      properties: {
        tests: { type: 'array', items: TEST_SCHEMA, minItems: 1, maxItems: BUDGETS.tests, description: `One to ${BUDGETS.tests} experiments. Each entry holds everything one experiment needs; nothing is inherited from other entries, so repeat block_entities, entities and block_data_version in every test that needs them.` },
      },
      required: ['tests'], additionalProperties: false,
    },
  },
};

function testBuildTool(placementMode = 'updates') {
  if (!['updates', 'strict'].includes(placementMode)) throw new Error('Unsupported placement mode');
  const tool = structuredClone(TEST_BUILD_SCHEMA);
  if (placementMode !== 'strict') delete tool.function.parameters.properties.tests.items.properties.scheduled_ticks;
  return tool;
}

const TEST_BUILD_TOOL = testBuildTool();

const TEST_BUILD_GUIDANCE = `## test_build
Call test_build with tests, a list of one to ${BUDGETS.tests} experiments. Each test provides probes (block positions), actions (optional), samples (optional tick numbers), and settle_ticks (optional, default ${DEFAULT_SETTLE_TICKS}). Each test imports the full current blueprint into a separate Minecraft world of its own using the same placement code as the live world in the browser: ticks frozen during import, with the build and a one-chunk margin kept loaded. The world is recreated for every test, so tests never see one another's state: use several tests for different input sequences, truth-table rows or variants of one observation plan. Tests may run at the same time; the results come back per test, in the order given, and a test that fails does not stop the others. A test runs the settling ticks, records an initial observation, then starts tick 0. Block actions are {tick,type,at}; type is press for a button, toggle for a lever, or use for a survival player's empty-hand right click through Minecraft's own interaction: it opens or closes a door, trapdoor or fence gate, steps a repeater's delay, switches a comparator's mode, advances and plays a note block, inverts a daylight detector, turns redstone dust from a dot to a cross or back, or wobbles a decorated pot, with the block's native updates, sounds and game events. Iron doors and trapdoors cannot be opened by hand, and dust that connects to the same sides either way does not change: such a use fails. A fence gate opens away from the player, so using one needs yaw, the direction the player faces in degrees (0 south, 90 west, 180 north, 270 east); no other block reads it. Lectern actions {tick,type:"book",at,present} remove and restore the same source book through native interactions; an absent book gives zero signal. Restoration uses native placement and resets to the first page. To supply a book to an initially empty lectern, add item (written_book or writable_book), count:1 and stack:{data_version,snbt} to a present:true book action. Minecraft upgrades the supplied item and places it through native book interaction. Supply fails if a book is already present or a previously removed book is held; it never overwrites or replaces the original book. Page actions are {tick,type:"page",at,page}, selecting a zero-based page through the native menu. An invalid page fails without changing the book; normal comparator updates and the lectern pulse remain native. Inventory actions insert {tick,type,at,slot,count,item}, extract {tick,type,at,slot,count} or drain {tick,type,at}. Slots address one block (one chest half); insertion requires available capacity, and never overwrites or partially inserts. Extraction requires the full count. Drain empties the whole block inventory, however much it holds, and its outcome records each removed stack with its slot, ID, count and complete SNBT. For an external item stream, drop {tick,type,at,item,count,velocity} supplies a native item stack at fractional coordinates with the specified incoming velocity. Both insert and drop accept optional stack:{data_version,snbt} for complete item data; Minecraft upgrades and decodes it, and its item ID/count must match the action. Without stack the item is plain. Invalid data fails without supplying anything. Its input outcome records entityUUID so you can track it to the output. Supply at the inlet required by the task; spawning an answer at the outlet does not test a circuit. Failed actions are reported. Native hoppers and droppers move the actual items afterward. Samples are taken after any actions at that tick. There are 20 game ticks per second; button release is handled by Minecraft. Each test starts fresh, so keep related actions in one test when state must persist.

Optional entities entries {at,data_version,snbt} import original boats, rafts, dropped items, experience orbs, item frames, paintings, armor stands or ordinary/container/furnace/TNT minecarts while frozen; preserve UUID and make NBT Pos match fractional at coordinates. Item-frame and painting attachment coordinates must be translated with Pos; native data fixing upgrades old TileX/TileY/TileZ to block_pos. Native data fixing and loading preserve their state. Other entity families, passengers and leash relationships fail explicitly. importedEntities records the loaded NBT. These test entities do not persist in the blueprint and must be included again in every test that needs them.

Optional block_entities entries {at,data_version,snbt} initialize functional block-entity data while frozen, using Minecraft's native version conversion; these are test inputs and do not persist in the blueprint. Include them again in every test that needs them. The full report records importedBlockEntities after conversion. Optional inventory_probes (a subset of probes) records all nonempty inventory slots, including complete item SNBT; missing containers fail explicitly by default. Set inventory_missing:"report" to observe moving/replaced containers: null means no container exists, [] means an existing empty container, and the actual block state is always recorded. Set item_entities:true to include loaded dropped items with positions and complete item SNBT; include these when checking conservation through item transport. Set living_entities:true to observe loaded non-player creatures with type, UUID, position, health, max_health and dead flag. These observations do not change Minecraft execution.

Choose sample times to cover the requested behavior; the default ${JSON.stringify(DEFAULT_SAMPLES)} is only a short observation window. The budgets below apply to each test on its own, not to the call as a whole. The configured work budget is ${BUDGETS.operations} operations: (probes + 54 × inventory probes) × (unique sample times + 1 initial observation) + actions + initial block entities + initial entities + saved scheduled ticks. Settling and timeline together may use ${BUDGETS.simulationTicks} game ticks. A test has a ${BUDGETS.wallMs / 1000}-second wall-clock budget after leaving the queue and may load ${BUDGETS.columns} chunk columns, including a one-chunk margin around the build, probes, and inputs. A test's request may be ${BUDGETS.requestBytes / MIB} MiB and its full report ${BUDGETS.reportBytes / MIB} MiB. The wall-clock budget is the limit that binds: columns with nothing scheduled cost nothing per tick, so ordinary tests take seconds, but dense active circuitry over many columns for many ticks can take longer than the budget. Such a test is stopped with a wallMs budget error that says it was too dense to finish in time and how many ticks it reached (settling included); shorten it (fewer ticks, a smaller area, less active circuitry) or split it into several tests. A test that reaches Minecraft behaviour the engine does not model (mobs among it) fails with "unported: <what>"; that is a limit of the harness, not a result about the build. Budget errors name the resource, requested amount, allowed amount, and operator setting; they are harness limits, not evidence of a defective build. Do not silently reduce required test coverage to fit.

The result is {ok, tests:[...]}: ok is true when every test completed, and tests[i] is the result of test i, or {ok:false, error} (with code and budget for a budget error) when that test failed. Each test's result identifies the tested blueprint revision, engine version, inputs, observed block states, and measured resource use. Observation blocks follow probe order. block is the vanilla block-state string; power, powered, and lit there describe that component. signal is Bukkit getBlockPower: received neighboring power, not the component’s own output or powered state. ok means the experiment completed; compare the evidence with the user's requirements yourself. preview contains initial states with zero-based probe indexes and coordinates, sampled changes {tick,probe,block,signal}, total change counts, and input failures. Its truncated flag identifies omitted preview entries; counts only cover requested samples, so no sampled changes does not prove nothing happened between them. Each test's full result is saved at its returned report path under /work/verification for bash analysis; small results also appear inline. The full JSON has probes, actions, sampleTicks, initial:{tick,blocks}, observations:[{tick,blocks}], and inputs with applied/error outcomes. Build and screenshot results identify their blueprint revision; changes after a test require another test before claiming the changed build works. Tests cover only the selected blocks and times.`;

module.exports = { TEST_BUILD_TOOL, testBuildTool, TEST_BUILD_GUIDANCE, BUDGETS, BUDGET_ENV, readBudgets, entityCoordinates, validateEntities, DEFAULT_SAMPLES, DEFAULT_SETTLE_TICKS, MAX_INLINE_BYTES };
