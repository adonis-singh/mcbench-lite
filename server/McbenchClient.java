import com.google.gson.JsonElement;
import com.google.gson.JsonParser;
import java.awt.image.BufferedImage;
import java.lang.classfile.ClassFile;
import java.lang.classfile.ClassModel;
import java.lang.classfile.CodeElement;
import java.lang.classfile.MethodModel;
import java.lang.classfile.constantpool.StringEntry;
import java.lang.classfile.instruction.FieldInstruction;
import java.lang.classfile.instruction.InvokeInstruction;
import java.lang.constant.ClassDesc;
import java.lang.constant.MethodTypeDesc;
import java.lang.reflect.AccessFlag;
import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.lang.reflect.ParameterizedType;
import java.lang.reflect.Proxy;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.charset.StandardCharsets;
import java.nio.file.FileSystem;
import java.nio.file.FileSystems;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.TreeSet;
import java.util.stream.Stream;
import javax.imageio.ImageIO;

// What Minecraft's client says of each block state, asked of the client
// itself, from the client jar BlueMap downloads, run in the bake's JVM
// (McbenchBake starts it, its libraries Paper's: the server's, which the
// client shares). It writes two files, each a line per state in the order
// of Minecraft's state ids, each state named as the block table names it,
// line for line:
//
// The chunk render layer the client draws each block state and its fluid
// in: ItemBlockRenderTypes's getChunkRenderType(BlockState) and
// getRenderLayer(FluidState), which the client's section compiler draws
// every block and fluid by. One line per layer, in the order of its enum
// (ChunkSectionLayer), then one per state:
//   layer  name  blended
//   state  blockLayer  fluidLayer  renderer
// blended is whether the layer's render pipeline has a blend function: the
// layers drawn blended, after the others, sorted (translucent and tripwire).
// renderer is the block entity type whose block entity renderer the client
// draws the state through as well (wholly, for a state whose block model has
// no elements), or - for none.
// The layers are the client's under its default graphics (Fancy), under
// which leaves are drawn cut out: ItemBlockRenderTypes.setCutoutLeaves.
//
// The colours the client's BlockColors gives each state (0xRRGGBB, the
// colour its tinted faces are multiplied by; ffffff for none), in the
// world the app builds in, one biome everywhere (paper-server.js BIOME):
//   tints  indices
//   state  world  single  particle  faces
// world: for each tint index a block model uses (0 up to the highest
// "tintindex" of the jar's block models), comma separated, getColor(state, level, pos, index) with a
// level that answers every biome colour resolver with the biome's own
// colour at the position asked. That is what the client's level answers
// there: ClientLevel.calculateBlockTint averages the resolver over the
// blocks around (the biome blend radius), all of the one biome, and a
// falling block's MovingBlockRenderState asks the biome alone. The biome's
// colours depend on the position only through a grass colour modifier
// (swamps, dark forests), which plains have none of; the one position
// asked is the origin.
// single: getColor(state, null, null, 0), as BlockRenderDispatcher's
// renderSingleBlock tints every tinted face of a block drawn with no level
// (a minecart's block, TNT about to explode).
// particle: the colour TerrainParticle multiplies a block's breaking
// particles by, its rule read from its constructor: getColor(state, level,
// pos, 0), except for the block it tests the state against (grass blocks,
// whose particles are their dirt).
// faces: the tint indices the faces of its block's models carry (the models
// its blockstate file names, their parents resolved), comma separated, or -.
//
// The biome is decoded from the jar's data pack (data/<ns>/worldgen/biome)
// by the codec of Biome that reads every key its file holds, through
// RegistryOps over Minecraft's built-in registries (its sounds). Its grass,
// foliage and dry foliage colours come from the colour maps the client's
// reload listeners load (each the class that names its image,
// textures/colormap/<name>.png, and hands its pixels to a static init) and
// Minecraft's own lookup. Those listeners decode the image with
// NativeImage, which needs LWJGL's natives: here the image is decoded with
// ImageIO into the same ARGB pixels (NativeImage.makePixelArray's); the one
// piece not run as Minecraft runs it, checked by
// test/block-tints-minecraft.test.js against the jar's image.
//
//   McbenchClient.read(client.jar, blockTable, <Minecraft's libraries, a class path>, biome)
//
// The client jar's classes are obfuscated, so the program finds what it
// asks by its shape (each step below says how). Minecraft is bootstrapped
// by its own server entry point, which with --initSettings writes
// server.properties, eula.txt and logs/ in the working directory and stops.
// The block states are the client's state registry (Block.BLOCK_STATE_REGISTRY:
// the one public static iterable among the block classes'), in order, each
// named as the block table names it, line for line.
public class McbenchClient {
  final List<String> names;
  final List<String> layerLines = new ArrayList<>();
  final List<String> layerStates = new ArrayList<>();
  final int indices;
  final int[] world, single, particle;
  final String[] faces;
  final String[] renderers;
  final ClassLoader loader;
  // What McbenchBlockEntities runs Minecraft's block entity renderers with:
  // the client's classes, its block states in order, BlockEntityRenderers's
  // providers by block entity type, and each state's block entity type with
  // a renderer (null for none), and BlockEntityType's factory of a block
  // entity at a position.
  final List<ClassModel> classes;
  final List<Object> states;
  final Map<?, ?> providers;
  final Object[] stateTypes;
  final Method createBlockEntity;
  final Class<?> blockPos;
  final Object ops;
  // HolderLookup.Provider over every built-in registry (RegistryOps is made
  // over it), for McbenchEntities to read saved entities with.
  final Object lookup;
  // BuiltInRegistries's registries.
  final List<Object> registries;
  final Method parse, result, jsonParse;
  // The world's one biome and ColorResolver.getColor, which a level of it
  // answers BlockAndTintGetter.getBlockTint with (McbenchTerrain).
  final Object biome;
  final Method resolveColor;

