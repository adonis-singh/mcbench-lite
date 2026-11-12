'use strict';

const net = require('net');

const AUTH = 3;
const AUTH_RESPONSE = 2;
const EXEC = 2;
const RESPONSE = 0;
const MAX_PACKET_LENGTH = 8192;

class RconError extends Error {}
class RconTimeout extends Error {}

function encodePacket(id, type, body) {
  const bodyBytes = Buffer.from(body, 'utf8');
  const packet = Buffer.alloc(4 + 4 + 4 + bodyBytes.length + 2);
  packet.writeInt32LE(packet.length - 4, 0);
  packet.writeInt32LE(id, 4);
  packet.writeInt32LE(type, 8);
  bodyBytes.copy(packet, 12);
  return packet;
}

class PacketDecoder {
  constructor() {
    this.buffer = Buffer.alloc(0);
  }

  push(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    const packets = [];
    while (this.buffer.length >= 4) {
      const length = this.buffer.readInt32LE(0);
      if (length < 10 || length > MAX_PACKET_LENGTH) {
        throw new RconError(`Malformed RCON packet length ${length}`);
      }
      if (this.buffer.length < length + 4) break;
      if (this.buffer[length + 2] !== 0 || this.buffer[length + 3] !== 0) {
        throw new RconError('Malformed RCON packet terminator');
      }
      packets.push({
        id: this.buffer.readInt32LE(4),
        type: this.buffer.readInt32LE(8),
        body: this.buffer.subarray(12, length + 2).toString('utf8'),
      });
      this.buffer = this.buffer.subarray(length + 4);
    }
    return packets;
  }
}

class RconConnection {
  constructor(socket) {
    this.socket = socket;
    this.decoder = new PacketDecoder();
    this.pending = null;
    this.nextId = 1;
    this.closed = false;
    socket.on('data', (chunk) => this.onData(chunk));
    socket.on('error', (error) => this.fail(new RconError(String(error.message || error))));
    socket.on('close', () => this.fail(new RconError('RCON connection closed')));
  }

  static async connect(host, port, password, timeoutMs = 10_000) {
    const socket = net.createConnection({ host, port });
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        socket.destroy();
        reject(new RconTimeout(`RCON connection timed out after ${timeoutMs}ms`));
      }, timeoutMs);
      socket.once('connect', () => {
        clearTimeout(timer);
        resolve();
      });
      socket.once('error', (error) => {
        clearTimeout(timer);
        reject(new RconError(String(error.message || error)));
      });
    });
    const connection = new RconConnection(socket);
    await connection.authenticate(password, timeoutMs);
    return connection;
  }

  // RCON answers one command at a time, so callers queue: a background
  // `op @a` sweep waits for the import's next command instead of failing it.
  exec(command, timeoutMs = 30_000) {
    const run = (this.queue || Promise.resolve()).then(() => this.send(command, timeoutMs));
    this.queue = run.catch(() => {});
    return run;
  }

  send(command, timeoutMs) {
    return new Promise((resolve, reject) => {
      if (!this.begin(reject)) return;
      const requestId = this.nextId++;
      const sentinelId = this.nextId++;
      this.pending = {
        kind: 'exec',
        requestId,
        sentinelId,
        sentinelSent: false,
        chunks: [],
        resolve,
        reject,
        timer: setTimeout(() => {
          this.fail(new RconTimeout(`RCON command timed out after ${timeoutMs}ms: ${command}`));
        }, timeoutMs),
      };
      this.socket.write(encodePacket(requestId, EXEC, command));
    });
  }

  authenticate(password, timeoutMs) {
    return new Promise((resolve, reject) => {
      if (!this.begin(reject)) return;
      const requestId = this.nextId++;
      this.pending = {
        kind: 'auth',
        requestId,
        resolve,
        reject,
        timer: setTimeout(() => {
          this.fail(new RconTimeout(`RCON authentication timed out after ${timeoutMs}ms`));
        }, timeoutMs),
      };
      this.socket.write(encodePacket(requestId, AUTH, password));
    });
  }

  begin(reject) {
    if (this.closed) {
      reject(new RconError('RCON connection is closed'));
      return false;
    }
    return true;
  }

  onData(chunk) {
    let packets;
    try {
      packets = this.decoder.push(chunk);
    } catch (error) {
      this.fail(error);
      return;
    }
    for (const packet of packets) {
      const pending = this.pending;
      if (!pending) continue;
      if (pending.kind === 'auth') {
        if (packet.type === RESPONSE && packet.id === pending.requestId) continue;
        if (packet.type !== AUTH_RESPONSE) {
          this.fail(new RconError(`Unexpected RCON packet type ${packet.type} during authentication`));
          return;
        }
        if (packet.id === -1) {
          this.fail(new RconError('RCON authentication failed'));
          return;
        }
        if (packet.id !== pending.requestId) {
          this.fail(new RconError(`RCON authentication reply for unknown id ${packet.id}`));
          return;
        }
        this.settle(pending).resolve();
        continue;
      }

      if (packet.type === RESPONSE && packet.id === pending.requestId) {
        pending.chunks.push(packet.body);
        if (!pending.sentinelSent) {
          pending.sentinelSent = true;
          this.socket.write(encodePacket(pending.sentinelId, EXEC, ''));
        }
        continue;
      }
      if (packet.type === RESPONSE && packet.id === pending.sentinelId) {
        this.settle(pending).resolve(pending.chunks.join(''));
        continue;
      }
      this.fail(new RconError(`Unexpected RCON packet id=${packet.id} type=${packet.type}`));
      return;
    }
  }

  settle(pending) {
    clearTimeout(pending.timer);
    this.pending = null;
    return pending;
  }

  fail(error) {
    if (this.pending) {
      const pending = this.pending;
      clearTimeout(pending.timer);
      this.pending = null;
      pending.reject(error);
    }
    if (!this.closed) this.close();
  }

  close() {
    this.closed = true;
    if (this.socket) this.socket.destroy();
    this.socket = null;
  }
}

module.exports = {
  RconConnection,
  RconTimeout,
  PacketDecoder,
  encodePacket,
};
