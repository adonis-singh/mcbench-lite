import java.lang.reflect.Method;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;

// What Minecraft's block renderer draws a block state by, asked of
// Minecraft's own code in the bake's JVM, where McbenchClient has started the
// client's classes: the seed its model's variants are chosen by, the offset
// its model is moved by, and what LiquidBlockRenderer.tesselate and
// FlowingFluid.getFlow ask of it (its fluid, and what it is as a fluid's
// neighbour). A line per state, in the order of Minecraft's state ids, each
// state named as the block table names it:
//   state  seedStep  offset  maxHorizontal  maxVertical  fluid  self  sides  flags  sturdy
// seedStep: "dx,dy,dz": BlockState.getSeed(pos) is Mth.getSeed of the
// position moved by it (0,0,0 for most blocks; a double plant's upper half
// takes its lower half's, 0,-1,0; a bed's foot its head's), found by asking
// getSeed at positions spread over the world and checked at each;
// the section renderer seeds a RandomSource with it before collecting the
// model's parts (SectionCompiler.compile, BlockStateModel.collectParts).
// offset: the state's BlockBehaviour.OffsetType, 0 none, 1 XZ, 2 XYZ, and
// its block's getMaxHorizontalOffset and getMaxVerticalOffset: what
// BlockState.getOffset gives at a position, the offset function's formula
// (written in renderer/src/mesh.rs and here, in `offset`), checked here
// against getOffset at the same positions for every state, bit for bit.
// fluid: "group,amount,falling" of its FluidState (getFluidState): group 0
// for none, else 1 + the index of its fluid among the fluid groups (fluids
// Fluid.isSame takes for one, in the order they are met), FluidState.getAmount
// and its FALLING value (0 or 1). self: the bits over the directions (down,
// up, north, south, west, east) toward which its own shape hides its fluid's
// face (LiquidBlockRenderer.isFaceOccludedBySelf). sides: "n,s,w,e", as the
// neighbour north, south, west or east of a fluid (the direction from the
// fluid to it), the greatest height of the fluid's side face it hides
// (LiquidBlockRenderer.isFaceOccludedByState of that direction, asked at
// each height its face occlusion shape's coordinates and 1 give, the face
// being hidden at every lesser height), -1 where it hides none. flags: 1 it
// hides the bottom face of a fluid above it (isFaceOccludedByNeighbor(DOWN,
// 0.8888889)), 2 the top face of a fluid below it at full height
// (isFaceOccludedByState(UP, 1)), 4 isSolid, 8 blocksMotion, 16
// isSolidRender, 32 an IceBlock, 64 a HalfTransparentBlock or LeavesBlock
// (beside which a water side face is drawn with the water overlay), 128
// emissiveRendering (its cell lights a fluid at full light). sturdy:
// the bits over the directions of isFaceSturdy (empty level, origin).
// The fluid groups then follow, a line each:
//   fluidgroup  index  lava  still  flow
// lava: whether it is FluidTags.LAVA (the fluid tags bound from the jar's
// data pack as a server binds them); still, flow: the textures
// LiquidBlockRenderer.setupSprites draws it with (its block's model's
// particle texture, ModelBakery's LAVA_FLOW or WATER_FLOW); and last:
//   fluidoverlay  texture  (ModelBakery.WATER_OVERLAY)
//
// The client's classes and members are named by their Mojang names
// (McbenchEntities.Mappings).
final class McbenchRenderStates {
  static void write(McbenchClient client, Path clientJar, Path mappings, Path target) throws Exception {
    McbenchEntities named = new McbenchEntities(client, clientJar, mappings);
    Class<?> blockPos = client.blockPos;
    var at = blockPos.getConstructor(int.class, int.class, int.class);
    Method getSeed = named.method("BlockBehaviour.BlockStateBase", "getSeed", blockPos);
    Method getOffset = named.method("BlockBehaviour.BlockStateBase", "getOffset", blockPos);
    Method hasOffset = named.method("BlockBehaviour.BlockStateBase", "hasOffsetFunction");
    Method getBlock = named.method("BlockBehaviour.BlockStateBase", "getBlock");
    Method maxHorizontal = named.method("BlockBehaviour", "getMaxHorizontalOffset");
    Method maxVertical = named.method("BlockBehaviour", "getMaxVerticalOffset");
    Method mthSeed = named.method("Mth", "getSeed", int.class, int.class, int.class);
    Class<?> vec3 = named.type("Vec3");
    // The fluid tags, bound as McbenchTerrain binds them.
    Class<?> packType = named.type("PackType");
    Object serverData = packType.getEnumConstants()[1];
    Object builder = named.type("VanillaPackResourcesBuilder").getConstructor().newInstance();
    named.method("VanillaPackResourcesBuilder", "pushAssetPath", packType, Path.class).invoke(builder, serverData, named.jar.getPath("/data"));
    named.method("VanillaPackResourcesBuilder", "exposeNamespace", String[].class).invoke(builder, (Object) new String[] { "minecraft" });
    Object info = named.type("PackLocationInfo").getConstructors()[0].newInstance("vanilla", null, null, java.util.Optional.empty());
    Object pack = named.method("VanillaPackResourcesBuilder", "build", named.type("PackLocationInfo")).invoke(builder, info);
    Object data = named.type("FallbackResourceManager").getConstructor(packType, String.class).newInstance(serverData, "minecraft");
    named.method("FallbackResourceManager", "push", named.type("PackResources")).invoke(data, pack);
    Object pending = ((java.util.Optional<?>) named.method("TagLoader", "loadPendingTags", named.resourceManager(), named.type("Registry")).invoke(null, data, named.field("BuiltInRegistries", "FLUID").get(null))).orElseThrow();
    named.method("Registry.PendingTags", "apply").invoke(pending);
    Object lavaTag = named.field("FluidTags", "LAVA").get(null);
    Class<?> fluidStateType = named.type("FluidState"), direction = named.type("Direction"), getter = named.type("BlockGetter");
    Method getFluidState = named.method("BlockBehaviour.BlockStateBase", "getFluidState");
    Method fluidEmpty = named.method("FluidState", "isEmpty"), fluidType = named.method("FluidState", "getType"), fluidAmount = named.method("FluidState", "getAmount");
    Method fluidIs = named.method("FluidState", "is", named.type("TagKey"));
    Method getValue = named.method("StateHolder", "getValue", named.type("Property"));
    Object falling = named.field("FlowingFluid", "FALLING").get(null);
    Method isSame = named.method("Fluid", "isSame", named.type("Fluid"));
    Method occludedByState = named.method("LiquidBlockRenderer", "isFaceOccludedByState", direction, float.class, client.states.get(0).getClass());
    Method faceShape = named.method("BlockBehaviour.BlockStateBase", "getFaceOcclusionShape", direction);
    Method coords = named.method("VoxelShape", "getCoords", named.type("Direction.Axis"));
    Method isSolid = named.method("BlockBehaviour.BlockStateBase", "isSolid"), blocksMotion = named.method("BlockBehaviour.BlockStateBase", "blocksMotion");
    Method solidRender = named.method("BlockBehaviour.BlockStateBase", "isSolidRender");
    Method sturdy = named.method("BlockBehaviour.BlockStateBase", "isFaceSturdy", getter, blockPos, direction);
    Method emissive = named.method("BlockBehaviour.BlockStateBase", "emissiveRendering", getter, blockPos);
    Object empty = null;
    for (Object constant : named.type("EmptyBlockGetter").getEnumConstants()) if (((Enum<?>) constant).name().equals("INSTANCE")) empty = constant;
    Object zero = named.field("BlockPos", "ZERO").get(null);
    Object[] directions = direction.getEnumConstants();
    Object yAxis = null;
    for (Object axis : named.type("Direction.Axis").getEnumConstants()) if (((Enum<?>) axis).name().equals("Y")) yAxis = axis;
    Class<?> ice = named.type("IceBlock"), halfTransparent = named.type("HalfTransparentBlock"), leaves = named.type("LeavesBlock");
    List<Object> groups = new ArrayList<>();
    List<Object> groupStates = new ArrayList<>();
    var x = vec3.getField(named.mappings.field(named.mappings.className("Vec3"), "x"));
    var y = vec3.getField(named.mappings.field(named.mappings.className("Vec3"), "y"));
    var z = vec3.getField(named.mappings.field(named.mappings.className("Vec3"), "z"));
    // Positions over the world: every low bit pattern of x and z, high and
    // negative coordinates, the build height's ends.
    List<int[]> positions = new ArrayList<>();
    java.util.Random spread = new java.util.Random(20261116L);
    for (int i = 0; i < 64; i++) positions.add(new int[] { spread.nextInt(60_000_000) - 30_000_000, spread.nextInt(1024) - 64, spread.nextInt(60_000_000) - 30_000_000 });
    for (int i = 0; i < 64; i++) positions.add(new int[] { i % 8 - 4, i / 8 - 64, i * 7 - 200 });
    List<String> lines = new ArrayList<>();
    for (int id = 0; id < client.states.size(); id++) {
      Object state = client.states.get(id);
      String name = client.names.get(id);
      String step = null;
      search:
      for (int dy : new int[] { 0, -1, 1, -2, 2 }) for (int dx = -1; dx <= 1; dx++) for (int dz = -1; dz <= 1; dz++) {
        boolean all = true;
        for (int[] p : positions) {
          long seed = (long) getSeed.invoke(state, at.newInstance(p[0], p[1], p[2]));
          if (seed != (long) mthSeed.invoke(null, p[0] + dx, p[1] + dy, p[2] + dz)) all = false;
        }
        if (all) {
          step = dx + "," + dy + "," + dz;
          break search;
        }
      }
      if (step == null) throw new IllegalStateException(name + "'s getSeed is not Mth.getSeed of its position or one beside it");
      int kind = 0;
      float horizontal = 0, vertical = 0;
      if ((boolean) hasOffset.invoke(state)) {
        Object block = getBlock.invoke(state);
        horizontal = (float) maxHorizontal.invoke(block);
        vertical = (float) maxVertical.invoke(block);
        kind = 1;
        for (int[] p : positions) if ((double) y.get(getOffset.invoke(state, at.newInstance(p[0], p[1], p[2]))) != 0) kind = 2;
        for (int[] p : positions) {
          Object offset = getOffset.invoke(state, at.newInstance(p[0], p[1], p[2]));
          double[] expected = offset(kind, horizontal, vertical, (long) mthSeed.invoke(null, p[0], 0, p[2]));
          if ((double) x.get(offset) != expected[0] || (double) y.get(offset) != expected[1] || (double) z.get(offset) != expected[2]) {
            throw new IllegalStateException(name + "'s offset at " + java.util.Arrays.toString(p) + " is not the offset function's formula");
          }
        }
      }
      Object fluid = getFluidState.invoke(state);
      int group = 0;
      String fluidFacts = "0,0,0";
      if (!(boolean) fluidEmpty.invoke(fluid)) {
        Object type = fluidType.invoke(fluid);
        for (int g = 0; g < groups.size() && group == 0; g++) if ((boolean) isSame.invoke(groups.get(g), type)) group = g + 1;
        if (group == 0) {
          groups.add(type);
          groupStates.add(fluid);
          group = groups.size();
        }
        fluidFacts = group + "," + fluidAmount.invoke(fluid) + "," + ((boolean) getValue.invoke(fluid, falling) ? 1 : 0);
      }
      int self = 0, sturdyBits = 0;
      for (int d = 0; d < 6; d++) {
        if ((boolean) occludedByState.invoke(null, opposite(directions, d), 1.0F, state)) self |= 1 << d;
        if ((boolean) sturdy.invoke(state, empty, zero, directions[d])) sturdyBits |= 1 << d;
      }
      // As a fluid's neighbour toward north, south, west, east (2..5).
      List<String> sides = new ArrayList<>();
      for (int d = 2; d < 6; d++) {
        var heights = new java.util.TreeSet<Double>();
        heights.add(1.0);
        for (double value : (java.util.List<Double>) coords.invoke(faceShape.invoke(state, opposite(directions, d)), yAxis)) if (value > 0) heights.add(value);
        double greatest = -1;
        for (double h : heights) if ((boolean) occludedByState.invoke(null, directions[d], (float) h, state)) greatest = Math.max(greatest, h);
        // Hidden at every height up to the greatest, checked at each.
        for (double h : heights) if (h <= greatest && !(boolean) occludedByState.invoke(null, directions[d], (float) h, state)) throw new IllegalStateException(name + " hides a fluid's side at " + greatest + " and not at " + h);
        sides.add(String.valueOf((float) greatest));
      }
      Object block = getBlock.invoke(state);
      int flags = ((boolean) occludedByState.invoke(null, directions[0], 0.8888889F, state) ? 1 : 0) | ((boolean) occludedByState.invoke(null, directions[1], 1.0F, state) ? 2 : 0)
        | ((boolean) isSolid.invoke(state) ? 4 : 0) | ((boolean) blocksMotion.invoke(state) ? 8 : 0) | ((boolean) solidRender.invoke(state) ? 16 : 0)
        | (ice.isInstance(block) ? 32 : 0) | (halfTransparent.isInstance(block) || leaves.isInstance(block) ? 64 : 0)
        | ((boolean) emissive.invoke(state, empty, zero) ? 128 : 0);
      lines.add(name + "\t" + step + "\t" + kind + "\t" + horizontal + "\t" + vertical + "\t" + fluidFacts + "\t" + self + "\t" + String.join(",", sides) + "\t" + flags + "\t" + sturdyBits);
    }
    // The groups' textures, as LiquidBlockRenderer.setupSprites takes them:
    // the still texture is the particle texture of its block's model (the
    // fluid's legacy block, FluidState.createLegacyBlock, its blockstate
    // file's model), the flowing one ModelBakery's material.
    Method legacy = named.method("FluidState", "createLegacyBlock");
    for (int g = 0; g < groups.size(); g++) {
      boolean lava = (boolean) fluidIs.invoke(groupStates.get(g), lavaTag);
      String blockId = McbenchClient.name(legacy.invoke(groupStates.get(g))).replaceFirst("\\[.*", "");
      lines.add("fluidgroup\t" + (g + 1) + "\t" + (lava ? 1 : 0) + "\t" + particle(named, blockId) + "\t" + material(named, lava ? "LAVA_FLOW" : "WATER_FLOW"));
    }
    lines.add("fluidoverlay\t" + material(named, "WATER_OVERLAY"));
    McbenchClient.write(target, lines);
  }

