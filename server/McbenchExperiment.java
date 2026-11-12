import com.google.gson.*;
import com.google.gson.stream.JsonWriter;
import net.minecraft.core.BlockPos;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.level.block.ButtonBlock;
import net.minecraft.world.level.block.LeverBlock;
import org.bukkit.World;
import org.bukkit.craftbukkit.CraftWorld;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.nio.file.*;
import java.io.*;
import java.nio.charset.StandardCharsets;

/**
 * One verifier experiment on a world that holds the placed build: imports its
 * block entities, entities and scheduled ticks, then at the end of each tick
 * applies its inputs, reads its probes and writes report.json, result.json
 * and progress.json to its folder. Two hosts run it: the Paper plugin
 * (McbenchVerify), the reference, and the engine (McbenchEngine), which ticks
 * its own level.
 */
@SuppressWarnings("removal")
public final class McbenchExperiment {
    // What a host does when the experiment ends, before its result is read:
    // Paper stops its ticks and waits out its light worker.
    public interface Host { void stopped(); }

    private static final Gson gson = new Gson();
    private final JsonObject request;
    private final World world;
    private final Path folder;
    private final Host host;
    private JsonObject result;
    private final JsonArray inputs = new JsonArray();
    private JsonArray actions;
    private BlockPos[] probes;
    private boolean[] inventoryProbes;
    private boolean reportMissingInventories, observeItemEntities, observeWorldInventories, observeLivingEntities;
    private JsonArray pendingItemEntities, pendingWorldInventories, pendingLivingEntities;
    private JsonArray[] previousInventories, pendingInventories;
    private JsonArray importedBlockEntities, importedEntities;
    private net.minecraft.world.item.ItemStack[][] inventorySnapshots;
    private JsonArray[] inventoryCache;
    private net.minecraft.server.level.ServerPlayer interactionPlayer;
    private final java.util.Map<BlockPos, net.minecraft.world.item.ItemStack> heldLecternBooks = new java.util.HashMap<>();

    private int[] samples;
    private JsonWriter report;
    private long reportBytes;
    private long reportLimit;
    private long startTime;
    private long probeNanos;
    private long maxProbeNanos;
    private long maxHeapBytes;
    private long observationCount;
    private int actionIndex;
    private int sampleIndex;
    private int failedInputs;
    private int lastTick = -1;
    private boolean running;
    private String[] previousBlocks;
    private int[] previousSignals;
    private boolean[] changedProbes;
    private long changeCount;
    private final JsonArray previewInitial = new JsonArray();
    private final JsonArray previewChanges = new JsonArray();
    private final JsonArray inputFailures = new JsonArray();
    private int previewBytes;
    private boolean compactReport;
    private final java.util.Map<String,String> migratedBlockPalette = new java.util.LinkedHashMap<>();
    private int pendingStart = -1, pendingEnd;
    private String[] pendingBlocks;
    private int[] pendingSignals;
    private long lastProgressNanos;


    public McbenchExperiment(JsonObject request, World world, Path folder, Host host) throws IOException {
        this.request = request;
        this.world = world;
        this.folder = folder;
        this.host = host;
        if(request.has("block_data_version") && request.get("block_data_version").getAsInt()>net.minecraft.SharedConstants.getCurrentVersion().dataVersion().version()) throw new IOException("Cannot import future block data version");
        probes = new BlockPos[request.getAsJsonArray("probes").size()];
        for (int i = 0; i < probes.length; i++) probes[i] = position(request.getAsJsonArray("probes").get(i).getAsJsonArray());
        reportMissingInventories = request.has("inventory_missing") && request.get("inventory_missing").getAsString().equals("report");
        observeItemEntities = request.has("item_entities") && request.get("item_entities").getAsBoolean();
        observeWorldInventories = request.has("world_inventories") && request.get("world_inventories").getAsBoolean();
        observeLivingEntities = request.has("living_entities") && request.get("living_entities").getAsBoolean();
        inventoryProbes = new boolean[probes.length];
        if (request.has("inventory_probes")) for (JsonElement entry : request.getAsJsonArray("inventory_probes")) {
            BlockPos at = position(entry.getAsJsonArray());
            for (int i = 0; i < probes.length; i++) if (probes[i].equals(at)) inventoryProbes[i] = true;
        }
        previousInventories = new JsonArray[probes.length]; pendingInventories = null;
        inventorySnapshots = new net.minecraft.world.item.ItemStack[probes.length][]; inventoryCache = new JsonArray[probes.length];
        previousBlocks = new String[probes.length];
        previousSignals = new int[probes.length];
        changedProbes = new boolean[probes.length];
        compactReport = request.has("reportEncoding") && request.get("reportEncoding").getAsString().equals("rle-v1");
        if (request.has("sampleSequence")) {
            JsonObject sequence = request.getAsJsonObject("sampleSequence");
            int start = sequence.get("start").getAsInt(), step = sequence.get("step").getAsInt();
            samples = new int[sequence.get("count").getAsInt()];
            for (int i = 0; i < samples.length; i++) samples[i] = Math.toIntExact((long)start + (long)i * step);
        } else {
            samples = new int[request.getAsJsonArray("samples").size()];
            for (int i = 0; i < samples.length; i++) samples[i] = request.getAsJsonArray("samples").get(i).getAsInt();
        }
        actions = request.getAsJsonArray("actions");
    }

    // A seeded experiment's world: the level's random, the server's entity
    // ids and Paper's shared entity random all start from the seed, so
    // engines' draws can be compared (scripts/ticker-parity.js). A test
    // without a seed keeps Minecraft's own unseeded randomness.
    public static void seed(ServerLevel level, long seed) throws ReflectiveOperationException {
        var random = net.minecraft.world.level.Level.class.getDeclaredField("random");
        random.setAccessible(true);
        random.set(level, net.minecraft.util.RandomSource.create(seed));
        // Entity ids come from one counter for the whole server, and items
        // read theirs as they move: a seeded run numbers its entities from
        // a fixed point.
        var counter = net.minecraft.world.entity.Entity.class.getDeclaredField("ENTITY_COUNTER");
        counter.setAccessible(true);
        ((java.util.concurrent.atomic.AtomicInteger) counter.get(null)).set(SEEDED_ENTITY_IDS);
        // Paper gives every entity one shared random, which draws an item's
        // scatter and an entity's UUID: a seeded run starts it from the seed,
        // as Paper makes it.
        var shared = Class.forName("net.minecraft.world.entity.Entity$RandomRandomSource").getDeclaredConstructor(long.class);
        shared.setAccessible(true);
        net.minecraft.world.entity.Entity.SHARED_RANDOM = (net.minecraft.util.RandomSource) shared.newInstance(seed);
        // A new world's UUID is random, and entities save it.
        setFinal(level, ServerLevel.class.getDeclaredField("uuid"), new java.util.UUID(seed, seed));
        // The loot random sequences start from the world seed, which Paper takes
        // from the server's main world, not the one made with this seed.
        setFinal(level, ServerLevel.class.getDeclaredField("randomSequences"), new net.minecraft.world.RandomSequences(seed));
    }

    // Sets a final instance field, which reflection cannot.
    private static void setFinal(Object target, java.lang.reflect.Field field, Object value) throws ReflectiveOperationException {
        var type = Class.forName("sun.misc.Unsafe");
        var theUnsafe = type.getDeclaredField("theUnsafe");
        theUnsafe.setAccessible(true);
        Object unsafe = theUnsafe.get(null);
        long offset = (long) type.getMethod("objectFieldOffset", java.lang.reflect.Field.class).invoke(unsafe, field);
        type.getMethod("putObject", Object.class, long.class, Object.class).invoke(unsafe, target, offset, value);
    }

    static final int SEEDED_ENTITY_IDS = 1 << 24;

    static long randomState(net.minecraft.util.RandomSource random) throws ReflectiveOperationException {
        var seed = net.minecraft.world.level.levelgen.LegacyRandomSource.class.getDeclaredField("seed");
        seed.setAccessible(true);
        return ((java.util.concurrent.atomic.AtomicLong) seed.get(random)).get();
    }

