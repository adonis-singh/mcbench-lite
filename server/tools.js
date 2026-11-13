'use strict';

// The programs the app downloads for itself instead of asking for them to be
// installed by hand: a JDK, rustup, Chromium's headless shell, and, when the
// person asks for one from Connect an AI, the Claude Code or Codex CLI. They
// are kept in one folder, MCBENCH_TOOLS_DIR or ~/.mcbench-lite/tools, which
// every checkout and data folder shares, so a scratch data folder downloads
// nothing again. Each program is pinned to a version and, for each of
// darwin-arm64, darwin-x64, linux-arm64 and linux-x64, to the sha256 of its
// download: a download whose bytes differ fails, and nothing of it is kept.
// A program is unpacked into a folder named after its version, which appears
// whole (a rename), so a folder that exists is a finished install.

const crypto = require('crypto');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { execFile } = require('child_process');
const { promisify } = require('util');

const setup = require('./setup');
const { step } = require('./progress');
const { withLock } = require('./pid-lock');
const { extractZip } = require('./zip-entry');

const execFileAsync = promisify(execFile);
const TOOLS_DIR = path.resolve(process.env.MCBENCH_TOOLS_DIR || path.join(os.homedir(), '.mcbench-lite', 'tools'));
const PLATFORM = `${process.platform}-${process.arch}`;

const TEMURIN = 'https://github.com/adoptium/temurin25-binaries/releases/download/jdk-25.0.5%2B8';
const RUSTUP_ARCHIVE = 'https://static.rust-lang.org/rustup/archive/1.29.3';
const CHROME_FOR_TESTING = 'https://cdn.playwright.dev/builds/cft';
const PLAYWRIGHT_BUILDS = 'https://cdn.playwright.dev/dbazure/download/playwright/builds/chromium';
const CODEX = 'https://github.com/openai/codex/releases/download/rust-v0.167.1';
const CLAUDE_CODE = 'https://storage.googleapis.com/claude-code-dist-86c565f3-f756-42ad-8dfa-d59b1c096819/claude-code-releases/2.1.311';

// Chromium's headless shell is the build playwright-core names in its
// browsers.json, from the addresses Playwright's own installer uses: Chrome
// for Testing's build, and on ARM Linux, which Chrome for Testing has no
// build for, Playwright's. Its checksums are kept per version, so a
// playwright-core that names another build fails here until it is pinned.
const HEADLESS_SHELL = JSON.parse(fs.readFileSync(path.join(path.dirname(require.resolve('playwright-core')), 'browsers.json'), 'utf8')).browsers.find((browser) => browser.name === 'chromium-headless-shell');
const CHROMIUM_SHA256 = {
  '149.0.7827.55': {
    'darwin-arm64': '302f82603be06683947594ecd60f849e362a8fe3dd82a89bd4408477c97e75a6',
    'darwin-x64': 'a32029e1861329a431b712d5b864e213d9cf8ef51a82ce4c24b27e25f6605434',
    'linux-arm64': '1652929a70f4afb17aca36fce073fb7ed22262d16825be761b0801972f43ac4f',
    'linux-x64': '410c9407d5de3fea80d9398666be06f2aa09154a3fa7b327dc254e336bb4c4b7',
  },
};

