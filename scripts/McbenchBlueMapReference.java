import com.flowpowered.math.vector.Vector2i;
import de.bluecolored.bluemap.common.BlueMapService;
import de.bluecolored.bluemap.common.config.MapConfig;
import de.bluecolored.bluemap.core.map.TextureGallery;
import de.bluecolored.bluemap.core.map.TileMetaConsumer;
import de.bluecolored.bluemap.core.map.hires.HiresModelManager;
import de.bluecolored.bluemap.core.resources.pack.resourcepack.ResourcePack;
import de.bluecolored.bluemap.core.storage.compression.Compression;
import de.bluecolored.bluemap.core.storage.file.FileStorage;
import de.bluecolored.bluemap.core.util.Grid;
import de.bluecolored.bluemap.core.util.Key;
import de.bluecolored.bluemap.core.util.WatchService;
import de.bluecolored.bluemap.core.world.BlockEntity;
import de.bluecolored.bluemap.core.world.BlockState;
import de.bluecolored.bluemap.core.world.Chunk;
import de.bluecolored.bluemap.core.world.DimensionType;
import de.bluecolored.bluemap.core.world.Entity;
import de.bluecolored.bluemap.core.world.LightData;
import de.bluecolored.bluemap.core.world.Region;
import de.bluecolored.bluemap.core.world.World;
import de.bluecolored.bluemap.core.world.biome.Biome;
import de.bluecolored.bluemap.core.map.hires.block.LiquidModelRenderer;
import de.bluecolored.bluemap.core.map.hires.block.ResourceModelRenderer;
import de.bluecolored.bluemap.core.map.hires.block.BlockStateModelRenderer;
import de.bluecolored.bluemap.core.map.hires.TileModelView;
import de.bluecolored.bluemap.core.resources.pack.resourcepack.model.Model;
import de.bluecolored.bluemap.core.util.Direction;
import de.bluecolored.bluemap.core.world.BlockProperties;
import de.bluecolored.bluemap.core.world.block.BlockNeighborhood;
import de.bluecolored.bluemap.core.world.block.ExtendedBlock;
import java.io.BufferedInputStream;
import java.io.BufferedReader;
import java.io.DataInputStream;
import java.io.IOException;
import java.io.InputStreamReader;
import java.lang.classfile.ClassFile;
import java.lang.classfile.ClassTransform;
import java.lang.classfile.CodeBuilder;
import java.lang.classfile.CodeElement;
import java.lang.classfile.CodeTransform;
import java.lang.classfile.Opcode;
import java.lang.classfile.instruction.InvokeInstruction;
import java.lang.classfile.instruction.StoreInstruction;
import java.lang.constant.ClassDesc;
import java.lang.constant.MethodTypeDesc;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Collection;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.function.Consumer;
import java.util.function.Predicate;

// BlueMap's own hires renderer drawing a blueprint, for scripts/render-parity.js
// to compare the browser's mesher (renderer/) with. Never part of the app.
//
// Run in a bake folder (server/bluemap.js's configuration, with the web app
// McbenchBake made, whose texture list the tiles index), with this file
// compiled together with server/McbenchBake.java:
//   java -cp <bluemap cli jar>:<classes> McbenchBlueMapReference --patch <folder>
//   java -cp <folder>:<bluemap cli jar>:<classes> McbenchBlueMapReference <minecraft version> <block table tsv> <the world's biome> <client jar>
// It draws the blocks Minecraft's client tints in the client's colours, as
// the bake does (McbenchBake.minecraftColors, from the bake's tints.tsv). A
// block Minecraft draws through a block entity renderer is drawn as the bake
// gives it to the mesher: by Minecraft's own blockstate and models (the
// client jar read by BlueMap without its resource extensions) and then the
// quads Minecraft's renderer draws, read from the bake's pack
// (web/mcbench-pack.bin), lit by the block's cell or the brighter of a
// double block's two (Minecraft.entity, which --patch has BlueMap's
// BlockStateModelRenderer call after a state's models).
// Where BlueMap's renderer asks its own block flags, it asks what Minecraft
// says, as the mesher does (Minecraft, from the block table): --patch writes
// BlueMap's ResourceModelRenderer and LiquidModelRenderer into <folder>, each
// call of a flag replaced with a call of Minecraft's answer and nothing else
// changed (patch), and the renderer runs with <folder> ahead of BlueMap's jar.
// Requests on stdin, one per line: <blueprint> <config folder> <tile folder>.
// <config folder> holds maps/live.conf, the map settings with the render
// mask. Every tile over each built chunk column and one block past it is
// written to <tile folder> as BlueMap's file storage lays it out; the reply
// is "MCBENCH_RENDERED <ms>".
public class McbenchBlueMapReference {
  static final TileMetaConsumer NO_LOWRES = (x, z, color, height, blockLight) -> {};