  public static McbenchClient read(Path client, Path table, String libraries, String biomeId) throws Exception {
    return new McbenchClient(client, table, libraries, biomeId);
  }

  McbenchClient(Path client, Path table, String libraries, String biomeId) throws Exception {
    names = new ArrayList<>();
    for (String line : Files.readAllLines(table)) {
      String first = line.substring(0, line.indexOf('\t'));
      if (!first.equals("default") && !first.equals("facehides") && !first.equals("faceoccludes") && !first.equals("skiprendering")) names.add(first);
    }

    ClassDesc pipeline = ClassDesc.of("com.mojang.blaze3d.pipeline.RenderPipeline");
    classes = new ArrayList<>();
    // By block, the tint indices its models' faces carry (its blockstate
    // file's models, their parents resolved).
    Map<String, Set<Integer>> blockTints = new java.util.HashMap<>();
    String biomeFile;
    Map<String, int[]> colormaps = new java.util.HashMap<>();
    try (FileSystem jar = FileSystems.newFileSystem(client)) {
      try (Stream<Path> files = Files.list(jar.getPath("/"))) {
        for (Path file : (Iterable<Path>) files::iterator) {
          if (file.getFileName().toString().endsWith(".class")) classes.add(ClassFile.of().parse(Files.readAllBytes(file)));
        }
      }
      Map<String, Set<Integer>> modelTints = new java.util.HashMap<>();
      try (Stream<Path> files = Files.list(jar.getPath("assets/minecraft/blockstates"))) {
        for (Path file : (Iterable<Path>) files::iterator) {
          Set<Integer> used = new TreeSet<>();
          for (String model : models(JsonParser.parseString(Files.readString(file)))) used.addAll(modelTints(jar, model, modelTints));
          blockTints.put("minecraft:" + file.getFileName().toString().replaceFirst("\\.json$", ""), used);
        }
      }
      int colon = biomeId.indexOf(':');
      biomeFile = Files.readString(jar.getPath("data/" + biomeId.substring(0, colon) + "/worldgen/biome/" + biomeId.substring(colon + 1) + ".json"));
      for (ClassModel model : classes) {
        for (var entry : model.constantPool()) {
          if (entry instanceof StringEntry string && string.stringValue().matches("textures/colormap/[a-z_]+\\.png")) {
            try (var stream = Files.newInputStream(jar.getPath("assets/minecraft/" + string.stringValue()))) {
              BufferedImage image = ImageIO.read(stream);
              colormaps.put(model.thisClass().asInternalName(), image.getRGB(0, 0, image.getWidth(), image.getHeight(), null, 0, image.getWidth()));
            }
          }
        }
      }
    }
    indices = blockTints.values().stream().flatMap(Set::stream).max(Integer::compare).orElseThrow() + 1;
    faces = new String[names.size()];
    for (int id = 0; id < names.size(); id++) {
      Set<Integer> used = blockTints.get(names.get(id).replaceFirst("\\[.*", ""));
      faces[id] = used.isEmpty() ? "-" : String.join(",", used.stream().map(String::valueOf).toList());
    }

    ClassModel layerClass = only("the layer enum", classes.stream()
      .filter((model) -> model.superclass().map((s) -> s.asInternalName().equals("java/lang/Enum")).orElse(false))
      .filter((model) -> model.fields().stream().anyMatch((field) -> !field.flags().has(AccessFlag.STATIC) && field.fieldTypeSymbol().equals(pipeline)))
      .toList());
    ClassDesc layerDesc = layerClass.thisClass().asSymbol();
    ClassModel typesClass = only("ItemBlockRenderTypes", classes.stream()
      .filter((model) -> layerMethods(model, layerDesc).size() == 2 && model.methods().stream().filter((method) -> isPublicStatic(method)
        && method.methodTypeSymbol().equals(MethodTypeDesc.of(ClassDesc.ofDescriptor("V"), ClassDesc.ofDescriptor("Z")))).count() == 1)
      .toList());

    // The client's classes are signed, in the unnamed package this class is
    // in too: they get a loader of their own.
    List<URL> urls = new ArrayList<>(List.of(client.toUri().toURL()));
    for (String library : libraries.split(java.io.File.pathSeparator)) urls.add(Path.of(library).toUri().toURL());
    loader = new URLClassLoader(urls.toArray(URL[]::new), new IcuLinkage(classes));
    Thread.currentThread().setContextClassLoader(loader);
    Class.forName("net.minecraft.server.Main", true, loader).getMethod("main", String[].class).invoke(null, (Object) new String[] { "--initSettings" });

    Class<?> layer = load(layerClass);
    Class<?> types = load(typesClass);
    Method leaves = only("the leaves' setting", List.of(types.getMethods()).stream()
      .filter((method) -> Modifier.isStatic(method.getModifiers()) && method.getReturnType() == void.class && List.of(method.getParameterTypes()).equals(List.of(boolean.class))).toList());
    leaves.invoke(null, true);
    List<Method> byState = List.of(types.getMethods()).stream()
      .filter((method) -> Modifier.isStatic(method.getModifiers()) && method.getReturnType() == layer && method.getParameterCount() == 1).toList();

    // The block classes are the keys of ItemBlockRenderTypes's block map,
    // and the classes above them.
    Set<Class<?>> blockClasses = new HashSet<>();
    for (Field map : types.getDeclaredFields()) {
      if (!Modifier.isStatic(map.getModifiers()) || !Map.class.isAssignableFrom(map.getType())) continue;
      map.setAccessible(true);
      for (Object key : ((Map<?, ?>) map.get(null)).keySet()) {
        for (Class<?> type = key.getClass(); type != Object.class; type = type.getSuperclass()) blockClasses.add(type);
      }
    }
    Set<Object> registries = new HashSet<>();
    for (Class<?> type : blockClasses) {
      for (Field field : type.getDeclaredFields()) {
        if (Modifier.isStatic(field.getModifiers()) && Modifier.isPublic(field.getModifiers()) && Iterable.class.isAssignableFrom(field.getType())) registries.add(field.get(null));
      }
    }
    Iterable<?> registry = (Iterable<?>) only("the block state registry", registries.stream().filter((candidate) -> named((Iterable<?>) candidate, names)).toList());
    states = new ArrayList<>();
    registry.forEach(states::add);
    Class<?> blockState = states.get(0).getClass();
    Method blockLayer = only("getChunkRenderType", byState.stream().filter((method) -> method.getParameterTypes()[0].isAssignableFrom(blockState)).toList());
    Method fluidLayer = only("getRenderLayer", byState.stream().filter((method) -> method != blockLayer).toList());
    Class<?> fluidState = fluidLayer.getParameterTypes()[0];
    Method getFluidState = only("BlockState.getFluidState", List.of(blockState.getMethods()).stream()
      .filter((method) -> method.getParameterCount() == 0 && method.getReturnType() == fluidState && !Modifier.isStatic(method.getModifiers())).toList());
    Field pipelineField = only("the layer's pipeline", List.of(layer.getDeclaredFields()).stream().filter((field) -> field.getType().getName().equals(pipeline.packageName() + "." + pipeline.displayName())).toList());
    pipelineField.setAccessible(true);
    Method blendFunction = pipelineField.getType().getMethod("getBlendFunction");
    for (Object constant : layer.getEnumConstants()) {
      boolean blended = ((Optional<?>) blendFunction.invoke(pipelineField.get(constant))).isPresent();
      layerLines.add("layer\t" + ((Enum<?>) constant).name() + "\t" + (blended ? 1 : 0));
    }
    for (Object state : states) {
      Enum<?> own = (Enum<?>) blockLayer.invoke(null, state);
      Enum<?> fluid = (Enum<?>) fluidLayer.invoke(null, getFluidState.invoke(state));
      layerStates.add(own.ordinal() + "\t" + fluid.ordinal());
    }

    // BlockColors: the one class with a public static factory of itself
    // (createDefault) and a public getColor(BlockState, BlockAndTintGetter,
    // BlockPos, int).
    String stateDescriptor = blockState.descriptorString();
    ClassModel colorsClass = only("BlockColors", classes.stream().filter((model) -> {
      String self = model.thisClass().asSymbol().descriptorString();
      return model.methods().stream().anyMatch((method) -> isPublicStatic(method) && method.methodType().stringValue().equals("()" + self))
        && model.methods().stream().anyMatch((method) -> isPublic(method) && getColorShape(method, stateDescriptor));
    }).toList());
    MethodModel getColorModel = only("BlockColors.getColor", colorsClass.methods().stream().filter((method) -> isPublic(method) && getColorShape(method, stateDescriptor)).toList());
    Class<?> colorsType = load(colorsClass);
    Object colors = only("BlockColors.createDefault", List.of(colorsType.getMethods()).stream()
      .filter((method) -> Modifier.isStatic(method.getModifiers()) && method.getParameterCount() == 0 && method.getReturnType() == colorsType).toList()).invoke(null);
    Method getColor = colorsType.getMethod(getColorModel.methodName().stringValue(),
      getColorModel.methodTypeSymbol().parameterList().stream().map(this::type).toArray(Class<?>[]::new));
    Class<?> tintGetter = getColor.getParameterTypes()[1];
    blockPos = getColor.getParameterTypes()[2];
    // BlockAndTintGetter.getBlockTint(BlockPos, ColorResolver): its one
    // abstract method of a position and an interface; ColorResolver's one
    // method, getColor(Biome, x, z).
    Method blockTint = only("BlockAndTintGetter.getBlockTint", List.of(tintGetter.getMethods()).stream()
      .filter((method) -> Modifier.isAbstract(method.getModifiers()) && method.getReturnType() == int.class && method.getParameterCount() == 2
        && method.getParameterTypes()[0] == blockPos && method.getParameterTypes()[1].isInterface()).toList());
    Class<?> resolverType = blockTint.getParameterTypes()[1];
    Method resolve = only("ColorResolver.getColor", List.of(resolverType.getMethods()).stream().filter((method) -> Modifier.isAbstract(method.getModifiers())).toList());
    Class<?> biomeType = resolve.getParameterTypes()[0];

    // The colour maps, each handed to the static init its listener calls.
    for (var entry : colormaps.entrySet()) {
      ClassModel listener = classes.stream().filter((model) -> model.thisClass().asInternalName().equals(entry.getKey())).findFirst().orElseThrow();
      InvokeInstruction init = only("the colour map init " + entry.getKey() + " calls", invocations(listener).stream()
        .filter((invoke) -> invoke.opcode() == java.lang.classfile.Opcode.INVOKESTATIC && invoke.type().stringValue().equals("([I)V")).toList());
      Class.forName(init.owner().asInternalName().replace('/', '.'), true, loader).getMethod(init.name().stringValue(), int[].class).invoke(null, (Object) entry.getValue());
    }

    // RegistryOps over every built-in registry: RegistryOps.create(ops,
    // HolderLookup.Provider) (the one static method of a DynamicOps and
    // something else that makes its own class, whose second parameter has a
    // static create(Stream) of itself), over the public fields of
    // BuiltInRegistries (the class whose registry getColor reads states'
    // blocks' ids in).
    Class<?> dynamicOps = Class.forName("com.mojang.serialization.DynamicOps", true, loader);
    Object json = Class.forName("com.mojang.serialization.JsonOps", true, loader).getField("INSTANCE").get(null);
    FieldInstruction registryRead = only("the registry BlockColors.getColor reads", getColorModel.code().orElseThrow().elementStream()
      .filter((element) -> element instanceof FieldInstruction field && field.opcode() == java.lang.classfile.Opcode.GETSTATIC).map((element) -> (FieldInstruction) element).toList());
    Class<?> builtIn = Class.forName(registryRead.owner().asInternalName().replace('/', '.'), true, loader);
    List<Object> builtInRegistries = new ArrayList<>();
    for (Field field : builtIn.getFields()) if (Modifier.isStatic(field.getModifiers())) builtInRegistries.add(field.get(null));
    Method createOps = null;
    Object provider = null;
    for (ClassModel model : classes) {
      for (MethodModel method : model.methods()) {
        MethodTypeDesc type = method.methodTypeSymbol();
        if (!isPublicStatic(method) || type.parameterCount() != 2 || !type.returnType().equals(model.thisClass().asSymbol())
          || !type.parameterType(0).descriptorString().equals("Lcom/mojang/serialization/DynamicOps;")) continue;
        Class<?> lookup = type(type.parameterType(1));
        Optional<Method> create = List.of(lookup.getMethods()).stream().filter((candidate) -> Modifier.isStatic(candidate.getModifiers())
          && candidate.getReturnType() == lookup && List.of(candidate.getParameterTypes()).equals(List.of(Stream.class))).findFirst();
        if (create.isEmpty()) continue;
        if (createOps != null) throw new IllegalStateException("Found more than one RegistryOps.create in the client jar");
        provider = create.get().invoke(null, builtInRegistries.stream());
        createOps = load(model).getMethod(method.methodName().stringValue(), dynamicOps, lookup);
      }
    }
    if (createOps == null) throw new IllegalStateException("Found no RegistryOps.create in the client jar");
    ops = createOps.invoke(null, json, provider);
    lookup = provider;
    this.registries = builtInRegistries;
    Class<?> decoder = Class.forName("com.mojang.serialization.Decoder", true, loader);
    parse = decoder.getMethod("parse", dynamicOps, Object.class);
    result = Class.forName("com.mojang.serialization.DataResult", true, loader).getMethod("result");
    Method jsonParse = Class.forName("com.google.gson.JsonParser", true, loader).getMethod("parseString", String.class);

    // The biome, by Biome.NETWORK_CODEC, which the client receives the
    // server's biomes in: of Biome's own codecs of a Biome, the one its
    // static initializer does not build its holder codecs from (those are
    // built from the data pack's codec, DIRECT_CODEC, which also reads the
    // biome's generation, of registries no client has).
    Object biomeJson = jsonParse.invoke(null, biomeFile);
    ClassModel biomeModel = classes.stream().filter((model) -> model.thisClass().asSymbol().descriptorString().equals(biomeType.descriptorString())).findFirst().orElseThrow();
    Set<String> readInInit = new HashSet<>();
    for (MethodModel method : biomeModel.methods()) {
      if (!method.methodName().stringValue().equals("<clinit>")) continue;
      method.code().orElseThrow().elementStream().forEach((element) -> {
        if (element instanceof FieldInstruction field && field.opcode() == java.lang.classfile.Opcode.GETSTATIC
          && field.owner().asSymbol().descriptorString().equals(biomeType.descriptorString())) readInInit.add(field.name().stringValue());
      });
    }
    Field biomeCodec = only("Biome.NETWORK_CODEC", List.of(biomeType.getFields()).stream().filter((field) -> Modifier.isStatic(field.getModifiers())
      && field.getGenericType() instanceof ParameterizedType generic && generic.getActualTypeArguments()[0] == biomeType
      && !readInInit.contains(field.getName())).toList());
    Object biome = decode(biomeCodec.get(null), biomeJson, "the biome " + biomeId);
    this.biome = biome;
    this.resolveColor = resolve;

    // A level of the one biome: getBlockTint answers the resolver with the
    // biome's colour at the position; it is asked nothing else.
    Constructor<?> at = blockPos.getConstructor(int.class, int.class, int.class);
    // The position's x and z, by the coordinates' getters, told apart by
    // what they answer for a position made for it.
    Object probe = at.newInstance(12345, 678, -9012);
    Method x = coordinate(blockPos, probe, 12345), z = coordinate(blockPos, probe, -9012);
    Object level = Proxy.newProxyInstance(loader, new Class<?>[] { tintGetter }, (proxy, method, arguments) -> {
      if (!method.equals(blockTint)) throw new UnsupportedOperationException("BlockColors asked the level " + method);
      Object pos = arguments[0];
      return resolve.invoke(arguments[1], biome, (double) (int) x.invoke(pos), (double) (int) z.invoke(pos));
    });
    Object origin = at.newInstance(0, 0, 0);

    // TerrainParticle: the one class whose constructor calls getColor; the
    // block it skips is the Block field read just before the state is tested
    // against it (BlockState.is(Block)), its test that method.
    ClassModel particleClass = only("TerrainParticle", classes.stream().filter((model) -> model.methods().stream()
      .anyMatch((method) -> method.methodName().stringValue().equals("<init>") && invocations(method).stream().anyMatch((invoke) -> sameMethod(invoke, colorsClass, getColorModel)))).toList());
    MethodModel particleInit = only("TerrainParticle's constructor", particleClass.methods().stream()
      .filter((method) -> method.methodName().stringValue().equals("<init>") && invocations(method).stream().anyMatch((invoke) -> sameMethod(invoke, colorsClass, getColorModel))).toList());
    List<CodeElement> code = particleInit.code().orElseThrow().elementStream().filter((element) -> element instanceof FieldInstruction || element instanceof InvokeInstruction).toList();
    Object skipped = null;
    Method is = null;
    for (int i = 0; i + 1 < code.size(); i++) {
      if (code.get(i) instanceof FieldInstruction field && field.opcode() == java.lang.classfile.Opcode.GETSTATIC && code.get(i + 1) instanceof InvokeInstruction test
        && test.owner().asSymbol().descriptorString().equals(stateDescriptor) && test.type().stringValue().equals("(" + field.typeSymbol().descriptorString() + ")Z")) {
        if (skipped != null) throw new IllegalStateException("TerrainParticle tests the state against more than one block");
        Class<?> blocks = Class.forName(field.owner().asInternalName().replace('/', '.'), true, loader);
        skipped = blocks.getField(field.name().stringValue()).get(null);
        is = blockState.getMethod(test.name().stringValue(), type(field.typeSymbol()));
      }
    }
    if (skipped == null) throw new IllegalStateException("TerrainParticle tests the state against no block");

    world = new int[states.size() * indices];
    single = new int[states.size()];
    particle = new int[states.size()];
    for (int id = 0; id < states.size(); id++) {
      Object state = states.get(id);
      for (int index = 0; index < indices; index++) world[id * indices + index] = (int) getColor.invoke(colors, state, level, origin, index);
      single[id] = (int) getColor.invoke(colors, state, null, null, 0);
      particle[id] = (boolean) is.invoke(state, skipped) ? -1 : world[id * indices];
    }

    // The block entity types the client draws through a block entity
    // renderer: the keys of BlockEntityRenderers's map of providers (the one
    // class with a static Map whose initializer reads block entity types),
    // each a value of BuiltInRegistries.BLOCK_ENTITY_TYPE (the registry
    // whose values make a block entity of a position and a state) named by
    // the registry's getKey (its method of a value that names every value
    // "namespace:path"). A state is drawn through the renderer of the type
    // that is valid for it (BlockEntityType.isValid).
    Object entityRegistry = only("BuiltInRegistries.BLOCK_ENTITY_TYPE", builtInRegistries.stream().filter((candidate) -> {
      if (!(candidate instanceof Iterable<?> values) || !values.iterator().hasNext()) return false;
      return List.of(values.iterator().next().getClass().getMethods()).stream().anyMatch((method) -> !Modifier.isStatic(method.getModifiers())
        && List.of(method.getParameterTypes()).equals(List.of(blockPos, blockState)));
    }).toList());
    Class<?> entityType = ((Iterable<?>) entityRegistry).iterator().next().getClass();
    Method isValid = only("BlockEntityType.isValid", List.of(entityType.getMethods()).stream().filter((method) -> !Modifier.isStatic(method.getModifiers())
      && method.getReturnType() == boolean.class && List.of(method.getParameterTypes()).equals(List.of(blockState))).toList());
    Method getKey = only("Registry.getKey", List.of(entityRegistry.getClass().getMethods()).stream().filter((method) -> {
      if (Modifier.isStatic(method.getModifiers()) || method.getParameterCount() != 1 || method.getParameterTypes()[0] != Object.class || method.getReturnType().isPrimitive()) return false;
      for (Object value : (Iterable<?>) entityRegistry) {
        try {
          Object key = method.invoke(entityRegistry, value);
          if (key == null || !key.toString().matches("[a-z0-9_.-]+:[a-z0-9_./-]+")) return false;
        } catch (ReflectiveOperationException | RuntimeException error) {
          return false;
        }
      }
      return true;
    }).toList());
    String entityTypeDescriptor = entityType.descriptorString();
    ClassModel renderersClass = only("BlockEntityRenderers", classes.stream().filter((model) -> model.fields().stream()
      .anyMatch((field) -> field.flags().has(AccessFlag.STATIC) && field.fieldTypeSymbol().descriptorString().equals("Ljava/util/Map;"))
      && model.methods().stream().anyMatch((method) -> method.methodName().stringValue().equals("<clinit>") && method.code().orElseThrow().elementStream()
        .anyMatch((element) -> element instanceof FieldInstruction field && field.opcode() == java.lang.classfile.Opcode.GETSTATIC
          && field.typeSymbol().descriptorString().equals(entityTypeDescriptor)))).toList());
    Field providers = only("BlockEntityRenderers's providers", List.of(load(renderersClass).getDeclaredFields()).stream()
      .filter((field) -> Modifier.isStatic(field.getModifiers()) && field.getType() == Map.class).toList());
    providers.setAccessible(true);
    this.providers = (Map<?, ?>) providers.get(null);
    List<Object> drawn = new ArrayList<>(this.providers.keySet());
    createBlockEntity = only("BlockEntityType.create", List.of(entityType.getMethods()).stream().filter((method) -> !Modifier.isStatic(method.getModifiers())
      && List.of(method.getParameterTypes()).equals(List.of(blockPos, blockState))).toList());
    stateTypes = new Object[states.size()];
    renderers = new String[states.size()];
    for (int id = 0; id < states.size(); id++) {
      String found = "-";
      for (Object type : drawn) {
        if (!(boolean) isValid.invoke(type, states.get(id))) continue;
        if (!found.equals("-")) throw new IllegalStateException(names.get(id) + " is valid for more than one block entity type");
        found = getKey.invoke(entityRegistry, type).toString();
        stateTypes[id] = type;
      }
      renderers[id] = found;
    }

    this.jsonParse = jsonParse;
  }

