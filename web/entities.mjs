// The entity layer of the viewer: draws the live world's entities, which
// BlueMap's own renderer leaves out, in BlueMap's web app (three.js), as
// Minecraft's own entity renderers draw them (entity-draw.mjs, over what the
// bake captured of them, server/McbenchEntities.java), textured from the
// bake's entity atlas and lit by the light the viewer's mesher holds at each
// entity. The app streams the whole entity list each frame (`set`); the
// layer keeps one object per entity id, moves it between frames and adds and
// removes objects as ids come and go.
import { EntityData, appearance, frame, geometry, lightCells, loneQuads, variantOf } from './entity-draw.mjs';

// An entity's place between frames. Each frame gives every entity's
// position and angles; the displayed values glide from where they are to the
// new ones over the time the last frame took, and jump when the entity moved
// further than `snapDistance` blocks (a teleport, not a motion).
export class EntityTracker {
  constructor({ snapDistance = 4, defaultInterval = 50, minInterval = 10, maxInterval = 250 } = {}) {
    this.snapDistance = snapDistance;
    this.defaultInterval = defaultInterval;
    this.minInterval = minInterval;
    this.maxInterval = maxInterval;
    this.entities = new Map();
    this.lastFrame = null;
  }

  // Takes a frame's whole list at time `now` (ms). Returns the ids that
  // appeared and the ids that are gone.
  set(list, now) {
    const seen = new Set();
    for (const entity of list) {
      if (seen.has(entity.id)) throw new Error(`Entity ${entity.id} is in the frame twice`);
      seen.add(entity.id);
      if (!Array.isArray(entity.pos) || entity.pos.length !== 3 || !entity.pos.every(Number.isFinite)) throw new Error(`Entity ${entity.id} has no position`);
      if (!Number.isFinite(entity.yaw) || !Number.isFinite(entity.pitch)) throw new Error(`Entity ${entity.id} has no angles`);
    }
    const duration = this.lastFrame === null ? this.defaultInterval
      : Math.min(Math.max(now - this.lastFrame, this.minInterval), this.maxInterval);
    this.lastFrame = now;
    const added = [];
    const removed = [];
    for (const id of this.entities.keys()) if (!seen.has(id)) removed.push(id);
    for (const id of removed) this.entities.delete(id);
    for (const entity of list) {
      const target = { pos: entity.pos, yaw: entity.yaw, pitch: entity.pitch };
      const known = this.entities.get(entity.id);
      if (!known) {
        added.push(entity.id);
        this.entities.set(entity.id, { entity, from: target, to: target, start: now, duration });
        continue;
      }
      const shown = this.sampleOne(known, now);
      const jumped = Math.hypot(...target.pos.map((v, i) => v - known.to.pos[i])) > this.snapDistance;
      known.entity = entity;
      known.from = jumped ? target : shown;
      known.to = target;
      known.start = now;
      known.duration = duration;
    }
    return { added, removed };
  }

  sampleOne(tween, now) {
    const t = Math.min(Math.max((now - tween.start) / tween.duration, 0), 1);
    const turn = (from, to) => from + (((to - from + 540) % 360) - 180) * t;
    return {
      pos: tween.from.pos.map((v, i) => v + (tween.to.pos[i] - v) * t),
      yaw: turn(tween.from.yaw, tween.to.yaw),
      pitch: tween.from.pitch + (tween.to.pitch - tween.from.pitch) * t,
    };
  }

  // Every entity as it is drawn at `now`.
  sample(now) {
    return [...this.entities.entries()].map(([id, tween]) => ({ id, entity: tween.entity, ...this.sampleOne(tween, now) }));
  }

  // Whether any entity is still gliding at `now`. A frame that keeps an
  // entity where it stands starts a glide from where it is shown to its
  // target, which can differ from it only past the precision the GPU draws
  // with (32-bit floats): such a glide does not move what is drawn.
  moving(now) {
    const drawn = (state) => [...state.pos, state.yaw, state.pitch].map(Math.fround);
    for (const tween of this.entities.values()) {
      if (now - tween.start >= tween.duration) continue;
      const from = drawn(this.sampleOne(tween, tween.start));
      const to = drawn(this.sampleOne(tween, tween.start + tween.duration));
      if (from.some((value, i) => value !== to[i])) return true;
    }
    return false;
  }
}