  public static void main(String[] args) throws Exception {
    if (args[0].equals("--patch")) {
      patch(Path.of(args[1]));
      return;
    }
    McbenchBake.logFailures();
    ResourcePack resources;
    Biome biome;
    try (var service = McbenchBake.service(args[0])) {
      resources = service.getOrLoadResourcePack();
      biome = service.loadDataPack(List.of()).getBiome(Key.parse(args[2]));
    }
    McbenchBake.minecraftColors(resources, Path.of("tints.tsv"));
    Minecraft.entities = Entities.read(Path.of("web/mcbench-pack.bin"));
    var vanilla = new ResourcePack(resources.getPackVersion());
    vanilla.loadResources(List.of(Path.of(args[3])));
    // Blocks drawn by Minecraft's own blockstates and models (the bake's
    // vanilla-blocks.txt): those drawn through a block entity renderer, and
    // those whose files BlueMap's resource extensions replace.
    for (String id : Files.readAllLines(Path.of("vanilla-blocks.txt"))) {
      Key key = Key.parse(id);
      var blockState = vanilla.getBlockStates().get(key);
      if (blockState == null) {
        resources.getBlockStates().remove(key);
        continue;
      }
      resources.getBlockStates().put(key, blockState);
      var sets = new ArrayList<de.bluecolored.bluemap.core.resources.pack.resourcepack.blockstate.VariantSet>();
      if (blockState.getVariants() != null) sets.addAll(List.of(blockState.getVariants().getVariants()));
      if (blockState.getMultipart() != null) sets.addAll(List.of(blockState.getMultipart().getParts()));
      for (var set : sets) for (var variant : set.getVariants()) {
        Key model = variant.getModel().getKey();
        if (vanilla.getModels().get(model) != null) resources.getModels().put(model, vanilla.getModels().get(model));
      }
    }
    TextureGallery gallery;
    try (var in = Files.newInputStream(Path.of("web/maps/live/textures.json"))) { gallery = TextureGallery.readTexturesFile(in); }
    var table = BlockTable.read(Path.of(args[1]));
    if (!ResourceModelRenderer.class.getProtectionDomain().getCodeSource().getLocation().getPath().endsWith("/")) {
      throw new IllegalStateException("BlueMap's renderer is not the patched one: run with --patch's folder ahead of BlueMap's jar");
    }
    Minecraft.table = table;
    ExecutorService workers = Executors.newFixedThreadPool(Runtime.getRuntime().availableProcessors());
    var input = new BufferedReader(new InputStreamReader(System.in));
    for (String line; (line = input.readLine()) != null;) {
      String[] request = line.split(" ");
      long start = System.nanoTime();
      var config = de.bluecolored.bluemap.common.config.BlueMapConfigManager.builder().minecraftVersion(args[0])
        .configRoot(Path.of(request[1])).packsFolder(Path.of("config/packs"))
        .usePluginConfig(false).isCli(true)
        .defaultDataFolder(Path.of("data")).defaultWebroot(Path.of("web")).build();
      MapConfig settings = config.getMapConfigs().get("live");
      var storage = new FileStorage(Path.of(request[2]), Compression.NONE, true);
      storage.initialize();
      BlueprintWorld blueprint = BlueprintWorld.read(Path.of(request[0]), table, biome);
      var models = new HiresModelManager(blueprint, storage.map("live").hiresTiles(), resources, gallery,
        settings, new Grid(settings.getHiresTileSize(), 2));
      Grid tileGrid = models.getTileGrid();
      Set<Vector2i> dirty = new HashSet<>();
      for (long column : blueprint.columns.keySet()) {
        int x = (int) (column >> 32), z = (int) column;
        int minX = tileGrid.getCellX((x << 4) - 1), maxX = tileGrid.getCellX((x << 4) + 16);
        int minZ = tileGrid.getCellY((z << 4) - 1), maxZ = tileGrid.getCellY((z << 4) + 16);
        for (int tileX = minX; tileX <= maxX; tileX++) for (int tileZ = minZ; tileZ <= maxZ; tileZ++) dirty.add(new Vector2i(tileX, tileZ));
      }
      var drawing = new ArrayList<Future<?>>();
      for (Vector2i tile : dirty) drawing.add(workers.submit(() -> models.render(tile, NO_LOWRES, true)));
      for (var tile : drawing) tile.get();
      if (McbenchBake.failure.get() != null) throw McbenchBake.failure.get();
      System.out.println("MCBENCH_RENDERED " + (System.nanoTime() - start) / 1_000_000);
      System.out.flush();
    }
    workers.shutdown();
  }

  static long key(int x, int z) { return (long) x << 32 | (z & 0xffffffffL); }

