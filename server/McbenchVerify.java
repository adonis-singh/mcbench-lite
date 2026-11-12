import com.google.gson.*;
import com.google.gson.stream.JsonWriter;
import com.destroystokyo.paper.event.server.ServerTickEndEvent;
import net.minecraft.core.BlockPos;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.level.block.ButtonBlock;
import net.minecraft.world.level.block.LeverBlock;
import org.bukkit.Bukkit;
import org.bukkit.World;
import org.bukkit.WorldCreator;
import org.bukkit.GameRule;
import org.bukkit.generator.ChunkGenerator;
import java.util.Comparator;
import java.util.Arrays;
import java.util.Random;
import java.util.concurrent.CompletableFuture;
import org.bukkit.Location;
import org.bukkit.craftbukkit.CraftServer;
import net.minecraft.commands.CommandSource;
import net.minecraft.commands.CommandSourceStack;
import net.minecraft.network.chat.Component;
import java.util.ArrayList;
import org.bukkit.command.Command;
import org.bukkit.command.CommandSender;
import org.bukkit.command.RemoteConsoleCommandSender;
import org.bukkit.craftbukkit.CraftWorld;
import org.bukkit.event.EventHandler;
import org.bukkit.event.Listener;
import org.bukkit.plugin.java.JavaPlugin;
import java.nio.file.*;
import java.io.*;
import java.nio.charset.StandardCharsets;

/** Runs only in the disposable verifier server. No design or repair logic. */
@SuppressWarnings("removal")
public final class McbenchVerify extends JavaPlugin implements Listener {
    private JsonObject request;
    private McbenchExperiment experiment;
    private World world;
    private boolean prepared;

    private void freshWorld() throws IOException {
        World old = Bukkit.getWorld("mcbench_test");
        if (old != null) {
            old.removePluginChunkTickets(this);
            if (!Bukkit.unloadWorld(old, false)) throw new IOException("Cannot unload previous test world");
        }
        Path folder = Bukkit.getWorldContainer().toPath().resolve("mcbench_test");
        if (Files.exists(folder)) try (var paths = Files.walk(folder)) {
            for (Path file : paths.sorted(Comparator.reverseOrder()).toList()) Files.delete(file);
        }
        // Paper's activation-range throttle is not vanilla behavior: loaded
        // entities must tick even without a nearby player. Set the supported
        // world option before construction/import, when activation is initialized.
        for (String group : new String[]{"animals","monsters","raiders","misc","water","villagers","flying-monsters"})
            org.spigotmc.SpigotConfig.config.set("world-settings.mcbench_test.entity-activation-range." + group, 0);
        var creator = new WorldCreator("mcbench_test").environment(World.Environment.NORMAL)
            .generator(new ChunkGenerator() {
                @Override public Location getFixedSpawnLocation(World created, Random random) { return new Location(created, 0, 80, 0); }
                // The live world's biome everywhere (minecraft-live.js's
                // generator), not the overworld's biomes for the seed.
                @Override public org.bukkit.generator.BiomeProvider getDefaultBiomeProvider(org.bukkit.generator.WorldInfo info) {
                    return new org.bukkit.generator.BiomeProvider() {
                        @Override public org.bukkit.block.Biome getBiome(org.bukkit.generator.WorldInfo info, int x, int y, int z) { return org.bukkit.block.Biome.PLAINS; }
                        @Override public java.util.List<org.bukkit.block.Biome> getBiomes(org.bukkit.generator.WorldInfo info) { return java.util.List.of(org.bukkit.block.Biome.PLAINS); }
                    };
                }
            }).generateStructures(false).keepSpawnLoaded(net.kyori.adventure.util.TriState.FALSE);
        // A seed fixes the world's random draws, for comparing engines; a
        // test without one keeps Minecraft's own unseeded randomness.
        if (request.has("seed")) creator.seed(request.get("seed").getAsLong());
        world = creator.createWorld();
        if (world == null) throw new IOException("Cannot create fresh test world");
        if (request.has("seed")) try { McbenchExperiment.seed(((CraftWorld) world).getHandle(), request.get("seed").getAsLong()); }
        catch (ReflectiveOperationException error) { throw new IOException("Cannot seed the test world", error); }
        if (((CraftWorld) world).getHandle().paperConfig().misc.redstoneImplementation
            != io.papermc.paper.configuration.WorldConfiguration.Misc.RedstoneImplementation.VANILLA) {
            throw new IOException("Verification requires the vanilla redstone implementation");
        }
        world.setAutoSave(false);
        world.setDifficulty(org.bukkit.Difficulty.PEACEFUL);
        world.setGameRule(GameRule.DO_MOB_SPAWNING, false);
        world.setGameRule(GameRule.DO_DAYLIGHT_CYCLE, false);
        world.setGameRule(GameRule.DO_WEATHER_CYCLE, false);
        world.setGameRule(GameRule.DO_FIRE_TICK, false);
        world.setGameRule(GameRule.RANDOM_TICK_SPEED, 0);
        world.setTime(6000);
    }

