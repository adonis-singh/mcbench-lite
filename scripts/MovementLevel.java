import java.util.*;
import net.minecraft.client.multiplayer.ClientLevel;
import net.minecraft.core.BlockPos;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.shapes.VoxelShape;

// A client level (ClientLevel's own methods for everything the movement code
// reaches that does not read a block) holding the blocks of a case: every
// chunk exists, empty ones are air, as the engine's void world makes them on
// demand. It is made without its constructor, which needs a running client.
public class MovementLevel extends ClientLevel {
  MovementLevel() { super(null, null, null, null, 0, 0, null, false, 0L, 0); }

  Map<Long, BlockState> blocks;
  long gameTime;
  List<Entity> entities;
  Map<Long, net.minecraft.world.level.block.entity.BlockEntity> blockEntities;
  List<net.minecraft.world.level.block.piston.PistonMovingBlockEntity> pistons;

  static MovementLevel make() throws Exception {
    MovementLevel level = (MovementLevel) MovementReference.unsafe.allocateInstance(MovementLevel.class);
    level.blocks = new HashMap<>();
    level.entities = new ArrayList<>();
    level.blockEntities = new HashMap<>();
    level.pistons = new ArrayList<>();
    var base = net.minecraft.world.level.Level.class;
    MovementReference.set(level, base, "random", net.minecraft.util.RandomSource.create(0));
    MovementReference.set(level, base, "isClientSide", true);
    // The overworld's dimension type with the engine's build height.
    var access = net.minecraft.core.RegistryAccess.EMPTY;
    var type = new net.minecraft.world.level.dimension.DimensionType(java.util.OptionalLong.empty(), true, false, false, true, 1.0, true, false,
      -64, 1024, 1024, net.minecraft.tags.BlockTags.INFINIBURN_OVERWORLD, net.minecraft.world.level.dimension.BuiltinDimensionTypes.OVERWORLD_EFFECTS, 0.0f,
      java.util.Optional.of(192), new net.minecraft.world.level.dimension.DimensionType.MonsterSettings(false, true, net.minecraft.util.valueproviders.UniformInt.of(0, 7), 0));
    MovementReference.set(level, base, "dimensionTypeRegistration", net.minecraft.core.Holder.direct(type));
    MovementReference.set(level, ClientLevel.class, "worldBorder", new net.minecraft.world.level.border.WorldBorder());
    MovementReference.set(level, base, "blockEntityTickers", new ArrayList<>());
    MovementReference.set(level, ClientLevel.class, "clientLevelData", new ClientLevel.ClientLevelData(net.minecraft.world.Difficulty.PEACEFUL, false, true));
    MovementReference.set(level, base, "registryAccess", MovementReference.registries);
    MovementReference.set(level, base, "damageSources", new net.minecraft.world.damagesource.DamageSources(MovementReference.registries));
    return level;
  }

  void put(BlockPos pos, BlockState state) {
    if (state.isAir() && state.getBlock() == Blocks.AIR) blocks.remove(pos.asLong());
    else blocks.put(pos.asLong(), state);
  }

  @Override public BlockState getBlockState(BlockPos pos) {
    if (isOutsideBuildHeight(pos)) return Blocks.VOID_AIR.defaultBlockState();
    BlockState state = blocks.get(pos.asLong());
    return state == null ? Blocks.AIR.defaultBlockState() : state;
  }

  @Override public FluidState getFluidState(BlockPos pos) { return getBlockState(pos).getFluidState(); }
  @Override public net.minecraft.world.level.block.entity.BlockEntity getBlockEntity(BlockPos pos) { return blockEntities.get(pos.asLong()); }
  @Override public void setBlockEntity(net.minecraft.world.level.block.entity.BlockEntity entity) { blockEntities.put(entity.getBlockPos().asLong(), entity); }
  @Override public void removeBlockEntity(BlockPos pos) { blockEntities.remove(pos.asLong()); }
  // A block set by a piston finishing: the new state and nothing else (the neighbours are not updated).
  @Override public boolean setBlock(BlockPos pos, BlockState state, int flags) { put(pos.immutable(), state); return true; }
  @Override public boolean setBlock(BlockPos pos, BlockState state, int flags, int recursion) { put(pos.immutable(), state); return true; }
  @Override public void neighborChanged(BlockPos pos, net.minecraft.world.level.block.Block block, net.minecraft.world.level.redstone.Orientation orientation) { }
  @Override public void neighborChanged(BlockState state, BlockPos pos, net.minecraft.world.level.block.Block block, net.minecraft.world.level.redstone.Orientation orientation, boolean moved) { }