  void writeLayers(Path target) throws Exception {
    var lines = new ArrayList<>(layerLines);
    for (int id = 0; id < names.size(); id++) lines.add(names.get(id) + "\t" + layerStates.get(id) + "\t" + renderers[id]);
    write(target, lines);
  }

  void writeTints(Path target) throws Exception {
    var lines = new ArrayList<String>();
    lines.add("tints\t" + indices);
    for (int id = 0; id < names.size(); id++) {
      var line = new StringBuilder(names.get(id)).append('\t');
      for (int index = 0; index < indices; index++) line.append(index > 0 ? "," : "").append(rgb(world[id * indices + index]));
      lines.add(line.append('\t').append(rgb(single[id])).append('\t').append(rgb(particle[id])).append('\t').append(faces[id]).toString());
    }
    write(target, lines);
  }

  static String rgb(int color) {
    return String.format("%06x", color & 0xffffff);
  }

  static void write(Path target, List<String> lines) throws Exception {
    Path partial = target.resolveSibling(target.getFileName() + ".part");
    Files.write(partial, (String.join("\n", lines) + "\n").getBytes(StandardCharsets.UTF_8));
    Files.move(partial, target);
  }

  Object decode(Object codec, Object input, String what) throws Exception {
    Object decoded = parse.invoke(codec, ops, input);
    return ((Optional<?>) result.invoke(decoded)).orElseThrow(() -> new IllegalStateException("Minecraft's codec could not read " + what + ": " + decoded));
  }

