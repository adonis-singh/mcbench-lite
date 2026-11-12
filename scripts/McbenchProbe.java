import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import java.io.*;
import java.util.*;

// The Java side of scripts/core-probe-parity.js: answers lines of
// "<command> <rest>" about saved data (SNBT, item stacks, block entities,
// entities) with the pinned server's own classes, one answer per line.
public class McbenchProbe {
  public static void main(String[] args) throws Exception {
    var failure = new java.util.concurrent.atomic.AtomicReference<Throwable>();
    var worker = new ca.spottedleaf.moonrise.common.util.TickThread(() -> {
      try { run(); } catch (Throwable error) { failure.set(error); }
    }, "probe");
    worker.start();
    worker.join();
    if (failure.get() != null) { failure.get().printStackTrace(System.err); System.exit(1); }
  }

  static void run() throws Exception {
    var out = McbenchEngine.reply;
    McbenchEngine.boot();
    var reader = new BufferedReader(new InputStreamReader(System.in));
    for (String line; (line = reader.readLine()) != null;) {
      int space = line.indexOf(' ');
      String cmd = space < 0 ? line : line.substring(0, space), rest = space < 0 ? "" : line.substring(space + 1);
      try {
        out.println("A " + handle(cmd, rest).replace('\n', ' '));
      } catch (Throwable error) {
        out.println("A EXC " + McbenchEngine.describe(error).split(" @ ")[0]);
      }
      out.flush();
    }
  }

  static McbenchLevel level;

  // A level of its own, seeded: block entities of one line do not meet those of another.
  static McbenchLevel fresh() throws Exception {
    var made = McbenchLevel.create(0);
    McbenchExperiment.seed(made, 0);
    McbenchEngine.activateAll(made);
    McbenchEngine.level = made;
    made.ticket(0, 0);
    return made;
  }