    // Once the build is placed: its imports, then the report's head.
    public void start(String engine, String minecraftVersion) throws Exception {
        importBlockEntities();
        importEntities();
        importScheduledTicks();
        result = new JsonObject();
        result.addProperty("engine", engine);
        result.addProperty("minecraftVersion", minecraftVersion);
        if(request.has("block_data_version")){
            result.add("sourceBlockDataVersion",request.get("block_data_version"));JsonArray changes=new JsonArray();
            for(var migration:migratedBlockPalette.entrySet())if(!migration.getKey().equals(migration.getValue())){
                JsonObject change=new JsonObject();change.addProperty("before",migration.getKey());change.addProperty("after",migration.getValue());changes.add(change);
            }
            result.add("blockStateMigrations",changes);
        }
        result.addProperty("placementMode", request.has("placementMode") ? request.get("placementMode").getAsString() : "updates");
        // A seeded run records where the level's random stands once the
        // world is placed, without drawing from it, so engines'
        // draws can be compared.
        if (request.has("seed")) result.addProperty("randomState", randomState(((CraftWorld) world).getHandle().random));
        var unsupported = io.papermc.paper.configuration.GlobalConfiguration.get().unsupportedSettings;
        JsonObject engineSettings = new JsonObject();
        engineSettings.addProperty("redstoneImplementation", ((CraftWorld) world).getHandle().paperConfig().misc.redstoneImplementation.name());
        engineSettings.addProperty("allowHeadlessPistons", unsupported.allowHeadlessPistons);
        engineSettings.addProperty("allowPermanentBlockBreakExploits", unsupported.allowPermanentBlockBreakExploits);
        engineSettings.addProperty("allowPistonDuplication", unsupported.allowPistonDuplication);
        var activation = ((CraftWorld) world).getHandle().spigotConfig;
        JsonObject activationRanges = new JsonObject();
        activationRanges.addProperty("animals",activation.animalActivationRange);
        activationRanges.addProperty("monsters",activation.monsterActivationRange);
        activationRanges.addProperty("raiders",activation.raiderActivationRange);
        activationRanges.addProperty("misc",activation.miscActivationRange);
        activationRanges.addProperty("water",activation.waterActivationRange);
        activationRanges.addProperty("villagers",activation.villagerActivationRange);
        activationRanges.addProperty("flying-monsters",activation.flyingMonsterActivationRange);
        if (activationRanges.entrySet().stream().anyMatch(e->e.getValue().getAsInt()!=0)) throw new IOException("Entity activation throttling must be disabled for Minecraft parity");
        engineSettings.add("entityActivationRanges",activationRanges);
        result.add("engineSettings", engineSettings);
        result.addProperty("physicsGuard", System.getProperty("mcbench.physicsGuard", "stock"));
        if (compactReport) result.addProperty("reportEncoding", "rle-v1");
        openReport();
    }

    public boolean started() { return result != null; }
    public boolean running() { return running; }
    // Sprint changes wall-clock pacing, not native tick order.
    public boolean sprint() { return !request.has("sprint") || request.get("sprint").getAsBoolean(); }

    // The first tick: settle ticks count from here.
    public void begin() {
        startTime = world.getGameTime();
        running = true;
        result.addProperty("sprinting", sprint());
    }

    // The end of each server tick (Paper's ServerTickEndEvent).
    public void endOfTick() {
        if (!running) return;
        try {
            long elapsed = world.getGameTime() - startTime;
            int settle = request.get("settle_ticks").getAsInt();
            if (elapsed < settle && elapsed % 20 == 0) progress("settling", elapsed, settle);
            if (elapsed < settle) return;
            int tick = Math.toIntExact(elapsed - settle);
            if (tick == lastTick) return;
            if (tick != lastTick + 1) throw new IllegalStateException("Missed game tick: " + tick);
            lastTick = tick;
            if (tick == 0) {
                report.name("initial"); observe(0, true);
                report.name("observations").beginArray();
            }
            // Requests are sorted once by the host. Advancing indices keeps the
            // tick loop independent of the total number of scheduled events.
            while (actionIndex < actions.size() && actions.get(actionIndex).getAsJsonObject().get("tick").getAsInt() == tick) {
                interact(actions.get(actionIndex++).getAsJsonObject());
            }
            if (sampleIndex < samples.length && samples[sampleIndex] == tick) {
                observe(tick, false);
                sampleIndex++;
            }
            if (tick % 20 == 0 || tick == request.get("duration").getAsInt()) {
                progress("running", tick, request.get("duration").getAsInt());
            }
            if (tick == request.get("duration").getAsInt()) finish(null);
        } catch (Exception error) {
            finish(error);
        }
    }

    BlockPos position(JsonArray p) {
        return new BlockPos(p.get(0).getAsInt(), p.get(1).getAsInt() + request.get("yOffset").getAsInt(), p.get(2).getAsInt());
    }

    private void openReport() throws IOException {
        reportLimit = request.getAsJsonObject("budgets").get("reportBytes").getAsLong();
        OutputStream file = Files.newOutputStream(folder.resolve("report.tmp"));
        OutputStream limited = new FilterOutputStream(file) {
            @Override public void write(byte[] bytes, int offset, int count) throws IOException {
                if (reportBytes + count > reportLimit) throw new ReportBudgetExceeded(reportBytes + count);
                out.write(bytes, offset, count);
                reportBytes += count;
            }
            @Override public void write(int value) throws IOException { write(new byte[]{(byte)value}, 0, 1); }
        };
        report = new JsonWriter(new BufferedWriter(new OutputStreamWriter(limited, StandardCharsets.UTF_8), 16384));
        report.beginObject();
        for (var entry : result.entrySet()) { report.name(entry.getKey()); gson.toJson(entry.getValue(), report); }
        report.name("worldRevision").value(request.get("worldRevision").getAsLong());
        report.name("probes"); gson.toJson(request.get("probes"), report);
        if (request.has("scheduled_ticks")) { report.name("importedScheduledTicks"); gson.toJson(request.get("scheduled_ticks"), report); }
        report.name("actions"); gson.toJson(request.get("actions"), report);
        if (importedEntities != null && !importedEntities.isEmpty()) { report.name("importedEntities"); gson.toJson(importedEntities, report); }
        if (importedBlockEntities != null && !importedBlockEntities.isEmpty()) { report.name("importedBlockEntities"); gson.toJson(importedBlockEntities, report); }
        if (compactReport && request.has("sampleSequence")) {
            report.name("sampleSequence"); gson.toJson(request.get("sampleSequence"), report);
        } else {
            report.name("sampleTicks").beginArray();
            for (int sample : samples) report.value(sample);
            report.endArray();
        }
        report.name("settleTicks").value(request.get("settle_ticks").getAsInt());
        report.name("cost"); gson.toJson(request.get("cost"), report);
        report.name("budgets"); gson.toJson(request.get("budgets"), report);
    }

    private void writeObservation(int tick, int endTick, String[] states, int[] signals, JsonArray[] inventories, JsonArray itemEntities, JsonArray worldInventories, JsonArray livingEntities, boolean range) throws IOException {
        report.beginObject().name("tick").value(tick);
        if (range) report.name("endTick").value(endTick);
        report.name("blocks").beginArray();
        for (int i = 0; i < states.length; i++) {
            report.beginObject().name("block").value(states[i]).name("signal").value(signals[i]);
            if (inventoryProbes[i]) {
                report.name("inventory");
                if (inventories[i] == null) report.jsonValue("null"); else gson.toJson(inventories[i], report);
            }
            report.endObject();
        }
        report.endArray();
        if (itemEntities != null) { report.name("item_entities"); gson.toJson(itemEntities, report); }
        if (worldInventories != null) { report.name("world_inventories"); gson.toJson(worldInventories, report); }
        if (livingEntities != null) { report.name("living_entities"); gson.toJson(livingEntities, report); }
        report.endObject();
    }

    private void flushPending() throws IOException {
        if (pendingStart < 0) return;
        writeObservation(pendingStart, pendingEnd, pendingBlocks, pendingSignals, pendingInventories, pendingItemEntities, pendingWorldInventories, pendingLivingEntities, true);
        pendingStart = -1;
    }