// Each program: its name in the terminal, its version, about how large its
// download is, how it is unpacked (a .tar.gz, a .zip, or the file itself,
// made executable), and per platform its address, checksum and the path of
// its program (`bin`, the folder of its programs, for the JDK) in its folder.
const PINS = {
  jdk: {
    title: 'Temurin 25.0.5+8, a JDK 25',
    version: '25.0.5+8',
    mb: 130,
    unpack: 'tar',
    platforms: {
      'darwin-arm64': { url: `${TEMURIN}/OpenJDK25U-jdk_aarch64_mac_hotspot_25.0.5_8.tar.gz`, sha256: '36dc14d797aaff6d89aab302cd053fdba6207dfdcf772052fa7b023a94b0e857', bin: 'jdk-25.0.5+8/Contents/Home/bin' },
      'darwin-x64': { url: `${TEMURIN}/OpenJDK25U-jdk_x64_mac_hotspot_25.0.5_8.tar.gz`, sha256: '32cd4d2151c91aa00d0fc911edfd05245ed40fa6b1aa894d5f35c928cd67a59d', bin: 'jdk-25.0.5+8/Contents/Home/bin' },
      'linux-arm64': { url: `${TEMURIN}/OpenJDK25U-jdk_aarch64_linux_hotspot_25.0.5_8.tar.gz`, sha256: 'bb5b2ded252f21c6090cf780b0a408ad0d83bbb557e702955643557f13231880', bin: 'jdk-25.0.5+8/bin' },
      'linux-x64': { url: `${TEMURIN}/OpenJDK25U-jdk_x64_linux_hotspot_25.0.5_8.tar.gz`, sha256: '68982529869257d0ee6b824ec5aa270ff8b118bb438670da0b97ebdab178d7fb', bin: 'jdk-25.0.5+8/bin' },
    },
  },
  'rustup-init': {
    title: 'rustup 1.29.3',
    version: '1.29.3',
    mb: 12,
    unpack: 'file',
    platforms: {
      'darwin-arm64': { url: `${RUSTUP_ARCHIVE}/aarch64-apple-darwin/rustup-init`, sha256: '63752a1919a3a4793173ec70f8f212621a7ce38ba6a246095984482f4de20f9f', bin: 'rustup-init' },
      'darwin-x64': { url: `${RUSTUP_ARCHIVE}/x86_64-apple-darwin/rustup-init`, sha256: 'bc107f1fb0228de7fa32413c89c1b305cea9cd256e94b01150cd054128145429', bin: 'rustup-init' },
      'linux-arm64': { url: `${RUSTUP_ARCHIVE}/aarch64-unknown-linux-gnu/rustup-init`, sha256: '9f608c142e0e38a78957d0661ba1dfaf0b6fb37184cfaa4b9a0aa57af6184282', bin: 'rustup-init' },
      'linux-x64': { url: `${RUSTUP_ARCHIVE}/x86_64-unknown-linux-gnu/rustup-init`, sha256: 'b1c8cb14e30889e7e2999e6b69e62d9e9b9ffc0f4df8180564d306d9680e6c55', bin: 'rustup-init' },
    },
  },
  chromium: {
    title: `Chromium ${HEADLESS_SHELL.browserVersion}'s headless shell`,
    version: HEADLESS_SHELL.browserVersion,
    mb: 110,
    unpack: 'zip',
    platforms: Object.fromEntries(Object.entries({
      'darwin-arm64': [`${CHROME_FOR_TESTING}/${HEADLESS_SHELL.browserVersion}/mac-arm64/chrome-headless-shell-mac-arm64.zip`, 'chrome-headless-shell-mac-arm64/chrome-headless-shell'],
      'darwin-x64': [`${CHROME_FOR_TESTING}/${HEADLESS_SHELL.browserVersion}/mac-x64/chrome-headless-shell-mac-x64.zip`, 'chrome-headless-shell-mac-x64/chrome-headless-shell'],
      'linux-arm64': [`${PLAYWRIGHT_BUILDS}/${HEADLESS_SHELL.revision}/chromium-headless-shell-linux-arm64.zip`, 'chrome-linux/headless_shell'],
      'linux-x64': [`${CHROME_FOR_TESTING}/${HEADLESS_SHELL.browserVersion}/linux64/chrome-headless-shell-linux64.zip`, 'chrome-headless-shell-linux64/chrome-headless-shell'],
    }).map(([platform, [url, bin]]) => [platform, { url, sha256: CHROMIUM_SHA256[HEADLESS_SHELL.browserVersion][platform], bin }])),
  },
  claude: {
    title: 'Claude Code 2.1.311',
    version: '2.1.311',
    mb: 280,
    unpack: 'file',
    platforms: {
      'darwin-arm64': { url: `${CLAUDE_CODE}/darwin-arm64/claude`, sha256: '85d8c08707fcfd1e78ffa0204eefbc6ec9e4fe22f375fb2767ef85d957ed3161', bin: 'claude' },
      'darwin-x64': { url: `${CLAUDE_CODE}/darwin-x64/claude`, sha256: 'd34e1cb82015b050471a97574aa6ea6f698ecf3a97a61c980961f43ad9f838f5', bin: 'claude' },
      'linux-arm64': { url: `${CLAUDE_CODE}/linux-arm64/claude`, sha256: '2e2496c9a8065df93acb0fca2d341f59a6b8777649b2e025ab8c9bcd5eef61f3', bin: 'claude' },
      'linux-x64': { url: `${CLAUDE_CODE}/linux-x64/claude`, sha256: '86dc1fcc69498159eaf15f446613e847010f9036bfbbdffd4c49350be8489ada', bin: 'claude' },
    },
  },
  // Codex's release package keeps the program at bin/codex, the layout
  // codex-cli.js mounts into its container on Linux as /opt/codex.
  codex: {
    title: 'Codex CLI 0.167.1',
    version: '0.167.1',
    mb: 180,
    unpack: 'tar',
    platforms: {
      'darwin-arm64': { url: `${CODEX}/codex-package-aarch64-apple-darwin.tar.gz`, sha256: '5bcbc5866cbca19f9299f7884e9b39b9ccb5469969c170b1f2d2cd6fc4b1ee72', bin: 'bin/codex' },
      'darwin-x64': { url: `${CODEX}/codex-package-x86_64-apple-darwin.tar.gz`, sha256: 'ade991ce7fef5b4f4af884c154b9455d493cd0caf13789fc3ff9f842897b55d3', bin: 'bin/codex' },
      'linux-arm64': { url: `${CODEX}/codex-package-aarch64-unknown-linux-musl.tar.gz`, sha256: 'ef3e1443a7795d596f1601a533d7225a1030488d60c965af1f0610ab95889031', bin: 'bin/codex' },
      'linux-x64': { url: `${CODEX}/codex-package-x86_64-unknown-linux-musl.tar.gz`, sha256: 'a945df3df83e849dd7b74787e27f8a459c1946b0e3afe3dc8eef8a8d1845fd4f', bin: 'bin/codex' },
    },
  },
};

