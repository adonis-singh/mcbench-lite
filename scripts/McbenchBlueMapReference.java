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
import java.io.BufferedInputStream;
import java.io.BufferedReader;
import java.io.DataInputStream;
import java.io.IOException;
import java.io.InputStreamReader;
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
//   java -cp <bluemap cli jar>:<classes> McbenchBlueMapReference <minecraft version> <block table tsv>
// Requests on stdin, one per line: <blueprint> <config folder> <tile folder>.
// <config folder> holds maps/live.conf, the map settings with the render
// mask. Every tile over each built chunk column and one block past it is
// written to <tile folder> as BlueMap's file storage lays it out; the reply
// is "MCBENCH_RENDERED <ms>".
class McbenchBlueMapReference {
  static final TileMetaConsumer NO_LOWRES = (x, z, color, height, blockLight) -> {};

  public static void main(String[] args) throws Exception {
    McbenchBake.logFailures();
    ResourcePack resources;
    Biome plains;
    try (var service = McbenchBake.service(args[0])) {
      resources = service.getOrLoadResourcePack();
      plains = service.loadDataPack(List.of()).getBiome(Key.minecraft("plains"));
    }
    TextureGallery gallery;
    try (var in = Files.newInputStream(Path.of("web/maps/live/textures.json"))) { gallery = TextureGallery.readTexturesFile(in); }
    var table = BlockTable.read(Path.of(args[1]));
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
      BlueprintWorld blueprint = BlueprintWorld.read(Path.of(request[0]), table, plains);
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

  // What Minecraft says of each block state: its default state, and how it
  // emits, blocks and passes light (written by McbenchBlockTable.java from
  // the pinned server).
  static final class BlockTable {
    final Map<String, Map<String, String>> defaults = new HashMap<>();
    final Map<String, int[]> light = new HashMap<>(); // emission, lightBlock, skylightDown, shapeOcclusion, full, empty

    static BlockTable read(Path file) throws IOException {
      var table = new BlockTable();
      for (String line : Files.readAllLines(file)) {
        String[] fields = line.split("\t");
        if (fields[0].equals("default")) table.defaults.put(fields[1], McbenchBake.properties(fields[2]));
        else table.light.put(fields[0], new int[] {
          Integer.parseInt(fields[1]), Integer.parseInt(fields[2]), Integer.parseInt(fields[3]),
          Integer.parseInt(fields[4]), Integer.parseInt(fields[5]), Integer.parseInt(fields[6]) });
      }
      return table;
    }
  }

  // A blueprint as a BlueMap world: its blocks at their Minecraft
  // coordinates, plains everywhere, and the light Minecraft's rules give
  // them in the void (sky light from above and from the open sides, block
  // light from emitting blocks, each step losing max(1, the light the next
  // block blocks), stopped by faces Minecraft treats as closed).
  static final class BlueprintWorld implements World {
    static final int MARGIN = 15;
    final Biome biome;
    BlockState[] palette;
    int[][] lightOf; // per palette entry: emission, lightBlock, skylightDown, shapeOcclusion, full, empty
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
      // below passes it on; every other column of the void is open sky.
      // A face Minecraft treats as closed stops it, as it stops spreading light.
      for (int x = minX; x < minX + sizeX; x++) for (int z = minZ; z < minZ + sizeZ; z++) {
        int[] above = lightOf[0];
        for (int y = minY + sizeY - 1; y >= minY; y--) {
          int at = index(x, y, z);
          int[] block = lightOf[blocks[at]];
          if (block[2] == 0 || closed(above, block, 0)) break;
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
    // `direction`: either one's facing side is a full face it shapes light with.
    static boolean closed(int[] source, int[] target, int direction) {
      return (source[3] == 1 && (source[4] >> direction & 1) == 1)
        || (target[3] == 1 && (target[4] >> OPPOSITE[direction] & 1) == 1);
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
          int next = skyLight && direction == 0 && current == 15 && target[2] == 1 ? 15 : current - Math.max(1, target[1]);
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
