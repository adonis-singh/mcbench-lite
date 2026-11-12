'use strict';
// HTTP CONNECT proxy on a Unix socket for containerized harnesses. It opens
// tunnels only to the listed model-provider hosts on port 443, refuses every
// other request, and records each decision.
const fs = require('node:fs');
const http = require('node:http');
const net = require('node:net');
const { once } = require('node:events');

function egressAllowed(target, allowedHosts) {
  const match = /^([a-z0-9.-]+):(\d+)$/i.exec(target);
  return Boolean(match) && match[2] === '443' && allowedHosts.includes(match[1].toLowerCase());
}

async function startEgressProxy(socketPath, allowedHosts, logFile) {
  const log = (target, allowed) => fs.appendFileSync(logFile, JSON.stringify({ at: new Date().toISOString(), target, allowed }) + '\n');
  const server = http.createServer((req, res) => { log(`${req.method} ${req.url}`, false); res.writeHead(403); res.end(); });
  server.on('connect', (req, client, head) => {
    const allowed = egressAllowed(req.url, allowedHosts);
    log(req.url, allowed);
    client.on('error', () => {});
    if (!allowed) return client.end('HTTP/1.1 403 Forbidden\r\n\r\n');
    const upstream = net.connect(443, req.url.split(':')[0], () => {
      client.write('HTTP/1.1 200 Connection Established\r\n\r\n');
      upstream.write(head);
      upstream.pipe(client);
      client.pipe(upstream);
    });
    upstream.on('error', () => client.destroy());
    client.on('close', () => upstream.destroy());
  });
  server.listen(socketPath);
  await once(server, 'listening');
  return server;
}

module.exports = { startEgressProxy, egressAllowed };
