import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LightLayer;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.BufferedReader;
import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.InputStreamReader;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.TreeSet;

// Paper's own light of blueprints, for scripts/mesher-parity.js --paper to
// check the live viewer's mesher (renderer/src/world.rs) against, cell for
// cell. Never part of the app.
//
// Each blueprint (the viewer's format, McbenchEngine.write, at Minecraft
// heights) is set into a fresh McbenchLevel: the pinned server's own level,
// chunk and light classes, Paper's light engine (Starlight) among them, its
// built chunk columns holding chunk tickets, which load the two rings of
// chunks around each. Every block is set with the flags a strict setblock
// sets it with (its state as the blueprint names it: no shape updates, no
// neighbour updates). Then every loaded chunk is lit again from its blocks
// by Starlight's relight, as McbenchEngine.stopped and McbenchVerify light a
// world for saving: the light the blocks define, not that of the order they
// were set in. Written is the sky and block light of every cell, from the
// world's floor to its ceiling, of the chunk columns within one of a built
// column. Their cells are 16 or more from every chunk not loaded and light
// travels at most 15, so they hold the light of a build standing alone in an
// endless empty world.
//
// Compiled with the engine's sources (server/engine.js SOURCES) on Paper's
// classpath, run in a folder holding the engine's world folder (its data
// packs: server/minecraft-dimension.js writeDimensionPack):
//   java -cp <paper classpath>:<classes> McbenchPaperLight
// Requests on stdin, one per line: <blueprint file> <light file>; the reply
// is "MCBENCH_LIT <columns> <ms>", or "MCBENCH_LIGHT_FAILED <reason>" after
// which it stops. The light file, big endian: i32 columns, then for each its
// i32 chunk x and z and 1024 x 16 x 16 bytes, height from the floor up, then
// z, then x: sky light << 4 | block light.
public class McbenchPaperLight {
  static final java.io.PrintStream reply = System.out;

  public static void main(String[] args) throws Exception {
    var failure = new java.util.concurrent.atomic.AtomicReference<Throwable>();
    // Paper accepts block changes only on a tick thread.
    var worker = new ca.spottedleaf.moonrise.common.util.TickThread(() -> {
      try {
        McbenchEngine.boot();
        var input = new BufferedReader(new InputStreamReader(System.in));
        for (String line; (line = input.readLine()) != null;) {
          String[] request = line.split(" ");
          long start = System.nanoTime();
          int columns = light(Path.of(request[0]), Path.of(request[1]));
          reply.println("MCBENCH_LIT " + columns + " " + (System.nanoTime() - start) / 1_000_000);
          reply.flush();
        }
      } catch (Throwable error) { failure.set(error); }
    }, "mcbench-paper-light");
    worker.start();
    worker.join();
    if (failure.get() != null) {
      reply.println("MCBENCH_LIGHT_FAILED " + McbenchEngine.describe(failure.get()));
      reply.flush();
      System.exit(1);
    }
  }

  static int light(Path blueprint, Path target) throws Exception {
    var states = new ArrayList<BlockState>();
    var blocks = new ArrayList<long[]>(); // x, y, z, palette entry
    var built = new TreeSet<Long>();
    try (var in = new DataInputStream(new BufferedInputStream(Files.newInputStream(blueprint)))) {
      for (int entries = in.readInt(); entries > 0; entries--) {
        var name = new StringBuilder(in.readUTF());
        int count = in.readInt();
        for (int n = 0; n < count; n++) name.append(n == 0 ? "[" : ",").append(in.readUTF()).append('=').append(in.readUTF());
        if (count > 0) name.append(']');
        states.add(net.minecraft.commands.arguments.blocks.BlockStateParser.parseForBlock(BuiltInRegistries.BLOCK, name.toString(), false).blockState());
      }
      for (int i = 0; i < 6; i++) in.readInt();
      for (int sections = in.readInt(); sections > 0; sections--) {
        int ox = in.readInt() << 4, oy = in.readInt() << 4, oz = in.readInt() << 4;
        for (int i = 0; i < 4096; i++) {
          int value = in.readUnsignedShort();
          if (value != 0) blocks.add(new long[] { ox + (i & 15), oy + (i >> 8), oz + (i >> 4 & 15), value });
        }
        built.add(ChunkPos.asLong(ox >> 4, oz >> 4));
      }
    }
    List<int[]> tickets = new ArrayList<>();
    for (long column : built) tickets.add(new int[] { ChunkPos.getX(column), ChunkPos.getZ(column) });
    McbenchLevel level = McbenchLevel.create(0, tickets);
    int flags = Block.UPDATE_CLIENTS | 816;
    var pos = new BlockPos.MutableBlockPos();
    for (long[] block : blocks) level.setBlock(pos.set((int) block[0], (int) block[1], (int) block[2]), states.get((int) block[3]), flags);
    var light = ((McbenchLevel.Chunks) level.getChunkSource()).light;
    light.flush();
    var loaded = new LinkedHashSet<ChunkPos>();
    for (long key : level.chunkMap.keySet().toLongArray()) loaded.add(new ChunkPos(key));
    var relit = new int[] { -1 };
    light.starlight().relightChunks(loaded, chunk -> {}, count -> relit[0] = count);
    if (relit[0] != loaded.size()) throw new IllegalStateException("Relit " + relit[0] + " of " + loaded.size() + " chunks");
    var compared = new TreeSet<Long>();
    for (long column : built) {
      for (int dx = -1; dx <= 1; dx++) for (int dz = -1; dz <= 1; dz++) compared.add(ChunkPos.asLong(ChunkPos.getX(column) + dx, ChunkPos.getZ(column) + dz));
    }
    int floor = level.getMinY(), height = level.getHeight();
    try (var out = new DataOutputStream(new BufferedOutputStream(Files.newOutputStream(target), 1 << 16))) {
      out.writeInt(compared.size());
      byte[] cells = new byte[height * 256];
      for (long column : compared) {
        int cx = ChunkPos.getX(column), cz = ChunkPos.getZ(column);
        out.writeInt(cx);
        out.writeInt(cz);
        int at = 0;
        for (int y = floor; y < floor + height; y++) for (int z = cz << 4; z < (cz << 4) + 16; z++) for (int x = cx << 4; x < (cx << 4) + 16; x++) {
          pos.set(x, y, z);
          cells[at++] = (byte) (level.getBrightness(LightLayer.SKY, pos) << 4 | level.getBrightness(LightLayer.BLOCK, pos));
        }
        out.write(cells);
      }
    }
    return compared.size();
  }
}
