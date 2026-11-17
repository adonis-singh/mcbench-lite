import java.io.BufferedOutputStream;
import java.io.BufferedReader;
import java.io.DataOutputStream;
import java.io.InputStreamReader;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;

// What Minecraft's client draws of the blocks and fluids of a world, asked
// of its own section renderer in the bake's JVM, where McbenchClient has
// started the client's classes: the reference the mesher (renderer/) is held
// to by scripts/mesher-minecraft-parity.js. Never part of the app.
//
// Each block of a scene is drawn as SectionCompiler.compile draws a block:
// its fluid, if it holds one, by LiquidBlockRenderer.tesselate (through
// BlockRenderDispatcher.renderLiquid); then, if its render shape is MODEL,
// its model's parts, collected by BlockStateModel.collectParts with a
// RandomSource seeded by BlockState.getSeed of its position, by
// ModelBlockRenderer.tesselateBlock with every face culled against its
// neighbour (checkSides), under a pose translated to the block's place in its
// section, with ambient occlusion on (the client's smooth lighting, the
// default option). The model is the client's own, baked by its ModelBakery
// from the jar's blockstates and models (McbenchEntities.bakeState); the
// fluid's sprites are set up as the client sets them up
// (LiquidBlockRenderer.setupSprites); the colours are BlockColors's.
//
// The level the renderers read is the scene (a BlockAndTintGetter of this
// program's): its block states, its sky and block light at each cell
// (getBrightness), the world's one biome for every colour resolver
// (getBlockTint, as McbenchClient's level answers BlockColors), and the
// client level's own getShade (ClientLevel.getShade of a level in the
// overworld's DimensionSpecialEffects). Asked of a cell outside the scene's
// box, or anything else, it fails.
//
// What is not run as the client runs it: the vertex sink. The client's
// VertexConsumer.putBulkData writes through LWJGL's MemoryStack, which needs
// LWJGL's natives; here putBulkData is answered by this program with the same
// arithmetic (each corner's colour the tint times its brightness, its light
// LightTexture.lightCoordsWithEmission of its light and the quad's emission,
// its position the pose's transform of the quad's) and keeps the inputs
// (brightness, tint, light) apart as well. The sprites are McbenchEntities's
// (no atlas is stitched): a quad's texture is the sprite it names, and its
// coordinates are turned back into coordinates over that texture (0 to 1).
// The client's own atlas shrinks every face's coordinates toward its middle by
// 4 / (the atlas's size in texels) (FaceBakery.shrinkUVs, and the fluid's
// lerp by uvShrinkRatio); the atlas here is 65,536 texels wide, which leaves
// that shrink under a thousandth of a texel.
//
// A scene file (written by renderer's `mcbench-mesh scene`), little-endian:
//   "MCBS" u32 1
//   u32 n, then n block state names (u16 length, UTF-8), as the block table names them
//   i32 min x, min y, min z, u32 size x, y, z: the box of cells the scene holds
//   u16 per cell, its state (an index into the names), x fastest, then z, then y
//   u8 per cell, its light: sky light << 4 | block light
//   i32 min x, max x, min z, max z: the cells drawn are the box's within these columns
// A capture file, little-endian:
//   "MCBT" u32 1
//   f32 x 12: getShade(direction, true) then getShade(direction, false), by
//     Direction ordinal (down, up, north, south, west, east)
//   u32 n, then n texture names (u16 length, UTF-8: "minecraft:block/stone")
//   then quads to the end, each:
//     u8 kind (0 a model's, 1 a fluid's), i32 x, y, z (the block), u32 state (its id)
//     u8 direction (the quad's facing, Direction ordinal; a fluid's quad's from its corners' winding)
//     u8 cullface (the direction whose getQuads list held it, 255 for the unculled list and for fluids)
//     u8 part (the index of its model part among those collected), u8 flags (1 shade, 2 drawn with
//       ambient occlusion, 4 the part asks for ambient occlusion)
//     i8 tint index (-1 for none), u8 light emission (the quad's), f32 r, g, b (the tint: BlockColors's
//       colour for its tint index, 1 for none; a fluid's: its colour before shading)
//     u16 texture (an index into the names)
//     4 corners, each: f32 x, y, z (blocks, from the block's lowest corner), f32 u, v (over the texture),
//       f32 brightness (a model's: ambient occlusion times getShade; a fluid's: its shade), i32 light
//       (packed as LightTexture packs it: block << 4 | sky << 20), u32 colour (ARGB as written)
//
//   java McbenchTerrain client.jar mappings.txt blockTable <Minecraft's libraries> biome
// then requests on stdin, one per line: <scene file> <capture file>; the
// reply to each is "MCBENCH_CAPTURED <quads> <ms>".
final class McbenchTerrain {
  final McbenchClient client;
  final McbenchEntities mc;
  final Object modelRenderer, liquidRenderer, renderShapeModel, random;
  final int noOverlay;
  final Method tesselateBlock, tesselateLiquid, collectParts, setSeed, getSeed, getRenderShape, getFluidState, fluidIsEmpty,
    getQuads, partAo, getLightEmission, quadVertices, quadTint, quadDirection, quadSprite, quadShade, quadEmission,
    poseLast, posePose, translate, transformPosition, withEmission, getShade, u0, u1, v0, v1, fluidIs, averageWaterColor;
  final Object lavaTag;
  final Class<?> poseStack, vector3f, blockAndTintGetter, vertexConsumer, direction, lightLayer;
  final Object[] directions;
  final Object clientLevel;
  final Map<String, Integer> stateIds = new HashMap<>();
  final Map<String, Integer> textures = new HashMap<>();
  final List<String> textureNames = new ArrayList<>();