  // BlueMap's renderer classes with each block flag they ask replaced by
  // Minecraft's answer (Minecraft's methods): in ResourceModelRenderer's
  // createElementFace, the culling test of a face toward its cullface
  // neighbour (isCulling, getCullingIdentical) by Block.shouldRenderFace's
  // and whether the model is shaded (isAmbientocclusion) by Minecraft's
  // (also not for a state that emits light); in its testAo, each neighbour's
  // isOccluding by whether it shades; in LiquidModelRenderer's
  // createElementFace, the culling of a fluid's face (isSameLiquid, then
  // isCulling but upward) by LiquidBlockRenderer's. Each is found by the
  // calls it makes, and fails unless found as often as BlueMap makes them.
  static void patch(Path folder) throws Exception {
    ClassDesc minecraft = ClassDesc.of("McbenchBlueMapReference$Minecraft");
    ClassDesc extended = ClassDesc.of(ExtendedBlock.class.getName()), neighbourhood = ClassDesc.of(BlockNeighborhood.class.getName());
    ClassDesc model = ClassDesc.of(Model.class.getName()), direction = ClassDesc.of(Direction.class.getName());
    ClassDesc properties = ClassDesc.of(BlockProperties.class.getName());
    ClassDesc bool = ClassDesc.ofDescriptor("Z");
    write(folder, ResourceModelRenderer.class, Map.of("createElementFace", 6, "testAo", 2, "render", 4), (method, counts) -> new CodeTransform() {
      int neighbour = -1;
      boolean afterNeighbour, dropOccluding;
      public void accept(CodeBuilder code, CodeElement element) {
        ClassDesc self = ClassDesc.of(ResourceModelRenderer.class.getName());
        if (element instanceof StoreInstruction store && afterNeighbour) neighbour = store.slot();
        afterNeighbour = element instanceof InvokeInstruction call && call.name().equalsString("getRotationRelativeBlock")
          && call.typeSymbol().parameterList().equals(List.of(direction));
        if (element instanceof InvokeInstruction call && call.owner().asSymbol().equals(properties) && call.name().equalsString("isCulling")) {
          if (neighbour < 0) throw new IllegalStateException("isCulling before the cullface neighbour is read");
          code.pop().aload(neighbour).aload(0).getfield(self, "block", neighbourhood)
            .invokestatic(minecraft, "hides", MethodTypeDesc.of(bool, extended, neighbourhood));
          counts.merge("isCulling", 1, Integer::sum);
        } else if (element instanceof InvokeInstruction call && call.owner().asSymbol().equals(properties) && call.name().equalsString("getCullingIdentical")) {
          code.pop().iconst_0();
          counts.merge("getCullingIdentical", 1, Integer::sum);
        } else if (element instanceof InvokeInstruction call && call.owner().asSymbol().equals(model) && call.name().equalsString("isAmbientocclusion")) {
          code.aload(0).getfield(self, "block", neighbourhood).invokestatic(minecraft, "ambientOcclusion", MethodTypeDesc.of(bool, model, neighbourhood));
          counts.merge("isAmbientocclusion", 1, Integer::sum);
        } else if (method.equals("render") && element instanceof InvokeInstruction call && call.owner().asSymbol().equals(properties) && call.name().equalsString("isRandomOffset")) {
          // Minecraft's offset in place of BlueMap's: the view moved here,
          // and BlueMap's own move skipped.
          code.pop().aload(1).aload(3).invokestatic(minecraft, "offset", MethodTypeDesc.of(ClassDesc.ofDescriptor("V"), neighbourhood, ClassDesc.of(TileModelView.class.getName()))).iconst_0();
          counts.merge("isRandomOffset", 1, Integer::sum);
        } else if (method.equals("testAo") && element instanceof InvokeInstruction call && call.owner().asSymbol().equals(extended) && call.name().equalsString("getProperties")) {
          code.invokestatic(minecraft, "shades", MethodTypeDesc.of(bool, extended));
          dropOccluding = true;
          counts.merge("getProperties", 1, Integer::sum);
        } else if (dropOccluding && element instanceof InvokeInstruction call && call.name().equalsString("isOccluding")) {
          dropOccluding = false;
          counts.merge("isOccluding", 1, Integer::sum);
        } else {
          if (dropOccluding) throw new IllegalStateException("testAo reads a neighbour's properties for more than isOccluding");
          code.with(element);
        }
      }
    }, Map.of("isCulling", 1, "getCullingIdentical", 1, "isAmbientocclusion", 1, "getProperties", 4, "isOccluding", 4, "isRandomOffset", 1));
    write(folder, LiquidModelRenderer.class, Map.of("createElementFace", 8), (method, counts) -> (code, element) -> {
      ClassDesc self = ClassDesc.of(LiquidModelRenderer.class.getName());
      if (element instanceof InvokeInstruction call && call.owner().asSymbol().equals(self) && call.name().equalsString("isSameLiquid")) {
        code.swap().getfield(self, "block", neighbourhood).swap().aload(1)
          .invokestatic(minecraft, "hidesFluid", MethodTypeDesc.of(bool, neighbourhood, extended, direction));
        counts.merge("isSameLiquid", 1, Integer::sum);
      } else if (element instanceof InvokeInstruction call && call.owner().asSymbol().equals(properties) && call.name().equalsString("isCulling")) {
        code.pop().iconst_0();
        counts.merge("isCulling", 1, Integer::sum);
      } else {
        code.with(element);
      }
    }, Map.of("isSameLiquid", 1, "isCulling", 1));
    ClassDesc state = ClassDesc.of(BlockState.class.getName()), view = ClassDesc.of(TileModelView.class.getName());
    ClassDesc resourceState = ClassDesc.of(de.bluecolored.bluemap.core.resources.pack.resourcepack.blockstate.BlockState.class.getName());
    write(folder, BlockStateModelRenderer.class, Map.of("renderModel", 4), (method, counts) -> (code, element) -> {
      if (element instanceof InvokeInstruction call && call.owner().asSymbol().equals(resourceState) && call.name().equalsString("forEach")) {
        code.invokestatic(minecraft, "variants", MethodTypeDesc.of(ClassDesc.ofDescriptor("V"), resourceState, state, ClassDesc.ofDescriptor("I"), ClassDesc.ofDescriptor("I"), ClassDesc.ofDescriptor("I"), ClassDesc.of(Consumer.class.getName())));
        counts.merge("forEach", 1, Integer::sum);
        return;
      }
      if (element instanceof java.lang.classfile.instruction.ReturnInstruction) {
        code.aload(1).aload(2).aload(3).invokestatic(minecraft, "entity", MethodTypeDesc.of(ClassDesc.ofDescriptor("V"), neighbourhood, state, view));
        counts.merge("return", 1, Integer::sum);
      }
      code.with(element);
    }, Map.of("return", 2, "forEach", 1));
    // BlockNeighborhood makes a cache entry as a copy of its main block that
    // says it is at 0, 0, 0 while reading where the main block is, and an
    // entry's move to where it says it is is skipped: the entry at 0, 0, 0
    // read the wrong block. Minecraft reads every block where it is: each
    // move of an entry is made through a position beside it.
    write(folder, BlockNeighborhood.class, Map.of("set", 3, "getBlock", 3), (method, counts) -> (code, element) -> {
      if (element instanceof InvokeInstruction call && call.owner().asSymbol().equals(extended) && call.name().equalsString("set")) {
        code.invokestatic(minecraft, "move", MethodTypeDesc.of(ClassDesc.ofDescriptor("V"), extended, ClassDesc.ofDescriptor("I"), ClassDesc.ofDescriptor("I"), ClassDesc.ofDescriptor("I")));
        counts.merge("set", 1, Integer::sum);
      } else code.with(element);
    }, Map.of("set", 2));
  }

  interface Patch {
    CodeTransform of(String method, Map<String, Integer> counts);
  }

  // Writes `type`'s class file into `folder` with the methods named in
  // `methods` (by their parameter count) transformed, failing unless each
  // replacement was made as often as `expected` says.
  static void write(Path folder, Class<?> type, Map<String, Integer> methods, Patch patch, Map<String, Integer> expected) throws Exception {
    String file = type.getName().replace('.', '/') + ".class";
    byte[] original;
    try (var in = type.getClassLoader().getResourceAsStream(file)) { original = in.readAllBytes(); }
    var counts = new HashMap<String, Integer>();
    var classFile = ClassFile.of();
    byte[] patched = classFile.transformClass(classFile.parse(original), (ClassTransform) (builder, element) -> {
      if (element instanceof java.lang.classfile.MethodModel method && methods.containsKey(method.methodName().stringValue())
        && method.methodTypeSymbol().parameterCount() == methods.get(method.methodName().stringValue())) {
        builder.transformMethod(method, java.lang.classfile.MethodTransform.transformingCode(patch.of(method.methodName().stringValue(), counts)));
      } else builder.with(element);
    });
    if (!counts.equals(expected)) throw new IllegalStateException(type.getSimpleName() + " made the replacements " + counts + ", not " + expected);
    Path target = folder.resolve(file);
    Files.createDirectories(target.getParent());
    Files.write(target, patched);
  }