// A download that fails on the network (no connection, a 5xx answer, a
// body that ends early) is tried again, DOWNLOAD_ATTEMPTS times in all,
// RETRY_MS apart. Any other failure, a checksum that does not match or a
// 4xx answer, fails at once.
const DOWNLOAD_ATTEMPTS = 3;
const RETRY_MS = 1000;
class NetworkFailure extends Error {}

// Downloads `url` to `target`, whose sha256 must be `sha256`, telling
// `onProgress` the bytes received ({ done, total }, total null when the
// server did not say). The file appears whole (a rename) only once its
// checksum matched.
async function download(url, target, options) {
  for (let attempt = 1; ; attempt += 1) {
    try {
      return await downloadOnce(url, target, options);
    } catch (error) {
      if (!(error instanceof NetworkFailure)) throw error;
      if (attempt === DOWNLOAD_ATTEMPTS) throw new Error(`${error.message} (tried ${DOWNLOAD_ATTEMPTS} times)`);
      console.log(`[tools] ${error.message}: trying again (${attempt + 1} of ${DOWNLOAD_ATTEMPTS})`);
      await new Promise((resolve) => setTimeout(resolve, RETRY_MS));
    }
  }
}

async function downloadOnce(url, target, { sha256, onProgress = () => {} }) {
  const failed = (why) => new NetworkFailure(`The download of ${url} failed: ${why}`);
  let response;
  try {
    response = await fetch(url, { redirect: 'follow' });
  } catch (error) {
    throw failed(error.cause?.message || error.message);
  }
  if (response.status >= 500) throw failed(`HTTP ${response.status}`);
  if (!response.ok) throw new Error(`The download of ${url} failed (HTTP ${response.status})`);
  const total = Number(response.headers.get('content-length')) || null;
  const temporary = `${target}.${process.pid}.download`;
  fs.mkdirSync(path.dirname(target), { recursive: true });
  const hash = crypto.createHash('sha256');
  const file = fs.openSync(temporary, 'w');
  let done = 0;
  try {
    for await (const chunk of response.body) {
      hash.update(chunk);
      fs.writeSync(file, chunk);
      done += chunk.length;
      onProgress({ done, total });
    }
    if (total !== null && done < total) throw failed(`it ended after ${done} of ${total} bytes`);
  } catch (error) {
    fs.rmSync(temporary, { force: true });
    throw error instanceof NetworkFailure ? error : failed(error.cause?.message || error.message);
  } finally {
    fs.closeSync(file);
  }
  const got = hash.digest('hex');
  if (got !== sha256) {
    fs.rmSync(temporary, { force: true });
    throw new Error(`${url} downloaded with sha256 ${got}, but mcbench-lite expects ${sha256}. It was not kept.`);
  }
  fs.renameSync(temporary, target);
}