  McbenchTerrain(McbenchClient client, McbenchEntities mc) throws Exception {
    this.client = client;
    this.mc = mc;
    for (int id = 0; id < client.names.size(); id++) stateIds.put(client.names.get(id), id);
    Class<?> blockState = client.states.get(0).getClass();

    // Smooth lighting on, as the client's options have it by default.
    Object game = null;
    for (var f : mc.type("Minecraft").getDeclaredFields()) if (java.lang.reflect.Modifier.isStatic(f.getModifiers()) && f.getType() == mc.type("Minecraft")) {
      f.setAccessible(true);
      game = f.get(null);
    }
    Object options = mc.get(game, "Minecraft", "options");
    Object smooth = McbenchBlockEntities.allocate(mc.type("OptionInstance"));
    mc.set(smooth, "OptionInstance", "value", true);
    mc.set(options, "Options", "ambientOcclusion", smooth);

    Object colors = mc.method("BlockColors", "createDefault").invoke(null);
    modelRenderer = mc.type("ModelBlockRenderer").getConstructor(mc.type("BlockColors")).newInstance(colors);
    liquidRenderer = mc.type("LiquidBlockRenderer").getConstructor().newInstance();
    Object shaper = mc.standIn(mc.type("BlockModelShaper"), Map.of(mc.method("BlockModelShaper", "getBlockModel", blockState), (a) -> {
      try {
        return mc.bakeState(a[1]);
      } catch (Exception error) {
        throw new IllegalStateException(error);
      }
    }));
    mc.method("LiquidBlockRenderer", "setupSprites", mc.type("BlockModelShaper"), mc.type("MaterialSet")).invoke(liquidRenderer, shaper, mc.bakeMaterials);

    poseStack = mc.type("PoseStack");
    blockAndTintGetter = mc.type("BlockAndTintGetter");
    vertexConsumer = mc.type("VertexConsumer");
    direction = mc.type("Direction");
    lightLayer = mc.type("LightLayer");
    directions = direction.getEnumConstants();
    tesselateBlock = mc.method("ModelBlockRenderer", "tesselateBlock", blockAndTintGetter, List.class, blockState, client.blockPos, poseStack, vertexConsumer, boolean.class, int.class);
    tesselateLiquid = mc.method("LiquidBlockRenderer", "tesselate", blockAndTintGetter, client.blockPos, vertexConsumer, blockState, mc.type("FluidState"));
    Class<?> randomSource = mc.type("RandomSource");
    random = mc.method("RandomSource", "create").invoke(null);
    setSeed = mc.method("RandomSource", "setSeed", long.class);
    collectParts = mc.method("BlockStateModel", "collectParts", randomSource, List.class);
    getSeed = mc.method("BlockBehaviour.BlockStateBase", "getSeed", client.blockPos);
    getRenderShape = mc.method("BlockBehaviour.BlockStateBase", "getRenderShape");
    getFluidState = mc.method("BlockBehaviour.BlockStateBase", "getFluidState");
    getLightEmission = mc.method("BlockBehaviour.BlockStateBase", "getLightEmission");
    fluidIsEmpty = mc.method("FluidState", "isEmpty");
    getQuads = mc.method("BlockModelPart", "getQuads", direction);
    partAo = mc.method("BlockModelPart", "useAmbientOcclusion");
    Object model = null;
    for (Object constant : mc.type("RenderShape").getEnumConstants()) if (((Enum<?>) constant).name().equals("MODEL")) model = constant;
    renderShapeModel = model;
    noOverlay = (int) mc.field("OverlayTexture", "NO_OVERLAY").get(null);
    quadVertices = mc.method("BakedQuad", "vertices");
    quadTint = mc.method("BakedQuad", "tintIndex");
    quadDirection = mc.method("BakedQuad", "direction");
    quadSprite = mc.method("BakedQuad", "sprite");
    quadShade = mc.method("BakedQuad", "shade");
    quadEmission = mc.method("BakedQuad", "lightEmission");
    poseLast = mc.method("PoseStack", "last");
    posePose = mc.method("PoseStack.Pose", "pose");
    translate = mc.method("PoseStack", "translate", float.class, float.class, float.class);
    vector3f = Class.forName("org.joml.Vector3f", true, client.loader);
    transformPosition = Class.forName("org.joml.Matrix4f", true, client.loader).getMethod("transformPosition", float.class, float.class, float.class, vector3f);
    withEmission = mc.method("LightTexture", "lightCoordsWithEmission", int.class, int.class);
    Class<?> sprite = mc.type("TextureAtlasSprite");
    u0 = mc.method("TextureAtlasSprite", "getU0");
    u1 = mc.method("TextureAtlasSprite", "getU1");
    v0 = mc.method("TextureAtlasSprite", "getV0");
    v1 = mc.method("TextureAtlasSprite", "getV1");

    // The block and fluid tags, bound as a server binds them from the jar's
    // data pack (TagLoader.loadPendingTags, then PendingTags.apply): the
    // fluid renderer tells lava by FluidTags.LAVA.
    Class<?> packType = mc.type("PackType");
    Object serverData = packType.getEnumConstants()[1];
    Object builder = mc.type("VanillaPackResourcesBuilder").getConstructor().newInstance();
    mc.method("VanillaPackResourcesBuilder", "pushAssetPath", packType, Path.class).invoke(builder, serverData, mc.jar.getPath("/data"));
    mc.method("VanillaPackResourcesBuilder", "exposeNamespace", String[].class).invoke(builder, (Object) new String[] { "minecraft" });
    Object info = mc.type("PackLocationInfo").getConstructors()[0].newInstance("vanilla", null, null, java.util.Optional.empty());
    Object pack = mc.method("VanillaPackResourcesBuilder", "build", mc.type("PackLocationInfo")).invoke(builder, info);
    Object data = mc.type("FallbackResourceManager").getConstructor(packType, String.class).newInstance(serverData, "minecraft");
    mc.method("FallbackResourceManager", "push", mc.type("PackResources")).invoke(data, pack);
    for (String registry : new String[] { "BLOCK", "FLUID" }) {
      Object pending = ((java.util.Optional<?>) mc.method("TagLoader", "loadPendingTags", mc.resourceManager(), mc.type("Registry")).invoke(null, data, mc.field("BuiltInRegistries", registry).get(null))).orElseThrow();
      mc.method("Registry.PendingTags", "apply").invoke(pending);
    }
    lavaTag = mc.field("FluidTags", "LAVA").get(null);
    fluidIs = mc.method("FluidState", "is", mc.type("TagKey"));
    averageWaterColor = mc.method("BiomeColors", "getAverageWaterColor", blockAndTintGetter, client.blockPos);

    // ClientLevel.getShade, of a client level in the overworld's effects.
    clientLevel = McbenchBlockEntities.allocate(mc.type("ClientLevel"));
    mc.set(clientLevel, "ClientLevel", "effects", mc.type("DimensionSpecialEffects.OverworldEffects").getConstructor().newInstance());
    getShade = mc.method("ClientLevel", "getShade", direction, boolean.class);
  }

