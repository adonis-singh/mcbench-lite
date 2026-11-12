import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.Property;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.io.BufferedOutputStream;
import java.io.BufferedReader;
import java.io.DataOutputStream;
import java.io.InputStreamReader;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

// The engine: the pinned server's own block code running in a server level
// that holds only the build (McbenchLevel), shipped as a native image (see
// server/engine.js). It places builds and runs the verifier's experiments
// (McbenchExperiment, shared with the Paper plugin).
//
// Placing follows the live import, with the pinned server's own block code:
// each setblock and fill command in the import's order, as those commands
// run them. A command places its block with flags 2 | 256 (the state its
// neighbours give it, then the properties the command names, as
// BlockInput.place does) and then notifies the neighbours of the change
// (ServerLevel.updateNeighboursOnBlockSet); a fill notifies them once all
// its blocks are placed. Shape updates, onPlace, neighbour updates in the
// server's own order and block entities follow from the level. Ticks stay
// frozen, as they are through an import: scheduled ticks wait in the queue.
// The result is the blueprint the renderer draws as a preview.
//
// Requests on stdin:  PLACE <id> <commands file> <blueprint file> <fresh|continue>
//                     VERIFY <id> <request file> <commands file> <output folder>  (see verify)
//                     TICK <id> <ticks> <patch file> <entities file>  (see tick)
//                     INPUT <id> <toggle|press|use> <x> <y> <z> <yaw>  (see input)
//                     DUMP <id> <file>  (see dump)
//                     STATE <id> <file>  (see state)
// A fresh request starts from an empty world; a continuing one applies its
// commands to the world the last request left, as an edit is applied to the
// live world.
// Replies on stdout:  MCBENCH_PLACED <id> <ms>  or  MCBENCH_PLACE_FAILED <id> <reason>
// (VERIFIED and VERIFY_FAILED for VERIFY; TICKED and TICK_FAILED for
// TICK; DONE and FAILED for INPUT and DUMP), and
// MCBENCH_ENGINE_FAILED <reason>
// if the engine itself stops.
// The blueprint file is the viewer's format (see server/blueprint.js).
//
// MCBENCH_ENGINE_STOCK (see McbenchLevel.configure): unset, the engine's fast
// paths, each of which gives what Paper's own code gives; 1, Paper's own code
// paths only; check, the fast paths compared against Paper's as they run.
public class McbenchEngine {
  // Minecraft's bootstrap sends System.out through its logger; replies use
  // the stream as it was before.
  static final java.io.PrintStream reply = System.out;
  static McbenchLevel level;

  // BlockInput.copyProperty: a property the shaped block does not have is left out.
  static <T extends Comparable<T>> BlockState with(BlockState target, Property<T> property, BlockState source) {
    return target.trySetValue(property, source.getValue(property));
  }

  public static void main(String[] args) throws Exception {
    // The engine serves the process that started it and dies with it, even
    // mid-experiment, however that process ends.
    var parent = ProcessHandle.current().parent().orElseThrow();
    var watch = new Thread(() -> {
      while (parent.isAlive()) {
        try { Thread.sleep(1000); } catch (InterruptedException error) { return; }
      }
      Runtime.getRuntime().halt(1);
    }, "mcbench-engine-parent");
    watch.setDaemon(true);
    watch.start();
    // Paper accepts block changes only on a tick thread; everything runs on one.
    var failure = new java.util.concurrent.atomic.AtomicReference<Throwable>();
    var worker = new ca.spottedleaf.moonrise.common.util.TickThread(() -> {
      try {
        boot();
        serve();
      } catch (Throwable error) { failure.set(error); }
    }, "mcbench-engine");
    worker.start();
    worker.join();
    // Paper's logger holds stderr by now: the reason goes out with the replies.
    if (failure.get() != null) {
      reply.println("MCBENCH_ENGINE_FAILED " + describe(failure.get()));
      reply.flush();
      System.exit(1);
    }
  }

  // Minecraft's registries, then the server's data as Main loads it for a new
  // world: the data packs a new world enables (the vanilla pack, then the
  // world folder's own: mcbench's build height, see minecraft-dimension.js),
  // loaded through WorldLoader into every registry, the tags, recipes and
  // loot tables. The world folder is the engine's own, beside it.
  static void boot() throws Exception {
    net.minecraft.SharedConstants.tryDetectVersion();
    net.minecraft.server.Bootstrap.bootStrap();
    // MCBENCH_ENGINE_STOCK, once (see McbenchLevel).
    McbenchLevel.configure();
    // Paper's global settings at their defaults, as a server writes them.
    var configuration = new io.papermc.paper.configuration.GlobalConfiguration();
    McbenchLevel.defaults(configuration);
    var set = io.papermc.paper.configuration.GlobalConfiguration.class.getDeclaredMethod("set", io.papermc.paper.configuration.GlobalConfiguration.class);
    set.setAccessible(true);
    set.invoke(null, configuration);
    var storage = net.minecraft.world.level.storage.LevelStorageSource.createDefault(Path.of("").toAbsolutePath())
      .validateAndCreateAccess("world", net.minecraft.world.level.dimension.LevelStem.OVERWORLD);
    McbenchLevel.storage = storage;
    var packs = net.minecraft.server.packs.repository.ServerPacksSource.createPackRepository(storage);
    var properties = new net.minecraft.server.dedicated.DedicatedServerProperties(new java.util.Properties(), new joptsimple.OptionParser().parse());
    var init = new net.minecraft.server.WorldLoader.InitConfig(new net.minecraft.server.WorldLoader.PackConfig(packs,
      new net.minecraft.world.level.WorldDataConfiguration(properties.initialDataPackConfiguration, net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS), false, true),
      net.minecraft.commands.Commands.CommandSelection.DEDICATED, properties.functionPermissionLevel);
    McbenchLevel.data = net.minecraft.server.WorldLoader.<net.minecraft.world.level.storage.WorldData, net.minecraft.server.WorldStem>load(init,
      context -> new net.minecraft.server.WorldLoader.DataLoadOutput<>(null, context.datapackDimensions()),
      net.minecraft.server.WorldStem::new, net.minecraft.Util.backgroundExecutor(), Runnable::run).join();
    McbenchLevel.data.dataPackResources().updateStaticRegistryTags();
  }

  // A fresh live world: a level for a frozen import, seeded as a seeded
  // verifier run seeds its world (the level's random, the entity id counter
  // and the entities' shared random), with every entity ticking as near a
  // player, as in the verifier's world: the live world is what a player
  // watching it sees, the same for the same requests.
  static McbenchLevel liveLevel() throws Exception {
    McbenchLevel level = McbenchLevel.create(0);
    McbenchExperiment.seed(level, 0);
    activateAll(level);
    actor = null;
    return level;
  }

  // The plugin turns off Paper's entity activation throttle for its world.
  static void activateAll(McbenchLevel level) {
    var activation = level.spigotConfig;
    activation.animalActivationRange = activation.monsterActivationRange = activation.raiderActivationRange = activation.miscActivationRange
      = activation.waterActivationRange = activation.villagerActivationRange = activation.flyingMonsterActivationRange = 0;
  }

  static void serve() throws Exception {
    level = liveLevel();
    reply.println("MCBENCH_ENGINE_READY");
    reply.flush();

    var input = new BufferedReader(new InputStreamReader(System.in));
    for (String line; (line = input.readLine()) != null;) {
      String[] request = line.split(" ");
      long start = System.nanoTime();
      // An error a request reported (and a VERIFY run never collects) is that request's alone.
      McbenchLevel.serverException = null;
      if (request[0].equals("INPUT") || request[0].equals("DUMP") || request[0].equals("STATE")) {
        try {
          if (request[0].equals("INPUT")) input(request);
          else if (request[0].equals("STATE")) state(Path.of(request[2]));
          else dump(Path.of(request[2]));
          reply.println("MCBENCH_DONE " + request[1] + " " + (System.nanoTime() - start) / 1_000_000);
        } catch (Exception | StackOverflowError error) {
          reply.println("MCBENCH_FAILED " + request[1] + " " + describe(error));
        }
      } else if (request[0].equals("TICK")) {
        try {
          tick(Integer.parseInt(request[2]), Path.of(request[3]), Path.of(request[4]));
          reply.println("MCBENCH_TICKED " + request[1] + " " + (System.nanoTime() - start) / 1_000_000);
        } catch (Exception | StackOverflowError error) {
          reply.println("MCBENCH_TICK_FAILED " + request[1] + " " + describe(error));
        }
      } else if (request[0].equals("VERIFY")) {
        try {
          verify(Path.of(request[2]), Path.of(request[3]), Path.of(request[4]));
          reply.println("MCBENCH_VERIFIED " + request[1] + " " + (System.nanoTime() - start) / 1_000_000);
        } catch (Exception | StackOverflowError error) {
          reply.println("MCBENCH_VERIFY_FAILED " + request[1] + " " + describe(error));
        }
      } else try {
        if (request[4].equals("fresh")) level = liveLevel();
        placeAll(level, Files.readAllLines(Path.of(request[2])), false);
        McbenchLevel.reported();
        write(Path.of(request[3]));
        reply.println("MCBENCH_PLACED " + request[1] + " " + (System.nanoTime() - start) / 1_000_000);
      } catch (Exception | StackOverflowError error) {
        reply.println("MCBENCH_PLACE_FAILED " + request[1] + " " + describe(error));
      }
      reply.flush();
    }
  }

  // An error and its causes on one line, with where the innermost arose.
  static String describe(Throwable error) {
    var text = new StringBuilder(String.valueOf(error));
    Throwable root = error;
    for (Throwable cause = error.getCause(); cause != null && cause != root; cause = cause.getCause()) {
      text.append(" <- ").append(cause);
      root = cause;
    }
    var frames = root.getStackTrace();
    for (int i = 0; i < Math.min(10, frames.length); i++) text.append(" @ ").append(frames[i]);
    return text.toString().replace('\n', ' ');
  }

  // The commands as the import runs them; `strict` adds the strict keyword
  // the verifier can run them with (flags 2 | 816, no neighbour shaping or
  // notifications).
  static void placeAll(McbenchLevel level, List<String> lines, boolean strict) throws Exception {
    int flags = Block.UPDATE_CLIENTS | (strict ? 816 : Block.UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS);
    // The live import force-loads every column it touches and the columns
    // around them before it places anything (minecraft-live.js applyWorld).
    if (level.imports) for (String line : lines) {
      String[] p = line.split(" ");
      boolean fill = p[0].equals("fill");
      int x1 = Integer.parseInt(p[1]) >> 4, z1 = Integer.parseInt(p[3]) >> 4;
      int x2 = fill ? Integer.parseInt(p[4]) >> 4 : x1, z2 = fill ? Integer.parseInt(p[6]) >> 4 : z1;
      for (int x = x1 - 1; x <= x2 + 1; x++) for (int z = z1 - 1; z <= z2 + 1; z++) level.ticket(x, z);
    }
    for (String line : lines) {
      String[] p = line.split(" ");
      boolean fill = p[0].equals("fill");
      var parsed = parseBlock(fill ? p[7] : p[4]);
      int x1 = Integer.parseInt(p[1]), y1 = Integer.parseInt(p[2]), z1 = Integer.parseInt(p[3]);
      int x2 = fill ? Integer.parseInt(p[4]) : x1, y2 = fill ? Integer.parseInt(p[5]) : y1, z2 = fill ? Integer.parseInt(p[6]) : z1;
      if (McbenchLevel.stock) {
        var changed = new ArrayList<Object[]>();
        // BlockPos.betweenClosed's order, as /fill places: x fastest, then y, then z.
        for (int z = z1; z <= z2; z++) for (int y = y1; y <= y2; y++) for (int x = x1; x <= x2; x++) {
          BlockPos pos = new BlockPos(x, y, z);
          BlockState old = level.getBlockState(pos);
          if (place(level, parsed, pos, flags) && !strict) changed.add(new Object[] { pos, old });
        }
        for (Object[] at : changed) level.updateNeighboursOnBlockSet((BlockPos) at[0], (BlockState) at[1]);
        continue;
      }
      // The same, with the blocks that changed kept as coordinates and the
      // states they had instead of an array and a position each (the
      // positions are made again, with the same coordinates, as the
      // neighbours are notified).
      int changes = 0;
      for (int z = z1; z <= z2; z++) for (int y = y1; y <= y2; y++) for (int x = x1; x <= x2; x++) {
        BlockPos pos = new BlockPos(x, y, z);
        BlockState old = level.getBlockState(pos);
        if (place(level, parsed, pos, flags) && !strict) {
          if (changes == changedOld.length) {
            changedOld = java.util.Arrays.copyOf(changedOld, changes * 2);
            changedAt = java.util.Arrays.copyOf(changedAt, changes * 6);
          }
          changedOld[changes] = old;
          changedAt[changes * 3] = x; changedAt[changes * 3 + 1] = y; changedAt[changes * 3 + 2] = z;
          changes++;
        }
      }
      for (int i = 0; i < changes; i++) {
        BlockState old = changedOld[i];
        changedOld[i] = null;
        level.updateNeighboursOnBlockSet(new BlockPos(changedAt[i * 3], changedAt[i * 3 + 1], changedAt[i * 3 + 2]), old);
      }
    }
  }