  Class<?> load(ClassModel model) throws ClassNotFoundException {
    return Class.forName(model.thisClass().asInternalName().replace('/', '.'), true, loader);
  }

  // The class a descriptor names (an object type, or int).
  Class<?> type(ClassDesc type) {
    if (type.descriptorString().equals("I")) return int.class;
    String descriptor = type.descriptorString();
    if (!descriptor.startsWith("L")) throw new IllegalStateException("Not a class or int: " + descriptor);
    try {
      return Class.forName(descriptor.substring(1, descriptor.length() - 1).replace('/', '.'), true, loader);
    } catch (ClassNotFoundException error) {
      throw new IllegalStateException(error);
    }
  }

  // The no-argument int method of `type` that answers `value` of `probe`.
  static Method coordinate(Class<?> type, Object probe, int value) throws Exception {
    List<Method> found = new ArrayList<>();
    for (Method method : type.getMethods()) {
      if (method.getParameterCount() == 0 && method.getReturnType() == int.class && !Modifier.isStatic(method.getModifiers()) && (int) method.invoke(probe) == value) found.add(method);
    }
    return only("the coordinate getter answering " + value, found);
  }

  static boolean getColorShape(MethodModel method, String state) {
    MethodTypeDesc type = method.methodTypeSymbol();
    return !method.flags().has(AccessFlag.STATIC) && type.returnType().descriptorString().equals("I") && type.parameterCount() == 4
      && type.parameterType(0).descriptorString().equals(state) && type.parameterType(3).descriptorString().equals("I");
  }

