'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const { test } = require('node:test');
const { MODULE } = require('../scripts/build-renderer');
const { bakedPack } = require('./helpers/community');

// What the bake captured of Minecraft's block entity renderers
// (McbenchBlockEntities.java), as the pack holds it and the live viewer and
// see_build draw it (Mesher.lone's entity faces), against values worked out
// here from Minecraft 1.21.10's own definitions, not read from the capture:
//
// A chest is ChestModel.createSingleBodyLayer's three cuboids, bottom
// addBox(1, 0, 1, 14, 10, 14), lid addBox(1, 0, 0, 14, 5, 14) at
// PartPose.offset(0, 9, 1) and lock addBox(7, -2, 14, 2, 4, 1) at the same
// offset, under ChestRenderer's pose: about the block's centre,
// rotateY(-facing.toYRot()), so the lock is on the facing side.
//
// A head standing on the ground faces where SkullBlock.getStateForPlacement
// turns it: rotation = RotationSegment.convertToSegment(the placer's yaw)
// (no half turn), sixteen steps of 22.5 degrees, and the head faces the
// placer, who looks along the yaw (0 south, 90 west): its face points along
// (sin yaw, 0, -cos yaw). Its face is the north face of SkullModel's head
// cuboid addBox(-4, -8, -4, 8, 8, 8) at texOffs(0, 0): texels 8 to 16
// across and 8 to 16 down.
//
// A banner's flag is drawn twice, the banner base untinted and the base
// pattern layer in the banner's DyeColor.getTextureDiffuseColor: red is
// 0xB02E26.
//
// Read from the data directory's bake (MCBENCH_DATA_DIR); skipped without one.
const PACK = bakedPack();

let mesher;
async function entityQuads(name) {
  if (!mesher) {
    const { Mesher } = await import('../web/mesher.mjs');
    mesher = await Mesher.create(fs.readFileSync(MODULE), PACK);
  }
  const { entities } = mesher.lone(name, 0);
  const quads = [];
  for (let f = 0; f < entities.count; f += 2) {
    const corners = [[f, 0], [f, 1], [f, 2], [f + 1, 2]].map(([face, k]) => Array.from(entities.position.slice(face * 9 + k * 3, face * 9 + k * 3 + 3)));
    const uv = [[f, 0], [f, 1], [f, 2], [f + 1, 2]].map(([face, k]) => Array.from(entities.uv.slice(face * 6 + k * 2, face * 6 + k * 2 + 2)));
    quads.push({ corners, uv, color: Array.from(entities.color.slice(f * 3, f * 3 + 3)) });
  }
  return quads;
}

const near = (a, b) => Math.abs(a - b) < 1e-5;
const normal = ([a, b, c]) => {
  const u = b.map((v, i) => v - a[i]);
  const w = c.map((v, i) => v - a[i]);
  const n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
  const length = Math.hypot(...n);
  return n.map((v) => v / length);
};

test('a chest is ChestModel\'s three cuboids under ChestRenderer\'s turn, its lock on the side it faces', { skip: !PACK }, async () => {
  for (const [facing, lockSide] of [['north', [0.5, 10.5 / 16, 0]], ['south', [0.5, 10.5 / 16, 1]], ['east', [1, 10.5 / 16, 0.5]], ['west', [0, 10.5 / 16, 0.5]]]) {
    const quads = await entityQuads(`minecraft:chest[facing=${facing},type=single,waterlogged=false]`);
    assert.equal(quads.length, 18, 'three cuboids of six faces');
    const all = quads.flatMap((quad) => quad.corners);
    const min = [0, 1, 2].map((axis) => Math.min(...all.map((p) => p[axis])));
    const max = [0, 1, 2].map((axis) => Math.max(...all.map((p) => p[axis])));
    // Bottom and lid span 1 to 15 across and 0 to 14 up; the lock reaches the facing side.
    assert.ok(near(min[1], 0) && near(max[1], 14 / 16), `${facing}: heights ${min[1]}..${max[1]}`);
    const lock = (axis) => (lockSide[axis] === 0.5 ? null : lockSide[axis]);
    for (const axis of [0, 2]) {
      const side = lock(axis);
      if (side === null) assert.ok(near(min[axis], 1 / 16) && near(max[axis], 15 / 16), `${facing}: axis ${axis} from ${min[axis]} to ${max[axis]}`);
      else assert.ok(side === 0 ? near(min[axis], 0) && near(max[axis], 15 / 16) : near(min[axis], 1 / 16) && near(max[axis], 1), `${facing}: the lock on axis ${axis} at ${side}, the chest from ${min[axis]} to ${max[axis]}`);
    }
  }
});

test('a standing head faces the placer SkullBlock turns it toward, by RotationSegment\'s sixteen steps', { skip: !PACK }, async () => {
  for (const [kind, height] of [['minecraft:skeleton_skull', 32], ['minecraft:zombie_head', 64]]) {
    for (const rotation of [0, 4, 8, 12, 2]) {
      const quads = await entityQuads(`${kind}[powered=false,rotation=${rotation}]`);
      const face = quads.filter((quad) => {
        const us = quad.uv.map(([u]) => u);
        const vs = quad.uv.map(([, v]) => v);
        return near(Math.min(...us), 8 / 64) && near(Math.max(...us), 16 / 64) && near(Math.min(...vs), 8 / height) && near(Math.max(...vs), 16 / height);
      });
      assert.equal(face.length, 1, `${kind} rotation ${rotation}: one face`);
      const yaw = rotation * 22.5 * Math.PI / 180;
      const expected = [Math.sin(yaw), 0, -Math.cos(yaw)];
      const got = normal(face[0].corners);
      assert.ok(got.every((v, i) => Math.abs(v - expected[i]) < 1e-4), `${kind} rotation ${rotation}: its face points ${got.map((v) => v.toFixed(3))}, not ${expected.map((v) => v.toFixed(3))}`);
    }
  }
});

test('a banner\'s flag is the base untinted and its base layer in the banner\'s dye colour', { skip: !PACK }, async () => {
  const colors = new Set((await entityQuads('minecraft:red_banner[rotation=0]')).map((quad) => quad.color.map((v) => Math.round(v * 255)).join(',')));
  assert.deepEqual([...colors].sort(), ['176,46,38', '255,255,255']);
});
