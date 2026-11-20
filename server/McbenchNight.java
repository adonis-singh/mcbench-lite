import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.lang.classfile.ClassFile;
import java.lang.classfile.ClassModel;
import java.lang.classfile.MethodModel;
import java.lang.classfile.instruction.ConstantInstruction;
import java.lang.classfile.instruction.InvokeInstruction;
import java.lang.constant.ClassDesc;
import java.lang.constant.MethodTypeDesc;
import java.lang.invoke.MethodType;
import java.lang.reflect.AccessFlag;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.lang.reflect.Proxy;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.nio.file.FileSystem;
import java.nio.file.FileSystems;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HexFormat;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.TreeSet;
import java.util.function.BiFunction;
import java.util.stream.Stream;

// Minecraft's own night, asked of its client in a JVM where McbenchClient
// has started the client's classes, and written as night.json: what the
// client computes at the level's time 6000 (noon) and 18000 (midnight) of
// day 0, under its default options (Options as constructed: brightness 0.5,
// darkness pulsing 1.0, lightning flashes shown) with no effect on the player
// (no night vision, no darkness), no rain, no thunder and no lightning flash.
// The live view draws night from it (ui-seam-viewer-renderer-answers.md, Day
// and night).
//
// Each value is what Minecraft's own code returns or hands its GPU, run
// here; where the client hands a value to a GPU this program stands in for
// the GPU and keeps what it was handed:
//
// The lightmap. LightTexture.updateLightTexture computes the LightmapInfo
// uniform block (its fields named by lightmap.fsh) and draws the 16 x 16
// texture with the shader core/lightmap.fsh. The uniforms are the bytes
// updateLightTexture writes into the buffer it maps (Std140Builder), at each
// time. The texture's colours are lightmap.fsh evaluated here on those
// uniforms (lightmapTexel: the shader's statements in its order, in 32-bit
// floats, GLSL's mix as x * (1 - a) + y * a); the shader's text is the
// client jar's, held to the sha256 of the text this evaluation was written
// from, so a jar with another shader fails. The texel the GPU stores is the
// colour as an 8-bit unorm (round(c * 255)). Texel (block, sky): x is the
// block light, y the sky light (lightmap.fsh reads them from texCoord.x and
// .y); terrain.vsh samples it with the texture's LINEAR filter at
// (UV2 / 256 + 0.5 / 16), UV2 being LightTexture.pack's block << 4, sky <<
// 20 halves, so a whole level is a texel's centre and smooth lighting's
// fractional levels blend neighbouring texels. blockLightRedFlicker, which
// LightTexture.tick moves by Math.random() each tick, is its initial value
// 0 (the walk's mean): BlockFactor is 1.5 at both times.
//
// The sky and fog. For each biome the overworld has (the biomes of the
// multi-noise preset the jar's world preset minecraft:normal names for the
// overworld, MultiNoiseBiomeSourceParameterList.Preset.usedBiomes, each
// decoded from the jar's data pack by Biome.NETWORK_CODEC as the client
// receives it), with that biome everywhere (one biome, as the app's world):
// SkyRenderer.extractRenderState's SkyRenderState (ClientLevel.getSkyColor,
// getSunAngle, getTimeOfDay, getStarBrightness, getMoonPhase,
// DimensionSpecialEffects.getSunriseOrSunsetColor), and
// FogRenderer.setupFog's fog colour (computeFogColor, through
// AtmosphericFogEnvironment's AirBasedFogEnvironment.getBaseColor) with the
// Fog uniform block it writes (its distances, which core/sky.fsh fogs the sky
// disc with), at the options' render distance.
//
// The sky's draws. LevelRenderer's sky pass (its lambda addSkyPass$7, run on
// the plains' SkyRenderState) at each time: every render pass it opens, with
// its pipeline, the uniforms it binds (DynamicUniforms.writeTransform's
// matrices and colour, the Fog block), the textures it samples, its vertex
// buffer and draw ranges. The vertex buffers are SkyRenderer's constructor's,
// as it builds them (buildStars: 1,500 tries of a RandomSource of the seed in
// its bytecode, the stars' quads in the order BufferBuilder writes them;
// buildMoonPhases: the 8 phases' quads with their texture coordinates; the
// sky discs; the sun's quad). The model view stack is the identity: the
// matrices are relative to the camera's view rotation, which the drawing
// side supplies. The moon's and the sun's textures are named by their path
// in the client jar (SkyRenderer.MOON_LOCATION, SUN_LOCATION); no texture is
// copied.
//
// The level is a ClientLevel of this program's: Minecraft's
// ClientLevelData (flat, as the app's void world is a flat world) at the
// time, the overworld's DimensionType decoded from the jar's data pack
// (dimension_type/overworld.json) and its DimensionSpecialEffects
// (forType), a BiomeManager whose source answers the biome everywhere, and
// a light engine whose sky light is 15 (open sky) everywhere. The player
// is a LocalPlayer with no effects and nothing on its eyes, its eye at the
// camera; the camera a Camera at a place in open air (CAMERA, above the
// app's build) looking down at the build (CAMERA_YAW, CAMERA_PITCH), in no
// fluid (see camera()). The colours above depend on where it is only
// through the biome, which is the same everywhere, and through its height
// below the void's darkness onset (the world's lowest y plus
// ClientLevelData.voidDarknessOnsetRange, 1 for a flat world), which it is
// far above; its look turns the fog toward the sunrise colour only at dawn
// and dusk.
//
// What is not run as the client runs it: the GPU (GpuDevice and what it
// makes, stood in for as above), and LWJGL's memory functions, which
// BufferBuilder writes vertices through and which need LWJGL's natives:
// org.lwjgl.system.MemoryUtil and MemoryStack are defined here (Lwjgl) with
// the methods the client's classes name, over memory blocks of this
// program's at addresses of its own (direct buffers, as native memory is),
// byte for byte as LWJGL stores them (the platform's order); and
// org.lwjgl.glfw.GLFW as a class with no members (Lwjgl.write says why).
//
//   java McbenchNight client.jar mappings.txt blockTable <Minecraft's libraries> biome <night.json>
public class McbenchNight {
  static final String VERSION = "1.21.10";
  static final long[] TIMES = { 6000, 18000 };
  static final float PARTIAL_TICK = 1.0F;
  static final double[] CAMERA = { 0.5, 128.0, 0.5 };
  static final float CAMERA_YAW = 0.0F, CAMERA_PITCH = 30.0F;
  // The sha256 of core/lightmap.fsh as lightmapTexel transcribes it.
  static final String LIGHTMAP_SHADER = "assets/minecraft/shaders/core/lightmap.fsh";
  static final String LIGHTMAP_SHADER_SHA256 = "316b4d3658d97f3344f637e81ca2f7644c351b6ec954b8efa3f12f85d1048f1d";

  final McbenchClient client;
  final McbenchEntities mc;
  final Gpu gpu;
  final FileSystem jar;

  McbenchNight(McbenchClient client, McbenchEntities mc, Path clientJar) throws Exception {
    this.client = client;
    this.mc = mc;
    this.jar = FileSystems.newFileSystem(clientJar);
    this.gpu = new Gpu();
  }

  public static void main(String[] args) throws Exception {
    write(Path.of(args[0]), Path.of(args[1]), Path.of(args[2]), args[3], args[4], Path.of(args[5]));
    // Minecraft's entry point (McbenchClient) leaves threads behind.
    System.exit(0);
  }

  // night.json of the client jar, as main writes it. The stand-in LWJGL
  // classes are written into night-lwjgl/ in the working directory, which
  // the client's loader reads after Minecraft's libraries.
  static void write(Path clientJar, Path mappings, Path blockTable, String libraries, String biome, Path out) throws Exception {
    Path lwjgl = Path.of("night-lwjgl").toAbsolutePath();
    Lwjgl.write(clientJar, lwjgl);
    McbenchClient client = McbenchClient.read(clientJar, blockTable, libraries + java.io.File.pathSeparator + lwjgl, biome);
    Lwjgl.install(client.loader);
    McbenchEntities mc = new McbenchEntities(client, clientJar, mappings);
    JsonObject night = new McbenchNight(client, mc, clientJar).capture();
    String text = new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(night) + "\n";
    Files.writeString(out, text, StandardCharsets.UTF_8);
  }

  // ---- The world: the client's objects this program makes. ----

  Object game, options, player, dimensionType, effects;
  final Map<String, Object> biomes = new TreeMap<>();
  // Every render pass of the sky passes run.
  final List<Pass> skyPasses = new ArrayList<>();

