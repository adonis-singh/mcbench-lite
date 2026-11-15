'use strict';

// The Codex command reached through each kind of shim on a scratch PATH:
// mise's (a symlink to one program that starts the tool it is named as),
// asdf's (a shell script that execs the manager, which execs the tool),
// npm's (a symlink to a Node launcher that starts the native program beside
// it as a child), the program itself or a symlink to it (Homebrew, the
// standalone installer), and MCBENCH_CODEX_BINARY naming a path or a bare
// command. Each leads to its own fake Codex install (a codex-package.json and
// bin/codex, the fake in test/fixtures): the version, the model list and a
// turn's launch on the host run it, and the container mounts its folder.

const { test, after } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFile } = require('node:child_process');
const { promisify } = require('node:util');

// The data folder, where the install is found from, is outside the
// temporary folder, as it is in use (the fake, like Codex on Linux, makes no
// apply_patch link for a CODEX_HOME under it).
const scratch = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-codex-shims-')));
fs.mkdirSync(path.join(scratch, 'tmp'));
Object.assign(process.env, {
  TMPDIR: path.join(scratch, 'tmp'),
  MCBENCH_DATA_DIR: path.join(scratch, 'data'),
  MCBENCH_SETTINGS_FILE: path.join(scratch, 'settings.json'),
  MCBENCH_TOOLS_DIR: path.join(scratch, 'tools'),
  CODEX_HOME: path.join(scratch, 'codex-home'),
});
delete process.env.MCBENCH_CODEX_BINARY;
const previousPath = process.env.PATH;
const settings = require('../server/settings');
const codexCli = require('../server/codex-cli');

after(() => {
  process.env.PATH = previousPath;
  fs.rmSync(scratch, { recursive: true, force: true });
});

const FAKE = path.join(__dirname, 'fixtures', 'fake-codex.js');
const write = (file, text) => {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, text, { mode: 0o755 });
  return file;
};
const link = (target, file) => {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.symlinkSync(target, file);
  return file;
};
// A Codex install in the release package's layout; its bin/codex runs the fake.
function install(folder) {
  write(path.join(folder, 'codex-package.json'), JSON.stringify({ layoutVersion: 1, version: '0.167.1', variant: 'codex', entrypoint: 'bin/codex' }));
  write(path.join(folder, 'bin', 'codex'), `#!${process.execPath}\nrequire(${JSON.stringify(FAKE)});\n`);
  return folder;
}
// One program that starts the tool it is named as, and fails as mise does otherwise.
const multiplexer = (file, program) => write(file, `#!/bin/sh\ncase "$(basename "$0")" in\n  codex) exec ${JSON.stringify(program)} "$@" ;;\nesac\n`
  + 'echo "multiplexer ERROR no tasks defined in $(pwd). Are you in a project directory?" >&2\nexit 1\n');
// asdf's shim and the manager it execs.
function asdf(dir, name, program) {
  const manager = write(path.join(dir, 'asdf', 'bin', 'asdf'),
    `#!/bin/sh\nif [ "$1" = exec ] && [ "$2" = codex ]; then shift 2; exec ${JSON.stringify(program)} "$@"; fi\necho "asdf: unknown command" >&2\nexit 1\n`);
  return write(path.join(dir, 'shims', name), `#!/bin/sh\nexec ${JSON.stringify(manager)} exec "codex" "$@"\n`);
}
// npm's global `codex`: a symlink to the package's launcher, which runs the
// platform package's program as a child (as on a Node without execve).
function npm(dir) {
  const pkg = path.join(dir, 'lib', 'node_modules', '@openai', 'codex');
  const folder = install(path.join(pkg, 'vendor', 'x86_64-unknown-linux-musl'));
  write(path.join(pkg, 'bin', 'codex.js'), `#!${process.execPath}\nconst { spawn } = require('node:child_process');\n`
    + `const child = spawn(${JSON.stringify(path.join(folder, 'bin', 'codex'))}, process.argv.slice(2), { stdio: 'inherit' });\n`
    + 'child.on(\'exit\', (code) => process.exit(code));\n');
  link(path.join('..', 'lib', 'node_modules', '@openai', 'codex', 'bin', 'codex.js'), path.join(dir, 'bin', 'codex'));
  return { bin: path.join(dir, 'bin'), folder };
}
const SYSTEM = ['/usr/bin', '/bin'];

