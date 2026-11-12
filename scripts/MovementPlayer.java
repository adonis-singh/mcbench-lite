import java.lang.reflect.Proxy;
import net.minecraft.client.Minecraft;
import net.minecraft.client.OptionInstance;
import net.minecraft.client.Options;
import net.minecraft.client.multiplayer.ClientPacketListener;
import net.minecraft.client.multiplayer.MultiPlayerGameMode;
import net.minecraft.client.player.ClientInput;
import net.minecraft.client.player.LocalPlayer;
import net.minecraft.client.tutorial.Tutorial;
import net.minecraft.world.entity.player.Input;
import net.minecraft.world.level.GameType;
import net.minecraft.world.phys.Vec2;
import net.minecraft.world.phys.Vec3;

// The player of a case: a LocalPlayer made with its own constructors, in a
// creative game, whose input is what KeyboardInput would make of the keys
// held (the same arithmetic on the same Input record).
public class MovementPlayer {
  // KeyboardInput.tick, with the keys given instead of read from the options.
  static class Keys extends ClientInput {
    Input next = Input.EMPTY;

    static float impulse(boolean positive, boolean negative) {
      if (positive == negative) return 0.0F;
      return positive ? 1.0F : -1.0F;
    }

    @Override public void tick() {
      keyPresses = next;
      float forward = impulse(keyPresses.forward(), keyPresses.backward());
      float left = impulse(keyPresses.left(), keyPresses.right());
      moveVector = new Vec2(left, forward).normalized();
    }
  }

  final LocalPlayer player;
  final Keys keys = new Keys();
  final MovementLevel level;

  static <T> OptionInstance<T> option(T value) throws Exception {
    @SuppressWarnings("unchecked")
    OptionInstance<T> instance = (OptionInstance<T>) MovementReference.unsafe.allocateInstance(OptionInstance.class);
    MovementReference.set(instance, OptionInstance.class, "value", value);
    return instance;
  }

  MovementPlayer(MovementLevel level, double x, double y, double z, float yaw, float pitch, boolean flying, boolean autoJump) throws Exception {
    this.level = level;
    var unsafe = MovementReference.unsafe;
    Minecraft minecraft = (Minecraft) unsafe.allocateInstance(Minecraft.class);
    Options options = (Options) unsafe.allocateInstance(Options.class);
    MovementReference.set(options, Options.class, "sprintWindow", option(7));
    MovementReference.set(options, Options.class, "autoJump", option(autoJump));
    MovementReference.set(minecraft, Minecraft.class, "options", options);
    MultiPlayerGameMode mode = (MultiPlayerGameMode) unsafe.allocateInstance(MultiPlayerGameMode.class);
    MovementReference.set(mode, MultiPlayerGameMode.class, "localPlayerMode", GameType.CREATIVE);
    MovementReference.set(minecraft, Minecraft.class, "gameMode", mode);
    Tutorial tutorial = (Tutorial) unsafe.allocateInstance(Tutorial.class);
    Class<?> step = Class.forName("net.minecraft.client.tutorial.TutorialStepInstance");
    MovementReference.set(tutorial, Tutorial.class, "instance", Proxy.newProxyInstance(step.getClassLoader(), new Class<?>[] { step }, (proxy, method, args) -> null));
    MovementReference.set(minecraft, Minecraft.class, "tutorial", tutorial);
    MovementReference.set(minecraft, Minecraft.class, "soundManager", unsafe.allocateInstance(Sounds.class));
    ClientPacketListener connection = (ClientPacketListener) unsafe.allocateInstance(Connection.class);
    MovementReference.set(connection, ClientPacketListener.class, "localGameProfile", new com.mojang.authlib.GameProfile(java.util.UUID.nameUUIDFromBytes("player".getBytes()), "player"));
    player = new LocalPlayer(minecraft, level, connection, null, null, Input.EMPTY, false);
    MovementReference.set(minecraft, Minecraft.class, "cameraEntity", player);
    var info = unsafe.allocateInstance(net.minecraft.client.multiplayer.PlayerInfo.class);
    MovementReference.set(info, net.minecraft.client.multiplayer.PlayerInfo.class, "gameMode", GameType.CREATIVE);
    MovementReference.set(player, net.minecraft.client.player.AbstractClientPlayer.class, "playerInfo", info);
    player.input = keys;
    MovementReference.set(player, LocalPlayer.class, "ambientSoundHandlers", new java.util.ArrayList<>());
    player.setClientLoaded(true);
    GameType.CREATIVE.updatePlayerAbilities(player.getAbilities());
    player.getAbilities().flying = flying;
    player.setPos(x, y, z);
    player.setYRot(yaw);
    player.setXRot(pitch);
    player.setOldPosAndRot();
    player.setOnGround(false);
  }