    private void observe(int tick, boolean initialObservation) throws IOException {
        long start = System.nanoTime();
        String[] states = new String[probes.length];
        int[] signals = new int[probes.length];
        JsonArray[] inventories = new JsonArray[probes.length];
        for (int i = 0; i < probes.length; i++) {
            BlockPos pos = probes[i];
            var block = world.getBlockAt(pos.getX(), pos.getY(), pos.getZ());
            String state = block.getBlockData().getAsString();
            int signal = block.getBlockPower();
            states[i] = state; signals[i] = signal;
            if (inventoryProbes[i]) inventories[i] = readInventory(i, pos);
            boolean initial = previousBlocks[i] == null;
            boolean changed = !initial && (!state.equals(previousBlocks[i]) || signal != previousSignals[i] || !java.util.Objects.equals(inventories[i], previousInventories[i]));
            if (changed) { changedProbes[i] = true; changeCount++; }
            if ((initial && i < 32) || changed) {
                JsonObject entry = new JsonObject();
                entry.addProperty("probe", i);
                if (initial) entry.add("at", request.getAsJsonArray("probes").get(i));
                else entry.addProperty("tick", tick);
                entry.addProperty("block", state); entry.addProperty("signal", signal);
                if (inventories[i] != null) entry.add("inventory", inventories[i]);
                int bytes = gson.toJson(entry).getBytes(StandardCharsets.UTF_8).length;
                // One bounded preview, in observation order. Full report is lossless.
                if (previewBytes + bytes <= 8000) {
                    (initial ? previewInitial : previewChanges).add(entry);
                    previewBytes += bytes;
                }
            }
            previousBlocks[i] = state; previousSignals[i] = signal; previousInventories[i] = inventories[i];
            observationCount++;
        }
        JsonArray itemEntities = observeItemEntities ? readItemEntities() : null;
        JsonArray worldInventories = observeWorldInventories ? readWorldInventories() : null;
        JsonArray livingEntities = observeLivingEntities ? readLivingEntities() : null;
        if (!compactReport || initialObservation) writeObservation(tick, tick, states, signals, inventories, itemEntities, worldInventories, livingEntities, false);
        else if (pendingStart >= 0 && tick == pendingEnd + 1 && Arrays.equals(states, pendingBlocks) && Arrays.equals(signals, pendingSignals) && Arrays.equals(inventories, pendingInventories) && java.util.Objects.equals(itemEntities, pendingItemEntities) && java.util.Objects.equals(worldInventories, pendingWorldInventories) && java.util.Objects.equals(livingEntities, pendingLivingEntities)) pendingEnd = tick;
        else {
            flushPending();
            pendingStart = pendingEnd = tick; pendingBlocks = states; pendingSignals = signals; pendingInventories = inventories; pendingItemEntities = itemEntities; pendingWorldInventories = worldInventories; pendingLivingEntities = livingEntities;
        }
        long duration = System.nanoTime() - start;
        probeNanos += duration;
        maxProbeNanos = Math.max(maxProbeNanos, duration);
        Runtime runtime = Runtime.getRuntime();
        maxHeapBytes = Math.max(maxHeapBytes, runtime.totalMemory() - runtime.freeMemory());
    }

    String upgradeBlockToken(String token) throws IOException {
        if(migratedBlockPalette.containsKey(token))return migratedBlockPalette.get(token);
        int bracket=token.indexOf('[');String name=bracket<0?token:token.substring(0,bracket);
        var tag=new net.minecraft.nbt.CompoundTag();tag.putString("Name",net.minecraft.resources.ResourceLocation.parse(name).toString());
        if(bracket>=0){
            if(!token.endsWith("]"))throw new IOException("Invalid source block properties");
            var properties=new net.minecraft.nbt.CompoundTag();
            for(String pair:token.substring(bracket+1,token.length()-1).split(",")){
                String[] kv=pair.split("=",-1);if(kv.length!=2||properties.contains(kv[0]))throw new IOException("Invalid source block property");properties.putString(kv[0],kv[1]);
            }
            tag.put("Properties",properties);
        }
        int version=request.get("block_data_version").getAsInt(),current=net.minecraft.SharedConstants.getCurrentVersion().dataVersion().version();
        tag=(net.minecraft.nbt.CompoundTag)net.minecraft.util.datafix.DataFixers.getDataFixer().update(
            net.minecraft.util.datafix.fixes.References.BLOCK_STATE,new com.mojang.serialization.Dynamic<>(net.minecraft.nbt.NbtOps.INSTANCE,tag),version,current).getValue();
        String result=tag.getString("Name").orElseThrow(()->new IOException("Native block data fixer omitted Name"));
        var properties=tag.getCompound("Properties").orElse(null);
        if(properties!=null&&!properties.isEmpty()){
            var keys=new ArrayList<>(properties.keySet());keys.sort(String::compareTo);var pairs=new ArrayList<String>();
            for(String key:keys)pairs.add(key+"="+properties.getString(key).orElseThrow(()->new IOException("Invalid upgraded property")));
            result+="["+String.join(",",pairs)+"]";
        }
        migratedBlockPalette.put(token,result);return result;
    }

    private void importBlockEntities() throws Exception {
        importedBlockEntities = new JsonArray();
        if (!request.has("block_entities")) return;
        var level = ((CraftWorld) world).getHandle();
        int current = net.minecraft.SharedConstants.getCurrentVersion().dataVersion().version();
        for (JsonElement value : request.getAsJsonArray("block_entities")) {
            JsonObject entry = value.getAsJsonObject(); BlockPos at = position(entry.getAsJsonArray("at"));
            var entity = level.getBlockEntity(at);
            if (entity == null) throw new IOException("No block entity at " + entry.get("at"));
            var tag = net.minecraft.nbt.TagParser.parseCompoundFully(entry.get("snbt").getAsString());
            String id = net.minecraft.world.level.block.entity.BlockEntityType.getKey(entity.getType()).toString();
            if (!tag.contains("id")) tag.putString("id", id);
            int version = entry.get("data_version").getAsInt();
            if (version > current) throw new IOException("Cannot import future block-entity data version " + version);
            tag = (net.minecraft.nbt.CompoundTag) net.minecraft.util.datafix.DataFixers.getDataFixer().update(
                net.minecraft.util.datafix.fixes.References.BLOCK_ENTITY,
                new com.mojang.serialization.Dynamic<>(net.minecraft.nbt.NbtOps.INSTANCE, tag), version, current).getValue();
            if (!tag.getString("id").orElse("").equals(id)) throw new IOException("Block entity ID mismatch at " + entry.get("at"));
            tag.putInt("x", at.getX()); tag.putInt("y", at.getY()); tag.putInt("z", at.getZ());
            var problems = new net.minecraft.util.ProblemReporter.Collector();
            entity.loadWithComponents(net.minecraft.world.level.storage.TagValueInput.create(problems, level.registryAccess(), tag));
            if (!problems.isEmpty()) throw new IOException("Invalid block entity at " + entry.get("at") + ": " + problems.getReport());
            entity.setChanged();
            JsonObject audit = new JsonObject(); audit.add("at", entry.get("at"));
            audit.addProperty("sourceDataVersion", version); audit.addProperty("dataVersion", current);
            audit.addProperty("snbt", entity.saveWithFullMetadata(level.registryAccess()).toString());
            importedBlockEntities.add(audit);
        }
    }

