import java.io.*;
import java.lang.reflect.*;
import java.nio.file.*;
import java.util.*;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.world.entity.player.Input;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.phys.AABB;

// The Java side of scripts/movement-parity.js: the creative-mode player's
// movement as Mojang's own 1.21.10 client code computes it. The class files
// are the client jar renamed to Mojang's official names (nothing of the
// client is rewritten), run headless: LocalPlayer is made with its real
// constructors, in a level (MovementLevel) that holds the case's blocks, and
// ticked as ClientLevel.tickNonPassenger ticks it, the input the keyboard
// would give set the way KeyboardInput sets it. Each tick's state is
// printed with every number as its exact bits.
//
// Cases come from a file of lines:
//   case <name>
//   set <x> <y> <z> <block state>        fill <x1> <y1> <z1> <x2> <y2> <z2> <block state>
//   player <x> <y> <z> <yaw> <pitch> <flying 0|1> [<autojump 0|1>]   (again: a new player in the same world)
//   edit <x> <y> <z> <block state>       (before the next tick)
//   editplayer <dx> <dy> <dz> <block state>   (the same at an offset from the player's block)
//   moving <x> <y> <z> <moving_piston state> <moved state> <facing> <extending 0|1> <source 0|1>   (a moving piston at progress 0)
//   boat <x> <y> <z>                     (an oak boat standing still)
//   tick <input bits> <yaw> <pitch>      (1 forward, 2 backward, 4 left, 8 right, 16 jump, 32 shift, 64 sprint)
//   end
public class MovementReference {
  static final sun.misc.Unsafe unsafe;
  static {
    try {
      Field f = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
      f.setAccessible(true);
      unsafe = (sun.misc.Unsafe) f.get(null);
    } catch (Exception e) { throw new RuntimeException(e); }
  }

  static Field field(Class<?> type, String name) throws Exception {
    Field f = type.getDeclaredField(name);
    f.setAccessible(true);
    return f;
  }

  static void set(Object target, Class<?> type, String name, Object value) throws Exception {
    Field f = type.getDeclaredField(name);
    if (Modifier.isStatic(f.getModifiers())) {
      unsafe.putObject(unsafe.staticFieldBase(f), unsafe.staticFieldOffset(f), value);
    } else {
      long offset = unsafe.objectFieldOffset(f);
      if (f.getType() == boolean.class) unsafe.putBoolean(target, offset, (Boolean) value);
      else if (f.getType() == int.class) unsafe.putInt(target, offset, (Integer) value);
      else if (f.getType() == float.class) unsafe.putFloat(target, offset, (Float) value);
      else if (f.getType() == long.class) unsafe.putLong(target, offset, (Long) value);
      else unsafe.putObject(target, offset, value);
    }
  }

  static Object get(Object target, Class<?> type, String name) throws Exception {
    return field(type, name).get(target);
  }

  public static void main(String[] args) throws Exception {
    PrintStream out = new PrintStream(new FileOutputStream(FileDescriptor.out), false);
    System.setOut(System.err);
    if (args[0].equals("--acos")) {
      // Math.acos for many arguments, as "<argument bits> <result bits>" lines, for core/src/jmath.rs.
      var random = new java.util.Random(1);
      var out2 = new StringBuilder();
      for (int i = 0; i < 300000; i++) {
        double x = switch (i % 6) {
          case 0 -> random.nextDouble() * 2 - 1;
          case 1 -> (random.nextDouble() * 2 - 1) * 1e-3;
          case 2 -> Math.copySign(1 - random.nextDouble() * 1e-6, random.nextBoolean() ? 1 : -1);
          case 3 -> Math.copySign(0.5 + (random.nextDouble() - 0.5) * 1e-6, random.nextBoolean() ? 1 : -1);
          case 4 -> (random.nextDouble() * 2 - 1) * 1e-18;
          default -> (random.nextInt(2001) - 1000) / 1000.0;
        };
        double r = Math.acos(x);
        if (Double.doubleToRawLongBits(r) != Double.doubleToRawLongBits(StrictMath.acos(x))) throw new IllegalStateException("Math.acos is not StrictMath.acos at " + x);
        out2.append(Double.doubleToRawLongBits(x)).append(' ').append(Double.doubleToRawLongBits(r)).append('\n');
      }
      Files.writeString(Path.of(args[1]), out2.toString());
      return;
    }
    boot();
    if (args[0].equals("--dump")) {
      dump(Path.of(args[1]));
      return;
    }
    List<String> lines = Files.readAllLines(Path.of(args[0]));
    List<String> block = new ArrayList<>();
    for (String line : lines) {
      if (line.startsWith("case ")) block.clear();
      block.add(line);
      if (line.equals("end")) {
        try {
          run(block, out);
        } catch (Throwable error) {
          out.println("error " + block.get(0).substring(5) + " " + error);
          error.printStackTrace();
        }
        out.flush();
      }
    }
  }

