import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.sounds.SoundEvent;
import net.minecraft.sounds.SoundSource;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.block.BaseRailBlock;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.EntityBlock;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.level.redstone.CollectingNeighborUpdater;
import net.minecraft.world.level.chunk.LevelChunkSection;
import net.minecraft.world.ticks.LevelChunkTicks;
import net.minecraft.world.ticks.LevelTicks;
import java.lang.reflect.Field;
import java.util.HashMap;
import java.util.HashSet;
import java.util.Map;
import java.util.Set;

// A server level that holds one build and nothing else, for running the
// pinned server's own block code outside a server: placing blocks (with the
// neighbour notifications, onPlace and removal effects Minecraft makes),
// block entities, and scheduled ticks in the server's own tick queues.
//
// Paper's ServerLevel needs a whole server to construct, so this one is
// allocated without its constructor; the fields block code reads (the
// random source, the neighbour updater, the tick queues, Paper's world
// settings) are set here. Blocks live in real LevelChunks and every block
// change, block entity and game event goes through Paper's own Level,
// LevelChunk and ServerLevel code; only the chunk system around them (which
// chunks exist and tick), the light engine and what reaches players or the
// disk (packets, sounds, saving) are stood in for.
public class McbenchLevel extends net.minecraft.server.level.ServerLevel {
  // MCBENCH_ENGINE_STOCK, read once when the engine starts (configure): unset
  // or 0, the engine's fast paths (each one gives exactly what Paper's own
  // code gives: see Section, Neighbors and getBlockState); 1, Paper's own
  // code paths only; check, the fast paths with every fast read compared
  // against Paper's own and a difference a failure.
  static boolean stock, check;

  static void configure() {
    String mode = System.getenv("MCBENCH_ENGINE_STOCK");
    stock = "1".equals(mode);
    check = "check".equals(mode);
    if (mode != null && !mode.isEmpty() && !mode.equals("0") && !stock && !check) throw new IllegalArgumentException("MCBENCH_ENGINE_STOCK is 0, 1 or check, not " + mode);
  }

  // The Bukkit server events are sent to: a CraftServer whose plugin manager
  // hands every event to a manager with no plugins, so each event goes out
  // as it was made, as on the live server.
  static org.bukkit.craftbukkit.CraftServer server;
  // An error Minecraft reported (ServerExceptionEvent), which Paper logs
  // and goes on from: the request it happened in fails with it (reported).
  static Throwable serverException;

  static void reported() {
    Throwable error = serverException;
    serverException = null;
    if (error != null) throw new IllegalStateException("Minecraft reported an error, which Paper logs and goes on from", error);
  }

  static org.bukkit.craftbukkit.CraftServer bukkitServer() throws Exception {
    // Permissions (a player's Bukkit entity subscribes to the defaults) are
    // kept by Paper's own permission manager.
    var permissionsClass = Class.forName("io.papermc.paper.plugin.manager.NormalPaperPermissionManager");
    var made = permissionsClass.getDeclaredConstructor();
    made.setAccessible(true);
    Object permissions = made.newInstance();
    var events = (org.bukkit.plugin.PluginManager) java.lang.reflect.Proxy.newProxyInstance(
      McbenchLevel.class.getClassLoader(), new Class<?>[] { org.bukkit.plugin.PluginManager.class }, (proxy, method, args) -> switch (method.getName()) {
        case "callEvent" -> {
          // Paper reports an error a tick survives (a block entity's, say)
          // with this event, then goes on; a run here stops at it.
          if (args[0] instanceof com.destroystokyo.paper.event.server.ServerExceptionEvent exception && serverException == null) serverException = exception.getException();
          yield null;
        }
        case "hashCode" -> System.identityHashCode(proxy);
        case "equals" -> proxy == args[0];
        case "toString" -> "McbenchLevel plugins";
        case "getPermission", "addPermission", "removePermission", "getDefaultPermissions", "recalculatePermissionDefaults",
          "subscribeToPermission", "unsubscribeFromPermission", "getPermissionSubscriptions", "subscribeToDefaultPerms",
          "unsubscribeFromDefaultPerms", "getDefaultPermSubscriptions", "getPermissions", "addPermissions", "clearPermissions" -> {
          var delegate = permissionsClass.getSuperclass().getDeclaredMethod(method.getName(), method.getParameterTypes());
          delegate.setAccessible(true);
          yield delegate.invoke(permissions, args);
        }
        default -> throw new UnsupportedOperationException("The build level's plugin manager has no " + method.getName());
      });
    var manager = (org.bukkit.plugin.SimplePluginManager) unsafe.allocateInstance(org.bukkit.plugin.SimplePluginManager.class);
    set(manager, org.bukkit.plugin.SimplePluginManager.class, "paperPluginManager", events);
    var craft = (org.bukkit.craftbukkit.CraftServer) unsafe.allocateInstance(org.bukkit.craftbukkit.CraftServer.class);
    set(craft, org.bukkit.craftbukkit.CraftServer.class, "pluginManager", manager);
    // The registries Bukkit's types are read from.
    org.bukkit.craftbukkit.CraftRegistry.setMinecraftRegistry(data.registries().compositeAccess());
    Field bukkit = org.bukkit.Bukkit.class.getDeclaredField("server");
    unsafe.putObject(unsafe.staticFieldBase(bukkit), unsafe.staticFieldOffset(bukkit), craft);
    return craft;
  }

  // The level's chunks: real LevelChunks, made as the chunk cache hands them
  // out (see chunk()).
  final it.unimi.dsi.fastutil.longs.Long2ObjectOpenHashMap<net.minecraft.world.level.chunk.LevelChunk> chunkMap = new it.unimi.dsi.fastutil.longs.Long2ObjectOpenHashMap<>();
  // Each chunk's entity slices, as the chunk system loads them with it.
  final it.unimi.dsi.fastutil.longs.Long2ObjectOpenHashMap<ca.spottedleaf.moonrise.patches.chunk_system.level.entity.ChunkEntitySlices> slices = new it.unimi.dsi.fastutil.longs.Long2ObjectOpenHashMap<>();
  net.minecraft.world.level.entity.EntityTickList entityTickList;
  // The columns holding chunk tickets, and the chunks ticking around them.
  it.unimi.dsi.fastutil.longs.LongOpenHashSet columns, ticking, loaded;

  // Never called: instances come from create().
  private McbenchLevel() {
    super(null, null, null, null, null, null, false, 0, null, false, null, null, null, null);
  }

  static final sun.misc.Unsafe unsafe;
  static {
    try {
      Field field = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
      field.setAccessible(true);
      unsafe = (sun.misc.Unsafe) field.get(null);
    } catch (ReflectiveOperationException error) { throw new ExceptionInInitializerError(error); }
  }

  static void set(Object target, Class<?> owner, String name, Object value) throws ReflectiveOperationException {
    Field field = owner.getDeclaredField(name);
    unsafe.putObject(target, unsafe.objectFieldOffset(field), value);
  }

  static void setInt(Object target, Class<?> owner, String name, int value) throws ReflectiveOperationException {
    unsafe.putInt(target, unsafe.objectFieldOffset(owner.getDeclaredField(name)), value);
  }

  // A level for a frozen import, whose columns the import force-loads as it
  // places (see McbenchEngine.placeAll).
  static McbenchLevel create(long seed) throws Exception {
    var level = allocate();
    level.imports = true;
    level.init(seed, "mcbench_live");
    return level;
  }

  // A level whose `columns` (chunk x, z pairs) hold plugin chunk tickets, as
  // the verifier's are.
  static McbenchLevel create(long seed, java.util.List<int[]> columns) throws Exception {
    var level = allocate();
    level.init(seed, "mcbench_test");
    for (int[] column : columns) level.ticket(column[0], column[1]);
    return level;
  }

  static McbenchLevel allocate() throws Exception {
    if (server == null) {
      server = bukkitServer();
      minecraft = minecraftServer();
      set(server, org.bukkit.craftbukkit.CraftServer.class, "console", minecraft);
      // Paper's server code reaches the plugin manager through its own
      // reference to the CraftServer (portal creation does).
      minecraft.server = server;
    }
    return (McbenchLevel) unsafe.allocateInstance(McbenchLevel.class);
  }

