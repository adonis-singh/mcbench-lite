import java.nio.file.*;
import java.security.MessageDigest;
import java.util.HexFormat;
import java.util.jar.JarFile;
import org.objectweb.asm.*;
import org.objectweb.asm.tree.*;

/** Adds only a listener-count guard around Paper's otherwise unconditional event allocation. */
public final class PreparePhysicsGuard {
    public static void main(String[] args) throws Exception {
        byte[] original;
        try (JarFile jar = new JarFile(args[0])) {
            original = jar.getInputStream(jar.getJarEntry("net/minecraft/world/level/redstone/NeighborUpdater.class")).readAllBytes();
        }
        String originalHash = HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(original));
        // Only this inspected Paper 1.21.10 implementation is approved. A server
        // upgrade must be reviewed and parity-tested before extending this list.
        if (!originalHash.equals("e4dd069a7acc65dc89fe8e1e491d9563a19dc0737f9426db3335e287f2e47c5c")) {
            System.err.println("Unsupported NeighborUpdater; retain stock execution");
            System.exit(42);
        }
        ClassNode type = new ClassNode();
        new ClassReader(original).accept(type, ClassReader.EXPAND_FRAMES);
        int patched = 0;
        for (MethodNode method : type.methods) {
            if (!method.name.equals("executeUpdate") || Type.getArgumentTypes(method.desc).length != 7) continue;
            AbstractInsnNode first = method.instructions.getFirst();
            while (first != null && first.getOpcode() < 0) first = first.getNext();
            if (!(first instanceof TypeInsnNode allocation) || allocation.getOpcode() != Opcodes.NEW || !allocation.desc.equals("org/bukkit/event/block/BlockPhysicsEvent")) throw new IllegalStateException("Unrecognized physics event allocation");
            JumpInsnNode branch = null;
            int calls = 0;
            for (AbstractInsnNode insn : method.instructions) {
                if (insn instanceof MethodInsnNode call && call.owner.equals("org/bukkit/event/block/BlockPhysicsEvent") && call.name.equals("isCancelled")) {
                    if (!(call.getNext() instanceof JumpInsnNode jump) || jump.getOpcode() != Opcodes.IFEQ) throw new IllegalStateException("Unrecognized cancellation branch");
                    branch = jump; calls++;
                }
            }
            if (calls != 1) throw new IllegalStateException("Expected exactly one physics event dispatch");
            // Set the unused event local to null so existing stack-map frames remain valid.
            InsnList guard = new InsnList();
            guard.add(new InsnNode(Opcodes.ACONST_NULL));
            guard.add(new VarInsnNode(Opcodes.ASTORE, 7));
            guard.add(new MethodInsnNode(Opcodes.INVOKESTATIC, "org/bukkit/event/block/BlockPhysicsEvent", "getHandlerList", "()Lorg/bukkit/event/HandlerList;", false));
            guard.add(new MethodInsnNode(Opcodes.INVOKEVIRTUAL, "org/bukkit/event/HandlerList", "getRegisteredListeners", "()[Lorg/bukkit/plugin/RegisteredListener;", false));
            guard.add(new InsnNode(Opcodes.ARRAYLENGTH));
            guard.add(new JumpInsnNode(Opcodes.IFEQ, branch.label));
            method.instructions.insertBefore(first, guard);
            patched++;
        }
        if (patched != 1) throw new IllegalStateException("Unsupported NeighborUpdater implementation");
        ClassWriter writer = new ClassWriter(ClassWriter.COMPUTE_MAXS);
        type.accept(writer);
        Path output = Path.of(args[1]); Files.createDirectories(output);
        Files.write(output.resolve("NeighborUpdater.class"), writer.toByteArray());
        Files.writeString(output.resolve("original.sha256"), originalHash);
    }
}
