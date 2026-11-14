import net.minecraft.core.Direction;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.RenderShape;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.phys.shapes.Shapes;
import net.minecraft.world.phys.shapes.VoxelShape;
import java.io.InputStream;
import java.io.PrintWriter;
import java.lang.classfile.ClassFile;
import java.lang.classfile.ClassModel;
import java.lang.classfile.CodeModel;
import java.lang.classfile.Instruction;
import java.lang.classfile.MethodModel;
import java.lang.classfile.instruction.FieldInstruction;
import java.lang.classfile.instruction.InvokeInstruction;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.stream.IntStream;

// Writes what the viewers need to know about every block state of the
// pinned Minecraft, read from Minecraft itself: its default state, how it
// emits, blocks and passes light, its fluid, whether it is drawn as a model,
// and which neighbours it hides its faces from. One line per state, in the
// order of Minecraft's state ids:
//   state  emission  lightBlock  fullFaces  faceShapes  fluid  model  skip
// one per block:  default  id  state
// and last:  faceoccludes  n  bits
//            skiprendering  rows  columns  row...
// fullFaces is a bit mask over Direction.ordinal(): down, up, north, south,
// west, east. faceShapes are the six face shapes (getFaceOcclusionShape, in that
// order) that the light engine tests light through, numbered among all of
// them, for a state that is conditionally full opaque (it can occlude and
// uses its shape for light); every other state is "0,0,0,0,0,0", shape 0
// being Shapes.empty(). faceoccludes holds Shapes.faceShapeOccludes of every
// ordered pair of the n face shapes, row by row: whether light passes
// between two blocks is that of the faces they turn to each other.
// fluid is the state id of the block of its fluid state
// (FluidState.createLegacyBlock: water[level=0] for a waterlogged stair,
// kelp or a bubble column; air, 0, for none): which fluid, its level and
// whether it is a source, as the fluid is drawn. model is 1 when the state's
// render shape is MODEL (the client draws its block model; a liquid, air,
// light or a barrier is drawn by nothing but its fluid, if any).
// skip is "row,column": the state's classes in BlockState.skipRendering,
// which hides a face of a state from a neighbour (glass beside glass, water
// beside water, a pane beside a pane it joins). The table holds
// skipRendering(neighbour, direction) of every ordered pair of states and
// every direction, Minecraft's answer for each, and no other answer: states
// are grouped by their answers (a row class: the same answers as the state
// skipping, against every state and direction; a column class: the same as
// the neighbour), class 0 never skipping or skipped against, and the row
// classes' answers are written on the skiprendering line, one field per row
// class: "first,mask,mask..." the bits over Direction.ordinal() of the
// column classes from `first` on (those past the last written never). The
// table is checked against the answers for every pair before it is written.
// A state is asked of only where its block overrides skipRendering: for the
// others the program proves from the pinned server's own bytecode, each time
// it runs, that the answer is false for every neighbour and direction (see
// Skips.neverSkips).
public class McbenchBlockTable {
  public static void main(String[] args) throws Exception {
    net.minecraft.SharedConstants.tryDetectVersion();
    net.minecraft.server.Bootstrap.bootStrap();
    Path target = Path.of(args[0]);
    Path partial = target.resolveSibling(target.getFileName() + ".part");
    Map<String, Integer> numbers = new LinkedHashMap<>();
    List<VoxelShape> shapes = new ArrayList<>();
    faceNumber(numbers, shapes, Shapes.empty());
    Skips skips = new Skips();
    try (PrintWriter out = new PrintWriter(Files.newBufferedWriter(partial))) {
      int id = 0;
      for (Block block : BuiltInRegistries.BLOCK) {
        out.println("default\t" + BuiltInRegistries.BLOCK.getKey(block) + "\t" + name(block.defaultBlockState()));
        for (BlockState state : block.getStateDefinition().getPossibleStates()) {
          if (Block.getId(state) != id++) throw new IllegalStateException(name(state) + " is not state " + (id - 1));
          int full = 0;
          for (Direction direction : Direction.values()) {
            if (Shapes.faceShapeOccludes(Shapes.empty(), state.getFaceOcclusionShape(direction))) full |= 1 << direction.ordinal();
          }
          StringBuilder faces = new StringBuilder();
          for (Direction direction : Direction.values()) {
            int number = state.starlight$isConditionallyFullOpaque() ? faceNumber(numbers, shapes, state.getFaceOcclusionShape(direction)) : 0;
            faces.append(direction.ordinal() == 0 ? "" : ",").append(number);
          }
          int fluid = Block.getId(state.getFluidState().createLegacyBlock());
          int model = state.getRenderShape() == RenderShape.MODEL ? 1 : 0;
          out.println(name(state) + "\t" + state.getLightEmission() + "\t" + state.getLightBlock() + "\t" + full + "\t" + faces
            + "\t" + fluid + "\t" + model + "\t" + skips.row[Block.getId(state)] + "," + skips.column[Block.getId(state)]);
        }
      }
      StringBuilder bits = new StringBuilder();
      for (VoxelShape a : shapes) for (VoxelShape b : shapes) bits.append(Shapes.faceShapeOccludes(a, b) ? '1' : '0');
      out.println("faceoccludes\t" + shapes.size() + "\t" + bits);
      out.println(skips.line());
    }
    Files.move(partial, target);
  }