  static String handle(String cmd, String rest) throws Exception {
    if (level == null || cmd.equals("entity")) level = fresh();
    var access = level.registryAccess();
    switch (cmd) {
      case "snbt": {
        var tag = net.minecraft.nbt.TagParser.parseCompoundFully(rest);
        return "OK " + tag;
      }
      case "value": {
        var tag = new net.minecraft.nbt.SnbtPrinterTagVisitor();
        var parsed = net.minecraft.nbt.TagParser.create(net.minecraft.nbt.NbtOps.INSTANCE).parseFully(rest);
        return "OK " + parsed;
      }
      case "stack": {
        var tag = net.minecraft.nbt.TagParser.parseCompoundFully(rest);
        var ctx = access.createSerializationContext(net.minecraft.nbt.NbtOps.INSTANCE);
        var result = net.minecraft.world.item.ItemStack.CODEC.parse(ctx, tag);
        if (result.error().isPresent()) return "ERR " + result.error().get().message();
        var stack = result.getOrThrow();
        var enc = net.minecraft.world.item.ItemStack.CODEC.encodeStart(ctx, stack).getOrThrow();
        return "OK " + enc + " | patch=" + stack.getComponentsPatch().isEmpty() + " max=" + stack.getMaxStackSize();
      }
      case "be": {
        // be x y z block snbt
        String[] p = rest.split(" ", 5);
        BlockPos at = new BlockPos(Integer.parseInt(p[0]), Integer.parseInt(p[1]), Integer.parseInt(p[2]));
        var parsed = net.minecraft.commands.arguments.blocks.BlockStateParser.parseForBlock(BuiltInRegistries.BLOCK, p[3], false);
        McbenchEngine.place(level, parsed, at, 2 | 816);
        var entity = level.getBlockEntity(at);
        if (entity == null) return "NOBE";
        // A copy: the grammar returns one shared compound for the text "{}", which putting a field in would change for every later parse.
        var tag = net.minecraft.nbt.TagParser.parseCompoundFully(p[4]).copy();
        String id = net.minecraft.world.level.block.entity.BlockEntityType.getKey(entity.getType()).toString();
        if (!tag.contains("id")) tag.putString("id", id);
        tag.putInt("x", at.getX()); tag.putInt("y", at.getY()); tag.putInt("z", at.getZ());
        var problems = new net.minecraft.util.ProblemReporter.Collector();
        entity.loadWithComponents(net.minecraft.world.level.storage.TagValueInput.create(problems, level.registryAccess(), tag));
        if (!problems.isEmpty()) return "PROBLEMS " + problems.getReport().replace('\n', ' ');
        entity.setChanged();
        return "OK " + entity.saveWithFullMetadata(level.registryAccess()) + " || " + entity.saveWithoutMetadata(level.registryAccess());
      }
      case "entity": {

        var tag = net.minecraft.nbt.TagParser.parseCompoundFully(rest).copy();
        var problems = new net.minecraft.util.ProblemReporter.Collector();
        var entity = net.minecraft.world.entity.EntityType.loadEntityRecursive(
          net.minecraft.world.level.storage.TagValueInput.create(problems, level.registryAccess(), tag), level, net.minecraft.world.entity.EntitySpawnReason.LOAD, e -> e);
        if (entity == null || !problems.isEmpty()) return "PROBLEMS " + problems.getReport().replace('\n', ' ');
        boolean added = level.tryAddFreshEntityWithPassengers(entity);
        var output = net.minecraft.world.level.storage.TagValueOutput.createWithContext(problems, level.registryAccess());
        boolean ok = entity.saveAsPassenger(output);
        return "OK added=" + added + " " + output.buildResult();
      }
      case "mpshape": {
        // mpshape <moved state> <direction> <extending 0|1> <source 0|1> <progress> <noclip direction or -> [x z]
        // The collision shape of a moving piston's block entity (PistonMovingBlockEntity.getCollisionShape),
        // as the block reads it from the level, and what depends on it: its faces and the sturdiness of each.
        String[] p = rest.split(" ");
        BlockPos at = p.length > 6 ? new BlockPos(Integer.parseInt(p[6]), 70, Integer.parseInt(p[7])) : new BlockPos(3, 70, 3);
        net.minecraft.world.level.block.state.BlockState moved = net.minecraft.commands.arguments.blocks.BlockStateParser.parseForBlock(BuiltInRegistries.BLOCK, p[0], false).blockState();
        var piston = new net.minecraft.world.level.block.piston.PistonMovingBlockEntity(at, net.minecraft.world.level.block.Blocks.MOVING_PISTON.defaultBlockState(),
          moved, net.minecraft.core.Direction.byName(p[1]), p[2].equals("1"), p[3].equals("1"));
        var progress = net.minecraft.world.level.block.piston.PistonMovingBlockEntity.class.getDeclaredField("progress");
        progress.setAccessible(true);
        progress.setFloat(piston, Float.parseFloat(p[4]));
        var noclip = net.minecraft.world.level.block.piston.PistonMovingBlockEntity.class.getDeclaredField("NOCLIP");
        noclip.setAccessible(true);
        @SuppressWarnings("unchecked") var noclipDirection = (ThreadLocal<net.minecraft.core.Direction>) noclip.get(null);
        var block = net.minecraft.world.level.block.Blocks.MOVING_PISTON.defaultBlockState();
        level.setBlock(at, block, 0);
        level.setBlockEntity(piston);
        try {
          noclipDirection.set(p[5].equals("-") ? null : net.minecraft.core.Direction.byName(p[5]));
          var shape = level.getBlockState(at).getCollisionShape(level, at);
          var out = new StringBuilder("OK ").append(McbenchEngine.shapeLine(shape));
          out.append(" | boxes");
          for (var box : shape.toAabbs()) out.append(' ').append(box.minX).append(',').append(box.minY).append(',').append(box.minZ).append(',').append(box.maxX).append(',').append(box.maxY).append(',').append(box.maxZ);
          for (var direction : net.minecraft.core.Direction.values()) {
            out.append(" | ").append(direction.getName()).append(' ').append(McbenchEngine.shapeLine(shape.getFaceShape(direction)));
            for (var support : net.minecraft.world.level.block.SupportType.values()) out.append(level.getBlockState(at).isFaceSturdy(level, at, direction, support) ? '1' : '0');
          }
          out.append(" | full ").append(net.minecraft.world.level.block.Block.isShapeFullBlock(shape));
          return out.toString();
        } finally {
          noclipDirection.set(null);
          level.setBlock(at, net.minecraft.world.level.block.Blocks.AIR.defaultBlockState(), 0);
        }
      }
      default: return "UNKNOWN " + cmd;
    }
  }
}