    private void importScheduledTicks() throws Exception {
        if (!request.has("scheduled_ticks")) return;
        if (!request.has("placementMode") || !request.get("placementMode").getAsString().equals("strict")) throw new IOException("Saved tick restoration requires frozen strict placement");
        var saved = request.getAsJsonObject("scheduled_ticks");
        int version = saved.get("data_version").getAsInt();
        int current = net.minecraft.SharedConstants.getCurrentVersion().dataVersion().version();
        if (version > current) throw new IOException("Future scheduled ticks cannot be downgraded");
        var level = ((CraftWorld) world).getHandle();
        var bounds = saved.getAsJsonArray("bounds");
        var lo = bounds.get(0).getAsJsonArray(); var hi = bounds.get(1).getAsJsonArray();
        int dy = request.get("yOffset").getAsInt();
        var box = new net.minecraft.world.level.levelgen.structure.BoundingBox(lo.get(0).getAsInt(),lo.get(1).getAsInt()+dy,lo.get(2).getAsInt(),hi.get(0).getAsInt(),hi.get(1).getAsInt()+dy,hi.get(2).getAsInt());
        // Strict block placement can schedule on-place updates. The saved queue
        // is authoritative inside this imported region, including an empty queue.
        // Validate the complete source queue before changing either native queue.
        var blockTicks = new ArrayList<net.minecraft.world.ticks.ScheduledTick<net.minecraft.world.level.block.Block>>();
        var fluidTicks = new ArrayList<net.minecraft.world.ticks.ScheduledTick<net.minecraft.world.level.material.Fluid>>();
        long now = level.getGameTime(); long largestOrder = -1;
        for (var value : saved.getAsJsonArray("entries")) {
            var e = value.getAsJsonObject(); var at = e.getAsJsonArray("at");
            var pos = new BlockPos(at.get(0).getAsInt(),at.get(1).getAsInt()+dy,at.get(2).getAsInt());
            if (!box.isInside(pos)) throw new IOException("Scheduled tick outside saved bounds");
            var id = net.minecraft.resources.ResourceLocation.parse(e.get("id").getAsString());
            long due = Math.addExact(now,e.get("delay").getAsLong());
            var priority = net.minecraft.world.ticks.TickPriority.byValue(e.get("priority").getAsInt());
            long order = Long.parseLong(e.get("sub_tick").getAsString());
            if (order == Long.MAX_VALUE) throw new IOException("Saved tick order leaves no room for subsequent native updates");
            largestOrder = Math.max(largestOrder,order);
            if (e.get("kind").getAsString().equals("block")) {
                var block = net.minecraft.core.registries.BuiltInRegistries.BLOCK.getOptional(id).orElseThrow(()->new IOException("Unknown saved block tick: " + id));
                blockTicks.add(new net.minecraft.world.ticks.ScheduledTick<>(block,pos,due,priority,order));
            } else if (e.get("kind").getAsString().equals("fluid")) {
                var fluid = net.minecraft.core.registries.BuiltInRegistries.FLUID.getOptional(id).orElseThrow(()->new IOException("Unknown saved fluid tick: " + id));
                fluidTicks.add(new net.minecraft.world.ticks.ScheduledTick<>(fluid,pos,due,priority,order));
            } else throw new IOException("Unknown saved tick kind");
        }
        // New native updates must follow all restored entries at equal time and
        // priority. Restore the world's monotonic sequence cursor as well.
        var counter = net.minecraft.world.level.Level.class.getDeclaredField("subTickCount");
        counter.setAccessible(true);
        counter.setLong(level,Math.max(counter.getLong(level),Math.addExact(largestOrder,1)));
        level.getBlockTicks().clearArea(box); level.getFluidTicks().clearArea(box);
        for (var tick : blockTicks) level.getBlockTicks().schedule(tick);
        for (var tick : fluidTicks) level.getFluidTicks().schedule(tick);
        for (var expected : blockTicks) {
            var actual = ((net.minecraft.world.ticks.LevelChunkTicks<net.minecraft.world.level.block.Block>) level.getChunkAt(expected.pos()).getBlockTicks()).getAll().filter(t->t.pos().equals(expected.pos()) && t.type()==expected.type()).findFirst().orElseThrow(()->new IOException("Missing restored block tick"));
            if (!actual.equals(expected)) throw new IOException("Native queue changed saved block tick timing or order");
        }
        for (var expected : fluidTicks) {
            var actual = ((net.minecraft.world.ticks.LevelChunkTicks<net.minecraft.world.level.material.Fluid>) level.getChunkAt(expected.pos()).getFluidTicks()).getAll().filter(t->t.pos().equals(expected.pos()) && t.type()==expected.type()).findFirst().orElseThrow(()->new IOException("Missing restored fluid tick"));
            if (!actual.equals(expected)) throw new IOException("Native queue changed saved fluid tick timing or order");
        }
    }

    // BEGIN_VERSIONED_ENTITY_IMPORT
    private void importEntities() throws Exception {
        importedEntities = new JsonArray();
        if (!request.has("entities")) return;
        var level = ((CraftWorld) world).getHandle();
        int current = net.minecraft.SharedConstants.getCurrentVersion().dataVersion().version();
        for (JsonElement value : request.getAsJsonArray("entities")) {
            JsonObject entry = value.getAsJsonObject();
            var tag = net.minecraft.nbt.TagParser.parseCompoundFully(entry.get("snbt").getAsString());
            int version = entry.get("data_version").getAsInt();
            if (version > current) throw new IOException("Cannot import future entity data version " + version);
            tag = (net.minecraft.nbt.CompoundTag) net.minecraft.util.datafix.DataFixers.getDataFixer().update(
                tag.contains("Passengers") ? net.minecraft.util.datafix.fixes.References.ENTITY_TREE : net.minecraft.util.datafix.fixes.References.ENTITY,
                new com.mojang.serialization.Dynamic<>(net.minecraft.nbt.NbtOps.INSTANCE, tag), version, current).getValue();
            // Boats and dropped items have no separate world-space anchors. Other entity families need
            // their own proven translation rules before this importer accepts them.
            String id = tag.getString("id").orElse("");
            boolean frame = id.equals("minecraft:item_frame") || id.equals("minecraft:glow_item_frame") || id.equals("minecraft:painting");
            if (!frame && !id.matches("minecraft:(chest_|hopper_|furnace_|tnt_)?minecart") && !id.equals("minecraft:item") && !id.equals("minecraft:experience_orb") && !id.equals("minecraft:armor_stand") && !id.matches("minecraft:[a-z_]+_(boat|raft)")) throw new IOException("Native entity import currently supports boats, rafts, dropped items, experience orbs, item frames, paintings, armor stands and ordinary container/furnace/TNT minecarts only: " + id);
            if (tag.contains("leash") || tag.contains("Leash")) throw new IOException("Entity leash relationships require a faithful relationship importer");
            net.minecraft.nbt.CompoundTag passengerTag = null;
            java.util.UUID passengerUuid = null;
            if (tag.contains("Passengers")) {
                var passengers = tag.getList("Passengers").orElseThrow(() -> new IOException("Invalid passenger list"));
                if (!id.equals("minecraft:minecart") || passengers.size()!=1) throw new IOException("Unsupported passengers: only a single original dolphin riding an ordinary minecart is supported");
                passengerTag = passengers.getCompound(0).orElseThrow(() -> new IOException("Invalid dolphin passenger"));
                if (!passengerTag.getString("id").orElse("").equals("minecraft:dolphin") || passengerTag.contains("Passengers") || passengerTag.contains("leash") || passengerTag.contains("Leash")) throw new IOException("Unsupported passenger family or relationship");
                int[] childUuid = passengerTag.getIntArray("UUID").orElseThrow(() -> new IOException("Passenger requires original UUID"));
                if(childUuid.length!=4)throw new IOException("Invalid passenger UUID");
                passengerUuid = new java.util.UUID(((long)childUuid[0]<<32)|(childUuid[1]&0xffffffffL),((long)childUuid[2]<<32)|(childUuid[3]&0xffffffffL));
                var childPos=passengerTag.getList("Pos").orElseThrow(() -> new IOException("Passenger requires original Pos"));
                if(childPos.size()!=3)throw new IOException("Invalid passenger Pos");
                var childTranslated=new net.minecraft.nbt.ListTag();
                for(int axis=0;axis<3;axis++){
                    double v=childPos.getDouble(axis).orElseThrow(() -> new IOException("Invalid passenger coordinate"));
                    if(!Double.isFinite(v)||Math.abs(v-entry.getAsJsonArray("at").get(axis).getAsDouble())>4)throw new IOException("Passenger is outside the mounted-entity import margin");
                    childTranslated.add(net.minecraft.nbt.DoubleTag.valueOf(v+(axis==1?request.get("yOffset").getAsInt():0)));
                }
                passengerTag.put("Pos",childTranslated);
            }
            var pos = tag.getList("Pos").orElseThrow(() -> new IOException("Entity needs Pos"));
            if (pos.size() != 3 || !tag.contains("UUID")) throw new IOException("Entity needs three Pos coordinates and its source UUID");
            int[] uuid = tag.getIntArray("UUID").orElseThrow(() -> new IOException("Entity needs a native UUID int array"));
            if (uuid.length != 4) throw new IOException("Entity UUID must contain four ints");
            var sourceUuid = new java.util.UUID(((long) uuid[0] << 32) | (uuid[1] & 0xffffffffL), ((long) uuid[2] << 32) | (uuid[3] & 0xffffffffL));
            var translated = new net.minecraft.nbt.ListTag();
            for (int axis = 0; axis < 3; axis++) {
                double local = pos.getDouble(axis).orElseThrow(() -> new IOException("Invalid entity Pos"));
                if (!Double.isFinite(local) || Double.compare(local, entry.getAsJsonArray("at").get(axis).getAsDouble()) != 0) throw new IOException("Entity Pos does not match declared at");
                translated.add(net.minecraft.nbt.DoubleTag.valueOf(local + (axis == 1 ? request.get("yOffset").getAsInt() : 0)));
            }
            BlockPos frameAnchor = null;
            if (frame) {
                int[] anchor = tag.getIntArray("block_pos").orElseThrow(()->new IOException("Item frame needs native attachment block_pos"));
                if (anchor.length != 3) throw new IOException("Invalid frame attachment coordinates");
                anchor[1] = Math.addExact(anchor[1],request.get("yOffset").getAsInt());
                tag.putIntArray("block_pos",anchor);
                frameAnchor = new BlockPos(anchor[0],anchor[1],anchor[2]);
            }
            tag.put("Pos", translated);
            var problems = new net.minecraft.util.ProblemReporter.Collector();
            var entity = net.minecraft.world.entity.EntityType.loadEntityRecursive(
                net.minecraft.world.level.storage.TagValueInput.create(problems, level.registryAccess(), tag),
                level, net.minecraft.world.entity.EntitySpawnReason.LOAD, e -> e);
            if (entity == null || !problems.isEmpty()) throw new IOException("Invalid native entity " + id + ": " + problems.getReport());
            if (frame && !((net.minecraft.world.entity.decoration.BlockAttachedEntity)entity).getPos().equals(frameAnchor)) throw new IOException("Native frame loader rejected the source attachment position");
            if (frame && (Double.compare(entity.getX(),translated.getDouble(0).orElseThrow())!=0 || Double.compare(entity.getY(),translated.getDouble(1).orElseThrow())!=0 || Double.compare(entity.getZ(),translated.getDouble(2).orElseThrow())!=0)) throw new IOException("Frame attachment/facing does not match source Pos");
            if (!entity.getUUID().equals(sourceUuid)) throw new IOException("Native import changed source entity UUID");
            if(passengerTag!=null){
                if(entity.getPassengers().size()!=1)throw new IOException("Native import lost passenger relationship");
                var child=entity.getPassengers().getFirst();
                if(!(child instanceof net.minecraft.world.entity.animal.Dolphin)||!child.getUUID().equals(passengerUuid)||child.getVehicle()!=entity)throw new IOException("Native import changed dolphin passenger identity");
                var expected=passengerTag.getList("Pos").orElseThrow();
                if(Double.compare(child.getX(),expected.getDouble(0).orElseThrow())!=0||Double.compare(child.getY(),expected.getDouble(1).orElseThrow())!=0||Double.compare(child.getZ(),expected.getDouble(2).orElseThrow())!=0)throw new IOException("Native import changed saved passenger position");
            }
            if (!level.tryAddFreshEntityWithPassengers(entity)) throw new IOException("Failed native entity insertion (possibly duplicate UUID): " + entity.getUUID());
            var output = net.minecraft.world.level.storage.TagValueOutput.createWithContext(problems, level.registryAccess());
            if (!entity.saveAsPassenger(output) || !problems.isEmpty()) throw new IOException("Could not audit imported entity " + id);
            JsonObject audit = new JsonObject(); audit.add("at", entry.get("at"));
            audit.addProperty("sourceDataVersion", version); audit.addProperty("dataVersion", current);
            audit.addProperty("snbt", output.buildResult().toString()); importedEntities.add(audit);
        }
    }