  // What Minecraft says where BlueMap's renderer asks its block flags, from
  // the block table, by the table's state of each BlueMap BlockState (the
  // state as BlueprintWorld builds it).
  public static final class Minecraft {
    static BlockTable table;
    static Entities entities;

    // After a state's models, the quads Minecraft's block entity renderer
    // draws of it, as renderer/'s Draw::entity draws them: unculled,
    // unshaded, in the sky and block light of the block's cell, the brighter
    // of each where the other half of a double block is the same block.
    public static void entity(BlockNeighborhood block, BlockState state, TileModelView view) {
      Entities.Drawn drawn = entities.of(state);
      if (drawn == null) return;
      var light = block.getLightData();
      int sky = light.getSkyLight(), glow = light.getBlockLight();
      if (drawn.partner()[0] != 0 || drawn.partner()[1] != 0 || drawn.partner()[2] != 0) {
        ExtendedBlock other = block.getNeighborBlock(drawn.partner()[0], drawn.partner()[1], drawn.partner()[2]);
        if (other.getBlockState().getId().equals(state.getId())) {
          sky = Math.max(sky, other.getLightData().getSkyLight());
          glow = Math.max(glow, other.getLightData().getBlockLight());
        }
      }
      var own = new TileModelView(view.getTileModel()).initialize();
      var tile = own.getTileModel();
      for (Entities.Quad quad : drawn.quads()) {
        float[] p = quad.corners(), uv = quad.uv();
        int face = own.add(2);
        tile.setPositions(face, p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7], p[8]);
        tile.setPositions(face + 1, p[0], p[1], p[2], p[6], p[7], p[8], p[9], p[10], p[11]);
        tile.setUvs(face, uv[0], uv[1], uv[2], uv[3], uv[4], uv[5]);
        tile.setUvs(face + 1, uv[0], uv[1], uv[4], uv[5], uv[6], uv[7]);
        for (int f = face; f < face + 2; f++) {
          tile.setAOs(f, 1, 1, 1).setColor(f, quad.color()[0], quad.color()[1], quad.color()[2]).setSunlight(f, sky).setBlocklight(f, glow).setMaterialIndex(f, quad.texture());
        }
      }
    }

    static int id(BlockState state) {
      return table.ids.get(state);
    }

    // The variants Minecraft's section renderer draws of a state at x, y, z
    // (renderer/src/mesh.rs Draw::model): from the sets its blockstate
    // applies (those BlueMap's BlockState.forEach visits), chosen with a
    // random source seeded by the state's seed (Mth.getSeed of its position
    // moved by the pack's seed step): a set of more than one variant takes
    // nextInt of its total weight; a multipart blockstate takes nextLong
    // first and seeds every set's choice with it. java.util.Random is
    // Minecraft's LegacyRandomSource, call for call.
    public static void variants(de.bluecolored.bluemap.core.resources.pack.resourcepack.blockstate.BlockState resource, BlockState state, int x, int y, int z, Consumer<de.bluecolored.bluemap.core.resources.pack.resourcepack.blockstate.Variant> consumer) {
      var sets = new ArrayList<de.bluecolored.bluemap.core.resources.pack.resourcepack.blockstate.VariantSet>();
      if (resource.getVariants() != null) {
        var chosen = resource.getVariants().getDefaultVariant();
        for (var set : resource.getVariants().getVariants()) if (set.getCondition().matches(state)) { chosen = set; break; }
        if (chosen != null) sets.add(chosen);
      }
      boolean multipart = resource.getMultipart() != null;
      if (multipart) for (var part : resource.getMultipart().getParts()) if (part.getCondition().matches(state)) sets.add(part);
      int[] random = entities.random(state);
      var source = new java.util.Random(seed(x + random[4], y + random[0], z + random[5]));
      long shared = multipart ? source.nextLong() : 0;
      for (var set : sets) {
        if (multipart) source.setSeed(shared);
        var variants = set.getVariants();
        if (variants.length == 1) {
          consumer.accept(variants[0]);
          continue;
        }
        int total = 0;
        for (var variant : variants) total += (int) variant.getWeight();
        int left = source.nextInt(total);
        for (var variant : variants) {
          left -= (int) variant.getWeight();
          if (left < 0) {
            consumer.accept(variant);
            break;
          }
        }
      }
    }

    // A cache entry moved to x, y, z by way of the position above it, so
    // that it reads the block there whatever it read before.
    public static void move(ExtendedBlock block, int x, int y, int z) {
      block.set(x, y + 1, z);
      block.set(x, y, z);
    }

    // Mth.getSeed
    static long seed(int x, int y, int z) {
      long seed = (long) (x * 3129871) ^ (long) z * 116129781L ^ (long) y;
      seed = seed * seed * 42317861L + seed * 11L;
      return seed >> 16;
    }

    // Minecraft's offset of the block's model (BlockState.getOffset, the
    // pack's offset kind and largest offsets), moving the model's faces.
    public static void offset(BlockNeighborhood block, TileModelView view) {
      int[] random = entities.random(block.getBlockState());
      if (random[1] == 0) return;
      float horizontal = Float.intBitsToFloat(random[2]), vertical = Float.intBitsToFloat(random[3]);
      long seed = seed(block.getX(), 0, block.getZ());
      double dx = Math.clamp(((float) (seed & 15L) / 15.0F - 0.5) * 0.5, (double) -horizontal, (double) horizontal);
      double dz = Math.clamp(((float) (seed >> 8 & 15L) / 15.0F - 0.5) * 0.5, (double) -horizontal, (double) horizontal);
      double dy = random[1] == 2 ? ((float) (seed >> 4 & 15L) / 15.0F - 1.0) * vertical : 0.0;
      view.translate((float) dx, (float) dy, (float) dz);
    }

