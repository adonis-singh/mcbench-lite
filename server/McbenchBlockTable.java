import net.minecraft.core.Direction;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.phys.shapes.Shapes;
import net.minecraft.world.phys.shapes.VoxelShape;
import java.io.PrintWriter;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

// Writes what the viewers need to know about every block state of the
// pinned Minecraft, read from Minecraft itself: its default state, and how
// it emits, blocks and passes light. One line per state, in the order of
// Minecraft's state ids:
//   state  emission  lightBlock  fullFaces  faceShapes
// one per block:  default  id  state
// and last:  faceoccludes  n  bits
// fullFaces is a bit mask over Direction.ordinal(): down, up, north, south,
// west, east. faceShapes are the six face shapes (getFaceOcclusionShape, in that
// order) that the light engine tests light through, numbered among all of
// them, for a state that is conditionally full opaque (it can occlude and
// uses its shape for light); every other state is "0,0,0,0,0,0", shape 0
// being Shapes.empty(). faceoccludes holds Shapes.faceShapeOccludes of every
// ordered pair of the n face shapes, row by row: whether light passes
// between two blocks is that of the faces they turn to each other.
public class McbenchBlockTable {
  public static void main(String[] args) throws Exception {
    net.minecraft.SharedConstants.tryDetectVersion();
    net.minecraft.server.Bootstrap.bootStrap();
    Path target = Path.of(args[0]);
    Path partial = target.resolveSibling(target.getFileName() + ".part");
    Map<String, Integer> numbers = new LinkedHashMap<>();
    List<VoxelShape> shapes = new ArrayList<>();
    faceNumber(numbers, shapes, Shapes.empty());
    try (PrintWriter out = new PrintWriter(Files.newBufferedWriter(partial))) {
      for (Block block : BuiltInRegistries.BLOCK) {
        out.println("default\t" + BuiltInRegistries.BLOCK.getKey(block) + "\t" + name(block.defaultBlockState()));
        for (BlockState state : block.getStateDefinition().getPossibleStates()) {
          int full = 0;
          for (Direction direction : Direction.values()) {
            if (Shapes.faceShapeOccludes(Shapes.empty(), state.getFaceOcclusionShape(direction))) full |= 1 << direction.ordinal();
          }
          StringBuilder faces = new StringBuilder();
          for (Direction direction : Direction.values()) {
            int number = state.starlight$isConditionallyFullOpaque() ? faceNumber(numbers, shapes, state.getFaceOcclusionShape(direction)) : 0;
            faces.append(direction.ordinal() == 0 ? "" : ",").append(number);
          }
          out.println(name(state) + "\t" + state.getLightEmission() + "\t" + state.getLightBlock() + "\t" + full + "\t" + faces);
        }
      }
      StringBuilder bits = new StringBuilder();
      for (VoxelShape a : shapes) for (VoxelShape b : shapes) bits.append(Shapes.faceShapeOccludes(a, b) ? '1' : '0');
      out.println("faceoccludes\t" + shapes.size() + "\t" + bits);
    }
    Files.move(partial, target);
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
