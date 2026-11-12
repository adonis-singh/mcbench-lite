import com.flowpowered.math.TrigMath;
import com.flowpowered.math.vector.Vector2i;
import com.flowpowered.math.vector.Vector3f;
import com.flowpowered.math.vector.Vector4f;
import de.bluecolored.bluemap.common.BlueMapService;
import de.bluecolored.bluemap.common.addons.AddonLoader;
import de.bluecolored.bluemap.common.config.BlueMapConfigManager;
import de.bluecolored.bluemap.common.config.MapConfig;
import de.bluecolored.bluemap.core.logger.Logger;
import de.bluecolored.bluemap.core.logger.PrintStreamLogger;
import de.bluecolored.bluemap.core.map.BmMap;
import de.bluecolored.bluemap.core.map.TextureGallery;
import de.bluecolored.bluemap.core.map.hires.ArrayTileModel;
import de.bluecolored.bluemap.core.map.hires.RenderSettings;
import de.bluecolored.bluemap.core.map.hires.TileModelView;
import de.bluecolored.bluemap.core.map.hires.block.BlockRendererType;
import de.bluecolored.bluemap.core.map.hires.block.LiquidModelRenderer;
import de.bluecolored.bluemap.core.map.hires.block.ResourceModelRenderer;
import de.bluecolored.bluemap.core.map.hires.block.color.BlendedBlockColorCalculator;
import de.bluecolored.bluemap.core.map.hires.block.color.BlockColorCalculator;
import de.bluecolored.bluemap.core.map.mask.Mask;
import de.bluecolored.bluemap.core.resources.pack.resourcepack.ResourcePack;
import de.bluecolored.bluemap.core.resources.pack.resourcepack.blockstate.Variant;
import de.bluecolored.bluemap.core.resources.pack.resourcepack.blockstate.VariantSet;
import de.bluecolored.bluemap.core.resources.pack.resourcepack.model.Element;
import de.bluecolored.bluemap.core.resources.pack.resourcepack.model.Face;
import de.bluecolored.bluemap.core.resources.pack.resourcepack.model.Model;
import de.bluecolored.bluemap.core.resources.pack.resourcepack.model.TextureVariable;
import de.bluecolored.bluemap.core.util.Direction;
import de.bluecolored.bluemap.core.util.Grid;
import de.bluecolored.bluemap.core.util.Key;
import de.bluecolored.bluemap.core.util.math.Color;
import de.bluecolored.bluemap.core.util.math.MatrixM3f;
import de.bluecolored.bluemap.core.util.math.MatrixM4f;
import de.bluecolored.bluemap.core.util.math.VectorM2f;
import de.bluecolored.bluemap.core.util.math.VectorM3f;
import de.bluecolored.bluemap.core.world.BlockProperties;
import de.bluecolored.bluemap.core.world.BlockState;
import de.bluecolored.bluemap.core.world.Chunk;
import de.bluecolored.bluemap.core.world.DimensionType;
import de.bluecolored.bluemap.core.world.Entity;
import de.bluecolored.bluemap.core.world.LightData;
import de.bluecolored.bluemap.core.world.Region;
import de.bluecolored.bluemap.core.world.World;
import de.bluecolored.bluemap.core.world.biome.Biome;
import de.bluecolored.bluemap.core.world.block.Block;
import de.bluecolored.bluemap.core.world.block.BlockNeighborhood;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.ArrayList;
import java.util.Collection;
import java.util.HashMap;
import java.util.IdentityHashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.concurrent.atomic.AtomicReference;
import java.util.function.Consumer;
import java.util.function.Predicate;