// The folder `name` is installed in, and the path of its program there.
function installed(name, { pins = PINS, dir = TOOLS_DIR } = {}) {
  const pin = pins[name];
  const folder = path.join(dir, `${name}-${pin.version}`);
  return { folder, program: path.join(folder, pin.platforms[PLATFORM].bin) };
}

// Installs `name` unless it is installed, in one process at a time (a lock
// in the tools folder), telling `onProgress` how far its download is.
// Resolves with the path of its program.
async function install(name, { pins = PINS, dir = TOOLS_DIR, onProgress } = {}) {
  const pin = pins[name];
  const platform = pin.platforms[PLATFORM];
  if (!platform) throw new Error(`mcbench-lite has no pinned ${pin.title} for ${PLATFORM}`);
  const { folder, program } = installed(name, { pins, dir });
  if (fs.existsSync(folder)) return program;
  // Private to the person's account, as the data folder is: the programs in
  // it run as them, and are checked only when they are downloaded.
  fs.mkdirSync(dir, { recursive: true, mode: 0o700 });
  await withLock(`${folder}.lock`, async () => {
    if (fs.existsSync(folder)) return;
    const staging = `${folder}.${process.pid}.partial`;
    fs.rmSync(staging, { recursive: true, force: true });
    fs.mkdirSync(staging);
    const archive = path.join(dir, `${name}-${pin.version}.${process.pid}.${pin.unpack}`);
    try {
      await download(platform.url, pin.unpack === 'file' ? path.join(staging, platform.bin) : archive, { sha256: platform.sha256, onProgress });
      if (pin.unpack === 'file') fs.chmodSync(path.join(staging, platform.bin), 0o755);
      if (pin.unpack === 'tar') await execFileAsync('tar', ['-xzf', archive, '-C', staging], { maxBuffer: 1 << 24 });
      if (pin.unpack === 'zip') await extractZip(archive, staging);
      fs.renameSync(staging, folder);
    } finally {
      fs.rmSync(archive, { force: true });
      fs.rmSync(staging, { recursive: true, force: true });
    }
  });
  return program;
}

// The JDK the app runs Java with: Temurin's `java` and `javac` in the tools
// folder, installed by the first start (installJdk), unless MCBENCH_JAVA
// and MCBENCH_JAVAC name others.
const JDK_BIN = installed('jdk').program;
const JAVA = process.env.MCBENCH_JAVA || path.join(JDK_BIN, 'java');
const JAVAC = process.env.MCBENCH_JAVAC || path.join(JDK_BIN, 'javac');

// Installs the JDK the first time Java is needed, unless MCBENCH_JAVA and
// MCBENCH_JAVAC name one.
let jdk = null;
function installJdk() {
  jdk ||= setup.track('jdk', async () => {
    if (process.env.MCBENCH_JAVA && process.env.MCBENCH_JAVAC) return;
    setup.report('jdk', { detail: `Downloading ${PINS.jdk.title}` });
    await step(`[jdk] downloading ${PINS.jdk.title} (about ${PINS.jdk.mb} MB) into ${TOOLS_DIR}`, '[jdk] JDK installed', () => install('jdk', { onProgress: setup.progress('jdk') }));
  }).catch((error) => {
    jdk = null;
    throw error;
  });
  return jdk;
}

// rustup and the toolchains it installs are the tools folder's: RUSTUP_HOME
// and CARGO_HOME are set for every rustup and cargo the app runs, so nothing
// in the person's shell profile, PATH or ~/.rustup changes.
const RUST_DIR = path.join(TOOLS_DIR, 'rust');
const RUST_ENV = { RUSTUP_HOME: path.join(RUST_DIR, 'rustup'), CARGO_HOME: path.join(RUST_DIR, 'cargo') };
const RUSTUP = path.join(RUST_ENV.CARGO_HOME, 'bin', 'rustup');