// The capture, its atlas and the map's texture names (which a block an
// entity draws names its textures by), fetched from the bake's folder.
export async function loadEntityAssets(baseUrl) {
  const fetched = async (name) => {
    const response = await fetch(new URL(name, baseUrl));
    if (!response.ok) throw new Error(`${name} is unavailable (HTTP ${response.status})`);
    return response.json();
  };
  const [json, textures, atlas] = await Promise.all([fetched('mcbench-entities.json'), fetched('maps/live/textures.json'), loadImage(new URL('mcbench-entities.png', baseUrl))]);
  return { data: new EntityData(json), textures: textures.map((texture) => texture.resourcePath), atlas };
}

function loadImage(url) {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error(`The entity atlas is unavailable: ${url}`));
    image.src = url;
  });
}

function columnMajor(matrix, target) {
  for (let i = 0; i < 16; i += 1) target.elements[i] = matrix[i];
}

const cellKey = (cell) => cell.join(',');

// A render type drawn blended, after the solid ones (its pipeline's blend).
const blended = (layer) => /translucent/.test(layer);

export class EntityLayer {
  // BlueMap: the app's `BlueMap` module namespace; app: window.bluemap;
  // assets: loadEntityAssets's; mesher: the tile sync's MesherClient, which
  // answers the light of the world and the models of blocks.
  constructor({ BlueMap, app, assets, mesher }) {
    this.BlueMap = BlueMap;
    this.Three = BlueMap.Three;
    this.viewer = app.mapViewer;
    this.events = app.events;
    this.data = assets.data;
    this.textures = assets.textures;
    this.rects = assets.data.json.atlas.rects;
    this.atlasSize = [assets.data.json.atlas.width, assets.data.json.atlas.height];
    this.mesher = mesher;
    this.tracker = new EntityTracker();
    this.objects = new Map(); // id -> { key, object, entity }
    this.geometries = new Map(); // appearance -> BufferGeometry
    this.lights = new Map(); // cell -> [block, sky]
    this.blocks = new Map(); // block state -> its models' quads
    this.list = null; // the last frame's entity list, as JSON
    this.latest = []; // the last frame's entities
    this.asking = null; // the request for light and blocks in flight
    this.group = new this.Three.Group();
    this.group.name = 'mcbench-entities';
    this.viewer.markers.add(this.group);
    this.materials = this.createMaterials(assets.atlas);
    this.onFrame = () => this.update(performance.now());
    this.events.addEventListener('bluemapRenderFrame', this.onFrame);
  }

  // The map's own hires material: BlueMap's shaders and uniforms, so entities are
  // lit as the tiles are. Its vertex shader reads normals as world normals
  // (tiles are never moved); an entity's are turned with it. Its fragment
  // shader blends in the overlay a renderer draws with (OverlayTexture: a
  // primed TNT's white flash), which tiles never have.
  createMaterials(atlas) {
    const { Three, BlueMap, viewer } = this;
    const texture = new Three.Texture(atlas);
    texture.magFilter = Three.NearestFilter;
    texture.minFilter = Three.NearestFilter;
    texture.generateMipmaps = false;
    texture.wrapS = Three.ClampToEdgeWrapping;
    texture.wrapT = Three.ClampToEdgeWrapping;
    texture.flipY = false;
    texture.needsUpdate = true;
    const replace = (source, from, to, what) => {
      if (!source.includes(from)) throw new Error(`BlueMap's hires ${what} shader no longer reads ${from}`);
      return source.replace(from, to);
    };
    let vertex = replace(BlueMap.HIRES_VERTEX_SHADER, 'vNormal = normal;', 'vNormal = normalize(mat3(modelMatrix) * normal);', 'vertex');
    vertex = replace(vertex, 'attribute float ao;', 'attribute float ao;\nattribute vec4 overlay;\nvarying vec4 vOverlay;', 'vertex');
    vertex = replace(vertex, 'vColor = color;', 'vColor = color;\n    vOverlay = overlay;', 'vertex');
    let fragment = replace(BlueMap.HIRES_FRAGMENT_SHADER, 'varying vec3 vColor;', 'varying vec3 vColor;\nvarying vec4 vOverlay;', 'fragment');
    fragment = replace(fragment, 'color.rgb *= vColor.rgb;', 'color.rgb *= vColor.rgb;\n\tcolor.rgb = mix(vOverlay.rgb, color.rgb, vOverlay.a);', 'fragment');
    const make = (transparent) => new Three.ShaderMaterial({
      uniforms: {
        ...viewer.data.uniforms,
        textureImage: { type: 't', value: texture },
        animationFrameHeight: { value: 1 },
        animationFrameIndex: { value: 0 },
        animationInterpolationFrameIndex: { value: 0 },
        animationInterpolation: { value: 0 },
      },
      vertexShader: vertex,
      fragmentShader: fragment,
      transparent,
      depthWrite: !transparent,
      depthTest: true,
      vertexColors: true,
      side: Three.DoubleSide,
    });
    return { solid: make(false), blended: make(true) };
  }