// The viewer's build-time assets, made by BlueMap's own code: BlueMap's web
// app, the map's settings and texture list, and the baked pack, everything
// the browser's mesher (renderer/) needs from BlueMap that does not depend
// on where a block stands. The mesher only does the work that does: picking
// variants by position hash, culling, light, ambient occlusion, liquids,
// transforms, sorting and writing the tile. No BlueMap runs in the app.
//
//   java -cp <bluemap cli jar>:<classes> McbenchBake <minecraft version> <block table tsv> [flow table out]
//
// run in a folder holding BlueMap's configuration (config/, written by
// server/bluemap.js, whose map "live" holds the render settings). It writes
// the web app to web/, the map's settings.json and textures.json to
// web/maps/live/ and the pack to web/mcbench-pack.bin, and the entity layer's
// atlas and index to web/mcbench-entities.png and .json (McbenchEntityAssets).
// It draws every model variant with BlueMap's own renderer too and fails
// unless the baked data rebuilds those faces (see check). With a third argument it also writes,
// for renderer/'s test, the flow angle BlueMap's LiquidModelRenderer gives
// every own level and four neighbours (see writeFlowTable).
//
// Format, little endian; str = u16 byte length + UTF-8; matrices are the 12
// floats m00..m23 (BlueMap's transforms never read the last row):
//   "MCBP" u32 version (1)
//   u32 blocks, each: str id, u32 first state, u8 properties, each:
//       str name, u8 default value, u8 values, each: str value
//     A block's states follow its first state in mixed radix over its
//     properties (sorted by name, the last varying fastest).
//   u32 states, each 16 bytes:
//       u8 flags: 1 culling, 2 occluding, 4 cullingIdentical, 8 alwaysWaterlogged,
//                 16 randomOffset, 32 waterlogged, 64 water, 128 air
//       u8 liquid level, u8 light emission, u8 light block,
//       u8 1 skylightDown | 2 shapeOcclusion | 4 tint blends,
//       u8 full faces (Direction.ordinal bits),
//       u16 block, u32 tint, u32 selection (0xFFFFFFFF: no blockstate resource)
//     (skylightDown, shapeOcclusion, full faces, emission and light block from
//     Paper's block table, everything else from BlueMap; "tint blends": the
//     state's color calculator averages the 5x3x5 blocks around it, reading
//     each through BlueMap's BlockNeighborhood cache)
//   u32 air state, u32 BlockState.WATER selection, u32 BlockState.WATER tint,
//     u8 BlockState.WATER tint blends
//   u32 tints, each: f32 r, g, b (the color BlueMap's calculator gives the
//     state in a plains world, as the face color it writes)
//   u32 matrices, each: 12 f32
//   u32 direction tables, each: 27 x (i8 dx, dy, dz): the neighbour BlueMap reads
//     for an offset (index (dx+1)*9 + (dy+1)*3 + dz+1) once turned with the
//     variant (ResourceModelRenderer.getRotationRelativeBlock)
//   u32 elements, each: f32 from xyz, to xyz, u32 matrix (rotation then 1/16
//     scale), i32 light emission, u8 faces, each: u8 direction (0 down, 1 up,
//     2 north, 3 south, 4 west, 5 east), f32 uv x4 (u, v; after face rotation
//     and uvlock), u32 texture, u8 tinted, u8 cullface (direction or 255)
//   u32 meshes, each: u8 ambient occlusion, u16 elements, u32 element each
//   u32 variants, each: u8 renderer (0 none: no model, 1 model, 2 liquid);
//       model: u32 mesh, u8 transformed, u32 matrix, u32 direction table;
//       liquid: u32 still texture, u32 flow texture
//   u32 variant sets, each: f64 total weight, u16 variants, each: f64 weight, u32 variant
//   u32 selections, each: u16 sets, u32 set each (the sets a state's
//     blockstate applies, in BlockState.forEach order)
//   liquid uvs: f32 x8 for each flow angle -180..180 (the up face), then f32 x8
//     for the sides (FLOWING_UV_SCALE)
class McbenchBake {
  static final Direction[] FACES = { Direction.DOWN, Direction.UP, Direction.NORTH, Direction.SOUTH, Direction.WEST, Direction.EAST };
  static final int NONE = -1;

  final ResourcePack pack;
  final TextureGallery gallery;
  final Biome plains;
  final BlockColorCalculator colors;
  final ResourceModelRenderer models;
  final ProbeWorld probe;
  final Field rendererVariant;
  final Method uvLockRotation, makeRotationRelative, calculator;

  final Pool tints = new Pool(), matrices = new Pool(), tables = new Pool(), elements = new Pool(),
    meshes = new Pool(), variants = new Pool(), sets = new Pool(), selections = new Pool();
  final IdentityHashMap<VariantSet, Integer> setIds = new IdentityHashMap<>();
  final Map<Variant, Integer> variantIds = new IdentityHashMap<>();
  final Map<Variant, BlockRendererType> variantTypes = new IdentityHashMap<>();

  McbenchBake(ResourcePack pack, TextureGallery gallery, Biome plains) throws Exception {
    this.pack = pack;
    this.gallery = gallery;
    this.plains = plains;
    this.colors = pack.createBlockColorCalculator();
    this.models = new ResourceModelRenderer(pack, gallery, SETTINGS);
    this.probe = new ProbeWorld(plains, pack);
    rendererVariant = ResourceModelRenderer.class.getDeclaredField("variant");
    rendererVariant.setAccessible(true);
    uvLockRotation = ResourceModelRenderer.class.getDeclaredMethod("uvLockRotation", Direction.class);
    uvLockRotation.setAccessible(true);
    makeRotationRelative = ResourceModelRenderer.class.getDeclaredMethod("makeRotationRelative", VectorM3f.class);
    makeRotationRelative.setAccessible(true);
    calculator = colors.getClass().getDeclaredMethod("getCalculator", BlockState.class);
    calculator.setAccessible(true);
  }