    // The server dies with the process that started it, however that ends.
    private static void watchParent() {
        var parent = ProcessHandle.current().parent().orElseThrow();
        var watch = new Thread(() -> {
            while (parent.isAlive()) {
                try { Thread.sleep(1000); } catch (InterruptedException error) { return; }
            }
            Runtime.getRuntime().halt(1);
        }, "mcbench-parent-watch");
        watch.setDaemon(true);
        watch.start();
    }

    @Override public void onEnable() {
        watchParent();
        Bukkit.getPluginManager().registerEvents(this, this);
    }

    @Override public boolean onCommand(CommandSender sender, Command command, String label, String[] args) {
        if (!(sender instanceof RemoteConsoleCommandSender)) return true;
        try {
            String action = args.length >= 1 ? args[0] : "";
            if (action.equals("prepare")) {
                if (experiment != null && experiment.running()) throw new IllegalStateException("A test is still running");
                // The last experiment's report and state go before the next request is read.
                experiment = null; prepared = false;
                // Each experiment's request arrives in its own file, read once.
                if (args.length != 2 || !args[1].matches("request-[0-9a-f-]{36}\\.json")) throw new IOException("prepare needs a request file name");
                Path requestFile = getDataFolder().toPath().resolve(args[1]);
                try (Reader input = Files.newBufferedReader(requestFile, StandardCharsets.UTF_8)) {
                    request = JsonParser.parseReader(input).getAsJsonObject();
                }
                Files.delete(requestFile);
                Bukkit.getServerTickManager().stopSprinting();
                Bukkit.getServerTickManager().setFrozen(true);
                freshWorld();
                experiment = new McbenchExperiment(request, world, getDataFolder().toPath(), this::stopped);
                prepared = false;
                if (request.has("asyncPreparation") && request.get("asyncPreparation").getAsBoolean()) {
                    prepareChunks(0);
                    sender.sendMessage("MCBENCH_PREPARING");
                } else {
                    // Workers already running may retain the older synchronous protocol.
                    for (JsonElement entry : request.getAsJsonArray("columns")) {
                        JsonArray p = entry.getAsJsonArray();
                        world.addPluginChunkTicket(p.get(0).getAsInt(), p.get(1).getAsInt(), this);
                    }
                    prepared = true;
                    sender.sendMessage("MCBENCH_READY " + world.getKey());
                }
            } else if (action.equals("apply")) {
                if (!prepared || experiment.running()) throw new IllegalStateException("Prepare a frozen world before placement");
                // Each batch arrives in its own file. Docker Desktop's file sharing can
                    // serve a stale, shorter copy of a file rewritten in place, which
                    // truncated commands mid-word; a name used once cannot be stale.
                    if (args.length != 2 || !args[1].matches("batch-[0-9a-f-]{36}\\.txt")) throw new IOException("apply needs a batch file name");
                    var batchFile = getDataFolder().toPath().resolve(args[1]);
                    var lines = Files.readAllLines(batchFile);
                    Files.delete(batchFile);
                if (lines.size() > 512) throw new IOException("Placement batch exceeds 512 commands");
                var errors = new ArrayList<String>();
                var source = new CommandSource() {
                    public void sendSystemMessage(Component message) { errors.add(message.getString()); }
                    public boolean acceptsSuccess() { return false; }
                    public boolean acceptsFailure() { return true; }
                    public boolean shouldInformAdmins() { return false; }
                    public CommandSender getBukkitSender(CommandSourceStack stack) { return sender; }
                };
                var server = ((CraftServer) Bukkit.getServer()).getServer();
                var stack = server.createCommandSourceStack().withLevel(((CraftWorld) world).getHandle()).withSource(source);
                int count = 0;
                for (String line : lines) {
                    if (!line.startsWith("setblock ") && !line.startsWith("fill ")) throw new IOException("Unsupported placement command");
                    if(request.has("block_data_version")){
                        String[] parts=line.split(" ");int index=line.startsWith("setblock ")?4:7;
                        if(parts.length<=index)throw new IOException("Invalid placement command");
                        parts[index]=experiment.upgradeBlockToken(parts[index]);line=String.join(" ",parts);
                    }
                    errors.clear();
                    try { server.getCommands().getDispatcher().execute(line + (request.has("placementMode") && request.get("placementMode").getAsString().equals("strict") ? " strict" : ""), stack); }
                    catch (com.mojang.brigadier.exceptions.CommandSyntaxException error) { errors.add(error.getRawMessage().getString()); }
                    for (String error : errors) if (!error.contains("No blocks were filled") && !error.contains("Could not set the block")) throw new IOException("Command " + count + ": " + error);
                    count++;
                }
                sender.sendMessage("MCBENCH_APPLIED " + count);
            } else if (action.equals("run")) {
                if (!prepared || experiment.started()) throw new IllegalStateException("Prepare a fresh test world first");
                experiment.start(Bukkit.getName() + " " + Bukkit.getVersion(), Bukkit.getMinecraftVersion());
                awaitTickingChunks(() -> {
                    experiment.begin();
                    Bukkit.getServerTickManager().setFrozen(false);
                    if (experiment.sprint()) {
                        Bukkit.getServerTickManager().requestGameToSprint(Integer.MAX_VALUE);
                        if (!Bukkit.getServerTickManager().isSprinting()) throw new IllegalStateException("Could not start tick sprint");
                    }
                }, experiment::finish);
                sender.sendMessage("MCBENCH_RUNNING");
            } else {
                sender.sendMessage("MCBENCH_ERROR unknown command");
            }
        } catch (Exception error) {
            if (experiment != null && experiment.started()) experiment.finish(error);
            sender.sendMessage("MCBENCH_ERROR " + error.getMessage());
        }
        return true;
    }