    // END_VERSIONED_ENTITY_IMPORT

    // Read-only census: no AI, health, movement or tick-state changes.
    private JsonArray readLivingEntities() {
        JsonArray values = new JsonArray();
        var entities = new ArrayList<>(world.getLivingEntities());
        entities.sort(Comparator.comparing(e -> e.getUniqueId().toString()));
        for (var entity : entities) {
            if (entity instanceof org.bukkit.entity.Player) continue;
            JsonObject value = new JsonObject();
            value.addProperty("uuid", entity.getUniqueId().toString());
            value.addProperty("type", entity.getType().getKey().toString());
            JsonArray at = new JsonArray(); var location = entity.getLocation();
            at.add(location.getX()); at.add(location.getY() - request.get("yOffset").getAsInt()); at.add(location.getZ());
            value.add("at", at);
            value.addProperty("health", entity.getHealth());
            var maximum = entity.getAttribute(org.bukkit.attribute.Attribute.MAX_HEALTH);
            if (maximum == null) throw new IllegalStateException("Living entity has no maximum health: " + entity.getType());
            value.addProperty("max_health", maximum.getValue());
            value.addProperty("dead", entity.isDead());
            values.add(value);
        }
        return values;
    }

    private JsonArray readItemEntities() {
        var level = ((CraftWorld) world).getHandle();
        JsonArray items = new JsonArray();
        var entities = new ArrayList<>(world.getEntitiesByClass(org.bukkit.entity.Item.class));
        entities.sort(Comparator.comparing(e -> e.getUniqueId().toString()));
        for (var entity : entities) {
            var item = org.bukkit.craftbukkit.inventory.CraftItemStack.asNMSCopy(entity.getItemStack());
            JsonObject value = new JsonObject();
            value.addProperty("uuid", entity.getUniqueId().toString());
            JsonArray at = new JsonArray(); var location = entity.getLocation();
            at.add(location.getX()); at.add(location.getY() - request.get("yOffset").getAsInt()); at.add(location.getZ());
            value.add("at", at);
            value.addProperty("id", net.minecraft.core.registries.BuiltInRegistries.ITEM.getKey(item.getItem()).toString());
            value.addProperty("count", item.getCount());
            value.addProperty("plain", item.getComponentsPatch().isEmpty());
            var encoded = net.minecraft.world.item.ItemStack.CODEC.encodeStart(level.registryAccess().createSerializationContext(net.minecraft.nbt.NbtOps.INSTANCE), item).getOrThrow();
            value.addProperty("snbt", encoded.toString()); addContainedItems(value,item,0); items.add(value);
        }
        return items;
    }

    private JsonArray readWorldInventories() throws IOException {
        var level = ((CraftWorld) world).getHandle();
        JsonArray values = new JsonArray();
        var chunks = new ArrayList<>(Arrays.asList(world.getLoadedChunks()));
        chunks.sort(Comparator.comparingInt(org.bukkit.Chunk::getX).thenComparingInt(org.bukkit.Chunk::getZ));
        for (var chunk : chunks) {
            var blockEntities = new ArrayList<>(level.getChunk(chunk.getX(),chunk.getZ()).getBlockEntities().values());
            blockEntities.sort(Comparator.comparingLong(e->e.getBlockPos().asLong()));
            for (var blockEntity : blockEntities) {
                JsonArray items=new JsonArray();
                if(blockEntity instanceof net.minecraft.world.Container container) {
                    if(container instanceof net.minecraft.world.RandomizableContainer loot && loot.getLootTable()!=null)throw new IOException("Cannot observe unopened container loot without changing world state");
                    for(int slot=0;slot<container.getContainerSize();slot++){var item=container.getItem(slot);if(!item.isEmpty())items.add(encodeInventoryItem(item,slot));}
                } else if(blockEntity instanceof net.minecraft.world.level.block.entity.LecternBlockEntity lectern) {
                    if(!lectern.getBook().isEmpty())items.add(encodeInventoryItem(lectern.getBook(),0));
                } else if(blockEntity instanceof net.minecraft.world.level.block.entity.CampfireBlockEntity campfire) {
                    var stacks=campfire.getItems();for(int slot=0;slot<stacks.size();slot++)if(!stacks.get(slot).isEmpty())items.add(encodeInventoryItem(stacks.get(slot),slot));
                } else continue;
                var pos=blockEntity.getBlockPos();JsonObject value=new JsonObject();value.addProperty("kind","block");value.addProperty("type",net.minecraft.core.registries.BuiltInRegistries.BLOCK.getKey(blockEntity.getBlockState().getBlock()).toString());
                JsonArray at=new JsonArray();at.add(pos.getX());at.add(pos.getY()-request.get("yOffset").getAsInt());at.add(pos.getZ());value.add("at",at);value.add("inventory",items);values.add(value);
            }
        }
        var entities = new ArrayList<>(world.getEntities());
        entities.sort(Comparator.comparing(e->e.getUniqueId().toString()));
        for (var bukkit : entities) {
            var entity = ((org.bukkit.craftbukkit.entity.CraftEntity)bukkit).getHandle();
            JsonArray items = new JsonArray();
            if (entity instanceof net.minecraft.world.entity.vehicle.ContainerEntity loot && loot.getContainerLootTable()!=null) throw new IOException("Cannot observe unopened entity loot without changing world state");
            if (entity instanceof net.minecraft.world.Container container) {
                for (int slot=0;slot<container.getContainerSize();slot++) {
                    var item=container.getItem(slot);if(!item.isEmpty())items.add(encodeInventoryItem(item,slot));
                }
            } else if (entity instanceof net.minecraft.world.entity.decoration.ItemFrame frame) {
                if(!frame.getItem().isEmpty())items.add(encodeInventoryItem(frame.getItem(),0));
            } else if (entity instanceof net.minecraft.world.entity.animal.Dolphin dolphin) {
                for(var slot:net.minecraft.world.entity.EquipmentSlot.values()){
                    var item=dolphin.getItemBySlot(slot);if(!item.isEmpty())items.add(encodeInventoryItem(item,slot.ordinal()));
                }
            } else if (!(entity instanceof net.minecraft.world.entity.vehicle.AbstractBoat) && !(entity instanceof net.minecraft.world.entity.vehicle.AbstractMinecart)) continue;
            JsonObject value = new JsonObject(); value.addProperty("kind","entity"); value.addProperty("uuid",entity.getUUID().toString());
            value.addProperty("type",net.minecraft.core.registries.BuiltInRegistries.ENTITY_TYPE.getKey(entity.getType()).toString());
            if (entity instanceof net.minecraft.world.entity.vehicle.AbstractBoat boat) value.add("vehicle_item",encodeInventoryItem(boat.getPickResult(),0));
            else if (entity instanceof net.minecraft.world.entity.vehicle.AbstractMinecart cart) value.add("vehicle_item",encodeInventoryItem(cart.getPickResult(),0));
            JsonArray at=new JsonArray();at.add(entity.getX());at.add(entity.getY()-request.get("yOffset").getAsInt());at.add(entity.getZ());value.add("at",at);value.add("inventory",items);values.add(value);
        }
        return values;
    }

