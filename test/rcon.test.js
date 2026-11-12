'use strict';

const assert = require('node:assert');
const net = require('net');
const { test } = require('node:test');

const {
  PacketDecoder,
  RconConnection,
  RconTimeout,
  encodePacket,
} = require('../server/rcon');

function scriptedServer(script) {
  const decoders = new WeakMap();
  const server = net.createServer((socket) => {
    const decoder = new PacketDecoder();
    decoders.set(socket, decoder);
    socket.on('data', (chunk) => {
      for (const packet of decoder.push(chunk)) {
        for (const reply of script(packet)) socket.write(reply);
      }
    });
  });
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => resolve({
      port: server.address().port,
      close: () => new Promise((done) => server.close(done)),
    }));
  });
}

test('RCON authenticates and joins multipart command replies', async () => {
  const server = await scriptedServer((packet) => {
    if (packet.type === 3) return [encodePacket(packet.id, 0, ''), encodePacket(packet.id, 2, '')];
    if (packet.body === '') return [encodePacket(packet.id, 0, '')];
    return [encodePacket(packet.id, 0, 'part one, '), encodePacket(packet.id, 0, 'part two')];
  });
  try {
    const connection = await RconConnection.connect('127.0.0.1', server.port, 'secret');
    assert.strictEqual(await connection.exec('ping'), 'part one, part two');
    connection.close();
  } finally {
    await server.close();
  }
});

test('RCON commands sent together run one after another in order', async () => {
  const server = await scriptedServer((packet) => {
    if (packet.type === 3) return [encodePacket(packet.id, 0, ''), encodePacket(packet.id, 2, '')];
    if (packet.body === '') return [encodePacket(packet.id, 0, '')];
    return [encodePacket(packet.id, 0, `ran ${packet.body}`)];
  });
  try {
    const connection = await RconConnection.connect('127.0.0.1', server.port, 'secret');
    const replies = await Promise.all(['op @a', 'tick freeze', 'save-all flush'].map((command) => connection.exec(command)));
    assert.deepStrictEqual(replies, ['ran op @a', 'ran tick freeze', 'ran save-all flush']);
    connection.close();
  } finally {
    await server.close();
  }
});

test('RCON packet decoder preserves partial frames', () => {
  const wire = encodePacket(7, 0, 'hello');
  const decoder = new PacketDecoder();
  assert.deepStrictEqual(decoder.push(wire.subarray(0, 5)), []);
  assert.deepStrictEqual(decoder.push(wire.subarray(5)), [{ id: 7, type: 0, body: 'hello' }]);
});

test('RCON authentication timeout never leaks the password', async () => {
  const server = await scriptedServer(() => []);
  try {
    const error = await RconConnection.connect('127.0.0.1', server.port, 'hunter2', 20).then(
      () => null,
      (value) => value,
    );
    assert.ok(error instanceof RconTimeout);
    assert.doesNotMatch(error.message, /hunter2/);
  } finally {
    await server.close();
  }
});
