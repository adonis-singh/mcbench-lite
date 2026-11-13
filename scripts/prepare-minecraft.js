'use strict';

// Makes the tables of Minecraft the Rust engine is built with
// (core/minecraft/<version>/: blocks.txt, loot.json, datafix.txt) from the
// server jar, as the app's first start does, unless they are there: Paper is
// downloaded into the data folder if it is not there yet, and the app's JDK
// (server/tools.js, downloaded into the tools folder if it is not there yet)
// reads them (server/minecraft-tables.js). The repository holds none
// of them. Delete the folder to make them again.
//
//   node scripts/prepare-minecraft.js

const { minecraftLive } = require('../server/minecraft-live');
const { tablesFolder } = require('../server/minecraft-tables');

minecraftLive.minecraftTables().then(() => {
  console.log(`${tablesFolder(minecraftLive.minecraftVersion)}: ready`);
  process.exit(0);
}, (error) => {
  console.error(error.message);
  process.exit(1);
});