  static Object opposite(Object[] directions, int d) {
    return directions[d ^ 1];
  }

  // A ModelBakery material's texture ("minecraft:block/water_flow").
  static String material(McbenchEntities named, String field) throws Exception {
    return named.get(named.field("ModelBakery", field).get(null), "Material", "texture").toString();
  }

  // The particle texture of a block's model: its blockstate file's first
  // model, its textures' "particle", parents followed.
  static String particle(McbenchEntities named, String blockId) throws Exception {
    String[] id = blockId.split(":", 2);
    var state = com.google.gson.JsonParser.parseString(java.nio.file.Files.readString(named.jar.getPath("assets/" + id[0] + "/blockstates/" + id[1] + ".json")));
    String model = McbenchClient.models(state).get(0);
    for (;;) {
      String[] m = model.contains(":") ? model.split(":", 2) : new String[] { "minecraft", model };
      var json = com.google.gson.JsonParser.parseString(java.nio.file.Files.readString(named.jar.getPath("assets/" + m[0] + "/models/" + m[1] + ".json"))).getAsJsonObject();
      if (json.has("textures") && json.getAsJsonObject("textures").has("particle")) {
        String texture = json.getAsJsonObject("textures").get("particle").getAsString();
        return texture.contains(":") ? texture : "minecraft:" + texture;
      }
      model = json.get("parent").getAsString();
    }
  }

  // BlockBehaviour.Properties.offsetType's functions, of the seed
  // Mth.getSeed(x, 0, z): XZ (kind 1) and XYZ (kind 2).
  static double[] offset(int kind, float horizontal, float vertical, long seed) {
    double dx = Math.clamp(((float) (seed & 15L) / 15.0F - 0.5) * 0.5, (double) -horizontal, (double) horizontal);
    double dz = Math.clamp(((float) (seed >> 8 & 15L) / 15.0F - 0.5) * 0.5, (double) -horizontal, (double) horizontal);
    double dy = kind == 2 ? ((float) (seed >> 4 & 15L) / 15.0F - 1.0) * vertical : 0.0;
    return new double[] { dx, dy, dz };
  }
}