function cases() {
  const root = (name) => path.join(scratch, name);
  const mise = install(root('mise/installs/codex/0.167.1'));
  multiplexer(root('mise/bin/mise'), path.join(mise, 'bin', 'codex'));
  link(root('mise/bin/mise'), root('mise/shims/codex'));
  const asdfInstall = install(root('asdf/installs/codex/0.167.1'));
  asdf(root('asdf'), 'codex', path.join(asdfInstall, 'bin', 'codex'));
  const npmInstall = npm(root('npm'));
  const direct = install(root('direct'));
  const standalone = install(root('standalone/releases/0.167.1'));
  link(path.join(standalone, 'bin', 'codex'), root('standalone/bin/codex'));
  const named = install(root('named/installs/0.167.1'));
  multiplexer(root('named/mise'), path.join(named, 'bin', 'codex'));
  link(root('named/mise'), root('named/elsewhere/codex'));
  const beta = install(root('beta/installs/0.167.1'));
  asdf(root('beta'), 'codex-beta', path.join(beta, 'bin', 'codex'));
  return [
    { kind: 'a mise-style symlink to a multiplexer', path: [root('mise/shims')], folder: mise },
    { kind: 'an asdf-style shell-script shim', path: [root('asdf/shims')], folder: asdfInstall },
    { kind: 'an npm-style symlink to a launcher that starts the program as a child', path: [npmInstall.bin], folder: npmInstall.folder },
    { kind: 'the program itself on PATH', path: [path.join(direct, 'bin')], folder: direct },
    { kind: 'a symlink to the program (Homebrew, the standalone installer)', path: [root('standalone/bin')], folder: standalone },
    { kind: 'MCBENCH_CODEX_BINARY naming a shim by path', path: [], binary: root('named/elsewhere/codex'), folder: named },
    { kind: 'MCBENCH_CODEX_BINARY naming a bare command on PATH', path: [root('beta/shims')], binary: 'codex-beta', folder: beta },
  ];
}

// The command the environment gives, as settings.js configures it.
function use({ path: dirs, binary }) {
  process.env.PATH = [...dirs, ...SYSTEM].join(path.delimiter);
  if (binary) process.env.MCBENCH_CODEX_BINARY = binary;
  else delete process.env.MCBENCH_CODEX_BINARY;
  settings.refreshRuntimes();
  codexCli.forgetModels();
}

test('every kind of shim runs the Codex it leads to on the host, and the container mounts that install', async () => {
  for (const entry of cases()) {
    use(entry);
    assert.equal(codexCli.version(), 'codex-cli 0.167.1', entry.kind);
    assert.deepEqual((await codexCli.listModels()).map((model) => model.id), ['fake-model'], entry.kind);

    // A turn's launch on the host (macOS) runs the command, and it is the install's program that runs.
    const dir = fs.mkdtempSync(path.join(scratch, 'turn-'));
    const runtimeDir = fs.mkdtempSync(path.join(scratch, 'run-'));
    const launch = await codexCli._internal.LAUNCHERS.darwin({ dir, runtimeDir, socket: path.join(runtimeDir, 'mcp.sock'), model: 'fake-model',
      effort: null, systemPrompt: 'S', resume: false, threadId: null, prompt: { images: [], tools: [], call: async () => ({}) } });
    try {
      const { stdout } = await promisify(execFile)(launch.command, launch.args, { cwd: launch.cwd, env: launch.env });
      assert.deepEqual(JSON.parse(stdout.split('\n')[0]), { type: 'fake.ran', executable: path.join(entry.folder, 'bin', 'codex') }, entry.kind);
    } finally {
      await launch.release();
    }

    // The container (Linux) mounts the install the command runs and starts its program.
    const found = await codexCli.codexInstall();
    assert.deepEqual(found, { folder: entry.folder, program: '/opt/codex/bin/codex' }, entry.kind);
    const args = codexCli.containerArgs({ name: 'n', runtimeDir, home: path.join(dir, 'home'), workspace: path.join(dir, 'workspace'), install: found, command: [found.program, 'exec'] });
    assert.equal(args[args.indexOf(`${entry.folder}:/opt/codex:ro`) - 1], '-v', entry.kind);
    assert.deepEqual(args.slice(-2), ['/opt/codex/bin/codex', 'exec'], entry.kind);
  }
});

test('a shim that leads nowhere fails, naming the command, and a program outside a Codex install is not mounted', async () => {
  const nowhere = path.join(scratch, 'nowhere');
  multiplexer(path.join(nowhere, 'mise'), path.join(nowhere, 'installs', 'codex', 'bin', 'codex'));
  link(path.join(nowhere, 'mise'), path.join(nowhere, 'shims', 'codex'));
  use({ path: [path.join(nowhere, 'shims')] });
  assert.throws(() => codexCli.version(), /Command failed: codex --version/);
  await assert.rejects(codexCli.listModels(), /Command failed: codex debug models/);
  await assert.rejects(codexCli.codexInstall(), /Command failed: codex --version/);

  // A Codex with no codex-package.json above it, as a lone binary in a system folder would be.
  const bare = write(path.join(scratch, 'bare', 'bin', 'codex'), `#!${process.execPath}\nrequire(${JSON.stringify(FAKE)});\n`);
  use({ path: [path.join(scratch, 'bare', 'bin')] });
  assert.equal(codexCli.version(), 'codex-cli 0.167.1');
  await assert.rejects(codexCli.codexInstall(), new RegExp(`^Error: \`codex --version\` \\(codex-cli 0\\.167\\.1\\) started ${bare.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}, which no codex-package\\.json`));
});