  static boolean sameMethod(InvokeInstruction invoke, ClassModel owner, MethodModel method) {
    return invoke.owner().asInternalName().equals(owner.thisClass().asInternalName()) && invoke.name().equals(method.methodName())
      && invoke.type().stringValue().equals(method.methodType().stringValue());
  }

  static List<InvokeInstruction> invocations(ClassModel model) {
    return model.methods().stream().flatMap((method) -> invocations(method).stream()).toList();
  }

  static List<InvokeInstruction> invocations(MethodModel method) {
    return method.code().map((code) -> code.elementStream().filter((element) -> element instanceof InvokeInstruction)
      .map((element) -> (InvokeInstruction) element).toList()).orElse(List.of());
  }

  // The models a blockstate file names (its variants' and multipart's).
  static List<String> models(JsonElement blockstate) {
    List<String> models = new ArrayList<>();
    var object = blockstate.getAsJsonObject();
    List<JsonElement> applied = new ArrayList<>();
    if (object.has("variants")) for (var variant : object.getAsJsonObject("variants").entrySet()) applied.add(variant.getValue());
    if (object.has("multipart")) for (JsonElement part : object.getAsJsonArray("multipart")) applied.add(part.getAsJsonObject().get("apply"));
    for (JsonElement apply : applied) {
      for (JsonElement variant : apply.isJsonArray() ? apply.getAsJsonArray() : List.of(apply)) models.add(variant.getAsJsonObject().get("model").getAsString());
    }
    return models;
  }