  // The blocks a command changed (see placeAll), reused from one command to
  // the next.
  static BlockState[] changedOld = new BlockState[64];
  static int[] changedAt = new int[64 * 3];

  // A command's block as BlockStateParser parses it. The result depends on
  // the text alone (and the registry), and nothing changes it afterwards, so
  // a text already parsed gives the result it gave. A text that does not
  // parse fails every time, as it does when parsed afresh.
  static final Map<String, BlockStateParser.BlockResult> parsedBlocks = new HashMap<>();

  static BlockStateParser.BlockResult parseBlock(String text) throws com.mojang.brigadier.exceptions.CommandSyntaxException {
    if (McbenchLevel.stock) return BlockStateParser.parseForBlock(BuiltInRegistries.BLOCK, text, false);
    var parsed = parsedBlocks.get(text);
    if (parsed == null) {
      parsed = BlockStateParser.parseForBlock(BuiltInRegistries.BLOCK, text, false);
      parsedBlocks.put(text, parsed);
    } else if (McbenchLevel.check) {
      var fresh = BlockStateParser.parseForBlock(BuiltInRegistries.BLOCK, text, false);
      if (fresh.blockState() != parsed.blockState() || !fresh.properties().equals(parsed.properties()) || fresh.nbt() != parsed.nbt()) throw new IllegalStateException("The block " + text + " parsed differently the second time");
    }
    return parsed;
  }

  // A verifier experiment (McbenchExperiment) as the plugin runs it: a fresh
  // level holding the request's columns, seeded as the plugin seeds its
  // world, the import's commands placed as the plugin's apply places them,
  // then server ticks until the experiment ends. A failure of the level
  // itself ends the run; an experiment that fails ends with its result, as
  // in Paper.
  static void verify(Path requestFile, Path commandsFile, Path folder) throws Exception {
    JsonObject request = JsonParser.parseString(Files.readString(requestFile)).getAsJsonObject();
    var columns = new ArrayList<int[]>();
    for (JsonElement column : request.getAsJsonArray("columns")) columns.add(new int[] { column.getAsJsonArray().get(0).getAsInt(), column.getAsJsonArray().get(1).getAsInt() });
    McbenchLevel level = McbenchLevel.create(request.has("seed") ? request.get("seed").getAsLong() : 0, columns);
    if (request.has("seed")) McbenchExperiment.seed(level, request.get("seed").getAsLong());
    activateAll(level);
    Files.createDirectories(folder);
    // At its end the experiment may save chunks (seeded runs), with their light.
    var experiment = new McbenchExperiment(request, level.getWorld(), folder, () -> stopped(level, request));
    var lines = Files.readAllLines(commandsFile);
    if (request.has("block_data_version")) for (int i = 0; i < lines.size(); i++) {
      String[] parts = lines.get(i).split(" ");
      int index = parts[0].equals("setblock") ? 4 : 7;
      parts[index] = experiment.upgradeBlockToken(parts[index]);
      lines.set(i, String.join(" ", parts));
    }
    placeAll(level, lines, request.get("placementMode").getAsString().equals("strict"));
    experiment.start("McbenchLevel " + net.minecraft.SharedConstants.getCurrentVersion().name(), net.minecraft.SharedConstants.getCurrentVersion().name());
    experiment.begin();
    // A request that asks for it gets the entities' state after every tick, in entity-trace.txt (for comparing engines).
    Path trace = request.has("entity_trace") && request.get("entity_trace").getAsBoolean() ? folder.resolve("entity-trace.txt") : null;
    if (trace != null) { Files.deleteIfExists(trace); trace(trace, level); }
    while (experiment.running()) {
      try {
        level.tick();
      } catch (Exception | StackOverflowError error) {
        experiment.finish(new IllegalStateException("The engine's level failed", error));
        throw error;
      }
      experiment.endOfTick();
      if (trace != null) trace(trace, level);
    }
  }

  // The experiment's end, as McbenchVerify.stopped ends it in Paper: what the
  // ticks changed is lit, and a seeded run, whose chunks are saved, has its
  // columns lit again from their blocks by Starlight's relight (Paper's
  // starlight$serverRelightChunks runs StarLightInterface.relightChunks over
  // the columns in this order). A relight gives the light the blocks define,
  // and its own nibble states: a section that only needed a nibble while
  // changes were lit is left without one, as Paper saves it.
  static void stopped(McbenchLevel level, JsonObject request) {
    var light = ((McbenchLevel.Chunks) level.getChunkSource()).light;
    light.flush();
    if (!request.has("seed")) return;
    var columns = new java.util.LinkedHashSet<ChunkPos>();
    for (JsonElement column : request.getAsJsonArray("columns")) columns.add(new ChunkPos(column.getAsJsonArray().get(0).getAsInt(), column.getAsJsonArray().get(1).getAsInt()));
    var relit = new int[] { -1 };
    light.starlight().relightChunks(columns, chunk -> {}, count -> relit[0] = count);
    if (relit[0] != columns.size()) throw new IllegalStateException("Relit " + relit[0] + " of " + columns.size() + " columns");
  }

  // What the entity trace holds after a tick: the game time, the level random, the entity id counter, the shared
  // random, each entity's state lines and the saved data of the vehicles, paintings and armor stands.
  static void trace(Path file, McbenchLevel level) throws Exception {
    var out = new StringBuilder();
    out.append("time ").append(level.getGameTime()).append('\n');
    out.append("random ").append(McbenchExperiment.randomState(level.random)).append('\n');
    var counter = net.minecraft.world.entity.Entity.class.getDeclaredField("ENTITY_COUNTER");
    counter.setAccessible(true);
    out.append("entityid ").append(((java.util.concurrent.atomic.AtomicInteger) counter.get(null)).get()).append('\n');
    var sharedValue = Class.forName("ca.spottedleaf.moonrise.common.util.ThreadUnsafeRandom").getDeclaredField("value");
    sharedValue.setAccessible(true);
    out.append("sharedrandom ").append(sharedValue.getLong(net.minecraft.world.entity.Entity.SHARED_RANDOM)).append('\n');
    entityLines(out, level);
    // The end gateways: age and teleport cooldown, by position.
    var gateways = new java.util.TreeMap<Long, String>();
    for (var chunk : level.chunkMap.values()) {
      for (var be : chunk.getBlockEntities().values()) {
        if (be instanceof net.minecraft.world.level.block.entity.TheEndGatewayBlockEntity gateway) {
          var pos = be.getBlockPos();
          gateways.put(pos.asLong(), "gateway " + pos.getX() + " " + pos.getY() + " " + pos.getZ() + " " + field(net.minecraft.world.level.block.entity.TheEndGatewayBlockEntity.class, "age").getLong(gateway) + " " + field(net.minecraft.world.level.block.entity.TheEndGatewayBlockEntity.class, "teleportCooldown").getInt(gateway));
        }
      }
    }
    for (var line : gateways.values()) out.append(line).append('\n');
    for (var event : (Iterable<net.minecraft.world.level.BlockEventData>) McbenchLevel.blockEventsField.get(level)) {
      out.append("blockevent ").append(event.pos().getX()).append(' ').append(event.pos().getY()).append(' ').append(event.pos().getZ()).append(' ').append(event.paramA()).append(' ').append(event.paramB()).append('\n');
    }
    for (var entity : level.getEntities().getAll()) {
      if (!(entity instanceof net.minecraft.world.entity.vehicle.AbstractMinecart || entity instanceof net.minecraft.world.entity.vehicle.AbstractBoat
        || entity instanceof net.minecraft.world.entity.decoration.Painting)) continue;
      var problems = new net.minecraft.util.ProblemReporter.Collector();
      var output = net.minecraft.world.level.storage.TagValueOutput.createWithContext(problems, level.registryAccess());
      out.append("save ").append(entity.getId()).append(' ');
      if (entity.saveAsPassenger(output) && problems.isEmpty()) out.append(output.buildResult().toString()); else out.append("-");
      out.append('\n');
    }
    Files.writeString(file, out, java.nio.file.StandardOpenOption.CREATE, java.nio.file.StandardOpenOption.APPEND);
  }

  // BlockInput.place for a command's block (no block entity data).
  static boolean place(McbenchLevel level, BlockStateParser.BlockResult parsed, BlockPos pos, int flags) {
    BlockState state = parsed.blockState();
    BlockState shaped = (flags & Block.UPDATE_KNOWN_SHAPE) != 0 ? state : Block.updateFromNeighbourShapes(state, level, pos);
    if (shaped.isAir()) shaped = state;
    // BlockInput.overwriteWithDefinedProperties: unchanged if the shape left
    // the state as parsed, else each named property copied where the shaped
    // block has it (trySetValue).
    if (shaped != state) for (var property : parsed.properties().keySet()) shaped = with(shaped, property, state);
    return level.setBlock(pos, shaped, flags);
  }