  // The geometry of an entity's look, in its frame: a group of two meshes,
  // its solid and its blended render types' quads.
  build(entity) {
    const { Three } = this;
    const quads = geometry(this.data, entity, {
      light: (x, y, z) => this.lightAt(x, y, z),
      blockQuads: (state) => this.blocks.get(state),
    });
    const group = new Three.Group();
    group.matrixAutoUpdate = false;
    for (const [material, own] of [['solid', quads.filter((quad) => !blended(quad.layer))], ['blended', quads.filter((quad) => blended(quad.layer))]]) {
      if (own.length === 0) continue;
      const mesh = new Three.Mesh(this.buffers(own), this.materials[material]);
      mesh.raycast = () => {};
      mesh.matrixAutoUpdate = false;
      group.add(mesh);
    }
    return group;
  }

  // Quads as BlueMap's hires attributes: two triangles each, with its
  // texture's place in the atlas, its colour, its overlay and its light.
  buffers(quads) {
    const { Three } = this;
    const n = quads.length * 6;
    const position = new Float32Array(n * 3), normal = new Float32Array(n * 3), uv = new Float32Array(n * 2), color = new Float32Array(n * 3);
    const overlay = new Float32Array(n * 4), ao = new Float32Array(n).fill(1), sunlight = new Float32Array(n), blocklight = new Float32Array(n);
    const [W, H] = this.atlasSize;
    let v = 0;
    for (const quad of quads) {
      // Water's mask (a boat's) draws no colour: not drawn here.
      if (!quad.texture) continue;
      const rect = this.rects[quad.texture];
      if (!rect) throw new Error(`The entity atlas has no texture ${quad.texture}`);
      const c = quad.corners;
      const ex = c[3] - c[0], ey = c[4] - c[1], ez = c[5] - c[2], fx = c[6] - c[0], fy = c[7] - c[1], fz = c[8] - c[2];
      let nx = ey * fz - ez * fy, ny = ez * fx - ex * fz, nz = ex * fy - ey * fx;
      const length = Math.hypot(nx, ny, nz) || 1;
      nx /= length; ny /= length; nz /= length;
      for (const k of [0, 1, 2, 0, 2, 3]) {
        position.set([c[k * 3], c[k * 3 + 1], c[k * 3 + 2]], v * 3);
        normal.set([nx, ny, nz], v * 3);
        uv.set([(rect[0] + quad.uv[k * 2] * rect[2]) / W, (rect[1] + quad.uv[k * 2 + 1] * rect[3]) / H], v * 2);
        color.set(quad.color.slice(0, 3), v * 3);
        overlay.set(quad.overlay, v * 4);
        blocklight[v] = quad.light[0];
        sunlight[v] = quad.light[1];
        v += 1;
      }
    }
    const geometry = new Three.BufferGeometry();
    geometry.setAttribute('position', new Three.BufferAttribute(position.subarray(0, v * 3), 3));
    geometry.setAttribute('normal', new Three.BufferAttribute(normal.subarray(0, v * 3), 3));
    geometry.setAttribute('uv', new Three.BufferAttribute(uv.subarray(0, v * 2), 2));
    geometry.setAttribute('color', new Three.BufferAttribute(color.subarray(0, v * 3), 3));
    geometry.setAttribute('overlay', new Three.BufferAttribute(overlay.subarray(0, v * 4), 4));
    geometry.setAttribute('ao', new Three.BufferAttribute(ao.subarray(0, v), 1));
    geometry.setAttribute('sunlight', new Three.BufferAttribute(sunlight.subarray(0, v), 1));
    geometry.setAttribute('blocklight', new Three.BufferAttribute(blocklight.subarray(0, v), 1));
    geometry.computeBoundingSphere();
    return geometry;
  }

  lightAt(x, y, z) {
    const known = this.lights.get(`${x},${y},${z}`);
    if (!known) throw new Error(`No light was asked of the mesher at ${x}, ${y}, ${z}`);
    return known;
  }