  // A chunk ticket at level 31 (a plugin's or a force load's): its chunk is
  // entity ticking, the ring around it block ticking (scheduled ticks, block
  // events and block entities run there) and the next ring loaded, each a
  // full chunk with its tick queues in the level's.
  // A chunk whose status rises is reported to the entity lookup and moved to
  // moonrise's lists of ticking chunks, as the chunk system does.
  void ticket(int x, int z) {
    if (columns.contains(ChunkPos.asLong(x, z))) return;
    var before = new HashMap<Long, net.minecraft.server.level.FullChunkStatus>();
    for (int dx = -2; dx <= 2; dx++) for (int dz = -2; dz <= 2; dz++) {
      long chunk = ChunkPos.asLong(x + dx, z + dz);
      if (chunkMap.containsKey(chunk)) before.put(chunk, status(chunk));
    }
    columns.add(ChunkPos.asLong(x, z));
    for (int dx = -2; dx <= 2; dx++) for (int dz = -2; dz <= 2; dz++) {
      long chunk = ChunkPos.asLong(x + dx, z + dz);
      loaded.add(chunk);
      if (Math.abs(dx) <= 1 && Math.abs(dz) <= 1) ticking.add(chunk);
    }
    for (int dx = -2; dx <= 2; dx++) for (int dz = -2; dz <= 2; dz++) {
      long key = ChunkPos.asLong(x + dx, z + dz);
      var chunk = chunk(x + dx, z + dz);
      var status = status(key);
      if (before.containsKey(key) && before.get(key) != status) moonrise$getEntityLookup().chunkStatusChange(x + dx, z + dz, status);
      if (status.isOrAfter(net.minecraft.server.level.FullChunkStatus.BLOCK_TICKING)) moonrise$getTickingChunks().add(chunk);
      if (status.isOrAfter(net.minecraft.server.level.FullChunkStatus.ENTITY_TICKING)) moonrise$getEntityTickingChunks().add(chunk);
    }
  }

  // Every chunk's block entities.
  java.util.List<BlockEntity> blockEntities() {
    var all = new java.util.ArrayList<BlockEntity>();
    for (var chunk : chunkMap.values()) all.addAll(chunk.getBlockEntities().values());
    return all;
  }

  net.minecraft.server.level.FullChunkStatus status(long chunk) {
    if (columns.contains(chunk)) return net.minecraft.server.level.FullChunkStatus.ENTITY_TICKING;
    if (ticking.contains(chunk)) return net.minecraft.server.level.FullChunkStatus.BLOCK_TICKING;
    return net.minecraft.server.level.FullChunkStatus.FULL;
  }

  // The chunk at x, z, made when first asked for. Outside the tickets' rings
  // that is a chunk Paper loads on demand: full, empty (the void world), and
  // not ticking.
  @SuppressWarnings("unchecked")
  ca.spottedleaf.concurrentutil.map.ConcurrentLong2ReferenceChainedHashTable<ca.spottedleaf.moonrise.patches.chunk_system.scheduling.NewChunkHolder> chunkHolders() throws ReflectiveOperationException {
    var field = ca.spottedleaf.moonrise.patches.chunk_system.scheduling.ChunkHolderManager.class.getDeclaredField("chunkHolders");
    return (ca.spottedleaf.concurrentutil.map.ConcurrentLong2ReferenceChainedHashTable<ca.spottedleaf.moonrise.patches.chunk_system.scheduling.NewChunkHolder>) unsafe.getObject(moonrise$getChunkTaskScheduler().chunkHolderManager, unsafe.objectFieldOffset(field));
  }

  // The chunk last asked for, which block code asks for again and again
  // (init starts it empty).
  long lastKey;
  net.minecraft.world.level.chunk.LevelChunk lastChunk;
  // The last chunk's sections (see Section), and every chunk's.
  Section[] lastSections;
  it.unimi.dsi.fastutil.longs.Long2ObjectOpenHashMap<Section[]> sectionMap;

  net.minecraft.world.level.chunk.LevelChunk chunk(int x, int z) {
    long key = ChunkPos.asLong(x, z);
    if (key == lastKey) return lastChunk;
    var chunk = chunkMap.get(key);
    if (chunk != null) {
      lastKey = key;
      lastChunk = chunk;
      lastSections = sectionMap.get(key);
      return chunk;
    }
    Section[] sections = null;
    if (stock) {
      chunk = new net.minecraft.world.level.chunk.LevelChunk(this, new ChunkPos(x, z));
    } else {
      // The sections a chunk makes for itself (ChunkAccess.replaceMissingSections), made here to be Sections.
      sections = new Section[getSectionsCount()];
      for (int i = 0; i < sections.length; i++) sections[i] = new Section(sectionFactory, this, new ChunkPos(x, z), getSectionYFromSectionIndex(i));
      chunk = new net.minecraft.world.level.chunk.LevelChunk(this, new ChunkPos(x, z), net.minecraft.world.level.chunk.UpgradeData.EMPTY, new LevelChunkTicks<>(), new LevelChunkTicks<>(), 0L, sections, null, null);
      sectionMap.put(key, sections);
    }
    net.minecraft.world.level.levelgen.Heightmap.primeHeightmaps(chunk, net.minecraft.world.level.chunk.status.ChunkStatus.FULL.heightmapsAfter());
    chunk.setFullStatus(() -> status(key));
    chunk.setLoaded(true);
    chunk.registerTickContainerInLevel(this);
    chunkMap.put(key, chunk);
    getChunkSource().fullChunks.put(key, chunk);
    try {
      var holder = (ca.spottedleaf.moonrise.patches.chunk_system.scheduling.NewChunkHolder) unsafe.allocateInstance(ca.spottedleaf.moonrise.patches.chunk_system.scheduling.NewChunkHolder.class);
      set(holder, ca.spottedleaf.moonrise.patches.chunk_system.scheduling.NewChunkHolder.class, "lastChunkCompletion",
        new ca.spottedleaf.moonrise.patches.chunk_system.scheduling.NewChunkHolder.ChunkCompletion(chunk, net.minecraft.world.level.chunk.status.ChunkStatus.FULL));
      // The chunk's points of interest, loaded with it: a chunk never saved
      // has none (ChunkLoadTask.PoiDataLoadTask).
      set(holder, ca.spottedleaf.moonrise.patches.chunk_system.scheduling.NewChunkHolder.class, "poiChunk",
        ca.spottedleaf.moonrise.patches.chunk_system.level.poi.PoiChunk.empty(this, x, z));
      chunkHolders().put(key, holder);
    } catch (ReflectiveOperationException error) { throw new IllegalStateException(error); }
    // The light a new chunk is generated with (ChunkStatus.LIGHT).
    var starlight = ((Chunks) getChunkSource()).light.starlight();
    starlight.lightChunk(chunk, ca.spottedleaf.moonrise.patches.starlight.light.StarLightEngine.getEmptySectionsForChunk(chunk));
    chunk.setLightCorrect(true);
    moonrise$getLoadedChunks().add(chunk);
    // NewChunkHolder's entity chunk: slices at the chunk's status, loaded
    // into the entity lookup.
    var entities = new ca.spottedleaf.moonrise.patches.chunk_system.level.entity.ChunkEntitySlices(this, x, z, status(key),
      new ca.spottedleaf.moonrise.patches.chunk_system.level.chunk.ChunkData(), ca.spottedleaf.moonrise.common.util.WorldUtil.getMinSection(this), ca.spottedleaf.moonrise.common.util.WorldUtil.getMaxSection(this));
    slices.put(key, entities);
    moonrise$getEntityLookup().entitySectionLoad(x, z, entities);
    lastKey = key;
    lastChunk = chunk;
    lastSections = sections;
    return chunk;
  }

  // A chunk section that keeps every state it holds in an array as well, for
  // Level.getBlockState (see there) to read in place of the palette. The one
  // method that writes a section's states, LevelChunkSection.setBlockState
  // (LevelChunk.setBlockState and nothing else reaches it), also writes the
  // array, before it returns, so every later read sees the state the palette
  // holds. The array is made when the section first holds a block that is
  // not Paper's air: until then every state is that air.
  static final class Section extends LevelChunkSection {
    BlockState[] flat;
    Section(net.minecraft.world.level.chunk.PalettedContainerFactory factory, McbenchLevel level, ChunkPos pos, int sectionY) {
      super(factory, level, pos, sectionY);
    }
    @Override public BlockState setBlockState(int x, int y, int z, BlockState state, boolean useLocks) {
      BlockState old = super.setBlockState(x, y, z, state, useLocks);
      if (flat == null) {
        BlockState air = Blocks.AIR.defaultBlockState();
        if (state == air) return old;
        flat = new BlockState[4096];
        java.util.Arrays.fill(flat, air);
      }
      flat[y << 8 | z << 4 | x] = state;
      return old;
    }
  }

  // CollectingNeighborUpdater, as Paper has it, with NeighborUpdater.executeUpdate
  // without its BlockPhysicsEvent. Paper makes that event for every neighbour
  // update (a CraftBlock for each position, a CraftBlockData for the state)
  // and hands it to the plugin manager, which here takes every event and does
  // nothing with it unless it is a ServerExceptionEvent (see bukkitServer):
  // no plugin listens and an event is never cancelled, so the update is
  // always handleNeighborChanged, as it is below. Everything else is
  // CollectingNeighborUpdater's code line for line: the same stack of
  // updates, the same order, the same limit on chained updates, the same
  // handling of a StackOverflowError and of any other error.
  static final class Neighbors extends CollectingNeighborUpdater {
    private static final org.slf4j.Logger LOGGER = com.mojang.logging.LogUtils.getLogger();
    private final net.minecraft.world.level.Level level;
    private final int maxChainedNeighborUpdates;
    private final java.util.ArrayDeque<Updates> stack = new java.util.ArrayDeque<>();
    private final java.util.List<Updates> addedThisLayer = new java.util.ArrayList<>();
    private int count = 0;
    private java.util.function.Consumer<BlockPos> debugListener;