  // Values interned by their bytes: equal records share one id.
  static final class Pool {
    final Map<ByteBuffer, Integer> ids = new HashMap<>();
    final ByteArrayOutputStream bytes = new ByteArrayOutputStream();

    int intern(Out record) {
      ByteBuffer key = ByteBuffer.wrap(record.toByteArray());
      Integer id = ids.get(key);
      if (id != null) return id;
      ids.put(key, ids.size());
      bytes.writeBytes(key.array());
      return ids.size() - 1;
    }

    void writeTo(Out out) {
      out.u32(ids.size());
      out.writeBytes(bytes.toByteArray());
    }
  }

  static final class Out extends ByteArrayOutputStream {
    Out u8(int value) { if (value < 0 || value > 255) throw new IllegalStateException("Not a byte: " + value); write(value); return this; }
    Out i8(int value) { if (value < -128 || value > 127) throw new IllegalStateException("Not a byte: " + value); write(value & 0xff); return this; }
    Out u16(int value) { if (value < 0 || value > 0xffff) throw new IllegalStateException("Not a u16: " + value); write(value & 0xff); write(value >>> 8); return this; }
    Out u32(int value) { for (int i = 0; i < 4; i++) write(value >>> (8 * i) & 0xff); return this; }
    Out u64(long value) { for (int i = 0; i < 8; i++) write((int) (value >>> (8 * i)) & 0xff); return this; }
    Out f32(float value) { return u32(Float.floatToRawIntBits(value)); }
    Out f64(double value) { return u64(Double.doubleToRawLongBits(value)); }
    Out str(String value) { byte[] text = value.getBytes(StandardCharsets.UTF_8); u16(text.length); writeBytes(text); return this; }
    Out matrix(MatrixM4f m) {
      return f32(m.m00).f32(m.m01).f32(m.m02).f32(m.m03).f32(m.m10).f32(m.m11).f32(m.m12).f32(m.m13)
        .f32(m.m20).f32(m.m21).f32(m.m22).f32(m.m23);
    }
  }

  // The settings the probe renders with: no mask, no caves, no top-only.
  static final RenderSettings SETTINGS = new RenderSettings() {
    public int getRemoveCavesBelowY() { return Integer.MIN_VALUE; }
    public int getCaveDetectionOceanFloor() { return 0; }
    public boolean isCaveDetectionUsesBlockLight() { return false; }
    public float getAmbientLight() { return 0; }
    public Mask getRenderMask() { return Mask.ALL; }
    public boolean isSaveHiresLayer() { return true; }
    public boolean isRenderTopOnly() { return false; }
  };

  // A plains void holding chosen states, as the blueprint world is plains
  // everywhere: where the tint and the probe renders are taken.
  static final class ProbeWorld implements World {
    final Biome biome;
    final ResourcePack pack;
    final Map<Long, BlockState> states = new HashMap<>();
    final Chunk chunk = new Chunk() {
      @Override public BlockState getBlockState(int x, int y, int z) { return states.getOrDefault(key(x, y, z), BlockState.AIR); }
      @Override public LightData getLightData(int x, int y, int z, LightData target) { return target.set(15, 0); }
      @Override public Biome getBiome(int x, int y, int z) { return biome; }
    };

    ProbeWorld(Biome biome, ResourcePack pack) { this.biome = biome; this.pack = pack; }

    static long key(int x, int y, int z) { return ((long) x & 0xfffff) << 40 | ((long) y & 0xfffff) << 20 | (long) z & 0xfffff; }
    BlockNeighborhood at(int x, int y, int z, BlockState state) {
      states.clear();
      states.put(key(x, y, z), state);
      var block = new BlockNeighborhood(new Block(this, x, y, z), pack, SETTINGS, getDimensionType());
      block.set(x, y, z);
      return block;
    }

    @Override public Chunk getChunk(int x, int z) { return chunk; }
    @Override public Chunk getChunkAtBlock(int x, int z) { return chunk; }
    @Override public void invalidateChunkCache() {}
    @Override public void invalidateChunkCache(int x, int z) {}
    @Override public String getId() { return "probe"; }
    @Override public DimensionType getDimensionType() { return DIMENSION; }
    @Override public Grid getChunkGrid() { return new Grid(16); }
    @Override public Grid getRegionGrid() { return new Grid(512); }
    @Override public Region<Chunk> getRegion(int x, int z) { throw new UnsupportedOperationException(); }
    @Override public Collection<Vector2i> listRegions() { return List.of(); }
    @Override public void preloadRegionChunks(int x, int z, Predicate<Vector2i> chunkFilter) {}
    @Override public void iterateEntities(int minX, int minZ, int maxX, int maxZ, Consumer<Entity> consumer) {}
  }