  // What the movement port reads of the client's blocks that the engine's
  // registry dump does not hold, as runs of state ids in
  // Block.BLOCK_STATE_REGISTRY order ("<kind> <first> <last> [<value>]"):
  // the states whose isSuffocating differs from its default (blocksMotion and
  // a full collision shape), and the states whose collision shape is
  // Shapes.block() itself, which BlockCollisions tests with AABB.intersects.
  static void dump(Path target) throws Exception {
    var out = new StringBuilder();
    var empty = net.minecraft.world.level.EmptyBlockGetter.INSTANCE;
    var context = net.minecraft.world.phys.shapes.CollisionContext.empty();
    int[] suffocating = { -1, -1 };
    boolean suffocatingValue = false;
    int[] cube = { -1, -1 };
    for (BlockState state : net.minecraft.world.level.block.Block.BLOCK_STATE_REGISTRY) {
      int id = net.minecraft.world.level.block.Block.BLOCK_STATE_REGISTRY.getId(state);
      boolean actual = state.isSuffocating(empty, BlockPos.ZERO);
      boolean normal = state.blocksMotion() && state.isCollisionShapeFullBlock(empty, BlockPos.ZERO);
      if (actual != normal) {
        if (suffocating[0] >= 0 && suffocatingValue == actual && suffocating[1] == id - 1) suffocating[1] = id;
        else {
          if (suffocating[0] >= 0) out.append("suffocating ").append(suffocating[0]).append(' ').append(suffocating[1]).append(' ').append(suffocatingValue).append('\n');
          suffocating[0] = suffocating[1] = id;
          suffocatingValue = actual;
        }
      }
      if (state.getCollisionShape(empty, BlockPos.ZERO, context) == net.minecraft.world.phys.shapes.Shapes.block()) {
        if (cube[0] >= 0 && cube[1] == id - 1) cube[1] = id;
        else {
          if (cube[0] >= 0) out.append("block ").append(cube[0]).append(' ').append(cube[1]).append('\n');
          cube[0] = cube[1] = id;
        }
      }
    }
    if (suffocating[0] >= 0) out.append("suffocating ").append(suffocating[0]).append(' ').append(suffocating[1]).append(' ').append(suffocatingValue).append('\n');
    if (cube[0] >= 0) out.append("block ").append(cube[0]).append(' ').append(cube[1]).append('\n');
    Files.writeString(target, out.toString());
  }

  static void boot() throws Exception {
    net.minecraft.SharedConstants.tryDetectVersion();
    net.minecraft.server.Bootstrap.bootStrap();
    var packs = net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
    var init = new net.minecraft.server.WorldLoader.InitConfig(
      new net.minecraft.server.WorldLoader.PackConfig(packs,
        new net.minecraft.world.level.WorldDataConfiguration(net.minecraft.world.level.DataPackConfig.DEFAULT, net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS), false, true),
      net.minecraft.commands.Commands.CommandSelection.DEDICATED, 4);
    var data = net.minecraft.server.WorldLoader.<Object, net.minecraft.server.WorldStem>load(init,
      context -> new net.minecraft.server.WorldLoader.DataLoadOutput<>(null, context.datapackDimensions()),
      (resources, reload, registries, unused) -> new net.minecraft.server.WorldStem(resources, reload, registries, null),
      net.minecraft.Util.backgroundExecutor(), Runnable::run).join();
    data.dataPackResources().updateStaticRegistryTags();
    registries = data.registries().compositeAccess();
  }