    Neighbors(net.minecraft.world.level.Level level, int maxChainedNeighborUpdates) {
      super(level, maxChainedNeighborUpdates);
      this.level = level;
      this.maxChainedNeighborUpdates = maxChainedNeighborUpdates;
    }

    @Override public void setDebugListener(java.util.function.Consumer<BlockPos> debugListener) {
      this.debugListener = debugListener;
    }

    @Override public void shapeUpdate(net.minecraft.core.Direction direction, BlockState state, BlockPos pos, BlockPos neighborPos, int flags, int recursionLeft) {
      addAndRun(pos, new ShapeUpdate(direction, state, pos.immutable(), neighborPos.immutable(), flags, recursionLeft));
    }

    @Override public void neighborChanged(BlockPos pos, Block neighborBlock, net.minecraft.world.level.redstone.Orientation orientation) {
      addAndRun(pos, new SimpleNeighborUpdate(pos, neighborBlock, orientation));
    }

    @Override public void neighborChanged(BlockState state, BlockPos pos, Block neighborBlock, net.minecraft.world.level.redstone.Orientation orientation, boolean movedByPiston) {
      addAndRun(pos, new FullNeighborUpdate(state, pos.immutable(), neighborBlock, orientation, movedByPiston));
    }

    @Override public void updateNeighborsAtExceptFromFacing(BlockPos pos, Block block, net.minecraft.core.Direction facing, net.minecraft.world.level.redstone.Orientation orientation) {
      addAndRun(pos, new MultiNeighborUpdate(pos.immutable(), block, orientation, facing));
    }

    private void addAndRun(BlockPos pos, Updates updates) {
      boolean flag = count > 0;
      boolean flag1 = maxChainedNeighborUpdates >= 0 && count >= maxChainedNeighborUpdates;
      count++;
      if (!flag1) {
        if (flag) addedThisLayer.add(updates);
        else stack.push(updates);
      } else if (count - 1 == maxChainedNeighborUpdates) {
        LOGGER.error("Too many chained neighbor updates. Skipping the rest. First skipped position: {}", pos.toShortString());
      }
      if (!flag) runUpdates();
    }

    private void runUpdates() {
      try {
        while (!stack.isEmpty() || !addedThisLayer.isEmpty()) {
          for (int i = addedThisLayer.size() - 1; i >= 0; i--) stack.push(addedThisLayer.get(i));
          addedThisLayer.clear();
          Updates neighborUpdates = stack.peek();
          if (debugListener != null) neighborUpdates.forEachUpdatedPos(debugListener);
          while (addedThisLayer.isEmpty()) {
            if (!neighborUpdates.runNext(level)) {
              stack.pop();
              break;
            }
          }
        }
      } finally {
        stack.clear();
        addedThisLayer.clear();
        count = 0;
      }
    }

    // NeighborUpdater.executeUpdate, without the event.
    static void executeUpdate(net.minecraft.world.level.Level level, BlockState state, BlockPos pos, Block neighborBlock, net.minecraft.world.level.redstone.Orientation orientation, boolean movedByPiston) {
      try {
        state.handleNeighborChanged(level, pos, neighborBlock, orientation, movedByPiston);
      } catch (StackOverflowError error) {
        net.minecraft.world.level.Level.lastPhysicsProblem = pos.immutable();
      } catch (Throwable error) {
        net.minecraft.CrashReport crashReport = net.minecraft.CrashReport.forThrowable(error, "Exception while updating neighbours");
        net.minecraft.CrashReportCategory category = crashReport.addCategory("Block being updated");
        category.setDetail("Source block type", () -> {
          try {
            return String.format(java.util.Locale.ROOT, "ID #%s (%s // %s)", BuiltInRegistries.BLOCK.getKey(neighborBlock), neighborBlock.getDescriptionId(), neighborBlock.getClass().getCanonicalName());
          } catch (Throwable inner) {
            return "ID #" + BuiltInRegistries.BLOCK.getKey(neighborBlock);
          }
        });
        net.minecraft.CrashReportCategory.populateBlockDetails(category, level, pos, state);
        throw new net.minecraft.ReportedException(crashReport);
      }
    }

    interface Updates {
      boolean runNext(net.minecraft.world.level.Level level);
      void forEachUpdatedPos(java.util.function.Consumer<BlockPos> action);
    }

    record FullNeighborUpdate(BlockState state, BlockPos pos, Block block, net.minecraft.world.level.redstone.Orientation orientation, boolean movedByPiston) implements Updates {
      @Override public boolean runNext(net.minecraft.world.level.Level level) {
        executeUpdate(level, state, pos, block, orientation, movedByPiston);
        return false;
      }
      @Override public void forEachUpdatedPos(java.util.function.Consumer<BlockPos> action) { action.accept(pos); }
    }

    static final class MultiNeighborUpdate implements Updates {
      private final BlockPos sourcePos;
      private final Block sourceBlock;
      private net.minecraft.world.level.redstone.Orientation orientation;
      private final net.minecraft.core.Direction skipDirection;
      private int idx = 0;

      MultiNeighborUpdate(BlockPos sourcePos, Block sourceBlock, net.minecraft.world.level.redstone.Orientation orientation, net.minecraft.core.Direction skipDirection) {
        this.sourcePos = sourcePos;
        this.sourceBlock = sourceBlock;
        this.orientation = orientation;
        this.skipDirection = skipDirection;
        if (net.minecraft.world.level.redstone.NeighborUpdater.UPDATE_ORDER[idx] == skipDirection) idx++;
      }

      @Override public boolean runNext(net.minecraft.world.level.Level level) {
        net.minecraft.core.Direction direction = net.minecraft.world.level.redstone.NeighborUpdater.UPDATE_ORDER[idx++];
        BlockPos blockPos = sourcePos.relative(direction);
        BlockState blockState = level.getBlockState(blockPos);
        net.minecraft.world.level.redstone.Orientation orientation = null;
        if (level.enabledFeatures().contains(net.minecraft.world.flag.FeatureFlags.REDSTONE_EXPERIMENTS)) {
          if (this.orientation == null) {
            this.orientation = net.minecraft.world.level.redstone.ExperimentalRedstoneUtils.initialOrientation(level, skipDirection == null ? null : skipDirection.getOpposite(), null);
          }
          orientation = this.orientation.withFront(direction);
        }
        executeUpdate(level, blockState, blockPos, sourceBlock, orientation, false);
        if (idx < net.minecraft.world.level.redstone.NeighborUpdater.UPDATE_ORDER.length && net.minecraft.world.level.redstone.NeighborUpdater.UPDATE_ORDER[idx] == skipDirection) idx++;
        return idx < net.minecraft.world.level.redstone.NeighborUpdater.UPDATE_ORDER.length;
      }

      @Override public void forEachUpdatedPos(java.util.function.Consumer<BlockPos> action) {
        for (net.minecraft.core.Direction direction : net.minecraft.world.level.redstone.NeighborUpdater.UPDATE_ORDER) {
          if (direction != skipDirection) action.accept(sourcePos.relative(direction));
        }
      }
    }

    record ShapeUpdate(net.minecraft.core.Direction direction, BlockState neighborState, BlockPos pos, BlockPos neighborPos, int updateFlags, int updateLimit) implements Updates {
      @Override public boolean runNext(net.minecraft.world.level.Level level) {
        net.minecraft.world.level.redstone.NeighborUpdater.executeShapeUpdate(level, direction, pos, neighborPos, neighborState, updateFlags, updateLimit);
        return false;
      }
      @Override public void forEachUpdatedPos(java.util.function.Consumer<BlockPos> action) { action.accept(pos); }
    }

    record SimpleNeighborUpdate(BlockPos pos, Block block, net.minecraft.world.level.redstone.Orientation orientation) implements Updates {
      @Override public boolean runNext(net.minecraft.world.level.Level level) {
        BlockState blockState = level.getBlockState(pos);
        executeUpdate(level, blockState, pos, block, orientation, false);
        return false;
      }
      @Override public void forEachUpdatedPos(java.util.function.Consumer<BlockPos> action) { action.accept(pos); }
    }
  }

  // Level.getFluidState and LevelChunk.getFluidState, read the same way.
  FluidState empty;

  @Override public FluidState getFluidState(BlockPos pos) {
    if (stock) return super.getFluidState(pos);
    int y = pos.getY();
    FluidState state;
    if (isOutsideBuildHeight(y)) {
      state = empty;
    } else {
      int x = pos.getX() >> 4, z = pos.getZ() >> 4;
      if (ChunkPos.asLong(x, z) != lastKey) chunk(x, z);
      Section section = lastSections[(y >> 4) - getMinSectionY()];
      state = section.hasOnlyAir() ? empty : section.flat[(y & 15) << 8 | (pos.getZ() & 15) << 4 | pos.getX() & 15].getFluidState();
    }
    if (check && state != super.getFluidState(pos)) throw new IllegalStateException("The fast fluid state at " + pos + " is " + state + ", Paper's is " + super.getFluidState(pos));
    return state;
  }

