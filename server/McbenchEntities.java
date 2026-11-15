import java.awt.image.BufferedImage;
import java.lang.classfile.ClassFile;
import java.lang.constant.ClassDesc;
import java.lang.constant.MethodTypeDesc;
import java.lang.invoke.MethodType;
import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.lang.reflect.ParameterizedType;
import java.lang.reflect.Proxy;
import java.lang.reflect.Type;
import java.nio.file.FileSystem;
import java.nio.file.FileSystems;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashMap;
import java.util.IdentityHashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.Executor;
import java.util.function.BiFunction;
import javax.imageio.ImageIO;

// What Minecraft's client draws of every kind of entity the engine holds,
// asked of its own entity renderers in the bake's JVM, where McbenchClient
// has started the client's classes; the live viewer's entity layer
// (web/entities.mjs) and see_build (web/viewer.js) draw entities from it,
// through web/entity-draw.mjs.
//
// Each kind's renderer is made as the client makes it (EntityRenderers's
// provider, from an EntityRendererProvider.Context), an entity of the kind is
// read from saved data as the client reads one (EntityType.create, then
// Entity.load from SNBT through TagValueInput), extracted into its render
// state at partial tick 0, and submitted, under the renderer's render offset
// as EntityRenderDispatcher submits it, to a SubmitNodeCollector of this
// program's. What it submits is drawn by Minecraft's own code: a model by
// Model.setupAnim and ModelPart.render into a VertexConsumer of this
// program's, each quad told its part by ModelPart.visit; custom geometry by
// its renderer; an item's baked quads (ItemModelResolver over item models
// baked by the client's own ModelBakery from the jar's models); a block
// model's parts (ModelBlockRenderer.renderModel's). A submitted block is
// kept as its state, which the page draws as the live viewer draws the
// block alone, and what the special block renderer draws of its block is
// captured as the client's collector draws it (SpecialBlockModelRenderer
// .renderByBlock).
//
// The client's classes and their members are named here by their Mojang
// names, which Mojang's mappings of the client jar (Mappings) turn into the
// jar's; a jar the mappings are not of fails at the first name it does not
// have.
//
// What varies by entity is found from what the renderer draws: a kind's
// variants (a field's every value: a minecart's TNT fuse, an armor stand's
// flags, a frame's facing and rotation, a painting's variant) are each
// captured; a value that varies continuously (yaw, pitch, a display offset,
// an armor stand's pose angles) is changed by a step from the variant's
// base, and each draw's pose moves by a turn about a line or a move along a
// vector (Motion), each model part's pose by a rate per unit (its fields,
// read from the part), and a draw that follows a model part (a held item, a
// head on a pose) keeps its pose in that part's. Each fit is checked against
// what the renderer draws with every value changed at once; the order the
// motions compose in is the one that draws it. How a kind is lit is asked of
// its renderer too (getPackedLightCoords in lights of each level, and each
// draw's light, its own or by cell).
//
// What the renderers are given to run with no window: the context's parts
// are the client's own where they load from the jar (EntityModelSet's
// vanilla set, the equipment assets by their reload listener, item models
// baked by ModelBakery, the special block renderers baked as the client
// bakes them, the jar's data registries a world loads for painting variants
// and damage types by RegistryDataLoader), and stand-ins of this program's
// where they would need the GPU or a running game: the atlases (each
// TextureAtlasSprite made here over a texture of the jar, in an atlas of
// this program's whose cells say which texture a coordinate is over; no atlas
// is stitched), the block model shaper (a state's model baked when asked),
// a level that is air everywhere and answers the light this program sets,
// and a bare game whose debug entries, options (no entity shadows: neither
// view draws them) and player (in survival, on no team) are the few its
// renderers read.
//
//   java McbenchEntities client.jar mappings.txt blockTable <Minecraft's libraries> biome capture out.json
// captures and writes out.json and its atlas, out.json.png.
final class McbenchEntities {
  final McbenchClient client;
  final ClassLoader loader;
  final FileSystem jar;
  final Mappings mappings;
  // A sprite made here, by the texture it stands for.
  final Map<Object, String> sprites = new IdentityHashMap<>();
  final Map<String, Object> spriteOf = new HashMap<>();
  final java.util.Set<String> made = new java.util.TreeSet<>();

  Object resources, context, level, reason, models, specials, registryAccess, damageSources, scoreboard;
  // The light the level answers, by LightLayer (its ordinal: 0 sky, 1 block)
  // and position.
  BiFunction<Integer, Object, Integer> light = (layer, pos) -> 0;
  Class<?> poseType, collectorType, cameraType;
  Map<?, ?> providers;
  // Every block state's unbaked model (the client's blockstate files, with
  // the item frames' fake states), and what baking them needs.
  Map<?, ?> roots;
  Object bakeModels, bakeMaterials, bakeSkins, bakeItems, bakeResolved, bakeMissing, spriteGetterProxy;
  final Map<Object, Object> bakedStates = new HashMap<>();
  Method byKey, create, load, parseTag, valueInput, createState, submit;
  Object discard;

  McbenchEntities(McbenchClient client, Path clientJar, Path mappings) throws Exception {
    this.client = client;
    this.loader = client.loader;
    this.jar = FileSystems.newFileSystem(clientJar);
    this.mappings = new Mappings(mappings);
  }

  static Class<?> cls(ClassLoader loader, String name) throws Exception {
    return Class.forName(name, true, loader);
  }

  Class<?> cls(String name) throws Exception {
    return cls(loader, name);
  }

  // Mojang's mappings of the client jar (its version's client_mappings,
  // which bluemap.js downloads beside the BlueMap jar): every class's name in
  // the jar by its Mojang name, and a class's members, read from its lines
  // the first time one of them is asked for. This program names the client's
  // classes and members only by their Mojang names; a jar the mappings are
  // not of fails at the first one it does not have.
  static final class Mappings {
    // A field (arguments null) or method of a class: its Mojang name, its
    // arguments' Mojang types as the mappings write them, its name in the jar.
    record Member(String name, String arguments, String jar) {}

    final String text;
    final Map<String, String> jarNames = new HashMap<>();
    // A class's simple name (ModelPart.Cube) to its Mojang name, or "" for
    // a simple name two classes have.
    final Map<String, String> bySimpleName = new HashMap<>();
    // Where each class's member lines begin and end in the text.
    final Map<String, int[]> memberLines = new HashMap<>();
    final Map<String, List<Member>> members = new HashMap<>();

    // A class's line is its Mojang name, " -> ", its name in the jar and
    // ":"; its members' lines below it are indented; comments begin "#".
    Mappings(Path file) throws Exception {
      text = Files.readString(file);
      String owner = null;
      for (int at = 0, end; at < text.length(); at = end + 1) {
        end = text.indexOf('\n', at);
        char first = text.charAt(at);
        if (first == ' ' || first == '#') continue;
        if (owner != null) memberLines.get(owner)[1] = at;
        int arrow = text.indexOf(" -> ", at);
        owner = text.substring(at, arrow);
        jarNames.put(owner, text.substring(arrow + 4, end - 1));
        bySimpleName.merge(owner.substring(owner.lastIndexOf('.') + 1).replace('$', '.'), owner, (one, other) -> "");
        memberLines.put(owner, new int[] { end + 1, text.length() });
      }
    }

    String className(String simpleName) {
      String name = bySimpleName.get(simpleName);
      if (name == null) throw new IllegalStateException("Mojang's client mappings have no class " + simpleName);
      if (name.isEmpty()) throw new IllegalStateException("Mojang's client mappings have more than one class " + simpleName);
      return name;
    }

    // Members are "type name -> jar" (a field) and "[from:to:]type
    // name(arguments)[:from:to] -> jar" (a method).
    List<Member> members(String owner) {
      return members.computeIfAbsent(owner, (name) -> {
        int[] lines = memberLines.get(name);
        List<Member> out = new ArrayList<>();
        for (String line : text.substring(lines[0], lines[1]).split("\n")) {
          line = line.strip();
          if (line.startsWith("#")) continue;
          int arrow = line.lastIndexOf(" -> "), open = line.indexOf('(');
          String jar = line.substring(arrow + 4);
          if (open < 0) out.add(new Member(line.substring(line.indexOf(' ') + 1, arrow), null, jar));
          else out.add(new Member(line.substring(line.lastIndexOf(' ', open) + 1, open), line.substring(open + 1, line.indexOf(')')), jar));
        }
        return out;
      });
    }

    // A type as the mappings write it ("net.minecraft.world.phys.Vec3",
    // "int[]") as the jar's Class.getTypeName says it.
    String jarType(String type) {
      int dims = type.indexOf('[');
      String base = dims < 0 ? type : type.substring(0, dims);
      return jarNames.getOrDefault(base, base) + (dims < 0 ? "" : type.substring(dims));
    }

    String field(String owner, String name) {
      for (Member member : members(owner)) if (member.arguments() == null && member.name().equals(name)) return member.jar();
      throw new IllegalStateException("Mojang's client mappings have no field " + owner + "." + name);
    }

    String method(String owner, String name, Class<?>... parameters) {
      String wanted = String.join(",", Arrays.stream(parameters).map(Class::getTypeName).toList());
      for (Member member : members(owner)) {
        if (member.arguments() == null || !member.name().equals(name)) continue;
        String arguments = String.join(",", member.arguments().isEmpty() ? List.of() : Arrays.stream(member.arguments().split(",")).map(this::jarType).toList());
        if (arguments.equals(wanted)) return member.jar();
      }
      throw new IllegalStateException("Mojang's client mappings have no method " + owner + "." + name + "(" + wanted + ")");
    }
  }

  // Each class, method and field the first time it is named, as named:
  // many are named for every quad drawn.
  final Map<String, Class<?>> types = new HashMap<>();
  final Map<List<Object>, Method> methods = new HashMap<>();
  final Map<List<String>, Field> fields = new HashMap<>();

  // The client's class of a simple Mojang name (the class's own, after its
  // outer classes': "ModelPart.Cube").
  Class<?> type(String name) throws Exception {
    Class<?> known = types.get(name);
    if (known != null) return known;
    String jarName = mappings.jarNames.get(mappings.className(name));
    try {
      types.put(name, known = cls(jarName));
      return known;
    } catch (ClassNotFoundException missing) {
      throw new IllegalStateException("The client jar has no class " + name + " (" + jarName + " in Mojang's mappings)", missing);
    }
  }

  Class<?> resourceManager() throws Exception {
    return type("ResourceManager");
  }

  // The method `name` declared by class `owner` (simple Mojang names) of
  // these parameters.
  Method method(String owner, String name, Class<?>... parameters) throws Exception {
    List<Object> key = List.of(owner, name, List.of(parameters));
    Method known = methods.get(key);
    if (known != null) return known;
    String jarName = mappings.method(mappings.className(owner), name, parameters);
    try {
      Method m = type(owner).getDeclaredMethod(jarName, parameters);
      m.setAccessible(true);
      methods.put(key, m);
      return m;
    } catch (NoSuchMethodException missing) {
      throw new IllegalStateException("The client jar has no method " + owner + "." + name + " (" + jarName + " in Mojang's mappings)", missing);
    }
  }

  Field field(String owner, String name) throws Exception {
    List<String> key = List.of(owner, name);
    Field known = fields.get(key);
    if (known != null) return known;
    String jarName = mappings.field(mappings.className(owner), name);
    try {
      Field f = type(owner).getDeclaredField(jarName);
      f.setAccessible(true);
      fields.put(key, f);
      return f;
    } catch (NoSuchFieldException missing) {
      throw new IllegalStateException("The client jar has no field " + owner + "." + name + " (" + jarName + " in Mojang's mappings)", missing);
    }
  }

  Object get(Object target, String owner, String name) throws Exception {
    return field(owner, name).get(target);
  }

  void set(Object target, String owner, String name, Object value) throws Exception {
    field(owner, name).set(target, value);
  }

  // Whether a proxy was asked `method` (a Method of the interface it
  // implements, or of an interface that one extends).
  static boolean asked(Method asked, Method method) {
    return asked.getName().equals(method.getName()) && Arrays.equals(asked.getParameterTypes(), method.getParameterTypes());
  }

  // The sprites made here all lie in one atlas of this program's, each in a
  // cell of its own (CELL texels square, CELLS a row), so that a texture
  // coordinate the client computes from a sprite (TextureAtlasSprite.getU)
  // says which texture it is over and where (decode). No atlas is stitched.
  static final int CELL = 1024, CELLS = 64, SIZE = CELL * CELLS;
  final List<String> cells = new ArrayList<>();
  final List<int[]> cellSizes = new ArrayList<>();

  // The sprite of a texture file ("minecraft:entity/minecart", under
  // textures/), in atlas `atlas`: a TextureAtlasSprite whose contents answer
  // as the texture's pixels (its first frame, for an animated one).
  Object sprite(Object atlas, String texture) throws Exception {
    Object known = spriteOf.get(texture);
    if (known != null) return known;
    String[] id = texture.split(":", 2);
    BufferedImage image;
    Path file = jar.getPath("assets/" + id[0] + "/textures/" + id[1] + ".png");
    boolean exists = Files.exists(file);
    if (!exists) {
      // A texture the client makes as it stitches its atlases (the missing
      // texture, an armor trim's palette permutations): 16 by 16 and opaque
      // here, and never drawn (decode refuses a quad of one).
      image = new BufferedImage(16, 16, BufferedImage.TYPE_INT_RGB);
    } else {
      try (var stream = Files.newInputStream(file)) {
        image = ImageIO.read(stream);
      }
    }
    Class<?> contentsType = type("SpriteContents");
    Class<?> spriteType = type("TextureAtlasSprite");
    int width = image.getWidth(), height = image.getHeight();
    // An animated texture's frames are its first frame's size.
    Path meta = jar.getPath("assets/" + id[0] + "/textures/" + id[1] + ".png.mcmeta");
    if (Files.exists(meta) && com.google.gson.JsonParser.parseString(Files.readString(meta)).getAsJsonObject().has("animation")) height = width;
    if (width > CELL || height > CELL) throw new IllegalStateException(texture + " is larger than a cell");
    int frameHeight = height;
    Object name = method("ResourceLocation", "fromNamespaceAndPath", String.class, String.class).invoke(null, id[0], id[1]);
    // SpriteContents.isTransparent(frame, x, y) and getUniqueFrames, which
    // ItemModelGenerator reads.
    Object contents = standIn(contentsType, Map.of(
      method("SpriteContents", "isTransparent", int.class, int.class, int.class), (args) -> ((image.getRGB((int) args[2], (int) args[3]) >>> 24) == 0),
      method("SpriteContents", "getUniqueFrames"), (args) -> java.util.stream.IntStream.of(0)));
    set(contents, "SpriteContents", "name", name);
    set(contents, "SpriteContents", "width", width);
    set(contents, "SpriteContents", "height", frameHeight);
    if (!exists) made.add(texture);
    int cell = cells.size();
    cells.add(texture);
    cellSizes.add(new int[] { width, frameHeight });
    Constructor<?> spriteMaker = spriteType.getDeclaredConstructor(name.getClass(), contentsType, int.class, int.class, int.class, int.class);
    spriteMaker.setAccessible(true);
    Object sprite = spriteMaker.newInstance(atlas, contents, SIZE, SIZE, cell % CELLS * CELL, cell / CELLS * CELL);
    sprites.put(sprite, texture);
    spriteOf.put(texture, sprite);
    return sprite;
  }

  // The texture and its own coordinates (0 to 1 over it) of a texture
  // coordinate over this program's atlas.
  record Texel(String texture, float u, float v) {}