  // The block states an entity's draws name: the block it shows.
  statesOf(entity) {
    const kind = this.data.kind(entity.type);
    if (entity.type === 'minecraft:item') return [];
    return variantOf(kind, entity).draws.some((draw) => draw.block === 'entity') ? [entity.block] : [];
  }

  // Takes a frame's whole entity list: the entities that appeared are made,
  // the ones that are gone removed, the rest keep their objects and glide to
  // their new places; an entity that looks different, or stands in other
  // light, is made again once the mesher has answered its light. A frame
  // whose list is the last one's (a still world sends one every tick) asks
  // for no frame of its own.
  set(entities, now = performance.now()) {
    const list = JSON.stringify(entities);
    const changed = list !== this.list;
    this.list = list;
    this.latest = entities;
    const { removed } = this.tracker.set(entities, now);
    for (const id of removed) {
      this.group.remove(this.objects.get(id).object);
      this.objects.delete(id);
    }
    if (changed) this.ask();
    this.update(now);
  }

  // Asks the mesher for the light at every entity's cells and the models
  // of the blocks they draw not yet known, then makes what looks different.
  // One request at a time; a list that came meanwhile is asked for next.
  ask() {
    if (this.asking) {
      this.again = true;
      return;
    }
    const entities = this.latest;
    // No entity asks nothing.
    if (entities.length === 0) {
      this.make(entities);
      return;
    }
    const cells = new Map();
    const states = new Set();
    for (const entity of entities) {
      for (const cell of lightCells(this.data, entity)) cells.set(cellKey(cell), cell);
      for (const state of this.statesOf(entity)) if (!this.blocks.has(state)) states.add(state);
    }
    this.asking = this.mesher.entities([...cells.values()], [...states]).then(({ light, blocks }) => {
      this.lights = new Map([...cells.keys()].map((key, i) => [key, light[i]]));
      for (const [state, models] of Object.entries(blocks)) this.blocks.set(state, loneQuads({ lone: () => ({ models }) }, this.textures, state));
      this.make(entities);
    }).catch((error) => {
      // Asked again with the next frame (a world on show sends one a tick).
      this.list = null;
      console.error('The entity layer could not draw the entities', error);
    }).finally(() => {
      this.asking = null;
      if (this.again) {
        this.again = false;
        this.ask();
      }
    });
  }

  make(entities) {
    const light = (x, y, z) => this.lightAt(x, y, z);
    const used = new Set();
    for (const entity of entities) {
      const key = appearance(this.data, entity, light);
      used.add(key);
      const known = this.objects.get(entity.id);
      if (known && known.key === key) {
        known.entity = entity;
        continue;
      }
      if (known) this.group.remove(known.object);
      let template = this.geometries.get(key);
      if (!template) this.geometries.set(key, template = this.build(entity));
      const object = template.clone();
      this.group.add(object);
      this.objects.set(entity.id, { key, object, entity });
    }
    for (const [key, template] of this.geometries) {
      if (used.has(key)) continue;
      template.traverse((object) => object.geometry?.dispose());
      this.geometries.delete(key);
    }
    this.update(performance.now());
    this.viewer.redraw();
  }

  // Puts every entity where it is at `now`. BlueMap draws idle frames only
  // when something changed, so the layer asks for the next one while any
  // entity glides or animates (a dropped item spins, an orb faces the
  // camera).
  update(now) {
    let awake = this.tracker.moving(now);
    const camera = this.cameraTurn();
    for (const state of this.tracker.sample(now)) {
      const built = this.objects.get(state.id);
      if (!built) continue;
      const ticks = built.entity.ticks + Math.min((now - this.tracker.lastFrame) / 50, 1);
      columnMajor(frame(this.data, built.entity, { pos: state.pos, yaw: state.yaw, pitch: state.pitch, age: ticks }, camera), built.object.matrix);
      built.object.matrixWorldNeedsUpdate = true;
      if (built.entity.type === 'minecraft:item' || built.entity.type === 'minecraft:experience_orb') awake = true;
    }
    if (awake) this.viewer.redraw();
  }

  // The camera's turn (its world rotation, column major), which an orb
  // faces.
  cameraTurn() {
    const m = this.viewer.camera.matrixWorld.elements;
    return [m[0], m[1], m[2], 0, m[4], m[5], m[6], 0, m[8], m[9], m[10], 0, 0, 0, 0, 1];
  }

  dispose() {
    this.events.removeEventListener('bluemapRenderFrame', this.onFrame);
    this.objects.clear();
    this.viewer.markers.remove(this.group);
  }
}
