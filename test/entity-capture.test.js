'use strict';

const assert = require('node:assert/strict');
const path = require('node:path');
const { test } = require('node:test');
const { bakedFile } = require('./helpers/community');

// What the bake captured of Minecraft's entity renderers
// (McbenchEntities.java, mcbench-entities.json), as both views draw it
// (web/entity-draw.mjs), against values worked out here from Minecraft
// 1.21.10's own definitions, not read from the capture:
//
// A minecart is MinecartModel.createBodyLayer's five cuboids: bottom
// addBox(-10, -8, -1, 20, 16, 2) at PartPose.offsetAndRotation(0, 4, 0,
// PI / 2, 0, 0), front at (-9, 4, 0, 0, 3 PI / 2, 0), back at (9, 4, 0, 0,
// PI / 2, 0), left at (0, 4, -7, 0, PI, 0) and right at offset(0, 4, 7),
// under AbstractMinecartRenderer.submit's pose: its offset by id (the seed
// id * 493286711, then seed * seed * 4392167121 + seed * 98761, three bits
// each from bit 16, 20 and 24, ((bits + 0.5) / 8 - 0.5) * 0.004), a lift of
// 0.375, a turn of 180 - yaw about y, and scale(-1, -1, 1).
//
// An armor stand's head is ArmorStandModel's addBox(-1, -7, -1, 2, 7, 2) at
// PartPose.offset(0, 1, 0), turned by setupAnim to the head pose in degrees
// (xRot = Mth.DEG_TO_RAD * headPose.x() ...), under LivingEntityRenderer's
// pose: a turn of 180 - body yaw (ArmorStandRenderer.setupRotations),
// scale(-1, -1, 1) and a drop of 1.501.
//
// A stick dropped is item/generated's ground display (translation 0, 2, 0
// sixteenths, scale 0.5) then LayerRenderState.submit's translate(-0.5,
// -0.5, -0.5); ItemModelGenerator's front and back faces span the whole
// sprite half a pixel either side of the block's middle (7.5 and 8.5
// sixteenths).
//
// An orb's block light is seven brighter (ExperienceOrbRenderer
// .getBlockLightLevel: clamp(light + 7, 0, 15)); a glow item frame's at
// least 5 (ItemFrameRenderer), and the item in it full bright
// (LightTexture.FULL_BRIGHT, 0xF000F0). Primed TNT flashes white
// (OverlayTexture.pack(OverlayTexture.u(1), 10)) while (int) (fuse + 1) / 5
// is even (TntRenderer, at partial tick 0).
//
// Read from the data directory's bake (MCBENCH_DATA_DIR); skipped without one.
const BAKED = bakedFile(path.join('web', 'mcbench-entities.json'));
const skip = BAKED ? false : 'no bake in the data directory';

let data;
async function capture() {
  if (!data) {
    const { EntityData } = await import('../web/entity-draw.mjs');
    data = new EntityData(JSON.parse(BAKED.toString('utf8')));
  }
  return data;
}

const near = (actual, expected, message) => {
  assert.equal(actual.length, expected.length, message);
  actual.forEach((value, i) => assert.ok(Math.abs(value - expected[i]) < 1e-5, `${message}: ${actual} is not ${expected}`));
};

// Java's long arithmetic, as AbstractMinecartRenderer seeds a cart.
function cartOffset(id) {
  let seed = BigInt.asIntN(64, BigInt(id) * 493286711n);
  seed = BigInt.asIntN(64, seed * seed * 4392167121n + seed * 98761n);
  return [16, 20, 24].map((shift) => ((Number((seed >> BigInt(shift)) & 7n) + 0.5) / 8 - 0.5) * 0.004);
}

test('a minecart is MinecartModel\'s cuboids under AbstractMinecartRenderer\'s pose', { skip }, async () => {
  const data = await capture();
  const kind = data.kind('minecraft:minecart');
  const [block, model] = kind.variants[''].draws;
  assert.equal(block.kind, 'block');
  assert.equal(block.block, 'entity', 'the block it shows is the entity\'s');
  const parts = Object.fromEntries(model.parts.map((part) => [part.path, part.pose]));
  const PI = Math.PI;
  near(parts['/bottom'], [0, 4, 0, PI / 2, 0, 0, 1, 1, 1], 'bottom');
  near(parts['/front'], [-9, 4, 0, 0, (3 * PI) / 2, 0, 1, 1, 1], 'front');
  near(parts['/back'], [9, 4, 0, 0, PI / 2, 0, 1, 1, 1], 'back');
  near(parts['/left'], [0, 4, -7, 0, PI, 0, 1, 1, 1], 'left');
  near(parts['/right'], [0, 4, 7, 0, 0, 0, 1, 1, 1], 'right');
  // The bottom's cuboid, in its part's space (sixteenths of a block).
  const bottom = model.parts.find((part) => part.path === '/bottom');
  const corners = data.mesh(bottom.mesh).flatMap((quad) => [0, 1, 2, 3].map((k) => Array.from(data.shapes.subarray(quad.shape * 20 + k * 3, quad.shape * 20 + k * 3 + 3))));
  near([0, 1, 2].map((axis) => Math.min(...corners.map((c) => c[axis]))), [-10 / 16, -8 / 16, -1 / 16], 'the bottom\'s lowest corner');
  near([0, 1, 2].map((axis) => Math.max(...corners.map((c) => c[axis]))), [10 / 16, 8 / 16, 1 / 16], 'its highest');
  // Turned 180 about y, then scale(-1, -1, 1): x kept, y and z flipped.
  const [ox, oy, oz] = cartOffset(data.json.id);
  near(data.poses[model.pose], [1, 0, 0, 0, 0, -1, 0, 0, 0, 0, -1, 0, ox, oy + 0.375, oz, 1], 'the model\'s pose');
  const yaw = model.motions.find((motion) => motion.value === 'yaw');
  near([...yaw.axis, yaw.rate], [0, 1, 0, -1], 'a turn of -1 degree a degree of yaw about y');
  // Another id, another offset (the capture's own drawing of id 7).
  const [ax, ay, az] = cartOffset(kind.check.id);
  near(kind.check.pose.slice(12, 15), [ax, ay + 0.375, az], 'id 7\'s pose');
  const page = await import('../web/entity-draw.mjs');
  near(page.minecartOffset(kind.check.id), cartOffset(kind.check.id).map(Math.fround), 'the page\'s offset');
});