  // Level.getBlockState and LevelChunk.getBlockStateFinal, with the chunk's
  // sections found without Level.getChunk's layers and the state read from the
  // section's array: the same state. (A section with no blocks but air reads
  // as Paper's air, as LevelChunk.getBlockStateFinal reads it.) A chunk not
  // made yet is made, as Level.getBlockState's getChunk does.
  BlockState air, voidAir;
  net.minecraft.world.level.chunk.PalettedContainerFactory sectionFactory;

  @Override public BlockState getBlockState(BlockPos pos) {
    if (stock || captureTreeGeneration) return super.getBlockState(pos);
    int y = pos.getY();
    BlockState state;
    if (isOutsideBuildHeight(y)) {
      state = voidAir;
    } else {
      int x = pos.getX() >> 4, z = pos.getZ() >> 4;
      if (ChunkPos.asLong(x, z) != lastKey) chunk(x, z);
      Section section = lastSections[(y >> 4) - getMinSectionY()];
      state = section.hasOnlyAir() ? air : section.flat[(y & 15) << 8 | (pos.getZ() & 15) << 4 | pos.getX() & 15];
    }
    if (check && state != super.getBlockState(pos)) throw new IllegalStateException("The fast block state at " + pos + " is " + state + ", Paper's is " + super.getBlockState(pos));
    return state;
  }