    // The direction (0 down, 1 up, 2 north, 3 south, 4 west, 5 east) from
    // `own` to its neighbour `other`.
    static int direction(ExtendedBlock own, ExtendedBlock other) {
      int dx = other.getX() - own.getX(), dy = other.getY() - own.getY(), dz = other.getZ() - own.getZ();
      for (int d = 0; d < 6; d++) {
        if (BlueprintWorld.STEPS[d][0] == dx && BlueprintWorld.STEPS[d][1] == dy && BlueprintWorld.STEPS[d][2] == dz) return d;
      }
      throw new IllegalStateException("Not a neighbour: " + dx + ", " + dy + ", " + dz);
    }

    // Block.shouldRenderFace: whether the block hides its face toward `neighbour`.
    public static boolean hides(ExtendedBlock neighbour, BlockNeighborhood own) {
      int a = id(own.getBlockState()), b = id(neighbour.getBlockState()), d = direction(own, neighbour);
      return table.hides[table.cull[a][d] * table.cullShapes + table.cull[b][d ^ 1]] || table.skips(table.row[a], table.column[b], d);
    }

    // LiquidBlockRenderer: whether the fluid of the block hides its face
    // toward `neighbour` in `toward`: the fluid's block skips rendering beside
    // it, or its face turned to the fluid is full and not above it.
    public static boolean hidesFluid(BlockNeighborhood own, ExtendedBlock neighbour, Direction toward) {
      int a = table.fluid[id(own.getBlockState())], b = id(neighbour.getBlockState()), d = direction(own, neighbour);
      return table.skips(table.row[a], table.column[b], d) || (toward != Direction.UP && table.hides[1 * table.cullShapes + table.cull[b][d ^ 1]]);
    }

    // Whether the block darkens the corners of the faces beside it.
    public static boolean shades(ExtendedBlock block) {
      return table.shades[id(block.getBlockState())];
    }

