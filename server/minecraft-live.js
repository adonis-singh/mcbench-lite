'use strict';

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const { execFile } = require('child_process');
const { promisify } = require('util');

const { blueMap: viewerAssets } = require('./bluemap');
const { installJdk, JAVA, JAVAC } = require('./tools');
const setup = require('./setup');
const { Y_OFFSET } = require('./minecraft-dimension');
const { importCommands } = require('./live-placement');
const { coreEngine } = require('./engine');
const { PaperFiles, PAPER } = require('./paper-files');
const { step } = require('./progress');
const { REGISTRY, missingTables, tablesFolder, writeRegistry, writeDataFix } = require('./minecraft-tables');

const execFileAsync = promisify(execFile);

// What the app's server keeps for the live world. The world itself runs in
// the page (web/live-world.mjs, the Rust engine as WebAssembly in a worker),
// one per viewer; this serves it what the page cannot make itself: the
// viewer's assets, Paper's classpath and block table, the engine's folder,
// and the tables of Minecraft the engine is built with (minecraft-tables.js).
// The viewer's assets: `viewer.name` is one of
//   idle       they were not asked for
//   preparing  they are being made (`done`, its promise)
//   ready      they are made (`done`, resolved; the page reads blueMap.assets)
//   failed     the last try failed (`error`, what the page is told, and
//              `at`, when): prepareViewer() tries again, and so does status()
//              once RETRY_MS have passed
// The page asks status() every second: a preparation that keeps failing is
// not run back to back.
const RETRY_MS = 30_000;

// Java programs over Paper's files (its launcher, the block table) run on
// the app's JDK (tools.js), in Paper's folder.
async function runJdk(args, { cwd, timeout }) {
  await installJdk();
  await execFileAsync(JAVA, args, { cwd, timeout, maxBuffer: 16 * 1024 * 1024 });
}

class MinecraftLive extends PaperFiles {
  constructor({
    yOffset = Y_OFFSET,
    blueMap = viewerAssets,
    blockTable = null,
    engine = coreEngine(),
    runJava = runJdk,
    ...files
  } = {}) {
    super({ runJava, javaReady: installJdk, ...files });
    if (blockTable) this.blockTable = blockTable;
    this.engine = engine;
    this.yOffset = yOffset;
    this.blueMap = blueMap;
    this.viewer = { name: 'idle' };
  }

  // Moves the viewer's assets to the state `next` from one of the states `from`.
  moveViewer(from, next) {
    if (!from.includes(this.viewer.name)) throw new Error(`The viewer's assets are ${this.viewer.name} and cannot become ${next.name}`);
    this.viewer = next;
  }

  // What the page needs to draw a world: its assets exist as soon as the
  // viewer is prepared, before any world does. Being asked RETRY_MS or more
  // after a failure is what tries again, so no failure lasts until the
  // server restarts.
  status() {
    const viewerError = this.viewer.name === 'failed' ? this.viewer.error : null;
    if (viewerError && Date.now() - this.viewer.at >= RETRY_MS) this.prepareViewer().catch(() => {});
    const assets = this.blueMap.assets;
    return {
      minecraftVersion: this.minecraftVersion,
      yOffset: this.yOffset,
      viewerUrl: assets?.viewerUrl ?? null,
      mesher: assets ? { wasm: assets.wasm, pack: assets.pack } : null,
      viewerError,
    };
  }

  // A build placed afresh by the server's engine, one at a time: resolves
  // with its blueprint (scripts that draw or compare blueprints).
  drawBlueprint(world) {
    return this.placeBlueprint(null, world);
  }

  placeBlueprint(_conversationId, world) {
    const placed = (this.blueprintQueue || Promise.resolve()).catch(() => {}).then(async () => {
      await this.warmEngine();
      fs.mkdirSync(this.engineFolder, { recursive: true });
      const commands = path.join(this.engineFolder, `commands-${crypto.randomUUID()}.txt`);
      fs.writeFileSync(commands, importCommands(world, null, this.yOffset).commands.map((command) => `${command}\n`).join(''));
      const file = path.join(this.engineFolder, 'blueprint.bin');
      try {
        await this.engine.place(commands, file, { fresh: true });
      } finally {
        fs.rmSync(commands, { force: true });
      }
      return fs.readFileSync(file);
    });
    this.blueprintQueue = placed;
    return placed;
  }

  async warmEngine() {
    return this.engine.warm({ folder: path.join(this.engineFolder, 'preview') });
  }

  // The viewer's assets (bluemap.js), made from Paper's block table: once
  // while they are made or after that worked, again after it failed. A
  // failure is shown in the viewer's place.
  prepareViewer() {
    if (['preparing', 'ready'].includes(this.viewer.name)) return this.viewer.done;
    const done = this.blockTable().then((table) => setup.track('viewer', () => this.blueMap.prepare(table)));
    this.moveViewer(['idle', 'failed'], { name: 'preparing', done });
    done.then(
      () => this.moveViewer(['preparing'], { name: 'ready', done }),
      (error) => this.moveViewer(['preparing'], { name: 'failed', error: `The viewer could not be prepared: ${String(error.message || error)}`, at: Date.now() }),
    );
    return done;
  }

  // The block table's file for the pinned Paper build and the table's
  // format (the hash of McbenchBlockTable.java), whether made yet or not.
  blockTableFile() {
    const format = crypto.createHash('sha256').update(fs.readFileSync(path.join(__dirname, 'McbenchBlockTable.java'))).digest('hex').slice(0, 8);
    return path.join(this.paperDir, `block-table-${PAPER[this.minecraftVersion].jar.replace(/\.jar$/, '')}-${format}.tsv`);
  }