  // BlockState.skipRendering of every ordered pair of states and every
  // direction, by class (see above).
  static final class Skips {
    final BlockState[] states;
    final int[] row;
    final int[] column;
    final List<int[]> masks = new ArrayList<>(); // by row class: the masks of column classes from first on
    final List<Integer> first = new ArrayList<>();

    Skips() throws Exception {
      List<BlockState> list = new ArrayList<>();
      Block.BLOCK_STATE_REGISTRY.forEach(list::add);
      states = list.toArray(BlockState[]::new);
      int n = states.length;
      Direction[] directions = Direction.values();
      // By state, the answers that are true, as neighbour * 6 + direction:
      // none for a state whose block never skips, every neighbour and
      // direction asked for the others.
      int[][] answers = new int[n][];
      Map<Class<?>, Boolean> never = new HashMap<>();
      for (int a = 0; a < n; a++) {
        // neverSkips reads the methods a BlockState runs.
        if (states[a].getClass() != BlockState.class) throw new IllegalStateException(name(states[a]) + " is a " + states[a].getClass());
        if (never.computeIfAbsent(states[a].getBlock().getClass(), Skips::neverSkips)) answers[a] = new int[0];
      }
      IntStream.range(0, n).filter((a) -> answers[a] == null).parallel().forEach((a) -> {
        BlockState state = states[a];
        int[] found = new int[0];
        int count = 0;
        for (int b = 0; b < n; b++) {
          BlockState neighbour = states[b];
          for (Direction direction : directions) {
            if (!state.skipRendering(neighbour, direction)) continue;
            if (count == found.length) found = Arrays.copyOf(found, Math.max(8, count * 2));
            found[count++] = b * 6 + direction.ordinal();
          }
        }
        answers[a] = Arrays.copyOf(found, count);
      });
      row = new int[n];
      List<int[]> rows = new ArrayList<>(List.of(new int[0]));
      Map<List<Integer>, Integer> rowOf = new HashMap<>(Map.of(List.of(), 0));
      for (int a = 0; a < n; a++) {
        int[] answer = answers[a];
        row[a] = rowOf.computeIfAbsent(Arrays.stream(answer).boxed().toList(), (key) -> {
          rows.add(answer);
          return rows.size() - 1;
        });
      }
      // By neighbour, the answers that are true, as row class * 6 + direction.
      List<List<Integer>> skippedBy = new ArrayList<>();
      for (int b = 0; b < n; b++) skippedBy.add(new ArrayList<>());
      for (int r = 0; r < rows.size(); r++) for (int answer : rows.get(r)) skippedBy.get(answer / 6).add(r * 6 + answer % 6);
      column = new int[n];
      List<List<Integer>> columns = new ArrayList<>(List.of(List.of()));
      Map<List<Integer>, Integer> columnOf = new HashMap<>(Map.of(List.of(), 0));
      for (int b = 0; b < n; b++) {
        List<Integer> answer = skippedBy.get(b);
        column[b] = columnOf.computeIfAbsent(answer, (key) -> {
          columns.add(answer);
          return columns.size() - 1;
        });
      }
      int[][] table = new int[rows.size()][columns.size()];
      for (int c = 0; c < columns.size(); c++) for (int answer : columns.get(c)) table[answer / 6][c] |= 1 << (answer % 6);
      for (int[] masks : table) {
        int lo = 0, hi = masks.length;
        while (lo < hi && masks[lo] == 0) lo++;
        while (hi > lo && masks[hi - 1] == 0) hi--;
        first.add(lo);
        this.masks.add(Arrays.copyOfRange(masks, lo, hi));
      }
      // The table against the answers, for every pair and direction: a
      // state's answer from the table is its row class's, so every state
      // holds the answers of its row class (the first state found with them),
      // and each row class's answers are the table's against every state.
      for (int a = 0; a < n; a++) {
        if (!Arrays.equals(answers[a], rows.get(row[a]))) throw new IllegalStateException(name(states[a]) + " is not in the row class of its answers");
      }
      int[] representative = new int[rows.size()];
      for (int a = n - 1; a >= 0; a--) representative[row[a]] = a;
      IntStream.range(0, rows.size()).parallel().forEach((r) -> {
        boolean[] held = new boolean[n * 6];
        for (int answer : rows.get(r)) held[answer] = true;
        for (int b = 0; b < n; b++) {
          for (int direction = 0; direction < 6; direction++) {
            if (skips(representative[r], b, direction) != held[b * 6 + direction]) {
              throw new IllegalStateException("The skipRendering table differs from Minecraft for " + name(states[representative[r]]) + " beside " + name(states[b]) + " to the " + directions[direction]);
            }
          }
        }
      });
    }