  // The tint indices of a model's faces: its own elements', or else its
  // parent's.
  static Set<Integer> modelTints(FileSystem jar, String model, Map<String, Set<Integer>> known) throws java.io.IOException {
    String name = model.replaceFirst("^minecraft:", "");
    Set<Integer> found = known.get(name);
    if (found != null) return found;
    var json = JsonParser.parseString(Files.readString(jar.getPath("assets/minecraft/models/" + name + ".json"))).getAsJsonObject();
    found = new TreeSet<>();
    if (json.has("elements")) {
      for (JsonElement element : json.getAsJsonArray("elements")) {
        for (var face : element.getAsJsonObject().getAsJsonObject("faces").entrySet()) {
          if (face.getValue().getAsJsonObject().has("tintindex")) found.add(face.getValue().getAsJsonObject().get("tintindex").getAsInt());
        }
      }
    } else if (json.has("parent") && !json.get("parent").getAsString().replaceFirst("^minecraft:", "").startsWith("builtin/")) {
      found = modelTints(jar, json.get("parent").getAsString(), known);
    }
    known.put(name, found);
    return found;
  }

  static boolean isPublic(MethodModel method) {
    return method.flags().has(AccessFlag.PUBLIC);
  }

  static boolean isPublicStatic(MethodModel method) {
    return method.flags().has(AccessFlag.PUBLIC) && method.flags().has(AccessFlag.STATIC);
  }