  // Field initialisers do not run for an allocated instance.
  void init(long seed, String name) throws Exception {
    lastKey = Long.MIN_VALUE;
    set(this, McbenchLevel.class, "chunkMap", new it.unimi.dsi.fastutil.longs.Long2ObjectOpenHashMap<net.minecraft.world.level.chunk.LevelChunk>());
    set(this, McbenchLevel.class, "slices", new it.unimi.dsi.fastutil.longs.Long2ObjectOpenHashMap<ca.spottedleaf.moonrise.patches.chunk_system.level.entity.ChunkEntitySlices>());
    columns = new it.unimi.dsi.fastutil.longs.LongOpenHashSet();
    ticking = new it.unimi.dsi.fastutil.longs.LongOpenHashSet();
    loaded = new it.unimi.dsi.fastutil.longs.LongOpenHashSet();
    sectionMap = new it.unimi.dsi.fastutil.longs.Long2ObjectOpenHashMap<>();
    tickRates = new net.minecraft.world.TickRateManager();
    // Level's own collections, which its constructor makes.
    var level = net.minecraft.world.level.Level.class;
    set(this, level, "blockEntityTickers", new java.util.ArrayList<net.minecraft.world.level.block.entity.TickingBlockEntity>());
    set(this, level, "pendingBlockEntityTickers", new java.util.ArrayList<net.minecraft.world.level.block.entity.TickingBlockEntity>());
    set(this, level, "capturedBlockStates", new java.util.LinkedHashMap<BlockPos, org.bukkit.craftbukkit.block.CraftBlockState>());
    set(this, level, "capturedTileEntities", new java.util.LinkedHashMap<BlockPos, BlockEntity>());
    set(this, level, "chunkPacketBlockController", io.papermc.paper.antixray.ChunkPacketBlockController.NO_OPERATION_INSTANCE);
    set(this, net.minecraft.server.level.ServerLevel.class, "gameEventDispatcher", new net.minecraft.world.level.gameevent.GameEventDispatcher(this));
    // Debug values for subscribed clients: there are none.
    set(this, net.minecraft.server.level.ServerLevel.class, "debugSynchronizers", new net.minecraft.util.debug.LevelDebugSynchronizers(this));
    // The overworld's dimension type from the server's data (mcbench's pack
    // sets its build height), and the height fields Level's constructor
    // derives from it.
    var type = data.registries().compositeAccess().lookupOrThrow(net.minecraft.core.registries.Registries.DIMENSION_TYPE)
      .getOrThrow(net.minecraft.world.level.dimension.BuiltinDimensionTypes.OVERWORLD);
    set(this, level, "dimensionTypeRegistration", type);
    int minY = type.value().minY(), maxY = minY + type.value().height() - 1;
    setInt(this, level, "minY", minY);
    setInt(this, level, "height", type.value().height());
    setInt(this, level, "maxY", maxY);
    setInt(this, level, "minSectionY", net.minecraft.core.SectionPos.blockToSectionCoord(minY));
    setInt(this, level, "maxSectionY", net.minecraft.core.SectionPos.blockToSectionCoord(maxY));
    setInt(this, level, "sectionsCount", net.minecraft.core.SectionPos.blockToSectionCoord(maxY) - net.minecraft.core.SectionPos.blockToSectionCoord(minY) + 1);
    // A new world's data, with the game rules the live server (McbenchLive)
    // or the verifier (McbenchVerify) gives its world: game time 0, noon.
    var gameRules = new net.minecraft.world.level.GameRules(net.minecraft.world.flag.FeatureFlags.VANILLA_SET);
    gameRules.getRule(net.minecraft.world.level.GameRules.RULE_DOMOBSPAWNING).set(false, null);
    gameRules.getRule(net.minecraft.world.level.GameRules.RULE_DAYLIGHT).set(false, null);
    gameRules.getRule(net.minecraft.world.level.GameRules.RULE_WEATHER_CYCLE).set(false, null);
    gameRules.getRule(net.minecraft.world.level.GameRules.RULE_DOFIRETICK).set(false, null);
    gameRules.getRule(net.minecraft.world.level.GameRules.RULE_RANDOMTICKING).set(0, null);
    if (imports) {
      gameRules.getRule(net.minecraft.world.level.GameRules.RULE_SPAWN_RADIUS).set(0, null);
      gameRules.getRule(net.minecraft.world.level.GameRules.RULE_KEEPINVENTORY).set(true, null);
    }
    var worldData = new net.minecraft.world.level.storage.PrimaryLevelData(
      new net.minecraft.world.level.LevelSettings(name, net.minecraft.world.level.GameType.SURVIVAL, false, net.minecraft.world.Difficulty.PEACEFUL, false, gameRules, net.minecraft.world.level.WorldDataConfiguration.DEFAULT),
      new net.minecraft.world.level.levelgen.WorldOptions(seed, false, false), net.minecraft.world.level.storage.PrimaryLevelData.SpecialWorldProperty.NONE, com.mojang.serialization.Lifecycle.stable());
    worldData.setDayTime(6000);
    set(this, level, "levelData", worldData);
    set(this, net.minecraft.server.level.ServerLevel.class, "serverLevelData", worldData);
    unsafe.putBoolean(this, unsafe.objectFieldOffset(net.minecraft.server.level.ServerLevel.class.getDeclaredField("tickTime")), true);
    set(this, net.minecraft.server.level.ServerLevel.class, "server", minecraft);
    // The rest of what Level's and ServerLevel's constructors make that
    // block and entity code reach.
    set(this, level, "thread", Thread.currentThread());
    set(this, level, "dimension", net.minecraft.resources.ResourceKey.create(net.minecraft.core.registries.Registries.DIMENSION, net.minecraft.resources.ResourceLocation.withDefaultNamespace(name)));
    dimensions.put(dimension(), this);
    set(this, level, "biomeManager", new net.minecraft.world.level.biome.BiomeManager(this, net.minecraft.world.level.biome.BiomeManager.obfuscateSeed(seed)));
    set(this, level, "damageSources", new net.minecraft.world.damagesource.DamageSources(data.registries().compositeAccess()));
    set(this, level, "explosionDensityCache", new HashMap<>());
    set(this, net.minecraft.server.level.ServerLevel.class, "pathTypesByPosCache", new net.minecraft.world.level.pathfinder.PathTypeCache());
    set(this, net.minecraft.server.level.ServerLevel.class, "sleepStatus", new net.minecraft.server.players.SleepStatus());
    set(this, net.minecraft.server.level.ServerLevel.class, "customSpawners", java.util.List.of());
    set(this, net.minecraft.server.level.ServerLevel.class, "uuid", java.util.UUID.nameUUIDFromBytes(name.getBytes(java.nio.charset.StandardCharsets.UTF_8)));
    set(this, net.minecraft.server.level.ServerLevel.class, "simpleRandom", new ca.spottedleaf.moonrise.common.util.SimpleThreadUnsafeRandom(seed));
    for (String list : new String[] { "loadedChunks", "tickingChunks", "entityTickingChunks", "playerTickingChunks" }) {
      set(this, net.minecraft.server.level.ServerLevel.class, list, new ca.spottedleaf.moonrise.common.list.ReferenceList<>(new net.minecraft.world.level.chunk.LevelChunk[0]));
    }
    set(this, net.minecraft.server.level.ServerLevel.class, "playerTickingRequests", new it.unimi.dsi.fastutil.longs.Long2IntOpenHashMap());
    set(this, net.minecraft.world.level.Level.class, "registryAccess", data.registries().compositeAccess());
    set(this, level, "palettedContainerFactory", net.minecraft.world.level.chunk.PalettedContainerFactory.create(data.registries().compositeAccess()));
    sectionFactory = net.minecraft.world.level.chunk.PalettedContainerFactory.create(data.registries().compositeAccess());
    air = Blocks.AIR.defaultBlockState();
    voidAir = Blocks.VOID_AIR.defaultBlockState();
    empty = net.minecraft.world.level.material.Fluids.EMPTY.defaultFluidState();
    set(this, net.minecraft.world.level.Level.class, "random", net.minecraft.util.RandomSource.create(seed));
    // A new world's random sequences (loot rolls), from the world seed.
    set(this, net.minecraft.server.level.ServerLevel.class, "randomSequences", new net.minecraft.world.RandomSequences(seed));
    set(this, net.minecraft.world.level.Level.class, "threadSafeRandom", net.minecraft.util.RandomSource.createThreadSafe());
    set(this, net.minecraft.world.level.Level.class, "neighborUpdater", stock ? new CollectingNeighborUpdater(this, 1_000_000) : new Neighbors(this, 1_000_000));
    set(this, net.minecraft.server.level.ServerLevel.class, "blockTicks", new LevelTicks<Block>(this::ticks));
    set(this, net.minecraft.server.level.ServerLevel.class, "fluidTicks", new LevelTicks<net.minecraft.world.level.material.Fluid>(this::ticks));
    set(this, net.minecraft.server.level.ServerLevel.class, "blockEvents", new it.unimi.dsi.fastutil.objects.ObjectLinkedOpenHashSet<net.minecraft.world.level.BlockEventData>());
    set(this, net.minecraft.server.level.ServerLevel.class, "blockEventsToReschedule", new java.util.ArrayList<net.minecraft.world.level.BlockEventData>(64));
    // Paper's world settings at their defaults, as a fresh world writes them.
    // Spigot's world settings at the defaults spigot.yml is written with
    // (hopper timing, item merge radius...): read from a configuration that
    // holds none.
    // Spigot writes the filled-in settings back out: to the engine's folder.
    if (org.spigotmc.SpigotConfig.config == null) {
      org.spigotmc.SpigotConfig.config = new org.bukkit.configuration.file.YamlConfiguration();
      Field file = org.spigotmc.SpigotConfig.class.getDeclaredField("CONFIG_FILE");
      unsafe.putObject(unsafe.staticFieldBase(file), unsafe.staticFieldOffset(file), new java.io.File("spigot.yml"));
    }
    var spigot = new org.spigotmc.SpigotWorldConfig("mcbench");
    var constructor = io.papermc.paper.configuration.WorldConfiguration.class.getDeclaredConstructor(org.spigotmc.SpigotWorldConfig.class, net.minecraft.resources.ResourceLocation.class);
    constructor.setAccessible(true);
    set(this, net.minecraft.world.level.Level.class, "spigotConfig", spigot);
    var callbacks = Class.forName("net.minecraft.server.level.ServerLevel$EntityCallbacks").getDeclaredConstructor(net.minecraft.server.level.ServerLevel.class);
    callbacks.setAccessible(true);
    @SuppressWarnings("unchecked") var levelCallbacks = (net.minecraft.world.level.entity.LevelCallback<Entity>) callbacks.newInstance(this);
    set(this, net.minecraft.world.level.Level.class, "entityLookup", new Entities(this, levelCallbacks));
    entityTickList = new net.minecraft.world.level.entity.EntityTickList();
    set(this, net.minecraft.server.level.ServerLevel.class, "entityTickList", entityTickList);
    set(this, net.minecraft.server.level.ServerLevel.class, "players", new java.util.ArrayList<net.minecraft.server.level.ServerPlayer>());
    set(this, net.minecraft.server.level.ServerLevel.class, "navigatingMobs", new it.unimi.dsi.fastutil.objects.ObjectOpenHashSet<net.minecraft.world.entity.Mob>());
    set(this, net.minecraft.server.level.ServerLevel.class, "waypointManager", new net.minecraft.server.waypoints.ServerWaypointManager());
    set(this, net.minecraft.server.level.ServerLevel.class, "nearbyPlayers", new ca.spottedleaf.moonrise.common.misc.NearbyPlayers(this));
    set(this, net.minecraft.server.level.ServerLevel.class, "dragonParts", new it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap<>());
    set(this, McbenchLevel.class, "border", new net.minecraft.world.level.border.WorldBorder());
    // moonrise's chunk holders, which Starlight finds chunks through: one
    // per chunk, holding it complete (see chunk()). Nothing here schedules
    // chunk work.
    var scheduler = unsafe.allocateInstance(ca.spottedleaf.moonrise.patches.chunk_system.scheduling.ChunkTaskScheduler.class);
    var holders = unsafe.allocateInstance(ca.spottedleaf.moonrise.patches.chunk_system.scheduling.ChunkHolderManager.class);
    set(holders, ca.spottedleaf.moonrise.patches.chunk_system.scheduling.ChunkHolderManager.class, "chunkHolders", new ca.spottedleaf.concurrentutil.map.ConcurrentLong2ReferenceChainedHashTable<>());
    set(scheduler, ca.spottedleaf.moonrise.patches.chunk_system.scheduling.ChunkTaskScheduler.class, "chunkHolderManager", holders);
    set(this, net.minecraft.server.level.ServerLevel.class, "chunkTaskScheduler", scheduler);
    var cache = (Chunks) unsafe.allocateInstance(Chunks.class);
    cache.level = this;
    set(cache, net.minecraft.server.level.ServerChunkCache.class, "level", this);
    // The chunks at full status, which Bukkit lists as loaded.
    set(cache, net.minecraft.server.level.ServerChunkCache.class, "fullChunks", new ca.spottedleaf.concurrentutil.map.ConcurrentLong2ReferenceChainedHashTable<net.minecraft.world.level.chunk.LevelChunk>());
    cache.light = (Light) unsafe.allocateInstance(Light.class);
    cache.light.pending = new java.util.LinkedHashMap<>();
    set(cache.light, net.minecraft.world.level.lighting.LevelLightEngine.class, "levelHeightAccessor", this);
    set(cache.light, net.minecraft.world.level.lighting.LevelLightEngine.class, "lightEngine", new ca.spottedleaf.moonrise.patches.starlight.light.StarLightInterface(cache, true, true, cache.light));
    var tracking = unsafe.allocateInstance(Tracking.class);
    set(cache, net.minecraft.server.level.ServerChunkCache.class, "chunkMap", tracking);
    set(this, net.minecraft.server.level.ServerLevel.class, "chunkSource", cache);
    // The level's points of interest (ServerLevel.getPoiManager is the chunk
    // map's), made before any chunk is (see chunk()).
    set(tracking, net.minecraft.server.level.ChunkMap.class, "poiManager", new Pois(this));
    // What ServerLevel's constructor makes for a new overworld that mobs
    // reach: the raids (none: no player brings one), the structures
    // (Structures) and the portal forcer.
    set(this, net.minecraft.server.level.ServerLevel.class, "raids", new net.minecraft.world.entity.raid.Raids());
    set(this, net.minecraft.server.level.ServerLevel.class, "structureManager", new Structures(this, worldData.worldGenOptions()));
    set(this, net.minecraft.server.level.ServerLevel.class, "portalForcer", new net.minecraft.world.level.portal.PortalForcer(this));
    var paper = constructor.newInstance(spigot, net.minecraft.resources.ResourceLocation.withDefaultNamespace("mcbench_live"));
    defaults(paper);
    set(this, net.minecraft.world.level.Level.class, "paperConfig", paper);
    // The Bukkit world, made from Paper's settings (void damage, spawn
    // limits) as Level's constructor makes it.
    set(this, net.minecraft.world.level.Level.class, "world", new org.bukkit.craftbukkit.CraftWorld(this, null, null, org.bukkit.World.Environment.NORMAL));
  }

  // Paper's settings sections are filled in by its configuration loader, not
  // their constructors: each one is made here with its default values.
  static void defaults(Object part) throws Exception {
    for (Class<?> type = part.getClass(); type != null && type != Object.class; type = type.getSuperclass()) {
      for (Field field : type.getDeclaredFields()) {
        // Inner sections refer back to their parent through a synthetic field.
        if (java.lang.reflect.Modifier.isStatic(field.getModifiers()) || field.isSynthetic()) continue;
        if (!io.papermc.paper.configuration.ConfigurationPart.class.isAssignableFrom(field.getType())) continue;
        field.setAccessible(true);
        Object value = field.get(part);
        if (value == null) {
          var make = field.getType().getDeclaredConstructors()[0];
          make.setAccessible(true);
          value = make.getParameterCount() == 0 ? make.newInstance() : make.newInstance(part);
          unsafe.putObject(part, unsafe.objectFieldOffset(field), value);
        }
        defaults(value);
      }
    }
  }

