'use strict';

// Stands in for a build program that fully escaped node:vm. The test runs this
// function's source with -e in the build process's main realm, with its event
// loop, under the exact launch configuration of server/build-sandbox.js; it
// prints what it could reach. It also tries to read the files in `paths.read`,
// list the folders in `paths.list` (keys, the app's settings, other chats'
// worlds) and write the files in `paths.write`.
module.exports = function probe(canary, writeTarget, port, parentPid, paths) {
  const results = {};
  const attempt = (name, fn) => {
    try { results[name] = `allowed:${String(fn()).slice(0, 80)}`; } catch (error) { results[name] = `denied:${error.code || error.message}`; }
  };
  const fs = process.getBuiltinModule('fs');
  const net = process.getBuiltinModule('net');

  attempt('readCanary', () => fs.readFileSync(canary, 'utf8'));
  attempt('readPasswd', () => fs.readFileSync('/etc/passwd', 'utf8'));
  attempt('readProcEnviron', () => fs.readFileSync('/proc/1/environ', 'utf8'));
  attempt('listRoot', () => fs.readdirSync('/').join(','));
  for (const file of paths.read) attempt(`read ${file}`, () => fs.readFileSync(file, 'utf8'));
  for (const dir of paths.list) attempt(`list ${dir}`, () => fs.readdirSync(dir).join(','));
  for (const file of paths.write) attempt(`write ${file}`, () => fs.writeFileSync(file, 'escaped'));
  attempt('writeFile', () => fs.writeFileSync(writeTarget, 'escaped'));
  attempt('requireOutside', () => require(canary));
  attempt('spawn', () => process.getBuiltinModule('child_process').execFileSync('/usr/bin/id').toString());
  attempt('worker', () => new (process.getBuiltinModule('worker_threads').Worker)('1', { eval: true }));
  attempt('addon', () => process.dlopen({ exports: {} }, '/usr/lib/libc.so.6'));
  attempt('binding', () => Object.keys(process.binding('fs')).length);
  attempt('inspector', () => { process.getBuiltinModule('inspector').open(0); return 'open'; });
  // node:sqlite opens files outside the permission model; it must not exist.
  attempt('sqlite', () => {
    const sqlite = process.getBuiltinModule('node:sqlite');
    if (!sqlite) throw new Error('unavailable');
    return new sqlite.DatabaseSync(`${canary}.db`, { readOnly: true }).prepare('select v from secret').get().v;
  });
  attempt('signalParent', () => process.kill(Number(parentPid), 0));
  attempt('pid', () => process.pid);
  attempt('env', () => {
    // bwrap always sets PWD, to the / the process starts in.
    const keys = Object.keys(process.env).filter((key) => key !== 'MCBENCH_MC_Y_OFFSET' && !(key === 'PWD' && process.env.PWD === '/'));
    if (keys.length) throw new Error(keys.join(','));
    return 'empty';
  });

  const settle = (name, promise) => promise.then(
    (value) => { results[name] = `allowed:${value}`; },
    (error) => { results[name] = `denied:${error.code || error.cause?.code || error.message}`; },
  );
  const connect = (host, to) => new Promise((resolve, reject) => {
    net.connect(to, host, () => resolve('connected')).on('error', reject);
  });
  Promise.all([
    settle('tcpLoopback', connect('127.0.0.1', Number(port))),
    settle('tcpInternet', connect('1.1.1.1', 80)),
    settle('dns', new Promise((resolve, reject) => {
      process.getBuiltinModule('dns').lookup('example.com', (error, address) => (error ? reject(error) : resolve(address)));
    })),
    settle('fetch', fetch(`http://127.0.0.1:${port}/`).then((response) => response.status)),
    settle('listen', new Promise((resolve, reject) => {
      const server = net.createServer();
      server.on('error', reject);
      server.listen(0, () => { resolve(server.address().port); server.close(); });
    })),
  ]).then(() => {
    process.stdout.write(JSON.stringify(results));
    process.exit(0);
  });
};