  // The state as BlueprintWorld builds it: every property, sorted.
  static BlockState state(String id, TreeMap<String, String> properties) {
    return id.equals("minecraft:air") && properties.isEmpty() ? BlockState.AIR : new BlockState(Key.parse(id), properties);
  }

  // Whether the state's color calculator is BlueMap's blended one, which
  // reads the blocks around it (BlockColorsConfig's getCalculator).
  int blends(BlockState state) throws Exception {
    return calculator.invoke(colors, state) instanceof BlendedBlockColorCalculator ? 4 : 0;
  }

  int tint(BlockState state) {
    Color color = colors.getBlockColor(probe.at(0, 0, 0, state), state, new Color());
    return tints.intern(new Out().f32(color.r).f32(color.g).f32(color.b));
  }

  // The variant sets BlockState.forEach visits for this state, in order.
  int selection(BlockState state) throws Exception {
    var resource = pack.getBlockState(state);
    if (resource == null) return NONE;
    var applied = new ArrayList<VariantSet>();
    var variantsResource = resource.getVariants();
    if (variantsResource != null) {
      VariantSet chosen = variantsResource.getDefaultVariant();
      for (VariantSet set : variantsResource.getVariants()) if (set.getCondition().matches(state)) { chosen = set; break; }
      if (chosen != null) applied.add(chosen);
    }
    if (resource.getMultipart() != null) {
      for (VariantSet part : resource.getMultipart().getParts()) if (part.getCondition().matches(state)) applied.add(part);
    }
    var record = new Out().u16(applied.size());
    for (VariantSet set : applied) record.u32(set(set, state));
    return selections.intern(record);
  }

  static final Field TOTAL_WEIGHT;
  static {
    try {
      TOTAL_WEIGHT = VariantSet.class.getDeclaredField("totalWeight");
      TOTAL_WEIGHT.setAccessible(true);
    } catch (NoSuchFieldException e) { throw new IllegalStateException(e); }
  }

  int set(VariantSet set, BlockState state) throws Exception {
    Integer known = setIds.get(set);
    if (known != null) return known;
    var record = new Out().f64(TOTAL_WEIGHT.getDouble(set)).u16(set.getVariants().length);
    for (Variant variant : set.getVariants()) record.f64(variant.getWeight()).u32(variant(variant, state));
    int id = sets.intern(record);
    setIds.put(set, id);
    return id;
  }

  // A variant's renderer: MissingModelRenderer draws with the first type
  // that is a fallback for the state, else the default one, so the state
  // decides; with BlueMap's own types it is always the default renderer.
  static BlockRendererType renderer(Variant variant, BlockState state) {
    if (variant.getRenderer() != BlockRendererType.MISSING) return variant.getRenderer();
    for (BlockRendererType type : BlockRendererType.REGISTRY.values()) if (type.isFallbackFor(state)) return type;
    return BlockRendererType.DEFAULT;
  }

  int variant(Variant variant, BlockState state) throws Exception {
    BlockRendererType type = renderer(variant, state);
    if (type != BlockRendererType.DEFAULT && type != BlockRendererType.LIQUID) throw new IOException("Unknown renderer " + type.getKey() + " for " + state);
    Integer known = variantIds.get(variant);
    if (known != null) {
      if (variantTypes.get(variant) != type) throw new IOException("A variant's renderer depends on the state: " + state);
      return known;
    }
    Model model = variant.getModel().getResource(pack.getModels()::get);
    var record = new Out();
    if (model == null) record.u8(0);
    else if (type == BlockRendererType.LIQUID) {
      TextureVariable still = model.getTextures().get("still"), flow = model.getTextures().get("flow");
      record.u8(2)
        .u32(gallery.get(still == null ? null : still.getTexturePath(model.getTextures()::get)))
        .u32(gallery.get(flow == null ? null : flow.getTexturePath(model.getTextures()::get)));
    } else {
      rendererVariant.set(models, variant);
      record.u8(1).u32(mesh(model, variant)).u8(variant.isTransformed() ? 1 : 0)
        .u32(matrices.intern(new Out().matrix(variant.getTransformMatrix()))).u32(directions());
      check(model, variant, state);
    }
    int id = variants.intern(record);
    variantIds.put(variant, id);
    variantTypes.put(variant, type);
    return id;
  }

  // Where ResourceModelRenderer looks for each neighbour offset under the
  // variant set in `models` (getRotationRelativeBlock).
  int directions() throws Exception {
    var record = new Out();
    for (int dx = -1; dx <= 1; dx++) for (int dy = -1; dy <= 1; dy++) for (int dz = -1; dz <= 1; dz++) {
      VectorM3f direction = new VectorM3f(0, 0, 0).set(dx, dy, dz);
      makeRotationRelative.invoke(models, direction);
      record.i8(Math.round(direction.x)).i8(Math.round(direction.y)).i8(Math.round(direction.z));
    }
    return tables.intern(record);
  }

