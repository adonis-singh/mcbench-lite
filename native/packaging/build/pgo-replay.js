'use strict';

// Sends a recording of pgo-workload.js to `<program> headless engine
// <tables>`, the instrumented mcbench-lite of a release build (pgo.md). It
// loads nothing of the Node server, so it runs on macOS, Linux and Windows.
//
//   node native/packaging/build/pgo-replay.js <recording> <bundle> <work> <program> [--times] [--write-counts]
//
// <recording> is pgo-workload.js's archive (a private recording of the
// corpus), unpacked into <work>/recording; <bundle> is the
// Minecraft data bundle (MCBENCH_DATA_BUNDLE), whose three engine tables are
// unpacked into <work>/tables. <work> is made fresh. For each of the
// recording's folders, in order, one engine process starts in
// <work>/<folder> with the folder's world/ beside it, is sent every line of
// requests.txt with `@<n>` replaced by the input file in/<n> and `@out` by an
// output of its own, one request after the answer to the one before, and
// ends by its input closing, so the instrumented program returns from main
// and writes its profile. A refusal is an answer (the engine refuses a spent
// world's requests until the next fresh placement); an engine that exits
// otherwise than with code 0 fails the replay.
//
// The counts file beside the recording (<recording> with .counts for
// .tar.zst: one line per folder, its name, its number of requests and the
// number its engine refused, from a replay on Linux) is what every replay
// must give: a folder whose counts differ fails the replay, so a profile of
// an engine that answered otherwise is not shipped. --write-counts writes
// that file from this replay instead (after the recording is made again).
// --times prints, per folder, the seconds summed by request word (PLACE,
// TICK, VERIFY, INPUT, the others together), each from the request's write
// to its answer.

const fs = require('fs');
const path = require('path');
const zlib = require('zlib');
const { spawn } = require('child_process');

const SWITCHES = ['--times', '--write-counts'];
const args = process.argv.slice(2).filter((arg) => !SWITCHES.includes(arg));
if (args.length !== 4 || process.argv.slice(2).some((arg) => arg.startsWith('--') && !SWITCHES.includes(arg))) {
  console.error('usage: node native/packaging/build/pgo-replay.js <recording> <bundle> <work> <program> [--times] [--write-counts]');
  process.exit(2);
}
const [archive, bundle, work, program] = args.map((arg) => path.resolve(arg));
const times = process.argv.includes('--times');
const writeCounts = process.argv.includes('--write-counts');
const countsFile = archive.replace(/\.tar\.zst$/, '.counts');
const TABLES = ['blocks.txt', 'loot.json', 'datafix.txt'];

// The files of a ustar archive compressed by zstd (the data bundle,
// native/tools/extract/README.md, and the recording) that `keep` takes,
// written under `folder`; resolves with their names.
function unpack(file, folder, keep) {
  const tar = zlib.zstdDecompressSync(fs.readFileSync(file));
  const found = [];
  for (let at = 0; at + 512 <= tar.length && tar[at] !== 0;) {
    const name = tar.subarray(at, at + 100).toString('utf8').replace(/\0.*$/s, '');
    const size = parseInt(tar.subarray(at + 124, at + 136).toString('utf8').replace(/\0.*$/s, '').trim(), 8);
    if (keep(name)) {
      fs.mkdirSync(path.dirname(path.join(folder, name)), { recursive: true });
      fs.writeFileSync(path.join(folder, name), tar.subarray(at + 512, at + 512 + size));
      found.push(name);
    }
    at += 512 + Math.ceil(size / 512) * 512;
  }
  return found;
}