  // What the pinned Minecraft says of each block state (McbenchBlockTable.java):
  // written once per Paper build and table format, read from the server's
  // own classes.
  async blockTable() {
    this.table ||= (async () => {
      const classpath = await this.ensurePaper();
      await installJdk();
      const source = path.join(__dirname, 'McbenchBlockTable.java');
      const file = this.blockTableFile();
      const name = path.basename(file);
      if (fs.existsSync(file)) return file;
      for (const old of fs.readdirSync(this.paperDir).filter((entry) => entry.startsWith('block-table-'))) {
        fs.rmSync(path.join(this.paperDir, old), { recursive: true, force: true });
      }
      const classes = path.join(this.paperDir, 'block-table-classes');
      fs.mkdirSync(classes, { recursive: true });
      await step("[viewer] reading every block state from Paper's classes", '[viewer] block table written', async () => {
        await execFileAsync(JAVAC, ['--release', '25', '-proc:none',
          '-classpath', classpath.map((file) => path.join(this.paperDir, file)).join(path.delimiter), '-d', classes, source], { timeout: 60_000 });
        await this.runJava(['-cp', ['block-table-classes', ...classpath].join(path.delimiter), 'McbenchBlockTable', name], { cwd: this.paperDir, timeout: 300_000 });
      });
      return file;
    })().catch((error) => {
      this.table = null;
      throw error;
    });
    return this.table;
  }

  // The chunk render layer the client draws each state and its fluid in
  // (McbenchRenderLayers.java), by the block table's states: written once
  // per block table and program, read from the client jar BlueMap's bake
  // downloads, its libraries Paper's (the server's, which the client shares).
  async renderLayers() {
    this.layers ||= (async () => {
      const table = await this.blockTable();
      await this.prepareViewer();
      const classpath = await this.ensurePaper();
      const source = path.join(__dirname, 'McbenchRenderLayers.java');
      const format = crypto.createHash('sha256').update(fs.readFileSync(source)).digest('hex').slice(0, 8);
      const file = path.join(this.paperDir, `render-layers-${path.basename(table, '.tsv').replace(/^block-table-/, '')}-${format}.tsv`);
      if (fs.existsSync(file)) return file;
      for (const old of fs.readdirSync(this.paperDir).filter((entry) => entry.startsWith('render-layers-'))) {
        fs.rmSync(path.join(this.paperDir, old), { recursive: true, force: true });
      }
      const classes = path.join(this.paperDir, 'render-layers-classes');
      // Minecraft's server entry point bootstraps the client's classes and
      // writes its settings in its working folder, which is let go after.
      const run = path.join(this.paperDir, 'render-layers-run');
      fs.rmSync(run, { recursive: true, force: true });
      fs.mkdirSync(run, { recursive: true });
      await step("[viewer] reading every block state's render layer from Minecraft's client", '[viewer] render layers written', async () => {
        await execFileAsync(JAVAC, ['-proc:none', '-d', classes, source], { timeout: 60_000 });
        const libraries = classpath.filter((entry) => entry.startsWith('libraries/')).map((entry) => path.join(this.paperDir, entry));
        await this.runJava(['-cp', classes, 'McbenchRenderLayers', this.blueMap.clientJar(), table, file, libraries.join(path.delimiter)], { cwd: run, timeout: 120_000 });
      });
      fs.rmSync(run, { recursive: true, force: true });
      return file;
    })().catch((error) => {
      this.layers = null;
      throw error;
    });
    return this.layers;
  }

  // Paper's files, saying so in the terminal while they are downloaded.
  // Paper's launcher runs on the app's JDK (runJdk), installed first.
  ensurePaper() {
    if (this.paper) return this.paper;
    return setup.track('minecraft', () => step(`[minecraft] downloading Paper ${this.minecraftVersion} (about 55 MB), which downloads the Minecraft server from Mojang (about 59 MB); the libraries come inside the two`,
      '[minecraft] Paper downloaded', () => super.ensurePaper()));
  }

  // Makes the tables of Minecraft the engine is built with
  // (core/minecraft/<version>/) from the server jar, those that are missing
  // (missingTables: the block and loot tables also when the Java engine's
  // sources changed since they were dumped): the engine is built after this.
  minecraftTables() {
    const missing = missingTables(this.minecraftVersion);
    if (!missing.length) return setup.track('tables', () => {});
    this.tables ||= (async () => {
      const classpath = (await this.ensurePaper()).map((file) => path.join(this.paperDir, file));
      await installJdk();
      const target = tablesFolder(this.minecraftVersion);
      await setup.track('tables', () => step("[minecraft] reading Minecraft's block, loot and data fixer tables from the server jar", "[minecraft] Minecraft's tables written", async () => {
        // Two JVMs at once: the tables are separate files from the same jars.
        await Promise.all([
          missing.some((name) => REGISTRY.includes(name)) && writeRegistry({ classpath, minecraftVersion: this.minecraftVersion, java: JAVA, javac: JAVAC }),
          missing.includes('datafix.txt') && writeDataFix({ classpath, target, java: JAVA, javac: JAVAC }),
        ]);
      }));
    })().finally(() => {
      this.tables = null;
    });
    return this.tables;
  }

  // The viewer's assets get ready before the first build opens. Resolves
  // once they are ready; a failure is printed here.
  warm() {
    const done = this.prepareViewer().then(() => this.renderLayers());
    done.catch((error) => console.error('[viewer]', error.message));
    return done;
  }

  async stop() {
    await this.engine.stop();
  }
}

const minecraftLive = new MinecraftLive();

module.exports = {
  MinecraftLive,
  minecraftLive,
};