  void setUp() throws Exception {
    // The GPU, before any class that asks RenderSystem for it, and this
    // thread as the render thread (RenderSystem.initRenderThread's one
    // assignment: its methods' signatures name GLFW, which reflection on
    // them would load).
    mc.field("RenderSystem", "renderThread").set(null, Thread.currentThread());
    mc.field("RenderSystem", "DEVICE").set(null, gpu.device());

    // Minecraft and its default options.
    Class<?> minecraft = mc.type("Minecraft");
    game = McbenchBlockEntities.allocate(minecraft);
    mc.field("Minecraft", "instance").set(null, game);
    Path folder = Files.createDirectories(Path.of("night-options").toAbsolutePath());
    if (Files.exists(folder.resolve("options.txt"))) throw new IllegalStateException(folder + " holds an options.txt; the options must be the defaults");
    options = mc.type("Options").getConstructor(minecraft, java.io.File.class).newInstance(game, folder.toFile());
    mc.set(game, "Minecraft", "options", options);
    Object target = mc.standIn(mc.type("RenderTarget"), Map.of(
      mc.method("RenderTarget", "getColorTextureView"), (a) -> gpu.view("main colour"),
      mc.method("RenderTarget", "getDepthTextureView"), (a) -> gpu.view("main depth")));
    mc.set(game, "Minecraft", "mainRenderTarget", target);

    // The overworld's dimension type and effects, from the jar's data pack.
    Object json = client.jsonParse.invoke(null, Files.readString(jar.getPath("data/minecraft/dimension_type/overworld.json")));
    dimensionType = client.decode(mc.field("DimensionType", "DIRECT_CODEC").get(null), json, "the overworld's dimension type");
    effects = mc.method("DimensionSpecialEffects", "forType", mc.type("DimensionType")).invoke(null, dimensionType);

    // The overworld's biomes.
    Object preset = JsonParser.parseString(Files.readString(jar.getPath("data/minecraft/worldgen/world_preset/normal.json")))
      .getAsJsonObject().getAsJsonObject("dimensions").getAsJsonObject("minecraft:overworld").getAsJsonObject("generator").getAsJsonObject("biome_source").get("preset").getAsString();
    String[] presetId = ((String) preset).split(":", 2);
    String presetName = JsonParser.parseString(Files.readString(jar.getPath("data/" + presetId[0] + "/worldgen/multi_noise_biome_source_parameter_list/" + presetId[1] + ".json")))
      .getAsJsonObject().get("preset").getAsString();
    Object found = null;
    for (var entry : ((Map<?, ?>) mc.field("MultiNoiseBiomeSourceParameterList.Preset", "BY_NAME").get(null)).entrySet()) {
      if (entry.getKey().toString().equals(presetName)) found = entry.getValue();
    }
    if (found == null) throw new IllegalStateException("No multi-noise preset " + presetName);
    Object biomeCodec = mc.field("Biome", "NETWORK_CODEC").get(null);
    Method location = mc.method("ResourceKey", "location");
    for (Object key : ((Stream<?>) mc.method("MultiNoiseBiomeSourceParameterList.Preset", "usedBiomes").invoke(found)).toList()) {
      String id = location.invoke(key).toString();
      String[] parts = id.split(":", 2);
      Object biomeJson = client.jsonParse.invoke(null, Files.readString(jar.getPath("data/" + parts[0] + "/worldgen/biome/" + parts[1] + ".json")));
      biomes.put(id, client.decode(biomeCodec, biomeJson, "the biome " + id));
    }

    // The player: no effects, nothing on its eyes.
    player = McbenchBlockEntities.allocate(mc.type("LocalPlayer"));
    mc.set(player, "LivingEntity", "activeEffects", new HashMap<>());
    mc.set(player, "Entity", "fluidOnEyes", new java.util.HashSet<>());
    // At the camera, its eye there (an allocated entity's eye height is 0).
    mc.set(player, "Entity", "position", mc.type("Vec3").getConstructor(double.class, double.class, double.class).newInstance(CAMERA[0], CAMERA[1], CAMERA[2]));
    mc.set(player, "Entity", "xo", CAMERA[0]);
    mc.set(player, "Entity", "yo", CAMERA[1]);
    mc.set(player, "Entity", "zo", CAMERA[2]);
    mc.set(game, "Minecraft", "player", player);
  }