    private JsonObject encodeInventoryItem(net.minecraft.world.item.ItemStack item,int slot) { return encodeInventoryItem(item,slot,0); }
    private JsonObject encodeInventoryItem(net.minecraft.world.item.ItemStack item,int slot,int depth) {
        if(depth>64)throw new IllegalStateException("Nested inventory depth exceeds supported observation depth");
        var level=((CraftWorld)world).getHandle();JsonObject value=new JsonObject();value.addProperty("slot",slot);
        value.addProperty("id",net.minecraft.core.registries.BuiltInRegistries.ITEM.getKey(item.getItem()).toString());value.addProperty("count",item.getCount());value.addProperty("plain",item.getComponentsPatch().isEmpty());
        value.addProperty("snbt",net.minecraft.world.item.ItemStack.CODEC.encodeStart(level.registryAccess().createSerializationContext(net.minecraft.nbt.NbtOps.INSTANCE),item).getOrThrow().toString());addContainedItems(value,item,depth);return value;
    }

    private void addContainedItems(JsonObject value,net.minecraft.world.item.ItemStack item,int depth) {
        JsonArray children=new JsonArray();
        var container=item.get(net.minecraft.core.component.DataComponents.CONTAINER);
        if(container!=null)for(var child:container.nonEmptyItems())children.add(encodeInventoryItem(child,children.size(),depth+1));
        var bundle=item.get(net.minecraft.core.component.DataComponents.BUNDLE_CONTENTS);
        if(bundle!=null)for(var child:bundle.items())if(!child.isEmpty())children.add(encodeInventoryItem(child,children.size(),depth+1));
        var projectiles=item.get(net.minecraft.core.component.DataComponents.CHARGED_PROJECTILES);
        if(projectiles!=null)for(var child:projectiles.getItems())if(!child.isEmpty())children.add(encodeInventoryItem(child,children.size(),depth+1));
        if(!children.isEmpty())value.add("containedItems",children);
    }

    private JsonArray readInventory(int index, BlockPos at) throws IOException {
        var level = ((CraftWorld) world).getHandle();
        if (!(level.getBlockEntity(at) instanceof net.minecraft.world.Container container)) {
            if (!reportMissingInventories) throw new IOException("Inventory probe is not a container: " + at);
            inventorySnapshots[index] = null; inventoryCache[index] = null;
            return null;
        }
        if (container instanceof net.minecraft.world.RandomizableContainer loot && loot.getLootTable()!=null) throw new IOException("Cannot observe unopened container loot without changing world state");
        var snapshot = inventorySnapshots[index];
        boolean same = snapshot != null && snapshot.length == container.getContainerSize();
        if (same) for (int slot = 0; slot < snapshot.length; slot++) {
            if (!net.minecraft.world.item.ItemStack.matches(snapshot[slot], container.getItem(slot))) { same = false; break; }
        }
        if (same) return inventoryCache[index];
        snapshot = new net.minecraft.world.item.ItemStack[container.getContainerSize()];
        JsonArray items = new JsonArray();
        for (int slot = 0; slot < container.getContainerSize(); slot++) {
            var item = container.getItem(slot); snapshot[slot] = item.copy(); if (item.isEmpty()) continue;
            JsonObject value = new JsonObject(); value.addProperty("slot", slot);
            value.addProperty("id", net.minecraft.core.registries.BuiltInRegistries.ITEM.getKey(item.getItem()).toString());
            value.addProperty("count", item.getCount());
            value.addProperty("plain", item.getComponentsPatch().isEmpty());
            var encoded = net.minecraft.world.item.ItemStack.CODEC.encodeStart(level.registryAccess().createSerializationContext(net.minecraft.nbt.NbtOps.INSTANCE), item).getOrThrow();
            value.addProperty("snbt", encoded.toString()); addContainedItems(value,item,0); items.add(value);
        }
        inventorySnapshots[index] = snapshot; inventoryCache[index] = items;
        return items;
    }

    private net.minecraft.world.item.ItemStack suppliedStack(ServerLevel level, JsonObject input) throws IOException {
        var key = net.minecraft.resources.ResourceLocation.parse(input.get("item").getAsString());
        var registry = net.minecraft.core.registries.BuiltInRegistries.ITEM;
        if (!registry.containsKey(key) || key.toString().equals("minecraft:air")) throw new IOException("Unknown or empty item");
        int count = input.get("count").getAsInt();
        var item = new net.minecraft.world.item.ItemStack(registry.getValue(key), count);
        if (input.has("stack")) {
            try {
                var saved = input.getAsJsonObject("stack");
                int version = saved.get("data_version").getAsInt();
                int current = net.minecraft.SharedConstants.getCurrentVersion().dataVersion().version();
                if (version > current) throw new IOException("Cannot supply future item data version " + version);
                var tag = net.minecraft.nbt.TagParser.parseCompoundFully(saved.get("snbt").getAsString());
                var fixed = net.minecraft.util.datafix.DataFixers.getDataFixer().update(
                    net.minecraft.util.datafix.fixes.References.ITEM_STACK,
                    new com.mojang.serialization.Dynamic<>(net.minecraft.nbt.NbtOps.INSTANCE, tag), version, current).getValue();
                item = net.minecraft.world.item.ItemStack.CODEC.parse(
                    level.registryAccess().createSerializationContext(net.minecraft.nbt.NbtOps.INSTANCE), fixed).getOrThrow();
                if (item.isEmpty() || !item.is(registry.getValue(key)) || item.getCount() != count) throw new IOException("Supplied stack ID/count does not match input");
            } catch (Exception error) { throw new IOException("Invalid supplied stack: " + error.getMessage(), error); }
        }
        if (count < 1 || count > item.getMaxStackSize()) throw new IOException("Supplied item count exceeds native stack size");
        return item;
    }

    private void inventoryInput(ServerLevel level, BlockPos at, JsonObject input) throws IOException {
        if (!(level.getBlockEntity(at) instanceof net.minecraft.world.Container container)) throw new IOException("Input is not a container");
        int slot = input.get("slot").getAsInt(), count = input.get("count").getAsInt();
        if (slot < 0 || slot >= container.getContainerSize()) throw new IOException("Invalid inventory slot");
        var old = container.getItem(slot);
        if (input.get("type").getAsString().equals("extract")) {
            if (old.getCount() < count) throw new IOException("Insufficient items; nothing extracted");
            container.removeItem(slot, count);
        } else {
            var item = suppliedStack(level, input);
            if (!old.isEmpty() && !net.minecraft.world.item.ItemStack.isSameItemSameComponents(old, item)) throw new IOException("Slot contains a different item; nothing inserted");
            if (old.getCount() + count > Math.min(item.getMaxStackSize(), container.getMaxStackSize(item)) || !container.canPlaceItem(slot, item)) throw new IOException("Items do not fit; nothing inserted");
            item.setCount(old.getCount() + count); container.setItem(slot, item);
        }
        container.setChanged();
    }