  // Entities, kept as Paper keeps them: moonrise's EntityLookup with
  // ServerLevel's own callbacks (which put ticking entities in the level's
  // tick list). ServerEntityLookup is final, so this is it method for method,
  // with the chunk system behind it stood in for: each chunk's entity slices
  // are made with the chunk (see chunk()).
  static final class Entities extends ca.spottedleaf.moonrise.patches.chunk_system.level.entity.EntityLookup {
    final McbenchLevel level;
    final ca.spottedleaf.moonrise.common.list.ReferenceList<Entity> trackerEntities = new ca.spottedleaf.moonrise.common.list.ReferenceList<>(new Entity[0]);
    Entities(McbenchLevel level, net.minecraft.world.level.entity.LevelCallback<Entity> callbacks) {
      super(level, callbacks);
      this.level = level;
    }
    @Override protected Boolean blockTicketUpdates() { return Boolean.FALSE; }
    @Override protected void setBlockTicketUpdates(Boolean value) {}
    @Override protected void checkThread(int x, int z, String reason) { ca.spottedleaf.moonrise.common.util.TickThread.ensureTickThread(level, x, z, reason); }
    @Override protected void checkThread(Entity entity, String reason) { ca.spottedleaf.moonrise.common.util.TickThread.ensureTickThread(entity, reason); }
    @Override protected ca.spottedleaf.moonrise.patches.chunk_system.level.entity.ChunkEntitySlices createEntityChunk(int x, int z, boolean transientChunk) {
      level.chunk(x, z);
      return level.slices.get(ChunkPos.asLong(x, z));
    }
    @Override protected void onEmptySlices(int x, int z) {}
    @Override protected void entitySectionChangeCallback(Entity entity, int oldX, int oldY, int oldZ, int x, int y, int z) {
      if (entity instanceof net.minecraft.server.level.ServerPlayer player) level.moonrise$getNearbyPlayers().tickPlayer(player);
      pearls(entity);
      ca.spottedleaf.moonrise.common.PlatformHooks.get().entityMove(entity, ca.spottedleaf.moonrise.common.util.CoordinateUtils.getChunkSectionKey(oldX, oldY, oldZ), ca.spottedleaf.moonrise.common.util.CoordinateUtils.getChunkSectionKey(x, y, z));
    }
    @Override protected void addEntityCallback(Entity entity) {
      if (entity instanceof net.minecraft.server.level.ServerPlayer player) level.moonrise$getNearbyPlayers().addPlayer(player);
      pearls(entity);
      entity.registerScheduler();
    }
    @Override protected void removeEntityCallback(Entity entity) {
      if (entity instanceof net.minecraft.server.level.ServerPlayer player) level.moonrise$getNearbyPlayers().removePlayer(player);
      pearls(entity);
    }
    @Override protected void entityStartLoaded(Entity entity) { trackerEntities.add(entity); }
    @Override protected void entityEndLoaded(Entity entity) { trackerEntities.remove(entity); }
    @Override protected void entityStartTicking(Entity entity) {}
    @Override protected void entityEndTicking(Entity entity) {}
    @Override protected boolean screenEntity(Entity entity, boolean fromDisk, boolean event) { return ca.spottedleaf.moonrise.common.PlatformHooks.get().screenEntity(level, entity, fromDisk, event); }
    // A thrown ender pearl keeps its chunk loaded with a ticket of its own.
    void pearls(Entity entity) {
      if (entity instanceof net.minecraft.world.entity.projectile.ThrownEnderpearl && ca.spottedleaf.moonrise.common.PlatformHooks.get().addTicketForEnderPearls(level)) {
        throw new UnsupportedOperationException("Ender pearl chunk tickets are not modelled");
      }
    }
  }

  // The level's points of interest (beds, bells, workstations, beehives,
  // lightning rods, portals...): Paper's own PoiManager, made as ChunkMap
  // makes it, which a block change updates (ServerLevel.updatePOIOnBlockStateChange)
  // and villagers, bees and golems read. Its sections live in moonrise's POI
  // chunks, one in each chunk's holder (see chunk()), empty as Paper loads it
  // for a chunk never saved. A section read in a chunk the level does not
  // hold is one Paper loads on demand (ChunkHolderManager.loadPoiChunk): a
  // chunk never saved, so it holds no points of interest, and none is ever
  // added there (blocks change only in chunks the level holds).
  static final class Pois extends net.minecraft.world.entity.ai.village.poi.PoiManager {
    final McbenchLevel level;
    Pois(McbenchLevel level) {
      super(new net.minecraft.world.level.chunk.storage.RegionStorageInfo(storage.getLevelId(), level.dimension(), "poi"),
        storage.getDimensionPath(level.dimension()).resolve("poi"), minecraft.getFixerUpper(), minecraft.forceSynchronousWrites(),
        level.registryAccess(), minecraft, level);
      this.level = level;
    }
    boolean held(long section) {
      return level.moonrise$getChunkTaskScheduler().chunkHolderManager.getChunkHolder(net.minecraft.core.SectionPos.x(section), net.minecraft.core.SectionPos.z(section)) != null;
    }
    @Override public java.util.Optional<net.minecraft.world.entity.ai.village.poi.PoiSection> getOrLoad(long section) {
      return held(section) ? super.getOrLoad(section) : java.util.Optional.empty();
    }
    @Override protected net.minecraft.world.entity.ai.village.poi.PoiSection getOrCreate(long section) {
      if (!held(section)) throw new IllegalStateException("A point of interest in a chunk the level does not hold: " + net.minecraft.core.SectionPos.of(section));
      return super.getOrCreate(section);
    }
  }

  // The level's structures, ServerLevel's StructureManager: what a chunk's
  // structure references say, none in a void world (a cat's or a variant's
  // spawn conditions ask it). The StructureCheck behind it reads saved
  // chunks and the generator to find structure starts, which only world
  // generation and locating structures do: generate-structures is off, so
  // ServerLevel.findNearestMapStructure looks for none, and nothing here
  // generates a chunk.
  static final class Structures extends net.minecraft.world.level.StructureManager {
    Structures(McbenchLevel level, net.minecraft.world.level.levelgen.WorldOptions options) {
      super(level, options, null);
    }
    @Override public net.minecraft.world.level.levelgen.structure.StructureCheckResult checkStructurePresence(ChunkPos chunk,
      net.minecraft.world.level.levelgen.structure.Structure structure, net.minecraft.world.level.levelgen.structure.placement.StructurePlacement placement, boolean skipKnownStructures) {
      throw new UnsupportedOperationException("Structure starts are not modelled");
    }
    @Override public void addReference(net.minecraft.world.level.levelgen.structure.StructureStart start) {
      throw new UnsupportedOperationException("Structure starts are not modelled");
    }
  }

  // The chunk cache, as ServerChunkCache answers block code: the level's
  // chunks, made when first asked for (Paper loads one on demand).
  static final class Chunks extends net.minecraft.server.level.ServerChunkCache {
    McbenchLevel level;
    Light light;
    private Chunks() { super(null, null, null, null, null, null, 0, 0, false, null, null); }
    @Override public net.minecraft.world.level.chunk.LevelChunk getChunkAtIfLoadedImmediately(int x, int z) { return level.chunkMap.get(ChunkPos.asLong(x, z)); }
    @Override public net.minecraft.world.level.chunk.LevelChunk getChunkAtIfCachedImmediately(int x, int z) { return level.chunkMap.get(ChunkPos.asLong(x, z)); }
    @Override public net.minecraft.world.level.chunk.LevelChunk getChunkNow(int x, int z) { return level.chunkMap.get(ChunkPos.asLong(x, z)); }
    @Override public net.minecraft.world.level.chunk.ChunkAccess getChunk(int x, int z, net.minecraft.world.level.chunk.status.ChunkStatus status, boolean create) {
      return create ? level.chunk(x, z) : level.chunkMap.get(ChunkPos.asLong(x, z));
    }
    @Override public boolean hasChunk(int x, int z) { return level.chunkMap.containsKey(ChunkPos.asLong(x, z)); }
    @Override public net.minecraft.world.level.chunk.LightChunk getChunkForLighting(int x, int z) { return level.chunkMap.get(ChunkPos.asLong(x, z)); }
    @Override public boolean isPositionTicking(long chunk) { return level.ticks(chunk); }
    @Override public net.minecraft.server.level.ThreadedLevelLightEngine getLightEngine() { return light; }
    @Override public void onSectionEmptinessChanged(int x, int y, int z, boolean empty) {}
    // Changes, light and entities are sent to players' clients: none here.
    @Override public void blockChanged(BlockPos pos) {}
    @Override public void onLightUpdate(net.minecraft.world.level.LightLayer layer, net.minecraft.core.SectionPos pos) {}
    @Override public void addEntity(Entity entity) {}
    @Override public void removeEntity(Entity entity) {}
  }