    // ModelBlockRenderer: a model is drawn with ambient occlusion where it
    // asks for it and its state emits no light.
    public static boolean ambientOcclusion(Model model, BlockNeighborhood own) {
      return model.isAmbientocclusion() && table.emission[id(own.getBlockState())] == 0;
    }
  }

  // What Minecraft says of each block state: its default state, and how it
  // emits, blocks and passes light (written by McbenchBlockTable.java from
  // the pinned server): per state its emission, light block and six face
  // shapes, and whether each ordered pair of face shapes closes a face.
  // And how Minecraft culls and shades: by state id its six cull shapes,
  // skipRendering classes, fluid's state id, whether it shades the faces
  // beside it and its emission; whether each ordered pair of cull shapes
  // hides a face, and the skipRendering classes' masks.
  // The block entity quads of the bake's pack (McbenchBake.java's format,
  // version 7), by state as BlueMap names it: each state's entity variant's
  // mesh and step to its other half.
  static final class Entities {
    record Quad(float[] corners, float[] uv, int texture, float[] color) {}
    record Drawn(List<Quad> quads, int[] partner) {}
    record Property(String name, List<String> values) {}
    record Block(int first, List<Property> properties) {}

    final Map<String, Block> byId = new HashMap<>();
    final Set<String> blocks = new java.util.TreeSet<>();
    int[] selection;
    // By state, its random class; each class: seed step y, offset kind, the
    // raw bits of its largest horizontal and vertical offsets, seed step x, z.
    int[] randomOf;
    List<int[]> randoms = new ArrayList<>();
    List<int[]> selections = new ArrayList<>(), sets = new ArrayList<>();
    List<Object[]> variants = new ArrayList<>();
    List<List<Quad>> meshes = new ArrayList<>();

    // The pack's state id of a BlueMap state.
    int id(BlockState state) {
      Block block = byId.get(state.getId().getFormatted());
      if (block == null) return -1;
      int index = 0;
      for (Property property : block.properties()) {
        String value = state.getProperties().get(property.name());
        index = index * property.values().size() + property.values().indexOf(value);
      }
      return block.first() + index;
    }

    int[] random(BlockState state) {
      int id = id(state);
      if (id < 0) throw new IllegalStateException("No state " + state + " in the pack");
      return randoms.get(randomOf[id]);
    }

    Drawn of(BlockState state) {
      int id = id(state);
      if (id < 0) return null;
      int chosen = selection[id];
      if (chosen < 0) return null;
      for (int set : selections.get(chosen)) for (int variant : sets.get(set)) {
        Object[] entry = variants.get(variant);
        if (entry != null) return new Drawn(meshes.get((Integer) entry[0]), (int[]) entry[1]);
      }
      return null;
    }

    // Moves past `count` bytes (counted before the move).
    static void skip(java.nio.ByteBuffer in, int count) {
      in.position(in.position() + count);
    }

    static Entities read(Path file) throws IOException {
      var in = java.nio.ByteBuffer.wrap(Files.readAllBytes(file)).order(java.nio.ByteOrder.LITTLE_ENDIAN);
      var entities = new Entities();
      byte[] magic = new byte[4];
      in.get(magic);
      if (!new String(magic).equals("MCBP") || in.getInt() != 7) throw new IOException("Not a version 7 baked pack");
      java.util.function.Supplier<String> str = () -> { byte[] b = new byte[Short.toUnsignedInt(in.getShort())]; in.get(b); return new String(b, java.nio.charset.StandardCharsets.UTF_8); };
      for (int n = in.getInt(), i = 0; i < n; i++) {
        String id = str.get();
        int first = in.getInt();
        var properties = new ArrayList<Property>();
        for (int p = Byte.toUnsignedInt(in.get()); p > 0; p--) {
          String name = str.get();
          in.get();
          var values = new ArrayList<String>();
          for (int v = Byte.toUnsignedInt(in.get()); v > 0; v--) values.add(str.get());
          properties.add(new Property(name, values));
        }
        entities.byId.put(id, new Block(first, properties));
      }
      int states = in.getInt();
      entities.selection = new int[states];
      entities.randomOf = new int[states];
      for (int i = 0; i < states; i++) {
        skip(in, 14);
        entities.selection[i] = in.getInt();
        entities.randomOf[i] = Short.toUnsignedInt(in.getShort());
      }
      skip(in, in.getInt() * 6);
      int shapes = Short.toUnsignedInt(in.getShort());
      skip(in, (shapes * shapes + 7) / 8);
      skip(in, in.getInt() * 12);
      int cullShapes = Short.toUnsignedInt(in.getShort());
      skip(in, (cullShapes * cullShapes + 7) / 8);
      for (int rows = Short.toUnsignedInt(in.getShort()); rows > 0; rows--) {
        in.getShort();
        skip(in, Short.toUnsignedInt(in.getShort()));
      }
      skip(in, 13);
      for (int n = in.getInt(); n > 0; n--) {
        int dx = in.get(), dy = in.get(), dz = in.get();
        entities.randoms.add(new int[] { dy, Byte.toUnsignedInt(in.get()), in.getInt(), in.getInt(), dx, dz });
        skip(in, 4 + 16 + 2); // the fluid facts
      }
      skip(in, in.getInt() * 9); // fluid groups
      in.getInt(); // the water overlay
      var tints = new ArrayList<float[]>();
      for (int n = in.getInt(); n > 0; n--) tints.add(new float[] { in.getFloat(), in.getFloat(), in.getFloat() });
      skip(in, in.getInt() * 48); // matrices
      skip(in, in.getInt() * 81); // direction tables
      for (int n = in.getInt(); n > 0; n--) { // elements
        skip(in, 32);
        skip(in, Byte.toUnsignedInt(in.get()) * 39);
      }
      for (int n = in.getInt(); n > 0; n--) { // meshes
        in.get();
        skip(in, Short.toUnsignedInt(in.getShort()) * 4);
      }
      for (int n = in.getInt(); n > 0; n--) {
        int kind = in.get();
        if (kind == 1) { skip(in, 13); entities.variants.add(null); }
        else if (kind == 2) { skip(in, 8); entities.variants.add(null); }
        else if (kind == 3) entities.variants.add(new Object[] { in.getInt(), new int[] { in.get(), in.get(), in.get() } });
        else entities.variants.add(null);
      }
      for (int n = in.getInt(); n > 0; n--) {
        in.getDouble();
        int[] list = new int[Short.toUnsignedInt(in.getShort())];
        for (int i = 0; i < list.length; i++) { in.getDouble(); list[i] = in.getInt(); }
        entities.sets.add(list);
      }
      for (int n = in.getInt(); n > 0; n--) {
        in.get();
        int[] list = new int[Short.toUnsignedInt(in.getShort())];
        for (int i = 0; i < list.length; i++) list[i] = in.getInt();
        entities.selections.add(list);
      }
      var quadShapes = new ArrayList<float[][]>();
      for (int n = in.getInt(); n > 0; n--) {
        float[] corners = new float[12], uv = new float[8];
        for (int i = 0; i < 12; i++) corners[i] = in.getFloat();
        for (int i = 0; i < 8; i++) uv[i] = in.getFloat();
        quadShapes.add(new float[][] { corners, uv });
      }
      var pool = new ArrayList<Quad>();
      for (int n = in.getInt(); n > 0; n--) {
        float[][] shape = quadShapes.get(in.getInt());
        int texture = in.getInt();
        pool.add(new Quad(shape[0], shape[1], texture, tints.get(in.getInt())));
      }
      for (int n = in.getInt(); n > 0; n--) {
        var quads = new ArrayList<Quad>();
        for (int q = in.getInt(); q > 0; q--) quads.add(pool.get(in.getInt()));
        entities.meshes.add(quads);
      }
      // The blocks some state of which draws quads.
      for (var entry : entities.byId.entrySet()) {
        Block block = entry.getValue();
        int count = 1;
        for (Property property : block.properties()) count *= property.values().size();
        for (int i = 0; i < count; i++) {
          int chosen = entities.selection[block.first() + i];
          if (chosen < 0) continue;
          for (int set : entities.selections.get(chosen)) for (int variant : entities.sets.get(set)) if (entities.variants.get(variant) != null) entities.blocks.add(entry.getKey());
        }
      }
      return entities;
    }
  }

  static final class BlockTable {
    final Map<String, Map<String, String>> defaults = new HashMap<>();
    final Map<String, int[]> light = new HashMap<>(); // emission, lightBlock, then the face shapes down, up, north, south, west, east
    int shapes;
    boolean[] occludes; // by a * shapes + b
    final Map<BlockState, Integer> ids = new HashMap<>();
    int[][] cull;
    int[] row, column, fluid, emission;
    boolean[] shades;
    int cullShapes;
    boolean[] hides; // by a * cullShapes + b
    int[] first;
    int[][] masks; // by row class, from column class first on

    boolean skips(int row, int column, int direction) {
      int at = column - first[row];
      return at >= 0 && at < masks[row].length && (masks[row][at] >> direction & 1) == 1;
    }

    static BlockTable read(Path file) throws IOException {
      var table = new BlockTable();
      var states = new ArrayList<String[]>();
      for (String line : Files.readAllLines(file)) {
        String[] fields = line.split("\t");
        if (fields[0].equals("skiprendering")) {
          int rows = Integer.parseInt(fields[1]);
          table.first = new int[rows];
          table.masks = new int[rows][];
          for (int r = 0; r < rows; r++) {
            String[] masks = fields[3 + r].split(",");
            table.first[r] = Integer.parseInt(masks[0]);
            table.masks[r] = new int[masks.length - 1];
            for (int i = 1; i < masks.length; i++) table.masks[r][i - 1] = Integer.parseInt(masks[i]);
          }
          continue;
        }
        if (fields[0].equals("facehides")) {
          table.cullShapes = Integer.parseInt(fields[1]);
          table.hides = new boolean[table.cullShapes * table.cullShapes];
          for (int at = 0; at < table.hides.length; at++) table.hides[at] = fields[2].charAt(at) == '1';
          continue;
        }
        if (fields[0].equals("faceoccludes")) {
          table.shapes = Integer.parseInt(fields[1]);
          table.occludes = new boolean[table.shapes * table.shapes];
          for (int at = 0; at < table.occludes.length; at++) table.occludes[at] = fields[2].charAt(at) == '1';
          continue;
        }
        if (fields[0].equals("default")) {
          table.defaults.put(fields[1], McbenchBake.properties(fields[2]));
          continue;
        }
        int[] light = new int[8];
        light[0] = Integer.parseInt(fields[1]);
        light[1] = Integer.parseInt(fields[2]);
        String[] faces = fields[4].split(",");
        for (int i = 0; i < 6; i++) light[2 + i] = Integer.parseInt(faces[i]);
        table.light.put(fields[0], light);
        states.add(fields);
      }
      if (table.occludes == null || table.hides == null || table.masks == null) throw new IOException("The block table has no faceoccludes, facehides or skiprendering line");
      int n = states.size();
      table.cull = new int[n][6];
      table.row = new int[n];
      table.column = new int[n];
      table.fluid = new int[n];
      table.emission = new int[n];
      table.shades = new boolean[n];
      for (int id = 0; id < n; id++) {
        String[] fields = states.get(id);
        String name = fields[0];
        String block = name.contains("[") ? name.substring(0, name.indexOf('[')) : name;
        table.ids.put(McbenchBake.state(block, new TreeMap<>(McbenchBake.properties(name))), id);
        table.emission[id] = Integer.parseInt(fields[1]);
        table.fluid[id] = Integer.parseInt(fields[5]);
        String[] skip = fields[7].split(",");
        table.row[id] = Integer.parseInt(skip[0]);
        table.column[id] = Integer.parseInt(skip[1]);
        String[] cull = fields[8].split(",");
        for (int d = 0; d < 6; d++) table.cull[id][d] = Integer.parseInt(cull[d]);
        table.shades[id] = fields[9].equals("1");
      }
      return table;
    }
  }

  // A blueprint as a BlueMap world: its blocks at their Minecraft
  // coordinates, the one biome everywhere, and the light Paper's light engine
  // gives them in the void (sky light straight down through blocks that
  // block no light and from the open sides, block light from emitting
  // blocks, each step losing max(1, the light the next block blocks),
  // stopped where the face shapes two blocks turn to each other close).
  static final class BlueprintWorld implements World {
    static final int MARGIN = 15;
    final Biome biome;
    BlockState[] palette;
    int[][] lightOf; // per palette entry: emission, lightBlock, six face shapes
    int shapes;
    boolean[] occludes;
    int minX, minY, minZ, sizeX, sizeY, sizeZ; // the lit volume: the build and its margins
    short[] blocks;
    byte[] sky, glow;
    final Map<Long, Chunk> columns = new HashMap<>();

    BlueprintWorld(Biome biome) { this.biome = biome; }

    static BlueprintWorld read(Path file, BlockTable table, Biome biome) throws IOException {
      var world = new BlueprintWorld(biome);
      var built = new HashSet<Long>();
      try (var in = new DataInputStream(new BufferedInputStream(Files.newInputStream(file)))) {
        int entries = in.readInt();
        world.palette = new BlockState[entries];
        world.lightOf = new int[entries][];
        world.shapes = table.shapes;
        world.occludes = table.occludes;
        for (int index = 0; index < entries; index++) {
          String id = in.readUTF();
          var properties = new TreeMap<String, String>();
          for (int count = in.readInt(); count > 0; count--) properties.put(in.readUTF(), in.readUTF());
          var defaults = table.defaults.get(id);
          if (defaults == null) throw new IOException("Unknown block " + id);
          var complete = new TreeMap<>(defaults);
          complete.putAll(properties);
          String name = McbenchBake.name(id, complete);
          world.lightOf[index] = table.light.get(name);
          if (world.lightOf[index] == null) throw new IOException("Unknown block state " + name);
          world.palette[index] = index == 0 ? BlockState.AIR : new BlockState(Key.parse(id), complete);
        }
        int[] bounds = new int[6];
        for (int i = 0; i < 6; i++) bounds[i] = in.readInt(); // min x, y, z, max x, y, z
        world.minX = bounds[0] - MARGIN; world.minZ = bounds[2] - MARGIN;
        world.minY = Math.max(-64, bounds[1] - MARGIN - 1);
        int maxY = Math.min(959, bounds[4] + MARGIN);
        world.sizeX = bounds[3] - bounds[0] + 1 + 2 * MARGIN;
        world.sizeZ = bounds[5] - bounds[2] + 1 + 2 * MARGIN;
        world.sizeY = maxY - world.minY + 1;
        world.blocks = new short[world.sizeX * world.sizeY * world.sizeZ];
        for (int chunks = in.readInt(); chunks > 0; chunks--) {
          int ox = in.readInt() << 4, oy = in.readInt() << 4, oz = in.readInt() << 4;
          for (int i = 0; i < 4096; i++) {
            short value = in.readShort();
            if (value != 0) world.blocks[world.index(ox + (i & 15), oy + (i >> 8), oz + (i >> 4 & 15))] = value;
          }
          built.add(key(ox >> 4, oz >> 4));
        }
      }
      world.light();
      for (long column : built) world.columns.put(column, world.new Column((int) (column >> 32), (int) column));
      return world;
    }

    int index(int x, int y, int z) { return ((y - minY) * sizeZ + (z - minZ)) * sizeX + (x - minX); }
    boolean inside(int x, int y, int z) {
      return x >= minX && x < minX + sizeX && y >= minY && y < minY + sizeY && z >= minZ && z < minZ + sizeZ;
    }
    int paletteAt(int x, int y, int z) { return inside(x, y, z) ? blocks[index(x, y, z)] : 0; }

    static final int[][] STEPS = { {0, -1, 0}, {0, 1, 0}, {0, 0, -1}, {0, 0, 1}, {-1, 0, 0}, {1, 0, 0} }; // Direction order
    static final int[] OPPOSITE = { 1, 0, 3, 2, 5, 4 };

    void light() {
      sky = new byte[blocks.length];
      glow = new byte[blocks.length];
      var queue = new ArrayDeque<int[]>();
      // Sky light falls straight down at full strength while each block
      // below blocks no light; every other column of the void is open sky.
      // A closed face stops it, as it stops spreading light.
      for (int x = minX; x < minX + sizeX; x++) for (int z = minZ; z < minZ + sizeZ; z++) {
        int[] above = lightOf[0];
        for (int y = minY + sizeY - 1; y >= minY; y--) {
          int at = index(x, y, z);
          int[] block = lightOf[blocks[at]];
          if (block[1] != 0 || closed(above, block, 0)) break;
          sky[at] = 15;
          queue.add(new int[] { x, y, z });
          above = block;
        }
      }
      spread(sky, queue, true);
      for (int x = minX; x < minX + sizeX; x++) for (int y = minY; y < minY + sizeY; y++) for (int z = minZ; z < minZ + sizeZ; z++) {
        int at = index(x, y, z), emission = lightOf[blocks[at]][0];
        if (emission > 0) { glow[at] = (byte) emission; queue.add(new int[] { x, y, z }); }
      }
      spread(glow, queue, false);
    }

    // Whether light cannot pass from `source` into its neighbour `target` in
    // `direction`: the face shapes the two turn to each other close the face
    // (Shapes.faceShapeOccludes).
    boolean closed(int[] source, int[] target, int direction) {
      return occludes[source[2 + direction] * shapes + target[2 + OPPOSITE[direction]]];
    }

    void spread(byte[] level, ArrayDeque<int[]> queue, boolean skyLight) {
      while (!queue.isEmpty()) {
        int[] at = queue.poll();
        int from = index(at[0], at[1], at[2]), current = level[from];
        int[] source = lightOf[blocks[from]];
        for (int direction = 0; direction < 6; direction++) {
          int x = at[0] + STEPS[direction][0], y = at[1] + STEPS[direction][1], z = at[2] + STEPS[direction][2];
          if (!inside(x, y, z)) continue;
          int to = index(x, y, z);
          int[] target = lightOf[blocks[to]];
          if (closed(source, target, direction)) continue;
          int next = current - Math.max(1, target[1]);
          if (next > level[to]) { level[to] = (byte) next; queue.add(new int[] { x, y, z }); }
        }
      }
    }

    // A chunk column of the blueprint. Each column spans its lowest to
    // highest block, aligned to steps of 8, as the live renderer measured a
    // Minecraft chunk (an empty column spans y = -64 to -57). Blocks lie only
    // inside the lit volume, so only it is searched.
    final class Column implements Chunk {
      final int[] minY = new int[256], maxY = new int[256];

      Column(int chunkX, int chunkZ) {
        for (int column = 0; column < 256; column++) {
          int x = chunkX << 4 | column & 15, z = chunkZ << 4 | column >> 4;
          int top = BlueprintWorld.this.minY + sizeY - 1, bottom = BlueprintWorld.this.minY;
          while (top >= bottom && paletteAt(x, top, z) == 0) top--;
          while (bottom <= top && paletteAt(x, bottom, z) == 0) bottom++;
          if (top < bottom) { top = -65; bottom = -64; }
          maxY[column] = top | 7;
          minY[column] = bottom & ~7;
        }
      }

      Column() {
        java.util.Arrays.fill(maxY, -65 | 7);
        java.util.Arrays.fill(minY, -64);
      }

      @Override public int getMaxY(int x, int z) { return maxY[(z & 15) << 4 | x & 15]; }
      @Override public int getMinY(int x, int z) { return minY[(z & 15) << 4 | x & 15]; }
      @Override public boolean isGenerated() { return true; }
      @Override public boolean hasLightData() { return true; }
      @Override public long getInhabitedTime() { return 0; }
      @Override public BlockState getBlockState(int x, int y, int z) { return palette[paletteAt(x, y, z)]; }
      @Override public LightData getLightData(int x, int y, int z, LightData target) {
        return inside(x, y, z) ? target.set(sky[index(x, y, z)], glow[index(x, y, z)]) : target.set(15, 0);
      }
      @Override public Biome getBiome(int x, int y, int z) { return biome; }
      @Override public boolean hasWorldSurfaceHeights() { return false; }
      @Override public int getWorldSurfaceY(int x, int z) { return 0; }
      @Override public boolean hasOceanFloorHeights() { return false; }
      @Override public int getOceanFloorY(int x, int z) { return 0; }
      @Override public BlockEntity getBlockEntity(int x, int y, int z) { return null; }
      @Override public void iterateBlockEntities(Consumer<BlockEntity> consumer) {}
    }

    // Columns with no blocks are all alike, and their light is open sky.
    Column empty;

    @Override public Chunk getChunk(int x, int z) {
      Chunk column = columns.get(key(x, z));
      if (column != null) return column;
      if (empty == null) empty = new Column();
      return empty;
    }
    @Override public Chunk getChunkAtBlock(int x, int z) { return getChunk(x >> 4, z >> 4); }
    @Override public void invalidateChunkCache() {}
    @Override public void invalidateChunkCache(int x, int z) {}
    @Override public String getId() { return "blueprint"; }
    @Override public DimensionType getDimensionType() { return McbenchBake.DIMENSION; }
    @Override public Grid getChunkGrid() { return new Grid(16); }
    @Override public Grid getRegionGrid() { return new Grid(512); }
    @Override public Region<Chunk> getRegion(int x, int z) { throw new UnsupportedOperationException("A blueprint has no region files"); }
    @Override public Collection<Vector2i> listRegions() { return List.of(); }
    @Override public WatchService<Vector2i> createRegionWatchService() { throw new UnsupportedOperationException("A blueprint has no region files"); }
    @Override public void preloadRegionChunks(int x, int z, Predicate<Vector2i> chunkFilter) {}
    @Override public void iterateEntities(int minX, int minZ, int maxX, int maxZ, Consumer<Entity> consumer) {}
  }
}