    // Empties one container block through its own removeItem, slot by slot, and
    // returns every removed stack encoded like an inventory observation.
    private JsonArray drainInventory(ServerLevel level, BlockPos at) throws IOException {
        if (!(level.getBlockEntity(at) instanceof net.minecraft.world.Container container)) throw new IOException("Input is not a container");
        if (container instanceof net.minecraft.world.RandomizableContainer loot && loot.getLootTable()!=null) throw new IOException("Cannot drain unopened container loot");
        JsonArray removed = new JsonArray();
        for (int slot = 0; slot < container.getContainerSize(); slot++) {
            var item = container.getItem(slot);
            if (item.isEmpty()) continue;
            removed.add(encodeInventoryItem(item, slot));
            container.removeItem(slot, item.getCount());
        }
        container.setChanged();
        return removed;
    }

    // Inputs that fail validation (IOException) are reported as failed
    // inputs; any other exception is a fault and ends the experiment.
    private net.minecraft.server.level.ServerPlayer inputPlayer(ServerLevel level) {
        if (interactionPlayer == null) interactionPlayer = newInputPlayer(level);
        return interactionPlayer;
    }

    // The blocks a "use" input takes (the verifier's and the live world's,
    // McbenchEngine.input): those whose useWithoutItem, a player's right click
    // with an empty hand, changes only the world's blocks and needs no more of
    // the player than its yaw, that it may build and the entity it is. Doors
    // and trapdoors (iron ones pass: no type that cannot be opened by hand
    // opens), fence gates (they open away from the side the yaw puts the
    // player on), repeaters, comparators, note blocks, daylight detectors,
    // redstone dust (a dot becomes a cross and a cross a dot, unless the dust
    // connects to the same sides either way: then it passes) and decorated pots
    // (they wobble).
    static boolean usable(net.minecraft.world.level.block.Block block) {
        return block instanceof net.minecraft.world.level.block.DoorBlock || block instanceof net.minecraft.world.level.block.TrapDoorBlock
            || block instanceof net.minecraft.world.level.block.FenceGateBlock || block instanceof net.minecraft.world.level.block.RepeaterBlock
            || block instanceof net.minecraft.world.level.block.ComparatorBlock || block instanceof net.minecraft.world.level.block.NoteBlock
            || block instanceof net.minecraft.world.level.block.DaylightDetectorBlock || block instanceof net.minecraft.world.level.block.RedStoneWireBlock
            || block instanceof net.minecraft.world.level.block.DecoratedPotBlock;
    }

    // The detached input player: a survival ServerPlayer that is in no world.
    // The verifier's lectern and use inputs and the live world's "use" input
    // (McbenchEngine.input) act as it. Making it draws an entity id and two
    // longs of the shared random (the insecure UUID Entity's constructor makes
    // before the game profile's replaces it); its yaw is a Math.random value
    // until an input sets it.
    static net.minecraft.server.level.ServerPlayer newInputPlayer(ServerLevel level) {
        var player = new net.minecraft.server.level.ServerPlayer(level.getServer(), level,
            new com.mojang.authlib.GameProfile(java.util.UUID.fromString("bea44406-5db6-43c8-aa1b-d0f14f04ac48"), "VerifierInput"), net.minecraft.server.level.ClientInformation.createDefault());
        // Initialize the detached input actor as a normal survival player.
        // The native initializer sets both game mode and abilities. The public
        // change method also broadcasts packets and requires a live connection;
        // there is no network player in this isolated input fixture.
        try {
            var initialize = net.minecraft.server.level.ServerPlayerGameMode.class.getDeclaredMethod(
                "setGameModeForPlayer", net.minecraft.world.level.GameType.class, net.minecraft.world.level.GameType.class);
            initialize.setAccessible(true);
            initialize.invoke(player.gameMode, net.minecraft.world.level.GameType.SURVIVAL, null);
        } catch (ReflectiveOperationException error) { throw new IllegalStateException(error); }
        if (!player.gameMode.isSurvival() || !player.mayBuild()) throw new IllegalStateException("Could not initialize native survival input actor");
        return player;
    }

    private void interact(JsonObject input) {
        JsonObject outcome = input.deepCopy();
        inputs.add(outcome);
        BlockPos pos = position(input.getAsJsonArray("at"));
        ServerLevel level = ((CraftWorld) world).getHandle();
        var state = level.getBlockState(pos);
        String type = input.get("type").getAsString();
        if (type.equals("drop")) {
            try {
                var stack = suppliedStack(level, input);
                var at = input.getAsJsonArray("at"); var velocity = input.getAsJsonArray("velocity");
                var entity = new net.minecraft.world.entity.item.ItemEntity(level,
                    at.get(0).getAsDouble(), at.get(1).getAsDouble() + request.get("yOffset").getAsInt(), at.get(2).getAsDouble(), stack,
                    velocity.get(0).getAsDouble(), velocity.get(1).getAsDouble(), velocity.get(2).getAsDouble());
                entity.setDefaultPickUpDelay();
                if (!level.addFreshEntity(entity)) throw new IOException("Native item supply failed");
                outcome.addProperty("entityUUID", entity.getUUID().toString()); outcome.addProperty("applied", true);
            } catch (IOException error) { outcome.addProperty("applied", false); outcome.addProperty("error", error.getMessage()); }
        } else if (type.equals("insert") || type.equals("extract")) {
            try { inventoryInput(level, pos, input); outcome.addProperty("applied", true); }
            catch (IOException error) { outcome.addProperty("applied", false); outcome.addProperty("error", error.getMessage()); }
        } else if (type.equals("drain")) {
            try { outcome.add("removed", drainInventory(level, pos)); outcome.addProperty("applied", true); }
            catch (IOException error) { outcome.addProperty("applied", false); outcome.addProperty("error", error.getMessage()); }
        } else if (type.equals("book")) {
            try {
                if (!(level.getBlockEntity(pos) instanceof net.minecraft.world.level.block.entity.LecternBlockEntity lectern)) throw new IOException("Book input requires a lectern");
                inputPlayer(level);
                boolean present = input.get("present").getAsBoolean();
                boolean supplied = input.has("stack");
                if (supplied && (!present || lectern.hasBook() || heldLecternBooks.containsKey(pos))) throw new IOException("Book supply requires an empty lectern with no previously removed book");
                if (present && !lectern.hasBook()) {
                    var carried = supplied ? suppliedStack(level, input) : heldLecternBooks.get(pos);
                    if (carried == null || carried.isEmpty()) throw new IOException("No previously removed source book is available for this lectern");
                    if (carried.getCount() != 1 || !(carried.is(net.minecraft.world.item.Items.WRITTEN_BOOK) || carried.is(net.minecraft.world.item.Items.WRITABLE_BOOK))) throw new IOException("Lectern supply requires exactly one native book");
                    if (!net.minecraft.world.level.block.LecternBlock.tryPlaceBook(interactionPlayer, level, pos, state, carried) || !lectern.hasBook()) throw new IOException("Native book replacement failed");
                    if (!carried.isEmpty()) throw new IOException("Native book replacement did not consume the carried book");
                    heldLecternBooks.remove(pos);
                } else if (!present && lectern.hasBook()) {
                    if (heldLecternBooks.containsKey(pos)) throw new IOException("A source book is already held for this lectern");
                    var inventory = interactionPlayer.getInventory();
                    if (!inventory.isEmpty()) throw new IOException("Input actor inventory is not empty");
                    var original = lectern.getBook().copy();
                    var menu = lectern.createMenu(0, inventory, interactionPlayer);
                    if (!(menu instanceof net.minecraft.world.inventory.LecternMenu lecternMenu) || !lecternMenu.clickMenuButton(interactionPlayer, net.minecraft.world.inventory.LecternMenu.BUTTON_TAKE_BOOK) || lectern.hasBook()) throw new IOException("Native book removal failed");
                    int slot = -1;
                    for (int i = 0; i < inventory.getContainerSize(); i++) if (!inventory.getItem(i).isEmpty()) {
                        if (slot != -1 || !net.minecraft.world.item.ItemStack.matches(original, inventory.getItem(i))) throw new IOException("Native removed book does not match the source book");
                        slot = i;
                    }
                    if (slot == -1) throw new IOException("Native source book was not transferred to the input actor");
                    heldLecternBooks.put(pos.immutable(), inventory.removeItemNoUpdate(slot));
                }
                outcome.addProperty("applied", true);
            } catch (IOException error) { outcome.addProperty("applied", false); outcome.addProperty("error", error.getMessage()); }
        } else if (type.equals("page")) {
            try {
                if (!(level.getBlockEntity(pos) instanceof net.minecraft.world.level.block.entity.LecternBlockEntity lectern) || !lectern.hasBook()) throw new IOException("Page input requires a lectern with a book");
                var book = lectern.getBook();
                var written = book.get(net.minecraft.core.component.DataComponents.WRITTEN_BOOK_CONTENT);
                var writable = book.get(net.minecraft.core.component.DataComponents.WRITABLE_BOOK_CONTENT);
                int count = written != null ? written.pages().size() : writable != null ? writable.pages().size() : 0;
                int page = input.get("page").getAsInt();
                if (page < 0 || page >= count) throw new IOException("Page is outside this book; nothing changed");
                inputPlayer(level);
                var menu = lectern.createMenu(0, interactionPlayer.getInventory(), interactionPlayer);
                if (!(menu instanceof net.minecraft.world.inventory.LecternMenu lecternMenu)) throw new IOException("Could not open native lectern menu");
                int previous = lectern.getPage();
                // Vanilla page-link/menu button path: native setData invokes the
                // lectern's page update, including its normal redstone pulse.
                if (!lecternMenu.clickMenuButton(interactionPlayer, net.minecraft.world.inventory.LecternMenu.BUTTON_PAGE_JUMP_RANGE_START + page) || lectern.getPage() != page) throw new IOException("Native page selection failed");
                outcome.addProperty("previousPage", previous); outcome.addProperty("applied", true);
            } catch (IOException error) { outcome.addProperty("applied", false); outcome.addProperty("error", error.getMessage()); }
        } else if (type.equals("use") && usable(state.getBlock())) {
            // A fence gate opens away from the player, so its use needs the
            // yaw; no other usable block reads it.
            if (!input.has("yaw") && state.getBlock() instanceof net.minecraft.world.level.block.FenceGateBlock) {
                outcome.addProperty("applied", false);
                outcome.addProperty("error", "Using a fence gate requires yaw, the direction the player faces (it opens away from the player)");
            } else {
                inputPlayer(level);
                if (input.has("yaw")) interactionPlayer.setYRot(input.get("yaw").getAsFloat());
                // Native interaction, as McbenchEngine.input's: not a scripted replacement state or observer pulse.
                var used = state.useWithoutItem(level, interactionPlayer, new net.minecraft.world.phys.BlockHitResult(
                    net.minecraft.world.phys.Vec3.atCenterOf(pos), net.minecraft.core.Direction.UP, pos, false));
                outcome.addProperty("applied", used.consumesAction());
                if (!used.consumesAction()) outcome.addProperty("error", "Use does nothing to " + world.getBlockAt(pos.getX(), pos.getY(), pos.getZ()).getBlockData().getAsString());
            }
        } else if (type.equals("press") && state.getBlock() instanceof ButtonBlock button) {
            if (state.getValue(ButtonBlock.POWERED)) {
                outcome.addProperty("applied", false);
                outcome.addProperty("error", "Button is already pressed");
            } else {
                button.press(state, level, pos, null);
                outcome.addProperty("applied", true);
            }
        } else if (type.equals("toggle") && state.getBlock() instanceof LeverBlock lever) {
            lever.pull(state, level, pos, null);
            outcome.addProperty("applied", true);
        } else {
            outcome.addProperty("applied", false);
            outcome.addProperty("error", "Input requires " + (type.equals("press") ? "a button" : type.equals("use") ? "a door, trapdoor, fence gate, repeater, comparator, note block, daylight detector, redstone dust or decorated pot" : "a lever") + "; found " + world.getBlockAt(pos.getX(), pos.getY(), pos.getZ()).getBlockData().getAsString());
        }
        if (!outcome.get("applied").getAsBoolean()) {
            failedInputs++;
            if (inputFailures.size() < 8) inputFailures.add(outcome);
        }
    }

