'use strict';

// What today's server says of every block of the block table, one JSON line
// each, for mcbench-setup's differential test (tests/differential.rs):
// block-icon.js's icon (its size and the sha256 of its RGBA pixels),
// block-tints.js's tints of its default state, minecraft-facts.js's facts,
// and at the end the item tags, the shipped tints' page bytes' sha256 and
// the block registry's sha256.
//
//   MCBENCH_DATA_DIR=<a data folder with today's bake and misode/mcmeta's
//   files in assets/1.21.10> node dump.js <repository> > node.jsonl

const crypto = require('crypto');
const path = require('path');

const repo = path.resolve(process.argv[2]);
// The server's own terminal lines go to stderr: stdout is the dump's.
console.log = (...args) => console.error(...args);
const { blockIcon, _internal: { decodePng } } = require(path.join(repo, 'server/block-icon'));
const { blockStates } = require(path.join(repo, 'server/block-states'));
const { blockTints } = require(path.join(repo, 'server/block-tints'));
const { minecraftFacts } = require(path.join(repo, 'server/minecraft-facts'));
const { getAsset } = require(path.join(repo, 'server/assets'));

const sha256 = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');
const out = (record) => process.stdout.write(`${JSON.stringify(record)}\n`);

(async () => {
  const [states, tints] = await Promise.all([blockStates(), blockTints()]);
  const facts = minecraftFacts();
  for (const id of states.defaults.keys()) {
    const name = id.replace(/^minecraft:/, '');
    // A block whose icon today's server cannot make (its texture is not in
    // the atlas) says so.
    let icon;
    try {
      const png = await blockIcon(name);
      icon = png && decodePng(png);
    } catch (error) {
      icon = { error: String(error.message) };
    }
    const state = states.idOf(id, {});
    const block = facts.blocks.get(id);
    out({
      id,
      icon: icon && (icon.error ? icon : { width: icon.width, rgba: sha256(icon.pixels) }),
      tintParticle: tints.particle[state],
      tintWorld: [...tints.world.subarray(state * tints.indices, (state + 1) * tints.indices)],
      className: facts.className(id),
      supertypes: block.supertypes,
      defaults: facts.defaults(id),
      air: facts.isAir(id),
      stateId: facts.stateId(id),
      heldBy: facts.heldBy(id, {}),
    });
  }
  out({
    itemTags: Object.fromEntries([...facts.itemTags].map(([tag, members]) => [tag, [...members]])),
    survives: sha256(Buffer.from(facts.survives)),
    held: sha256(Buffer.from(facts.held)),
    tintBytes: sha256(tints.bytes()),
    registry: sha256(await getAsset('blocks.json')),
  });
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
