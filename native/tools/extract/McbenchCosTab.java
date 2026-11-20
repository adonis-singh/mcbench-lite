import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.SplittableRandom;

// The release-time check of blocks.txt's `mthtab COS_TAB` line (README.md,
// COS_TAB): the bundle carries one Mth.COS_TAB for every platform, the one
// x86-64's Math.cos makes; an arm64 JVM's Math.cos is FdLibm's
// (StrictMath), which differs in some entries' last bits. The engine reads
// the table only in Mth.atan2, for a minecart's yaw, a float. This runs
// Minecraft's own Mth.atan2 with each table, the shipped one and
// StrictMath.cos of Mth's own ASIN_TAB, over a fixed sample of minecart
// movements per tick, and fails if any float yaw differs.
//
// The sample: SplittableRandom(20261117), 4,000,000 draws, in turn of four
// kinds: dx and dz uniform in [-0.5, 0.5); uniform in [-8, 8); a diagonal
// (|dx| = |dz|, uniform in [-0.5, 0.5)); thousandths in [-0.4, 0.4]; a draw
// is used when dx^2 + dz^2 > 0.001, as AbstractMinecart does (3,947,203).
//
//   java -cp <paper classpath>:<these classes> McbenchCosTab <blocks.txt>
public class McbenchCosTab {
  public static void main(String[] args) throws Exception {
    String line = Files.readAllLines(Path.of(args[0])).stream().filter((l) -> l.startsWith("mthtab\tCOS_TAB\t")).findFirst()
      .orElseThrow(() -> new IllegalStateException(args[0] + " has no mthtab COS_TAB line"));
    String[] words = line.split("\t")[2].split(",");
    Class<?> mth = Class.forName("net.minecraft.util.Mth");
    Field cosField = mth.getDeclaredField("COS_TAB");
    Field asinField = mth.getDeclaredField("ASIN_TAB");
    cosField.setAccessible(true);
    asinField.setAccessible(true);
    double[] cos = (double[]) cosField.get(null);
    double[] asin = (double[]) asinField.get(null);
    if (words.length != cos.length) throw new IllegalStateException("COS_TAB has " + words.length + " entries, Mth's " + cos.length);
    double[] shipped = new double[cos.length];
    double[] strict = new double[cos.length];
    int entries = 0;
    for (int i = 0; i < cos.length; i++) {
      shipped[i] = Double.longBitsToDouble(Long.parseUnsignedLong(words[i], 16));
      strict[i] = StrictMath.cos(asin[i]);
      if (Double.doubleToRawLongBits(shipped[i]) != Double.doubleToRawLongBits(strict[i])) entries++;
    }
    Method atan2 = mth.getDeclaredMethod("atan2", double.class, double.class);
    SplittableRandom random = new SplittableRandom(20261117);
    int draws = 4_000_000;
    double[] dx = new double[draws];
    double[] dz = new double[draws];
    for (int s = 0; s < draws; s++) {
      switch (s % 4) {
        case 0 -> { dx[s] = random.nextDouble(-0.5, 0.5); dz[s] = random.nextDouble(-0.5, 0.5); }
        case 1 -> { dx[s] = random.nextDouble(-8, 8); dz[s] = random.nextDouble(-8, 8); }
        case 2 -> { double v = random.nextDouble(-0.5, 0.5); dx[s] = v; dz[s] = random.nextBoolean() ? v : -v; }
        default -> { dx[s] = random.nextInt(-400, 401) / 1000.0; dz[s] = random.nextInt(-400, 401) / 1000.0; }
      }
    }
    double[] first = new double[draws];
    System.arraycopy(shipped, 0, cos, 0, cos.length);
    for (int s = 0; s < draws; s++) {
      if (dx[s] * dx[s] + dz[s] * dz[s] > 0.001) first[s] = (double) atan2.invoke(null, dz[s], dx[s]);
    }
    System.arraycopy(strict, 0, cos, 0, cos.length);
    long used = 0, doubles = 0, floats = 0;
    for (int s = 0; s < draws; s++) {
      if (!(dx[s] * dx[s] + dz[s] * dz[s] > 0.001)) continue;
      used++;
      double second = (double) atan2.invoke(null, dz[s], dx[s]);
      if (Double.doubleToRawLongBits(second) != Double.doubleToRawLongBits(first[s])) doubles++;
      if (Float.floatToRawIntBits((float) (second * 180.0 / Math.PI)) != Float.floatToRawIntBits((float) (first[s] * 180.0 / Math.PI))) floats++;
    }
    System.out.println("COS_TAB entries that differ between the shipped table and FdLibm's: " + entries + " of " + cos.length);
    System.out.println("minecart movements " + used + ": atan2 doubles that differ " + doubles + ", float yaws that differ " + floats);
    if (floats != 0) {
      System.out.println("FAILED: a minecart's yaw differs between the shipped COS_TAB and an arm64 JVM's");
      System.exit(1);
    }
  }
}
