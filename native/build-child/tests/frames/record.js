'use strict';

// Records the build child's process protocol from today's child,
// server/build-child.js, run by Node 26.13.1 with an empty environment:
// for each case of cases.json, <name>.in is what the parent writes on the
// child's stdin (the request frame, the world frame, then the answers to the
// child's READ and SETTLED frames in order; build-child.js reads stdin only
// when it needs the next bytes, so they can all be written at once) and
// <name>.out is what the child wrote on stdout. Every world here is
// encodeWorld(new World()).
//
//   node native/build-child/tests/frames/record.js
//
// The child's tests/protocol.rs requires mcbench-build-child to answer each
// .in with exactly the .out; mcbench-build's tests/child/protocol.rs requires
// the parent to write exactly the .in and to read the .out as it reads the
// real child's answer.

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const here = __dirname;
const server = path.resolve(here, '../../../../server');
const { encodeWorld } = require(path.join(server, 'build-transfer.js'));
const { World } = require(path.join(server, 'world.js'));

if (process.version !== 'v26.13.1') throw new Error(`record.js needs Node 26.13.1, not ${process.version}`);

const u32 = (n) => { const b = Buffer.alloc(4); b.writeUInt32LE(n, 0); return b; };
const frame = (payload) => Buffer.concat([u32(payload.length), payload]);
const empty = encodeWorld(new World());

for (const c of JSON.parse(fs.readFileSync(path.join(here, 'cases.json'), 'utf8'))) {
  const parts = [frame(Buffer.from(JSON.stringify(c.request), 'utf8')), frame(empty)];
  for (const answer of c.answers) {
    if ('read' in answer) parts.push(Buffer.from([answer.ok ? 1 : 0]), frame(Buffer.from(answer.text, 'utf8')));
    else parts.push(frame(empty));
  }
  const input = Buffer.concat(parts);
  const run = spawnSync(process.execPath, [path.join(server, 'build-child.js')], { input, env: {}, cwd: '/' });
  if (run.status !== 0) throw new Error(`${c.name}: build-child.js exited ${run.status}: ${run.stderr}`);
  fs.writeFileSync(path.join(here, `${c.name}.in`), input);
  fs.writeFileSync(path.join(here, `${c.name}.out`), run.stdout);
  console.log(`${c.name}: ${input.length} bytes in, ${run.stdout.length} bytes out`);
}