  float shade(int direction, boolean shaded) throws Exception {
    return (float) getShade.invoke(clientLevel, directions[direction], shaded);
  }

  int texture(Object sprite) {
    String name = mc.sprites.get(sprite);
    if (name == null) throw new IllegalStateException("A quad over a sprite this program did not make: " + sprite);
    return textures.computeIfAbsent(name, (key) -> {
      textureNames.add(key);
      return textureNames.size() - 1;
    });
  }

  // A scene: its states by cell and light by cell, over its box.
  static final class Scene {
    Object[] palette;
    int minX, minY, minZ, sizeX, sizeY, sizeZ;
    short[] cells;
    byte[] light;
    int maskMinX, maskMaxX, maskMinZ, maskMaxZ;

    int index(int x, int y, int z) {
      int dx = x - minX, dy = y - minY, dz = z - minZ;
      if (dx < 0 || dy < 0 || dz < 0 || dx >= sizeX || dy >= sizeY || dz >= sizeZ) throw new IllegalStateException("Minecraft's renderer asked of " + x + ", " + y + ", " + z + ", outside the scene");
      return (dy * sizeZ + dz) * sizeX + dx;
    }
  }

  Scene read(Path file) throws Exception {
    ByteBuffer in = ByteBuffer.wrap(Files.readAllBytes(file)).order(ByteOrder.LITTLE_ENDIAN);
    byte[] magic = new byte[4];
    in.get(magic);
    if (!new String(magic, StandardCharsets.US_ASCII).equals("MCBS") || in.getInt() != 1) throw new IllegalStateException(file + " is not a version 1 scene");
    Scene scene = new Scene();
    scene.palette = new Object[in.getInt()];
    for (int i = 0; i < scene.palette.length; i++) {
      byte[] name = new byte[Short.toUnsignedInt(in.getShort())];
      in.get(name);
      String state = new String(name, StandardCharsets.UTF_8);
      Integer id = stateIds.get(state);
      if (id == null) throw new IllegalStateException("No block state " + state + " in the block table");
      scene.palette[i] = client.states.get(id);
    }
    scene.minX = in.getInt();
    scene.minY = in.getInt();
    scene.minZ = in.getInt();
    scene.sizeX = in.getInt();
    scene.sizeY = in.getInt();
    scene.sizeZ = in.getInt();
    int cells = Math.multiplyExact(Math.multiplyExact(scene.sizeX, scene.sizeY), scene.sizeZ);
    scene.cells = new short[cells];
    in.asShortBuffer().get(scene.cells);
    in.position(in.position() + cells * 2);
    scene.light = new byte[cells];
    in.get(scene.light);
    scene.maskMinX = in.getInt();
    scene.maskMaxX = in.getInt();
    scene.maskMinZ = in.getInt();
    scene.maskMaxZ = in.getInt();
    if (in.hasRemaining()) throw new IllegalStateException(file + " has trailing bytes");
    return scene;
  }