test('an armor stand\'s head is ArmorStandModel\'s head, turned by the head pose, under LivingEntityRenderer\'s pose', { skip }, async () => {
  const data = await capture();
  const [model] = data.kind('minecraft:armor_stand').variants['false,true,false,false'].draws;
  near(data.poses[model.pose], [1, 0, 0, 0, 0, -1, 0, 0, 0, 0, -1, 0, 0, 1.501, 0, 1], 'turned 180, scale(-1, -1, 1), dropped 1.501');
  const head = model.parts.find((part) => part.path === '/head');
  near(head.pose, [0, 1, 0, 0, 0, 0, 1, 1, 1], 'offset(0, 1, 0)');
  const degree = Math.PI / 180;
  near(head.deps['Head.x'], [0, 0, 0, degree, 0, 0, 0, 0, 0], 'xRot a radian\'s degree a degree of Head.x');
  near(head.deps['Head.y'], [0, 0, 0, 0, degree, 0, 0, 0, 0], 'yRot by Head.y');
  const corners = data.mesh(head.mesh).flatMap((quad) => [0, 1, 2, 3].map((k) => Array.from(data.shapes.subarray(quad.shape * 20 + k * 3, quad.shape * 20 + k * 3 + 3))));
  near([0, 1, 2].map((axis) => Math.min(...corners.map((c) => c[axis]))), [-1 / 16, -7 / 16, -1 / 16], 'addBox(-1, -7, -1, ...)');
  near([0, 1, 2].map((axis) => Math.max(...corners.map((c) => c[axis]))), [1 / 16, 0, 1 / 16], '... 2, 7, 2');
});

test('a dropped stick is ItemModelGenerator\'s quads under item/generated\'s ground display', { skip }, async () => {
  const data = await capture();
  const [ground] = data.items['minecraft:stick'].ground;
  near(data.poses[ground.pose], [0.5, 0, 0, 0, 0, 0.5, 0, 0, 0, 0, 0.5, 0, -0.25, -0.125, -0.25, 1], 'T(0, 2/16, 0) S(0.5) T(-0.5, -0.5, -0.5)');
  const quads = data.mesh(ground.mesh).map((quad) => Array.from(data.shapes.subarray(quad.shape * 20, quad.shape * 20 + 20)));
  for (const z of [7.5 / 16, 8.5 / 16]) {
    const face = quads.find((shape) => [2, 5, 8, 11].every((i) => Math.abs(shape[i] - z) < 1e-6));
    assert.ok(face, `a face at z ${z}`);
    near([0, 1].map((axis) => Math.min(face[axis], face[3 + axis], face[6 + axis], face[9 + axis])), [0, 0], 'over the whole block');
    near([0, 1].map((axis) => Math.max(face[axis], face[3 + axis], face[6 + axis], face[9 + axis])), [1, 1], 'over the whole block');
  }
  assert.ok(quads.length > 2, 'and the sides of its pixels\' edges');
  assert.ok(data.mesh(ground.mesh).every((quad) => data.textures[quad.texture] === 'minecraft:item/stick'));
});

test('kinds are lit as their renderers light them, and primed TNT flashes as TntRenderer flashes it', { skip }, async () => {
  const data = await capture();
  const levels = Array.from({ length: 16 }, (_, i) => i);
  assert.deepEqual(data.kind('minecraft:experience_orb').light.block, levels.map((l) => Math.min(l + 7, 15)));
  assert.deepEqual(data.kind('minecraft:glow_item_frame').light.block, levels.map((l) => Math.max(l, 5)));
  assert.deepEqual(data.kind('minecraft:minecart').light.block, levels);
  assert.deepEqual(data.kind('minecraft:minecart').light.sky, levels);
  const glow = data.kind('minecraft:glow_item_frame').variants['south,false,0'].draws;
  assert.deepEqual(glow.map((draw) => draw.light), ['entity', 0xF000F0], 'the frame in its light, its item full bright');
  for (const [fuse, variant] of Object.entries(data.kind('minecraft:tnt').variants)) {
    const flash = Math.trunc((Number(fuse) + 1) / 5) % 2 === 0;
    assert.equal(variant.draws[0].overlay, flash ? (10 << 16 | 15) : (10 << 16), `fuse ${fuse}`);
  }
});
