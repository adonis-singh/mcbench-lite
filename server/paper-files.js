'use strict';

// Paper's files for one Minecraft version, laid out in a folder: the server
// jar the engine's classes are read from (the Java engine, the block table
// and the verifier's Paper runtime all run on its classpath) and its
// libraries.

const fs = require('fs');
const path = require('path');
const { execFile } = require('child_process');
const { promisify } = require('util');

const { MC_VERSION } = require('./assets');
const tools = require('./tools');
const setup = require('./setup');
const { graalvmHome } = require('./engine');
const { readZipEntry } = require('./zip-entry');

const execFileAsync = promisify(execFile);
const DATA_DIR = process.env.MCBENCH_DATA_DIR
  ? path.resolve(process.env.MCBENCH_DATA_DIR)
  : path.join(__dirname, '..', 'data');
const PAPER_DIR = process.env.MCBENCH_MC_DATA_DIR
  ? path.resolve(process.env.MCBENCH_MC_DATA_DIR)
  : path.join(DATA_DIR, 'minecraft-live', 'paper');
// Java refuses a cache whose classpath jars changed size or time, so every
// copy of Paper's files carries this time; their bytes are fixed by the build.
const CLASSPATH_TIME = new Date('2000-01-01T00:00:00Z');
// Paper is pinned per Minecraft version (the newest stable build of each).
const PAPER = Object.fromEntries([
  ['1.21.4', 232, '5ee4f542f628a14c644410b08c94ea42e772ef4d29fe92973636b6813d4eaffc'],
  ['1.21.8', 60, '8de7c52c3b02403503d16fac58003f1efef7dd7a0256786843927fa92ee57f1e'],
  ['1.21.10', 130, '158703f75a26f842ea656b3dc6d75bf3d1ec176b97a2c36384d0b80b3871af53'],
  ['1.21.11', 132, '5ffef465eeeb5f2a3c23a24419d97c51afd7dbb4923ff42df9a3f58bba1ccfba'],
].map(([version, build, sha256]) => [version, {
  jar: `paper-${version}-${build}.jar`,
  sha256,
  url: `https://fill-data.papermc.io/v1/objects/${sha256}/paper-${version}-${build}.jar`,
}]));

// Java programs over Paper's files (its launcher, the block table) run on
// the GraalVM JDK the engine is built with, in Paper's folder.
async function java(args, { cwd, timeout }) {
  await execFileAsync(path.join(graalvmHome(), 'bin', 'java'), args, { cwd, timeout, maxBuffer: 16 * 1024 * 1024 });
}

class PaperFiles {
  constructor({
    minecraftVersion = MC_VERSION,
    paperDir = PAPER_DIR,
    download = tools.download,
    runJava = java,
  } = {}) {
    if (!PAPER[minecraftVersion]) throw new Error('Unsupported Minecraft runtime version');
    this.minecraftVersion = minecraftVersion;
    this.paperDir = path.resolve(paperDir);
    this.download = download;
    this.runJava = runJava;
  }

  // The server's classpath, relative to the data folder: Paper's patched
  // server jar, then its libraries, in the order Paper's launcher loads them.
  // The jar is kept once its download matched the pinned checksum, and the
  // launcher's patch-only mode lays out its files once.
  ensurePaper() {
    this.paper ||= this.layOutPaper().catch((error) => {
      this.paper = null;
      throw error;
    });
    return this.paper;
  }

  async layOutPaper() {
    const { jar, sha256, url } = PAPER[this.minecraftVersion];
    const target = path.join(this.paperDir, jar);
    fs.mkdirSync(this.paperDir, { recursive: true });
    if (!fs.existsSync(target)) await this.download(url, target, { sha256, onProgress: setup.progress('minecraft') });
    const list = (name, folder) => readZipEntry(target, `META-INF/${name}`).toString('utf8').trim().split('\n')
      .map((line) => `${folder}/${line.split('\t')[2]}`);
    const classpath = [...list('versions.list', 'versions'), ...list('libraries.list', 'libraries')];
    if (!classpath.every((file) => fs.existsSync(path.join(this.paperDir, file)))) {
      setup.report('minecraft', { detail: "Paper's launcher is downloading the Minecraft server and its libraries from Mojang", progress: undefined });
      await this.runJava(['-Dpaperclip.patchonly=true', '-jar', jar], { cwd: this.paperDir, timeout: 300_000 });
    }
    for (const file of classpath) {
      const full = path.join(this.paperDir, file);
      if (fs.statSync(full).mtimeMs !== CLASSPATH_TIME.getTime()) fs.utimesSync(full, CLASSPATH_TIME, CLASSPATH_TIME);
    }
    // The vanilla jar the launcher patched from is not on the classpath.
    fs.rmSync(path.join(this.paperDir, 'cache'), { recursive: true, force: true });
    return classpath;
  }

  // Where the engines' programs and worlds are kept.
  get engineFolder() {
    return path.join(this.paperDir, 'engine');
  }
}

module.exports = { PaperFiles, PAPER, CLASSPATH_TIME, DATA_DIR, PAPER_DIR, java };