// Installs rustup alone into RUST_DIR, unless it is there: the pinned
// rustup-init, run with no toolchain and no change to PATH, into a folder
// that appears whole. The toolchain core/ pins is installed by
// engine.js installRust.
async function installRustup({ onProgress } = {}) {
  if (fs.existsSync(RUST_DIR)) return;
  const init = await install('rustup-init', { onProgress });
  await withLock(`${RUST_DIR}.lock`, async () => {
    if (fs.existsSync(RUST_DIR)) return;
    const staging = `${RUST_DIR}.${process.pid}.partial`;
    fs.rmSync(staging, { recursive: true, force: true });
    try {
      await execFileAsync(init, ['-y', '--quiet', '--no-modify-path', '--profile', 'minimal', '--default-toolchain', 'none'], {
        env: { ...process.env, RUSTUP_HOME: path.join(staging, 'rustup'), CARGO_HOME: path.join(staging, 'cargo') },
        maxBuffer: 1 << 24,
      });
      fs.renameSync(staging, RUST_DIR);
    } finally {
      fs.rmSync(staging, { recursive: true, force: true });
    }
  });
}

// see_build's browser: the headless shell playwright-core names, installed
// by the first start, and checked to start on this machine (checkShell).
// Resolves with the path of its program.
let shell = null;
function headlessShell() {
  shell ||= setup.track('browser', async () => {
    const program = await step(`[see_build] downloading ${PINS.chromium.title} (about ${PINS.chromium.mb} MB) into ${TOOLS_DIR}`, '[see_build] headless browser installed',
      () => install('chromium', { onProgress: setup.progress('browser') }));
    await checkShell(program);
    return program;
  }).catch((error) => {
    shell = null;
    throw error;
  });
  return shell;
}

// Whether the headless shell starts here: it is run with --version, which
// loads every library it is linked with. A Linux without the shell's shared
// libraries (a server install, a container; a desktop has them) fails with
// the libraries `ldd` finds missing and the one command that installs them:
// playwright-core's own install-deps, run with this Node as root, which
// knows the packages of each Debian and Ubuntu release. Without apt-get the
// libraries are named for the system's own package manager.
async function checkShell(program, { platform = process.platform, ldd = 'ldd', aptGet = '/usr/bin/apt-get' } = {}) {
  try {
    await execFileAsync(program, ['--version'], { timeout: 60_000 });
    return;
  } catch (error) {
    const said = String(error.stderr || error.message).trim();
    const missing = platform === 'linux'
      ? [...new Set([...(await execFileAsync(ldd, [program])).stdout.matchAll(/^\s*(\S+) => not found/gm)].map((match) => match[1]))]
      : [];
    if (!missing.length) throw new Error(`The headless browser ${program} does not start: ${said}`);
    const install = fs.existsSync(aptGet)
      ? `Install them with this command, then try again:\n${process.getuid() === 0 ? '' : 'sudo '}${[process.execPath, path.join(path.dirname(require.resolve('playwright-core')), 'cli.js'), 'install-deps', 'chromium'].map(shellWord).join(' ')}`
      : "Install them with this system's package manager, then try again.";
    throw new Error(`The headless browser cannot start: this system does not have the shared libraries ${missing.join(', ')}. ${install}`);
  }
}

// A word as a shell reads it: quoted when it holds anything but plain characters.
const shellWord = (word) => (/^[\w@%+=:,./-]+$/.test(word) ? word : `'${word.replace(/'/g, "'\\''")}'`);

// The program the app runs for a CLI harness (claude or codex) that no
// environment variable names: the one on PATH, as before the tools folder
// existed, and when there is none the tools folder's (cli-install.js puts
// it there on request).
function cliProgram(program) {
  return onPath(program) ? program : installed(program).program;
}

// Whether `program` is on PATH, as a spawn would find it.
function onPath(program) {
  return (process.env.PATH || '').split(path.delimiter).filter(Boolean).some((folder) => {
    try {
      fs.accessSync(path.join(folder, program), fs.constants.X_OK);
      return true;
    } catch {
      return false;
    }
  });
}

module.exports = {
  PINS, PLATFORM, TOOLS_DIR, JAVA, JAVAC, RUST_ENV, RUSTUP, CHROMIUM_SHA256,
  download, install, installed, installJdk, installRustup, headlessShell, checkShell, onPath, cliProgram,
};