    @EventHandler public void tick(ServerTickEndEvent event) {
        if (experiment != null) experiment.endOfTick();
    }

    // The experiment's end: ticks stop, and in a seeded run, whose chunks are
    // read, the columns are lit again from their blocks.
    //
    // Paper's light worker lights the changes of a chunk in batches whose
    // edges fall where its threads happen to be, and Starlight's incremental
    // light depends on those edges: a run can end with light that a later
    // batch would have removed (soontech-MG02 sometimes ends with a few cells
    // of block light too bright at a chunk border), and that state stays the
    // same however long it is waited for. Paper's own relight of loaded chunks
    // (starlight$serverRelightChunks) gives the light the blocks define, the
    // same whenever it is asked. Its task runs on the chunk system's
    // area-dependent queue over the columns' area, so it runs once every light
    // task queued before it near the columns has run.
    private void stopped() {
        Bukkit.getServerTickManager().stopSprinting();
        Bukkit.getServerTickManager().setFrozen(true);
        if (!request.has("seed")) return;
        var level = ((CraftWorld) world).getHandle();
        var provider = (ca.spottedleaf.moonrise.patches.starlight.light.StarLightLightingProvider) level.getChunkSource().getLightEngine();
        var columns = new ArrayList<net.minecraft.world.level.ChunkPos>();
        for (JsonElement column : request.getAsJsonArray("columns")) {
            JsonArray at = column.getAsJsonArray();
            columns.add(new net.minecraft.world.level.ChunkPos(at.get(0).getAsInt(), at.get(1).getAsInt()));
        }
        var lit = new java.util.concurrent.CountDownLatch(1);
        var relit = new java.util.concurrent.atomic.AtomicInteger(-1);
        if (provider.starlight$serverRelightChunks(columns, chunk -> {}, count -> { relit.set(count); lit.countDown(); }) != columns.size())
            throw new IllegalStateException("A column cannot be relit");
        try { lit.await(); } catch (InterruptedException interrupted) { throw new IllegalStateException(interrupted); }
        if (relit.get() != columns.size()) throw new IllegalStateException("Relit " + relit.get() + " of " + columns.size() + " columns");
        if (provider.starlight$getLightEngine().hasUpdates()) throw new IllegalStateException("Light changes queued after the relight");
    }