  static net.minecraft.core.RegistryAccess registries;

  static BlockState parse(String text) throws Exception {
    return net.minecraft.commands.arguments.blocks.BlockStateParser.parseForBlock(net.minecraft.core.registries.BuiltInRegistries.BLOCK, text, false).blockState();
  }

  static void run(List<String> block, PrintStream out) throws Exception {
    String name = block.get(0).substring(5);
    MovementLevel level = MovementLevel.make();
    MovementPlayer player = null;
    int tick = 0;
    out.println("case " + name);
    for (String line : block.subList(1, block.size())) {
      String[] p = line.split(" ");
      switch (p[0]) {
        case "set" -> level.put(new BlockPos(Integer.parseInt(p[1]), Integer.parseInt(p[2]), Integer.parseInt(p[3])), parse(p[4]));
        case "fill" -> {
          BlockState state = parse(p[7]);
          int x1 = Integer.parseInt(p[1]), y1 = Integer.parseInt(p[2]), z1 = Integer.parseInt(p[3]);
          int x2 = Integer.parseInt(p[4]), y2 = Integer.parseInt(p[5]), z2 = Integer.parseInt(p[6]);
          for (int z = z1; z <= z2; z++) for (int y = y1; y <= y2; y++) for (int x = x1; x <= x2; x++) level.put(new BlockPos(x, y, z), state);
        }
        case "editplayer" -> {
          // A block changes at an offset from the player's block, between two ticks.
          BlockPos at = player.player.blockPosition();
          level.put(at.offset(Integer.parseInt(p[1]), Integer.parseInt(p[2]), Integer.parseInt(p[3])), parse(p[4]));
        }
        case "edit" -> level.put(new BlockPos(Integer.parseInt(p[1]), Integer.parseInt(p[2]), Integer.parseInt(p[3])), parse(p[4]));
        case "boat" -> {
          // A boat standing still, which the player collides with.
          var boat = new net.minecraft.world.entity.vehicle.Boat(net.minecraft.world.entity.EntityType.OAK_BOAT, level, () -> net.minecraft.world.item.Items.OAK_BOAT);
          boat.setPos(Double.parseDouble(p[1]), Double.parseDouble(p[2]), Double.parseDouble(p[3]));
          level.entities.add(boat);
        }
        case "moving" -> {
          // A moving piston (block and block entity) at its progress 0.
          BlockPos at = new BlockPos(Integer.parseInt(p[1]), Integer.parseInt(p[2]), Integer.parseInt(p[3]));
          BlockState moving = parse(p[4]);
          level.put(at, moving);
          var piston = new net.minecraft.world.level.block.piston.PistonMovingBlockEntity(at, moving, parse(p[5]), net.minecraft.core.Direction.byName(p[6]), p[7].equals("1"), p[8].equals("1"));
          piston.setLevel(level);
          // A client's moving piston lingers 5 ticks at its end until the server's block arrives; here the
          // server is the engine, whose block is set at once.
          set(piston, net.minecraft.world.level.block.piston.PistonMovingBlockEntity.class, "deathTicks", 5);
          level.setBlockEntity(piston);
          level.pistons.add(piston);
        }
        case "player" -> {
          if (player != null) level.entities.remove(player.player);
          player = new MovementPlayer(level, Double.parseDouble(p[1]), Double.parseDouble(p[2]), Double.parseDouble(p[3]),
            Float.parseFloat(p[4]), Float.parseFloat(p[5]), p[6].equals("1"), p.length > 7 && p[7].equals("1"));
          tick = 0;
          level.entities.add(player.player);
          out.println(player.dump(-1));
        }
        case "tick" -> {
          // The client ticks its entities, then its block entities, then its time.
          player.tick(Integer.parseInt(p[1]), Float.parseFloat(p[2]), Float.parseFloat(p[3]));
          level.tickPistons();
          level.tickTime();
          out.println(player.dump(tick++));
        }
        case "end" -> { }
        default -> throw new IllegalArgumentException("bad line " + line);
      }
    }
    out.println("done");
  }
}
