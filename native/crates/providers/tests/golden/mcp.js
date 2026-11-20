#!/usr/bin/env node
'use strict';
// Writes mcp.jsonl beside this script: each step of mcp-steps.json sent to
// today's server/mcp.js serveMcp over its Unix socket, and every line the
// server answered before the step's wait ended. The host is the one
// tests/unix/bridge_tests.rs gives the Rust bridge: `echo` answers its
// name, arguments, callId and toolUseId as text; `image` an image block;
// `fail` throws `failed: <name>`; `slow` answers after 300 ms; `hang` never
// answers, so a cancel or a disconnect is what ends it.
//
//   node mcp.js [<checkout>]

const fs = require('node:fs');
const net = require('node:net');
const os = require('node:os');
const path = require('node:path');

const root = path.resolve(process.argv[2] || path.join(__dirname, '../../../../..'));
const { serveMcp } = require(path.join(root, 'server/mcp'));
const steps = JSON.parse(fs.readFileSync(path.join(__dirname, 'mcp-steps.json'), 'utf8'));
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const host = {
  tools: [
    { type: 'function', function: { name: 'echo', description: 'Echo', parameters: { type: 'object', properties: { n: { type: 'number' } } } } },
    { type: 'function', function: { name: 'nodesc', parameters: { type: 'object' } } },
  ],
  async call(name, args, { signal, callId, toolUseId }) {
    if (name === 'fail') throw new Error(`failed: ${name}`);
    if (name === 'image') return [{ type: 'image', data: 'AA==', mimeType: 'image/png' }, { type: 'text', text: 'shot' }];
    if (name === 'slow') await sleep(300);
    if (name === 'hang') await new Promise((resolve, reject) => signal.addEventListener('abort', () => reject(signal.reason)));
    return [{ type: 'text', text: JSON.stringify({ name, args, callId, toolUseId: toolUseId ?? null }) }];
  },
};

async function main() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-mcp-golden-'));
  const socket = path.join(dir, 'mcp.sock');
  const server = await serveMcp(socket, host);
  const out = [];
  for (const step of steps) {
    const client = net.connect(socket);
    await new Promise((resolve) => client.once('connect', resolve));
    let received = '';
    client.on('data', (chunk) => { received += chunk; });
    for (const part of step.send) {
      client.write(part);
      await sleep(step.gap ?? 20);
    }
    await sleep(step.wait);
    if (step.end) client.end();
    await sleep(100);
    client.destroy();
    out.push({ name: step.name, received });
  }
  await server.shutdown();
  fs.rmSync(dir, { recursive: true, force: true });
  fs.writeFileSync(path.join(__dirname, 'mcp.jsonl'), out.map((o) => JSON.stringify(o)).join('\n') + '\n');
  console.log(`mcp golden: ${out.length} steps`);
}

main();
