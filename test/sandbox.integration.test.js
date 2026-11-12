'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

test('sandbox renders SVG and cancellation kills foreground and background processes in both shell modes', {
  skip: process.env.MCBENCH_SANDBOX_INTEGRATION !== '1', timeout: 600_000,
}, async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-sandbox-integration-'));
  process.env.MCBENCH_DATA_DIR = root;
  const sandbox = require('../server/sandbox');
  const id = `integration-${process.pid}`;
  try {
    sandbox.writeWorkFile(id, 'fixture.svg', '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="red"/></svg>');
    const svg = await sandbox.exec(id, 'rsvg-convert fixture.svg -o native.png && convert fixture.svg converted.png && identify native.png converted.png', 20_000);
    assert.equal(svg.exitCode, 0, svg.formatted);
    assert.match(svg.formatted, /PNG 10x10/);
    for (const network of [false, true]) {
      const controller = new AbortController();
      const tag = network ? 'network' : 'persistent';
      const running = sandbox.exec(id, `touch ${tag}-started; (sleep 3; touch ${tag}-leaked) & wait`, 15_000, { network, signal: controller.signal });
      // Attach the rejection handler before cancelling.
      const cancelled = assert.rejects(running, /abort/i);
      const deadline = Date.now() + 15_000;
      while (!fs.existsSync(path.join(sandbox.workDir(id), `${tag}-started`))) {
        if (Date.now() > deadline) throw new Error('Sandbox command did not start');
        await new Promise(resolve => setTimeout(resolve, 50));
      }
      controller.abort();
      await cancelled;
      await new Promise(resolve => setTimeout(resolve, 3200));
      assert.equal(fs.existsSync(path.join(sandbox.workDir(id), `${tag}-leaked`)), false);
    }
    const next = await sandbox.exec(id, 'test -f fixture.svg && printf recovered', 5000);
    assert.equal(next.exitCode, 0);
    assert.match(next.formatted, /recovered/);

    // A timeout ends the shell and its jobs but keeps the container.
    const { execFileSync } = require('node:child_process');
    const containerId = () => execFileSync('docker', ['inspect', '-f', '{{.Id}}', sandbox._internal.containerName(id)], { encoding: 'utf8' }).trim();
    const before = containerId();
    await sandbox.exec(id, 'cd /tmp && export MARK=1', 5000);
    const timedOut = await sandbox.exec(id, '(sleep 3; touch /work/timeout-leaked) & sleep 30', 1000);
    assert.equal(timedOut.exitCode, 124);
    assert.match(timedOut.formatted, /shell session was reset/);
    const fresh = await sandbox.exec(id, 'pwd; printf "mark=%s" "$MARK"', 5000);
    assert.match(fresh.formatted, /\/work\nmark=$/);
    assert.equal(containerId(), before);
    await new Promise(resolve => setTimeout(resolve, 3200));
    assert.equal(fs.existsSync(path.join(sandbox.workDir(id), 'timeout-leaked')), false);

    // A networked command is one-shot: what it leaves running ends with it.
    const shot = await sandbox.exec(id, '(sleep 2; touch net-bg) & printf launched', 10_000, { network: true });
    assert.match(shot.formatted, /launched/);
    const started = Date.now();
    const again = await sandbox.exec(id, 'printf again', 10_000, { network: true });
    assert.match(again.formatted, /again/);
    assert.ok(Date.now() - started < 1500, `a warm networked call took ${Date.now() - started} ms`);
    await new Promise(resolve => setTimeout(resolve, 2500));
    assert.equal(fs.existsSync(path.join(sandbox.workDir(id), 'net-bg')), false);
  } finally {
    await sandbox.destroy(id);
    fs.rmSync(root, { recursive: true, force: true });
  }
});

// A networked command reaches the internet but not the machine it runs on:
// a server on the host's loopback (where this app's API listens), the
// Docker gateway and the metadata address all refuse it; DNS still resolves,
// the rules hold after the container was stopped, and it has no IPv6: every
// interface has it off and none has an IPv6 address.
test('the networked sandbox cannot reach the host, the LAN or a metadata service', {
  skip: process.env.MCBENCH_SANDBOX_INTEGRATION !== '1', timeout: 600_000,
}, async () => {
  const http = require('node:http');
  const { execFileSync } = require('node:child_process');
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-sandbox-egress-'));
  process.env.MCBENCH_DATA_DIR = root;
  // The first test's data folder is gone: load the sandbox for this one.
  for (const name of ['sandbox', 'storage']) delete require.cache[require.resolve(`../server/${name}`)];
  const sandbox = require('../server/sandbox');
  const id = `egress-${process.pid}`;
  const server = http.createServer((req, res) => res.end('HOST-REACHED'));
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address();
  const probe = `for url in http://host.docker.internal:${port}/ http://$(awk '$2 == "00000000" { printf "%d.%d.%d.%d", "0x" substr($3,7,2), "0x" substr($3,5,2), "0x" substr($3,3,2), "0x" substr($3,1,2) }' /proc/net/route):${port}/ http://169.254.169.254/; do
    curl -s -m 3 -o /dev/null -w "%{http_code} " "$url"; done; getent hosts example.com > /dev/null && printf dns
    printf ' ipv6-off:%s addresses:%s' "$(cat /proc/sys/net/ipv6/conf/*/disable_ipv6 | sort -u | tr -d '\\n')" "$(wc -l < /proc/net/if_inet6)"`;
  try {
    for (const round of ['fresh', 'restarted']) {
      const result = await sandbox.exec(id, probe, 30_000, { network: true });
      assert.doesNotMatch(result.formatted, /HOST-REACHED|200/, round);
      assert.match(result.formatted, /000 000 000 dns ipv6-off:1 addresses:0/, round);
      execFileSync('docker', ['stop', sandbox._internal.containerName(id, { network: true })]);
    }
  } finally {
    server.close();
    await sandbox.destroy(id);
    fs.rmSync(root, { recursive: true, force: true });
  }
});
