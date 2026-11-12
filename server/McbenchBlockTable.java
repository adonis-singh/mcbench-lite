import net.minecraft.core.Direction;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.phys.shapes.Shapes;
import net.minecraft.world.phys.shapes.VoxelShape;
import java.io.PrintWriter;
import java.nio.file.Files;
import java.nio.file.Path;

// Writes what the blueprint preview needs to know about every block state of
// the pinned Minecraft, read from Minecraft itself: its default state, and
// how it emits, blocks and passes light. One line per state:
//   state  emission  lightBlock  skylightDown  shapeOcclusion  fullFaces  emptyFaces
// and one per block:  default  id  state
// Faces are bit masks over Direction.ordinal(): down, up, north, south, west, east.
public class McbenchBlockTable {
  public static void main(String[] args) throws Exception {
    net.minecraft.SharedConstants.tryDetectVersion();
    net.minecraft.server.Bootstrap.bootStrap();
    Path target = Path.of(args[0]);
    Path partial = target.resolveSibling(target.getFileName() + ".part");
    try (PrintWriter out = new PrintWriter(Files.newBufferedWriter(partial))) {
      for (Block block : BuiltInRegistries.BLOCK) {
        out.println("default\t" + BuiltInRegistries.BLOCK.getKey(block) + "\t" + name(block.defaultBlockState()));
        for (BlockState state : block.getStateDefinition().getPossibleStates()) {
          int full = 0, empty = 0;
          for (Direction direction : Direction.values()) {
            VoxelShape face = state.getFaceOcclusionShape(direction);
            if (face.isEmpty()) empty |= 1 << direction.ordinal();
            else if (Shapes.faceShapeOccludes(Shapes.empty(), face)) full |= 1 << direction.ordinal();
          }
          out.println(name(state) + "\t" + state.getLightEmission() + "\t" + state.getLightBlock()
            + "\t" + (state.propagatesSkylightDown() ? 1 : 0) + "\t" + (state.useShapeForLightOcclusion() ? 1 : 0)
            + "\t" + full + "\t" + empty);
        }
      }
    }
    Files.move(partial, target);
  }

  // "minecraft:oak_stairs[facing=east,half=bottom,...]", properties by name.
  static String name(BlockState state) {
    return state.toString().replaceFirst("^Block\\{([^}]*)\\}", "$1");
  }
}