  Texel decode(float u, float v) {
    int cx = (int) Math.floor(u * CELLS), cy = (int) Math.floor(v * CELLS);
    // A coordinate on a cell's far edge is its sprite's u1 or v1.
    int cell = cy * CELLS + cx;
    int[] size = null;
    for (int dx = 0; dx >= -1 && size == null; dx--) for (int dy = 0; dy >= -1 && size == null; dy--) {
      int at = (cy + dy) * CELLS + cx + dx;
      if (cy + dy < 0 || cx + dx < 0 || at >= cells.size()) continue;
      int[] candidate = cellSizes.get(at);
      float lu = u * SIZE - (cx + dx) * CELL, lv = v * SIZE - (cy + dy) * CELL;
      if (lu >= -1e-3f && lu <= candidate[0] + 1e-3f && lv >= -1e-3f && lv <= candidate[1] + 1e-3f) {
        size = candidate;
        cell = at;
      }
    }
    if (size == null) throw new IllegalStateException("A texture coordinate over no sprite: " + u + ", " + v);
    String texture = cells.get(cell);
    if (made.contains(texture)) throw new IllegalStateException("A quad of a texture the client makes, not a file of the jar: " + texture);
    // A coordinate the client computed over this atlas is within a few
    // thousandths of a texel of the one it stands for (a float's precision
    // at the atlas's size): the texel, to a sixty-fourth, makes it the same
    // whichever cell the texture was given.
    double u0 = Math.round((u * (double) SIZE - cell % CELLS * CELL) * 64) / 64.0, v0 = Math.round((v * (double) SIZE - cell / CELLS * CELL) * 64) / 64.0;
    return new Texel(texture, (float) (u0 / size[0]), (float) (v0 / size[1]));
  }

  // The texture file a sprite of an atlas is made from: the atlas's sprite
  // sources as the client reads them (SpriteSourceList.load of its
  // definition, AtlasManager's KNOWN_ATLASES), the last that names the
  // sprite: a directory lister's file under its source, or a single file's.
  // Null for a sprite made otherwise (palette permutations).
  final Map<Object, List<Object>> sources = new HashMap<>();

  String fileOf(Object atlas, Object sprite) throws Exception {
    List<Object> list = sources.get(atlas);
    if (list == null) {
      Object definition = null;
      // An atlas is named by its texture or by its definition.
      for (Object config : (List<?>) field("AtlasManager", "KNOWN_ATLASES").get(null)) if (get(config, "AtlasManager.AtlasConfig", "textureId").equals(atlas) || get(config, "AtlasManager.AtlasConfig", "definitionLocation").equals(atlas)) definition = get(config, "AtlasManager.AtlasConfig", "definitionLocation");
      if (definition == null) throw new IllegalStateException("No atlas " + atlas);
      Object loaded = method("SpriteSourceList", "load", resourceManager(), type("ResourceLocation")).invoke(null, resources, definition);
      sources.put(atlas, list = new ArrayList<>((List<?>) get(loaded, "SpriteSourceList", "sources")));
    }
    String[] id = sprite.toString().split(":", 2);
    String found = null;
    for (Object source : list) {
      if (source.getClass() == type("DirectoryLister")) {
        String path = (String) get(source, "DirectoryLister", "sourcePath"), prefix = (String) get(source, "DirectoryLister", "idPrefix");
        if (!id[1].startsWith(prefix)) continue;
        String rest = path + "/" + id[1].substring(prefix.length());
        if (Files.exists(jar.getPath("assets/" + id[0] + "/textures/" + rest + ".png"))) found = id[0] + ":" + rest;
      } else if (source.getClass() == type("SingleFile")) {
        Object file = get(source, "SingleFile", "resourceId");
        Object named = ((java.util.Optional<Object>) get(source, "SingleFile", "spriteId")).orElse(file);
        if (named.equals(sprite)) found = file.toString();
      }
    }
    return found == null ? sprite.toString() : found;
  }

  Object atlasSprite(Object atlas, Object id) throws Exception {
    return sprite(atlas, fileOf(atlas, id));
  }

  // A Material's sprite (MaterialSet.get).
  Object materialSprite(Object material) throws Exception {
    return atlasSprite(get(material, "Material", "atlasLocation"), get(material, "Material", "texture"));
  }

  // An instance of a subclass of `type` made here, whose methods `answers`
  // names answer as given (with their arguments), every other as its own:
  // made without running a constructor.
  Object standIn(Class<?> type, Map<Method, java.util.function.Function<Object[], Object>> answers) throws Exception {
    List<Method> list = new ArrayList<>(answers.keySet());
    var self = ClassDesc.of("McbenchStandIn" + type.getName().replace('$', '_') + System.identityHashCode(answers));
    var handlerType = ClassDesc.of("java.util.function.BiFunction");
    var objectType = ClassDesc.of("java.lang.Object");
    byte[] bytes = ClassFile.of().build(self, (made) -> {
      made.withSuperclass(ClassDesc.of(type.getName())).withFlags(java.lang.reflect.AccessFlag.PUBLIC);
      made.withField("HANDLER", handlerType, Modifier.PUBLIC | Modifier.STATIC);
      for (int index = 0; index < list.size(); index++) {
        Method m = list.get(index);
        int at = index;
        var descriptor = MethodTypeDesc.ofDescriptor(MethodType.methodType(m.getReturnType(), m.getParameterTypes()).toMethodDescriptorString());
        made.withMethodBody(m.getName(), descriptor, Modifier.isPublic(m.getModifiers()) ? Modifier.PUBLIC : Modifier.PROTECTED, (code) -> {
          code.getstatic(self, "HANDLER", handlerType);
          code.loadConstant(at);
          code.invokestatic(ClassDesc.of("java.lang.Integer"), "valueOf", MethodTypeDesc.ofDescriptor("(I)Ljava/lang/Integer;"));
          Class<?>[] params = m.getParameterTypes();
          code.loadConstant(params.length + 1);
          code.anewarray(objectType);
          code.dup();
          code.loadConstant(0);
          code.aload(0);
          code.aastore();
          int slot = 1;
          for (int p = 0; p < params.length; p++) {
            code.dup();
            code.loadConstant(p + 1);
            Class<?> t = params[p];
            var kind = java.lang.classfile.TypeKind.from(t);
            code.loadLocal(kind, slot);
            slot += kind.slotSize();
            if (t.isPrimitive()) {
              Class<?> box = MethodType.methodType(t).wrap().returnType();
              code.invokestatic(ClassDesc.of(box.getName()), "valueOf", MethodTypeDesc.of(ClassDesc.of(box.getName()), t.describeConstable().orElseThrow()));
            }
            code.aastore();
          }
          code.invokeinterface(handlerType, "apply", MethodTypeDesc.of(objectType, objectType, objectType));
          Class<?> r = m.getReturnType();
          if (r == void.class) {
            code.pop();
            code.return_();
          } else if (r.isPrimitive()) {
            Class<?> box = MethodType.methodType(r).wrap().returnType();
            var boxDesc = ClassDesc.of(box.getName());
            code.checkcast(boxDesc);
            code.invokevirtual(boxDesc, r.getName() + "Value", MethodTypeDesc.of(r.describeConstable().orElseThrow()));
            code.return_(java.lang.classfile.TypeKind.from(r));
          } else {
            code.checkcast(r.describeConstable().orElseThrow());
            code.areturn();
          }
        });
      }
    });
    Class<?> made = new McbenchBlockEntities.Defining(loader).define(bytes);
    made.getField("HANDLER").set(null, (BiFunction<Object, Object, Object>) (index, args) -> answers.get(list.get((Integer) index)).apply((Object[]) args));
    return McbenchBlockEntities.allocate(made);
  }

  // What the client's entity renderers draw of every kind the engine holds,
  // captured in the JVM McbenchClient has started the client in.
  static McbenchEntities capture(McbenchClient client, Path clientJar, Path mappings) throws Exception {
    var entities = new McbenchEntities(client, clientJar, mappings);
    entities.setUp();
    entities.capture();
    return entities;
  }

  public static void main(String[] args) throws Exception {
    McbenchClient client = McbenchClient.read(Path.of(args[0]), Path.of(args[2]), args[3], args[4]);
    long started = System.nanoTime();
    var entities = capture(client, Path.of(args[0]), Path.of(args[1]));
    System.out.println("captured in " + (System.nanoTime() - started) / 1_000_000 + " ms: " + entities.kinds.size() + " kinds, " + entities.meshes.size() + " meshes, " + entities.shapes.size() + " shapes, " + entities.textures.size() + " textures");
    var atlased = new java.util.TreeSet<>(entities.textures);
    atlased.remove("");
    entities.write(Path.of(args[6]), McbenchEntityAssets.atlas(Path.of(args[0]), atlased, Path.of(args[6] + ".png")));
    System.out.println("written " + Files.size(Path.of(args[6])) + " bytes");
    // Minecraft's entry point (McbenchClient) leaves threads behind.
    System.exit(0);
  }

  void setUp() throws Exception {
    // The registries of item model, property, tint source and special
    // renderer types the client fills as it starts: each a class holding a
    // LateBoundIdMapper keyed by ResourceLocation, filled by its public static
    // bootstrap (unless filled already).
    for (String name : new String[] { "ItemTintSources", "ConditionalItemModelProperties", "ItemModels", "RangeSelectItemModelProperties", "SelectItemModelProperties", "SpecialModelRenderers", "SpriteSources" }) {
      Class<?> holder = type(name);
      Field mapper = null;
      for (Field f : holder.getDeclaredFields()) if (Modifier.isStatic(f.getModifiers()) && f.getType() == type("ExtraCodecs.LateBoundIdMapper")) mapper = f;
      mapper.setAccessible(true);
      Object ids = mapper.get(null);
      boolean empty = true;
      for (Field f : ids.getClass().getDeclaredFields()) {
        f.setAccessible(true);
        if (f.get(ids) instanceof Map<?, ?> map && !map.isEmpty()) empty = false;
      }
      if (empty) method(name, "bootstrap").invoke(null);
    }
    providers = (Map<?, ?>) field("EntityRenderers", "PROVIDERS").get(null);
    poseType = type("PoseStack");
    collectorType = type("SubmitNodeCollector");
    cameraType = type("CameraRenderState");
    submit = method("EntityRenderer", "submit", type("EntityRenderState"), poseType, collectorType, cameraType);
    createState = method("EntityRenderer", "createRenderState", type("Entity"), float.class);

    // The client's resources: its jar's assets, as Minecraft's vanilla pack.
    Class<?> packType = type("PackType");
    Object clientResources = packType.getEnumConstants()[0];
    Object builder = type("VanillaPackResourcesBuilder").getConstructor().newInstance();
    method("VanillaPackResourcesBuilder", "pushAssetPath", packType, Path.class).invoke(builder, clientResources, jar.getPath("/assets"));
    method("VanillaPackResourcesBuilder", "exposeNamespace", String[].class).invoke(builder, (Object) new String[] { "minecraft" });
    Class<?> location = type("PackLocationInfo");
    Object info = location.getConstructors()[0].newInstance("vanilla", null, null, java.util.Optional.empty());
    Object pack = method("VanillaPackResourcesBuilder", "build", location).invoke(builder, info);
    resources = type("FallbackResourceManager").getConstructor(packType, String.class).newInstance(clientResources, "minecraft");
    method("FallbackResourceManager", "push", type("PackResources")).invoke(resources, pack);
    // And its data pack, for the registries a world loads from data
    // (RegistryDataLoader's WORLDGEN_REGISTRIES) that an entity asks of its
    // level: painting variants, damage types.
    Object serverData = packType.getEnumConstants()[1];
    Object dataBuilder = type("VanillaPackResourcesBuilder").getConstructor().newInstance();
    method("VanillaPackResourcesBuilder", "pushAssetPath", packType, Path.class).invoke(dataBuilder, serverData, jar.getPath("/data"));
    method("VanillaPackResourcesBuilder", "exposeNamespace", String[].class).invoke(dataBuilder, (Object) new String[] { "minecraft" });
    Object dataPack = method("VanillaPackResourcesBuilder", "build", location).invoke(dataBuilder, info);
    Object data = type("FallbackResourceManager").getConstructor(packType, String.class).newInstance(serverData, "minecraft");
    method("FallbackResourceManager", "push", type("PackResources")).invoke(data, dataPack);
    List<Object> wanted = new ArrayList<>();
    for (Object registry : (List<?>) field("RegistryDataLoader", "WORLDGEN_REGISTRIES").get(null)) {
      String key = get(registry, "RegistryDataLoader.RegistryData", "key").toString();
      if (key.contains("minecraft:painting_variant") || key.contains("minecraft:damage_type")) wanted.add(registry);
    }
    Object loadedRegistries = method("RegistryDataLoader", "load", resourceManager(), List.class, List.class).invoke(null, data, client.registries, wanted);
    Method lookup = method("RegistryAccess", "lookup", type("ResourceKey")), key = method("Registry", "key");
    registryAccess = Proxy.newProxyInstance(loader, new Class<?>[] { type("RegistryAccess") }, (proxy, m, a) -> {
      if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, a);
      if (asked(m, lookup)) {
        java.util.Optional<?> found = (java.util.Optional<?>) lookup.invoke(loadedRegistries, a[0]);
        if (found.isPresent()) return found;
        for (Object registry : client.registries) if (key.invoke(registry).equals(a[0])) return java.util.Optional.of(registry);
        return java.util.Optional.empty();
      }
      throw new UnsupportedOperationException("An entity asked the registries " + m);
    });

    // The client's loading and baking, run here in turn: baked on several
    // threads at once, an item model's quads come out in an order that
    // differs from one bake to the next.
    Executor direct = Runnable::run;
    Class<?> resourceManager = resourceManager();
    Class<?> manager = type("ModelManager");
    Map<?, ?> unbaked = (Map<?, ?>) ((CompletableFuture<?>) method("ModelManager", "loadBlockModels", resourceManager, Executor.class).invoke(null, resources, direct)).join();
    Object items = ((CompletableFuture<?>) method("ClientItemInfoLoader", "scheduleLoad", resourceManager, Executor.class).invoke(null, resources, direct)).join();
    Object blockStates = ((CompletableFuture<?>) method("BlockStateModelLoader", "loadBlockStates", resourceManager, Executor.class).invoke(null, resources, direct)).join();
    roots = (Map<?, ?>) method("BlockStateModelLoader.LoadedModels", "models").invoke(blockStates);
    Object resolved = method("ModelManager", "discoverModelDependencies", Map.class, type("BlockStateModelLoader.LoadedModels"), type("ClientItemInfoLoader.LoadedClientInfos")).invoke(null, unbaked, blockStates, items);
    Method resolvedModels = method("ModelManager.ResolvedModels", "models"), missingModel = method("ModelManager.ResolvedModels", "missing");

