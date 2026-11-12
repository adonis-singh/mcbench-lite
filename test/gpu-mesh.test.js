'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');

test('uploaded mesh releases CPU vertices and keeps draw counts through replacement and removal', async () => {
  // Exercise the helper shipped in the single viewer module: the isolated
  // benchmark server serves that module without a build step.
  const source = require('node:fs').readFileSync(require('node:path').join(__dirname, '../web/viewer.js'), 'utf8');
  const helper = source.slice(source.indexOf('function retainGpuMesh('), source.indexOf('const SEE_THROUGH'));
  const retainGpuMesh = require('node:vm').runInThisContext(`(${helper.trim()})`);
  const uploads = [];
  const mesh = {
    quads: [{ vertex: 'opaque' }, { vertex: 'transparent' }],
    lines: [{ vertex: 'line' }],
    rebuild() {
      uploads.push({ quads: [...this.quads], lines: [...this.lines] });
      this.gpuBuffer = uploads.length;
      return this;
    },
    clear() { this.quads = []; this.lines = []; return this; },
  };
  retainGpuMesh(mesh);
  assert.equal(mesh.rebuild(), mesh);
  assert.equal(uploads[0].quads.length, 2);
  assert.deepEqual(mesh.quads, []);
  assert.deepEqual(mesh.lines, []);
  assert.equal(mesh.gpuBuffer, 1);
  assert.equal(mesh.isEmpty(), false);
  assert.equal(mesh.quadIndices(), 12);
  assert.equal(mesh.quadVertices(), 8);
  assert.equal(mesh.lineVertices(), 2);

  mesh.clear();
  mesh.quads.push({ vertex: 'replacement' });
  mesh.rebuild();
  assert.equal(mesh.quadIndices(), 6);
  assert.equal(mesh.lineVertices(), 0);
  assert.deepEqual(uploads[1].quads, [{ vertex: 'replacement' }]);
  mesh.clear();
  mesh.rebuild();
  assert.equal(mesh.isEmpty(), true);
  assert.equal(mesh.quadIndices(), 0);
});