  // The level's blocks, every chunk's sections in order.
  static void write(Path target) throws Exception {
    var palette = new ArrayList<BlockState>();
    var index = new HashMap<BlockState, Integer>();
    palette.add(Blocks.AIR.defaultBlockState());
    index.put(Blocks.AIR.defaultBlockState(), 0);
    var sections = new java.util.LinkedHashMap<Long, short[]>();
    var stream = new Stream(palette, index);
    int[] bounds = { Integer.MAX_VALUE, Integer.MAX_VALUE, Integer.MAX_VALUE, Integer.MIN_VALUE, Integer.MIN_VALUE, Integer.MIN_VALUE };
    var keys = level.chunkMap.keySet().toLongArray();
    java.util.Arrays.sort(keys);
    for (long key : keys) {
      var chunk = level.chunkMap.get(key);
      var chunkSections = chunk.getSections();
      for (int i = 0; i < chunkSections.length; i++) {
        var section = chunkSections[i];
        if (section.hasOnlyAir()) continue;
        int sx = chunk.getPos().x, sy = level.getMinSectionY() + i, sz = chunk.getPos().z;
        // The section's values, made when its first block that is not air is
        // met (in the section's order, as before), and its blocks by plane
        // along x, y and z.
        short[] values = null;
        int[][] counts = new int[3][16];
        int held = 0;
        // The last state met and its palette index (the same state often runs on).
        BlockState last = null;
        int lastValue = 0;
        for (int at = 0; at < 4096; at++) {
          BlockState state = section.getBlockState(at & 15, at >> 8, (at >> 4) & 15);
          if (state.isAir()) continue;
          int value;
          if (state == last) value = lastValue;
          else {
            value = index.computeIfAbsent(state, added -> { palette.add(added); return palette.size() - 1; });
            last = state;
            lastValue = value;
          }
          if (values == null) {
            values = new short[4096];
            sections.put(BlockPos.asLong(sx, sy, sz), values);
          }
          values[at] = (short) value;
          counts[0][at & 15]++;
          counts[1][at >> 8]++;
          counts[2][(at >> 4) & 15]++;
          held++;
          int x = sx * 16 + (at & 15), y = sy * 16 + (at >> 8), z = sz * 16 + ((at >> 4) & 15);
          if (x < bounds[0]) bounds[0] = x;
          if (y < bounds[1]) bounds[1] = y;
          if (z < bounds[2]) bounds[2] = z;
          if (x > bounds[3]) bounds[3] = x;
          if (y > bounds[4]) bounds[4] = y;
          if (z > bounds[5]) bounds[5] = z;
        }
        if (values != null) stream.count(new int[] { sx, sy, sz }, counts, ChunkPos.asLong(sx, sz), held);
      }
    }
    if (sections.isEmpty()) throw new IllegalStateException("The import places no blocks");
    Path partial = target.resolveSibling(target.getFileName() + ".part");
    try (var out = new DataOutputStream(new BufferedOutputStream(Files.newOutputStream(partial)))) {
      out.writeInt(palette.size());
      for (BlockState state : palette) writeState(out, state);
      for (int value : bounds) out.writeInt(value);
      out.writeInt(sections.size());
      for (var entry : sections.entrySet()) {
        BlockPos section = BlockPos.of(entry.getKey());
        out.writeInt(section.getX()); out.writeInt(section.getY()); out.writeInt(section.getZ());
        for (short value : entry.getValue()) out.writeShort(value);
      }
    }
    Files.move(partial, target, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
    // The blueprint is the base the live world's next patches apply to.
    McbenchEngine.stream = stream;
    level.changes = new it.unimi.dsi.fastutil.longs.Long2ObjectLinkedOpenHashMap<>();
  }

  // The live world's stream since its last base (write): the palette the
  // patches index, and the blocks that are not air counted by plane along x,
  // y and z (by section coordinate, its 16 planes) and by chunk column, which
  // the bounds and the built columns (chunk columns holding a block) follow
  // as blocks change.
  record Stream(List<BlockState> palette, Map<BlockState, Integer> index, List<java.util.TreeMap<Integer, int[]>> planes, it.unimi.dsi.fastutil.longs.Long2IntOpenHashMap columns) {
    Stream(List<BlockState> palette, Map<BlockState, Integer> index) {
      this(palette, index, List.of(new java.util.TreeMap<>(), new java.util.TreeMap<>(), new java.util.TreeMap<>()), new it.unimi.dsi.fastutil.longs.Long2IntOpenHashMap());
    }

    // A section's blocks: `counts` by plane along each axis, `held` in all.
    void count(int[] section, int[][] counts, long column, int held) {
      for (int axis = 0; axis < 3; axis++) {
        int[] plane = planes.get(axis).computeIfAbsent(section[axis], key -> new int[16]);
        for (int n = 0; n < 16; n++) plane[n] += counts[axis][n];
      }
      columns.addTo(column, held);
    }

    // One block that is not air more (`count` 1) or fewer (-1) at `at`.
    void count(int[] at, int count) {
      for (int axis = 0; axis < 3; axis++) {
        int[] plane = planes.get(axis).computeIfAbsent(at[axis] >> 4, key -> new int[16]);
        plane[at[axis] & 15] += count;
        if (java.util.Arrays.stream(plane).allMatch(n -> n == 0)) planes.get(axis).remove(at[axis] >> 4);
      }
      long column = ChunkPos.asLong(at[0] >> 4, at[2] >> 4);
      if (columns.addTo(column, count) + count == 0) columns.remove(column);
    }

    // The bounds of the blocks (min x, y, z, max x, y, z), or null for none.
    int[] bounds() {
      if (columns.isEmpty()) return null;
      int[] bounds = new int[6];
      for (int axis = 0; axis < 3; axis++) {
        var low = planes.get(axis).firstEntry();
        int n = 0;
        while (low.getValue()[n] == 0) n++;
        bounds[axis] = (low.getKey() << 4) + n;
        var high = planes.get(axis).lastEntry();
        n = 15;
        while (high.getValue()[n] == 0) n--;
        bounds[axis + 3] = (high.getKey() << 4) + n;
      }
      return bounds;
    }
  }
  static Stream stream;

  // Ticks the live world `ticks` times, then writes what changed: a patch
  // (the format in web/live-patch.mjs) of every block that differs from the
  // world before, with the bounds and the count of built columns after it,
  // which a block placed past the bounds, or in a column that had none,
  // grows, and air on the bounds' edge, or in a column's last block, shrinks.
  // The entities go to their own file each time, as JSON.
  static void tick(int ticks, Path patchFile, Path entitiesFile) throws Exception {
    for (int i = 0; i < ticks; i++) level.tick();
    // Light what the ticks changed, as Paper's light worker does between ticks.
    ((McbenchLevel.Chunks) level.getChunkSource()).light.flush();
    var changes = new ArrayList<BlockPos>();
    for (var change : level.changes.long2ObjectEntrySet()) {
      BlockPos pos = BlockPos.of(change.getLongKey());
      BlockState now = level.getBlockState(pos);
      if (now == change.getValue()) continue;
      changes.add(pos);
      if (now.isAir() != change.getValue().isAir()) stream.count(new int[] { pos.getX(), pos.getY(), pos.getZ() }, now.isAir() ? -1 : 1);
    }
    level.changes = new it.unimi.dsi.fastutil.longs.Long2ObjectLinkedOpenHashMap<>();
    writeEntities(entitiesFile);
    int[] bounds = stream.bounds();
    if (bounds == null) throw new IllegalStateException("The import places no blocks");
    int known = stream.palette().size();
    var indices = new int[changes.size()];
    for (int i = 0; i < indices.length; i++) {
      indices[i] = stream.index().computeIfAbsent(level.getBlockState(changes.get(i)), added -> { stream.palette().add(added); return stream.palette().size() - 1; });
    }
    Path partial = patchFile.resolveSibling(patchFile.getFileName() + ".part");
    try (var out = new DataOutputStream(new BufferedOutputStream(Files.newOutputStream(partial)))) {
      out.writeInt(stream.palette().size() - known);
      for (BlockState state : stream.palette().subList(known, stream.palette().size())) writeState(out, state);
      for (int value : bounds) out.writeInt(value);
      out.writeInt(stream.columns().size());
      out.writeInt(changes.size());
      for (int i = 0; i < indices.length; i++) {
        BlockPos pos = changes.get(i);
        out.writeInt(pos.getX()); out.writeInt(pos.getY()); out.writeInt(pos.getZ());
        out.writeShort(indices[i]);
      }
    }
    Files.move(partial, patchFile, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
  }

  // A player's input to the live world, with the yaw the player looks with
  // (degrees, Minecraft's convention: 0 south, 90 west; a 32-bit float).
  // "toggle" pulls a lever and "press" pushes a button unless it is pressed
  // already, as the verifier makes them (McbenchExperiment.interact: no
  // player). "use" is a player's right click with an empty hand, Minecraft's
  // own BlockState.useWithoutItem, on the blocks McbenchExperiment.usable
  // lists. The player is a ServerPlayer that is in no world
  // (McbenchExperiment.newInputPlayer, the one the verifier's use inputs are
  // made by). What the methods ask of it is its yaw (a fence gate opens away
  // from the side the player is on: Entity.getDirection), whether it may build
  // (a repeater, comparator, daylight detector or redstone dust change only
  // for a player who may), the entity its sounds and game events name (a sculk
  // sensor hears the player's uuid), and its statistics (a note block awards
  // one). A use the block passes on (an iron door, dust that would connect to
  // the same sides) is no input. The player's making draws an entity
  // id and the shared random's values, which a player who had joined would
  // have drawn already: they are put back, so the world holds what it held.
  static net.minecraft.server.level.ServerPlayer actor;

  static void input(String[] request) throws Exception {
    String type = request[2];
    BlockPos pos = new BlockPos(Integer.parseInt(request[3]), Integer.parseInt(request[4]), Integer.parseInt(request[5]));
    BlockState state = level.getBlockState(pos);
    float yaw = Float.parseFloat(request[6]);
    if (type.equals("toggle") && state.getBlock() instanceof net.minecraft.world.level.block.LeverBlock lever) lever.pull(state, level, pos, null);
    else if (type.equals("press") && state.getBlock() instanceof net.minecraft.world.level.block.ButtonBlock button) {
      if (!state.getValue(net.minecraft.world.level.block.ButtonBlock.POWERED)) button.press(state, level, pos, null);
    } else if (type.equals("use") && McbenchExperiment.usable(state.getBlock())) {
      if (actor == null) actor = newActor();
      actor.setYRot(yaw);
      var result = state.useWithoutItem(level, actor, new net.minecraft.world.phys.BlockHitResult(
        net.minecraft.world.phys.Vec3.atCenterOf(pos), net.minecraft.core.Direction.UP, pos, false));
      if (!result.consumesAction()) throw new IllegalArgumentException("No " + type + " input at " + pos + ": " + state);
    } else throw new IllegalArgumentException("No " + type + " input at " + pos + ": " + state);
    McbenchLevel.reported();
  }

  // The input player, made without a trace on the world.
  static net.minecraft.server.level.ServerPlayer newActor() throws Exception {
    var counter = (java.util.concurrent.atomic.AtomicInteger) field(net.minecraft.world.entity.Entity.class, "ENTITY_COUNTER").get(null);
    var sharedValue = field(Class.forName("ca.spottedleaf.moonrise.common.util.ThreadUnsafeRandom"), "value");
    int id = counter.get();
    long shared = sharedValue.getLong(net.minecraft.world.entity.Entity.SHARED_RANDOM);
    var player = McbenchExperiment.newInputPlayer(level);
    counter.set(id);
    sharedValue.setLong(net.minecraft.world.entity.Entity.SHARED_RANDOM, shared);
    return player;
  }

  // What the Rust engine (core/) reads of Minecraft: every block, the
  // behaviour methods its class overrides below Block, and its properties;
  // every block state in Block.BLOCK_STATE_REGISTRY order, with its
  // property values and the data Minecraft computes for it (as its state
  // cache does, with an empty level at the origin). Lines, tab-separated:
  //   block <key> <class> <overrides, comma-separated or -> <first state id> <state count> <default state id> <friction> <speed factor> <jump factor> <loot table or -> <supertypes> <explosion resistance>
  //   property <name> <values, comma-separated>
  //   state <id> <value indices, comma-separated or -> <flags> <sturdy faces> <push reaction> <light emission> <light block> <instrument> <destroy speed> <collision shape> <fluid state>
  //   tag <name> <block keys, comma-separated or ->   (every block tag, sorted by name)
  //   shape <x coords> <y coords> <z coords> <voxel sizes x,y,z> <full voxels> <single box or -> <occludes full block 0|1>
  //   item <key> <max stack size> <block key or -> <damage resistant tag or ->
  //   fluid <id> <fluid key> <falling 0|1 or -> <level or -> <amount> <source 0|1> <legacy block state id>   (every fluid state, in Fluid.FLUID_STATE_REGISTRY order)
  //   occludes <direction> <first shape> <hex bits, one per second shape (bit i of the hex digit string read from the left, 4 shapes per digit, most significant first)>
  //     Shapes.mergedFaceOccludes(first, second, direction) for every pair of the dump's shapes
  //   flag <name> <block keys, comma-separated or ->  (canBeReplacedContext: the blocks whose class overrides
  //     canBeReplaced(BlockState, BlockPlaceContext))
  // The supertypes are the simple names of the block's superclasses below Block (nearest first)
  // and of the interfaces LiquidBlockContainer, SimpleWaterloggedBlock and BucketPickup it implements, comma-separated.
  // A state's collision shape is the number of its shape line (in order,
  // from 0): getCollisionShape at the origin with an empty collision context
  // (the shape of every class that does not override getCollisionShape or
  // getShape, nor offsets by position). A shape's coordinates are its
  // getCoords per axis, comma-separated; its voxels its DiscreteVoxelShape's
  // isFull, a 0 or 1 per voxel at (x * sizeY + y) * sizeZ + z; its single box
  // moonrise's single-AABB representation, min x, y, z, max x, y, z.
  // Flags (bits): 1 air, 2 redstone conductor, 4 signal source, 8 analog
  // output, 16 liquid, 32 replaceable, 64 block entity, 128 blocks motion,
  // 256 solid, 512 collision shape full block, 1024 solid render, 2048 offset by
  // position (getOffset), 4096 large collision shape, 8192 dynamic shape (no
  // state cache: face sturdiness and collision fullness read the level).
  // Sturdy faces: bit (direction * 3 + support type), directions in
  // Direction order (down, up, north, south, west, east), support types
  // FULL, CENTER, RIGID.
  static final java.util.Set<String> BEHAVIOUR = java.util.Set.of("updateShape", "neighborChanged", "onPlace", "affectNeighborsAfterRemoval",
    "updateIndirectNeighbourShapes", "tick", "randomTick", "getSignal", "getDirectSignal", "isSignalSource", "hasAnalogOutputSignal",
    "getAnalogOutputSignal", "triggerEvent", "canSurvive", "entityInside", "stepOn", "fallOn", "updateEntityMovementAfterFallOn",
    "onProjectileHit", "useWithoutItem", "useItemOn", "attack", "onExplosionHit", "playerWillDestroy", "destroy", "setPlacedBy",
    "newBlockEntity", "getTicker", "isRandomlyTicking", "onRemove", "spawnAfterBreak", "getInputSignal", "getOutputSignal", "getCollisionShape", "getShape",
    "canPlaceLiquid", "placeLiquid");
  // The dump's distinct shapes, by number (see shapeNumber).
  static final List<net.minecraft.world.phys.shapes.VoxelShape> shapeObjects = new ArrayList<>();

  // A block's superclasses below Block and the liquid interfaces it implements.
  static String supertypes(Block block) {
    var names = new ArrayList<String>();
    for (Class<?> type = block.getClass().getSuperclass(); type != Block.class && type != net.minecraft.world.level.block.state.BlockBehaviour.class && type != null; type = type.getSuperclass()) names.add(type.getSimpleName());
    for (Class<?> type : new Class<?>[] { net.minecraft.world.level.block.LiquidBlockContainer.class, net.minecraft.world.level.block.SimpleWaterloggedBlock.class, net.minecraft.world.level.block.BucketPickup.class, net.minecraft.world.level.block.BonemealableBlock.class }) {
      if (type.isInstance(block)) names.add(type.getSimpleName());
    }
    return names.isEmpty() ? "-" : String.join(",", names);
  }

  static void dump(Path target) throws Exception {
    shapeObjects.clear();
    var out = new StringBuilder();
    var directions = net.minecraft.core.Direction.values();
    var supports = net.minecraft.world.level.block.SupportType.values();
    var empty = net.minecraft.world.level.EmptyBlockGetter.INSTANCE;
    var shapes = new java.util.LinkedHashMap<String, Integer>();
    var faces = new java.util.LinkedHashMap<String, Integer>();
    var faceShapes = new ArrayList<net.minecraft.world.phys.shapes.VoxelShape>();
    var occlusion = new StringBuilder();
    // Shape 0 of the faces is Shapes.empty(), which light is also tested through.
    faceNumber(faces, faceShapes, net.minecraft.world.phys.shapes.Shapes.empty());
    for (Block block : BuiltInRegistries.BLOCK) {
      var overrides = new java.util.TreeSet<String>();
      for (Class<?> type = block.getClass(); type != Block.class; type = type.getSuperclass()) {
        for (var method : type.getDeclaredMethods()) if (BEHAVIOUR.contains(method.getName())) overrides.add(method.getName());
      }
      var states = block.getStateDefinition().getPossibleStates();
      out.append("block\t").append(BuiltInRegistries.BLOCK.getKey(block)).append('\t').append(block.getClass().getName()).append('\t')
        .append(overrides.isEmpty() ? "-" : String.join(",", overrides)).append('\t').append(Block.getId(states.get(0))).append('\t').append(states.size())
        .append('\t').append(Block.getId(block.defaultBlockState())).append('\t').append(block.getFriction()).append('\t').append(block.getSpeedFactor())
        .append('\t').append(block.getJumpFactor()).append('\t')
        .append(block.getLootTable().map(key -> key.location().toString()).orElse("-")).append('\t').append(supertypes(block)).append('\t').append(block.getExplosionResistance()).append('\n');
      var properties = new ArrayList<>(block.getStateDefinition().getProperties());
      for (var property : properties) {
        var values = new ArrayList<String>();
        for (var value : property.getPossibleValues()) values.add(valueName(property, value));
        out.append("property\t").append(property.getName()).append('\t').append(String.join(",", values)).append('\n');
      }
      for (BlockState state : states) {
        var indices = new ArrayList<String>();
        for (var property : properties) indices.add(String.valueOf(new ArrayList<>(property.getPossibleValues()).indexOf(state.getValue(property))));
        int flags = (state.isAir() ? 1 : 0) | (state.isRedstoneConductor(empty, BlockPos.ZERO) ? 2 : 0) | (state.isSignalSource() ? 4 : 0)
          | (state.hasAnalogOutputSignal() ? 8 : 0) | (state.liquid() ? 16 : 0) | (state.canBeReplaced() ? 32 : 0) | (state.hasBlockEntity() ? 64 : 0)
          | (state.blocksMotion() ? 128 : 0) | (state.isSolid() ? 256 : 0) | (state.isCollisionShapeFullBlock(empty, BlockPos.ZERO) ? 512 : 0)
          | (state.isSolidRender() ? 1024 : 0) | (state.hasOffsetFunction() ? 2048 : 0) | (state.hasLargeCollisionShape() ? 4096 : 0)
          | (state.getBlock().hasDynamicShape() ? 8192 : 0) | (state.getBlock() instanceof net.minecraft.world.level.block.LeavesBlock ? 16384 : 0)
          | (state.useShapeForLightOcclusion() ? 32768 : 0) | (state.canOcclude() ? 65536 : 0);
        long sturdy = 0;
        for (int d = 0; d < directions.length; d++) for (int t = 0; t < supports.length; t++) {
          if (state.isFaceSturdy(empty, BlockPos.ZERO, directions[d], supports[t])) sturdy |= 1L << (d * supports.length + t);
        }
        out.append("state\t").append(Block.getId(state)).append('\t').append(indices.isEmpty() ? "-" : String.join(",", indices)).append('\t')
          .append(flags).append('\t').append(sturdy).append('\t').append(state.getPistonPushReaction().name()).append('\t')
          .append(state.getLightEmission()).append('\t').append(state.getLightBlock()).append('\t').append(state.instrument().getSerializedName())
          .append('\t').append(state.getDestroySpeed(empty, BlockPos.ZERO))
          .append('\t').append(shapeNumber(shapes, state.getCollisionShape(empty, BlockPos.ZERO, net.minecraft.world.phys.shapes.CollisionContext.empty())))
          .append('\t').append(net.minecraft.world.level.material.Fluid.FLUID_STATE_REGISTRY.getId(state.getFluidState())).append('\n');
        // Starlight's occlusion: a state that is conditionally full opaque (it
        // occludes and uses its shape for light) has a face shape per
        // direction, which light is tested through.
        if (state.starlight$isConditionallyFullOpaque()) {
          occlusion.append("occlusion\t").append(Block.getId(state));
          for (var direction : directions) occlusion.append('\t').append(faceNumber(faces, faceShapes, state.getFaceOcclusionShape(direction)));
          occlusion.append('\n');
        }
      }
    }
    var tags = BuiltInRegistries.BLOCK.getTags().sorted(java.util.Comparator.comparing(tag -> tag.key().location().toString())).toList();
    for (var tag : tags) {
      var keys = new ArrayList<String>();
      for (var holder : tag) keys.add(BuiltInRegistries.BLOCK.getKey(holder.value()).toString());
      out.append("tag\t").append(tag.key().location()).append('\t').append(keys.isEmpty() ? "-" : String.join(",", keys)).append('\n');
    }
    out.append(occlusion);
    // Whether two face shapes together occlude (Shapes.faceShapeOccludes), for
    // every ordered pair: n * n flags, row by row.
    out.append("faceoccludes\t").append(faceShapes.size()).append('\t');
    for (var a : faceShapes) for (var b : faceShapes) out.append(net.minecraft.world.phys.shapes.Shapes.faceShapeOccludes(a, b) ? '1' : '0');
    out.append('\n');
    int shapeIndex = 0;
    for (String shape : shapes.keySet()) out.append("shape\t").append(shape).append('\t').append(shapeObjects.get(shapeIndex++).moonrise$occludesFullBlock() ? 1 : 0).append('\n');
    for (var item : BuiltInRegistries.ITEM) {
      var resistant = item.components().get(net.minecraft.core.component.DataComponents.DAMAGE_RESISTANT);
      out.append("item\t").append(BuiltInRegistries.ITEM.getKey(item)).append('\t').append(item.getDefaultMaxStackSize()).append('\t')
        .append(item instanceof net.minecraft.world.item.BlockItem block ? BuiltInRegistries.BLOCK.getKey(block.getBlock()).toString() : "-")
        .append('\t').append(resistant == null ? "-" : resistant.types().location().toString()).append('\n');
    }
    for (var fluid : net.minecraft.world.level.material.Fluid.FLUID_STATE_REGISTRY) {
      var type = fluid.getType();
      boolean flowing = type instanceof net.minecraft.world.level.material.FlowingFluid;
      out.append("fluid\t").append(net.minecraft.world.level.material.Fluid.FLUID_STATE_REGISTRY.getId(fluid)).append('\t').append(BuiltInRegistries.FLUID.getKey(type)).append('\t')
        .append(flowing ? (fluid.getValue(net.minecraft.world.level.material.FlowingFluid.FALLING) ? "1" : "0") : "-").append('\t')
        .append(flowing && !fluid.isSource() ? String.valueOf(fluid.getValue(net.minecraft.world.level.material.FlowingFluid.LEVEL)) : "-").append('\t')
        .append(fluid.getAmount()).append('\t').append(fluid.isSource() ? 1 : 0).append('\t').append(Block.getId(fluid.createLegacyBlock())).append('\n');
    }
    for (var direction : directions) {
      for (int first = 0; first < shapeObjects.size(); first++) {
        out.append("occludes\t").append(direction.ordinal()).append('\t').append(first).append('\t');
        for (int at = 0; at < shapeObjects.size(); at += 4) {
          int digit = 0;
          for (int bit = 0; bit < 4; bit++) {
            digit = digit << 1 | (at + bit < shapeObjects.size() && net.minecraft.world.phys.shapes.Shapes.mergedFaceOccludes(shapeObjects.get(first), shapeObjects.get(at + bit), direction) ? 1 : 0);
          }
          out.append(Character.forDigit(digit, 16));
        }
        out.append('\n');
      }
    }
    // Appended line types. dispense: DispenserBlock.getDispenseMethod's inputs for an item (whether its feature is
    // enabled, the class of the behaviour registered for it, whether it has the equippable component).
    for (var item : BuiltInRegistries.ITEM) {
      var behaviour = net.minecraft.world.level.block.DispenserBlock.DISPENSER_REGISTRY.get(item);
      out.append("dispense\t").append(BuiltInRegistries.ITEM.getKey(item)).append('\t').append(item.isEnabled(net.minecraft.world.flag.FeatureFlags.VANILLA_SET) ? 1 : 0)
        .append('\t').append(behaviour == null ? "-" : behaviour.getClass().getName()).append('\t')
        .append(item.components().has(net.minecraft.core.component.DataComponents.EQUIPPABLE) ? 1 : 0).append('\n');
    }
    // compost: ComposterBlock.COMPOSTABLES, the chance of each item raising the level.
    for (var entry : net.minecraft.world.level.block.ComposterBlock.COMPOSTABLES.object2FloatEntrySet()) {
      out.append("compost\t").append(BuiltInRegistries.ITEM.getKey(entry.getKey().asItem())).append('\t').append(entry.getFloatValue()).append('\n');
    }
    // recipe: the crafting recipes the server holds, with each ingredient as the items it accepts (tags resolved) and,
    // last, the result as ItemStack.CODEC writes it (a transmute recipe's result is the item, count and component
    // changes it applies to its input).
    //   recipe shaped <id> <width> <height> <cells> <result>
    //   recipe shapeless <id> <ingredients> <result>
    //   recipe transmute <id> <ingredients> <result>
    //   recipe other <id> <class> <items it can match>
    // cooking: the smelting, blasting, smoking and campfire cooking recipes, in the recipe manager's order.
    //   cooking <recipe type> <id> <ingredient> <result> <experience> <cooking time>
    var serialization = McbenchLevel.data.registries().compositeAccess().createSerializationContext(net.minecraft.nbt.NbtOps.INSTANCE);
    var recipes = new ArrayList<net.minecraft.world.item.crafting.RecipeHolder<?>>(McbenchLevel.data.dataPackResources().getRecipeManager().getRecipes());
    recipes.sort(java.util.Comparator.comparing(holder -> holder.id().location().toString()));
    var transmuteResult = net.minecraft.world.item.crafting.TransmuteRecipe.class.getDeclaredField("result");
    transmuteResult.setAccessible(true);
    var registries = McbenchLevel.data.registries().compositeAccess();
    for (var holder : recipes) {
      if (holder.value() instanceof net.minecraft.world.item.crafting.AbstractCookingRecipe cooking) {
        out.append("cooking\t").append(BuiltInRegistries.RECIPE_TYPE.getKey(cooking.getType())).append('\t').append(holder.id().location()).append('\t')
          .append(ingredient(cooking.input())).append('\t')
          .append(stackText(cooking.assemble(new net.minecraft.world.item.crafting.SingleRecipeInput(net.minecraft.world.item.ItemStack.EMPTY), registries), serialization)).append('\t')
          .append(cooking.experience()).append('\t').append(cooking.cookingTime()).append('\n');
        continue;
      }
      if (!(holder.value() instanceof net.minecraft.world.item.crafting.CraftingRecipe recipe)) continue;
      out.append("recipe\t");
      if (recipe.getClass() == net.minecraft.world.item.crafting.ShapedRecipe.class && recipe instanceof net.minecraft.world.item.crafting.ShapedRecipe shaped) {
        var cells = new ArrayList<String>();
        for (var cell : shaped.getIngredients()) cells.add(cell.map(McbenchEngine::ingredient).orElse("-"));
        out.append("shaped\t").append(holder.id().location()).append('\t').append(shaped.getWidth()).append('\t').append(shaped.getHeight()).append('\t').append(String.join(";", cells))
          .append('\t').append(stackText(shaped.assemble(net.minecraft.world.item.crafting.CraftingInput.EMPTY, registries), serialization, holder.id()));
      } else if (recipe.getClass() == net.minecraft.world.item.crafting.ShapelessRecipe.class && recipe instanceof net.minecraft.world.item.crafting.ShapelessRecipe shapeless) {
        var cells = new ArrayList<String>();
        for (var cell : shapeless.placementInfo().ingredients()) cells.add(ingredient(cell));
        out.append("shapeless\t").append(holder.id().location()).append('\t').append(String.join(";", cells))
          .append('\t').append(stackText(shapeless.assemble(net.minecraft.world.item.crafting.CraftingInput.EMPTY, registries), serialization));
      } else if (recipe.getClass() == net.minecraft.world.item.crafting.TransmuteRecipe.class && recipe instanceof net.minecraft.world.item.crafting.TransmuteRecipe transmute) {
        var cells = new ArrayList<String>();
        for (var cell : transmute.placementInfo().ingredients()) cells.add(ingredient(cell));
        var result = (net.minecraft.world.item.crafting.TransmuteResult) transmuteResult.get(transmute);
        out.append("transmute\t").append(holder.id().location()).append('\t').append(String.join(";", cells)).append('\t')
          .append(stackText(new net.minecraft.world.item.ItemStack(result.item(), result.count(), result.components()), serialization));
      } else {
        out.append("other\t").append(holder.id().location()).append('\t').append(recipe.getClass().getName()).append('\t').append(universe(holder.id().location().toString()));
      }
      out.append('\n');
    }
    // fuel: FuelValues.vanillaBurnTimes, the item's burn time in ticks.
    var fuels = net.minecraft.world.level.block.entity.FuelValues.vanillaBurnTimes(registries, net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS);
    for (var item : fuels.fuelItems()) {
      out.append("fuel\t").append(BuiltInRegistries.ITEM.getKey(item)).append('\t').append(fuels.burnDuration(new net.minecraft.world.item.ItemStack(item))).append('\n');
    }
    // PotionBrewing.bootstrap: the containers a potion is brewed in, and the potion and container mixes in order.
    //   brewcontainer <items>
    //   potionmix <from potion> <ingredient> <to potion>
    //   containermix <from item> <ingredient> <to item>
    var brewing = net.minecraft.world.item.alchemy.PotionBrewing.bootstrap(net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS);
    for (var container : (List<?>) brewingField(brewing, "containers")) out.append("brewcontainer\t").append(ingredient((net.minecraft.world.item.crafting.Ingredient) container)).append('\n');
    for (var kind : List.of("potionMixes", "containerMixes")) {
      for (var mix : (List<?>) brewingField(brewing, kind)) {
        var mixClass = mix.getClass();
        var from = (net.minecraft.core.Holder<?>) brewingField(mix, mixClass, "from");
        var to = (net.minecraft.core.Holder<?>) brewingField(mix, mixClass, "to");
        out.append(kind.equals("potionMixes") ? "potionmix\t" : "containermix\t").append(from.unwrapKey().orElseThrow().location()).append('\t')
          .append(ingredient((net.minecraft.world.item.crafting.Ingredient) brewingField(mix, mixClass, "ingredient"))).append('\t').append(to.unwrapKey().orElseThrow().location()).append('\n');
      }
    }
    // jukebox: each song's length in seconds and comparator output.
    for (var song : registries.lookupOrThrow(net.minecraft.core.registries.Registries.JUKEBOX_SONG).listElements().sorted(java.util.Comparator.comparing(holder -> holder.key().location().toString())).toList()) {
      out.append("jukebox\t").append(song.key().location()).append('\t').append(song.value().lengthInSeconds()).append('\t').append(song.value().comparatorOutput()).append('\n');
    }
    // remainder: the item an item leaves when it is crafted with or smelted as fuel (Item.getCraftingRemainder).
    for (var item : BuiltInRegistries.ITEM) {
      var remainder = item.getCraftingRemainder();
      if (!remainder.isEmpty()) out.append("remainder\t").append(BuiltInRegistries.ITEM.getKey(item)).append('\t').append(BuiltInRegistries.ITEM.getKey(remainder.getItem())).append('\t').append(remainder.getCount()).append('\n');
    }
    // Game events, their frequencies and tags; the flammable blocks; the
    // beacon beam blocks' colors; the pressure plates' constructor arguments;
    // whether a command block is automatic; the item tag that dampens
    // vibrations.
    //   gameevent <key> <notification radius> <vibration frequency> <tags, comma-separated or ->
    //   fire <block key> <ignite odds> <burn odds>
    //   beam <block key> <DyeColor.getTextureDiffuseColor>
    //   plate <block key> <EVERYTHING|MOBS> <max weight, or - for a plate that is not weighted>
    //   automatic <block key> <true|false>
    //   itemtag <name> <item keys, comma-separated or ->
    var eventTags = new java.util.TreeMap<String, java.util.TreeSet<String>>();
    for (var tag : BuiltInRegistries.GAME_EVENT.getTags().toList()) {
      for (var holder : tag) eventTags.computeIfAbsent(BuiltInRegistries.GAME_EVENT.getKey(holder.value()).toString(), unused -> new java.util.TreeSet<>()).add(tag.key().location().toString());
    }
    for (var event : BuiltInRegistries.GAME_EVENT) {
      var key = BuiltInRegistries.GAME_EVENT.getKey(event).toString();
      var eventTagNames = eventTags.get(key);
      out.append("gameevent\t").append(key).append('\t').append(event.notificationRadius()).append('\t')
        .append(net.minecraft.world.level.gameevent.vibrations.VibrationSystem.getGameEventFrequency(BuiltInRegistries.GAME_EVENT.wrapAsHolder(event))).append('\t')
        .append(eventTagNames == null ? "-" : String.join(",", eventTagNames)).append('\n');
    }
    var fire = (net.minecraft.world.level.block.FireBlock) Blocks.FIRE;
    var burnOdds = net.minecraft.world.level.block.FireBlock.class.getDeclaredField("burnOdds");
    burnOdds.setAccessible(true);
    @SuppressWarnings("unchecked") var burn = (it.unimi.dsi.fastutil.objects.Object2IntMap<Block>) burnOdds.get(fire);
    var plateType = net.minecraft.world.level.block.BasePressurePlateBlock.class.getDeclaredField("type");
    plateType.setAccessible(true);
    var plateWeight = net.minecraft.world.level.block.WeightedPressurePlateBlock.class.getDeclaredField("maxWeight");
    plateWeight.setAccessible(true);
    var automatic = net.minecraft.world.level.block.CommandBlock.class.getDeclaredField("automatic");
    automatic.setAccessible(true);
    for (Block block : BuiltInRegistries.BLOCK) {
      var key = BuiltInRegistries.BLOCK.getKey(block);
      if (fire.igniteOdds.containsKey(block)) out.append("fire\t").append(key).append('\t').append(fire.igniteOdds.getInt(block)).append('\t').append(burn.getInt(block)).append('\n');
      if (block instanceof net.minecraft.world.level.block.BeaconBeamBlock beam) out.append("beam\t").append(key).append('\t').append(beam.getColor().getTextureDiffuseColor()).append('\n');
      if (block instanceof net.minecraft.world.level.block.BasePressurePlateBlock plate) {
        out.append("plate\t").append(key).append('\t').append(((net.minecraft.world.level.block.state.properties.BlockSetType) plateType.get(plate)).pressurePlateSensitivity()).append('\t')
          .append(plate instanceof net.minecraft.world.level.block.WeightedPressurePlateBlock ? String.valueOf(plateWeight.getInt(plate)) : "-").append('\n');
      }
      if (block instanceof net.minecraft.world.level.block.CommandBlock command) out.append("automatic\t").append(key).append('\t').append(automatic.getBoolean(command)).append('\n');
    }
    for (var tag : BuiltInRegistries.ITEM.getTags().sorted(java.util.Comparator.comparing(tag -> tag.key().location().toString())).toList()) {
      var keys = new ArrayList<String>();
      for (var holder : tag) keys.add(BuiltInRegistries.ITEM.getKey(holder.value()).toString());
      out.append("itemtag\t").append(tag.key().location()).append('\t').append(keys.isEmpty() ? "-" : String.join(",", keys)).append('\n');
    }
    // flag lines: a set of blocks by name.
    var replaceable = new ArrayList<String>();
    for (Block block : BuiltInRegistries.BLOCK) {
      search: for (Class<?> type = block.getClass(); type != Block.class; type = type.getSuperclass()) {
        for (var method : type.getDeclaredMethods()) {
          if (method.getName().equals("canBeReplaced") && method.getParameterCount() == 2
            && method.getParameterTypes()[1] == net.minecraft.world.item.context.BlockPlaceContext.class) {
            replaceable.add(BuiltInRegistries.BLOCK.getKey(block).toString());
            break search;
          }
        }
      }
    }
    out.append("flag\tcanBeReplacedContext\t").append(replaceable.isEmpty() ? "-" : String.join(",", replaceable)).append('\n');
    // itemcomponent: each item's default components that are saved, as their codec writes them (the prototypes that
    // a stack's component patch is compared with, in imports of saved data).
    var componentOps = McbenchLevel.data.registries().compositeAccess().createSerializationContext(net.minecraft.nbt.NbtOps.INSTANCE);
    for (var item : BuiltInRegistries.ITEM) {
      for (var component : item.components()) {
        if (component.type().isTransient()) continue;
        out.append("itemcomponent\t").append(BuiltInRegistries.ITEM.getKey(item)).append('\t').append(BuiltInRegistries.DATA_COMPONENT_TYPE.getKey(component.type()))
          .append('\t').append(component.encodeValue(componentOps).getOrThrow()).append('\n');
      }
    }
    // entries: the keys of the entries of the registries that saved data names (enchantments, as the data pack holds them;
    // the data component types, which a tooltip display names).
    var access = McbenchLevel.data.registries().compositeAccess();
    for (var registry : List.of(net.minecraft.core.registries.Registries.ENCHANTMENT, net.minecraft.core.registries.Registries.POTION, net.minecraft.core.registries.Registries.MOB_EFFECT, net.minecraft.core.registries.Registries.BANNER_PATTERN, net.minecraft.core.registries.Registries.DATA_COMPONENT_TYPE)) {
      var keys = new ArrayList<String>();
      access.lookupOrThrow(registry).listElementIds().forEach(key -> keys.add(key.location().toString()));
      java.util.Collections.sort(keys);
      out.append("entries\t").append(registry.location()).append('\t').append(String.join(",", keys)).append('\n');
    }
    // entitytype: every entity type's dimensions (EntityType.getDimensions: width, height, eye height) and whether fire
    // cannot hurt it, in registry order.
    for (var type : BuiltInRegistries.ENTITY_TYPE) {
      var dimensions = type.getDimensions();
      out.append("entitytype\t").append(BuiltInRegistries.ENTITY_TYPE.getKey(type)).append('\t').append(dimensions.width()).append('\t').append(dimensions.height())
        .append('\t').append(dimensions.eyeHeight()).append('\t').append(type.fireImmune() ? 1 : 0).append('\n');
    }
    // mthtab: Mth's ASIN_TAB and COS_TAB (what Mth.atan2 reads), as the raw bits of each entry, comma-separated.
    for (String name : new String[] { "ASIN_TAB", "COS_TAB" }) {
      var table = net.minecraft.util.Mth.class.getDeclaredField(name);
      table.setAccessible(true);
      var values = new ArrayList<String>();
      for (double value : (double[]) table.get(null)) values.add(Long.toHexString(Double.doubleToRawLongBits(value)));
      out.append("mthtab\t").append(name).append('\t').append(String.join(",", values)).append('\n');
    }
    // painting: the painting variants the data pack holds, in registry order (the first is the one a painting has
    // before its saved variant is read), with their size in blocks.
    access.lookupOrThrow(net.minecraft.core.registries.Registries.PAINTING_VARIANT).listElements().forEach(holder ->
      out.append("painting\t").append(holder.key().location()).append('\t').append(holder.value().width()).append('\t').append(holder.value().height()).append('\n'));
    // waxed: HoneycombItem.WAXABLES, the block each waxes to.
    for (var entry : new java.util.TreeMap<>(net.minecraft.world.item.HoneycombItem.WAXABLES.get().entrySet().stream().collect(java.util.stream.Collectors.toMap(
      e -> BuiltInRegistries.BLOCK.getKey(e.getKey()).toString(), e -> BuiltInRegistries.BLOCK.getKey(e.getValue()).toString()))).entrySet()) {
      out.append("waxed\t").append(entry.getKey()).append('\t').append(entry.getValue()).append('\n');
    }
    // The enchantments of the tag #minecraft:curse, which RepairItemRecipe carries over.
    var curses = new ArrayList<String>();
    access.lookupOrThrow(net.minecraft.core.registries.Registries.ENCHANTMENT).get(net.minecraft.tags.EnchantmentTags.CURSE).orElseThrow()
      .forEach(holder -> curses.add(holder.unwrapKey().orElseThrow().location().toString()));
    java.util.Collections.sort(curses);
    out.append("entries\t#minecraft:enchantment/curse\t").append(String.join(",", curses)).append('\n');
    Files.writeString(target, out);
  }

  // A stack as ItemStack.CODEC writes it to NBT, as text.
  static String stackText(net.minecraft.world.item.ItemStack stack, com.mojang.serialization.DynamicOps<net.minecraft.nbt.Tag> ops) {
    return stackText(stack, ops, null);
  }

  static String stackText(net.minecraft.world.item.ItemStack stack, com.mojang.serialization.DynamicOps<net.minecraft.nbt.Tag> ops, Object label) {
    return net.minecraft.world.item.ItemStack.CODEC.encodeStart(ops, stack).getOrThrow(message -> new IllegalStateException(message + " for " + stack + " of " + label)).toString();
  }

  // A field of PotionBrewing or of one of its mixes.
  static Object brewingField(Object target, String name) throws Exception {
    return brewingField(target, target.getClass(), name);
  }

  static Object brewingField(Object target, Class<?> type, String name) throws Exception {
    var field = type.getDeclaredField(name);
    field.setAccessible(true);
    return field.get(target);
  }

  // The items a recipe with its own matching code can match: its matches() returns false when any item in the
  // input is outside this set (read off each class's code; a recipe not known here can match anything).
  @SuppressWarnings("unchecked")
  static String universe(String id) throws Exception {
    var shapes = net.minecraft.world.item.crafting.FireworkStarRecipe.class.getDeclaredField("SHAPE_BY_ITEM");
    shapes.setAccessible(true);
    var shapeItems = ((Map<net.minecraft.world.item.Item, ?>) shapes.get(null)).keySet();
    var keys = new ArrayList<String>();
    for (var item : BuiltInRegistries.ITEM) {
      var stack = item.getDefaultInstance();
      boolean dye = item instanceof net.minecraft.world.item.DyeItem, banner = item instanceof net.minecraft.world.item.BannerItem;
      boolean in = switch (id) {
        case "minecraft:armor_dye" -> stack.is(net.minecraft.tags.ItemTags.DYEABLE) || dye;
        case "minecraft:banner_duplicate" -> banner;
        case "minecraft:book_cloning" -> stack.has(net.minecraft.core.component.DataComponents.WRITTEN_BOOK_CONTENT) || stack.is(net.minecraft.tags.ItemTags.BOOK_CLONING_TARGET)
          || item == net.minecraft.world.item.Items.WRITTEN_BOOK;
        case "minecraft:decorated_pot" -> stack.is(net.minecraft.tags.ItemTags.DECORATED_POT_INGREDIENTS);
        case "minecraft:firework_rocket" -> item == net.minecraft.world.item.Items.PAPER || item == net.minecraft.world.item.Items.GUNPOWDER || item == net.minecraft.world.item.Items.FIREWORK_STAR;
        case "minecraft:firework_star" -> shapeItems.contains(item) || item == net.minecraft.world.item.Items.GLOWSTONE_DUST || item == net.minecraft.world.item.Items.DIAMOND
          || item == net.minecraft.world.item.Items.GUNPOWDER || dye;
        case "minecraft:firework_star_fade" -> dye || item == net.minecraft.world.item.Items.FIREWORK_STAR;
        case "minecraft:map_extending" -> item == net.minecraft.world.item.Items.PAPER || item == net.minecraft.world.item.Items.FILLED_MAP;
        case "minecraft:map_cloning" -> stack.has(net.minecraft.core.component.DataComponents.MAP_ID) || item == net.minecraft.world.item.Items.MAP || item == net.minecraft.world.item.Items.FILLED_MAP;
        case "minecraft:repair_item" -> stack.has(net.minecraft.core.component.DataComponents.MAX_DAMAGE);
        case "minecraft:shield_decoration" -> banner || item == net.minecraft.world.item.Items.SHIELD;
        case "minecraft:tipped_arrow" -> item == net.minecraft.world.item.Items.ARROW || item == net.minecraft.world.item.Items.LINGERING_POTION;
        default -> true;
      };
      if (in) keys.add(BuiltInRegistries.ITEM.getKey(item).toString());
    }
    return String.join(",", keys);
  }

  // An ingredient as the keys of the items it accepts.
  @SuppressWarnings("deprecation")
  static String ingredient(net.minecraft.world.item.crafting.Ingredient ingredient) {
    var keys = new ArrayList<String>();
    ingredient.items().forEach(holder -> keys.add(BuiltInRegistries.ITEM.getKey(holder.value()).toString()));
    return String.join(",", keys);
  }

  // A shape's number among the dump's distinct shapes, numbered as first
  // met (shape lines, without their "shape" field).
  static String shapeNumber(Map<String, Integer> shapes, net.minecraft.world.phys.shapes.VoxelShape shape) {
    return String.valueOf(shapes.computeIfAbsent(shapeLine(shape), added -> { shapeObjects.add(shape); return shapes.size(); }));
  }

  // A face shape's number among the occlusion shapes, and the first shape met
  // of each (the one the pairs are tested with).
  static int faceNumber(Map<String, Integer> faces, List<net.minecraft.world.phys.shapes.VoxelShape> representatives, net.minecraft.world.phys.shapes.VoxelShape shape) {
    return faces.computeIfAbsent(shapeLine(shape), added -> { representatives.add(shape); return faces.size(); });
  }

  static String shapeLine(net.minecraft.world.phys.shapes.VoxelShape shape) {
    var line = new StringBuilder();
    for (var axis : net.minecraft.core.Direction.Axis.values()) {
      var coords = shape.getCoords(axis);
      for (int i = 0; i < coords.size(); i++) line.append(i == 0 ? "" : ",").append(coords.getDouble(i));
      line.append('\t');
    }
    var voxels = shape.shape;
    line.append(voxels.getXSize()).append(',').append(voxels.getYSize()).append(',').append(voxels.getZSize()).append('\t');
    for (int x = 0; x < voxels.getXSize(); x++) for (int y = 0; y < voxels.getYSize(); y++) for (int z = 0; z < voxels.getZSize(); z++) line.append(voxels.isFull(x, y, z) ? '1' : '0');
    var box = shape.moonrise$getSingleAABBRepresentation();
    line.append('\t').append(box == null ? "-" : box.minX + "," + box.minY + "," + box.minZ + "," + box.maxX + "," + box.maxY + "," + box.maxZ);
    return line.toString();
  }

  // What the live world holds besides its blocks, for comparing engines
  // (scripts/core-parity.js), one item per line: the game time, the level
  // random's seed, the next sub-tick order; every scheduled block tick
  // (x y z block trigger priority sub-tick order, by sub-tick order), then
  // every scheduled fluid tick (the same with the fluid's key);
  // every queued block event in order (x y z block a b); every block
  // entity (x y z type data, by position, its saved data with keys in
  // order); the block entity tickers in order (x y z, or removed), then
  // those pending.
  static void state(Path target) throws Exception {
    var out = new StringBuilder();
    out.append("time ").append(level.getGameTime()).append('\n');
    out.append("random ").append(McbenchExperiment.randomState(level.random)).append('\n');
    var subTick = net.minecraft.world.level.Level.class.getDeclaredField("subTickCount");
    subTick.setAccessible(true);
    out.append("subtick ").append(subTick.getLong(level)).append('\n');
    var ticks = new ArrayList<net.minecraft.world.ticks.ScheduledTick<Block>>();
    for (var chunk : level.chunkMap.values()) ((net.minecraft.world.ticks.LevelChunkTicks<Block>) chunk.getBlockTicks()).getAll().forEach(ticks::add);
    ticks.sort(java.util.Comparator.comparingLong(net.minecraft.world.ticks.ScheduledTick::subTickOrder));
    for (var tick : ticks) {
      out.append("tick ").append(tick.pos().getX()).append(' ').append(tick.pos().getY()).append(' ').append(tick.pos().getZ()).append(' ')
        .append(BuiltInRegistries.BLOCK.getKey(tick.type())).append(' ').append(tick.triggerTick()).append(' ').append(tick.priority().getValue())
        .append(' ').append(tick.subTickOrder()).append('\n');
    }
    var fluidTicks = new ArrayList<net.minecraft.world.ticks.ScheduledTick<net.minecraft.world.level.material.Fluid>>();
    for (var chunk : level.chunkMap.values()) ((net.minecraft.world.ticks.LevelChunkTicks<net.minecraft.world.level.material.Fluid>) chunk.getFluidTicks()).getAll().forEach(fluidTicks::add);
    fluidTicks.sort(java.util.Comparator.comparingLong(net.minecraft.world.ticks.ScheduledTick::subTickOrder));
    for (var tick : fluidTicks) {
      out.append("fluidtick ").append(tick.pos().getX()).append(' ').append(tick.pos().getY()).append(' ').append(tick.pos().getZ()).append(' ')
        .append(BuiltInRegistries.FLUID.getKey(tick.type())).append(' ').append(tick.triggerTick()).append(' ').append(tick.priority().getValue())
        .append(' ').append(tick.subTickOrder()).append('\n');
    }
    var events = (it.unimi.dsi.fastutil.objects.ObjectLinkedOpenHashSet<net.minecraft.world.level.BlockEventData>) McbenchLevel.blockEventsField.get(level);
    for (var event : events) {
      out.append("event ").append(event.pos().getX()).append(' ').append(event.pos().getY()).append(' ').append(event.pos().getZ()).append(' ')
        .append(BuiltInRegistries.BLOCK.getKey(event.block())).append(' ').append(event.paramA()).append(' ').append(event.paramB()).append('\n');
    }
    var entities = new ArrayList<net.minecraft.world.level.block.entity.BlockEntity>(level.blockEntities());
    entities.sort(java.util.Comparator.<net.minecraft.world.level.block.entity.BlockEntity>comparingInt(e -> e.getBlockPos().getX())
      .thenComparingInt(e -> e.getBlockPos().getY()).thenComparingInt(e -> e.getBlockPos().getZ()));
    for (var entity : entities) {
      out.append("blockentity ").append(entity.getBlockPos().getX()).append(' ').append(entity.getBlockPos().getY()).append(' ').append(entity.getBlockPos().getZ())
        .append(' ').append(BuiltInRegistries.BLOCK_ENTITY_TYPE.getKey(entity.getType())).append(' ');
      snbt(out, entity.saveWithoutMetadata(level.registryAccess()));
      out.append('\n');
    }
    var pending = net.minecraft.world.level.Level.class.getDeclaredField("pendingBlockEntityTickers");
    pending.setAccessible(true);
    for (var list : List.of(level.blockEntityTickers, (List<net.minecraft.world.level.block.entity.TickingBlockEntity>) pending.get(level))) {
      for (var ticker : list) {
        if (ticker.isRemoved()) out.append("ticker removed\n");
        else out.append("ticker ").append(ticker.getPos().getX()).append(' ').append(ticker.getPos().getY()).append(' ').append(ticker.getPos().getZ()).append('\n');
      }
      if (list == level.blockEntityTickers) out.append("pending\n");
    }
    // The game event listeners of each chunk section that has any (in the
    // chunks' key order, then the section's y: "listeners <chunk x> <chunk z>
    // <section y>:" and each listener's block position, in the order it
    // registered), and then each beacon's hidden state ("beacon <x> <y> <z>
    // <lastCheckY> <beam sections> <sections being checked>", each as
    // "<color>:<height>" comma-separated or -).
    var registriesField = net.minecraft.world.level.chunk.LevelChunk.class.getDeclaredField("gameEventListenerRegistrySections");
    registriesField.setAccessible(true);
    var listenersField = net.minecraft.world.level.gameevent.EuclideanGameEventListenerRegistry.class.getDeclaredField("listeners");
    listenersField.setAccessible(true);
    var listenerChunks = level.chunkMap.keySet().toLongArray();
    java.util.Arrays.sort(listenerChunks);
    for (long key : listenerChunks) {
      @SuppressWarnings("unchecked") var registries = (it.unimi.dsi.fastutil.ints.Int2ObjectMap<net.minecraft.world.level.gameevent.GameEventListenerRegistry>) registriesField.get(level.chunkMap.get(key));
      var sectionYs = registries.keySet().toIntArray();
      java.util.Arrays.sort(sectionYs);
      for (int sectionY : sectionYs) {
        @SuppressWarnings("unchecked") var listeners = (List<net.minecraft.world.level.gameevent.GameEventListener>) listenersField.get(registries.get(sectionY));
        if (listeners.isEmpty()) continue;
        out.append("listeners ").append(ChunkPos.getX(key)).append(' ').append(ChunkPos.getZ(key)).append(' ').append(sectionY).append(':');
        for (int i = 0; i < listeners.size(); i++) {
          var at = ((net.minecraft.world.level.gameevent.BlockPositionSource) listeners.get(i).getListenerSource()).pos();
          out.append(i > 0 ? " " : "").append(at.getX()).append(',').append(at.getY()).append(',').append(at.getZ());
        }
        out.append('\n');
      }
    }
    var lastCheckY = net.minecraft.world.level.block.entity.BeaconBlockEntity.class.getDeclaredField("lastCheckY");
    lastCheckY.setAccessible(true);
    var beamSections = net.minecraft.world.level.block.entity.BeaconBlockEntity.class.getDeclaredField("beamSections");
    beamSections.setAccessible(true);
    var checkingSections = net.minecraft.world.level.block.entity.BeaconBlockEntity.class.getDeclaredField("checkingBeamSections");
    checkingSections.setAccessible(true);
    for (var entity : entities) {
      if (!(entity instanceof net.minecraft.world.level.block.entity.BeaconBlockEntity beacon)) continue;
      out.append("beacon ").append(beacon.getBlockPos().getX()).append(' ').append(beacon.getBlockPos().getY()).append(' ').append(beacon.getBlockPos().getZ())
        .append(' ').append(lastCheckY.getInt(beacon));
      for (var field : new java.lang.reflect.Field[] { beamSections, checkingSections }) {
        @SuppressWarnings("unchecked") var sections = (List<net.minecraft.world.level.block.entity.BeaconBeamOwner.Section>) field.get(beacon);
        out.append(' ');
        if (sections.isEmpty()) out.append('-');
        for (int i = 0; i < sections.size(); i++) out.append(i > 0 ? "," : "").append(sections.get(i).getColor()).append(':').append(sections.get(i).getHeight());
      }
      out.append('\n');
    }
    // The entity layer: the entity id counter, Entity.SHARED_RANDOM's seed, each loot random sequence's
    // generator, and each entity's fields (doubles and floats as raw bits), in the entity list's order.
    var counter = net.minecraft.world.entity.Entity.class.getDeclaredField("ENTITY_COUNTER");
    counter.setAccessible(true);
    out.append("entityid ").append(((java.util.concurrent.atomic.AtomicInteger) counter.get(null)).get()).append('\n');
    var sharedValue = Class.forName("ca.spottedleaf.moonrise.common.util.ThreadUnsafeRandom").getDeclaredField("value");
    sharedValue.setAccessible(true);
    out.append("sharedrandom ").append(sharedValue.getLong(net.minecraft.world.entity.Entity.SHARED_RANDOM)).append('\n');
    var sequencesField = net.minecraft.server.level.ServerLevel.class.getDeclaredField("randomSequences");
    sequencesField.setAccessible(true);
    var sequencesMap = net.minecraft.world.RandomSequences.class.getDeclaredField("sequences");
    sequencesMap.setAccessible(true);
    var sourceField = net.minecraft.world.RandomSequence.class.getDeclaredField("source");
    sourceField.setAccessible(true);
    var generatorField = net.minecraft.world.level.levelgen.XoroshiroRandomSource.class.getDeclaredField("randomNumberGenerator");
    generatorField.setAccessible(true);
    var seedLo = net.minecraft.world.level.levelgen.Xoroshiro128PlusPlus.class.getDeclaredField("seedLo");
    seedLo.setAccessible(true);
    var seedHi = net.minecraft.world.level.levelgen.Xoroshiro128PlusPlus.class.getDeclaredField("seedHi");
    seedHi.setAccessible(true);
    var sequences = new java.util.TreeMap<String, Object>();
    for (var sequence : ((Map<?, ?>) sequencesMap.get(sequencesField.get(level))).entrySet()) sequences.put(sequence.getKey().toString(), sequence.getValue());
    for (var sequence : sequences.entrySet()) {
      var generator = generatorField.get(sourceField.get(sequence.getValue()));
      out.append("sequence ").append(sequence.getKey()).append(' ').append(seedLo.getLong(generator)).append(' ').append(seedHi.getLong(generator)).append('\n');
    }
    entityLines(out, level);
    // The light engine: the changes gathered (per chunk in the order they were
    // first gathered, positions sorted, sections n unchanged, t became empty,
    // f became not empty), then, once they are lit, each chunk's light
    // (nibbleToken) and heightmaps.
    var light = ((McbenchLevel.Chunks) level.getChunkSource()).light;
    out.append("skydarken ").append(level.getSkyDarken()).append('\n');
    for (var entry : light.pending.entrySet()) {
      var positions = new ArrayList<BlockPos>(entry.getValue().positions());
      positions.sort(java.util.Comparator.<BlockPos>comparingInt(BlockPos::getX).thenComparingInt(BlockPos::getY).thenComparingInt(BlockPos::getZ));
      var names = new ArrayList<String>();
      for (var at : positions) names.add(at.getX() + "," + at.getY() + "," + at.getZ());
      var sections = new StringBuilder();
      for (Boolean section : entry.getValue().sections()) sections.append(section == null ? 'n' : section ? 't' : 'f');
      out.append("pending ").append(ChunkPos.getX(entry.getKey())).append(' ').append(ChunkPos.getZ(entry.getKey())).append(' ')
        .append(names.isEmpty() ? "-" : String.join(";", names)).append(' ').append(sections).append('\n');
    }
    light.flush();
    var chunkKeys = level.chunkMap.keySet().toLongArray();
    var sortedKeys = new ArrayList<Long>();
    for (long key : chunkKeys) sortedKeys.add(key);
    sortedKeys.sort(java.util.Comparator.<Long>comparingInt(ChunkPos::getX).thenComparingInt(ChunkPos::getZ));
    for (long key : sortedKeys) {
      var chunk = level.chunkMap.get(key);
      out.append("light ").append(ChunkPos.getX(key)).append(' ').append(ChunkPos.getZ(key)).append(' ').append(chunk.isLightCorrect() ? 1 : 0).append('\n');
      for (boolean sky : new boolean[] { true, false }) {
        boolean[] empty = sky ? chunk.starlight$getSkyEmptinessMap() : chunk.starlight$getBlockEmptinessMap();
        var map = new StringBuilder();
        if (empty == null) map.append('-'); else for (boolean e : empty) map.append(e ? '1' : '0');
        out.append(sky ? "sky " : "block ").append(map);
        for (var nibble : sky ? chunk.starlight$getSkyNibbles() : chunk.starlight$getBlockNibbles()) out.append(' ').append(nibbleToken(nibble));
        out.append('\n');
      }
    }
    for (long key : sortedKeys) {
      var chunk = level.chunkMap.get(key);
      out.append("heightmaps ").append(ChunkPos.getX(key)).append(' ').append(ChunkPos.getZ(key)).append('\n');
      var maps = new java.util.EnumMap<net.minecraft.world.level.levelgen.Heightmap.Types, net.minecraft.world.level.levelgen.Heightmap>(net.minecraft.world.level.levelgen.Heightmap.Types.class);
      for (var map : chunk.getHeightmaps()) maps.put(map.getKey(), map.getValue());
      for (var type : new net.minecraft.world.level.levelgen.Heightmap.Types[] { net.minecraft.world.level.levelgen.Heightmap.Types.MOTION_BLOCKING,
        net.minecraft.world.level.levelgen.Heightmap.Types.MOTION_BLOCKING_NO_LEAVES, net.minecraft.world.level.levelgen.Heightmap.Types.OCEAN_FLOOR,
        net.minecraft.world.level.levelgen.Heightmap.Types.WORLD_SURFACE }) {
        var longs = new ArrayList<String>();
        for (long value : maps.get(type).getRawData()) longs.add(Long.toHexString(value));
        out.append(type.getSerializationKey()).append(' ').append(String.join(",", longs)).append('\n');
      }
    }
    Files.writeString(target, out);
  }

  // Each entity's fields as state writes them (doubles and floats as raw bits), in the entity list's order. The
  // kinds that have hidden fields beyond the common ones write them after the common ones (see the Rust
  // entity::state_text, which writes the same).
  static void entityLines(StringBuilder out, McbenchLevel level) throws Exception {
    for (var entity : level.getEntities().getAll()) {
      out.append("entity ").append(entity.getId()).append(' ').append(entity.getUUID().getMostSignificantBits()).append(' ').append(entity.getUUID().getLeastSignificantBits());
      for (double value : new double[] { entity.getX(), entity.getY(), entity.getZ(), entity.getDeltaMovement().x, entity.getDeltaMovement().y, entity.getDeltaMovement().z, entity.xo, entity.yo, entity.zo }) {
        out.append(' ').append(Long.toHexString(Double.doubleToRawLongBits(value)));
      }
      out.append(' ').append(Integer.toHexString(Float.floatToRawIntBits(entity.getYRot()))).append(' ').append(Integer.toHexString(Float.floatToRawIntBits(entity.getXRot())));
      out.append(' ').append(entity.onGround() ? 1 : 0).append(entity.horizontalCollision ? 1 : 0).append(entity.verticalCollision ? 1 : 0).append(entity.hasImpulse ? 1 : 0)
        .append(entity.isRemoved() ? 1 : 0).append(' ').append(entity.tickCount).append(' ').append(entity.portalCooldown);
      out.append(' ').append(entity.mainSupportingBlockPos.map(p -> p.getX() + "," + p.getY() + "," + p.getZ()).orElse("-"));
      if (entity instanceof net.minecraft.world.entity.item.ItemEntity item) {
        out.append(" item ").append(BuiltInRegistries.ITEM.getKey(item.getItem().getItem())).append(' ').append(item.getItem().getCount()).append(' ').append(item.age).append(' ').append(item.pickupDelay).append(' ').append(item.health);
      }
      // A falling block: its state, time, drop flags (dropItem, cancelDrop, hurtEntities), fall damage per distance (raw bits), maximum, start position.
      if (entity instanceof net.minecraft.world.entity.item.FallingBlockEntity falling) {
        out.append(" falling ").append(BlockStateParser.serialize(falling.getBlockState())).append(' ').append(falling.time).append(' ')
          .append(falling.dropItem ? 1 : 0).append(falling.cancelDrop ? 1 : 0).append(falling.hurtEntities ? 1 : 0).append(' ')
          .append(Integer.toHexString(Float.floatToRawIntBits(falling.fallDamagePerDistance))).append(' ').append(falling.fallDamageMax).append(' ')
          .append(falling.getStartPos().getX()).append(',').append(falling.getStartPos().getY()).append(',').append(falling.getStartPos().getZ());
      }
      // Primed TNT: its fuse, explosion power (raw bits) and block state.
      if (entity instanceof net.minecraft.world.entity.item.PrimedTnt tnt) {
        out.append(" tnt ").append(tnt.getFuse()).append(' ').append(Integer.toHexString(Float.floatToRawIntBits(tnt.explosionPower))).append(' ').append(BlockStateParser.serialize(tnt.getBlockState()));
      }
      entityKindFields(out, entity);
      // The fall distance (raw bits), the fire ticks, whether the entity is in water, the ticks frozen and whether it is in powder snow.
      out.append(" fd ").append(Long.toHexString(Double.doubleToRawLongBits(entity.fallDistance))).append(" fire ").append(entity.getRemainingFireTicks()).append(entity.isInWater() ? " 1" : " 0")
        .append(" frozen ").append(entity.getTicksFrozen()).append(entity.isInPowderSnow ? " 1" : " 0");
      // The portal process: how long the entity has been in the portal, whether it is in it this tick, and where it came in.
      if (entity.portalProcess == null) out.append(" portal -");
      else out.append(" portal ").append(entity.portalProcess.getPortalTime()).append(entity.portalProcess.isInsidePortalThisTick() ? " 1 " : " 0 ")
        .append(entity.portalProcess.getEntryPosition().getX()).append(',').append(entity.portalProcess.getEntryPosition().getY()).append(',').append(entity.portalProcess.getEntryPosition().getZ());
      out.append('\n');
    }
  }

  static String hex(float value) { return Integer.toHexString(Float.floatToRawIntBits(value)); }
  static String hex(double value) { return Long.toHexString(Double.doubleToRawLongBits(value)); }

  // The hidden fields of the vehicles, paintings and armor stands.
  static void entityKindFields(StringBuilder out, net.minecraft.world.entity.Entity entity) throws Exception {
    if (entity instanceof net.minecraft.world.entity.vehicle.AbstractMinecart cart) {
      // A minecart: its type, whether it is on rails, flipped, in its first tick; hurt time, direction, damage (raw bits).
      out.append(" cart ").append(BuiltInRegistries.ENTITY_TYPE.getKey(cart.getType())).append(' ').append(cart.isOnRails() ? 1 : 0).append(cart.isFlipped() ? 1 : 0)
        .append(cart.isFirstTick() ? 1 : 0).append(' ').append(cart.getHurtTime()).append(' ').append(cart.getHurtDir()).append(' ').append(hex(cart.getDamage()));
      if (cart instanceof net.minecraft.world.entity.vehicle.MinecartHopper hopper) {
        out.append(" hopper ").append(hopper.isEnabled() ? 1 : 0).append(field(net.minecraft.world.entity.vehicle.MinecartHopper.class, "consumedItemThisFrame").getBoolean(hopper) ? 1 : 0);
      }
      if (cart instanceof net.minecraft.world.entity.vehicle.MinecartFurnace furnace) {
        var hasFuel = net.minecraft.world.entity.vehicle.MinecartFurnace.class.getDeclaredMethod("hasFuel");
        hasFuel.setAccessible(true);
        out.append(" furnace ").append(furnace.fuel).append(' ').append(hex(furnace.push.x)).append(' ').append(hex(furnace.push.z)).append(' ').append((boolean) hasFuel.invoke(furnace) ? 1 : 0);
      }
      if (cart instanceof net.minecraft.world.entity.vehicle.MinecartTNT tnt) {
        out.append(" tntcart ").append(tnt.fuse).append(' ').append(hex(tnt.explosionPowerBase)).append(' ').append(hex(tnt.explosionSpeedFactor)).append(' ')
          .append(field(net.minecraft.world.entity.vehicle.MinecartTNT.class, "ignitionSource").get(tnt) != null ? 1 : 0);
      }
    }
    if (entity instanceof net.minecraft.world.entity.decoration.Painting painting) {
      // A painting: its variant, direction and the counter to its next survival check.
      out.append(" painting ").append(painting.getVariant().unwrapKey().orElseThrow().location()).append(' ').append(painting.getDirection().getSerializedName()).append(' ')
        .append(field(net.minecraft.world.entity.decoration.BlockAttachedEntity.class, "checkInterval").getInt(painting));
    }
    if (entity instanceof net.minecraft.world.entity.vehicle.AbstractBoat boat) {
      // A boat: status and old status, whether it is in its first tick, the water level, land friction, last vertical speed, out of control
      // ticks and delta rotation (raw bits), the bubble time, whether it is above a bubble column and which way it pulls, hurt time, direction, damage.
      var type = net.minecraft.world.entity.vehicle.AbstractBoat.class;
      var bubbleTime = type.getDeclaredMethod("getBubbleTime");
      bubbleTime.setAccessible(true);
      var oldStatus = field(type, "oldStatus").get(boat);
      out.append(" boat ").append(boat.status == null ? "-" : boat.status.name()).append(' ').append(oldStatus == null ? "-" : oldStatus.toString()).append(' ')
        .append(field(net.minecraft.world.entity.Entity.class, "firstTick").getBoolean(boat) ? 1 : 0).append(' ').append(hex(field(type, "waterLevel").getDouble(boat))).append(' ').append(hex(field(type, "landFriction").getFloat(boat)))
        .append(' ').append(hex(field(type, "lastYd").getDouble(boat))).append(' ').append(hex(field(type, "outOfControlTicks").getFloat(boat)))
        .append(' ').append(hex(field(type, "deltaRotation").getFloat(boat))).append(' ').append(bubbleTime.invoke(boat)).append(' ')
        .append(field(type, "isAboveBubbleColumn").getBoolean(boat) ? 1 : 0).append(field(type, "bubbleColumnDirectionIsDown").getBoolean(boat) ? 1 : 0)
        .append(' ').append(boat.getHurtTime()).append(' ').append(boat.getHurtDir()).append(' ').append(hex(boat.getDamage()));
    }
    if (entity instanceof net.minecraft.world.entity.decoration.ArmorStand stand) {
      // An armor stand: health (raw bits), hurt time, death time, air, and the flags small, arms, base plate hidden, marker, invisible.
      out.append(" stand ").append(hex(stand.getHealth())).append(' ').append(stand.hurtTime).append(' ').append(stand.deathTime).append(' ').append(stand.getAirSupply()).append(' ')
        .append(stand.isSmall() ? 1 : 0).append(stand.showArms() ? 1 : 0).append(stand.showBasePlate() ? 0 : 1).append(stand.isMarker() ? 1 : 0).append(stand.isInvisible() ? 1 : 0);
    }
    if (entity instanceof net.minecraft.world.entity.ExperienceOrb orb) {
      // An experience orb: its value, count, age and health.
      var type = net.minecraft.world.entity.ExperienceOrb.class;
      out.append(" orb ").append(orb.getValue()).append(' ').append(orb.count).append(' ').append(field(type, "age").getInt(orb)).append(' ').append(field(type, "health").getInt(orb));
    }
    if (entity instanceof net.minecraft.world.entity.vehicle.AbstractMinecart || entity instanceof net.minecraft.world.entity.vehicle.AbstractBoat || entity instanceof net.minecraft.world.entity.decoration.ArmorStand) {
      // The distances movement counted for steps (raw bits).
      out.append(" move ").append(hex(entity.moveDist)).append(' ').append(hex(entity.flyDist)).append(' ').append(hex(field(net.minecraft.world.entity.Entity.class, "nextStep").getFloat(entity)));
    }
  }

  // A Starlight nibble as state writes it: its visible and updating states,
  // d if it holds changes to make visible, the FNV-1a hash of its visible
  // bytes after a colon and of its working bytes, where it has others, after
  // a slash.
  static String nibbleToken(ca.spottedleaf.moonrise.patches.starlight.light.SWMRNibbleArray nibble) throws ReflectiveOperationException {
    var type = ca.spottedleaf.moonrise.patches.starlight.light.SWMRNibbleArray.class;
    var token = new StringBuilder();
    token.append(field(type, "stateVisible").getInt(nibble)).append(field(type, "stateUpdating").getInt(nibble));
    if (field(type, "updatingDirty").getBoolean(nibble)) token.append('d');
    byte[] visible = (byte[]) field(type, "storageVisible").get(nibble), updating = (byte[]) field(type, "storageUpdating").get(nibble);
    if (visible != null) token.append(':').append(String.format("%016x", fnv(visible)));
    if (updating != null && updating != visible) token.append('/').append(String.format("%016x", fnv(updating)));
    return token.toString();
  }

  static java.lang.reflect.Field field(Class<?> type, String name) throws ReflectiveOperationException {
    var field = type.getDeclaredField(name);
    field.setAccessible(true);
    return field;
  }

  static long fnv(byte[] bytes) {
    long hash = 0xcbf29ce484222325L;
    for (byte value : bytes) {
      hash ^= value & 0xFFL;
      hash *= 0x100000001b3L;
    }
    return hash;
  }

  // A tag as SNBT with each compound's keys in order.
  static void snbt(StringBuilder out, net.minecraft.nbt.Tag tag) {
    if (tag instanceof net.minecraft.nbt.CompoundTag compound) {
      out.append('{');
      var keys = new ArrayList<>(compound.keySet());
      java.util.Collections.sort(keys);
      for (int i = 0; i < keys.size(); i++) {
        if (i > 0) out.append(',');
        out.append(keys.get(i)).append(':');
        snbt(out, compound.get(keys.get(i)));
      }
      out.append('}');
    } else if (tag instanceof net.minecraft.nbt.ListTag list) {
      out.append('[');
      for (int i = 0; i < list.size(); i++) {
        if (i > 0) out.append(',');
        snbt(out, list.get(i));
      }
      out.append(']');
    } else {
      out.append(tag);
    }
  }

  @SuppressWarnings("unchecked")
  static <T extends Comparable<T>> String valueName(Property<T> property, Object value) {
    return property.getName((T) value);
  }

  // A palette entry as blueprints write it: the block's id, then its
  // properties.
  static void writeState(DataOutputStream out, BlockState state) throws java.io.IOException {
    out.writeUTF(BuiltInRegistries.BLOCK.getKey(state.getBlock()).toString());
    var values = state.getValues();
    out.writeInt(values.size());
    for (var property : values.keySet()) {
      out.writeUTF(property.getName());
      out.writeUTF(name(property, state));
    }
  }

  // The live world's entities as the viewer draws them: each one's id, type,
  // position, rotation and box, and what it shows (an item, a block).
  static void writeEntities(Path target) throws Exception {
    var list = new com.google.gson.JsonArray();
    for (var entity : level.getEntities().getAll()) {
      var value = new JsonObject();
      value.addProperty("id", entity.getId());
      value.addProperty("type", BuiltInRegistries.ENTITY_TYPE.getKey(entity.getType()).toString());
      var pos = new com.google.gson.JsonArray();
      pos.add(entity.getX()); pos.add(entity.getY()); pos.add(entity.getZ());
      value.add("pos", pos);
      value.addProperty("yaw", entity.getYRot());
      value.addProperty("pitch", entity.getXRot());
      value.addProperty("width", entity.getBbWidth());
      value.addProperty("height", entity.getBbHeight());
      net.minecraft.world.item.ItemStack item = null;
      if (entity instanceof net.minecraft.world.entity.item.ItemEntity drop) item = drop.getItem();
      if (entity instanceof net.minecraft.world.entity.decoration.ItemFrame frame) {
        item = frame.getItem();
        value.addProperty("rotation", frame.getRotation());
      }
      if (entity instanceof net.minecraft.world.entity.decoration.ArmorStand stand) item = stand.getMainHandItem();
      if (item != null && !item.isEmpty()) {
        value.addProperty("item", BuiltInRegistries.ITEM.getKey(item.getItem()).toString());
        value.addProperty("count", item.getCount());
      }
      if (entity instanceof net.minecraft.world.entity.decoration.HangingEntity hanging) value.addProperty("facing", hanging.getDirection().getSerializedName());
      if (entity instanceof net.minecraft.world.entity.decoration.Painting painting) value.addProperty("variant", painting.getVariant().unwrapKey().orElseThrow().location().toString());
      if (entity instanceof net.minecraft.world.entity.item.FallingBlockEntity falling) value.addProperty("block", BlockStateParser.serialize(falling.getBlockState()));
      if (entity instanceof net.minecraft.world.entity.item.PrimedTnt tnt) {
        value.addProperty("block", BlockStateParser.serialize(tnt.getBlockState()));
        value.addProperty("fuse", tnt.getFuse());
      }
      if (entity instanceof net.minecraft.world.entity.vehicle.AbstractMinecart cart) value.addProperty("block", BlockStateParser.serialize(cart.getDisplayBlockState()));
      list.add(value);
    }
    Path partial = target.resolveSibling(target.getFileName() + ".part");
    Files.writeString(partial, list.toString());
    Files.move(partial, target, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
  }

  static <T extends Comparable<T>> String name(Property<T> property, BlockState state) {
    return property.getName(state.getValue(property));
  }
}