  // A sound manager that plays nothing.
  static class Sounds extends net.minecraft.client.sounds.SoundManager {
    Sounds() { super(null, null); }
    @Override public net.minecraft.client.sounds.SoundEngine.PlayResult play(net.minecraft.client.resources.sounds.SoundInstance instance) { return null; }
    @Override public void playDelayed(net.minecraft.client.resources.sounds.SoundInstance instance, int ticks) { }
    @Override public void stop(net.minecraft.client.resources.sounds.SoundInstance instance) { }
  }

  // A connection that sends nothing.
  static class Connection extends ClientPacketListener {
    Connection() { super(null, null, null); }
    @Override public void send(net.minecraft.network.protocol.Packet<?> packet) { }
  }

  // ClientLevel.tickNonPassenger with the keys held and the look the mouse left.
  void tick(int bits, float yaw, float pitch) {
    keys.next = new Input((bits & 1) != 0, (bits & 2) != 0, (bits & 4) != 0, (bits & 8) != 0, (bits & 16) != 0, (bits & 32) != 0, (bits & 64) != 0);
    player.setYRot(yaw);
    player.setXRot(pitch);
    player.setOldPosAndRot();
    player.tickCount++;
    player.tick();
  }

  static String d(double v) { return Long.toHexString(Double.doubleToRawLongBits(v)); }
  static String f(float v) { return Integer.toHexString(Float.floatToRawIntBits(v)); }

  static Object read(Object target, Class<?> type, String name) {
    try { return MovementReference.get(target, type, name); } catch (Exception e) { throw new RuntimeException(e); }
  }

  // Everything the port holds that a tick changes, as exact bits.
  String dump(int tick) {
    Vec3 p = player.position(), v = player.getDeltaMovement();
    var living = net.minecraft.world.entity.LivingEntity.class;
    var entity = net.minecraft.world.entity.Entity.class;
    var local = LocalPlayer.class;
    var player_ = net.minecraft.world.entity.player.Player.class;
    @SuppressWarnings("unchecked")
    var fluids = (it.unimi.dsi.fastutil.objects.Object2DoubleMap<net.minecraft.tags.TagKey<net.minecraft.world.level.material.Fluid>>) read(player, entity, "fluidHeight");
    @SuppressWarnings("unchecked")
    var eyes = (java.util.Set<net.minecraft.tags.TagKey<net.minecraft.world.level.material.Fluid>>) read(player, entity, "fluidOnEyes");
    return "t" + tick + " p=" + d(p.x) + "," + d(p.y) + "," + d(p.z) + " v=" + d(v.x) + "," + d(v.y) + "," + d(v.z)
      + " og=" + (player.onGround() ? 1 : 0) + " fl=" + (player.getAbilities().flying ? 1 : 0) + " pose=" + player.getPose().name()
      + " sp=" + (player.isSprinting() ? 1 : 0) + " cr=" + (player.isCrouching() ? 1 : 0) + " hc=" + (player.horizontalCollision ? 1 : 0)
      + " vc=" + (player.verticalCollision ? 1 : 0) + " fd=" + d(player.fallDistance)
      + " in=" + f(player.xxa) + "," + f(player.zza) + " tr=" + read(player, player_, "jumpTriggerTime") + "," + read(player, local, "sprintTriggerTime") + "," + read(player, living, "noJumpDelay") + "," + read(player, local, "autoJumpTime")
      + " sw=" + (player.isSwimming() ? 1 : 0) + " wt=" + (player.isInWater() ? 1 : 0)
      + " fh=" + d(fluids.getDouble(net.minecraft.tags.FluidTags.WATER)) + "," + d(fluids.getDouble(net.minecraft.tags.FluidTags.LAVA))
      + " ey=" + (eyes.contains(net.minecraft.tags.FluidTags.WATER) ? 1 : 0) + (eyes.contains(net.minecraft.tags.FluidTags.LAVA) ? 1 : 0)
      + " sb=" + player.mainSupportingBlockPos.map(b -> b.getX() + ":" + b.getY() + ":" + b.getZ()).orElse("-")
      + " mc=" + (player.minorHorizontalCollision ? 1 : 0) + " vb=" + (player.verticalCollisionBelow ? 1 : 0)
      + " sm=" + d(((Vec3) read(player, entity, "stuckSpeedMultiplier")).x) + "," + d(((Vec3) read(player, entity, "stuckSpeedMultiplier")).y)
      + " ib=" + (read(player, entity, "inBlockState") == null ? "-" : String.valueOf(net.minecraft.world.level.block.Block.getId((net.minecraft.world.level.block.state.BlockState) read(player, entity, "inBlockState"))))
      + " dim=" + f(player.getBbWidth()) + "," + f(player.getBbHeight()) + "," + f(player.getEyeHeight());
  }
}