async function replay(sequence, tables) {
  const name = path.basename(sequence);
  const folder = path.join(work, name);
  fs.cpSync(path.join(sequence, 'world'), path.join(folder, 'world'), { recursive: true });
  const lines = fs.readFileSync(path.join(sequence, 'requests.txt'), 'utf8').split('\n').filter(Boolean);
  const child = spawn(program, ['headless', 'engine', tables], { cwd: folder, stdio: ['pipe', 'pipe', 'inherit'] });
  const exited = new Promise((resolve, reject) => {
    child.on('error', reject);
    child.on('close', resolve);
  });
  // Each answer line resolves the request waiting for it.
  let buffer = '';
  let waiting = null;
  child.stdout.on('data', (chunk) => {
    buffer += chunk;
    const answers = buffer.split('\n');
    buffer = answers.pop();
    for (const answer of answers) if (waiting && waiting.test(answer)) waiting.resolve(answer);
  });
  const answer = (test) => new Promise((resolve, reject) => {
    waiting = { test, resolve };
    exited.then((code) => reject(new Error(`the engine of ${name} exited (code ${code}) before it answered`)), reject);
  });
  await answer((line) => line === 'MCBENCH_ENGINE_READY');
  let refused = 0;
  const started = Date.now();
  const spent = { PLACE: 0, TICK: 0, VERIFY: 0, INPUT: 0, other: 0 };
  for (const [index, line] of lines.entries()) {
    const words = line.split(' ').map((word, at) => {
      // A VERIFY's output is a folder of its own; another request's is a
      // file, which a later request may replace.
      if (word === '@out' && line.startsWith('VERIFY ')) {
        const out = path.join(folder, `verify-${index % 4}`);
        fs.rmSync(out, { recursive: true, force: true });
        fs.mkdirSync(out);
        return out;
      }
      if (word === '@out') return path.join(folder, `out-${at}`);
      return word.startsWith('@') ? path.join(sequence, 'in', word.slice(1)) : word;
    });
    const sent = process.hrtime.bigint();
    child.stdin.write(`${words.join(' ')}\n`);
    const reply = await answer((text) => text.split(' ')[1] === words[1] && /^MCBENCH_/.test(text));
    spent[words[0] in spent ? words[0] : 'other'] += Number(process.hrtime.bigint() - sent) / 1e9;
    if (reply.split(' ')[0].endsWith('FAILED')) refused++;
  }
  child.stdin.end();
  const code = await exited;
  if (code !== 0) throw new Error(`the engine of ${name} exited with code ${code}, so it wrote no profile`);
  console.log(`pgo-replay: ${name}: ${lines.length} requests, ${refused} refused, ${((Date.now() - started) / 1000).toFixed(1)} s`);
  if (times) console.log(`pgo-replay: ${name} times: ${Object.entries(spent).map(([word, seconds]) => `${word} ${seconds.toFixed(3)} s`).join(', ')}`);
  return { name, requests: lines.length, refused };
}

async function main() {
  fs.rmSync(work, { recursive: true, force: true });
  const tables = path.join(work, 'tables');
  const found = unpack(bundle, tables, (name) => TABLES.includes(name));
  const missing = TABLES.filter((name) => !found.includes(name));
  if (missing.length) throw new Error(`${bundle} holds no ${missing.join(', ')}`);
  const recording = path.join(work, 'recording');
  unpack(archive, recording, () => true);
  const sequences = fs.readdirSync(recording).sort().map((name) => path.join(recording, name));
  if (!sequences.length) throw new Error(`${recording} holds no recorded engine`);
  const expected = writeCounts ? null : new Map(fs.readFileSync(countsFile, 'utf8').split('\n').filter(Boolean).map((line) => {
    const [name, requests, refused] = line.split('\t');
    return [name, { requests: Number(requests), refused: Number(refused) }];
  }));
  if (expected && expected.size !== sequences.length) throw new Error(`${countsFile} names ${expected.size} folders and the recording holds ${sequences.length}`);
  const counts = [];
  for (const sequence of sequences) {
    const got = await replay(sequence, tables);
    counts.push(got);
    const want = expected && expected.get(got.name);
    if (want && (want.requests !== got.requests || want.refused !== got.refused)) {
      throw new Error(`the engine of ${got.name} answered ${got.requests} requests with ${got.refused} refused, and ${countsFile} expects ${want.requests} with ${want.refused}: this program does not answer as the recording's engine did, so its profile is not used`);
    }
    if (expected && !want) throw new Error(`${countsFile} has no line for ${got.name}`);
  }
  if (writeCounts) {
    fs.writeFileSync(countsFile, counts.map(({ name, requests, refused }) => `${name}\t${requests}\t${refused}\n`).join(''));
    console.log(`pgo-replay: ${countsFile} written`);
  }
}

main().catch((error) => {
  console.error(`pgo-replay: ${error.message}`);
  process.exit(1);
});
