'use strict';
// Minimal MCP server (JSON-RPC 2.0, newline-delimited) on a Unix socket,
// bound to one conversation turn or batch attempt. CLI harnesses reach it
// through mcp-bridge.py as an ordinary stdio MCP server; the Pi extension
// connects to it directly. `host.tools` is the offered tool list and
// `host.call(name, args, { signal, callId, toolUseId })` resolves to MCP
// content blocks; `toolUseId` is the model's tool_use id when the client sends
// it (Claude Code does, as `_meta["claudecode/toolUseId"]`).
const fs = require('node:fs');
const net = require('node:net');
const path = require('node:path');
const readline = require('node:readline');
const { once } = require('node:events');

const BRIDGE = path.join(__dirname, 'mcp-bridge.py');

async function serveMcp(socketPath, host) {
  fs.mkdirSync(path.dirname(socketPath), { recursive: true });
  const sockets = new Set();
  const server = net.createServer(socket => {
    sockets.add(socket);
    const inflight = new Map();
    const send = message => { if (!socket.destroyed) socket.write(JSON.stringify({ jsonrpc: '2.0', ...message }) + '\n'); };
    socket.on('close', () => {
      sockets.delete(socket);
      for (const controller of inflight.values()) controller.abort(new Error('MCP client disconnected'));
    });
    // A client that exits mid-reply (EPIPE) ends in 'close' above. readline
    // re-emits its input's errors on the interface, so both get a listener.
    socket.on('error', () => {});
    readline.createInterface({ input: socket }).on('error', () => {}).on('line', async line => {
      if (!line.trim()) return;
      let message;
      try { message = JSON.parse(line); } catch { return send({ id: null, error: { code: -32700, message: 'Parse error' } }); }
      const { id, method, params = {} } = message;
      if (id === undefined) {
        if (method === 'notifications/cancelled') inflight.get(params.requestId)?.abort(new Error('Cancelled by harness'));
        return;
      }
      if (method === 'initialize') {
        return send({ id, result: { protocolVersion: params.protocolVersion, capabilities: { tools: { listChanged: false } }, serverInfo: { name: 'mcbench', version: '1' } } });
      }
      if (method === 'ping') return send({ id, result: {} });
      if (method === 'tools/list') {
        const tools = host.tools.map(({ function: f }) => ({ name: f.name, description: f.description, inputSchema: f.parameters }));
        host.listedTools = tools.map(t => t.name);
        return send({ id, result: { tools } });
      }
      if (method === 'tools/call') {
        const controller = new AbortController();
        inflight.set(id, controller);
        try {
          const content = await host.call(params.name, params.arguments ?? {}, { signal: controller.signal, callId: params._meta?.callId ?? String(id), toolUseId: params._meta?.['claudecode/toolUseId'] });
          send({ id, result: { content, isError: false } });
        } catch (error) {
          send({ id, error: { code: -32603, message: error.message } });
        } finally { inflight.delete(id); }
        return;
      }
      send({ id, error: { code: -32601, message: `Method not found: ${method}` } });
    });
  });
  server.listen(socketPath);
  await once(server, 'listening');
  // Stops serving once the harness has exited, including any connection a
  // killed harness left behind.
  server.shutdown = () => { for (const socket of sockets) socket.destroy(); return new Promise(resolve => server.close(resolve)); };
  return server;
}

module.exports = { serveMcp, BRIDGE };