  // The chunk map, which code reaches for to send packets to the players
  // tracking an entity: there are none.
  static final class Tracking extends net.minecraft.server.level.ChunkMap {
    private Tracking() { super(null, null, null, null, null, null, null, null, null, null, null, 0, false); }
    @Override public void sendToTrackingPlayers(Entity entity, net.minecraft.network.protocol.Packet<? super net.minecraft.network.protocol.game.ClientGamePacketListener> packet) {}
    @Override public void sendToTrackingPlayersFiltered(Entity entity, net.minecraft.network.protocol.Packet<? super net.minecraft.network.protocol.game.ClientGamePacketListener> packet, java.util.function.Predicate<net.minecraft.server.level.ServerPlayer> filter) {}
    @Override protected void sendToTrackingPlayersAndSelf(Entity entity, net.minecraft.network.protocol.Packet<? super net.minecraft.network.protocol.game.ClientGamePacketListener> packet) {}
  }

  // The light engine: Paper's own (moonrise's Starlight) behind the
  // ThreadedLevelLightEngine block code reports changes to. Paper gathers
  // each chunk's changes into one task for a worker thread, so when a tick
  // reads light its last changes may or may not be lit yet. Here each
  // chunk's changes are gathered the same way and lit before light is next
  // read (or a chunk is saved), which gives the light Paper's worker settles
  // to: light levels do not depend on when or in what order changes are lit.
  static final class Light extends net.minecraft.server.level.ThreadedLevelLightEngine {
    private Light() { super(null, null, false, null, null); }
    // Each chunk's unlit changes: its changed positions and section emptiness
    // (a ServerChunkTasks' changedPositions and changedSectionSet).
    record Pending(java.util.Set<BlockPos> positions, Boolean[] sections) {}
    java.util.LinkedHashMap<Long, Pending> pending;
    Pending pending(int x, int z) {
      var starlight = starlight();
      return pending.computeIfAbsent(ChunkPos.asLong(x, z), unused -> new Pending(new HashSet<>(), new Boolean[starlight.maxSection - starlight.minSection + 1]));
    }
    // Lights every chunk's gathered changes.
    void flush() {
      while (!pending.isEmpty()) {
        var first = pending.entrySet().iterator().next();
        pending.remove(first.getKey());
        boolean sections = java.util.Arrays.stream(first.getValue().sections()).anyMatch(java.util.Objects::nonNull);
        lightChunk(ChunkPos.getX(first.getKey()), ChunkPos.getZ(first.getKey()), first.getValue().positions(), sections ? first.getValue().sections() : null);
      }
    }
    @Override public net.minecraft.world.level.lighting.LayerLightEventListener getLayerListener(net.minecraft.world.level.LightLayer layer) {
      flush();
      return super.getLayerListener(layer);
    }
    @Override public int getRawBrightness(BlockPos pos, int darkening) {
      flush();
      return super.getRawBrightness(pos, darkening);
    }
    ca.spottedleaf.moonrise.patches.starlight.light.StarLightInterface starlight() { return starlight$getLightEngine(); }
    // ServerLightQueue's chunk task (LightQueue.ChunkTasks.runTasks), run now.
    void lightChunk(int x, int z, java.util.Set<BlockPos> positions, Boolean[] sections) {
      var starlight = starlight();
      var sky = starlight.getSkyLightEngine();
      var block = starlight.getBlockLightEngine();
      try {
        sky.blocksChangedInChunk(starlight.getLightAccess(), x, z, positions, sections);
        block.blocksChangedInChunk(starlight.getLightAccess(), x, z, positions, sections);
      } finally {
        starlight.releaseSkyLightEngine(sky);
        starlight.releaseBlockLightEngine(block);
      }
    }
    @Override public void checkBlock(BlockPos pos) {
      pending(pos.getX() >> 4, pos.getZ() >> 4).positions().add(pos.immutable());
    }
    @Override public void updateSectionStatus(net.minecraft.core.SectionPos pos, boolean empty) {
      pending(pos.getX(), pos.getZ()).sections()[pos.getY() - starlight().minSection] = empty;
    }
  }


  @Override public net.minecraft.world.level.chunk.LevelChunk getChunkIfLoaded(int x, int z) { return chunkMap.get(ChunkPos.asLong(x, z)); }
  // Entity sections load with their chunk.
  @Override public boolean areEntitiesLoaded(long chunk) { return chunkMap.containsKey(chunk); }

  // The world border at its default size, as in a world no one has changed.
  final net.minecraft.world.level.border.WorldBorder border = new net.minecraft.world.level.border.WorldBorder();
  @Override public net.minecraft.world.level.border.WorldBorder getWorldBorder() { return border; }

  // The server's data (see McbenchEngine.boot).
  static net.minecraft.server.WorldStem data;
  // The engine's world folder (McbenchEngine.boot).
  static net.minecraft.world.level.storage.LevelStorageSource.LevelStorageAccess storage;

  // The server, for what block code asks of it: recipes, loot tables,
  // brewing and fuels, as the server holds them.
  static net.minecraft.server.MinecraftServer minecraft;
  @Override public net.minecraft.server.MinecraftServer getServer() { return minecraft; }

  // MinecraftServer.getGameRules is the overworld's, and the server holds no
  // levels here (see below). A trial spawner asks it for spawner_blocks_enabled
  // (TrialSpawner.canSpawnInLevel), which Paper's overworld holds at its default:
  // nothing sets it. (Never constructed: Unsafe makes it.)
  static final class EngineServer extends net.minecraft.server.dedicated.DedicatedServer {
    static final net.minecraft.world.level.GameRules DEFAULT_RULES = new net.minecraft.world.level.GameRules(net.minecraft.world.flag.FeatureFlags.VANILLA_SET);
    EngineServer() { super(null, null, null, null, null, null, null, null, null); }
    @Override public net.minecraft.world.level.GameRules getGameRules() { return DEFAULT_RULES; }
    // The level a dimension names, as the server's own map of its levels
    // answers: a villager finds the level of its job site and bed this way
    // (a GlobalPos). The levels stay out of that map, whose levels the
    // server's mid-tick chunk work goes over (see minecraftServer).
    @Override public net.minecraft.server.level.ServerLevel getLevel(net.minecraft.resources.ResourceKey<net.minecraft.world.level.Level> dimension) { return dimensions.get(dimension); }
  }

  // Each dimension's level: the last one made with its name (see init).
  static final Map<net.minecraft.resources.ResourceKey<net.minecraft.world.level.Level>, McbenchLevel> dimensions = new HashMap<>();

  static net.minecraft.server.MinecraftServer minecraftServer() throws Exception {
    var server = (net.minecraft.server.MinecraftServer) unsafe.allocateInstance(EngineServer.class);
    set(server, net.minecraft.server.MinecraftServer.class, "resources",
      new net.minecraft.server.MinecraftServer.ReloadableResources(data.resourceManager(), data.dataPackResources()));
    set(server, net.minecraft.server.MinecraftServer.class, "registries", data.registries());
    // Paper reaches the server from anywhere through MinecraftServer.getServer().
    Field instance = net.minecraft.server.MinecraftServer.class.getDeclaredField("SERVER");
    unsafe.putObject(unsafe.staticFieldBase(instance), unsafe.staticFieldOffset(instance), server);
    // server.properties at its defaults; Spigot's world settings read the
    // view and simulation distances from it, which no block reads.
    var settings = (net.minecraft.server.dedicated.DedicatedServerSettings) unsafe.allocateInstance(net.minecraft.server.dedicated.DedicatedServerSettings.class);
    set(settings, net.minecraft.server.dedicated.DedicatedServerSettings.class, "properties",
      new net.minecraft.server.dedicated.DedicatedServerProperties(new java.util.Properties(), new joptsimple.OptionParser().parse()));
    set(server, net.minecraft.server.dedicated.DedicatedServer.class, "settings", settings);
    // The main world's data, as a new server makes it from those properties
    // (survival, easy): players joining take its game mode.
    var properties = settings.getProperties();
    set(server, net.minecraft.server.MinecraftServer.class, "worldData", new net.minecraft.world.level.storage.PrimaryLevelData(
      new net.minecraft.world.level.LevelSettings("world", properties.gameMode.get(), properties.hardcore, properties.difficulty.get(), false,
        new net.minecraft.world.level.GameRules(net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS), net.minecraft.world.level.WorldDataConfiguration.DEFAULT),
      properties.worldOptions, net.minecraft.world.level.storage.PrimaryLevelData.SpecialWorldProperty.NONE, com.mojang.serialization.Lifecycle.stable()));
    // Brewing recipes and furnace fuels, as the server makes them for its
    // enabled features.
    // No players are on, and no client subscribes to debug values.
    var players = unsafe.allocateInstance(net.minecraft.server.dedicated.DedicatedPlayerList.class);
    set(players, net.minecraft.server.players.PlayerList.class, "players", new java.util.concurrent.CopyOnWriteArrayList<net.minecraft.server.level.ServerPlayer>());
    set(players, net.minecraft.server.players.PlayerList.class, "playersByUUID", new HashMap<java.util.UUID, net.minecraft.server.level.ServerPlayer>());
    set(players, net.minecraft.server.players.PlayerList.class, "playersByName", new HashMap<String, net.minecraft.server.level.ServerPlayer>());
    // A player made for inputs (McbenchExperiment.inputPlayer) gets the
    // statistics and advancements its world folder holds for it: none.
    set(players, net.minecraft.server.players.PlayerList.class, "server", server);
    // Operators, from the engine folder's ops.json, which holds none; Bukkit
    // asks the same list.
    set(players, net.minecraft.server.players.PlayerList.class, "ops", new net.minecraft.server.players.ServerOpList(net.minecraft.server.players.PlayerList.OPLIST_FILE, new net.minecraft.server.notifications.EmptyNotificationService()));
    set(McbenchLevel.server, org.bukkit.craftbukkit.CraftServer.class, "playerList", players);
    set(server, net.minecraft.server.MinecraftServer.class, "storageSource", storage);
    set(server, net.minecraft.server.MinecraftServer.class, "fixerUpper", net.minecraft.util.datafix.DataFixers.getDataFixer());
    set(server, net.minecraft.server.MinecraftServer.class, "playerList", players);
    set(server, net.minecraft.server.MinecraftServer.class, "debugSubscribers", new net.minecraft.util.debug.ServerDebugSubscribers(server));
    set(server, net.minecraft.server.MinecraftServer.class, "scoreboard", new net.minecraft.server.ServerScoreboard(server));
    // Bukkit's view of it, which statistics awarded to players update.
    McbenchLevel.server.scoreboardManager = new org.bukkit.craftbukkit.scoreboard.CraftScoreboardManager(server, server.getScoreboard());
    // Mid-tick chunk work (MinecraftServer.moonrise$executeMidTickTasks) goes
    // over the server's levels: none have any.
    set(server, net.minecraft.server.MinecraftServer.class, "levels", new java.util.LinkedHashMap<>());
    server.potionBrewing = net.minecraft.world.item.alchemy.PotionBrewing.bootstrap(net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS);
    set(server, net.minecraft.server.MinecraftServer.class, "fuelValues", net.minecraft.world.level.block.entity.FuelValues.vanillaBurnTimes(
      data.registries().compositeAccess(), net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS));
    return server;
  }

