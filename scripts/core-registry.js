'use strict';

// Writes what the Rust engine (core/) reads of Minecraft, for the pinned
// version, into core/minecraft/<version>/:
//   blocks.txt  the Java engine's dump (McbenchEngine.dump): blocks, states,
//               properties, tags, collision shapes, items
//   loot.json   the server jar's block loot tables, by block key
// (server/minecraft-tables.js), on the JDK.
//
//   MCBENCH_DATA_DIR=<scratch copy> node scripts/core-registry.js

const path = require('path');
const { minecraftLive } = require('../server/minecraft-live');
const { writeRegistry, tablesFolder } = require('../server/minecraft-tables');
const { checkJdk, JAVA, JAVAC } = require('../server/bluemap');

// The same code the app's first start makes them with
// (MinecraftLive.minecraftTables).
async function main() {
  await checkJdk();
  const classpath = (await minecraftLive.ensurePaper()).map((file) => path.join(minecraftLive.paperDir, file));
  const target = tablesFolder(minecraftLive.minecraftVersion);
  const count = await writeRegistry({ classpath, minecraftVersion: minecraftLive.minecraftVersion, target, java: JAVA, javac: JAVAC });
  console.log(`${target}: blocks.txt, loot.json (${count} block loot tables)`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