  int mesh(Model model, Variant variant) throws Exception {
    Element[] modelElements = model.getElements();
    int count = modelElements == null ? 0 : modelElements.length;
    var record = new Out().u8(model.isAmbientocclusion() ? 1 : 0).u16(count);
    for (int i = 0; i < count; i++) record.u32(element(model, modelElements[i], variant));
    return meshes.intern(record);
  }

  int element(Model model, Element element, Variant variant) throws Exception {
    if (element == null) throw new IOException("A model has a null element");
    Vector3f from = element.getFrom(), to = element.getTo();
    var transform = new MatrixM4f().copy(element.getRotation().getMatrix()).scale(1f / 16f, 1f / 16f, 1f / 16f);
    var faces = new ArrayList<Direction>();
    for (Direction direction : FACES) if (element.getFaces().get(direction) != null) faces.add(direction);
    var record = new Out().f32(from.getX()).f32(from.getY()).f32(from.getZ()).f32(to.getX()).f32(to.getY()).f32(to.getZ())
      .u32(matrices.intern(new Out().matrix(transform))).u32(element.getLightEmission()).u8(faces.size());
    for (Direction direction : faces) {
      Face face = element.getFaces().get(direction);
      record.u8(index(direction));
      for (float value : uvs(face, direction, variant)) record.f32(value);
      record.u32(gallery.get(face.getTexture().getTexturePath(model.getTextures()::get)))
        .u8(face.getTintindex() >= 0 ? 1 : 0).u8(face.getCullface() == null ? 255 : index(face.getCullface()));
    }
    return elements.intern(record);
  }

  static int index(Direction direction) {
    for (int i = 0; i < 6; i++) if (FACES[i] == direction) return i;
    throw new IllegalStateException();
  }

  // ResourceModelRenderer.createElementFace's UVs, with the variant set in `models`.
  float[] uvs(Face face, Direction direction, Variant variant) throws Exception {
    Vector4f uvRaw = face.getUv();
    float uvx = uvRaw.getX() / 16f, uvy = uvRaw.getY() / 16f, uvz = uvRaw.getZ() / 16f, uvw = uvRaw.getW() / 16f;
    VectorM2f[] rawUvs = { new VectorM2f(0, 0).set(uvx, uvw), new VectorM2f(0, 0).set(uvz, uvw), new VectorM2f(0, 0).set(uvz, uvy), new VectorM2f(0, 0).set(uvx, uvy) };
    VectorM2f[] uvs = new VectorM2f[4];
    int rotationSteps = Math.floorDiv(face.getRotation(), 90) % 4;
    if (rotationSteps < 0) rotationSteps += 4;
    for (int i = 0; i < 4; i++) uvs[i] = rawUvs[(rotationSteps + i) % 4];
    if (variant.isUvlock() && variant.isTransformed()) {
      float uvRotation = (float) uvLockRotation.invoke(models, direction);
      float cx = TrigMath.cos(uvRotation), cy = TrigMath.sin(uvRotation);
      for (VectorM2f uv : uvs) {
        uv.translate(-0.5f, -0.5f);
        uv.rotate(cx, cy);
        uv.translate(0.5f, 0.5f);
      }
    }
    return new float[] { uvs[0].x, uvs[0].y, uvs[1].x, uvs[1].y, uvs[2].x, uvs[2].y, uvs[3].x, uvs[3].y };
  }

  // Draws the variant with BlueMap's own renderer in an empty plains world,
  // where no face is culled, and fails unless the faces it wrote are the
  // ones the baked data rebuilds: the UVs and textures above, and the
  // positions renderer/ makes (element corners, element matrix, variant
  // matrix, random offset at 0, 0).
  static final Field SIZE, POSITION, UV, MATERIAL;
  static {
    try {
      SIZE = ArrayTileModel.class.getDeclaredField("size");
      POSITION = ArrayTileModel.class.getDeclaredField("position");
      UV = ArrayTileModel.class.getDeclaredField("uv");
      MATERIAL = ArrayTileModel.class.getDeclaredField("materialIndex");
      for (Field field : new Field[] { SIZE, POSITION, UV, MATERIAL }) field.setAccessible(true);
    } catch (NoSuchFieldException e) { throw new IllegalStateException(e); }
  }
  static final int[][] CORNERS = { {0, 2, 3, 1}, {5, 7, 6, 4}, {2, 0, 4, 6}, {1, 3, 7, 5}, {0, 1, 5, 4}, {3, 2, 6, 7} };