    // Whether every state of a block of `type` answers skipRendering false
    // for every neighbour and direction, proved from the server's bytecode:
    // BlockState.skipRendering(neighbour, direction) is BlockStateBase's,
    // which returns getBlock().skipRendering(asState(), neighbour, direction);
    // getBlock() is BlockStateBase's, which returns the state's block; and
    // the block's skipRendering(state, neighbour, direction), as `type`
    // dispatches it, is BlockBehaviour's, which returns false.
    static boolean neverSkips(Class<?> type) {
      try {
        Method own = dispatched(type, "skipRendering", BlockState.class, BlockState.class, Direction.class);
        if (own.getDeclaringClass() != net.minecraft.world.level.block.state.BlockBehaviour.class) return false;
        String base = "net/minecraft/world/level/block/state/BlockBehaviour$BlockStateBase";
        String state = "net/minecraft/world/level/block/state/BlockState";
        String block = "net/minecraft/world/level/block/Block";
        String direction = "net/minecraft/core/Direction";
        expect(dispatched(BlockState.class, "skipRendering", BlockState.class, Direction.class), base,
          "ALOAD_0", "INVOKEVIRTUAL " + base + ".getBlock()L" + block + ";", "ALOAD_0", "INVOKEVIRTUAL " + base + ".asState()L" + state + ";",
          "ALOAD_1", "ALOAD_2", "INVOKEVIRTUAL " + block + ".skipRendering(L" + state + ";L" + state + ";L" + direction + ";)Z", "IRETURN");
        expect(dispatched(BlockState.class, "getBlock"), base,
          "ALOAD_0", "GETFIELD net/minecraft/world/level/block/state/StateHolder.owner", "CHECKCAST", "ARETURN");
        expect(own, "net/minecraft/world/level/block/state/BlockBehaviour", "ICONST_0", "IRETURN");
        return true;
      } catch (Exception error) {
        throw new IllegalStateException(error);
      }
    }

    // The method a call on an instance of `type` runs: the first class up
    // from it that declares it.
    static Method dispatched(Class<?> type, String name, Class<?>... parameters) throws NoSuchMethodException {
      for (Class<?> at = type; at != null; at = at.getSuperclass()) {
        try {
          return at.getDeclaredMethod(name, parameters);
        } catch (NoSuchMethodException next) {
          // Declared further up.
        }
      }
      throw new NoSuchMethodException(type.getName() + "." + name);
    }

    // Fails unless `method`, declared by `owner`, is exactly `code`.
    static void expect(Method method, String owner, String... code) throws Exception {
      if (!method.getDeclaringClass().getName().replace('.', '/').equals(owner)) throw new IllegalStateException(method + " is not " + owner + "'s");
      ClassModel model;
      try (InputStream in = method.getDeclaringClass().getClassLoader().getResourceAsStream(owner + ".class")) {
        model = ClassFile.of().parse(in.readAllBytes());
      }
      String descriptor = java.lang.invoke.MethodType.methodType(method.getReturnType(), method.getParameterTypes()).toMethodDescriptorString();
      MethodModel found = model.methods().stream()
        .filter((each) -> each.methodName().equalsString(method.getName()) && each.methodType().equalsString(descriptor)).findFirst().orElseThrow();
      CodeModel body = found.code().orElseThrow();
      List<String> instructions = body.elementStream().filter((element) -> element instanceof Instruction).map((element) -> switch (element) {
        case InvokeInstruction invoke -> invoke.opcode() + " " + invoke.owner().asInternalName() + "." + invoke.name().stringValue() + invoke.type().stringValue();
        case FieldInstruction field -> field.opcode() + " " + field.owner().asInternalName() + "." + field.name().stringValue();
        default -> ((Instruction) element).opcode().toString();
      }).toList();
      if (!instructions.equals(List.of(code))) throw new IllegalStateException(owner + "." + method.getName() + " is " + instructions + ", not " + List.of(code));
    }

    boolean skips(int a, int b, int direction) {
      int r = row[a], at = column[b] - first.get(r);
      int[] masks = this.masks.get(r);
      return at >= 0 && at < masks.length && (masks[at] >> direction & 1) == 1;
    }

    String line() {
      StringBuilder line = new StringBuilder("skiprendering\t" + masks.size() + "\t" + (Arrays.stream(column).max().getAsInt() + 1));
      for (int r = 0; r < masks.size(); r++) {
        line.append('\t').append(first.get(r));
        for (int mask : masks.get(r)) line.append(',').append(mask);
      }
      return line.toString();
    }
  }

  // A face shape's number: shapes made of the same boxes are one, the first
  // met of them the one its pairs are tested with.
  static int faceNumber(Map<String, Integer> numbers, List<VoxelShape> shapes, VoxelShape shape) {
    return numbers.computeIfAbsent(shape.toAabbs().toString(), key -> {
      shapes.add(shape);
      return shapes.size() - 1;
    });
  }

  // "minecraft:oak_stairs[facing=east,half=bottom,...]", properties by name.
  static String name(BlockState state) {
    return state.toString().replaceFirst("^Block\\{([^}]*)\\}", "$1");
  }
}