  static List<MethodModel> layerMethods(ClassModel model, ClassDesc layer) {
    return model.methods().stream().filter((method) -> isPublicStatic(method)
      && method.methodTypeSymbol().returnType().equals(layer) && method.methodTypeSymbol().parameterCount() == 1).toList();
  }

  // Whether `states` are the block table's states, by name, in order.
  static boolean named(Iterable<?> states, List<String> names) {
    int index = 0;
    for (Object state : states) {
      if (index >= names.size() || !name(state).equals(names.get(index))) return false;
      index++;
    }
    return index == names.size();
  }

  // As McbenchBlockTable names a state.
  static String name(Object state) {
    return state.toString().replaceFirst("^Block\\{([^}]*)\\}", "$1");
  }

  // The client's Font catches com.ibm.icu.text.ArabicShapingException where it
  // shapes right-to-left text, and the JVM resolves that type when it links
  // Font; the client's own libraries hold ICU, the server's (which this runs
  // on) do not. The exception, defined here as an Exception of that name, is
  // all that linking asks: the text the block entity renderers lay out at
  // rest (a sign's, empty) is never shaped (McbenchBlockEntities). Likewise
  // the item model property local_time (SelectItemModelProperties, which
  // McbenchEntities registers as the client does) formats the date with an
  // ICU SimpleDateFormat: the chest's item model draws a Christmas chest
  // from December 24 to 26. Each ICU class asked for is defined here with the
  // constructors and methods the client's classes name of it, each answering
  // as if ICU knew nothing (null, zero or false): a date formats to null,
  // which no case of a select matches, so such an item shows its fallback, the
  // chest of every other day. A class's superclass is ICU's for the two that
  // are assigned to another.
  static final class IcuLinkage extends ClassLoader {
    static final Map<String, String> PARENTS = Map.of(
      "com/ibm/icu/text/ArabicShapingException", "java/lang/Exception",
      "com/ibm/icu/text/SimpleDateFormat", "com/ibm/icu/text/DateFormat");
    // By ICU class, the methods named of it: name and descriptor, and
    // whether static (an invokestatic, or a static method handle).
    final Map<String, Map<String, Boolean>> named = new java.util.HashMap<>();