    // A seeded run's chunks, for comparing engines: each of the request's
    // columns as Minecraft saves it (blocks, block entities, scheduled
    // ticks, light, heightmaps), keyed "x,z".
    static JsonObject chunks(ServerLevel level, JsonArray columns) {
        JsonObject chunks = new JsonObject();
        for (JsonElement column : columns) {
            int x = column.getAsJsonArray().get(0).getAsInt(), z = column.getAsJsonArray().get(1).getAsInt();
            var saved = net.minecraft.world.level.chunk.storage.SerializableChunkData.copyOf(level, level.getChunk(x, z)).write();
            chunks.addProperty(x + "," + z, saved.toString());
        }
        return chunks;
    }

    public void finish(Exception error) {
        running = false;
        host.stopped();
        if (request.has("seed")) result.add("chunks", chunks(((CraftWorld) world).getHandle(), request.getAsJsonArray("columns")));
        result.addProperty("elapsedTicks", lastTick);
        result.addProperty("probeCount", probes.length);
        result.addProperty("sampleCount", sampleIndex);
        result.addProperty("inputCount", inputs.size());
        result.addProperty("failedInputs", failedInputs);
        result.addProperty("observationCount", observationCount);
        JsonObject metrics = new JsonObject();
        metrics.addProperty("probeReadMs", probeNanos / 1e6);
        metrics.addProperty("maxSampleReadMs", maxProbeNanos / 1e6);
        metrics.addProperty("maxObservedHeapBytes", maxHeapBytes);
        metrics.addProperty("maxHeapBytes", Runtime.getRuntime().maxMemory());
        result.add("metrics", metrics);
        JsonObject preview = new JsonObject();
        preview.add("initial", previewInitial);
        preview.add("changes", previewChanges);
        preview.add("inputFailures", inputFailures);
        preview.addProperty("changeCount", changeCount);
        int changedCount = 0;
        for (boolean changed : changedProbes) if (changed) changedCount++;
        preview.addProperty("changedProbeCount", changedCount);
        preview.addProperty("truncated", previewInitial.size() < probes.length || previewChanges.size() < changeCount || inputFailures.size() < failedInputs);
        preview.addProperty("note", "Probe indexes are zero-based. Changes compare adjacent requested samples, starting with initial; unsampled transitions are not counted. signal is Bukkit getBlockPower (received neighbor power), not a component's own power/powered/lit state; read those in block.");
        result.add("preview", preview);
        try {
            if (error == null) {
                flushPending();
                report.endArray().name("inputs"); gson.toJson(inputs, report);
                report.name("ok").value(true);
                for (var entry : result.entrySet()) {
                    if (!entry.getKey().equals("engine") && !entry.getKey().equals("minecraftVersion") && !entry.getKey().equals("reportEncoding")) {
                        report.name(entry.getKey()); gson.toJson(entry.getValue(), report);
                    }
                }
                report.endObject();
                report.close();
                Files.move(folder.resolve("report.tmp"), folder.resolve("report.json"), StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
            }
        } catch (Exception failure) { error = failure; }
        if (error != null && report != null) { try { report.close(); } catch (Exception ignored) {} }
        result.addProperty("ok", error == null);
        result.addProperty("reportBytes", reportBytes);
        if (error != null) {
            result.addProperty("error", error.toString());
            Throwable cause = error;
            while (cause.getCause() != null) cause = cause.getCause();
            if (cause instanceof ReportBudgetExceeded exceeded) {
                result.addProperty("budgetResource", "reportBytes");
                result.addProperty("requested", exceeded.requested);
            }
        }
        try { writeJson(folder, "result", result); }
        catch (IOException failure) { throw new UncheckedIOException("Cannot write test result", failure); }
    }

    private static final class ReportBudgetExceeded extends IOException {
        final long requested;
        ReportBudgetExceeded(long requested) { super("Report byte budget exceeded"); this.requested = requested; }
    }

    void progress(String phase, long tick, int totalTicks) throws IOException {
        long now = System.nanoTime();
        if (tick != totalTicks && now - lastProgressNanos < 100_000_000L) return;
        lastProgressNanos = now;
        if (report != null) report.flush();
        JsonObject status = new JsonObject();
        status.addProperty("phase", phase);
        status.addProperty("tick", tick);
        status.addProperty("totalTicks", totalTicks);
        status.addProperty("observations", observationCount);
        status.addProperty("reportBytes", reportBytes);
        writeJson(folder, "progress", status);
    }

    static void writeJson(Path folder, String name, JsonObject value) throws IOException {
        Path temporary = folder.resolve(name + ".tmp");
        Files.writeString(temporary, gson.toJson(value));
        Files.move(temporary, folder.resolve(name + ".json"), StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
    }
}
