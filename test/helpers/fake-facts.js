'use strict';

// A stand-in for what the engine's table says of Minecraft
// (server/minecraft-facts.js), so tests of the code that imports, builds and
// verifies run without the tables a first start makes: the block ids of
// `FakeFacts.supported` are of the import's last pass, those of `beds` are of
// BedBlock (a foot facing north by default), the item tags are `itemTags`,
// and a full block below holds a block that cannot survive alone. Install it before the code under test asks.
class FakeFacts {
  static supported = new Set(['minecraft:torch', 'minecraft:wall_torch', 'minecraft:redstone_wire', 'minecraft:sand', 'minecraft:water', 'minecraft:lever', 'minecraft:rail']);

  static beds = new Set(['minecraft:red_bed']);

  static itemTags = new Map([['minecraft:lectern_books', new Set(['minecraft:written_book', 'minecraft:writable_book'])]]);

  lastPass(palette) {
    return Uint8Array.from(palette.entries, (entry) => (entry && FakeFacts.supported.has(entry.id) ? 1 : 0));
  }

  is(id, className) {
    return className === 'BedBlock' && FakeFacts.beds.has(id);
  }

  defaults(id) {
    return FakeFacts.beds.has(id) ? { facing: 'north', occupied: 'false', part: 'foot' } : {};
  }

  itemTag(name) {
    return FakeFacts.itemTags.get(name);
  }

  heldBy() {
    return [[0, -1, 0]];
  }
}

function installFakeFacts() {
  const fake = new FakeFacts();
  require('../../server/minecraft-facts').minecraftFacts = () => fake;
  return FakeFacts;
}

module.exports = { FakeFacts, installFakeFacts };
