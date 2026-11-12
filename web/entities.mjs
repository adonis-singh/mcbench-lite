// The entity layer of the viewer: draws the live world's entities, which
// BlueMap's own renderer leaves out, in BlueMap's web app (three.js). The
// app streams the whole entity list each frame (`set`); the layer keeps one
// object per entity id, moves it between frames and adds and removes
// objects as ids come and go. Models are in entity-models.mjs, textured from
// the atlas the bake makes (server/McbenchEntityAssets.java).
import { EntityIndex, EntityModels, appearance, compose, translation } from './entity-models.mjs';

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

  // Whether any entity is still gliding at `now`.
  moving(now) {
    for (const tween of this.entities.values()) if (now - tween.start < tween.duration) return true;
    return false;
  }
}

// The atlas image and the index, fetched from the bake's folder.
export async function loadEntityAssets(baseUrl) {
  const [response, atlas] = await Promise.all([fetch(new URL('mcbench-entities.json', baseUrl)), loadImage(new URL('mcbench-entities.png', baseUrl))]);
  if (!response.ok) throw new Error(`The entity index is unavailable (HTTP ${response.status})`);
  return { index: await response.json(), atlas };
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

const hue = (text) => {
  let h = 0;
  for (const c of text) h = (h * 31 + c.charCodeAt(0)) % 360;
  return h;
};

export class EntityLayer {
  // BlueMap: the app's `BlueMap` module namespace; app: window.bluemap;
  // assets: { index, atlas } (loadEntityAssets).
  constructor({ BlueMap, app, assets }) {
    this.BlueMap = BlueMap;
    this.Three = BlueMap.Three;
    this.viewer = app.mapViewer;
    this.events = app.events;
    this.models = new EntityModels(new EntityIndex(assets.index));
    this.tracker = new EntityTracker();
    this.objects = new Map();
    this.geometries = new WeakMap();
    this.group = new this.Three.Group();
    this.group.name = 'mcbench-entities';
    this.viewer.markers.add(this.group);
    this.materials = this.createMaterials(assets.atlas);
    this.onFrame = () => this.update(performance.now());
    this.events.addEventListener('bluemapRenderFrame', this.onFrame);
    installStyles();
  }

  // The map's own hires material: BlueMap's shaders and uniforms, so entities are
  // lit as the tiles are. Its vertex shader reads normals as world normals
  // (tiles are never moved); an entity's are turned with it.
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
    const vertex = BlueMap.HIRES_VERTEX_SHADER.replace('vNormal = normal;', 'vNormal = normalize(mat3(modelMatrix) * normal);');
    if (vertex === BlueMap.HIRES_VERTEX_SHADER) throw new Error("BlueMap's hires vertex shader no longer reads vNormal = normal");
    const atlasMaterial = new Three.ShaderMaterial({
      uniforms: {
        ...viewer.data.uniforms,
        textureImage: { type: 't', value: texture },
        animationFrameHeight: { value: 1 },
        animationFrameIndex: { value: 0 },
        animationInterpolationFrameIndex: { value: 0 },
        animationInterpolation: { value: 0 },
      },
      vertexShader: vertex,
      fragmentShader: BlueMap.HIRES_FRAGMENT_SHADER,
      transparent: false,
      depthWrite: true,
      depthTest: true,
      vertexColors: true,
      side: Three.DoubleSide,
    });
    const flash = new Three.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.6, depthWrite: false });
    return { atlas: atlasMaterial, flash };
  }

  geometry(mesh) {
    let geometry = this.geometries.get(mesh);
    if (geometry) return geometry;
    const { Three } = this;
    geometry = new Three.BufferGeometry();
    const count = mesh.position.length / 3;
    geometry.setAttribute('position', new Three.BufferAttribute(mesh.position, 3));
    geometry.setAttribute('normal', new Three.BufferAttribute(mesh.normal, 3));
    geometry.setAttribute('uv', new Three.BufferAttribute(mesh.uv, 2));
    geometry.setAttribute('color', new Three.BufferAttribute(mesh.color, 3));
    // The tiles' light attributes: full sky light, no block light.
    geometry.setAttribute('ao', new Three.BufferAttribute(new Float32Array(count).fill(1), 1));
    geometry.setAttribute('sunlight', new Three.BufferAttribute(new Float32Array(count).fill(15), 1));
    geometry.setAttribute('blocklight', new Three.BufferAttribute(new Float32Array(count), 1));
    geometry.computeBoundingSphere();
    this.geometries.set(mesh, geometry);
    return geometry;
  }

  // The objects of one entity: a root that follows the entity, with a child
  // per part.
  build(entity) {
    const { Three } = this;
    const recipe = this.models.describe(entity);
    const root = new Three.Group();
    root.matrixAutoUpdate = false;
    const parts = recipe.parts.map((part) => {
      let object;
      if (part.box) object = this.boxObject(part.box);
      else {
        object = new Three.Mesh(this.geometry(part.mesh), this.materials[part.material]);
        object.raycast = () => {};
      }
      object.matrixAutoUpdate = false;
      columnMajor(part.matrix, object.matrix);
      object.matrixWorldNeedsUpdate = true;
      root.add(object);
      return { object, update: part.update };
    });
    this.group.add(root);
    return { root, recipe, parts };
  }

  // A type with no model: its bounding box, translucent and outlined, named.
  boxObject({ width, height, label }) {
    const { Three, BlueMap } = this;
    const color = new Three.Color(`hsl(${hue(label)}, 70%, 60%)`);
    const geometry = new Three.BoxGeometry(width, height, width);
    const object = new Three.Group();
    const fill = new Three.Mesh(geometry, new Three.MeshBasicMaterial({ color, transparent: true, opacity: 0.28, depthWrite: false }));
    const edges = new Three.LineSegments(new Three.EdgesGeometry(geometry), new Three.LineBasicMaterial({ color }));
    fill.raycast = () => {};
    edges.raycast = () => {};
    object.add(fill, edges);
    const tag = new BlueMap.HtmlMarker(`mcbench-entity-${label}`);
    tag.updateFromData({
      position: { x: 0, y: height / 2 + 0.2, z: 0 },
      html: `<div class="mcbench-entity-tag">${label.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`)}</div>`,
      anchor: { x: 0, y: 0 },
      classes: [],
      listed: false,
      maxDistance: 60,
    });
    object.add(tag);
    object.userData.tag = tag;
    return object;
  }

  // HtmlMarker cleans up only its inner element; its outer one is removed here.
  release(built) {
    this.group.remove(built.root);
    built.root.traverse((object) => {
      if (object.isHtmlMarker) object.elementObject.element.remove();
    });
  }

  // Takes a frame's whole entity list: the entities that appeared are made,
  // the ones that are gone removed, the rest keep their objects and glide to
  // their new places; an entity that looks different is made again.
  set(entities, now = performance.now()) {
    const { removed } = this.tracker.set(entities, now);
    for (const id of removed) {
      this.release(this.objects.get(id));
      this.objects.delete(id);
    }
    for (const entity of entities) {
      const key = appearance(entity);
      const known = this.objects.get(entity.id);
      if (known && known.key === key) continue;
      if (known) this.release(known);
      this.objects.set(entity.id, { ...this.build(entity), key });
    }
    this.update(now);
    this.viewer.redraw();
  }

  // Puts every entity where it is at `now`. BlueMap draws idle frames only
  // when something changed, so the layer asks for the next one while any
  // entity glides or animates.
  update(now) {
    const seconds = now / 1000;
    let awake = this.tracker.moving(now);
    for (const state of this.tracker.sample(now)) {
      const built = this.objects.get(state.id);
      const [x, y, z] = state.pos;
      columnMajor(compose(translation(x, y, z), built.recipe.orient(state.yaw, state.pitch)), built.root.matrix);
      built.root.matrixWorldNeedsUpdate = true;
      if (built.recipe.animated) awake = true;
      for (const part of built.parts) {
        if (!part.update) continue;
        const matrix = part.update(seconds, state.entity);
        part.object.visible = matrix !== null;
        if (matrix) columnMajor(matrix, part.object.matrix);
        part.object.matrixWorldNeedsUpdate = true;
      }
    }
    if (awake) this.viewer.redraw();
  }

  dispose() {
    this.events.removeEventListener('bluemapRenderFrame', this.onFrame);
    for (const built of this.objects.values()) this.release(built);
    this.objects.clear();
    this.viewer.markers.remove(this.group);
  }
}

function installStyles() {
  if (document.getElementById('mcbench-entity-styles')) return;
  const style = document.createElement('style');
  style.id = 'mcbench-entity-styles';
  style.textContent = '.mcbench-entity-tag{transform:translate(-50%,-100%);padding:1px 6px;border-radius:4px;background:rgba(12,12,12,.7);color:#f5f5f5;font:500 11px/1.3 -apple-system,BlinkMacSystemFont,"Segoe UI",system-ui,sans-serif;white-space:nowrap;pointer-events:none}';
  document.head.appendChild(style);
}
