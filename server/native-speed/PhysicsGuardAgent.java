import java.lang.instrument.*;
import java.nio.file.*;
import java.security.*;
import java.util.HexFormat;

/** Leaves the server jar untouched and declines unknown/remapped class bytes. */
public final class PhysicsGuardAgent {
    public static void premain(String directory, Instrumentation instrumentation) throws Exception {
        Path root = Path.of(directory);
        String expected = Files.readString(root.resolve("original.sha256")).trim();
        byte[] replacement = Files.readAllBytes(root.resolve("NeighborUpdater.class"));
        instrumentation.addTransformer(new ClassFileTransformer() {
            @Override public byte[] transform(ClassLoader loader, String name, Class<?> type, ProtectionDomain domain, byte[] bytes) {
                if (!name.equals("net/minecraft/world/level/redstone/NeighborUpdater")) return null;
                try {
                    String actual = HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes));
                    if (!actual.equals(expected)) {
                        System.err.println("MCBENCH_PHYSICS_GUARD_SKIPPED: class bytes differ; stock behavior retained");
                        return null;
                    }
                    System.setProperty("mcbench.physicsGuard", "listener-guard-v1");
                    System.err.println("MCBENCH_PHYSICS_GUARD_APPLIED");
                    return replacement;
                } catch (Exception error) { throw new IllegalStateException(error); }
            }
        });
    }
}