    IcuLinkage(List<ClassModel> classes) {
      super(ClassLoader.getPlatformClassLoader());
      for (ClassModel model : classes) {
        // Only a class whose constant pool names ICU is read further.
        boolean names = false;
        for (var entry : model.constantPool()) {
          if (entry instanceof java.lang.classfile.constantpool.ClassEntry type && type.asInternalName().startsWith("com/ibm/icu/")) names = true;
          if (entry instanceof java.lang.classfile.constantpool.MethodHandleEntry handle && handle.reference().owner().asInternalName().startsWith("com/ibm/icu/")) {
            add(handle.reference().owner().asInternalName(), handle.reference().name().stringValue() + handle.reference().type().stringValue(), handle.kind() == 6);
          }
        }
        if (!names) continue;
        for (MethodModel method : model.methods()) {
          for (InvokeInstruction invoke : invocations(method)) {
            if (invoke.owner().asInternalName().startsWith("com/ibm/icu/")) add(invoke.owner().asInternalName(), invoke.name().stringValue() + invoke.type().stringValue(), invoke.opcode() == java.lang.classfile.Opcode.INVOKESTATIC);
          }
        }
      }
    }

    void add(String owner, String method, boolean isStatic) {
      named.computeIfAbsent(owner, (key) -> new java.util.TreeMap<>()).put(method, isStatic);
    }

    @Override protected Class<?> findClass(String name) throws ClassNotFoundException {
      String internal = name.replace('.', '/');
      if (!internal.startsWith("com/ibm/icu/")) throw new ClassNotFoundException(name);
      ClassDesc self = ClassDesc.ofInternalName(internal), superclass = ClassDesc.ofInternalName(PARENTS.getOrDefault(internal, "java/lang/Object"));
      byte[] bytes = ClassFile.of().build(self, (type) -> {
        type.withSuperclass(superclass).withFlags(AccessFlag.PUBLIC);
        Map<String, Boolean> methods = new java.util.TreeMap<>(named.getOrDefault(internal, Map.of()));
        methods.put("<init>()V", false);
        methods.forEach((method, isStatic) -> {
          int open = method.indexOf('(');
          String methodName = method.substring(0, open);
          if (!isStatic && List.of("equals", "hashCode", "toString").contains(methodName)) return;
          MethodTypeDesc descriptor = MethodTypeDesc.ofDescriptor(method.substring(open));
          type.withMethodBody(methodName, descriptor, AccessFlag.PUBLIC.mask() | (isStatic ? AccessFlag.STATIC.mask() : 0), (code) -> {
            if (methodName.equals("<init>")) code.aload(0).invokespecial(superclass, "<init>", MethodTypeDesc.of(ClassDesc.ofDescriptor("V"))).return_();
            else {
              var returned = java.lang.classfile.TypeKind.from(descriptor.returnType());
              switch (returned) {
                case VOID -> code.return_();
                case REFERENCE -> code.aconst_null().areturn();
                case LONG -> code.lconst_0().lreturn();
                case FLOAT -> code.fconst_0().freturn();
                case DOUBLE -> code.dconst_0().dreturn();
                default -> code.iconst_0().ireturn();
              }
            }
          });
        });
      });
      return defineClass(name, bytes, 0, bytes.length);
    }
  }

  static <T> T only(String what, List<T> found) {
    if (found.size() != 1) throw new IllegalStateException("Found " + found.size() + " candidates for " + what + " in the client jar, not one");
    return found.get(0);
  }
}