  void check(Model model, Variant variant, BlockState state) throws Exception {
    var tile = new ArrayTileModel(100);
    BlockNeighborhood block = probe.at(0, 0, 0, state);
    new ResourceModelRenderer(pack, gallery, SETTINGS).render(block, variant, new TileModelView(tile).initialize(), new Color());
    rendererVariant.set(models, variant);
    int size = SIZE.getInt(tile);
    float[] position = (float[]) POSITION.get(tile), uv = (float[]) UV.get(tile);
    int[] material = (int[]) MATERIAL.get(tile);
    int face = 0;
    boolean offset = block.getProperties().isRandomOffset();
    float dx = (hash(0, 0, 123984) - 0.5f) * 0.75f, dz = (hash(0, 0, 345542) - 0.5f) * 0.75f;
    for (Element element : model.getElements() == null ? new Element[0] : model.getElements()) {
      float[][] c = corners(element);
      var transform = new MatrixM4f().copy(element.getRotation().getMatrix()).scale(1f / 16f, 1f / 16f, 1f / 16f);
      for (int f = 0; f < 6; f++) {
        Face resource = element.getFaces().get(FACES[f]);
        if (resource == null) continue;
        float[] uvs = uvs(resource, FACES[f], variant);
        int texture = gallery.get(resource.getTexture().getTexturePath(model.getTextures()::get));
        int[][] triangles = { {0, 1, 2}, {0, 2, 3} };
        for (int[] triangle : triangles) {
          if (face >= size || material[face] != texture) throw new IOException("Probe mismatch (texture) for " + state);
          for (int v = 0; v < 3; v++) {
            int corner = triangle[v];
            float[] p = apply(transform, c[CORNERS[f][corner]]);
            if (variant.isTransformed()) p = apply(variant.getTransformMatrix(), p);
            if (offset) { p[0] += dx; p[1] += 0; p[2] += dz; }
            for (int k = 0; k < 3; k++) if (Float.floatToRawIntBits(p[k]) != Float.floatToRawIntBits(position[face * 9 + v * 3 + k]))
              throw new IOException("Probe mismatch (position) for " + state);
            for (int k = 0; k < 2; k++) if (Float.floatToRawIntBits(uvs[corner * 2 + k]) != Float.floatToRawIntBits(uv[face * 6 + v * 2 + k]))
              throw new IOException("Probe mismatch (uv) for " + state);
          }
          face++;
        }
      }
    }
    if (face != size) throw new IOException("Probe mismatch (face count) for " + state);
  }

  static float[][] corners(Element element) {
    float minX = element.getFrom().getX(), minY = element.getFrom().getY(), minZ = element.getFrom().getZ();
    float maxX = element.getTo().getX(), maxY = element.getTo().getY(), maxZ = element.getTo().getZ();
    return new float[][] { {minX, minY, minZ}, {minX, minY, maxZ}, {maxX, minY, minZ}, {maxX, minY, maxZ},
      {minX, maxY, minZ}, {minX, maxY, maxZ}, {maxX, maxY, minZ}, {maxX, maxY, maxZ} };
  }

  // ArrayTileModel.transform for one point.
  static float[] apply(MatrixM4f m, float[] p) {
    float x = p[0], y = p[1], z = p[2];
    return new float[] { m.m00 * x + m.m01 * y + m.m02 * z + m.m03, m.m10 * x + m.m11 * y + m.m12 * z + m.m13, m.m20 * x + m.m21 * y + m.m22 * z + m.m23 };
  }

  static float hash(int x, int z, long seed) {
    final long hash = x * 73428767L ^ z * 4382893L ^ seed * 457;
    return (hash * (hash + 456149) & 0x00ffffff) / (float) 0x01000000;
  }

  // LiquidModelRenderer's UVs for the up face at each flow angle, and for the sides.
  void liquidUvs(Out out) throws Exception {
    for (int angle = -180; angle <= 180; angle++) {
      var transform = new MatrixM3f().identity().translate(-0.5f, -0.5f).scale(0.5f, 0.5f, 1).rotate(-angle, 0, 0, 1).translate(0.5f, 0.5f);
      for (float[] uv : new float[][] { {0, 1}, {1, 1}, {1, 0}, {0, 0} }) {
        VectorM2f point = new VectorM2f(0, 0).set(uv[0], uv[1]).transform(transform);
        out.f32(point.x).f32(point.y);
      }
    }
    Field scale = LiquidModelRenderer.class.getDeclaredField("FLOWING_UV_SCALE");
    scale.setAccessible(true);
    var sides = (MatrixM3f) scale.get(null);
    for (float[] uv : new float[][] { {0, 1}, {1, 1}, {1, 0}, {0, 0} }) {
      VectorM2f point = new VectorM2f(0, 0).set(uv[0], uv[1]).transform(sides);
      out.f32(point.x).f32(point.y);
    }
  }

  record TableState(String name, int[] light) {}