    models = method("EntityModelSet", "vanilla").invoke(null);
    Class<?> materialSet = type("MaterialSet");
    Method materialGet = method("MaterialSet", "get", type("Material"));
    Object materials = Proxy.newProxyInstance(loader, new Class<?>[] { materialSet }, (proxy, m, a) -> {
      if (asked(m, materialGet)) return materialSprite(a[0]);
      throw new UnsupportedOperationException("The material set was asked " + m);
    });
    Object skins = McbenchBlockEntities.allocate(type("PlayerSkinRenderCache"));
    Class<?> bakery = type("ModelBakery");
    bakeModels = models;
    bakeMaterials = materials;
    bakeSkins = skins;
    bakeItems = method("ClientItemInfoLoader.LoadedClientInfos", "contents").invoke(items);
    bakeResolved = resolvedModels.invoke(resolved);
    bakeMissing = missingModel.invoke(resolved);
    Object bake = bakery.getConstructors()[0].newInstance(models, materials, skins, Map.of(), bakeItems, bakeResolved, bakeMissing);
    Class<?> spriteGetter = type("SpriteGetter");
    Object getter = Proxy.newProxyInstance(loader, new Class<?>[] { spriteGetter }, (proxy, m, a) -> {
      if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, a);
      if (a[0] instanceof String missing) throw new IllegalStateException("A model names no texture " + missing + " (" + a[1] + ")");
      return materialSprite(a[0]);
    });
    spriteGetterProxy = getter;
    Object baked = ((CompletableFuture<?>) method("ModelBakery", "bakeModels", spriteGetter, Executor.class).invoke(bake, getter, direct)).join();
    Map<?, ?> itemModels = (Map<?, ?>) method("ModelBakery.BakingResult", "itemStackModels").invoke(baked);

    // The model manager the item model resolver asks: its item models and
    // their properties, as baked.
    Object modelManager = McbenchBlockEntities.allocate(manager);
    for (Field f : manager.getDeclaredFields()) {
      if (f.getType() != Map.class || Modifier.isStatic(f.getModifiers())) continue;
      Type value = ((ParameterizedType) f.getGenericType()).getActualTypeArguments()[1];
      f.setAccessible(true);
      if (value == type("ItemModel")) f.set(modelManager, itemModels);
      else f.set(modelManager, method("ModelBakery.BakingResult", "itemProperties").invoke(baked));
    }
    // Its missing models (the bake's), which a renderer asking for a model
    // that is not there would draw.
    set(modelManager, "ModelManager", "missingModels", method("ModelBakery.BakingResult", "missingModels").invoke(baked));
    Object resolver = type("ItemModelResolver").getConstructor(manager).newInstance(modelManager);

    // The atlases (AtlasManager): each a TextureAtlas of this program's,
    // whose sprites are made here (atlasSprite).
    Class<?> atlasType = type("TextureAtlas");
    Map<Object, Object> atlasesByLocation = new HashMap<>();
    Class<?> atlasesType = type("AtlasManager");
    Object atlases = standIn(atlasesType, Map.of(
      method("AtlasManager", "getAtlasOrThrow", type("ResourceLocation")), (a) -> atlasesByLocation.computeIfAbsent(a[1], (where) -> {
        try {
          Object made = standIn(atlasType, Map.of(method("TextureAtlas", "getSprite", type("ResourceLocation")), (b) -> {
            try {
              return atlasSprite(where, b[1]);
            } catch (Exception error) {
              throw new IllegalStateException(error);
            }
          }));
          set(made, "TextureAtlas", "location", where);
          return made;
        } catch (Exception error) {
          throw new IllegalStateException(error);
        }
      }),
      method("AtlasManager", "get", type("Material")), (a) -> {
        try {
          return materialSprite(a[1]);
        } catch (Exception error) {
          throw new IllegalStateException(error);
        }
      }));

    // The font, over a glyph provider that fails if asked (as for block
    // entities).
    Class<?> fontType = type("Font");
    Constructor<?> fontMaker = fontType.getConstructors()[0];
    Object font = fontMaker.newInstance(Proxy.newProxyInstance(loader, new Class<?>[] { fontMaker.getParameterTypes()[0] }, (proxy, m, a) -> {
      throw new UnsupportedOperationException("An entity renderer asked the font " + m);
    }));

    // The equipment assets, loaded by their reload listener from the jar.
    Object equipment = type("EquipmentAssetManager").getConstructor().newInstance();
    Object profiler = null;
    for (Field f : type("ProfilerFiller").getFields()) if (Modifier.isStatic(f.getModifiers()) && f.getType() == type("ProfilerFiller")) profiler = f.get(null);
    if (profiler == null) profiler = method("Profiler", "get").invoke(null);
    Object loaded = method("SimpleJsonResourceReloadListener", "prepare", resourceManager, type("ProfilerFiller")).invoke(equipment, resources, profiler);
    method("EquipmentAssetManager", "apply", Map.class, resourceManager, type("ProfilerFiller")).invoke(equipment, loaded, resources, profiler);

    // The special block renderers, as the client bakes them.
    Class<?> bakingContext = type("SpecialModelRenderer.BakingContext");
    Object atlasesFinal = atlases;
    Method entityModelSet = method("SpecialModelRenderer.BakingContext", "entityModelSet"), materialSetOf = method("SpecialModelRenderer.BakingContext", "materials"), skinCache = method("SpecialModelRenderer.BakingContext", "playerSkinRenderCache");
    Object specialBaking = Proxy.newProxyInstance(loader, new Class<?>[] { bakingContext }, (proxy, m, a) -> {
      if (asked(m, entityModelSet)) return models;
      if (asked(m, materialSetOf)) return atlasesFinal;
      if (asked(m, skinCache)) return skins;
      throw new UnsupportedOperationException("The special renderers' baking asked " + m);
    });
    specials = method("SpecialBlockModelRenderer", "vanilla", bakingContext).invoke(null, specialBaking);

    McbenchBlockEntities.game(client);
    // The game's debug screen entries, none of them on (an entity renderer
    // asks whether hitboxes are shown).
    Object game = null;
    for (Field f : type("Minecraft").getDeclaredFields()) if (Modifier.isStatic(f.getModifiers()) && f.getType() == type("Minecraft")) {
      f.setAccessible(true);
      game = f.get(null);
    }
    Object debug = McbenchBlockEntities.allocate(type("DebugScreenEntryList"));
    set(debug, "DebugScreenEntryList", "currentlyEnabled", List.of());
    set(game, "Minecraft", "debugEntries", debug);
    // The game's options, of which an entity renderer asks only whether
    // entity shadows are drawn: no, as neither view draws them.
    Object options = McbenchBlockEntities.allocate(type("Options"));
    Object shadows = McbenchBlockEntities.allocate(type("OptionInstance"));
    set(shadows, "OptionInstance", "value", false);
    set(options, "Options", "entityShadows", shadows);
    set(game, "Minecraft", "options", options);
    // The player the renderers ask whether an invisible entity shows to:
    // one in survival (not a spectator) on no team.
    Object player = standIn(type("LocalPlayer"), Map.of(method("Player", "isSpectator"), (a) -> false));
    set(game, "Minecraft", "player", player);
    Object dispatcher = McbenchBlockEntities.allocate(type("EntityRenderDispatcher"));
    // The block renderer's model shaper: each block state's model, baked by
    // the client's ModelBakery the first time it is asked for.
    Class<?> shaperType = type("BlockModelShaper");
    Object shaper = standIn(shaperType, Map.of(method("BlockModelShaper", "getBlockModel", client.states.get(0).getClass()), (a) -> {
      try {
        return bakeState(a[1]);
      } catch (Exception error) {
        throw new IllegalStateException(error);
      }
    }));
    Object blocks = McbenchBlockEntities.allocate(type("BlockRenderDispatcher"));
    set(blocks, "BlockRenderDispatcher", "blockModelShaper", shaper);
    context = type("EntityRendererProvider.Context").getConstructors()[0].newInstance(dispatcher, resolver, McbenchBlockEntities.allocate(type("MapRenderer")), blocks, resources, models, equipment, atlases, font, skins);

    // The level the entity is in: air everywhere, at game time 0; asked
    // anything else it answers its type's zero.
    Class<?> levelType = type("Level");
    Class<?> blockState = client.states.get(0).getClass();
    Object features = field("FeatureFlags", "DEFAULT_FLAGS").get(null);
    damageSources = type("DamageSources").getConstructor(type("RegistryAccess")).newInstance(registryAccess);
    scoreboard = type("Scoreboard").getConstructor().newInstance();
    level = McbenchBlockEntities.level(client, levelType, (m, a) -> {
      if (m.getReturnType() == blockState && a.length == 1 && client.blockPos.isInstance(a[0])) return client.states.get(0);
      if (m.getReturnType() == features.getClass()) return features;
      if (m.getReturnType() == registryAccess.getClass().getInterfaces()[0]) return registryAccess;
      if (m.getReturnType() == damageSources.getClass()) return damageSources;
      // getScoreboard: an empty Scoreboard (no entity is on a team).
      if (m.getReturnType() == scoreboard.getClass()) return scoreboard;
      // getBrightness(LightLayer, pos): the light this program sets.
      if (m.getReturnType() == int.class && a.length == 2 && a[0] instanceof Enum<?> layer && client.blockPos.isInstance(a[1])) return light.apply(layer.ordinal(), a[1]);
      Class<?> r = m.getReturnType();
      if (r == boolean.class) return false;
      if (r == int.class) return 0;
      if (r == long.class) return 0L;
      if (r == float.class) return 0f;
      if (r == double.class) return 0d;
      if (r == byte.class) return (byte) 0;
      if (r == short.class) return (short) 0;
      if (r == char.class) return (char) 0;
      return null;
    });
    for (Object constant : type("EntitySpawnReason").getEnumConstants()) if (((Enum<?>) constant).name().equals("LOAD")) reason = constant;
  }

  // A block state's model, as the client bakes it.
  Object bakeState(Object state) throws Exception {
    Object known = bakedStates.get(state);
    if (known != null) return known;
    Object root = roots.get(state);
    if (root == null) throw new IllegalStateException("No blockstate file draws " + state);
    Class<?> bakery = type("ModelBakery");
    Object bake = bakery.getConstructors()[0].newInstance(bakeModels, bakeMaterials, bakeSkins, Map.of(state, root), Map.of(), bakeResolved, bakeMissing);
    Object baked = ((CompletableFuture<?>) method("ModelBakery", "bakeModels", type("SpriteGetter"), Executor.class).invoke(bake, spriteGetterProxy, (Executor) Runnable::run)).join();
    Object model = ((Map<?, ?>) method("ModelBakery.BakingResult", "blockStateModels").invoke(baked)).get(state);
    bakedStates.put(state, model);
    return model;
  }

  // One quad as drawn: its corners (x, y, z four times), texture
  // coordinates (u, v four times) over its texture, colour (ARGB), the light
  // of each corner (packed as LightTexture packs it) and the model part it
  // is of ("" for none).
  record Quad(float[] position, float[] uv, int color, int[] light, String part, String texture) {}

  // What one submit drew. kind: model (a model or model part, or a block
  // model's parts), custom (geometry drawn by the renderer), item (an item's
  // baked quads), block (a block state, drawn as the client draws a block
  // alone) or moving (a block state drawn as a falling block). pose: the pose
  // it was submitted under (column major). For model, custom and item:
  // quads in the entity's space, each with its texture and colour (an item's
  // in its tint index's tint), and the render type's name; light and overlay
  // as submitted (-1 for none); tint the model's colour; state: a block's
  // state, an item's display context; parts: a model's parts.
  record Draw(String kind, float[] pose, List<Quad> quads, String renderType, int light, int overlay, int tint, String state, Map<String, Part> parts) {}

  // A model part as drawn: its parent's path, its pose (ModelPart's x, y, z,
  // xRot, yRot, zRot, xScale, yScale, zScale) and its own cubes' quads in its
  // own space (each cube compiled under no pose).
  record Part(String parent, float[] pose, List<Quad> quads) {}

  float[] matrix(Object pose) throws Exception {
    Object last = method("PoseStack", "last").invoke(pose);
    Object m = method("PoseStack.Pose", "pose").invoke(last);
    float[] out = new float[16];
    m.getClass().getMethod("get", float[].class).invoke(m, (Object) out);
    return out;
  }

  // The vertices a model or a piece of custom geometry draws, into a
  // VertexConsumer of this program's: VertexConsumer.addVertex(x, y, z,
  // colour, u, v, overlay, light, normal) as a model's cubes call it, or its
  // builder (addVertex, then the colour, uv, light and normal setters) as
  // custom geometry does.
  final class Vertices implements InvocationHandler {
    final List<float[]> positions = new ArrayList<>();
    final List<float[]> uvs = new ArrayList<>();
    final List<Integer> colors = new ArrayList<>();
    final List<Integer> lights = new ArrayList<>();
    Object proxy;

    @Override public Object invoke(Object self, Method m, Object[] a) throws Throwable {
      if (asked(m, addFullVertex)) {
        positions.add(new float[] { (float) a[0], (float) a[1], (float) a[2] });
        colors.add((int) a[3]);
        uvs.add(new float[] { (float) a[4], (float) a[5] });
        lights.add((int) a[7]);
        return null;
      }
      if (m.isDefault()) return InvocationHandler.invokeDefault(self, m, a);
      // The abstract builder methods: addVertex(x, y, z), setColor, setUv,
      // setUv2 (light), and setUv1 (overlay) and setNormal, not kept.
      if (asked(m, addVertex)) {
        positions.add(new float[] { (float) a[0], (float) a[1], (float) a[2] });
        colors.add(-1);
        uvs.add(new float[2]);
        lights.add(0);
      } else if (asked(m, setColor)) colors.set(colors.size() - 1, (int) a[3] << 24 | (int) a[0] << 16 | (int) a[1] << 8 | (int) a[2]);
      else if (asked(m, setUv)) uvs.set(uvs.size() - 1, new float[] { (float) a[0], (float) a[1] });
      else if (asked(m, setUv2)) lights.set(lights.size() - 1, (int) a[0] | (int) a[1] << 16);
      return self;
    }

    List<Quad> quads(String part, String texture) {
      if (positions.size() % 4 != 0) throw new IllegalStateException("Drawn " + positions.size() + " vertices, not quads");
      List<Quad> quads = new ArrayList<>();
      for (int v = 0; v < positions.size(); v += 4) {
        float[] position = new float[12], uv = new float[8];
        int[] light = new int[4];
        for (int k = 0; k < 4; k++) {
          System.arraycopy(positions.get(v + k), 0, position, k * 3, 3);
          uv[k * 2] = uvs.get(v + k)[0];
          uv[k * 2 + 1] = uvs.get(v + k)[1];
          light[k] = lights.get(v + k);
        }
        quads.add(new Quad(position, uv, colors.get(v), light, part, texture));
      }
      return quads;
    }
  }

  // Quads whose texture coordinates are over this program's atlas, each
  // told its texture and its own coordinates.
  List<Quad> decoded(List<Quad> quads) {
    List<Quad> out = new ArrayList<>();
    for (Quad quad : quads) {
      String texture = null;
      float[] uv = new float[8];
      for (int k = 0; k < 4; k++) {
        Texel texel = decode(quad.uv()[k * 2], quad.uv()[k * 2 + 1]);
        if (texture != null && !texture.equals(texel.texture())) throw new IllegalStateException("A quad over two textures: " + texture + ", " + texel.texture());
        texture = texel.texture();
        uv[k * 2] = texel.u();
        uv[k * 2 + 1] = texel.v();
      }
      out.add(new Quad(quad.position(), uv, quad.color(), quad.light(), quad.part(), texture));
    }
    return out;
  }

  // A baked quad's corners and texture coordinates (BakedQuad's vertices:
  // x, y, z as float bits, then colour, then u, v).
  // `tint` gives the colour of a quad of a tint index (-1: of none).
  List<Quad> baked(List<?> bakedQuads, int light, java.util.function.IntUnaryOperator tint) throws Exception {
    List<Quad> quads = new ArrayList<>();
    for (Object quad : bakedQuads) {
      int[] data = (int[]) method("BakedQuad", "vertices").invoke(quad);
      int tintIndex = (int) method("BakedQuad", "tintIndex").invoke(quad);
      int stride = data.length / 4;
      float[] position = new float[12], uv = new float[8];
      for (int k = 0; k < 4; k++) {
        for (int axis = 0; axis < 3; axis++) position[k * 3 + axis] = Float.intBitsToFloat(data[k * stride + axis]);
        uv[k * 2] = Float.intBitsToFloat(data[k * stride + 4]);
        uv[k * 2 + 1] = Float.intBitsToFloat(data[k * stride + 5]);
      }
      quads.add(new Quad(position, uv, tint.applyAsInt(tintIndex), new int[] { light, light, light, light }, "", null));
    }
    // A model's quads come out of the client's bake in an order that
    // differs from one bake to the next (an item's front before its back, or
    // after): they are kept in the order of their corners and coordinates,
    // so the capture is the same each time.
    List<Quad> out = new ArrayList<>(decoded(quads));
    out.sort((a, b) -> {
      int byPosition = Arrays.compare(a.position(), b.position());
      return byPosition != 0 ? byPosition : Arrays.compare(a.uv(), b.uv());
    });
    return out;
  }

  Method addFullVertex, addVertex, setColor, setUv, setUv2;

  Vertices vertices() throws Exception {
    if (addVertex == null) {
      addFullVertex = method("VertexConsumer", "addVertex", float.class, float.class, float.class, int.class, float.class, float.class, int.class, int.class, float.class, float.class, float.class);
      addVertex = method("VertexConsumer", "addVertex", float.class, float.class, float.class);
      setColor = method("VertexConsumer", "setColor", int.class, int.class, int.class, int.class);
      setUv = method("VertexConsumer", "setUv", float.class, float.class);
      setUv2 = method("VertexConsumer", "setUv2", int.class, int.class);
    }
    Vertices handler = new Vertices();
    handler.proxy = Proxy.newProxyInstance(loader, new Class<?>[] { type("VertexConsumer") }, handler);
    return handler;
  }

  // Everything a renderer submits for one render state: the draws, in the
  // order submitted.
  List<Draw> draw(Object renderer, Object state, Object camera) throws Exception {
    List<Draw> draws = new ArrayList<>();
    Object pose = poseType.getConstructor().newInstance();
    // As EntityRenderDispatcher.submit draws an entity: under its renderer's
    // render offset (EntityRenderer.getRenderOffset: an item frame's, toward
    // its wall), from the entity's position.
    Object offset = method("EntityRenderer", "getRenderOffset", type("EntityRenderState")).invoke(renderer, state);
    method("PoseStack", "translate", double.class, double.class, double.class).invoke(pose, get(offset, "Vec3", "x"), get(offset, "Vec3", "y"), get(offset, "Vec3", "z"));
    Object[] collector = new Object[1];
    collector[0] = Proxy.newProxyInstance(loader, new Class<?>[] { collectorType }, (proxy, m, a) -> {
      if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, a);
      Object[] args = a == null ? new Object[0] : a;
      Class<?>[] p = m.getParameterTypes();
      // order(int): the collector itself.
      if (p.length == 1 && p[0] == int.class) return proxy;
      submitted(draws, m, args, collector[0]);
      return null;
    });
    submit.invoke(renderer, state, pose, collector[0], camera);
    return draws;
  }

  void submitted(List<Draw> draws, Method m, Object[] a, Object collector) throws Exception {
    Class<?>[] p = m.getParameterTypes();
    String name = p.length + ":" + Arrays.stream(p).map(Class::getSimpleName).toList();
    Class<?> modelType = type("Model"), partType = type("ModelPart");
    if (p[0] == modelType) {
      // submitModel(model, state, pose, renderType, light, overlay, tint,
      // sprite, outline, crumbling)
      Object model = a[0];
      for (Method setup : model.getClass().getMethods()) {
        if (setup.getParameterCount() == 1 && setup.getParameterTypes()[0] == Object.class && setup.getReturnType() == void.class) {
          setup.setAccessible(true);
          setup.invoke(model, a[1]);
        }
      }
      Object root = get(model, "Model", "root");
      String texture = texture(a[7], a[3]);
      draws.add(new Draw("model", matrix(a[2]), modelQuads(root, a[2], (int) a[4], (int) a[5], (int) a[6], texture), renderTypeName(a[3]), (int) a[4], (int) a[5], (int) a[6], null, parts(root, (int) a[4], (int) a[5], (int) a[6], texture)));
    } else if (p[0] == partType) {
      // submitModelPart(part, pose, renderType, light, overlay, sprite,
      // sheeted, foil, tint, crumbling, outline)
      String texture = texture(a[5], a[2]);
      draws.add(new Draw("model", matrix(a[1]), modelQuads(a[0], a[1], (int) a[3], (int) a[4], (int) a[8], texture), renderTypeName(a[2]), (int) a[3], (int) a[4], (int) a[8], null, parts(a[0], (int) a[3], (int) a[4], (int) a[8], texture)));
    } else if (p.length == 5 && p[1] == client.states.get(0).getClass()) {
      // submitBlock(pose, state, light, overlay, outline): the block's model
      // (BlockRenderDispatcher.renderSingleBlock), and what the special block
      // renderer draws of its block, as the client's collector submits it.
      draws.add(new Draw("block", matrix(a[0]), List.of(), null, (int) a[2], (int) a[3], -1, McbenchClient.name(a[1]), null));
      Object block = null;
      for (Method get : a[1].getClass().getMethods()) if (get.getParameterCount() == 0 && get.getReturnType() == type("Block")) block = get.invoke(a[1]);
      Object none = null;
      for (Object context : type("ItemDisplayContext").getEnumConstants()) if (((Enum<?>) context).name().equals("NONE")) none = context;
      method("SpecialBlockModelRenderer", "renderByBlock", type("Block"), type("ItemDisplayContext"), poseType, collectorType, int.class, int.class, int.class).invoke(specials, block, none, a[0], collector, a[2], a[3], a[4]);
    } else if (p.length == 2 && p[1] == type("MovingBlockRenderState")) {
      draws.add(new Draw("moving", matrix(a[0]), List.of(), null, -1, -1, -1, McbenchClient.name(get(a[1], "MovingBlockRenderState", "blockState")), null));
    } else if (p.length == 9 && p[2] == type("BlockStateModel")) {
      // submitBlockModel(pose, renderType, model, r, g, b, light, overlay,
      // outline): the quads of the model's parts as ModelBlockRenderer's
      // renderModel draws them (collectParts of RandomSource.create(42), each
      // part's quads toward each direction and toward none), the tinted ones
      // in the colour given.
      Object random = null;
      for (Method create : type("RandomSource").getMethods()) if (Modifier.isStatic(create.getModifiers()) && create.getReturnType() == type("RandomSource") && List.of(create.getParameterTypes()).equals(List.of(long.class))) random = create.invoke(null, 42L);
      List<Object> parts = new ArrayList<>();
      method("BlockStateModel", "collectParts", type("RandomSource"), List.class).invoke(a[2], random, parts);
      int tint = 0xff000000 | Math.round((float) a[3] * 255) << 16 | Math.round((float) a[4] * 255) << 8 | Math.round((float) a[5] * 255);
      List<Quad> quads = new ArrayList<>();
      List<Object> directions = new ArrayList<>(Arrays.asList(type("Direction").getEnumConstants()));
      directions.add(null);
      for (Object part : parts) for (Object direction : directions) quads.addAll(baked((List<?>) method("BlockModelPart", "getQuads", type("Direction")).invoke(part, direction), (int) a[6], (index) -> index == -1 ? -1 : tint));
      draws.add(new Draw("model", matrix(a[0]), transform(quads, matrix(a[0])), renderTypeName(a[1]), (int) a[6], (int) a[7], tint, null, null));
    } else if (p.length == 9 && p[5] == int[].class) {
      // submitItem(pose, context, light, overlay, outline, tints, quads,
      // renderType, foil)
      // Each quad in the tint of its tint index (white past the tints').
      int[] tints = (int[]) a[5];
      List<Quad> quads = transform(baked((List<?>) a[6], (int) a[2], (index) -> index >= 0 && index < tints.length ? tints[index] : -1), matrix(a[0]));
      draws.add(new Draw("item", matrix(a[0]), quads, renderTypeName(a[7]), (int) a[2], (int) a[3], -1, ((Enum<?>) a[1]).name().toLowerCase(java.util.Locale.ROOT), null));
    } else if (p.length == 3 && p[2] == type("SubmitNodeCollector.CustomGeometryRenderer")) {
      // submitCustomGeometry(pose, renderType, renderer)
      Vertices vertices = vertices();
      Object last = method("PoseStack", "last").invoke(a[0]);
      Method render = type("SubmitNodeCollector.CustomGeometryRenderer").getMethods()[0];
      render.setAccessible(true);
      render.invoke(a[2], last, vertices.proxy);
      // Over an atlas (a render type of an atlas, by its texture or its
      // definition) its sprites' coordinates; else over the render type's
      // texture.
      String named = String.valueOf(a[1]).replaceFirst("(?s)^.*?texture\\[Optional\\[([^\\]]+)\\].*$", "$1");
      boolean atlas = false;
      for (Object config : (List<?>) field("AtlasManager", "KNOWN_ATLASES").get(null)) if (get(config, "AtlasManager.AtlasConfig", "textureId").toString().equals(named) || get(config, "AtlasManager.AtlasConfig", "definitionLocation").toString().equals(named)) atlas = true;
      String texture = atlas ? null : McbenchBlockEntities.texture(null, a[1]);
      List<Quad> quads = vertices.quads("", atlas ? null : texture);
      draws.add(new Draw("custom", matrix(a[0]), atlas ? decoded(quads) : quads, renderTypeName(a[1]), -1, -1, -1, null, null));
    } else if (p.length == 3 && p[1] == type("EntityRenderState")) {
      // submitFlame (an entity on fire) and the debug hitbox: not drawn.
      throw new UnsupportedOperationException("A renderer submitted " + name);
    } else {
      throw new UnsupportedOperationException("A renderer submitted " + name);
    }
  }

  // Quads' corners under a pose (column major).
  static List<Quad> transform(List<Quad> quads, float[] m) {
    List<Quad> out = new ArrayList<>();
    for (Quad quad : quads) {
      float[] p = quad.position(), q = new float[12];
      for (int k = 0; k < 4; k++) {
        float x = p[k * 3], y = p[k * 3 + 1], z = p[k * 3 + 2];
        q[k * 3] = m[0] * x + m[4] * y + m[8] * z + m[12];
        q[k * 3 + 1] = m[1] * x + m[5] * y + m[9] * z + m[13];
        q[k * 3 + 2] = m[2] * x + m[6] * y + m[10] * z + m[14];
      }
      out.add(new Quad(q, quad.uv(), quad.color(), quad.light(), quad.part(), quad.texture()));
    }
    return out;
  }

  String texture(Object sprite, Object renderType) {
    // A render type of no texture (the boat's water mask, which writes only
    // depth, so water is not drawn inside the boat): "".
    if (sprite == null && String.valueOf(renderType).contains("[texture, ")) return "";
    if (sprite != null) {
      String texture = sprites.get(sprite);
      if (texture == null) throw new IllegalStateException("A sprite not made here: " + sprite);
      return texture;
    }
    return McbenchBlockEntities.texture(null, renderType);
  }

  // A render type's name (RenderType's toString begins "RenderType[name:").
  static String renderTypeName(Object renderType) {
    String text = String.valueOf(renderType);
    var match = java.util.regex.Pattern.compile("^RenderType\\[([a-z_]+):").matcher(text);
    if (!match.find()) throw new IllegalStateException("Not a render type: " + text);
    return match.group(1);
  }

  // A model part's quads under `pose`, drawn by Minecraft's own
  // ModelPart.render, each told its part by ModelPart.visit (the path of the
  // part whose cube drew the same corners).
  List<Quad> modelQuads(Object root, Object pose, int light, int overlay, int color, String texture) throws Exception {
    Class<?> partType = type("ModelPart");
    Vertices drawn = vertices();
    method("ModelPart", "render", poseType, type("VertexConsumer"), int.class, int.class, int.class).invoke(root, pose, drawn.proxy, light, overlay, color);
    Map<String, String> parts = new HashMap<>();
    Object visitor = Proxy.newProxyInstance(loader, new Class<?>[] { type("ModelPart.Visitor") }, (proxy, m, a) -> {
      Vertices cube = vertices();
      method("ModelPart.Cube", "compile", type("PoseStack.Pose"), type("VertexConsumer"), int.class, int.class, int.class).invoke(a[3], a[0], cube.proxy, light, overlay, color);
      for (Quad quad : cube.quads("", null)) parts.put(Arrays.toString(quad.position()), (String) a[1]);
      return null;
    });
    // visit walks from the pose it is given; drawn from a copy of it.
    Object copy = poseType.getConstructor().newInstance();
    Object matrix = cls("org.joml.Matrix4f").getConstructor().newInstance();
    matrix.getClass().getMethod("set", float[].class).invoke(matrix, (Object) matrix(pose));
    method("PoseStack", "mulPose", cls("org.joml.Matrix4fc")).invoke(copy, matrix);
    method("ModelPart", "visit", poseType, type("ModelPart.Visitor")).invoke(root, copy, visitor);
    List<Quad> quads = new ArrayList<>();
    for (Quad quad : drawn.quads("", texture)) {
      String part = parts.get(Arrays.toString(quad.position()));
      if (part == null) throw new IllegalStateException("A model drew a quad of no part");
      quads.add(new Quad(quad.position(), quad.uv(), quad.color(), quad.light(), part, texture));
    }
    return quads;
  }

  // A model's parts as Minecraft draws them (ModelPart.render: a part not
  // visible is not drawn, nor its children; one that skips drawing draws only
  // its children), by path as ModelPart.visit names them.
  Map<String, Part> parts(Object root, int light, int overlay, int color, String texture) throws Exception {
    Map<String, Part> parts = new LinkedHashMap<>();
    walk(root, "", null, parts, light, overlay, color, texture);
    return parts;
  }

  // A part that is not drawn is kept with no quads, as a layer may still
  // draw at its pose (an item in the hand of an armor stand without arms).
  void walk(Object part, String path, String parent, Map<String, Part> parts, int light, int overlay, int color, String texture) throws Exception {
    walk(part, path, parent, parts, light, overlay, color, texture, true);
  }

  void walk(Object part, String path, String parent, Map<String, Part> parts, int light, int overlay, int color, String texture, boolean drawn) throws Exception {
    List<?> cubes = (List<?>) get(part, "ModelPart", "cubes");
    Map<?, ?> children = (Map<?, ?>) get(part, "ModelPart", "children");
    if (cubes.isEmpty() && children.isEmpty()) return;
    drawn &= (boolean) get(part, "ModelPart", "visible");
    float[] pose = new float[9];
    String[] poseFields = { "x", "y", "z", "xRot", "yRot", "zRot", "xScale", "yScale", "zScale" };
    for (int i = 0; i < 9; i++) pose[i] = (float) get(part, "ModelPart", poseFields[i]);
    List<Quad> quads = new ArrayList<>();
    if (drawn && !(boolean) get(part, "ModelPart", "skipDraw")) {
      Object identity = type("PoseStack.Pose").getConstructor().newInstance();
      for (Object cube : cubes) {
        Vertices compiled = vertices();
        method("ModelPart.Cube", "compile", type("PoseStack.Pose"), type("VertexConsumer"), int.class, int.class, int.class).invoke(cube, identity, compiled.proxy, light, overlay, color);
        quads.addAll(compiled.quads(path, texture));
      }
    }
    parts.put(path, new Part(parent, pose, quads));
    for (var child : children.entrySet()) walk(child.getValue(), path + "/" + child.getKey(), path, parts, light, overlay, color, texture, drawn);
  }

  // ---- rendering one entity ----

  final Map<String, Object> rendererOf = new HashMap<>();
  static final int ENTITY_ID = 1;
  int id = ENTITY_ID;

  Object entityType(String type) throws Exception {
    return ((java.util.Optional<?>) method("EntityType", "byString", String.class).invoke(null, type)).orElseThrow(() -> new IllegalStateException("No entity type " + type));
  }

  Object renderer(String type) throws Exception {
    Object known = rendererOf.get(type);
    if (known == null) rendererOf.put(type, known = McbenchBlockEntities.create(providers.get(entityType(type)), context));
    return known;
  }

  // An entity of `type` at the origin, read from saved data (SNBT) as the
  // client reads an entity (Entity.load).
  Object entity(String type, String saved) throws Exception {
    // Every entity made here takes the same id (Entity.ENTITY_COUNTER, the
    // one static AtomicInteger of Entity, set back), which a renderer may
    // read (a minecart's offset of its own).
    for (Field counter : type("Entity").getDeclaredFields()) {
      if (Modifier.isStatic(counter.getModifiers()) && counter.getType() == java.util.concurrent.atomic.AtomicInteger.class) {
        counter.setAccessible(true);
        ((java.util.concurrent.atomic.AtomicInteger) counter.get(null)).set(id - 1);
      }
    }
    Object entity = method("EntityType", "create", type("Level"), type("EntitySpawnReason")).invoke(entityType(type), level, reason);
    Object compound = method("TagParser", "parseCompoundFully", String.class).invoke(null, saved);
    Object input = method("TagValueInput", "create", type("ProblemReporter"), type("HolderLookup.Provider"), type("CompoundTag")).invoke(null, field("ProblemReporter", "DISCARDING").get(null), registryAccess, compound);
    method("Entity", "load", type("ValueInput")).invoke(entity, input);
    return entity;
  }

  // What the renderer draws of the entity, at partial tick 0, in the light
  // the level answers.
  List<Draw> render(String type, String saved, Object camera) throws Exception {
    Object entity = entity(type, saved);
    Object state = createState.invoke(renderer(type), entity, 0f);
    adjust.accept(state);
    return draw(renderer(type), state, camera);
  }

  // What is set on a render state after it is extracted (an item's bob
  // offset, which the client draws at random).
  java.util.function.Consumer<Object> adjust = (state) -> {};

  List<Draw> render(String type, String saved) throws Exception {
    return render(type, saved, cameraType.getConstructor().newInstance());
  }

  // ---- matrices (column major, as JOML's get(float[])) ----

  static float[] multiply(float[] a, float[] b) {
    float[] out = new float[16];
    for (int c = 0; c < 4; c++) for (int r = 0; r < 4; r++) {
      float sum = 0;
      for (int k = 0; k < 4; k++) sum += a[k * 4 + r] * b[c * 4 + k];
      out[c * 4 + r] = sum;
    }
    return out;
  }

  // The inverse of an affine matrix.
  static float[] invert(float[] m) {
    double a = m[0], b = m[4], c = m[8], d = m[1], e = m[5], f = m[9], g = m[2], h = m[6], i = m[10];
    double det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    if (Math.abs(det) < 1e-12) throw new IllegalStateException("A singular pose");
    double[] inv = { (e * i - f * h) / det, (c * h - b * i) / det, (b * f - c * e) / det, (f * g - d * i) / det, (a * i - c * g) / det, (c * d - a * f) / det, (d * h - e * g) / det, (b * g - a * h) / det, (a * e - b * d) / det };
    float[] out = new float[16];
    // inv is row major 3x3.
    for (int r = 0; r < 3; r++) for (int col = 0; col < 3; col++) out[col * 4 + r] = (float) inv[r * 3 + col];
    for (int r = 0; r < 3; r++) out[12 + r] = (float) -(inv[r * 3] * m[12] + inv[r * 3 + 1] * m[13] + inv[r * 3 + 2] * m[14]);
    out[15] = 1;
    return out;
  }

  static float difference(float[] a, float[] b) {
    float most = 0;
    for (int i = 0; i < a.length; i++) most = Math.max(most, Math.abs(a[i] - b[i]));
    return most;
  }

  static final float[] IDENTITY = { 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1 };

  // A rotation of `degrees` about the line through `point` along the unit
  // `axis`.
  static float[] rotation(double[] axis, double[] point, double degrees) {
    double t = Math.toRadians(degrees), c = Math.cos(t), sn = Math.sin(t), x = axis[0], y = axis[1], z = axis[2];
    double[][] r = {
      { c + x * x * (1 - c), x * y * (1 - c) - z * sn, x * z * (1 - c) + y * sn },
      { y * x * (1 - c) + z * sn, c + y * y * (1 - c), y * z * (1 - c) - x * sn },
      { z * x * (1 - c) - y * sn, z * y * (1 - c) + x * sn, c + z * z * (1 - c) } };
    float[] m = new float[16];
    for (int row = 0; row < 3; row++) for (int col = 0; col < 3; col++) m[col * 4 + row] = (float) r[row][col];
    for (int row = 0; row < 3; row++) m[12 + row] = (float) (point[row] - (r[row][0] * point[0] + r[row][1] * point[1] + r[row][2] * point[2]));
    m[15] = 1;
    return m;
  }

  static float[] translation(double[] v, double amount) {
    float[] m = IDENTITY.clone();
    for (int i = 0; i < 3; i++) m[12 + i] = (float) (v[i] * amount);
    return m;
  }

  // What a change of one value does to a pose, from the pose before
  // (`from`) and after (`to`) a change of `by`: a rotation about a line,
  // turning `rate` degrees for each unit of the value (axis made to point
  // with its largest component positive), or a translation of `vector` per
  // unit. Null when it does nothing; fails for anything else.
  record Motion(String value, double[] axis, double[] point, double rate, double[] vector) {
    float[] matrix(double amount) {
      return axis != null ? rotation(axis, point, rate * amount) : translation(vector, amount);
    }
  }

  static Motion motion(String value, float[] from, float[] to, double by) {
    float[] m = multiply(to, invert(from));
    double[][] r = new double[3][3];
    for (int row = 0; row < 3; row++) for (int col = 0; col < 3; col++) r[row][col] = m[col * 4 + row];
    double[] t = { m[12], m[13], m[14] };
    double trace = r[0][0] + r[1][1] + r[2][2];
    double[] w = { r[2][1] - r[1][2], r[0][2] - r[2][0], r[1][0] - r[0][1] };
    double angle = Math.atan2(Math.sqrt(w[0] * w[0] + w[1] * w[1] + w[2] * w[2]) / 2, (trace - 1) / 2);
    if (angle < 1e-5) {
      if (Math.abs(t[0]) + Math.abs(t[1]) + Math.abs(t[2]) < 1e-6) return null;
      return new Motion(value, null, null, 0, new double[] { t[0] / by, t[1] / by, t[2] / by });
    }
    double[] axis = { r[2][1] - r[1][2], r[0][2] - r[2][0], r[1][0] - r[0][1] };
    double length = Math.sqrt(axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]);
    for (int i = 0; i < 3; i++) axis[i] /= length;
    int largest = 0;
    for (int i = 1; i < 3; i++) if (Math.abs(axis[i]) > Math.abs(axis[largest])) largest = i;
    double sign = axis[largest] < 0 ? -1 : 1;
    for (int i = 0; i < 3; i++) axis[i] *= sign;
    double degrees = Math.toDegrees(angle) * sign;
    // A rotation moves nothing along its axis.
    if (Math.abs(axis[0] * t[0] + axis[1] * t[1] + axis[2] * t[2]) > 1e-4) throw new IllegalStateException("A change of " + value + " that turns and moves along the turn: " + Arrays.toString(m) + " from " + Arrays.toString(from) + " to " + Arrays.toString(to));
    // The point on the axis nearest the origin: (I - R) p = t, a . p = 0.
    double[][] a = new double[3][3];
    for (int row = 0; row < 3; row++) for (int col = 0; col < 3; col++) a[row][col] = (row == col ? 1 : 0) - r[row][col] + axis[row] * axis[col];
    double[] point = solve(a, t);
    return new Motion(value, axis, point, degrees / by, null);
  }

  static double[] solve(double[][] a, double[] b) {
    double det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0]) + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
    double[] out = new double[3];
    for (int k = 0; k < 3; k++) {
      double[][] m = { a[0].clone(), a[1].clone(), a[2].clone() };
      for (int row = 0; row < 3; row++) m[row][k] = b[row];
      out[k] = (m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])) / det;
    }
    return out;
  }

  // ModelPart.translateAndRotate of a pose (x, y, z, xRot, yRot, zRot and
  // the scales): the translation by x, y, z sixteenths, then the rotation
  // Quaternionf.rotationZYX(zRot, yRot, xRot), then the scale.
  static float[] partMatrix(float[] p) {
    double cx = Math.cos(p[3]), sx = Math.sin(p[3]), cy = Math.cos(p[4]), sy = Math.sin(p[4]), cz = Math.cos(p[5]), sz = Math.sin(p[5]);
    // Rz * Ry * Rx, row major.
    double[][] r = {
      { cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx },
      { sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx },
      { -sy, cy * sx, cy * cx } };
    float[] m = new float[16];
    for (int row = 0; row < 3; row++) for (int col = 0; col < 3; col++) m[col * 4 + row] = (float) (r[row][col] * p[6 + col]);
    m[12] = p[0] / 16;
    m[13] = p[1] / 16;
    m[14] = p[2] / 16;
    m[15] = 1;
    return m;
  }

  // ---- what varies, from what the renderer draws ----

  // The values an entity's look varies with continuously, each with the
  // saved data it is read from: `saved` makes the entity's SNBT of values
  // by name. A variant is fitted at `base` by a change of each value by
  // `step`, and checked at `check`, every value changed at once.
  record Values(java.util.function.Function<Map<String, Double>, String> saved, Map<String, Double> base, Map<String, Double> step, Map<String, Double> check) {}

  // One draw of a variant: the draw at the base values, the motions of its
  // pose by value (outermost first), each model part's pose's change per
  // unit of each value, and for a draw that follows a model part, the part
  // and its pose in that part's space.
  record Fitted(Draw draw, List<Motion> motions, Map<String, Map<String, float[]>> deps, int attachDraw, String attachPart, float[] attachMatrix) {}

  // The pose of each part of a model draw (by path, the composition from the
  // root of each part's translateAndRotate) under the draw's pose.
  static Map<String, float[]> partPoses(float[] pose, Map<String, Part> parts, Map<String, float[]> fields) {
    Map<String, float[]> out = new HashMap<>();
    for (var entry : parts.entrySet()) {
      float[] m = partMatrix(fields.get(entry.getKey()));
      for (String at = entry.getValue().parent(); at != null; at = parts.get(at).parent()) m = multiply(partMatrix(fields.get(at)), m);
      out.put(entry.getKey(), multiply(pose, m));
    }
    return out;
  }

  static Map<String, float[]> fields(Map<String, Part> parts) {
    Map<String, float[]> out = new HashMap<>();
    parts.forEach((path, part) -> out.put(path, part.pose()));
    return out;
  }

  static float[] moved(List<Motion> motions, Map<String, Double> values, Map<String, Double> base, float[] pose) {
    float[] m = IDENTITY;
    for (Motion motion : motions) m = multiply(m, motion.matrix(values.get(motion.value()) - base.get(motion.value())));
    return multiply(m, pose);
  }

  List<Fitted> fit(String type, Values values) throws Exception {
    List<Draw> base = render(type, values.saved().apply(values.base()));
    // In the values' names' order, so the capture is the same each time.
    Map<String, List<Draw>> stepped = new LinkedHashMap<>();
    for (String value : new java.util.TreeSet<>(values.step().keySet())) {
      Map<String, Double> at = new HashMap<>(values.base());
      at.put(value, at.get(value) + values.step().get(value));
      List<Draw> draws = render(type, values.saved().apply(at));
      if (draws.size() != base.size()) throw new IllegalStateException(type + " draws " + draws.size() + " things at another " + value + ", " + base.size() + " at its base");
      stepped.put(value, draws);
    }
    List<Fitted> fitted = new ArrayList<>();
    for (int d = 0; d < base.size(); d++) {
      Draw draw = base.get(d);
      List<Motion> motions = new ArrayList<>();
      Map<String, Map<String, float[]>> deps = new LinkedHashMap<>();
      boolean followsPart = false;
      for (var entry : stepped.entrySet()) {
        String value = entry.getKey();
        Draw moved = entry.getValue().get(d);
        if (!moved.kind().equals(draw.kind()) || moved.quads().size() != draw.quads().size()) throw new IllegalStateException(type + "'s draw " + d + " changes with " + value);
        Motion motion = motion(value, draw.pose(), moved.pose(), values.step().get(value));
        if (motion != null) motions.add(motion);
        if (draw.parts() != null) {
          for (var part : draw.parts().entrySet()) {
            float[] before = part.getValue().pose(), after = moved.parts().get(part.getKey()).pose();
            float[] rate = new float[9];
            boolean any = false;
            for (int i = 0; i < 9; i++) {
              rate[i] = (float) ((after[i] - before[i]) / values.step().get(value));
              any |= Math.abs(after[i] - before[i]) > 1e-7;
            }
            if (any) deps.computeIfAbsent(part.getKey(), (key) -> new LinkedHashMap<>()).put(value, rate);
          }
        }
      }
      fitted.add(new Fitted(draw, motions, deps, -1, null, null));
    }
    // A draw that is not a model's and moves otherwise than by its own
    // motions follows a part of a model draw: the one under whose pose its
    // pose stays the same at every change.
    for (int d = 0; d < fitted.size(); d++) {
      Fitted f = fitted.get(d);
      boolean rigid = true;
      for (var entry : stepped.entrySet()) {
        float[] predicted = moved(f.motions(), stepAt(values, entry.getKey()), values.base(), f.draw().pose());
        if (difference(predicted, entry.getValue().get(d).pose()) > 1e-4) rigid = false;
      }
      if (rigid) continue;
      found:
      for (int m = 0; m < fitted.size(); m++) {
        Fitted model = fitted.get(m);
        if (m == d || model.draw().parts() == null || model.attachPart() != null) continue;
        Map<String, float[]> poses = partPoses(model.draw().pose(), model.draw().parts(), fields(model.draw().parts()));
        for (String part : model.draw().parts().keySet()) {
          float[] k = multiply(invert(poses.get(part)), f.draw().pose());
          boolean same = true;
          for (var entry : stepped.entrySet()) {
            Draw other = entry.getValue().get(m);
            float[] there = partPoses(other.pose(), other.parts(), fields(other.parts())).get(part);
            if (difference(multiply(there, k), entry.getValue().get(d).pose()) > 1e-4) same = false;
          }
          if (same) {
            if (!f.deps().isEmpty()) throw new IllegalStateException(type + "'s draw " + d + " follows a part and moves its own parts");
            fitted.set(d, new Fitted(f.draw(), List.of(), Map.of(), m, part, k));
            break found;
          }
        }
      }
      if (fitted.get(d).attachPart() == null) throw new IllegalStateException(type + "'s draw " + d + " (" + f.draw().kind() + ") moves with no motion and no part");
      // Found in draw order: a part that follows must be of a draw before.
      if (fitted.get(d).attachDraw() > d) throw new IllegalStateException(type + "'s draw " + d + " follows a later draw");
    }
    // The motions' order: the one that draws the check as Minecraft does.
    List<Draw> checked = render(type, values.saved().apply(values.check()));
    for (int d = 0; d < fitted.size(); d++) {
      Fitted f = fitted.get(d);
      if (f.attachPart() != null) continue;
      List<Motion> best = null;
      for (List<Motion> order : orders(f.motions())) {
        if (difference(moved(order, values.check(), values.base(), f.draw().pose()), checked.get(d).pose()) < 1e-4) best = order;
      }
      if (best == null) throw new IllegalStateException(type + "'s draw " + d + " is drawn at the check where no order of its motions puts it: " + Arrays.toString(checked.get(d).pose()) + " " + f.motions().stream().map((m) -> m.value() + Arrays.toString(m.axis()) + Arrays.toString(m.point()) + m.rate() + Arrays.toString(m.vector())).toList() + " base pose " + Arrays.toString(f.draw().pose()) + " saved " + values.saved().apply(values.base()));
      fitted.set(d, new Fitted(f.draw(), best, f.deps(), -1, null, null));
    }
    // And every quad of the check where the fit puts it.
    for (int d = 0; d < fitted.size(); d++) {
      Fitted f = fitted.get(d);
      Draw truth = checked.get(d);
      float[] pose = pose(fitted, d, values.check(), values.base());
      if (f.draw().parts() != null) {
        // ModelPart.render draws a part's cubes, then its children's: the
        // parts' order here.
        Map<String, float[]> poses = partPoses(pose, f.draw().parts(), checkFields(f, values));
        List<Quad> predicted = new ArrayList<>();
        for (var part : f.draw().parts().entrySet()) predicted.addAll(transform(part.getValue().quads(), poses.get(part.getKey())));
        if (predicted.size() != truth.quads().size()) throw new IllegalStateException(type + "'s draw " + d + " draws " + truth.quads().size() + " quads at the check, the fit " + predicted.size());
        for (int q = 0; q < predicted.size(); q++) {
          float off = difference(predicted.get(q).position(), truth.quads().get(q).position());
          if (off > 1e-3) throw new IllegalStateException(type + "'s part " + truth.quads().get(q).part() + " is drawn at the check " + off + " from where the fit puts it");
        }
      } else if (difference(pose, truth.pose()) > 1e-4) throw new IllegalStateException(type + "'s draw " + d + " is drawn at the check away from where the fit puts it");
    }
    return fitted;
  }

  static Map<String, Double> stepAt(Values values, String value) {
    Map<String, Double> at = new HashMap<>(values.base());
    at.put(value, at.get(value) + values.step().get(value));
    return at;
  }

  static Map<String, float[]> checkFields(Fitted f, Values values) {
    Map<String, float[]> out = new HashMap<>();
    f.draw().parts().forEach((path, part) -> {
      float[] p = part.pose().clone();
      for (var dep : f.deps().getOrDefault(path, Map.of()).entrySet()) {
        double change = values.check().get(dep.getKey()) - values.base().get(dep.getKey());
        for (int i = 0; i < 9; i++) p[i] += (float) (dep.getValue()[i] * change);
      }
      out.put(path, p);
    });
    return out;
  }

  // A fitted draw's pose at `values`.
  static float[] pose(List<Fitted> fitted, int d, Map<String, Double> values, Map<String, Double> base) {
    Fitted f = fitted.get(d);
    if (f.attachPart() == null) return moved(f.motions(), values, base, f.draw().pose());
    Fitted model = fitted.get(f.attachDraw());
    Map<String, float[]> fields = new HashMap<>();
    model.draw().parts().forEach((path, part) -> {
      float[] p = part.pose().clone();
      for (var dep : model.deps().getOrDefault(path, Map.of()).entrySet()) {
        double change = values.get(dep.getKey()) - base.get(dep.getKey());
        for (int i = 0; i < 9; i++) p[i] += (float) (dep.getValue()[i] * change);
      }
      fields.put(path, p);
    });
    return multiply(partPoses(moved(model.motions(), values, base, model.draw().pose()), model.draw().parts(), fields).get(f.attachPart()), f.attachMatrix());
  }

  static List<List<Motion>> orders(List<Motion> motions) {
    if (motions.size() <= 1) return List.of(motions);
    List<List<Motion>> out = new ArrayList<>();
    for (int i = 0; i < motions.size(); i++) {
      List<Motion> rest = new ArrayList<>(motions);
      Motion first = rest.remove(i);
      for (List<Motion> tail : orders(rest)) {
        List<Motion> order = new ArrayList<>(List.of(first));
        order.addAll(tail);
        out.add(order);
      }
    }
    return out;
  }

  static String f(double value) {
    return (float) value + "f";
  }

  // ---- the output ----

  final List<String> textures = new ArrayList<>();
  final Map<String, Integer> textureIds = new HashMap<>();
  final List<float[]> shapes = new ArrayList<>();
  final Map<Values32, Integer> shapeIds = new HashMap<>();
  final List<List<int[]>> meshes = new ArrayList<>();
  final Map<Values32, Integer> meshIds = new HashMap<>();
  final com.google.gson.JsonObject kinds = new com.google.gson.JsonObject();
  final com.google.gson.JsonObject items = new com.google.gson.JsonObject();
  final com.google.gson.JsonObject specialBlocks = new com.google.gson.JsonObject();

  // A pose (a matrix, column major), kept once.
  final List<float[]> poses = new ArrayList<>();
  final Map<Values32, Integer> poseIds = new HashMap<>();

  int pose(float[] matrix) {
    return poseIds.computeIfAbsent(new Values32(matrix, null), (key) -> {
      poses.add(matrix);
      return poses.size() - 1;
    });
  }

  int textureId(String texture) {
    return textureIds.computeIfAbsent(texture, (key) -> {
      textures.add(key);
      return textures.size() - 1;
    });
  }

  // A list of quads, kept once: each quad its shape (its corners and texture
  // coordinates, kept once), texture and colour.
  int mesh(List<Quad> quads) {
    List<int[]> mesh = new ArrayList<>();
    for (Quad quad : quads) {
      if (quad.texture() == null) throw new IllegalStateException("A quad of no texture");
      float[] shape = new float[20];
      System.arraycopy(quad.position(), 0, shape, 0, 12);
      // A quad of no texture (a boat's water mask, whose model's texture
      // size is none) keeps no texture coordinates.
      if (!quad.texture().isEmpty()) System.arraycopy(quad.uv(), 0, shape, 12, 8);
      int id = shapeIds.computeIfAbsent(new Values32(shape, null), (k) -> {
        shapes.add(shape);
        return shapes.size() - 1;
      });
      mesh.add(new int[] { id, textureId(quad.texture()), quad.color() });
    }
    int[] flat = new int[mesh.size() * 3];
    for (int q = 0; q < mesh.size(); q++) System.arraycopy(mesh.get(q), 0, flat, q * 3, 3);
    return meshIds.computeIfAbsent(new Values32(null, flat), (k) -> {
      meshes.add(mesh);
      return meshes.size() - 1;
    });
  }

  // Floats or ints compared by value, as keys.
  record Values32(float[] floats, int[] ints) {
    @Override public boolean equals(Object other) {
      return other instanceof Values32 that && Arrays.equals(floats, that.floats) && Arrays.equals(ints, that.ints);
    }

    @Override public int hashCode() {
      return 31 * Arrays.hashCode(floats) + Arrays.hashCode(ints);
    }
  }

  static com.google.gson.JsonArray array(float[] values) {
    var out = new com.google.gson.JsonArray();
    for (float v : values) out.add(v);
    return out;
  }

  static com.google.gson.JsonArray array(double[] values) {
    var out = new com.google.gson.JsonArray();
    for (double v : values) out.add((float) v);
    return out;
  }

  // A fitted variant as the page reads it (see the header).
  com.google.gson.JsonObject variant(List<Fitted> fitted, Map<String, Double> base, java.util.function.Function<Draw, String> block, Map<Integer, String> slots, Map<Integer, Object> lights) throws Exception {
    var out = new com.google.gson.JsonObject();
    var baseJson = new com.google.gson.JsonObject();
    new java.util.TreeMap<>(base).forEach(baseJson::addProperty);
    out.add("base", baseJson);
    var draws = new com.google.gson.JsonArray();
    for (int d = 0; d < fitted.size(); d++) {
      Fitted f = fitted.get(d);
      Draw draw = f.draw();
      var json = new com.google.gson.JsonObject();
      json.addProperty("kind", draw.parts() != null ? "parts" : slots.containsKey(d) ? "item" : draw.kind().equals("block") || draw.kind().equals("moving") ? draw.kind() : "mesh");
      if (draw.renderType() != null) json.addProperty("layer", draw.renderType());
      // The overlay it was submitted with; none for a draw that takes none
      // (custom geometry, a moving block).
      if (draw.overlay() != -1) json.addProperty("overlay", draw.overlay());
      Object light = lights.get(d);
      if (light instanceof Integer fixed) json.addProperty("light", fixed);
      else if (light instanceof com.google.gson.JsonElement cells) json.add("light", cells);
      else json.addProperty("light", "entity");
      if (f.attachPart() != null) {
        var attach = new com.google.gson.JsonObject();
        attach.addProperty("draw", f.attachDraw());
        attach.addProperty("part", f.attachPart());
        attach.addProperty("matrix", pose(f.attachMatrix()));
        json.add("attach", attach);
      } else {
        json.addProperty("pose", pose(draw.pose()));
        var motions = new com.google.gson.JsonArray();
        for (Motion m : f.motions()) {
          var motion = new com.google.gson.JsonObject();
          motion.addProperty("value", m.value());
          if (m.axis() != null) {
            motion.add("axis", array(m.axis()));
            motion.add("point", array(m.point()));
            motion.addProperty("rate", (float) m.rate());
          } else motion.add("vector", array(m.vector()));
          motions.add(motion);
        }
        json.add("motions", motions);
      }
      if (draw.parts() != null) {
        var parts = new com.google.gson.JsonArray();
        for (var entry : draw.parts().entrySet()) {
          var part = new com.google.gson.JsonObject();
          part.addProperty("path", entry.getKey());
          if (entry.getValue().parent() != null) part.addProperty("parent", entry.getValue().parent());
          part.add("pose", array(entry.getValue().pose()));
          var deps = new com.google.gson.JsonObject();
          f.deps().getOrDefault(entry.getKey(), Map.of()).forEach((value, rate) -> deps.add(value, array(rate)));
          part.add("deps", deps);
          part.addProperty("mesh", mesh(entry.getValue().quads()));
          parts.add(part);
        }
        json.add("parts", parts);
      } else if (slots.containsKey(d)) {
        String[] slot = slots.get(d).split(" ");
        json.addProperty("slot", slot[0]);
        json.addProperty("context", slot[1]);
      } else if (draw.kind().equals("block") || draw.kind().equals("moving")) {
        json.addProperty("block", block.apply(draw));
      } else {
        // The quads under the draw's pose.
        json.addProperty("mesh", mesh(transform(draw.quads(), invert(draw.pose()))));
      }
      draws.add(json);
    }
    out.add("draws", draws);
    return out;
  }

  // ---- light ----

  static int pack(int block, int sky) {
    return block << 4 | sky << 20;
  }

  // How a kind is lit (EntityRenderer.getPackedLightCoords): the point the
  // light is read at, from the entity's position (getLightProbePosition at
  // partial tick 0), and the block and sky light it is drawn in for each
  // level of each there (getBlockLightLevel, getSkyLightLevel).
  com.google.gson.JsonObject lighting(String type, String saved) throws Exception {
    Object entity = entity(type, saved);
    Object renderer = renderer(type);
    Method packed = method("EntityRenderer", "getPackedLightCoords", type("Entity"), float.class);
    Object probe = method("Entity", "getLightProbePosition", float.class).invoke(entity, 0f);
    double[] at = { (double) get(probe, "Vec3", "x"), (double) get(probe, "Vec3", "y"), (double) get(probe, "Vec3", "z") };
    var block = new com.google.gson.JsonArray();
    var sky = new com.google.gson.JsonArray();
    for (int level = 0; level < 16; level++) {
      int value = level;
      light = (layer, pos) -> layer == 1 ? value : 0;
      block.add((int) packed.invoke(renderer, entity, 0f) >> 4 & 15);
      light = (layer, pos) -> layer == 0 ? value : 0;
      sky.add((int) packed.invoke(renderer, entity, 0f) >> 20 & 15);
    }
    light = (layer, pos) -> 0;
    var out = new com.google.gson.JsonObject();
    out.add("probe", array(at));
    out.add("block", block);
    out.add("sky", sky);
    return out;
  }

  // How each draw of a variant is lit: in the entity's light (null), in a
  // light of its own (the packed value), or each quad in the light of a
  // block cell of its own (its step from the entity's cell): found by
  // drawing it in two lights, then in lights that say each cell's place.
  Map<Integer, Object> lights(String type, String saved) throws Exception {
    Map<Integer, Object> out = new HashMap<>();
    // The entity's light in each (getPackedLightCoords).
    Method packed = method("EntityRenderer", "getPackedLightCoords", type("Entity"), float.class);
    light = (layer, pos) -> layer == 1 ? 3 : 9;
    List<Draw> first = render(type, saved);
    int one = (int) packed.invoke(renderer(type), entity(type, saved), 0f);
    light = (layer, pos) -> layer == 1 ? 6 : 12;
    List<Draw> second = render(type, saved);
    int two = (int) packed.invoke(renderer(type), entity(type, saved), 0f);
    Object origin = client.blockPos.getConstructor(int.class, int.class, int.class).newInstance(0, 0, 0);
    Method x = McbenchClient.coordinate(client.blockPos, client.blockPos.getConstructor(int.class, int.class, int.class).newInstance(12345, 678, -9012), 12345);
    Method y = McbenchClient.coordinate(client.blockPos, client.blockPos.getConstructor(int.class, int.class, int.class).newInstance(12345, 678, -9012), 678);
    Method z = McbenchClient.coordinate(client.blockPos, client.blockPos.getConstructor(int.class, int.class, int.class).newInstance(12345, 678, -9012), -9012);
    light = (layer, pos) -> {
      try {
        return layer == 1 ? ((int) x.invoke(pos) + 8) & 15 : ((int) y.invoke(pos) + 8) & 15;
      } catch (Exception error) {
        throw new IllegalStateException(error);
      }
    };
    List<Draw> xy = render(type, saved);
    light = (layer, pos) -> {
      try {
        return layer == 1 ? ((int) z.invoke(pos) + 8) & 15 : 0;
      } catch (Exception error) {
        throw new IllegalStateException(error);
      }
    };
    List<Draw> zs = render(type, saved);
    light = (layer, pos) -> 0;
    for (int d = 0; d < first.size(); d++) {
      Draw a = first.get(d), b = second.get(d);
      if (a.kind().equals("custom")) {
        // Light by vertex.
        boolean entityLight = true, fixed = true;
        for (int q = 0; q < a.quads().size(); q++) for (int k = 0; k < 4; k++) {
          entityLight &= a.quads().get(q).light()[k] == one && b.quads().get(q).light()[k] == two;
          fixed &= a.quads().get(q).light()[k] == b.quads().get(q).light()[k] && a.quads().get(q).light()[k] == a.quads().get(0).light()[0];
        }
        if (entityLight) continue;
        if (fixed) {
          out.put(d, a.quads().get(0).light()[0]);
          continue;
        }
        var cells = new com.google.gson.JsonArray();
        for (int q = 0; q < a.quads().size(); q++) {
          int[] l1 = xy.get(d).quads().get(q).light(), l2 = zs.get(d).quads().get(q).light();
          for (int k = 1; k < 4; k++) if (l1[k] != l1[0] || l2[k] != l2[0]) throw new IllegalStateException(type + " lights a quad's corners from different cells");
          cells.add(array(new float[] { ((l1[0] >> 4 & 15) - 8), ((l1[0] >> 20 & 15) - 8), ((l2[0] >> 4 & 15) - 8) }));
        }
        var json = new com.google.gson.JsonObject();
        json.add("cells", cells);
        out.put(d, json);
        continue;
      }
      if (a.light() == one && b.light() == two) continue;
      if (a.light() == b.light()) out.put(d, a.light());
      else throw new IllegalStateException(type + "'s draw " + d + " is lit by neither the entity's light nor one of its own");
    }
    return out;
  }

  // ---- the kinds ----

  static Values still(String saved) {
    return new Values((v) -> saved, Map.of(), Map.of(), Map.of());
  }

  static Values turning(java.util.function.Function<Map<String, Double>, String> saved) {
    return new Values(saved, Map.of("yaw", 0.0), Map.of("yaw", 30.0), Map.of("yaw", 123.4));
  }

  // One variant of a kind: fitted, lit, written.
  com.google.gson.JsonObject variant(String type, Values values, java.util.function.Function<Draw, String> block, Map<Integer, String> slots) throws Exception {
    List<Fitted> fitted = fit(type, values);
    var out = variant(fitted, values.base(), block, slots, lights(type, values.saved().apply(values.base())));
    return out;
  }

  // The block a kind's block draw shows: the entity's own ("entity"), the
  // one it was captured with.
  static java.util.function.Function<Draw, String> entityBlock(String captured) {
    return (draw) -> {
      if (!draw.state().equals(captured)) throw new IllegalStateException("A block draw of " + draw.state() + ", not the entity's " + captured);
      return "entity";
    };
  }

  com.google.gson.JsonObject kind(String type, String lightSaved, List<String> by, Map<String, com.google.gson.JsonObject> variants) throws Exception {
    var out = new com.google.gson.JsonObject();
    out.add("light", lighting(type, lightSaved));
    var fields = new com.google.gson.JsonArray();
    by.forEach(fields::add);
    out.add("by", fields);
    var json = new com.google.gson.JsonObject();
    variants.forEach(json::add);
    out.add("variants", json);
    kinds.add(type, out);
    return out;
  }

  void capture() throws Exception {
    // Minecarts (the engine's classes, core/src/entity/minecart.rs): turned
    // by yaw and pitch, the block shown standing off by the display offset.
    // A block of the jar's own shows where a block draw is (the page draws
    // the entity's), with what the client's special block renderer draws of
    // it (specials).
    for (String type : List.of("minecraft:minecart", "minecraft:chest_minecart", "minecraft:furnace_minecart", "minecraft:hopper_minecart", "minecraft:tnt_minecart")) {
      boolean tnt = type.equals("minecraft:tnt_minecart");
      Map<String, com.google.gson.JsonObject> variants = new LinkedHashMap<>();
      for (int fuse = tnt ? -1 : 0; fuse < (tnt ? FUSES : 1); fuse++) {
        String primed = tnt ? ",fuse:" + fuse : "";
        Values values = new Values((v) -> "{Rotation:[" + f(v.get("yaw")) + "," + f(v.get("pitch")) + "],CustomDisplayTile:1b,DisplayOffset:" + Math.round(v.get("offset")) + ",DisplayState:{Name:\"minecraft:stone\"}" + primed + "}",
          Map.of("yaw", 0.0, "pitch", 0.0, "offset", 6.0), Map.of("yaw", 30.0, "pitch", 20.0, "offset", 3.0), Map.of("yaw", 77.0, "pitch", -33.0, "offset", 11.0));
        variants.put(tnt ? String.valueOf(fuse) : "", variant(type, values, entityBlock("minecraft:stone"), Map.of()));
      }
      var kind = kind(type, "{}", tnt ? List.of("fuse") : List.of(), variants);
      if (tnt) kind.add("fold", fold("fuse", variants));
    }
    // A minecart of another id, for the page's test of the offset a cart is
    // drawn at by its id.
    {
      id = 7;
      var check = new com.google.gson.JsonObject();
      check.addProperty("id", id);
      check.add("pose", array(render("minecraft:minecart", "{}").get(0).pose()));
      id = ENTITY_ID;
      kinds.getAsJsonObject("minecraft:minecart").add("check", check);
    }
    // Primed TNT, by fuse.
    {
      Map<String, com.google.gson.JsonObject> variants = new LinkedHashMap<>();
      for (int fuse = 0; fuse < FUSES; fuse++) variants.put(String.valueOf(fuse), variant("minecraft:tnt", still("{fuse:" + fuse + "s}"), entityBlock("minecraft:tnt[unstable=false]"), Map.of()));
      var kind = kind("minecraft:tnt", "{}", List.of("fuse"), variants);
      kind.add("fold", fold("fuse", variants));
    }
    // A falling block.
    kind("minecraft:falling_block", "{BlockState:{Name:\"minecraft:sand\"}}", List.of(), Map.of("", variant("minecraft:falling_block", still("{BlockState:{Name:\"minecraft:sand\"}}"), entityBlock("minecraft:sand"), Map.of())));

    // Boats and rafts of every wood, with and without a chest: every entity
    // type the engine makes a boat of (core/src/entity/boat.rs: a key that
    // ends _boat or _raft), turned by yaw.
    for (Object type : (Iterable<?>) entityTypes()) {
      String key = entityKey(type);
      if (!key.endsWith("_boat") && !key.endsWith("_raft")) continue;
      kind(key, "{}", List.of(), Map.of("", variant(key, turning((v) -> "{Rotation:[" + f(v.get("yaw")) + ",0f]}"), null, Map.of())));
    }

    // Golems (the engine makes them of carved pumpkins), turned by yaw; a
    // copper golem by its weather state.
    kind("minecraft:iron_golem", "{}", List.of(), Map.of("", variant("minecraft:iron_golem", turning((v) -> "{Rotation:[" + f(v.get("yaw")) + ",0f]}"), null, Map.of())));
    kind("minecraft:snow_golem", "{}", List.of(), Map.of("", variant("minecraft:snow_golem", turning((v) -> "{Rotation:[" + f(v.get("yaw")) + ",0f]}"), null, Map.of())));
    {
      Map<String, com.google.gson.JsonObject> variants = new LinkedHashMap<>();
      for (int weather = 0; weather < 4; weather++) {
        String state = WEATHER[weather];
        variants.put(String.valueOf(weather), variant("minecraft:copper_golem", turning((v) -> "{Rotation:[" + f(v.get("yaw")) + ",0f],weather_state:\"" + state + "\"}"), null, Map.of()));
      }
      kind("minecraft:copper_golem", "{}", List.of("weather"), variants);
    }

    armorStands();
    frames();
    paintings();
    orbs();
    items();
    equipment();
    specials();
  }

  // ---- experience orbs ----

  // The orb's icon (ExperienceOrbRenderState's one int, from getIcon of its
  // value), by value.
  int orbIcon(int value) throws Exception {
    Object state = createState.invoke(renderer("minecraft:experience_orb"), entity("minecraft:experience_orb", "{Value:" + value + "s}"), 0f);
    for (Field f : state.getClass().getDeclaredFields()) if (f.getType() == int.class && !Modifier.isStatic(f.getModifiers())) return f.getInt(state);
    throw new IllegalStateException("An orb's render state of no icon");
  }

  void orbs() throws Exception {
    // The values at which the icon changes (getIcon grows with the value):
    // each the least value of its icon, found by halving.
    var icons = new com.google.gson.JsonArray();
    int low = 0, top = Short.MAX_VALUE;
    int icon = orbIcon(low);
    icons.add(array(new float[] { low, icon }));
    while (orbIcon(top) != icon) {
      int a = low, b = top;
      while (b - a > 1) {
        int middle = (a + b) >>> 1;
        if (orbIcon(middle) == icon) a = middle;
        else b = middle;
      }
      low = b;
      icon = orbIcon(low);
      icons.add(array(new float[] { low, icon }));
    }
    // The orb faces the camera: drawn under a camera turned a quarter about
    // y, its pose is its pose under no turn, turned so about a point.
    Object straight = cameraType.getConstructor().newInstance();
    set(straight, "CameraRenderState", "orientation", cls("org.joml.Quaternionf").getConstructor().newInstance());
    Object turned = cameraType.getConstructor().newInstance();
    Object quarter = cls("org.joml.Quaternionf").getConstructor().newInstance();
    quarter.getClass().getMethod("rotationY", float.class).invoke(quarter, (float) (Math.PI / 2));
    set(turned, "CameraRenderState", "orientation", quarter);
    Map<String, com.google.gson.JsonObject> variants = new LinkedHashMap<>();
    for (var entry : icons) {
      int value = (int) entry.getAsJsonArray().get(0).getAsFloat(), own = (int) entry.getAsJsonArray().get(1).getAsFloat();
      String saved = "{Value:" + value + "s}";
      List<Draw> plain = render("minecraft:experience_orb", saved, straight), round = render("minecraft:experience_orb", saved, turned);
      List<Fitted> fitted = new ArrayList<>();
      for (Draw draw : plain) fitted.add(new Fitted(draw, List.of(), Map.of(), -1, null, null));
      var variant = variant(fitted, Map.of(), null, Map.of(), lights("minecraft:experience_orb", saved));
      Motion turn = motion("camera", plain.get(0).pose(), round.get(0).pose(), 90);
      if (turn == null || Math.abs(turn.axis()[1] - 1) > 1e-5 || Math.abs(turn.rate() - 1) > 1e-4) throw new IllegalStateException("An orb that does not turn with the camera");
      variant.getAsJsonArray("draws").get(0).getAsJsonObject().add("billboard", array(turn.point()));
      variants.put(String.valueOf(own), variant);
    }
    var kind = kind("minecraft:experience_orb", "{Value:1s}", List.of("icon"), variants);
    kind.add("icons", icons);
  }

  // ---- items ----

  // The display contexts entities draw items in that the page draws items
  // of: dropped (ground), in a frame (fixed), held by an armor stand and
  // worn on its head.
  static final List<String> CONTEXTS = List.of("ground", "fixed", "third_person_right_hand", "third_person_left_hand", "head");

  void items() throws Exception {
    Object registry = registryOf(type("Item"));
    List<String> names = new ArrayList<>();
    for (Object key : (java.util.Set<?>) method("Registry", "keySet").invoke(registry)) names.add(key.toString());
    java.util.Collections.sort(names);
    names.remove("minecraft:air");
    // A dropped stack is drawn in copies, more for more items (ItemCluster
    // RenderState.getRenderedAmount): by count, how many.
    adjust = (state) -> {
      try {
        set(state, "ItemEntityRenderState", "bobOffset", 0f);
        set(state, "EntityRenderState", "ageInTicks", 0f);
      } catch (Exception error) {
        throw new IllegalStateException(error);
      }
    };
    int layers = render("minecraft:item", "{Item:{id:\"minecraft:stick\",count:1}}").size();
    var tiers = new com.google.gson.JsonArray();
    for (int count = 1; count <= 64; count++) tiers.add(render("minecraft:item", "{Item:{id:\"minecraft:stick\",count:" + count + "}}").size() / layers);
    for (String name : names) {
      var item = new com.google.gson.JsonObject();
      for (String context : CONTEXTS) {
        var draws = new com.google.gson.JsonArray();
        for (Draw draw : item(name, context)) draws.add(standalone(draw));
        item.add(context, draws);
      }
      // Dropped: the copies of a full stack, each the first's draws moved.
      List<Draw> dropped = render("minecraft:item", "{Item:{id:\"" + name + "\",count:64}}");
      int per = dropped.size() / tiers.get(63).getAsInt();
      var first = new com.google.gson.JsonArray();
      for (int d = 0; d < per; d++) first.add(standalone(dropped.get(d)));
      var copies = new com.google.gson.JsonArray();
      for (int c = 1; c * per < dropped.size(); c++) {
        float[] k = multiply(dropped.get(c * per).pose(), invert(dropped.get(0).pose()));
        for (int d = 0; d < per; d++) if (difference(multiply(k, dropped.get(d).pose()), dropped.get(c * per + d).pose()) > 1e-4) throw new IllegalStateException(name + "'s copies are not the first moved");
        copies.add(pose(k));
      }
      item.add("dropped", first);
      item.add("copies", copies);
      items.add(name, item);
    }
    // How the spin and bob go with age, for the page's test: a stick at an
    // age and bob offset.
    adjust = (state) -> {
      try {
        set(state, "ItemEntityRenderState", "bobOffset", 0.7f);
        set(state, "EntityRenderState", "ageInTicks", 37f);
      } catch (Exception error) {
        throw new IllegalStateException(error);
      }
    };
    var check = new com.google.gson.JsonObject();
    check.addProperty("age", 37);
    check.addProperty("bob", 0.7f);
    check.add("pose", array(render("minecraft:item", "{Item:{id:\"minecraft:stick\",count:1}}").get(0).pose()));
    adjust = (state) -> {};
    var kind = kind("minecraft:item", "{Item:{id:\"minecraft:stick\",count:1}}", List.of(), Map.of());
    kind.add("tiers", tiers);
    kind.add("check", check);
  }

  // A draw on its own (an item's, a special renderer's): its quads under
  // its pose, the pose, and how it is lit.
  com.google.gson.JsonObject standalone(Draw draw) throws Exception {
    var json = new com.google.gson.JsonObject();
    json.addProperty("layer", draw.renderType());
    if (draw.overlay() != -1) json.addProperty("overlay", draw.overlay());
    json.addProperty("pose", pose(draw.pose()));
    json.addProperty("mesh", mesh(transform(draw.quads(), invert(draw.pose()))));
    return json;
  }

  // ---- what an armor stand wears ----

  // Every item the client draws on an armor stand otherwise than as an item
  // in a hand or on the head (armor and elytra through their equipment
  // models, heads through their skull models): by item, slot and size, the
  // draws it adds to the stand's.
  void equipment() throws Exception {
    var out = new com.google.gson.JsonObject();
    Values bare = standValues("", "");
    int stand = render("minecraft:armor_stand", bare.saved().apply(bare.base())).size();
    for (var entry : items.entrySet()) {
      String name = entry.getKey();
      String equipped = equippable(name);
      if (equipped == null) continue;
      for (String slot : List.of(equipped)) {
        for (boolean small : List.of(false, true)) {
          String flags = "Small:" + (small ? 1 : 0) + "b,";
          Values values = standValues(flags, slot + ":{id:\"" + name + "\",count:1}");
          List<Draw> worn = render("minecraft:armor_stand", values.saved().apply(values.base()));
          // Drawn as an item on the head is the stand's own (its head slot).
          if (worn.size() == stand || worn.stream().allMatch((d) -> !d.kind().equals("model") || worn.indexOf(d) < stand) && worn.stream().anyMatch((d) -> d.kind().equals("item"))) continue;
          List<Fitted> fitted = fit("minecraft:armor_stand", values);
          var variant = variant(fitted, values.base(), null, Map.of(), lights("minecraft:armor_stand", values.saved().apply(values.base())));
          var draws = variant.getAsJsonArray("draws");
          var added = new com.google.gson.JsonArray();
          for (int d = stand; d < draws.size(); d++) added.add(draws.get(d));
          if (added.isEmpty()) continue;
          var bySlot = out.has(name) ? out.getAsJsonObject(name) : new com.google.gson.JsonObject();
          var bySize = bySlot.has(slot) ? bySlot.getAsJsonObject(slot) : new com.google.gson.JsonObject();
          bySize.add(String.valueOf(small), added);
          bySlot.add(slot, bySize);
          out.add(name, bySlot);
        }
      }
    }
    kinds.getAsJsonObject("minecraft:armor_stand").add("equipment", out);
  }

  // The built-in registry of values of a class (blocks, items).
  Object registryOf(Class<?> type) {
    for (Object registry : client.registries) if (registry instanceof Iterable<?> values && values.iterator().hasNext() && type.isInstance(values.iterator().next())) return registry;
    throw new IllegalStateException("No registry of " + type);
  }

  // The slot an item is worn in (its equippable component's, lower case),
  // or null for an item with none.
  String equippable(String item) throws Exception {
    Object stack = stack(item, 1);
    Object components = null;
    for (Method m : stack.getClass().getMethods()) if (m.getParameterCount() == 0 && Iterable.class.isAssignableFrom(m.getReturnType()) && m.getReturnType().getName().length() <= 4) components = m.invoke(stack);
    // TypedDataComponent(type, value), the record's fields in order (the
    // client's record accessors are renamed).
    for (Object typed : (Iterable<?>) components) {
      List<Field> fields = Arrays.stream(typed.getClass().getDeclaredFields()).filter((f) -> !Modifier.isStatic(f.getModifiers())).toList();
      fields.forEach((f) -> f.setAccessible(true));
      Object type = fields.get(0).get(typed), value = fields.get(1).get(typed);
      String key = method("Registry", "getKey", Object.class).invoke(registryOf(type.getClass()), type).toString();
      if (!key.equals("minecraft:equippable")) continue;
      for (Field component : value.getClass().getDeclaredFields()) {
        if (Modifier.isStatic(component.getModifiers())) continue;
        component.setAccessible(true);
        Object slot = component.get(value);
        if (slot instanceof Enum<?> e && Arrays.stream(e.getClass().getEnumConstants()).anyMatch((c) -> ((Enum<?>) c).name().equals("HEAD"))) return e.name().toLowerCase(java.util.Locale.ROOT);
      }
    }
    return null;
  }

  // ---- writing ----

  static String base64(java.nio.ByteBuffer bytes) {
    return java.util.Base64.getEncoder().encodeToString(Arrays.copyOf(bytes.array(), bytes.position()));
  }

  // The capture as the page reads it (see the header).
  void write(Path target, com.google.gson.JsonObject atlas) throws Exception {
    var out = new com.google.gson.JsonObject();
    out.add("atlas", atlas);
    out.addProperty("version", 1);
    out.addProperty("id", ENTITY_ID);
    var names = new com.google.gson.JsonArray();
    textures.forEach(names::add);
    out.add("textures", names);
    var shapeBytes = java.nio.ByteBuffer.allocate(shapes.size() * 80).order(java.nio.ByteOrder.LITTLE_ENDIAN);
    for (float[] shape : shapes) for (float v : shape) shapeBytes.putFloat(v);
    out.addProperty("shapes", base64(shapeBytes));
    int quads = meshes.stream().mapToInt(List::size).sum();
    var quadBytes = java.nio.ByteBuffer.allocate(quads * 12).order(java.nio.ByteOrder.LITTLE_ENDIAN);
    var meshBytes = java.nio.ByteBuffer.allocate((meshes.size() + 1) * 4).order(java.nio.ByteOrder.LITTLE_ENDIAN);
    int at = 0;
    for (List<int[]> mesh : meshes) {
      meshBytes.putInt(at);
      for (int[] quad : mesh) for (int v : quad) quadBytes.putInt(v);
      at += mesh.size();
    }
    meshBytes.putInt(at);
    out.addProperty("quads", base64(quadBytes));
    var poseBytes = java.nio.ByteBuffer.allocate(poses.size() * 64).order(java.nio.ByteOrder.LITTLE_ENDIAN);
    for (float[] matrix : poses) for (float v : matrix) poseBytes.putFloat(v);
    out.addProperty("poses", base64(poseBytes));
    out.addProperty("meshes", base64(meshBytes));
    out.add("kinds", kinds);
    out.add("items", items);
    out.add("specials", specialBlocks);
    Path partial = target.resolveSibling(target.getFileName() + ".part");
    Files.writeString(partial, out.toString());
    Files.move(partial, target, java.nio.file.StandardCopyOption.REPLACE_EXISTING);
  }

  // ---- the special block renderers ----

  // What the client's special block renderer draws of each block that has
  // one (SpecialBlockModelRenderer.renderByBlock, as its collector does for
  // every block submitted): a block drawn by an entity is its model and
  // this.
  void specials() throws Exception {
    Object none = null;
    for (Object c : type("ItemDisplayContext").getEnumConstants()) if (((Enum<?>) c).name().equals("NONE")) none = c;
    // The blocks it has a renderer of (its map's keys), by name.
    Object blockRegistry = registryOf(type("Block"));
    Map<String, Object> blocks = new java.util.TreeMap<>();
    for (Object block : ((Map<?, ?>) get(specials, "SpecialBlockModelRenderer", "renderers")).keySet()) blocks.put(method("Registry", "getKey", Object.class).invoke(blockRegistry, block).toString(), block);
    for (Object block : blocks.values()) {
      List<Draw> draws = new ArrayList<>();
      Object pose = poseType.getConstructor().newInstance();
      Object[] collector = new Object[1];
      collector[0] = Proxy.newProxyInstance(loader, new Class<?>[] { collectorType }, (proxy, m, a) -> {
        if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, a);
        if (m.getParameterCount() == 1 && m.getParameterTypes()[0] == int.class) return proxy;
        submitted(draws, m, a, collector[0]);
        return null;
      });
      method("SpecialBlockModelRenderer", "renderByBlock", type("Block"), type("ItemDisplayContext"), poseType, collectorType, int.class, int.class, int.class).invoke(specials, block, none, pose, collector[0], pack(15, 15), 655360, 0);
      if (draws.isEmpty()) continue;
      var json = new com.google.gson.JsonArray();
      for (Draw draw : draws) json.add(standalone(draw));
      specialBlocks.add(method("Registry", "getKey", Object.class).invoke(blockRegistry, block).toString(), json);
    }
  }

  // ---- items ----

  // A stack of an item, read by ItemStack.CODEC (the codec of its one
  // public static MapCodec).
  Object stack(String item, int count) throws Exception {
    if (stackCodec == null) {
      Field mapCodec = null;
      for (Field f : type("ItemStack").getFields()) if (Modifier.isStatic(f.getModifiers()) && f.getType().getName().equals("com.mojang.serialization.MapCodec")) mapCodec = f;
      Object codec = mapCodec.get(null);
      stackCodec = codec.getClass().getMethod("codec").invoke(codec);
    }
    return client.decode(stackCodec, client.jsonParse.invoke(null, "{\"id\":\"" + item + "\",\"count\":" + count + "}"), "a stack of " + item);
  }

  Object stackCodec;

  // What an item is drawn as in a display context, alone, under no pose
  // (ItemModelResolver.updateForTopItem of its own stack, then
  // ItemStackRenderState.submit).
  List<Draw> item(String item, String context) throws Exception {
    Object state = type("ItemStackRenderState").getConstructor().newInstance();
    Object display = null;
    for (Object c : type("ItemDisplayContext").getEnumConstants()) if (((Enum<?>) c).name().toLowerCase(java.util.Locale.ROOT).equals(context)) display = c;
    Object resolver = get(this.context, "EntityRendererProvider.Context", "itemModelResolver");
    method("ItemModelResolver", "updateForTopItem", type("ItemStackRenderState"), type("ItemStack"), type("ItemDisplayContext"), type("Level"), type("ItemOwner"), int.class).invoke(resolver, state, stack(item, 1), display, level, null, 0);
    List<Draw> draws = new ArrayList<>();
    Object pose = poseType.getConstructor().newInstance();
    Object[] collector = new Object[1];
    collector[0] = Proxy.newProxyInstance(loader, new Class<?>[] { collectorType }, (proxy, m, a) -> {
      if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, a);
      if (m.getParameterCount() == 1 && m.getParameterTypes()[0] == int.class) return proxy;
      submitted(draws, m, a, collector[0]);
      return null;
    });
    method("ItemStackRenderState", "submit", poseType, collectorType, int.class, int.class, int.class).invoke(state, pose, collector[0], pack(15, 15), 655360, 0);
    return draws;
  }

  // An item draw's pose in the item's own space: the pose it was submitted
  // under, less the pose of the item alone in the same context (the first
  // draw of each), so another item's draws can go there.
  float[] itemFrame(Draw draw, String item) throws Exception {
    List<Draw> alone = item(item, draw.state());
    return multiply(draw.pose(), invert(alone.get(0).pose()));
  }

  // ---- armor stands ----

  Values standValues(String flags, String equipment) {
    Map<String, Double> base = new LinkedHashMap<>(), step = new LinkedHashMap<>(), check = new LinkedHashMap<>();
    var random = new java.util.Random(7);
    base.put("yaw", 0.0);
    step.put("yaw", 30.0);
    check.put("yaw", 123.4);
    for (int p = 0; p < POSES.length; p++) for (int axis = 0; axis < 3; axis++) {
      String name = POSES[p] + "." + "xyz".charAt(axis);
      base.put(name, POSE_BASE[p][axis]);
      step.put(name, 20.0);
      check.put(name, POSE_BASE[p][axis] + random.nextDouble() * 60 - 30);
    }
    return new Values((v) -> {
      StringBuilder pose = new StringBuilder();
      for (int p = 0; p < POSES.length; p++) pose.append(p > 0 ? "," : "").append(POSES[p]).append(":[").append(f(v.get(POSES[p] + ".x"))).append(",").append(f(v.get(POSES[p] + ".y"))).append(",").append(f(v.get(POSES[p] + ".z"))).append("]");
      return "{Rotation:[" + f(v.get("yaw")) + ",0f]," + flags + "Pose:{" + pose + "},equipment:{" + equipment + "}}";
    }, base, step, check);
  }

  static final String[] POSES = { "Head", "Body", "LeftArm", "RightArm", "LeftLeg", "RightLeg" };
  // ArmorStand's default poses (DEFAULT_HEAD_POSE ...), degrees.
  static final double[][] POSE_BASE = { { 0, 0, 0 }, { 0, 0, 0 }, { -10, 0, -10 }, { -15, 0, 10 }, { -1, 0, -1 }, { 1, 0, 1 } };
  // An item each, to find where an item of a slot is drawn: in the main
  // hand, the off hand and on the head.
  static final Map<String, String> HELD = Map.of("mainhand", "minecraft:stick", "offhand", "minecraft:blaze_rod", "head", "minecraft:feather");

  void armorStands() throws Exception {
    Map<String, com.google.gson.JsonObject> variants = new LinkedHashMap<>();
    String held = HELD.entrySet().stream().map((e) -> e.getKey() + ":{id:\"" + e.getValue() + "\",count:1}").reduce((a, b) -> a + "," + b).orElseThrow();
    for (int bits = 0; bits < 16; bits++) {
      boolean small = (bits & 1) != 0, arms = (bits & 2) != 0, plate = (bits & 4) != 0, invisible = (bits & 8) != 0;
      String flags = "Small:" + (small ? 1 : 0) + "b,ShowArms:" + (arms ? 1 : 0) + "b,NoBasePlate:" + (plate ? 1 : 0) + "b,Invisible:" + (invisible ? 1 : 0) + "b,";
      Values values = standValues(flags, held);
      List<Fitted> fitted = fit("minecraft:armor_stand", values);
      Map<Integer, String> slots = new HashMap<>();
      for (int d = 0; d < fitted.size(); d++) {
        Fitted f = fitted.get(d);
        if (!f.draw().kind().equals("item")) continue;
        String texture = f.draw().quads().get(0).texture();
        String slot = HELD.entrySet().stream().filter((e) -> texture.equals(e.getValue().replace(":", ":item/"))).map(Map.Entry::getKey).findFirst().orElseThrow();
        slots.put(d, slot + " " + f.draw().state());
        // The draw's place for any item: under the part, less this item's
        // own pose in its context.
        float[] own = invert(item(HELD.get(slot), f.draw().state()).get(0).pose());
        if (f.attachPart() != null) fitted.set(d, new Fitted(f.draw(), f.motions(), f.deps(), f.attachDraw(), f.attachPart(), multiply(f.attachMatrix(), own)));
        else fitted.set(d, new Fitted(new Draw(f.draw().kind(), multiply(f.draw().pose(), own), f.draw().quads(), f.draw().renderType(), f.draw().light(), f.draw().overlay(), f.draw().tint(), f.draw().state(), null), f.motions(), f.deps(), -1, null, null));
      }
      variants.put(small + "," + arms + "," + plate + "," + invisible, variant(fitted, values.base(), null, slots, lights("minecraft:armor_stand", values.saved().apply(values.base()))));
    }
    kind("minecraft:armor_stand", "{}", List.of("small", "show_arms", "no_base_plate", "invisible"), variants);
  }

  // ---- item frames ----

  // Direction by 3D data value (down, up, north, south, west, east), as an
  // item frame's Facing is saved.
  static final String[] FACINGS = { "down", "up", "north", "south", "west", "east" };

  void frames() throws Exception {
    for (String type : List.of("minecraft:item_frame", "minecraft:glow_item_frame")) {
      Map<String, com.google.gson.JsonObject> variants = new LinkedHashMap<>();
      for (int facing = 0; facing < 6; facing++) for (int invisible = 0; invisible < 2; invisible++) for (int rotation = 0; rotation < 8; rotation++) {
        String saved = "{block_pos:[I;0,0,0],Facing:" + facing + "b,Invisible:" + invisible + "b,ItemRotation:" + rotation + "b,Item:{id:\"minecraft:stick\",count:1}}";
        List<Fitted> fitted = fit(type, still(saved));
        Map<Integer, String> slots = new HashMap<>();
        for (int d = 0; d < fitted.size(); d++) {
          Fitted f = fitted.get(d);
          if (!f.draw().kind().equals("item")) continue;
          slots.put(d, "item " + f.draw().state());
          float[] pose = multiply(f.draw().pose(), invert(item("minecraft:stick", f.draw().state()).get(0).pose()));
          Draw moved = new Draw(f.draw().kind(), pose, f.draw().quads(), f.draw().renderType(), f.draw().light(), f.draw().overlay(), f.draw().tint(), f.draw().state(), null);
          fitted.set(d, new Fitted(moved, f.motions(), f.deps(), -1, null, null));
        }
        variants.put(FACINGS[facing] + "," + (invisible == 1) + "," + rotation, variant(fitted, Map.of(), null, slots, lights(type, saved)));
      }
      kind(type, "{block_pos:[I;0,0,0],Facing:3b}", List.of("facing", "invisible", "rotation"), variants);
    }
  }

  // ---- paintings ----

  // Direction by 2D data value (south, west, north, east), as a painting's
  // facing is saved.
  static final String[] HORIZONTAL = { "south", "west", "north", "east" };

  void paintings() throws Exception {
    Map<String, com.google.gson.JsonObject> variants = new LinkedHashMap<>();
    Object registry = ((java.util.Optional<?>) method("RegistryAccess", "lookup", type("ResourceKey")).invoke(registryAccess, paintingRegistry())).orElseThrow();
    List<String> names = new ArrayList<>();
    for (Object key : (java.util.Set<?>) method("Registry", "keySet").invoke(registry)) names.add(key.toString());
    java.util.Collections.sort(names);
    for (String variant : names) for (int facing = 0; facing < 4; facing++) {
      String saved = "{block_pos:[I;0,0,0],facing:" + facing + "b,variant:\"" + variant + "\"}";
      variants.put(variant + "," + HORIZONTAL[facing], variant("minecraft:painting", still(saved), null, Map.of()));
    }
    kind("minecraft:painting", "{block_pos:[I;0,0,0],facing:0b}", List.of("variant", "facing"), variants);
  }

  Object paintingRegistry() throws Exception {
    for (Object registry : (List<?>) field("RegistryDataLoader", "WORLDGEN_REGISTRIES").get(null)) {
      Object key = get(registry, "RegistryDataLoader.RegistryData", "key");
      if (key.toString().contains("minecraft:painting_variant")) return key;
    }
    throw new IllegalStateException("No painting variant registry");
  }

  // WeatheringCopper.WeatherState by ordinal, as saved.
  static final String[] WEATHER = { "unaffected", "exposed", "weathered", "oxidized" };

  // BuiltInRegistries.ENTITY_TYPE, and an entity type's key.
  Iterable<?> entityTypes() throws Exception {
    Object any = entityType("minecraft:pig");
    for (Object registry : client.registries) {
      if (registry instanceof Iterable<?> values && values.iterator().hasNext() && values.iterator().next().getClass() == any.getClass()) return values;
    }
    throw new IllegalStateException("No entity type registry");
  }

  String entityKey(Object type) throws Exception {
    return method("EntityType", "getKey", type("EntityType")).invoke(null, type).toString();
  }

  static final int FUSES = 80;

  // A value whose variants repeat from `from` on with a period: the
  // smallest such, checked over every variant captured.
  static com.google.gson.JsonObject fold(String value, Map<String, com.google.gson.JsonObject> variants) {
    List<Integer> keys = variants.keySet().stream().map(Integer::parseInt).sorted().toList();
    int last = keys.get(keys.size() - 1);
    for (int from = keys.get(0); from <= last; from++) {
      for (int period = 1; from + 2 * period <= last + 1; period++) {
        boolean repeats = true;
        for (int at = from + period; at <= last && repeats; at++) repeats = variants.get(String.valueOf(at)).toString().equals(variants.get(String.valueOf(at - period)).toString());
        if (repeats) {
          var out = new com.google.gson.JsonObject();
          out.addProperty("value", value);
          out.addProperty("from", from);
          out.addProperty("period", period);
          return out;
        }
      }
    }
    throw new IllegalStateException(value + "'s variants do not repeat");
  }
}