    private void prepareChunks(int start) {
        JsonArray columns = request.getAsJsonArray("columns");
        if (start >= columns.size()) {
            awaitTickingChunks(() -> {
                prepared = true;
                JsonObject ready = new JsonObject();
                ready.addProperty("ok", true); ready.addProperty("dimension", world.getKey().toString());
                ready.addProperty("batchPlacement", true);
                try { McbenchExperiment.writeJson(getDataFolder().toPath(), "prepared", ready); }
                catch (IOException error) { throw new java.io.UncheckedIOException(error); }
            }, error -> preparationFailed(error));
            return;
        }
        int end = Math.min(start + 32, columns.size());
        CompletableFuture<?>[] batch = new CompletableFuture<?>[end - start];
        for (int i = start; i < end; i++) {
            JsonArray p = columns.get(i).getAsJsonArray();
            batch[i - start] = world.getChunkAtAsync(p.get(0).getAsInt(), p.get(1).getAsInt(), true);
        }
        CompletableFuture.allOf(batch).whenComplete((ignored, failure) -> Bukkit.getScheduler().runTask(this, () -> {
            if (failure != null) { preparationFailed(failure); return; }
            try {
                for (int i = start; i < end; i++) {
                    JsonArray p = columns.get(i).getAsJsonArray();
                    world.addPluginChunkTicket(p.get(0).getAsInt(), p.get(1).getAsInt(), this);
                }
                experiment.progress("loading", end, columns.size());
                prepareChunks(end);
            } catch (Exception error) { preparationFailed(error); }
        }));
    }

    private void awaitTickingChunks(Runnable ready, java.util.function.Consumer<Exception> failed) {
        try {
            ServerLevel level = ((CraftWorld) world).getHandle();
            JsonArray columns = request.getAsJsonArray("columns");
            int active = 0;
            for (JsonElement entry : columns) {
                JsonArray p = entry.getAsJsonArray();
                if (level.isPositionEntityTicking(new BlockPos(p.get(0).getAsInt() * 16, 0, p.get(1).getAsInt() * 16))) active++;
            }
            if (active == columns.size()) { ready.run(); return; }
            // A FULL chunk future is not yet an entity-ticking chunk. Wait with
            // game time frozen so wall-clock chunk I/O cannot delay native physics.
            experiment.progress("activating", active, columns.size());
            Bukkit.getScheduler().runTaskLater(this, () -> awaitTickingChunks(ready, failed), 1L);
        } catch (Exception error) { failed.accept(error); }
    }

    private void preparationFailed(Throwable error) {
        prepared = false;
        try {
            JsonObject failed = new JsonObject(); failed.addProperty("ok", false); failed.addProperty("error", error.toString());
            McbenchExperiment.writeJson(getDataFolder().toPath(), "prepared", failed);
        } catch (IOException failure) { getLogger().severe("Cannot report preparation failure: " + failure); }
    }

}