  void write(Path tablePath, Path target) throws Exception {
    // The block table: per block its default state, then its states.
    var blocks = new LinkedHashMap<String, String>(); // id -> default state
    var blockStates = new LinkedHashMap<String, List<TableState>>();
    for (String line : Files.readAllLines(tablePath)) {
      String[] fields = line.split("\t");
      if (fields[0].equals("default")) {
        blocks.put(fields[1], fields[2]);
        blockStates.put(fields[1], new ArrayList<>());
        continue;
      }
      String id = fields[0].contains("[") ? fields[0].substring(0, fields[0].indexOf('[')) : fields[0];
      int[] light = new int[6];
      for (int i = 0; i < 6; i++) light[i] = Integer.parseInt(fields[i + 1]);
      blockStates.get(id).add(new TableState(fields[0], light));
    }
    var out = new Out();
    out.writeBytes("MCBP".getBytes(StandardCharsets.US_ASCII));
    out.u32(1);
    var states = new Out();
    int stateCount = 0, air = NONE, block = 0;
    out.u32(blocks.size());
    for (var entry : blocks.entrySet()) {
      String id = entry.getKey();
      List<TableState> table = blockStates.get(id);
      var values = new TreeMap<String, List<String>>();
      var light = new HashMap<String, int[]>();
      for (TableState tableState : table) {
        properties(tableState.name()).forEach((key, value) -> {
          var known = values.computeIfAbsent(key, k -> new ArrayList<>());
          if (!known.contains(value)) known.add(value);
        });
        light.put(tableState.name(), tableState.light());
      }
      var defaults = properties(entry.getValue());
      out.str(id).u32(stateCount).u8(values.size());
      for (var property : values.entrySet()) {
        out.str(property.getKey()).u8(property.getValue().indexOf(defaults.get(property.getKey()))).u8(property.getValue().size());
        for (String value : property.getValue()) out.str(value);
      }
      var names = new ArrayList<>(values.keySet());
      int combinations = 1;
      for (var list : values.values()) combinations *= list.size();
      if (combinations != table.size()) throw new IOException(id + " has " + table.size() + " states, not every combination of its properties");
      for (int index = 0; index < combinations; index++) {
        var properties = new TreeMap<String, String>();
        for (int p = names.size() - 1, rest = index; p >= 0; p--) {
          var list = values.get(names.get(p));
          properties.put(names.get(p), list.get(rest % list.size()));
          rest /= list.size();
        }
        String name = name(id, properties);
        int[] lightOf = light.get(name);
        if (lightOf == null) throw new IOException("No block table line for " + name);
        BlockState state = state(id, properties);
        if (state.isAir() && id.equals("minecraft:air")) air = stateCount;
        BlockProperties props = pack.getBlockProperties(state);
        int flags = (props.isCulling() ? 1 : 0) | (props.isOccluding() ? 2 : 0) | (props.getCullingIdentical() ? 4 : 0)
          | (props.isAlwaysWaterlogged() ? 8 : 0) | (props.isRandomOffset() ? 16 : 0) | (state.isWaterlogged() ? 32 : 0)
          | (state.isWater() ? 64 : 0) | (state.isAir() ? 128 : 0);
        states.u8(flags).u8(state.getLiquidLevel()).u8(lightOf[0]).u8(lightOf[1]).u8(lightOf[2] | lightOf[3] << 1 | blends(state)).u8(lightOf[4])
          .u16(block).u32(tint(state)).u32(selection(state));
        stateCount++;
      }
      block++;
    }
    if (air == NONE) throw new IOException("The block table has no air");
    out.u32(stateCount);
    out.writeBytes(states.toByteArray());
    out.u32(air).u32(selection(BlockState.WATER)).u32(tint(BlockState.WATER)).u8(blends(BlockState.WATER));
    for (Pool pool : new Pool[] { tints, matrices, tables, elements, meshes, variants, sets, selections }) pool.writeTo(out);
    liquidUvs(out);
    Path partial = target.resolveSibling(target.getFileName() + ".part");
    Files.write(partial, out.toByteArray());
    Files.move(partial, target, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
    System.out.println("MCBENCH_BAKED " + stateCount + " states, " + elements.ids.size() + " elements, " + variants.ids.size()
      + " variants, " + out.size() + " bytes");
  }

  // For renderer/'s test: LiquidModelRenderer.getFlowingAngle, called on
  // BlueMap's own renderer, for water of every level whose four sides
  // (-x, +x, -z, +z) each hold air (code 0) or water of level code - 1
  // (1..16). One i16 per case, the own level slowest, then the sides in
  // that order, each a base-17 digit.
  void writeFlowTable(Path target) throws Exception {
    var renderer = new LiquidModelRenderer(pack, gallery, SETTINGS);
    Field blockField = LiquidModelRenderer.class.getDeclaredField("block");
    Field stateField = LiquidModelRenderer.class.getDeclaredField("blockState");
    Method angle = LiquidModelRenderer.class.getDeclaredMethod("getFlowingAngle");
    for (var accessible : new java.lang.reflect.AccessibleObject[] { blockField, stateField, angle }) accessible.setAccessible(true);
    BlockState[] water = new BlockState[16];
    for (int level = 0; level < 16; level++) water[level] = new BlockState(Key.minecraft("water"), new TreeMap<>(Map.of("level", String.valueOf(level))));
    int[][] sides = { {-1, 0}, {1, 0}, {0, -1}, {0, 1} };
    var out = new Out();
    for (int own = 0; own < 16; own++) for (int code = 0; code < 17 * 17 * 17 * 17; code++) {
      BlockNeighborhood block = probe.at(0, 0, 0, water[own]);
      for (int side = 0, rest = code; side < 4; side++, rest /= 17) {
        int neighbour = rest % 17;
        if (neighbour > 0) probe.states.put(ProbeWorld.key(sides[3 - side][0], 0, sides[3 - side][1]), water[neighbour - 1]);
      }
      blockField.set(renderer, block);
      stateField.set(renderer, water[own]);
      int result = (int) angle.invoke(renderer);
      out.u16(result & 0xffff);
    }
    Files.write(target, out.toByteArray());
  }

  // What Minecraft's overworld is to BlueMap here: the live world's
  // dimension, 1024 blocks tall (see minecraft-dimension.js).
  static final DimensionType DIMENSION = new DimensionType() {
    public boolean isNatural() { return true; }
    public boolean hasSkylight() { return true; }
    public boolean hasCeiling() { return false; }
    public float getAmbientLight() { return 0; }
    public int getMinY() { return -64; }
    public int getHeight() { return 1024; }
    public Long getFixedTime() { return null; }
    public double getCoordinateScale() { return 1; }
  };

  // A block table state's properties ("id[key=value,...]"), sorted.
  static Map<String, String> properties(String state) {
    var properties = new TreeMap<String, String>();
    int open = state.indexOf('[');
    if (open < 0) return properties;
    for (String pair : state.substring(open + 1, state.length() - 1).split(",")) {
      int equals = pair.indexOf('=');
      properties.put(pair.substring(0, equals), pair.substring(equals + 1));
    }
    return properties;
  }

  // A state as Minecraft names it: every property, sorted.
  static String name(String id, Map<String, String> properties) {
    if (properties.isEmpty()) return id;
    var text = new StringBuilder(id).append('[');
    properties.forEach((key, value) -> text.append(key).append('=').append(value).append(','));
    text.setLength(text.length() - 1);
    return text.append(']').toString();
  }

  static final AtomicReference<IOException> failure = new AtomicReference<>();

  // BlueMap logs some failures instead of throwing them.
  static void logFailures() {
    Logger.global.clear();
    Logger.global.put(new PrintStreamLogger(System.err, System.err) {
      @Override public void logError(String message, Throwable cause) {
        failure.compareAndSet(null, new IOException(message, cause));
        super.logError(message, cause);
      }
    });
  }

  // BlueMap as the CLI starts it, on the configuration in config/.
  static BlueMapService service(String version) throws Exception {
    var packs = Path.of("config/packs");
    Files.createDirectories(packs);
    AddonLoader.INSTANCE.tryLoadAddons(packs);
    var config = BlueMapConfigManager.builder().minecraftVersion(version)
      .configRoot(Path.of("config")).packsFolder(packs)
      .usePluginConfig(false).isCli(true)
      .defaultDataFolder(Path.of("data")).defaultWebroot(Path.of("web")).build();
    return new BlueMapService(config);
  }

  public static void main(String[] args) throws Exception {
    logFailures();
    try (var service = service(args[0])) {
      ResourcePack resources = service.getOrLoadResourcePack();
      service.createOrUpdateWebApp(true);
      Biome plains = service.loadDataPack(List.of()).getBiome(Key.minecraft("plains"));
      // The map, saved as BlueMap saves a new one: its settings, and the
      // texture list the viewer resolves every tile's materials against,
      // which the pack's texture indices follow.
      MapConfig settings = service.getConfig().getMapConfigs().get("live");
      var map = new BmMap("live", settings.getName(), new ProbeWorld(plains, resources),
        service.getOrLoadStorage(settings.getStorage()).map("live"), resources, settings);
      map.save();
      var bake = new McbenchBake(resources, map.getTextureGallery(), plains);
      bake.write(Path.of(args[1]), Path.of("web/mcbench-pack.bin"));
      McbenchEntityAssets.write(service.getConfig().getCoreConfig().getData().resolve("minecraft-client-" + args[0] + ".jar"),
        Path.of("web/mcbench-entities.png"), Path.of("web/mcbench-entities.json"));
      if (args.length > 2) bake.writeFlowTable(Path.of(args[2]));
    }
    if (failure.get() != null) throw failure.get();
  }
}