  // A client level at `time` of day 0 whose every place is `biome`.
  Object level(long time, Object biome) throws Exception {
    Object level = McbenchBlockEntities.allocate(mc.type("ClientLevel"));
    // Any difficulty: none of the values read depends on it.
    Object difficulty = mc.type("Difficulty").getEnumConstants()[2];
    Object levelData = mc.type("ClientLevel.ClientLevelData").getConstructor(mc.type("Difficulty"), boolean.class, boolean.class).newInstance(difficulty, false, true);
    mc.method("ClientLevel.ClientLevelData", "setDayTime", long.class).invoke(levelData, time);
    mc.set(level, "ClientLevel", "clientLevelData", levelData);
    mc.set(level, "Level", "levelData", levelData);
    mc.set(level, "Level", "dimensionTypeRegistration", mc.method("Holder", "direct", Object.class).invoke(null, dimensionType));
    mc.set(level, "ClientLevel", "effects", effects);
    mc.set(level, "ClientLevel", "minecraft", game);
    Object holder = mc.method("Holder", "direct", Object.class).invoke(null, biome);
    Method getNoiseBiome = mc.method("BiomeManager.NoiseBiomeSource", "getNoiseBiome", int.class, int.class, int.class);
    Object source = Proxy.newProxyInstance(client.loader, new Class<?>[] { mc.type("BiomeManager.NoiseBiomeSource") }, (proxy, m, a) -> {
      if (McbenchEntities.asked(m, getNoiseBiome)) return holder;
      throw new UnsupportedOperationException("The biome source was asked " + m);
    });
    mc.set(level, "Level", "biomeManager", mc.type("BiomeManager").getConstructor(mc.type("BiomeManager.NoiseBiomeSource"), long.class).newInstance(source, 0L));
    // Its light engine, which AtmosphericFogEnvironment.setupFog asks the
    // sky light at the camera: 15, open sky, everywhere.
    Method getLightValue = mc.method("LayerLightEventListener", "getLightValue", client.blockPos);
    Object sky = Proxy.newProxyInstance(client.loader, new Class<?>[] { mc.type("LayerLightEventListener") }, (proxy, m, a) -> {
      if (McbenchEntities.asked(m, getLightValue)) return 15;
      if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, a);
      throw new UnsupportedOperationException("The fog asked the sky light " + m);
    });
    Object engine = mc.standIn(mc.type("LevelLightEngine"), Map.of(mc.method("LevelLightEngine", "getLayerListener", mc.type("LightLayer")), (a) -> {
      if (!((Enum<?>) a[1]).name().equals("SKY")) throw new UnsupportedOperationException("The fog asked the light engine for " + a[1]);
      return sky;
    }));
    Object chunks = McbenchBlockEntities.allocate(mc.type("ClientChunkCache"));
    mc.set(chunks, "ClientChunkCache", "lightEngine", engine);
    mc.set(level, "ClientLevel", "chunkSource", chunks);
    mc.set(game, "Minecraft", "level", level);
    return level;
  }

  // The camera: at CAMERA, turned to CAMERA_YAW and CAMERA_PITCH, its
  // entity the player. It is as Camera() makes it, not set up by
  // Camera.setup, so getFluidInCamera answers FogType.NONE: its answer for
  // a camera in open air (set up, it asks the level at the camera and at
  // its near plane's corners, which it sizes by the GLFW window, a class
  // that cannot load without LWJGL's natives).
  Object camera() throws Exception {
    Object camera = mc.type("Camera").getConstructor().newInstance();
    mc.set(camera, "Camera", "entity", player);
    mc.method("Camera", "setPosition", double.class, double.class, double.class).invoke(camera, CAMERA[0], CAMERA[1], CAMERA[2]);
    mc.method("Camera", "setRotation", float.class, float.class).invoke(camera, CAMERA_YAW, CAMERA_PITCH);
    if ((boolean) mc.get(camera, "Camera", "initialized")) throw new IllegalStateException("A new Camera is initialized");
    return camera;
  }

  // ---- The capture. ----

  JsonObject capture() throws Exception {
    setUp();
    JsonObject root = new JsonObject();
    root.addProperty("minecraft", VERSION);
    root.addProperty("made", "server/McbenchNight.java: Minecraft " + VERSION + "'s client code run at the level's time 6000 and 18000 of day 0, default options, no effects, no rain, no lightning");
    JsonObject sources = new JsonObject();
    root.add("sources", sources);
    JsonObject conditions = new JsonObject();
    conditions.add("times", array(TIMES));
    conditions.addProperty("partialTick", PARTIAL_TICK);
    conditions.addProperty("gamma", ((Number) option("gamma")).floatValue());
    conditions.addProperty("darknessEffectScale", ((Number) option("darknessEffectScale")).floatValue());
    conditions.addProperty("hideLightningFlash", (Boolean) option("hideLightningFlash"));
    int renderDistance = (int) mc.method("Options", "getEffectiveRenderDistance").invoke(options);
    conditions.addProperty("renderDistance", renderDistance);
    conditions.add("camera", array(CAMERA));
    conditions.addProperty("cameraYaw", CAMERA_YAW);
    conditions.addProperty("cameraPitch", CAMERA_PITCH);
    conditions.addProperty("flatWorld", true);
    conditions.addProperty("fov", ((Number) option("fov")).intValue());
    root.add("conditions", conditions);
    sources.addProperty("conditions", "Options(Minecraft, File) as constructed with no options.txt: gamma, darknessEffectScale, hideLightningFlash, getEffectiveRenderDistance; ClientLevel.ClientLevelData(difficulty, hardcore false, isFlat true).setDayTime");

    String plains = "minecraft:plains";
    if (!biomes.containsKey(plains)) throw new IllegalStateException("The overworld has no " + plains);
    sources.addProperty("biomes", "MultiNoiseBiomeSourceParameterList.Preset.usedBiomes of the preset data/minecraft/worldgen/world_preset/normal.json names for minecraft:overworld; each decoded by Biome.NETWORK_CODEC");

    // The lightmap at each time.
    JsonObject lightmaps = new JsonObject();
    for (long time : TIMES) lightmaps.add(Long.toString(time), lightmap(time));
    root.add("lightmap", lightmaps);
    sources.addProperty("lightmap.uniforms", "LightTexture.updateLightTexture: the LightmapInfo block it writes through Std140Builder (ClientLevel.getSkyDarken(1.0), DimensionType.ambientLight, blockLightRedFlicker, GameRenderer.getDarkenWorldAmount, Options.gamma)");
    sources.addProperty("lightmap.texels", LIGHTMAP_SHADER + " (sha256 " + LIGHTMAP_SHADER_SHA256 + ") evaluated on the uniforms by McbenchNight.lightmapTexel; texels[sky][block] as the shader's float colour and as the RGBA8 texel round(c * 255)");

    // The sky and fog by biome at each time.
    JsonObject sky = new JsonObject();
    Object fogRenderer = mc.type("FogRenderer").getConstructor().newInstance();
    for (var biome : biomes.entrySet()) {
      JsonObject byTime = new JsonObject();
      for (long time : TIMES) byTime.add(Long.toString(time), skyAndFog(time, biome.getValue(), fogRenderer, renderDistance));
      sky.add(biome.getKey(), byTime);
    }
    root.add("sky", sky);
    sources.addProperty("sky.state", "SkyRenderer.extractRenderState(level, partialTick, Camera.getPosition, state): SkyRenderState.skyColor (ClientLevel.getSkyColor), sunAngle (getSunAngle), timeOfDay (getTimeOfDay), rainBrightness, starBrightness (getStarBrightness), moonPhase (getMoonPhase), sunriseAndSunsetColor (DimensionSpecialEffects.getSunriseOrSunsetColor), isSunriseOrSunset, shouldRenderDarkDisc");
    sources.addProperty("sky.fog", "FogRenderer.setupFog(camera, renderDistance, false, deltaTracker, GameRenderer.getDarkenWorldAmount, level): the colour it returns (computeFogColor) and the Fog block it writes for FogMode.WORLD (getBuffer)");

    // The sky's draws in the plains (the app's world's biome), the star and
    // moon geometry, and the moon's texture.
    JsonObject draws = new JsonObject();
    Object skyRenderer = mc.type("SkyRenderer").getConstructor().newInstance();
    for (long time : TIMES) draws.add(Long.toString(time), skyPass(time, biomes.get(plains), skyRenderer, fogRenderer, renderDistance));
    root.add("skyDraws", draws);
    sources.addProperty("skyDraws", "LevelRenderer.lambda$addSkyPass$7 (the sky pass) on the SkyRenderState of minecraft:plains: each render pass's label, RenderPipeline.getLocation and its state (shaders, ShaderDefines, BlendFunction, depth test, writes, culling, VertexFormat.Mode), uniforms (DynamicUniforms.writeTransform's modelView, colorModulator, modelOffset, textureMatrix; Fog), samplers, vertex buffer (by the label it was created with) and draws");
    root.add("buffers", buffers());
    sources.addProperty("buffers", "the vertex buffers SkyRenderer() builds (buildStars, buildMoonPhases, buildSunQuad, buildSkyDisc) that skyDraws draws, each by the label GpuDevice.createBuffer was handed, decoded by the drawing RenderPipeline.getVertexFormat (its elements in order, at VertexFormat.getOffset)");

    JsonObject stars = new JsonObject();
    stars.addProperty("seed", starSeed());
    stars.addProperty("indexCount", (int) mc.get(skyRenderer, "SkyRenderer", "starIndexCount"));
    stars.addProperty("buffer", "Stars vertex buffer");
    root.add("stars", stars);
    sources.addProperty("stars", "SkyRenderer.buildStars: the seed its RandomSource.create is given (read from its bytecode), SkyRenderer.starIndexCount, its vertices in buffers[\"Stars vertex buffer\"] (VertexFormat.Mode.QUADS)");

    JsonObject moon = new JsonObject();
    Object moonLevel = level(18000, biomes.get(plains));
    moon.addProperty("time", 18000);
    moon.addProperty("day", 0);
    moon.addProperty("phase", (int) mc.method("LevelTimeAccess", "getMoonPhase").invoke(moonLevel));
    moon.addProperty("texture", jarPath(mc.field("SkyRenderer", "MOON_LOCATION").get(null)));
    moon.addProperty("sunTexture", jarPath(mc.field("SkyRenderer", "SUN_LOCATION").get(null)));
    moon.addProperty("buffer", "Moon phases");
    root.add("moon", moon);
    sources.addProperty("moon", "LevelTimeAccess.getMoonPhase (DimensionType.moonPhase of the day time) at 18000; SkyRenderer.MOON_LOCATION and SUN_LOCATION as paths in the client jar; the phases' quads in buffers[\"Moon phases\"] (buildMoonPhases), the one drawn in skyDraws[\"18000\"] (SkyRenderer.renderMoon)");
    return root;
  }

  Object option(String name) throws Exception {
    return mc.method("OptionInstance", "get").invoke(mc.method("Options", name).invoke(options));
  }

  // ---- The lightmap. ----

  JsonObject lightmap(long time) throws Exception {
    String shader = HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(Files.readAllBytes(jar.getPath(LIGHTMAP_SHADER))));
    if (!shader.equals(LIGHTMAP_SHADER_SHA256)) throw new IllegalStateException("The client jar's " + LIGHTMAP_SHADER + " (sha256 " + shader + ") is not the shader lightmapTexel transcribes");
    Object level = level(time, biomes.get("minecraft:plains"));
    Object renderer = McbenchBlockEntities.allocate(mc.type("GameRenderer"));
    Object texture = mc.type("LightTexture").getConstructor(mc.type("GameRenderer"), mc.type("Minecraft")).newInstance(renderer, game);
    mc.set(texture, "LightTexture", "updateLightTexture", true);
    gpu.passes.clear();
    mc.method("LightTexture", "updateLightTexture", float.class).invoke(texture, PARTIAL_TICK);
    if (gpu.passes.size() != 1) throw new IllegalStateException("updateLightTexture drew " + gpu.passes.size() + " passes, not one");
    Pass pass = gpu.passes.get(0);
    ByteBuffer info = pass.uniforms.get("LightmapInfo");
    if (info == null) throw new IllegalStateException("updateLightTexture bound no LightmapInfo");
    int size = (int) mc.field("LightTexture", "LIGHTMAP_UBO_SIZE").get(null);
    if (info.remaining() != size) throw new IllegalStateException("LightmapInfo is " + info.remaining() + " bytes, LightTexture.LIGHTMAP_UBO_SIZE " + size);
    // std140 of lightmap.fsh's block: seven floats, then two vec3s each at
    // a 16-byte boundary.
    ByteBuffer b = info.duplicate().order(ByteOrder.nativeOrder());
    int at = b.position();
    float[] u = new float[13];
    for (int i = 0; i < 7; i++) u[i] = b.getFloat(at + 4 * i);
    for (int i = 0; i < 3; i++) u[7 + i] = b.getFloat(at + 32 + 4 * i);
    for (int i = 0; i < 3; i++) u[10 + i] = b.getFloat(at + 48 + 4 * i);
    if (size != 64) throw new IllegalStateException("LightmapInfo's std140 size is 64 bytes, LightTexture's " + size);

    JsonObject out = new JsonObject();
    out.addProperty("pipeline", pass.pipeline);
    out.addProperty("skyDarken", (float) mc.method("ClientLevel", "getSkyDarken", float.class).invoke(level, 1.0F));
    JsonObject uniforms = new JsonObject();
    String[] names = { "AmbientLightFactor", "SkyFactor", "BlockFactor", "NightVisionFactor", "DarknessScale", "DarkenWorldFactor", "BrightnessFactor" };
    for (int i = 0; i < 7; i++) uniforms.addProperty(names[i], u[i]);
    uniforms.add("SkyLightColor", array(new float[] { u[7], u[8], u[9] }));
    uniforms.add("AmbientColor", array(new float[] { u[10], u[11], u[12] }));
    out.add("uniforms", uniforms);
    JsonArray floats = new JsonArray(), bytes = new JsonArray();
    for (int sky = 0; sky < 16; sky++) {
      JsonArray floatRow = new JsonArray(), byteRow = new JsonArray();
      for (int block = 0; block < 16; block++) {
        float[] c = lightmapTexel(u, block, sky);
        floatRow.add(array(c));
        byteRow.add(array(new int[] { unorm8(c[0]), unorm8(c[1]), unorm8(c[2]), 255 }));
      }
      floats.add(floatRow);
      bytes.add(byteRow);
    }
    out.add("texels", floats);
    out.add("rgba8", bytes);
    return out;
  }

  static int unorm8(float c) {
    return Math.round(c * 255.0F);
  }

  static float mix(float x, float y, float a) {
    return x * (1.0F - a) + y * a;
  }

  static float getBrightness(float level) {
    return level / (4.0F - 3.0F * level);
  }

  // core/lightmap.fsh's main at texel (block, sky), on its uniforms u:
  // AmbientLightFactor, SkyFactor, BlockFactor, NightVisionFactor,
  // DarknessScale, DarkenWorldFactor, BrightnessFactor, SkyLightColor (3),
  // AmbientColor (3). A fragment of the 16 x 16 target is at its texel's
  // centre, so floor(texCoord * 16) is the texel's index.
  static float[] lightmapTexel(float[] u, int block, int sky) {
    float ambientLightFactor = u[0], skyFactor = u[1], blockFactor = u[2], nightVisionFactor = u[3], darknessScale = u[4], darkenWorldFactor = u[5], brightnessFactor = u[6];
    float[] skyLightColor = { u[7], u[8], u[9] }, ambientColor = { u[10], u[11], u[12] };
    float blockBrightness = getBrightness((float) block / 15.0F) * blockFactor;
    float skyBrightness = getBrightness((float) sky / 15.0F) * skyFactor;
    float[] color = {
      blockBrightness,
      blockBrightness * ((blockBrightness * 0.6F + 0.4F) * 0.6F + 0.4F),
      blockBrightness * (blockBrightness * blockBrightness * 0.6F + 0.4F),
    };
    for (int i = 0; i < 3; i++) color[i] = mix(color[i], ambientColor[i], ambientLightFactor);
    for (int i = 0; i < 3; i++) color[i] += skyLightColor[i] * skyBrightness;
    for (int i = 0; i < 3; i++) color[i] = mix(color[i], 0.75F, 0.04F);
    if (ambientLightFactor == 0.0F) {
      float[] darkened = { color[0] * 0.7F, color[1] * 0.6F, color[2] * 0.6F };
      for (int i = 0; i < 3; i++) color[i] = mix(color[i], darkened[i], darkenWorldFactor);
    }
    if (nightVisionFactor > 0.0F) {
      float maxComponent = Math.max(color[0], Math.max(color[1], color[2]));
      if (maxComponent < 1.0F) {
        for (int i = 0; i < 3; i++) color[i] = mix(color[i], color[i] / maxComponent, nightVisionFactor);
      }
    }
    if (ambientLightFactor == 0.0F) {
      for (int i = 0; i < 3; i++) color[i] = color[i] - darknessScale;
    }
    for (int i = 0; i < 3; i++) color[i] = Math.min(Math.max(color[i], 0.0F), 1.0F);
    float maxComponent = Math.max(Math.max(color[0], color[1]), color[2]);
    float maxInverted = 1.0F - maxComponent;
    float maxScaled = 1.0F - maxInverted * maxInverted * maxInverted * maxInverted;
    float[] notGamma = new float[3];
    for (int i = 0; i < 3; i++) notGamma[i] = color[i] * (maxScaled / maxComponent);
    for (int i = 0; i < 3; i++) color[i] = mix(color[i], notGamma[i], brightnessFactor);
    for (int i = 0; i < 3; i++) color[i] = mix(color[i], 0.75F, 0.04F);
    return color;
  }

  // ---- The sky and the fog. ----

  // DeltaTracker.ONE: a frame a whole tick long, at partial tick 1.
  Object deltaTracker() throws Exception {
    Object one = mc.field("DeltaTracker", "ONE").get(null);
    if ((float) mc.method("DeltaTracker", "getGameTimeDeltaPartialTick", boolean.class).invoke(one, false) != PARTIAL_TICK) throw new IllegalStateException("DeltaTracker.ONE's partial tick is not " + PARTIAL_TICK);
    return one;
  }

  Object skyState(Object level, Object skyRenderer, Object camera) throws Exception {
    Object state = mc.type("SkyRenderState").getConstructor().newInstance();
    mc.method("SkyRenderer", "extractRenderState", mc.type("ClientLevel"), float.class, mc.type("Vec3"), mc.type("SkyRenderState"))
      .invoke(skyRenderer, level, PARTIAL_TICK, mc.method("Camera", "getPosition").invoke(camera), state);
    return state;
  }

  JsonObject skyAndFog(long time, Object biome, Object fogRenderer, int renderDistance) throws Exception {
    Object level = level(time, biome);
    Object camera = camera();
    JsonObject out = new JsonObject();
    Object state = skyState(level, McbenchBlockEntities.allocate(mc.type("SkyRenderer")), camera);
    JsonObject sky = new JsonObject();
    sky.addProperty("skyType", mc.get(state, "SkyRenderState", "skyType").toString());
    sky.addProperty("skyColor", argb((int) mc.get(state, "SkyRenderState", "skyColor")));
    for (String name : new String[] { "sunAngle", "timeOfDay", "rainBrightness", "starBrightness" }) sky.addProperty(name, (float) mc.get(state, "SkyRenderState", name));
    sky.addProperty("moonPhase", (int) mc.get(state, "SkyRenderState", "moonPhase"));
    sky.addProperty("sunriseAndSunsetColor", argb((int) mc.get(state, "SkyRenderState", "sunriseAndSunsetColor")));
    sky.addProperty("isSunriseOrSunset", (boolean) mc.get(state, "SkyRenderState", "isSunriseOrSunset"));
    sky.addProperty("shouldRenderDarkDisc", (boolean) mc.get(state, "SkyRenderState", "shouldRenderDarkDisc"));
    out.add("state", sky);
    Object renderer = McbenchBlockEntities.allocate(mc.type("GameRenderer"));
    float darken = (float) mc.method("GameRenderer", "getDarkenWorldAmount", float.class).invoke(renderer, PARTIAL_TICK);
    Object fog = mc.method("FogRenderer", "setupFog", mc.type("Camera"), int.class, boolean.class, mc.type("DeltaTracker"), float.class, mc.type("ClientLevel"))
      .invoke(fogRenderer, camera, renderDistance, false, deltaTracker(), darken, level);
    out.add("fogColor", vector4(fog));
    out.add("fog", fogBlock(gpu.bytes(mc.method("FogRenderer", "getBuffer", mc.type("FogRenderer.FogMode")).invoke(fogRenderer, mc.field("FogRenderer.FogMode", "WORLD").get(null)))));
    return out;
  }

  // The Fog block (fog.glsl's layout, std140) of these bytes.
  static JsonObject fogBlock(ByteBuffer b) {
    int at = b.position();
    JsonObject out = new JsonObject();
    out.add("FogColor", array(new float[] { b.getFloat(at), b.getFloat(at + 4), b.getFloat(at + 8), b.getFloat(at + 12) }));
    String[] names = { "FogEnvironmentalStart", "FogEnvironmentalEnd", "FogRenderDistanceStart", "FogRenderDistanceEnd", "FogSkyEnd", "FogCloudsEnd" };
    for (int i = 0; i < names.length; i++) out.addProperty(names[i], b.getFloat(at + 16 + 4 * i));
    return out;
  }

  // ---- The sky pass. ----

  JsonArray skyPass(long time, Object biome, Object skyRenderer, Object fogRenderer, int renderDistance) throws Exception {
    Object level = level(time, biome);
    Object camera = camera();
    Object state = skyState(level, skyRenderer, camera);
    Object renderer = McbenchBlockEntities.allocate(mc.type("GameRenderer"));
    float darken = (float) mc.method("GameRenderer", "getDarkenWorldAmount", float.class).invoke(renderer, PARTIAL_TICK);
    mc.method("FogRenderer", "setupFog", mc.type("Camera"), int.class, boolean.class, mc.type("DeltaTracker"), float.class, mc.type("ClientLevel"))
      .invoke(fogRenderer, camera, renderDistance, false, deltaTracker(), darken, level);
    Object fogSlice = mc.method("FogRenderer", "getBuffer", mc.type("FogRenderer.FogMode")).invoke(fogRenderer, mc.field("FogRenderer.FogMode", "WORLD").get(null));
    Object levelRenderer = McbenchBlockEntities.allocate(mc.type("LevelRenderer"));
    mc.set(levelRenderer, "LevelRenderer", "skyRenderer", skyRenderer);
    // The textures SkyRenderer.initTextures would ask the texture manager
    // for: views named by the texture's path in the jar.
    for (String[] texture : new String[][] { { "sunTexture", "SUN_LOCATION" }, { "moonTexture", "MOON_LOCATION" }, { "endSkyTexture", "END_SKY_LOCATION" }, { "endFlashTexture", "END_LIGHT_LOCATION" } }) {
      Object made = mc.standIn(mc.type("AbstractTexture"), Map.of(mc.method("AbstractTexture", "getTextureView"), (a) -> {
        try {
          return gpu.view(jarPath(mc.field("SkyRenderer", texture[1]).get(null)));
        } catch (Exception error) {
          throw new IllegalStateException(error);
        }
      }));
      mc.set(skyRenderer, "SkyRenderer", texture[0], made);
    }
    gpu.passes.clear();
    mc.method("LevelRenderer", "lambda$addSkyPass$7", mc.cls("com.mojang.blaze3d.buffers.GpuBufferSlice"), mc.type("SkyRenderState")).invoke(levelRenderer, fogSlice, state);
    JsonArray passes = new JsonArray();
    for (Pass pass : gpu.passes) passes.add(pass.json(this));
    skyPasses.addAll(gpu.passes);
    return passes;
  }

  // ---- The vertex buffers SkyRenderer built. ----

  // Every vertex buffer a sky pass drew, by the label it was made with,
  // decoded by the vertex format of the pipeline that drew it: each
  // vertex's elements in the format's order (VertexFormat.getElements, at
  // getOffset), floats as floats and bytes as unsigned integers.
  JsonObject buffers() throws Exception {
    Class<?> formatType = mc.cls("com.mojang.blaze3d.vertex.VertexFormat");
    Class<?> elementType = mc.cls("com.mojang.blaze3d.vertex.VertexFormatElement");
    Class<?> pipelineType = mc.cls("com.mojang.blaze3d.pipeline.RenderPipeline");
    Map<String, Object> formats = new TreeMap<>();
    for (Pass pass : skyPasses) {
      Object format = pipelineType.getMethod("getVertexFormat").invoke(pass.renderPipeline);
      Object known = formats.putIfAbsent(pass.vertexBuffer, format);
      if (known != null && !known.equals(format)) throw new IllegalStateException(pass.vertexBuffer + " is drawn in two vertex formats");
    }
    JsonObject out = new JsonObject();
    for (var drawn : formats.entrySet()) {
      Buffer buffer = null;
      for (Buffer made : gpu.created) {
        if (!made.label.equals(drawn.getKey())) continue;
        if (buffer != null) throw new IllegalStateException("Two vertex buffers labelled " + made.label);
        buffer = made;
      }
      Object format = drawn.getValue();
      int stride = (int) formatType.getMethod("getVertexSize").invoke(format);
      List<?> elements = (List<?>) formatType.getMethod("getElements").invoke(format);
      if (buffer.initialLength % stride != 0) throw new IllegalStateException(buffer.label + " holds " + buffer.initialLength + " bytes, not whole vertices of " + stride);
      JsonObject entry = new JsonObject();
      entry.addProperty("format", format.toString());
      JsonArray names = new JsonArray();
      for (Object element : elements) {
        JsonObject described = new JsonObject();
        described.addProperty("name", (String) formatType.getMethod("getElementName", elementType).invoke(format, element));
        described.addProperty("type", ((Enum<?>) elementType.getMethod("type").invoke(element)).name());
        described.addProperty("count", (int) elementType.getMethod("count").invoke(element));
        names.add(described);
      }
      entry.add("elements", names);
      entry.addProperty("vertexSize", stride);
      ByteBuffer b = buffer.bytes.duplicate().order(ByteOrder.nativeOrder());
      JsonArray vertices = new JsonArray();
      for (int at = 0; at < buffer.initialLength; at += stride) {
        JsonArray v = new JsonArray();
        for (Object element : elements) {
          int offset = at + (int) formatType.getMethod("getOffset", elementType).invoke(format, element);
          int count = (int) elementType.getMethod("count").invoke(element);
          String type = ((Enum<?>) elementType.getMethod("type").invoke(element)).name();
          for (int k = 0; k < count; k++) {
            switch (type) {
              case "FLOAT" -> v.add(b.getFloat(offset + 4 * k));
              case "UBYTE" -> v.add(Byte.toUnsignedInt(b.get(offset + k)));
              default -> throw new IllegalStateException(buffer.label + " has an element of type " + type + ", which this program does not decode");
            }
          }
        }
        vertices.add(v);
      }
      entry.add("vertices", vertices);
      out.add(buffer.label, entry);
    }
    return out;
  }

  // The seed SkyRenderer.buildStars gives RandomSource.create: the long
  // constant loaded just before that call in its bytecode.
  long starSeed() throws Exception {
    String owner = mc.type("SkyRenderer").getName().replace('.', '/');
    String method = mc.mappings.method(mc.mappings.className("SkyRenderer"), "buildStars");
    String random = mc.type("RandomSource").getName().replace('.', '/');
    String create = mc.mappings.method(mc.mappings.className("RandomSource"), "create", long.class);
    ClassModel model = ClassFile.of().parse(Files.readAllBytes(jar.getPath(owner + ".class")));
    Long seed = null, last = null;
    for (MethodModel m : model.methods()) {
      if (!m.methodName().stringValue().equals(method) || !m.methodTypeSymbol().parameterList().isEmpty()) continue;
      for (var element : m.code().orElseThrow().elementList()) {
        if (element instanceof ConstantInstruction constant && constant.constantValue() instanceof Long value) last = value;
        if (element instanceof InvokeInstruction invoke && invoke.owner().asInternalName().equals(random) && invoke.name().stringValue().equals(create)
          && invoke.typeSymbol().equals(MethodTypeDesc.ofDescriptor("(J)L" + random + ";"))) {
          if (seed != null || last == null) throw new IllegalStateException("SkyRenderer.buildStars does not create one RandomSource of a constant seed");
          seed = last;
        }
      }
    }
    if (seed == null) throw new IllegalStateException("SkyRenderer.buildStars creates no RandomSource");
    return seed;
  }

  // "minecraft:textures/environment/moon_phases.png" as the client jar's path.
  String jarPath(Object location) throws Exception {
    String[] id = location.toString().split(":", 2);
    String path = "assets/" + id[0] + "/" + id[1];
    if (!Files.exists(jar.getPath(path))) throw new IllegalStateException("The client jar has no " + path);
    return path;
  }

  // ---- JSON. ----

  static String argb(int color) {
    return String.format("%08x", color);
  }

  static JsonArray array(float[] values) {
    JsonArray out = new JsonArray();
    for (float value : values) out.add(value);
    return out;
  }

  static JsonArray array(double[] values) {
    JsonArray out = new JsonArray();
    for (double value : values) out.add(value);
    return out;
  }

  static JsonArray array(int[] values) {
    JsonArray out = new JsonArray();
    for (int value : values) out.add(value);
    return out;
  }

  static JsonArray array(long[] values) {
    JsonArray out = new JsonArray();
    for (long value : values) out.add(value);
    return out;
  }

  JsonArray vector4(Object vector) throws Exception {
    Class<?> type = vector.getClass();
    return array(new float[] { type.getField("x").getFloat(vector), type.getField("y").getFloat(vector), type.getField("z").getFloat(vector), type.getField("w").getFloat(vector) });
  }

  JsonArray vector3(Object vector) throws Exception {
    Class<?> type = vector.getClass();
    return array(new float[] { type.getField("x").getFloat(vector), type.getField("y").getFloat(vector), type.getField("z").getFloat(vector) });
  }

  // A JOML Matrix4fc's sixteen values, column by column (m00 m01 m02 m03
  // m10 ...: JOML's mCR, column C row R).
  JsonArray matrix(Object matrix) throws Exception {
    float[] values = new float[16];
    Class<?> type = Class.forName("org.joml.Matrix4fc", true, client.loader);
    for (int c = 0; c < 4; c++) for (int r = 0; r < 4; r++) values[c * 4 + r] = (float) type.getMethod("m" + c + r).invoke(matrix);
    return array(values);
  }

  // ---- The GPU, stood in for. ----

  // A buffer the client made: its label, its bytes (a copy of the data it
  // was made with, or zeros; direct, as a mapped GPU buffer is, which JOML's
  // writes into a buffer need), and the length it was made with.
  static final class Buffer {
    final String label;
    final ByteBuffer bytes;
    final int initialLength;
    final Object handle;

    Buffer(String label, ByteBuffer bytes, Object handle) {
      this.label = label;
      this.bytes = bytes;
      this.initialLength = bytes.capacity();
      this.handle = handle;
    }
  }

  // A render pass the client opened, and what it set and drew.
  static final class Pass {
    String label, pipeline, vertexBuffer, indexBuffer;
    Object renderPipeline;
    final Map<String, ByteBuffer> uniforms = new TreeMap<>();
    final Map<String, Object> transforms = new TreeMap<>();
    final Map<String, String> samplers = new TreeMap<>();
    final List<int[]> draws = new ArrayList<>();

    JsonObject json(McbenchNight night) throws Exception {
      JsonObject out = new JsonObject();
      out.addProperty("label", label);
      out.addProperty("pipeline", pipeline);
      out.add("pipelineState", night.pipelineJson(renderPipeline));
      if (vertexBuffer != null) out.addProperty("vertexBuffer", vertexBuffer);
      JsonObject u = new JsonObject();
      for (var entry : transforms.entrySet()) u.add(entry.getKey(), night.transformJson(entry.getValue()));
      for (var entry : uniforms.entrySet()) {
        if (entry.getKey().equals("Fog")) u.add("Fog", fogBlock(entry.getValue()));
        else if (!transforms.containsKey(entry.getKey())) u.addProperty(entry.getKey(), "bound");
      }
      out.add("uniforms", u);
      JsonObject s = new JsonObject();
      for (var entry : samplers.entrySet()) s.addProperty(entry.getKey(), entry.getValue());
      out.add("samplers", s);
      JsonArray d = new JsonArray();
      for (int[] draw : draws) {
        JsonObject one = new JsonObject();
        if (draw.length == 2) {
          one.addProperty("firstVertex", draw[0]);
          one.addProperty("vertexCount", draw[1]);
        } else {
          one.addProperty("baseVertex", draw[0]);
          one.addProperty("firstIndex", draw[1]);
          one.addProperty("indexCount", draw[2]);
          one.addProperty("instanceCount", draw[3]);
        }
        d.add(one);
      }
      out.add("draws", d);
      return out;
    }
  }

  // What a RenderPipeline draws with: its shaders, their defines, its
  // blending, depth test and writes, culling and vertex mode.
  JsonObject pipelineJson(Object pipeline) throws Exception {
    Class<?> type = mc.cls("com.mojang.blaze3d.pipeline.RenderPipeline");
    JsonObject out = new JsonObject();
    out.addProperty("vertexShader", type.getMethod("getVertexShader").invoke(pipeline).toString());
    out.addProperty("fragmentShader", type.getMethod("getFragmentShader").invoke(pipeline).toString());
    Object defines = type.getMethod("getShaderDefines").invoke(pipeline);
    JsonObject values = new JsonObject();
    for (var entry : new TreeMap<>((Map<?, ?>) mc.method("ShaderDefines", "values").invoke(defines)).entrySet()) values.addProperty(entry.getKey().toString(), entry.getValue().toString());
    out.add("defines", values);
    JsonArray flags = new JsonArray();
    for (Object flag : new TreeSet<>((java.util.Set<?>) mc.method("ShaderDefines", "flags").invoke(defines))) flags.add(flag.toString());
    out.add("flags", flags);
    var blend = (java.util.Optional<?>) type.getMethod("getBlendFunction").invoke(pipeline);
    if (blend.isPresent()) {
      Class<?> function = mc.cls("com.mojang.blaze3d.pipeline.BlendFunction");
      JsonObject b = new JsonObject();
      for (String part : new String[] { "sourceColor", "destColor", "sourceAlpha", "destAlpha" }) b.addProperty(part, function.getMethod(part).invoke(blend.get()).toString());
      out.add("blend", b);
    } else out.add("blend", com.google.gson.JsonNull.INSTANCE);
    out.addProperty("depthTest", type.getMethod("getDepthTestFunction").invoke(pipeline).toString());
    for (String flag : new String[] { "isWriteColor", "isWriteAlpha", "isWriteDepth", "isCull" }) out.addProperty(flag.substring(2, 3).toLowerCase() + flag.substring(3), (boolean) type.getMethod(flag).invoke(pipeline));
    out.addProperty("vertexFormatMode", type.getMethod("getVertexFormatMode").invoke(pipeline).toString());
    return out;
  }

  // DynamicUniforms.writeTransform's arguments, as the slice it returned
  // stands for.
  record Transform(Object modelView, Object colorModulator, Object modelOffset, Object textureMatrix, float lineWidth) {}

  JsonObject transformJson(Object value) throws Exception {
    Transform t = (Transform) value;
    JsonObject out = new JsonObject();
    out.add("ModelViewMat", matrix(t.modelView()));
    out.add("ColorModulator", vector4(t.colorModulator()));
    out.add("ModelOffset", vector3(t.modelOffset()));
    out.add("TextureMat", matrix(t.textureMatrix()));
    out.addProperty("LineWidth", t.lineWidth());
    return out;
  }

  final class Gpu {
    final List<Buffer> created = new ArrayList<>();
    final Map<Object, Buffer> byHandle = new IdentityHashMap<>();
    final List<Pass> passes = new ArrayList<>();
    final Map<Object, String> views = new IdentityHashMap<>();
    final Map<Object, Transform> transforms = new IdentityHashMap<>();
    Object device;

    Object device() throws Exception {
      if (device != null) return device;
      Class<?> type = mc.cls("com.mojang.blaze3d.systems.GpuDevice");
      device = Proxy.newProxyInstance(client.loader, new Class<?>[] { type }, (proxy, m, a) -> {
        switch (m.getName()) {
          case "createBuffer": {
            String label = (String) ((java.util.function.Supplier<?>) a[0]).get();
            ByteBuffer bytes;
            if (a[2] instanceof ByteBuffer data) {
              bytes = ByteBuffer.allocateDirect(data.remaining()).order(ByteOrder.nativeOrder());
              bytes.put(data.duplicate()).flip();
            } else bytes = ByteBuffer.allocateDirect((int) a[2]).order(ByteOrder.nativeOrder());
            return buffer(label, (int) a[1], bytes).handle;
          }
          case "createCommandEncoder": return encoder();
          case "getUniformOffsetAlignment": return 256;
          case "createTexture": return mc.standIn(mc.cls("com.mojang.blaze3d.textures.GpuTexture"), Map.of());
          case "createTextureView": return view("a texture the client made");
          default: throw new UnsupportedOperationException("The client asked the GPU " + m);
        }
      });
      // DynamicUniforms.writeTransform: kept, its slice standing for it.
      Class<?> dynamic = mc.type("DynamicUniforms");
      Object uniforms = mc.standIn(dynamic, Map.of(mc.method("DynamicUniforms", "writeTransform", mc.cls("org.joml.Matrix4fc"), mc.cls("org.joml.Vector4fc"), mc.cls("org.joml.Vector3fc"), mc.cls("org.joml.Matrix4fc"), float.class), (a) -> {
        try {
          Object copy4 = mc.cls("org.joml.Matrix4f").getConstructor(mc.cls("org.joml.Matrix4fc")).newInstance(a[1]);
          Object colour = mc.cls("org.joml.Vector4f").getConstructor(mc.cls("org.joml.Vector4fc")).newInstance(a[2]);
          Object offset = mc.cls("org.joml.Vector3f").getConstructor(mc.cls("org.joml.Vector3fc")).newInstance(a[3]);
          Object texture = mc.cls("org.joml.Matrix4f").getConstructor(mc.cls("org.joml.Matrix4fc")).newInstance(a[4]);
          Buffer buffer = buffer("DynamicTransforms", 0, ByteBuffer.allocateDirect(16));
          Object slice = slice(buffer.handle, 0, 16);
          transforms.put(slice, new Transform(copy4, colour, offset, texture, (float) a[5]));
          return slice;
        } catch (Exception error) {
          throw new IllegalStateException(error);
        }
      }));
      mc.field("RenderSystem", "dynamicUniforms").set(null, uniforms);
      return device;
    }

    Buffer buffer(String label, int usage, ByteBuffer bytes) throws Exception {
      Class<?> type = mc.cls("com.mojang.blaze3d.buffers.GpuBuffer");
      Object handle = mc.standIn(type, Map.of(type.getMethod("isClosed"), (a) -> false, type.getMethod("close"), (a) -> null));
      Field usageField = type.getDeclaredField("usage"), sizeField = type.getDeclaredField("size");
      usageField.setAccessible(true);
      sizeField.setAccessible(true);
      usageField.setInt(handle, usage);
      sizeField.setInt(handle, bytes.capacity());
      Buffer buffer = new Buffer(label, bytes, handle);
      created.add(buffer);
      byHandle.put(handle, buffer);
      return buffer;
    }

    Object slice(Object handle, int offset, int length) throws Exception {
      return mc.cls("com.mojang.blaze3d.buffers.GpuBufferSlice").getConstructor(mc.cls("com.mojang.blaze3d.buffers.GpuBuffer"), int.class, int.class).newInstance(handle, offset, length);
    }

    // The bytes a GpuBuffer or GpuBufferSlice holds, positioned at its start.
    ByteBuffer bytes(Object bufferOrSlice) throws Exception {
      Class<?> sliceType = mc.cls("com.mojang.blaze3d.buffers.GpuBufferSlice");
      if (sliceType.isInstance(bufferOrSlice)) {
        Buffer buffer = byHandle.get(sliceType.getMethod("buffer").invoke(bufferOrSlice));
        int offset = (int) sliceType.getMethod("offset").invoke(bufferOrSlice), length = (int) sliceType.getMethod("length").invoke(bufferOrSlice);
        ByteBuffer view = buffer.bytes.duplicate().order(ByteOrder.nativeOrder());
        view.position(offset).limit(offset + length);
        return view.slice().order(ByteOrder.nativeOrder());
      }
      return byHandle.get(bufferOrSlice).bytes.duplicate().order(ByteOrder.nativeOrder());
    }

    Object view(String name) {
      try {
        Object view = mc.standIn(mc.cls("com.mojang.blaze3d.textures.GpuTextureView"), Map.of());
        views.put(view, name);
        return view;
      } catch (Exception error) {
        throw new IllegalStateException(error);
      }
    }

    Object encoder() throws Exception {
      Class<?> type = mc.cls("com.mojang.blaze3d.systems.CommandEncoder");
      Class<?> mapped = mc.cls("com.mojang.blaze3d.buffers.GpuBuffer$MappedView");
      return Proxy.newProxyInstance(client.loader, new Class<?>[] { type }, (proxy, m, a) -> {
        switch (m.getName()) {
          case "mapBuffer": {
            ByteBuffer data = bytes(a[0]);
            return Proxy.newProxyInstance(client.loader, new Class<?>[] { mapped }, (p, n, b) -> switch (n.getName()) {
              case "data" -> data;
              case "close" -> null;
              default -> throw new UnsupportedOperationException("The client asked a mapped buffer " + n);
            });
          }
          case "writeToBuffer": {
            ByteBuffer target = bytes(a[0]);
            target.put(((ByteBuffer) a[1]).duplicate());
            return null;
          }
          case "createRenderPass": {
            Pass pass = new Pass();
            pass.label = (String) ((java.util.function.Supplier<?>) a[0]).get();
            passes.add(pass);
            return renderPass(pass);
          }
          case "clearColorTexture", "clearDepthTexture", "clearColorAndDepthTextures": return null;
          case "createFence": {
            // Every command has completed as it is given.
            Class<?> fence = mc.cls("com.mojang.blaze3d.buffers.GpuFence");
            return Proxy.newProxyInstance(client.loader, new Class<?>[] { fence }, (p, n, b) -> switch (n.getName()) {
              case "awaitCompletion" -> true;
              case "close" -> null;
              default -> throw new UnsupportedOperationException("The client asked a fence " + n);
            });
          }
          default: throw new UnsupportedOperationException("The client asked the command encoder " + m);
        }
      });
    }

    Object renderPass(Pass pass) throws Exception {
      Class<?> type = mc.cls("com.mojang.blaze3d.systems.RenderPass");
      Class<?> sliceType = mc.cls("com.mojang.blaze3d.buffers.GpuBufferSlice");
      Method location = mc.cls("com.mojang.blaze3d.pipeline.RenderPipeline").getMethod("getLocation");
      return Proxy.newProxyInstance(client.loader, new Class<?>[] { type }, (proxy, m, a) -> {
        switch (m.getName()) {
          case "setPipeline": pass.pipeline = location.invoke(a[0]).toString(); pass.renderPipeline = a[0]; return null;
          case "setUniform": {
            String name = (String) a[0];
            if (sliceType.isInstance(a[1]) && transforms.containsKey(a[1])) pass.transforms.put(name, transforms.get(a[1]));
            ByteBuffer bound = bytes(a[1]), copy = ByteBuffer.allocate(bound.remaining()).order(ByteOrder.nativeOrder());
            copy.put(bound.duplicate()).flip();
            pass.uniforms.put(name, copy);
            return null;
          }
          case "bindSampler": {
            String name = views.get(a[1]);
            if (name == null) throw new IllegalStateException("A render pass sampled a texture view this program did not make");
            pass.samplers.put((String) a[0], name);
            return null;
          }
          case "setVertexBuffer": pass.vertexBuffer = byHandle.get(a[1]).label; return null;
          case "setIndexBuffer": pass.indexBuffer = a[0] == null ? null : byHandle.containsKey(a[0]) ? byHandle.get(a[0]).label : "an index buffer"; return null;
          case "draw": pass.draws.add(new int[] { (int) a[0], (int) a[1] }); return null;
          case "drawIndexed": pass.draws.add(new int[] { (int) a[0], (int) a[1], (int) a[2], (int) a[3] }); return null;
          case "close", "pushDebugGroup", "popDebugGroup", "enableScissor", "disableScissor": return null;
          default: throw new UnsupportedOperationException("The client asked a render pass " + m);
        }
      });
    }
  }

  // ---- LWJGL's memory functions, stood in for. ----

  // org.lwjgl.system.MemoryUtil, its MemoryAllocator and MemoryStack, with
  // every method the client jar's classes name of them (each answering
  // through MemoryUtil.HANDLER, by its class's name, its name and
  // descriptor), over memory blocks of this program's: an address is a
  // block's start plus an offset, the blocks far apart; bytes are stored in
  // the platform's order, as LWJGL's natives store them. A method the client
  // names that this stand-in does not answer fails when called.
  static final class Lwjgl {
    static final String UTIL = "org/lwjgl/system/MemoryUtil", ALLOCATOR = "org/lwjgl/system/MemoryUtil$MemoryAllocator", STACK = "org/lwjgl/system/MemoryStack", GLFW = "org/lwjgl/glfw/GLFW";
    static final List<String> CLASSES = List.of(UTIL, ALLOCATOR, STACK);
    static final TreeMap<Long, ByteBuffer> blocks = new TreeMap<>();
    static final Map<ByteBuffer, Long> addresses = new IdentityHashMap<>();
    static long next = 1L << 40;

    static void write(Path clientJar, Path folder) throws Exception {
      Map<String, Map<String, Boolean>> named = new TreeMap<>();
      try (FileSystem jar = FileSystems.newFileSystem(clientJar); Stream<Path> files = Files.list(jar.getPath("/"))) {
        for (Path file : (Iterable<Path>) files::iterator) {
          if (!file.getFileName().toString().endsWith(".class")) continue;
          ClassModel model = ClassFile.of().parse(Files.readAllBytes(file));
          for (MethodModel method : model.methods()) {
            for (InvokeInstruction invoke : McbenchClient.invocations(method)) {
              String owner = invoke.owner().asInternalName();
              if (CLASSES.contains(owner)) named.computeIfAbsent(owner, (k) -> new TreeMap<>()).put(invoke.name().stringValue() + invoke.type().stringValue(), invoke.opcode() == java.lang.classfile.Opcode.INVOKESTATIC);
            }
          }
        }
      }
      Files.createDirectories(folder.resolve("org/lwjgl/system"));
      ClassDesc handler = ClassDesc.of("java.util.function.BiFunction"), object = ClassDesc.of("java.lang.Object");
      for (String owner : CLASSES) {
        boolean isInterface = owner.equals(ALLOCATOR);
        ClassDesc self = ClassDesc.ofInternalName(owner);
        byte[] bytes = ClassFile.of().build(self, (type) -> {
          type.withFlags(isInterface ? AccessFlag.PUBLIC.mask() | AccessFlag.INTERFACE.mask() | AccessFlag.ABSTRACT.mask() : AccessFlag.PUBLIC.mask());
          if (owner.equals(UTIL)) type.withField("HANDLER", handler, Modifier.PUBLIC | Modifier.STATIC);
          if (!isInterface) type.withMethodBody("<init>", MethodTypeDesc.ofDescriptor("()V"), AccessFlag.PUBLIC.mask(), (code) -> code.aload(0).invokespecial(ClassDesc.of("java.lang.Object"), "<init>", MethodTypeDesc.ofDescriptor("()V")).return_());
          named.getOrDefault(owner, Map.of()).forEach((method, isStatic) -> {
            int open = method.indexOf('(');
            String name = method.substring(0, open);
            MethodTypeDesc descriptor = MethodTypeDesc.ofDescriptor(method.substring(open));
            if (isInterface) {
              type.withMethod(name, descriptor, AccessFlag.PUBLIC.mask() | AccessFlag.ABSTRACT.mask(), (m) -> {});
              return;
            }
            // An instance method's arguments begin with the instance.
            int first = isStatic ? 0 : 1;
            type.withMethodBody(name, descriptor, AccessFlag.PUBLIC.mask() | (isStatic ? AccessFlag.STATIC.mask() : 0), (code) -> {
              code.getstatic(ClassDesc.ofInternalName(UTIL), "HANDLER", handler);
              code.loadConstant(owner.substring(owner.lastIndexOf('/') + 1) + "." + method);
              code.loadConstant(descriptor.parameterCount() + first);
              code.anewarray(object);
              if (!isStatic) {
                code.dup();
                code.loadConstant(0);
                code.aload(0);
                code.aastore();
              }
              int slot = first;
              for (int p = 0; p < descriptor.parameterCount(); p++) {
                code.dup();
                code.loadConstant(p + first);
                ClassDesc parameter = descriptor.parameterType(p);
                var kind = java.lang.classfile.TypeKind.from(parameter);
                code.loadLocal(kind, slot);
                slot += kind.slotSize();
                if (parameter.isPrimitive()) {
                  Class<?> box = MethodType.methodType(primitive(parameter)).wrap().returnType();
                  code.invokestatic(ClassDesc.of(box.getName()), "valueOf", MethodTypeDesc.of(ClassDesc.of(box.getName()), parameter));
                }
                code.aastore();
              }
              code.invokeinterface(handler, "apply", MethodTypeDesc.of(object, object, object));
              ClassDesc returned = descriptor.returnType();
              var kind = java.lang.classfile.TypeKind.from(returned);
              if (kind == java.lang.classfile.TypeKind.VOID) {
                code.pop();
                code.return_();
              } else if (returned.isPrimitive()) {
                Class<?> box = MethodType.methodType(primitive(returned)).wrap().returnType();
                ClassDesc boxDesc = ClassDesc.of(box.getName());
                code.checkcast(boxDesc);
                code.invokevirtual(boxDesc, primitive(returned).getName() + "Value", MethodTypeDesc.of(returned));
                code.return_(kind);
              } else {
                code.checkcast(returned);
                code.areturn();
              }
            });
          });
        });
        Files.write(folder.resolve(owner + ".class"), bytes);
      }
      // org.lwjgl.glfw.GLFW with no members: InputConstants's static
      // initializer looks up GLFW.glfwRawMouseMotionSupported and treats a
      // GLFW without it as one whose raw mouse motion is unsupported (the
      // KeyMappings Options makes need InputConstants). Any call of GLFW's
      // fails.
      Files.createDirectories(folder.resolve("org/lwjgl/glfw"));
      Files.write(folder.resolve(GLFW + ".class"), ClassFile.of().build(ClassDesc.ofInternalName(GLFW), (type) -> type.withFlags(AccessFlag.PUBLIC)));
    }

    static Class<?> primitive(ClassDesc desc) {
      return switch (desc.descriptorString()) {
        case "Z" -> boolean.class;
        case "B" -> byte.class;
        case "S" -> short.class;
        case "C" -> char.class;
        case "I" -> int.class;
        case "J" -> long.class;
        case "F" -> float.class;
        case "D" -> double.class;
        default -> throw new IllegalStateException("Not a primitive: " + desc);
      };
    }

    static void install(ClassLoader loader) throws Exception {
      Class<?> util = Class.forName(UTIL.replace('/', '.'), true, loader);
      Class<?> allocatorType = Class.forName(ALLOCATOR.replace('/', '.'), true, loader);
      Class<?> stack = Class.forName(STACK.replace('/', '.'), true, loader);
      if (util.getClassLoader() != loader) throw new IllegalStateException("MemoryUtil is not the stand-in");
      Object allocator = Proxy.newProxyInstance(loader, new Class<?>[] { allocatorType }, (proxy, m, a) -> switch (m.getName() + MethodType.methodType(m.getReturnType(), m.getParameterTypes()).toMethodDescriptorString()) {
        case "malloc(J)J" -> malloc((long) a[0]);
        case "calloc(JJ)J" -> malloc(Math.multiplyExact((long) a[0], (long) a[1]));
        case "realloc(JJ)J" -> realloc((long) a[0], (long) a[1]);
        case "free(J)V" -> {
          if ((long) a[0] != 0 && blocks.remove((long) a[0]) == null) throw new IllegalStateException("free of an address that was not allocated");
          yield null;
        }
        default -> throw new UnsupportedOperationException("LWJGL's allocator stand-in does not answer " + m);
      });
      BiFunction<Object, Object, Object> handler = (name, arguments) -> {
        Object[] a = (Object[]) arguments;
        return switch ((String) name) {
          case "MemoryUtil.getAllocator(Z)Lorg/lwjgl/system/MemoryUtil$MemoryAllocator;" -> allocator;
          // A stack's memory is a block of its own each time, never reused.
          case "MemoryStack.stackPush()Lorg/lwjgl/system/MemoryStack;", "MemoryStack.stackGet()Lorg/lwjgl/system/MemoryStack;" -> {
            try {
              yield stack.getConstructor().newInstance();
            } catch (ReflectiveOperationException error) {
              throw new IllegalStateException(error);
            }
          }
          case "MemoryStack.close()V" -> null;
          case "MemoryStack.malloc(I)Ljava/nio/ByteBuffer;", "MemoryStack.calloc(I)Ljava/nio/ByteBuffer;" -> view(malloc((int) a[1]), (int) a[1]);
          case "MemoryStack.mallocInt(I)Ljava/nio/IntBuffer;", "MemoryStack.callocInt(I)Ljava/nio/IntBuffer;" -> view(malloc(4L * (int) a[1]), 4 * (int) a[1]).asIntBuffer();
          case "MemoryUtil.memAlloc(I)Ljava/nio/ByteBuffer;" -> view(malloc((int) a[0]), (int) a[0]);
          case "MemoryUtil.memCalloc(I)Ljava/nio/ByteBuffer;" -> view(malloc((int) a[0]), (int) a[0]);
          case "MemoryUtil.memFree(Ljava/nio/Buffer;)V" -> {
            Long address = a[0] == null ? null : addresses.remove(a[0]);
            if (a[0] != null && (address == null || blocks.remove(address) == null)) throw new IllegalStateException("memFree of a buffer the stand-in did not allocate");
            yield null;
          }
          case "MemoryUtil.memByteBuffer(JI)Ljava/nio/ByteBuffer;" -> view((long) a[0], (int) a[1]);
          case "MemoryUtil.memAddress(Ljava/nio/ByteBuffer;)J" -> {
            Long address = addresses.get(a[0]);
            if (address == null) throw new IllegalStateException("memAddress of a buffer the stand-in did not make");
            yield address + ((ByteBuffer) a[0]).position();
          }
          case "MemoryUtil.memCopy(JJJ)V" -> {
            long from = (long) a[0], to = (long) a[1], length = (long) a[2];
            byte[] bytes = new byte[Math.toIntExact(length)];
            at(from, bytes.length).get(bytes);
            at(to, bytes.length).put(bytes);
            yield null;
          }
          case "MemoryUtil.memSet(JIJ)V" -> {
            ByteBuffer target = at((long) a[0], Math.toIntExact((long) a[2]));
            while (target.hasRemaining()) target.put((byte) (int) a[1]);
            yield null;
          }
          case "MemoryUtil.memPutByte(JB)V" -> { at((long) a[0], 1).put((byte) a[1]); yield null; }
          case "MemoryUtil.memPutShort(JS)V" -> { at((long) a[0], 2).putShort((short) a[1]); yield null; }
          case "MemoryUtil.memPutInt(JI)V" -> { at((long) a[0], 4).putInt((int) a[1]); yield null; }
          case "MemoryUtil.memPutLong(JJ)V" -> { at((long) a[0], 8).putLong((long) a[1]); yield null; }
          case "MemoryUtil.memPutFloat(JF)V" -> { at((long) a[0], 4).putFloat((float) a[1]); yield null; }
          case "MemoryUtil.memPutDouble(JD)V" -> { at((long) a[0], 8).putDouble((double) a[1]); yield null; }
          case "MemoryUtil.memGetByte(J)B" -> at((long) a[0], 1).get();
          case "MemoryUtil.memGetShort(J)S" -> at((long) a[0], 2).getShort();
          case "MemoryUtil.memGetInt(J)I" -> at((long) a[0], 4).getInt();
          case "MemoryUtil.memGetLong(J)J" -> at((long) a[0], 8).getLong();
          case "MemoryUtil.memGetFloat(J)F" -> at((long) a[0], 4).getFloat();
          case "MemoryUtil.memGetDouble(J)D" -> at((long) a[0], 8).getDouble();
          default -> throw new UnsupportedOperationException("LWJGL's MemoryUtil stand-in does not answer " + name);
        };
      };
      util.getField("HANDLER").set(null, handler);
    }

    static long malloc(long size) {
      long address = next;
      next += ((size >> 20) + 2) << 20;
      blocks.put(address, ByteBuffer.allocateDirect(Math.toIntExact(size)).order(ByteOrder.nativeOrder()));
      return address;
    }

    static long realloc(long address, long size) {
      long made = malloc(size);
      if (address != 0) {
        ByteBuffer old = blocks.remove(address);
        if (old == null) throw new IllegalStateException("realloc of an address that was not allocated");
        ByteBuffer copy = old.duplicate();
        copy.clear().limit(Math.toIntExact(Math.min(old.capacity(), size)));
        blocks.get(made).duplicate().put(copy);
      }
      return made;
    }

    // `length` bytes at `address`, as a buffer positioned there.
    static ByteBuffer at(long address, int length) {
      var block = blocks.floorEntry(address);
      if (block == null) throw new IllegalStateException("An address below every block");
      long offset = address - block.getKey();
      if (offset + length > block.getValue().capacity()) throw new IllegalStateException("An access past its block's end");
      ByteBuffer out = block.getValue().duplicate().order(ByteOrder.nativeOrder());
      out.position((int) offset).limit((int) offset + length);
      return out;
    }

    static ByteBuffer view(long address, int length) {
      ByteBuffer out = at(address, length).slice().order(ByteOrder.nativeOrder());
      addresses.put(out, address);
      return out;
    }
  }
}