  // Level.tickBlockEntities for the moving pistons, in the order they were made.
  void tickPistons() {
    for (var piston : new ArrayList<>(pistons)) {
      if (!piston.isRemoved()) net.minecraft.world.level.block.piston.PistonMovingBlockEntity.tick(this, piston.getBlockPos(), getBlockState(piston.getBlockPos()), piston);
    }
  }
  @Override public net.minecraft.world.level.BlockGetter getChunkForCollisions(int x, int z) { return this; }
  @Override public boolean hasChunkAt(BlockPos pos) { return true; }
  @Override public boolean hasChunkAt(int x, int z) { return true; }
  @Override public boolean hasChunksAt(int x1, int z1, int x2, int z2) { return true; }
  @Override public boolean hasChunk(int x, int z) { return true; }
  @Override public boolean isClientSide() { return true; }
  @Override public net.minecraft.world.flag.FeatureFlagSet enabledFeatures() { return net.minecraft.world.flag.FeatureFlags.VANILLA_SET; }
  @Override public long getGameTime() { return gameTime; }
  // The client's end of a tick: the game time.
  void tickTime() { gameTime++; getLevelData().setGameTime(gameTime); }
  @Override public int getMinY() { return -64; }
  @Override public int getHeight() { return 1024; }
  @Override public net.minecraft.world.level.border.WorldBorder getWorldBorder() {
    try { return (net.minecraft.world.level.border.WorldBorder) MovementReference.get(this, ClientLevel.class, "worldBorder"); } catch (Exception e) { throw new RuntimeException(e); }
  }

  @Override public List<Entity> getEntities(Entity except, AABB box, java.util.function.Predicate<? super Entity> filter) {
    List<Entity> found = new ArrayList<>();
    for (Entity entity : entities) if (entity != except && entity.getBoundingBox().intersects(box) && filter.test(entity)) found.add(entity);
    return found;
  }

  @Override public <T extends Entity> List<T> getEntities(net.minecraft.world.level.entity.EntityTypeTest<Entity, T> test, AABB box, java.util.function.Predicate<? super T> filter) {
    return new ArrayList<>();
  }

  @Override public List<Entity> getPushableEntities(Entity entity, AABB box) {
    return new ArrayList<>();
  }

  @Override public void addParticle(net.minecraft.core.particles.ParticleOptions o, double a, double b, double c, double d, double e, double f) { }
  @Override public void addParticle(net.minecraft.core.particles.ParticleOptions o, boolean x, boolean y, double a, double b, double c, double d, double e, double f) { }
  @Override public void playLocalSound(double x, double y, double z, net.minecraft.sounds.SoundEvent s, net.minecraft.sounds.SoundSource c, float v, float p, boolean d) { }
  @Override public void playLocalSound(Entity e, net.minecraft.sounds.SoundEvent s, net.minecraft.sounds.SoundSource c, float v, float p) { }
  @Override public void playSeededSound(Entity e, double x, double y, double z, net.minecraft.core.Holder<net.minecraft.sounds.SoundEvent> s, net.minecraft.sounds.SoundSource c, float v, float p, long seed) { }
  @Override public void playSeededSound(Entity e, Entity f, net.minecraft.core.Holder<net.minecraft.sounds.SoundEvent> s, net.minecraft.sounds.SoundSource c, float v, float p, long seed) { }
  @Override public void gameEvent(net.minecraft.core.Holder<net.minecraft.world.level.gameevent.GameEvent> e, net.minecraft.world.phys.Vec3 p, net.minecraft.world.level.gameevent.GameEvent.Context c) { }
}