  // The level is the overworld (its dimension type is, see init): ServerLevel
  // reads this from the storage access Paper gives a loaded world, which is
  // not here. Fire looks for nether portals when it is placed in the
  // overworld.
  @Override public net.minecraft.resources.ResourceKey<net.minecraft.world.level.dimension.LevelStem> getTypeKey() { return net.minecraft.world.level.dimension.LevelStem.OVERWORLD; }
  @Override public org.bukkit.craftbukkit.CraftServer getCraftServer() { return server; }
  @Override public net.minecraft.world.flag.FeatureFlagSet enabledFeatures() { return net.minecraft.world.flag.FeatureFlags.VANILLA_SET; }

  // Whether a chunk's blocks tick (NewChunkHolder.isTickingReady).
  boolean ticks(long chunk) { return ticking.contains(chunk); }
  @Override public boolean shouldTickBlocksAt(long chunk) { return ticks(chunk); }

  // Ticks run normally: not frozen, not stepping.
  net.minecraft.world.TickRateManager tickRates;
  @Override public net.minecraft.world.TickRateManager tickRateManager() { return tickRates; }

  // One server tick of ServerLevel.tick, with the parts that reach blocks
  // and entities in its order: the game time, scheduled block then fluid
  // ticks, the raids, the chunk cache's tick, block events, entities, then
  // block entities. The rest has
  // nothing to act on here: the weather and daylight cycles are off, random
  // ticks are 0, there are no players, and nothing is spawned.
  void tick() {
    // ServerLevel.handlingTick: true from the tick's start through its block
    // events (pistons read it).
    setHandlingTick(true);
    net.minecraft.server.MinecraftServer.currentTick++;
    updateSkyBrightness();
    tickTime();
    var environment = paperConfig().environment;
    ((LevelTicks<Block>) getBlockTicks()).tick(getGameTime(), environment.maxBlockTicks, this::tickBlock);
    ((LevelTicks<net.minecraft.world.level.material.Fluid>) getFluidTicks()).tick(getGameTime(), environment.maxFluidTicks, this::tickFluid);
    getRaids().tick(this);
    // ServerChunkCache.tickChunks seeds its spawning-chunk shuffle from the
    // level's random every tick, spawning or not; the chunk ticks themselves
    // (precipitation, random ticks, here 0) draw from simpleRandom.
    random.nextLong();
    // ChunkMap.tick: the points of interest's village distances.
    getPoiManager().tick(() -> true);
    runBlockEvents();
    setHandlingTick(false);
    // ServerLevel.tick's entities, with its per-entity step.
    io.papermc.paper.entity.activation.ActivationRange.activateEntities(this);
    entityTickList.forEach(entity -> {
      if (entity.isRemoved() || tickRates.isEntityFrozen(entity)) return;
      entity.checkDespawn();
      Entity vehicle = entity.getVehicle();
      if (vehicle != null) {
        if (!vehicle.isRemoved() && vehicle.hasPassenger(entity)) return;
        entity.stopRiding();
      }
      guardEntityTick(this::tickNonPassenger, entity);
    });
    tickBlockEntities();
    reported();
  }

  void tickBlock(BlockPos pos, Block block) {
    BlockState state = getBlockState(pos);
    if (state.is(block)) state.tick(this, pos, random);
  }

  void tickFluid(BlockPos pos, net.minecraft.world.level.material.Fluid fluid) {
    FluidState state = getFluidState(pos);
    if (state.is(fluid)) state.tick(this, pos, getBlockState(pos));
  }

  // ServerLevel.runBlockEvents, without the packets to players.
  @SuppressWarnings("unchecked")
  void runBlockEvents() {
    try {
      var events = (it.unimi.dsi.fastutil.objects.ObjectLinkedOpenHashSet<net.minecraft.world.level.BlockEventData>) blockEventsField.get(this);
      var reschedule = (java.util.List<net.minecraft.world.level.BlockEventData>) rescheduleField.get(this);
      reschedule.clear();
      while (!events.isEmpty()) {
        var event = events.removeFirst();
        if (shouldTickBlocksAt(event.pos())) {
          BlockState state = getBlockState(event.pos());
          if (state.is(event.block())) state.triggerEvent(this, event.pos(), event.paramA(), event.paramB());
        } else {
          reschedule.add(event);
        }
      }
      events.addAll(reschedule);
    } catch (IllegalAccessException error) { throw new IllegalStateException(error); }
  }

  void setHandlingTick(boolean handling) {
    try {
      handlingTickField.setBoolean(this, handling);
    } catch (IllegalAccessException error) { throw new IllegalStateException(error); }
  }

  static final Field blockEventsField, rescheduleField, handlingTickField;
  static {
    try {
      blockEventsField = net.minecraft.server.level.ServerLevel.class.getDeclaredField("blockEvents");
      blockEventsField.setAccessible(true);
      rescheduleField = net.minecraft.server.level.ServerLevel.class.getDeclaredField("blockEventsToReschedule");
      rescheduleField.setAccessible(true);
      handlingTickField = net.minecraft.server.level.ServerLevel.class.getDeclaredField("handlingTick");
      handlingTickField.setAccessible(true);
    } catch (ReflectiveOperationException error) { throw new ExceptionInInitializerError(error); }
  }

  boolean imports;

  // ServerLevel's own, which forgets the path types cached at the position and
  // makes the navigations that cross it recompute; the chunk source's blockChanged
  // (the packets to players) does nothing here.
  @Override public void sendBlockUpdated(BlockPos pos, BlockState old, BlockState now, int flags) {
    super.sendBlockUpdated(pos, old, now, flags);
  }

  // The live world's block changes since its last frame: each changed
  // position and the block it held before (McbenchEngine.tick). Other
  // levels keep none.
  it.unimi.dsi.fastutil.longs.Long2ObjectLinkedOpenHashMap<BlockState> changes;
  @Override public boolean setBlock(BlockPos pos, BlockState state, int flags, int recursionLeft) {
    if (changes != null && !isOutsideBuildHeight(pos)) changes.putIfAbsent(pos.asLong(), getBlockState(pos));
    return super.setBlock(pos, state, flags, recursionLeft);
  }
  @Override public void setBlocksDirty(BlockPos pos, BlockState old, BlockState now) {}
  @Override public void playSeededSound(Entity entity, double x, double y, double z, Holder<SoundEvent> sound, SoundSource source, float volume, float pitch, long seed) {}
  @Override public void playSeededSound(Entity entity, Entity at, Holder<SoundEvent> sound, SoundSource source, float volume, float pitch, long seed) {}
  @Override public void levelEvent(Entity entity, int type, BlockPos pos, int data) {}
  @Override public void destroyBlockProgress(int id, BlockPos pos, int progress) {}
  // Marks the chunk for saving: nothing is saved here.
  @Override public void blockEntityChanged(BlockPos pos) {}
}