  // The scene as the renderers' level.
  Object level(Scene scene) throws Exception {
    Method x = McbenchClient.coordinate(client.blockPos, client.blockPos.getConstructor(int.class, int.class, int.class).newInstance(12345, 678, -9012), 12345);
    Method y = McbenchClient.coordinate(client.blockPos, client.blockPos.getConstructor(int.class, int.class, int.class).newInstance(12345, 678, -9012), 678);
    Method z = McbenchClient.coordinate(client.blockPos, client.blockPos.getConstructor(int.class, int.class, int.class).newInstance(12345, 678, -9012), -9012);
    Method blockState = mc.method("BlockGetter", "getBlockState", client.blockPos);
    Method fluidState = mc.method("BlockGetter", "getFluidState", client.blockPos);
    Method blockEntity = mc.method("BlockGetter", "getBlockEntity", client.blockPos);
    Method brightness = mc.method("BlockAndTintGetter", "getBrightness", lightLayer, client.blockPos);
    Method tint = mc.method("BlockAndTintGetter", "getBlockTint", client.blockPos, mc.type("ColorResolver"));
    Method shade = mc.method("BlockAndTintGetter", "getShade", direction, boolean.class);
    Method height = mc.method("LevelHeightAccessor", "getHeight"), minY = mc.method("LevelHeightAccessor", "getMinY");
    return Proxy.newProxyInstance(client.loader, new Class<?>[] { blockAndTintGetter }, (proxy, m, a) -> {
      if (McbenchEntities.asked(m, blockState) || McbenchEntities.asked(m, fluidState)) {
        Object state = scene.palette[scene.cells[scene.index((int) x.invoke(a[0]), (int) y.invoke(a[0]), (int) z.invoke(a[0]))]];
        return McbenchEntities.asked(m, blockState) ? state : getFluidState.invoke(state);
      }
      if (McbenchEntities.asked(m, brightness)) {
        int light = scene.light[scene.index((int) x.invoke(a[1]), (int) y.invoke(a[1]), (int) z.invoke(a[1]))];
        return ((Enum<?>) a[0]).name().equals("SKY") ? light >> 4 & 15 : light & 15;
      }
      if (McbenchEntities.asked(m, tint)) return client.resolveColor.invoke(a[1], client.biome, (double) (int) x.invoke(a[0]), (double) (int) z.invoke(a[0]));
      if (McbenchEntities.asked(m, shade)) return getShade.invoke(clientLevel, a[0], a[1]);
      if (McbenchEntities.asked(m, blockEntity)) return null;
      if (McbenchEntities.asked(m, height)) return 1024;
      if (McbenchEntities.asked(m, minY)) return -64;
      if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, a);
      throw new UnsupportedOperationException("Minecraft's renderer asked the scene " + m);
    });
  }

  // One quad as drawn.
  static final class Quad {
    int kind, x, y, z, state, direction, cullface = 255, part, flags, tintIndex = -1, emission, texture = -1;
    float r = 1, g = 1, b = 1;
    final float[] corners = new float[4 * 6];
    final int[] light = new int[4], color = new int[4];
    int vertices;
  }

  // Captures a scene into `out`; the number of quads.
  int capture(Scene scene, Path out) throws Exception {
    Object level = level(scene);
    List<Quad> quads = new ArrayList<>();
    Quad[] current = new Quad[1];
    // While a block's model is drawn: its parts' quads, by which list holds
    // them (a direction, or 255 for the unculled), and part.
    Map<Object, int[]> lists = new IdentityHashMap<>();
    int[] at = new int[3];
    Method putBulkData = mc.method("VertexConsumer", "putBulkData", mc.type("PoseStack.Pose"), mc.type("BakedQuad"), float[].class, float.class, float.class, float.class, float.class, int[].class, int.class, boolean.class);
    Method addVertex = mc.method("VertexConsumer", "addVertex", float.class, float.class, float.class);
    Method setColorFloat = mc.method("VertexConsumer", "setColor", float.class, float.class, float.class, float.class);
    Method setUv = mc.method("VertexConsumer", "setUv", float.class, float.class);
    Method setLight = mc.method("VertexConsumer", "setLight", int.class);
    Method setNormal = mc.method("VertexConsumer", "setNormal", float.class, float.class, float.class);
    Object[] fluidSprite = new Object[1];
    // The colour LiquidBlockRenderer.tesselate tints the fluid being drawn
    // with: white for lava, else BiomeColors.getAverageWaterColor.
    int[] fluidTint = new int[1];
    boolean[] blockAo = new boolean[1];
    Object consumer = Proxy.newProxyInstance(client.loader, new Class<?>[] { vertexConsumer }, (proxy, m, a) -> {
      if (McbenchEntities.asked(m, putBulkData)) {
        Object bakedQuad = a[1];
        Quad quad = new Quad();
        quad.kind = 0;
        quad.x = at[0];
        quad.y = at[1];
        quad.z = at[2];
        int[] list = lists.get(bakedQuad);
        if (list == null) throw new IllegalStateException("A quad drawn that no collected part holds");
        quad.cullface = list[0];
        quad.part = list[1];
        quad.direction = ((Enum<?>) quadDirection.invoke(bakedQuad)).ordinal();
        boolean shaded = (boolean) quadShade.invoke(bakedQuad);
        quad.flags = (shaded ? 1 : 0) | (blockAo[0] ? 2 : 0) | (list[2] != 0 ? 4 : 0);
        quad.tintIndex = (int) quadTint.invoke(bakedQuad);
        quad.emission = (int) quadEmission.invoke(bakedQuad);
        quad.r = (float) a[3];
        quad.g = (float) a[4];
        quad.b = (float) a[5];
        Object sprite = quadSprite.invoke(bakedQuad);
        quad.texture = texture(sprite);
        int[] vertices = (int[]) quadVertices.invoke(bakedQuad);
        float[] brightness = (float[]) a[2];
        int[] lightmap = (int[]) a[7];
        Object matrix = posePose.invoke(a[0]);
        for (int v = 0; v < 4; v++) {
          Object position = transformPosition.invoke(matrix, Float.intBitsToFloat(vertices[v * 8]), Float.intBitsToFloat(vertices[v * 8 + 1]), Float.intBitsToFloat(vertices[v * 8 + 2]), vector3f.getConstructor().newInstance());
          float px = (float) vector3f.getField("x").get(position), py = (float) vector3f.getField("y").get(position), pz = (float) vector3f.getField("z").get(position);
          quad.corners[v * 6] = (float) ((double) px - (at[0] & 15));
          quad.corners[v * 6 + 1] = (float) ((double) py - (at[1] & 15));
          quad.corners[v * 6 + 2] = (float) ((double) pz - (at[2] & 15));
          float[] uv = local(sprite, Float.intBitsToFloat(vertices[v * 8 + 4]), Float.intBitsToFloat(vertices[v * 8 + 5]));
          quad.corners[v * 6 + 3] = uv[0];
          quad.corners[v * 6 + 4] = uv[1];
          quad.corners[v * 6 + 5] = brightness[v];
          quad.light[v] = (int) withEmission.invoke(null, lightmap[v], quad.emission);
          // VertexConsumer.putBulkData's colour of a corner, read alpha
          // being the quad's own white (BakedQuad's colour words are -1).
          float alpha = (float) a[6];
          quad.color[v] = (int) (alpha * 255.0F) << 24 | (int) (255 * brightness[v] * quad.r) << 16 | (int) (255 * brightness[v] * quad.g) << 8 | (int) (255 * brightness[v] * quad.b);
        }
        quads.add(quad);
        return null;
      }
      if (McbenchEntities.asked(m, addVertex)) {
        Quad quad = current[0];
        if (quad == null || quad.vertices == 4) {
          quad = current[0] = new Quad();
          quad.kind = 1;
          quad.r = (fluidTint[0] >> 16 & 255) / 255.0F;
          quad.g = (fluidTint[0] >> 8 & 255) / 255.0F;
          quad.b = (fluidTint[0] & 255) / 255.0F;
          quad.x = at[0];
          quad.y = at[1];
          quad.z = at[2];
          quads.add(quad);
        }
        int v = quad.vertices;
        quad.corners[v * 6] = (float) ((double) (float) a[0] - (at[0] & 15));
        quad.corners[v * 6 + 1] = (float) ((double) (float) a[1] - (at[1] & 15));
        quad.corners[v * 6 + 2] = (float) ((double) (float) a[2] - (at[2] & 15));
        return proxy;
      }
      if (McbenchEntities.asked(m, setColorFloat)) {
        Quad quad = current[0];
        int v = quad.vertices;
        // The colour is the fluid's tint times the face's shade: the shade
        // is kept as the corner's brightness.
        float tint = Math.max(quad.r, Math.max(quad.g, quad.b));
        float colour = quad.r == tint ? (float) a[0] : quad.g == tint ? (float) a[1] : (float) a[2];
        quad.corners[v * 6 + 5] = colour / tint;
        quad.color[v] = (int) ((float) a[3] * 255.0F) << 24 | (int) ((float) a[0] * 255.0F) << 16 | (int) ((float) a[1] * 255.0F) << 8 | (int) ((float) a[2] * 255.0F);
        return proxy;
      }
      if (McbenchEntities.asked(m, setUv)) {
        Quad quad = current[0];
        Object sprite = spriteOf((float) a[0], (float) a[1], fluidSprite[0]);
        quad.texture = texture(sprite);
        float[] uv = local(sprite, (float) a[0], (float) a[1]);
        quad.corners[quad.vertices * 6 + 3] = uv[0];
        quad.corners[quad.vertices * 6 + 4] = uv[1];
        return proxy;
      }
      if (McbenchEntities.asked(m, setLight)) {
        current[0].light[current[0].vertices] = (int) a[0];
        return proxy;
      }
      if (McbenchEntities.asked(m, setNormal)) {
        current[0].vertices++;
        return proxy;
      }
      throw new UnsupportedOperationException("Minecraft's renderer asked the vertex sink " + m);
    });

    Object pose = poseStack.getConstructor().newInstance();
    Method pushPose = mc.method("PoseStack", "pushPose"), popPose = mc.method("PoseStack", "popPose");
    Object partsAt = null;
    mc.method("ModelBlockRenderer", "enableCaching").invoke(null);
    var constructor = client.blockPos.getConstructor(int.class, int.class, int.class);
    for (int y = scene.minY; y < scene.minY + scene.sizeY; y++) {
      for (int z = scene.minZ; z < scene.minZ + scene.sizeZ; z++) {
        if (z < scene.maskMinZ || z > scene.maskMaxZ) continue;
        for (int x = scene.minX; x < scene.minX + scene.sizeX; x++) {
          if (x < scene.maskMinX || x > scene.maskMaxX) continue;
          Object state = scene.palette[scene.cells[scene.index(x, y, z)]];
          Object fluid = getFluidState.invoke(state);
          boolean model = getRenderShape.invoke(state) == renderShapeModel;
          if ((boolean) fluidIsEmpty.invoke(fluid) && !model) continue;
          Object pos = constructor.newInstance(x, y, z);
          at[0] = x;
          at[1] = y;
          at[2] = z;
          if (!(boolean) fluidIsEmpty.invoke(fluid)) {
            current[0] = null;
            fluidTint[0] = (boolean) fluidIs.invoke(fluid, lavaTag) ? 0xffffff : (int) averageWaterColor.invoke(null, level, pos);
            tesselateLiquid.invoke(liquidRenderer, level, pos, consumer, state, fluid);
            if (current[0] != null && current[0].vertices != 4) throw new IllegalStateException("A fluid drew a quad of " + current[0].vertices + " corners");
            current[0] = null;
          }
          if (model) {
            setSeed.invoke(random, (long) getSeed.invoke(state, pos));
            List<Object> parts = new ArrayList<>();
            collectParts.invoke(mc.bakeState(state), random, parts);
            lists.clear();
            for (int part = 0; part < parts.size(); part++) {
              int ao = (boolean) partAo.invoke(parts.get(part)) ? 1 : 0;
              for (int d = 0; d <= 6; d++) {
                for (Object q : (List<?>) getQuads.invoke(parts.get(part), d < 6 ? directions[d] : null)) lists.putIfAbsent(q, new int[] { d < 6 ? d : 255, part, ao });
              }
            }
            blockAo[0] = !parts.isEmpty() && (int) getLightEmission.invoke(state) == 0 && (boolean) partAo.invoke(parts.get(0));
            pushPose.invoke(pose);
            translate.invoke(pose, (float) (x & 15), (float) (y & 15), (float) (z & 15));
            tesselateBlock.invoke(modelRenderer, level, parts, state, pos, pose, consumer, true, noOverlay);
            popPose.invoke(pose);
          }
        }
      }
    }
    mc.method("ModelBlockRenderer", "clearCache").invoke(null);

    // A fluid's quads: the facing of their corners' winding.
    for (Quad quad : quads) if (quad.kind == 1) quad.direction = facing(quad.corners);

    try (var data = new DataOutputStream(new BufferedOutputStream(Files.newOutputStream(out), 1 << 20))) {
      ByteBuffer header = ByteBuffer.allocate(8 + 48).order(ByteOrder.LITTLE_ENDIAN);
      header.put("MCBT".getBytes(StandardCharsets.US_ASCII)).putInt(1);
      for (boolean shaded : new boolean[] { true, false }) for (int d = 0; d < 6; d++) header.putFloat(shade(d, shaded));
      data.write(header.array());
      ByteBuffer count = ByteBuffer.allocate(4).order(ByteOrder.LITTLE_ENDIAN).putInt(textureNames.size());
      data.write(count.array());
      for (String name : textureNames) {
        byte[] bytes = name.getBytes(StandardCharsets.UTF_8);
        data.write(ByteBuffer.allocate(2).order(ByteOrder.LITTLE_ENDIAN).putShort((short) bytes.length).array());
        data.write(bytes);
      }
      ByteBuffer record = ByteBuffer.allocate(1 + 12 + 4 + 4 + 2 + 12 + 2 + 4 * 32).order(ByteOrder.LITTLE_ENDIAN);
      for (Quad quad : quads) {
        if (quad.texture < 0) throw new IllegalStateException("A quad without a texture at " + quad.x + ", " + quad.y + ", " + quad.z);
        record.clear();
        record.put((byte) quad.kind).putInt(quad.x).putInt(quad.y).putInt(quad.z);
        record.putInt((int) stateIds.get(McbenchClient.name(scene.palette[scene.cells[scene.index(quad.x, quad.y, quad.z)]])));
        record.put((byte) quad.direction).put((byte) quad.cullface).put((byte) quad.part).put((byte) quad.flags);
        record.put((byte) quad.tintIndex).put((byte) quad.emission);
        record.putFloat(quad.r).putFloat(quad.g).putFloat(quad.b);
        record.putShort((short) quad.texture);
        for (int v = 0; v < 4; v++) {
          for (int k = 0; k < 6; k++) record.putFloat(quad.corners[v * 6 + k]);
          record.putInt(quad.light[v]).putInt(quad.color[v]);
        }
        data.write(record.array(), 0, record.position());
      }
    }
    return quads.size();
  }

  // The facing of a quad's corners as wound (the larger axis of the normal
  // of its first three corners), as a Direction ordinal.
  static int facing(float[] c) {
    double ax = c[6] - c[0], ay = c[7] - c[1], az = c[8] - c[2];
    double bx = c[12] - c[0], by = c[13] - c[1], bz = c[14] - c[2];
    double nx = ay * bz - az * by, ny = az * bx - ax * bz, nz = ax * by - ay * bx;
    if (Math.abs(ny) >= Math.abs(nx) && Math.abs(ny) >= Math.abs(nz)) return ny < 0 ? 0 : 1;
    if (Math.abs(nz) >= Math.abs(nx)) return nz < 0 ? 2 : 3;
    return nx < 0 ? 4 : 5;
  }

  // A coordinate over this program's atlas, over its sprite's texture.
  float[] local(Object sprite, float u, float v) throws Exception {
    double lu = (u - (double) (float) u0.invoke(sprite)) / ((float) u1.invoke(sprite) - (double) (float) u0.invoke(sprite));
    double lv = (v - (double) (float) v0.invoke(sprite)) / ((float) v1.invoke(sprite) - (double) (float) v0.invoke(sprite));
    return new float[] { (float) lu, (float) lv };
  }

  // The sprite a fluid's coordinate lies over: one of LiquidBlockRenderer's
  // sprites (still, flowing, overlay), whichever's cell holds it.
  Object spriteOf(float u, float v, Object unused) throws Exception {
    Object found = null;
    for (String field : new String[] { "lavaIcons", "waterIcons" }) {
      for (Object sprite : (Object[]) mc.get(liquidRenderer, "LiquidBlockRenderer", field)) found = within(sprite, u, v, found);
    }
    found = within(mc.get(liquidRenderer, "LiquidBlockRenderer", "waterOverlay"), u, v, found);
    if (found == null) throw new IllegalStateException("A fluid's coordinate over none of its sprites: " + u + ", " + v);
    return found;
  }

  Object within(Object sprite, float u, float v, Object found) throws Exception {
    float a = (float) u0.invoke(sprite), b = (float) u1.invoke(sprite), c = (float) v0.invoke(sprite), d = (float) v1.invoke(sprite);
    if (u < a || u > b || v < c || v > d) return found;
    if (found != null && found != sprite) throw new IllegalStateException("A fluid's coordinate over two sprites");
    return sprite;
  }

  public static void main(String[] args) throws Exception {
    McbenchClient client = McbenchClient.read(Path.of(args[0]), Path.of(args[2]), args[3], args[4]);
    McbenchEntities mc = new McbenchEntities(client, Path.of(args[0]), Path.of(args[1]));
    mc.setUp();
    McbenchTerrain terrain = new McbenchTerrain(client, mc);
    System.out.println("MCBENCH_READY");
    System.out.flush();
    var input = new BufferedReader(new InputStreamReader(System.in));
    for (String line; (line = input.readLine()) != null;) {
      String[] request = line.split(" ");
      long started = System.nanoTime();
      int quads = terrain.capture(terrain.read(Path.of(request[0])), Path.of(request[1]));
      System.out.println("MCBENCH_CAPTURED " + quads + " " + (System.nanoTime() - started) / 1_000_000);
      System.out.flush();
    }
    // Minecraft's entry point (McbenchClient) leaves threads behind.
    System.exit(0);
  }
}
