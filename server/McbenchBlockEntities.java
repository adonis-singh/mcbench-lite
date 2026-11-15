import java.lang.classfile.ClassFile;
import java.lang.classfile.CodeElement;
import java.lang.classfile.Opcode;
import java.lang.classfile.instruction.FieldInstruction;
import java.lang.classfile.instruction.NewObjectInstruction;
import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.lang.reflect.Proxy;
import java.lang.reflect.RecordComponent;
import java.lang.reflect.UndeclaredThrowableException;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.IdentityHashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.concurrent.CompletableFuture;
import java.util.regex.Pattern;

// What Minecraft's client draws of every block state through a block entity
// renderer, asked of its own renderers, in the bake's JVM, where McbenchClient
// has started the client's classes. For each block entity type that has a
// renderer (BlockEntityRenderers's providers), the renderer is made as the
// client makes it, from a BlockEntityRendererProvider.Context; for each state
// of the type, a block entity of the state at the origin, in no level, is
// extracted into the renderer's render state at partial tick 0 and submitted
// to a SubmitNodeCollector of this program's. What it submits is drawn by
// Minecraft's own code, Model.setupAnim and renderToBuffer or
// ModelPart.render, into a VertexConsumer of this program's, under the pose
// the renderer left on its PoseStack, as the client's model feature renderer
// draws a submitted model. Each quad keeps its corners (blocks, from the
// block's lowest corner), its texture coordinates over its texture (the
// material's texture as the renderer chose it from its MaterialSet, or the
// render type's), and the colour it is tinted with.
//
// What the renderers are given to run with no window, no level and no player:
// the context's EntityModelSet is the vanilla one (its static factory); its
// MaterialSet answers each Material with a sprite of its own, by which the
// material is known again (no atlas is stitched: a submitted model is drawn
// without its sprite's mapping, so its coordinates stay over the texture);
// the Font is made over a glyph provider that fails if asked (the text drawn
// at rest is a placed sign's, empty, and is only measured); every other part
// of the context is a bare instance of its class, which a renderer that uses
// it fails on; and Minecraft.getInstance() is a bare game whose user
// properties are offline's (the sign renderer asks whether text is filtered).
// The client's Font also needs ICU's ArabicShapingException to link
// (McbenchClient.IcuLinkage).
//
// Renderers that still cannot run so draw nothing here, and their block is
// drawn by its block model alone: `refused` names each type and why.
final class McbenchBlockEntities {
  record Quad(float[] position, float[] uv, int color, String texture) {}

  final Map<String, List<Quad>> quads = new LinkedHashMap<>();
  // By state with another half, the step to it (x, y, z): a double chest's
  // or a bed's, whose renderer draws both halves in the light of the
  // brighter (BrightnessCombiner, through DoubleBlockCombiner).
  final Map<String, int[]> partners = new LinkedHashMap<>();
  final Map<String, String> refused = new TreeMap<>();
  final java.util.Set<String> asked = new java.util.TreeSet<>();

  static McbenchBlockEntities capture(McbenchClient client) throws Exception {
    var captured = new McbenchBlockEntities();
    var materials = new IdentityHashMap<Object, String>();
    Object context = context(client, materials);
    game(client);
    Object origin = client.blockPos.getConstructor(int.class, int.class, int.class).newInstance(0, 0, 0);
    // The level the block entity is in: the block alone, at the origin, in
    // air (state 0), at game time 0; asked anything else it answers its
    // type's zero (`asked` keeps what was asked).
    Class<?> blockState = client.states.get(0).getClass();
    Class<?> blockEntity = client.createBlockEntity.getReturnType();
    Method setLevel = null;
    for (Method method : blockEntity.getMethods()) {
      if (method.getParameterCount() == 1 && method.getReturnType() == void.class && Modifier.isAbstract(method.getParameterTypes()[0].getModifiers()) && !method.getParameterTypes()[0].isInterface()
        && methods(method.getParameterTypes()[0]).stream().anyMatch((m) -> m.getReturnType() == blockState && List.of(m.getParameterTypes()).equals(List.of(client.blockPos)))) setLevel = method;
    }
    if (setLevel == null) throw new IllegalStateException("No BlockEntity.setLevel in the client jar");
    Object[] here = new Object[1];
    Object level = level(client, setLevel.getParameterTypes()[0], (method, arguments) -> {
      if (method.getReturnType() == blockState && arguments.length == 1 && client.blockPos.isInstance(arguments[0])) return arguments[0].equals(origin) ? here[0] : client.states.get(0);
      captured.asked.add(method.getReturnType().getSimpleName() + " " + method.getName() + Arrays.toString(method.getParameterTypes()));
      Class<?> r = method.getReturnType();
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
    Method placeIn = setLevel;
    Map<Object, Object> renderers = new LinkedHashMap<>();
    for (int id = 0; id < client.states.size(); id++) {
      Object type = client.stateTypes[id];
      if (type == null) continue;
      String typeName = client.renderers[id];
      if (captured.refused.containsKey(typeName)) continue;
      try {
        Object renderer = renderers.get(type);
        if (renderer == null) renderers.put(type, renderer = create(client.providers.get(type), context));
        var quads = new ArrayList<Quad>();
        here[0] = client.states.get(id);
        int[] partner = partner(client.states.get(id));
        if (partner != null) captured.partners.put(client.names.get(id), partner);
        draw(client, renderer, type, client.states.get(id), origin, materials, quads, (made) -> placeIn.invoke(made, level));
        captured.quads.put(client.names.get(id), quads);
      } catch (Throwable error) {
        Throwable cause = error;
        while ((cause instanceof InvocationTargetException || cause instanceof UndeclaredThrowableException) && cause.getCause() != null) cause = cause.getCause();
        StackTraceElement[] trace = cause.getStackTrace();
        captured.refused.put(typeName, cause + (trace.length > 0 ? " at " + trace[0] : ""));
        for (int other = 0; other < client.states.size(); other++) if (typeName.equals(client.renderers[other])) captured.quads.remove(client.names.get(other));
      }
    }
    return captured;
  }

  // The other half of a double block, as DoubleBlockCombiner finds it: its
  // block's static getBlockType(BlockState) (an enum of SINGLE, FIRST and
  // SECOND) and getConnectedDirection(BlockState) (a Direction, its step by
  // the direction's name); null for none.
  static int[] partner(Object state) throws Exception {
    Object block = null;
    for (Method method : state.getClass().getMethods()) {
      if (method.getParameterCount() == 0 && method.getName().length() <= 2 && !method.getReturnType().isPrimitive() && !method.getReturnType().isEnum()) {
        Object value = method.invoke(state);
        if (value != null && hasDoubleType(value.getClass(), state.getClass())) block = value;
      }
    }
    if (block == null) return null;
    Method type = null, connected = null;
    for (Class<?> c = block.getClass(); c != Object.class; c = c.getSuperclass()) for (Method method : c.getDeclaredMethods()) {
      if (!Modifier.isStatic(method.getModifiers()) || method.getParameterCount() != 1 || method.getParameterTypes()[0] != state.getClass() || !method.getReturnType().isEnum()) continue;
      List<String> names = Arrays.stream(method.getReturnType().getEnumConstants()).map((constant) -> ((Enum<?>) constant).name()).toList();
      if (names.equals(List.of("SINGLE", "FIRST", "SECOND"))) type = method;
      else if (names.size() == 6) connected = method;
    }
    type.setAccessible(true);
    connected.setAccessible(true);
    if (((Enum<?>) type.invoke(null, state)).name().equals("SINGLE")) return null;
    Object direction = connected.invoke(null, state);
    int[] step = new int[3];
    String name = ((Enum<?>) direction).name();
    step[0] = name.equals("EAST") ? 1 : name.equals("WEST") ? -1 : 0;
    step[1] = name.equals("UP") ? 1 : name.equals("DOWN") ? -1 : 0;
    step[2] = name.equals("SOUTH") ? 1 : name.equals("NORTH") ? -1 : 0;
    return step;
  }

  static boolean hasDoubleType(Class<?> block, Class<?> stateType) {
    for (Class<?> c = block; c != null && c != Object.class; c = c.getSuperclass()) for (Method method : c.getDeclaredMethods()) {
      if (Modifier.isStatic(method.getModifiers()) && method.getParameterCount() == 1 && method.getParameterTypes()[0] == stateType && method.getReturnType().isEnum()
        && Arrays.stream(method.getReturnType().getEnumConstants()).map((constant) -> ((Enum<?>) constant).name()).toList().equals(List.of("SINGLE", "FIRST", "SECOND"))) return true;
    }
    return false;
  }

  static Object unsafe;
  static Method allocateInstance;

  // A bare instance of a class: its fields at their zero values.
  static Object allocate(Class<?> type) throws Exception {
    if (unsafe == null) {
      Class<?> holder = Class.forName("sun.misc.Unsafe");
      Field field = holder.getDeclaredField("theUnsafe");
      field.setAccessible(true);
      unsafe = field.get(null);
      allocateInstance = holder.getMethod("allocateInstance", Class.class);
    }
    return allocateInstance.invoke(unsafe, type);
  }

  // BlockEntityRendererProvider.create: the providers' one abstract method.
  static Object create(Object provider, Object context) throws Exception {
    for (Class<?> face : provider.getClass().getInterfaces()) for (Method method : face.getMethods()) {
      if (Modifier.isAbstract(method.getModifiers()) && method.getParameterCount() == 1 && method.getParameterTypes()[0] == context.getClass()) return method.invoke(provider, context);
    }
    throw new IllegalStateException("A block entity renderer provider without create(Context)");
  }

  static Object context(McbenchClient client, Map<Object, String> materials) throws Exception {
    Object provider = client.providers.values().iterator().next();
    Class<?> context = null;
    for (Class<?> face : provider.getClass().getInterfaces()) for (Method method : face.getMethods()) {
      if (Modifier.isAbstract(method.getModifiers()) && method.getParameterCount() == 1 && method.getParameterTypes()[0].isRecord()) context = method.getParameterTypes()[0];
    }
    if (context == null) throw new IllegalStateException("No BlockEntityRendererProvider.Context in the client jar");
    RecordComponent[] components = context.getRecordComponents();
    Class<?>[] types = new Class<?>[components.length];
    Object[] values = new Object[components.length];
    for (int i = 0; i < components.length; i++) {
      Class<?> type = types[i] = components[i].getType();
      Method factory = null;
      for (Method method : type.getDeclaredMethods()) {
        if (Modifier.isStatic(method.getModifiers()) && Modifier.isPublic(method.getModifiers()) && method.getParameterCount() == 0 && method.getReturnType() == type) factory = method;
      }
      Constructor<?>[] constructors = type.getConstructors();
      if (factory != null) values[i] = factory.invoke(null);
      else if (type.isInterface()) {
        values[i] = Proxy.newProxyInstance(client.loader, new Class<?>[] { type }, (proxy, method, arguments) -> {
          if (method.getParameterCount() == 1 && !method.getReturnType().isPrimitive() && !method.getReturnType().isInterface()) {
            Object sprite = allocate(method.getReturnType());
            materials.put(sprite, arguments[0].toString());
            return sprite;
          }
          throw new UnsupportedOperationException("A block entity renderer asked " + method);
        });
      } else if (constructors.length == 1 && constructors[0].getParameterCount() > 0 && Arrays.stream(constructors[0].getParameterTypes()).allMatch(Class::isInterface)) {
        values[i] = constructors[0].newInstance(Arrays.stream(constructors[0].getParameterTypes()).map((face) -> Proxy.newProxyInstance(client.loader, new Class<?>[] { face }, (proxy, method, arguments) -> {
          throw new UnsupportedOperationException("A block entity renderer asked " + method);
        })).toArray());
      } else values[i] = allocate(type);
    }
    return context.getDeclaredConstructor(types).newInstance(values);
  }

  // Minecraft.getInstance(): the class net.minecraft.client.main.Main makes
  // that holds a static field of its own type, a bare instance whose user
  // properties (the CompletableFuture its method answering
  // UserApiService.UserProperties reads) are offline's.
  static void game(McbenchClient client) throws Exception {
    byte[] main;
    try (var stream = client.loader.getResourceAsStream("net/minecraft/client/main/Main.class")) {
      main = stream.readAllBytes();
    }
    Class<?> properties = Class.forName("com.mojang.authlib.minecraft.UserApiService$UserProperties", true, client.loader);
    Object offline = Class.forName("com.mojang.authlib.minecraft.UserApiService", true, client.loader).getField("OFFLINE_PROPERTIES").get(null);
    int games = 0;
    for (var method : ClassFile.of().parse(main).methods()) {
      for (CodeElement element : (Iterable<CodeElement>) method.code().map((code) -> code.elementStream()).orElse(java.util.stream.Stream.empty())::iterator) {
        if (!(element instanceof NewObjectInstruction made) || made.className().asInternalName().contains("/")) continue;
        Class<?> type = Class.forName(made.className().asInternalName(), true, client.loader);
        Field instance = null;
        for (Field field : type.getDeclaredFields()) if (Modifier.isStatic(field.getModifiers()) && field.getType() == type) instance = field;
        if (instance == null) continue;
        Object game = allocate(type);
        instance.setAccessible(true);
        instance.set(null, game);
        byte[] bytes;
        try (var stream = client.loader.getResourceAsStream(type.getName() + ".class")) {
          bytes = stream.readAllBytes();
        }
        for (var getter : ClassFile.of().parse(bytes).methods()) {
          if (getter.methodTypeSymbol().parameterCount() != 0 || !getter.methodTypeSymbol().returnType().descriptorString().equals(properties.descriptorString())) continue;
          for (CodeElement step : (Iterable<CodeElement>) getter.code().orElseThrow().elementStream()::iterator) {
            if (step instanceof FieldInstruction read && read.opcode() == Opcode.GETFIELD && read.typeSymbol().descriptorString().equals("Ljava/util/concurrent/CompletableFuture;")) {
              Field future = type.getDeclaredField(read.name().stringValue());
              future.setAccessible(true);
              future.set(game, CompletableFuture.completedFuture(offline));
            }
          }
        }
        games++;
      }
    }
    if (games != 1) throw new IllegalStateException("Found " + games + " games made by the client's Main, not one");
  }

  // A Level of this program's: a subclass of the client's Level made here,
  // every method it can override handing its call to `answer` (with the
  // method and its arguments), made without running a constructor.
  static Object level(McbenchClient client, Class<?> levelType, java.util.function.BiFunction<Method, Object[], Object> answer) throws Exception {
    var overridden = new LinkedHashMap<String, Method>();
    var seen = new java.util.HashSet<String>();
    for (Class<?> c = levelType; c != null && c != Object.class; c = c.getSuperclass()) {
      for (Method m : c.getDeclaredMethods()) {
        String key = m.getName() + java.lang.invoke.MethodType.methodType(m.getReturnType(), m.getParameterTypes()).toMethodDescriptorString();
        if (!seen.add(key)) continue;
        int mods = m.getModifiers();
        if (Modifier.isStatic(mods) || Modifier.isPrivate(mods) || Modifier.isFinal(mods) || m.isSynthetic() || !(Modifier.isPublic(mods) || Modifier.isProtected(mods))) continue;
        overridden.put(key, m);
      }
    }
    for (Method m : levelType.getMethods()) {
      String key = m.getName() + java.lang.invoke.MethodType.methodType(m.getReturnType(), m.getParameterTypes()).toMethodDescriptorString();
      if (seen.add(key) && !Modifier.isStatic(m.getModifiers()) && !Modifier.isFinal(m.getModifiers()) && m.getDeclaringClass() != Object.class && !m.isSynthetic()) overridden.put(key, m);
    }
    List<Method> list = new ArrayList<>(overridden.values());
    var self = java.lang.constant.ClassDesc.of("McbenchLevel" + System.identityHashCode(answer));
    var parent = java.lang.constant.ClassDesc.of(levelType.getName());
    var handlerType = java.lang.constant.ClassDesc.of("java.util.function.BiFunction");
    var objectType = java.lang.constant.ClassDesc.of("java.lang.Object");
    var objects = objectType.arrayType();
    byte[] bytes = ClassFile.of().build(self, (type) -> {
      type.withSuperclass(parent).withFlags(java.lang.reflect.AccessFlag.PUBLIC);
      type.withField("HANDLER", handlerType, Modifier.PUBLIC | Modifier.STATIC);
      for (int index = 0; index < list.size(); index++) {
        Method m = list.get(index);
        int at = index;
        var descriptor = java.lang.constant.MethodTypeDesc.ofDescriptor(java.lang.invoke.MethodType.methodType(m.getReturnType(), m.getParameterTypes()).toMethodDescriptorString());
        int flags = Modifier.isPublic(m.getModifiers()) ? Modifier.PUBLIC : Modifier.PROTECTED;
        type.withMethodBody(m.getName(), descriptor, flags, (code) -> {
          code.getstatic(self, "HANDLER", handlerType);
          code.loadConstant(at);
          code.invokestatic(java.lang.constant.ClassDesc.of("java.lang.Integer"), "valueOf", java.lang.constant.MethodTypeDesc.ofDescriptor("(I)Ljava/lang/Integer;"));
          Class<?>[] params = m.getParameterTypes();
          code.loadConstant(params.length);
          code.anewarray(objectType);
          int slot = 1;
          for (int p = 0; p < params.length; p++) {
            code.dup();
            code.loadConstant(p);
            Class<?> t = params[p];
            var kind = java.lang.classfile.TypeKind.from(t);
            code.loadLocal(kind, slot);
            slot += kind.slotSize();
            if (t.isPrimitive()) {
              Class<?> box = java.lang.invoke.MethodType.methodType(t).wrap().returnType();
              code.invokestatic(java.lang.constant.ClassDesc.of(box.getName()), "valueOf", java.lang.constant.MethodTypeDesc.of(java.lang.constant.ClassDesc.of(box.getName()), t.describeConstable().orElseThrow()));
            }
            code.aastore();
          }
          code.invokeinterface(handlerType, "apply", java.lang.constant.MethodTypeDesc.of(objectType, objectType, objectType));
          Class<?> r = m.getReturnType();
          if (r == void.class) {
            code.pop();
            code.return_();
          } else if (r.isPrimitive()) {
            Class<?> box = java.lang.invoke.MethodType.methodType(r).wrap().returnType();
            var boxDesc = java.lang.constant.ClassDesc.of(box.getName());
            code.checkcast(boxDesc);
            code.invokevirtual(boxDesc, r.getName() + "Value", java.lang.constant.MethodTypeDesc.of(r.describeConstable().orElseThrow()));
            code.return_(java.lang.classfile.TypeKind.from(r));
          } else {
            code.checkcast(r.describeConstable().orElseThrow());
            code.areturn();
          }
        });
      }
    });
    Class<?> made = new Defining(client.loader).define(bytes);
    made.getField("HANDLER").set(null, (java.util.function.BiFunction<Object, Object, Object>) (index, args) -> answer.apply(list.get((Integer) index), (Object[]) args));
    return allocate(made);
  }

  // A loader beside the client's, for the level made here.
  static final class Defining extends ClassLoader {
    Defining(ClassLoader parent) {
      super(parent);
    }

    Class<?> define(byte[] bytes) {
      return defineClass(null, bytes, 0, bytes.length);
    }
  }

  static List<Method> methods(Class<?> type) {
    return List.of(type.getMethods());
  }

  // Model.renderToBuffer or ModelPart.render: the object's one method of
  // (PoseStack, VertexConsumer, int, int, int).
  static Method renderMethod(Object target, Class<?> poseType) {
    for (Method method : methods(target.getClass())) {
      Class<?>[] p = method.getParameterTypes();
      if (p.length == 5 && p[0] == poseType && p[1].isInterface() && p[2] == int.class && p[3] == int.class && p[4] == int.class && method.getReturnType() == void.class) return method;
    }
    return null;
  }

  static boolean isRenderType(Object value) {
    for (Class<?> c = value.getClass(); c != null; c = c.getSuperclass()) for (Field f : c.getDeclaredFields()) if (f.getType().getName().equals("com.mojang.blaze3d.pipeline.RenderPipeline")) return true;
    return false;
  }

  static final Pattern MATERIAL_TEXTURE = Pattern.compile("texture=([a-z0-9_.-]+:[a-z0-9_./-]+)");
  static final Pattern TYPE_TEXTURE = Pattern.compile("([a-z0-9_.-]+):textures/([a-z0-9_./-]+)\\.png");

  static String texture(String material, Object renderType) {
    if (material != null) {
      var match = MATERIAL_TEXTURE.matcher(material);
      if (!match.find()) throw new IllegalStateException("A material without a texture: " + material);
      return match.group(1);
    }
    var match = TYPE_TEXTURE.matcher(String.valueOf(renderType));
    if (!match.find()) throw new IllegalStateException("A render type without a texture: " + renderType);
    return match.group(1) + ":" + match.group(2);
  }

  // One state through the renderer (BlockEntityRenderer's createRenderState,
  // extractRenderState and submit: its interface's methods of no, five and
  // four arguments).
  interface Placing {
    void place(Object blockEntity) throws Exception;
  }

  static void draw(McbenchClient client, Object renderer, Object type, Object state, Object origin, Map<Object, String> materials, List<Quad> quads, Placing placing) throws Exception {
    Class<?> face = null;
    for (Class<?> c = renderer.getClass(); face == null && c != null; c = c.getSuperclass()) for (Class<?> f : c.getInterfaces()) face = f;
    Method createState = null, extract = null, submit = null;
    for (Method method : methods(face)) {
      if (method.getParameterCount() == 0 && Modifier.isAbstract(method.getModifiers()) && !method.getReturnType().isPrimitive()) createState = method;
      if (method.getParameterCount() == 5) extract = method;
      if (method.getParameterCount() == 4 && method.getReturnType() == void.class) submit = method;
    }
    Object blockEntity = client.createBlockEntity.invoke(type, origin, state);
    placing.place(blockEntity);
    Object renderState = createState.invoke(renderer);
    // The camera, which no renderer at rest draws by, a few blocks off.
    Object camera = extract.getParameterTypes()[3].getConstructor(double.class, double.class, double.class).newInstance(0.5, 1.5, 3.5);
    extract.invoke(renderer, blockEntity, renderState, 0f, camera, null);
    Class<?> poseType = submit.getParameterTypes()[1];
    Object pose = poseType.getConstructor().newInstance();
    Class<?> collectorType = submit.getParameterTypes()[2];
    Object collector = Proxy.newProxyInstance(client.loader, new Class<?>[] { collectorType }, (proxy, method, arguments) -> {
      // A default method is the collector's own, which hands everything to
      // the method it calls.
      if (method.isDefault()) return InvocationHandler.invokeDefault(proxy, method, arguments);
      Object[] args = arguments == null ? new Object[0] : arguments;
      Object model = null, posed = null, renderType = null, sprite = null;
      int modelAt = -1;
      List<Integer> ints = new ArrayList<>();
      for (int i = 0; i < args.length; i++) {
        Object a = args[i];
        if (a == null) continue;
        if (poseType.isInstance(a)) posed = a;
        else if (materials.containsKey(a)) sprite = a;
        else if (a instanceof Integer n) ints.add(n);
        else if (model == null && renderMethod(a, poseType) != null) { model = a; modelAt = i; }
        else if (renderType == null && isRenderType(a)) renderType = a;
      }
      if (model == null) return text(client, method, args);
      // Model.setupAnim(state): the render state submitted after the model.
      if (modelAt + 1 < args.length && args[modelAt + 1] != null && !poseType.isInstance(args[modelAt + 1])) {
        for (Method setup : methods(model.getClass())) {
          if (setup.getParameterCount() == 1 && setup.getParameterTypes()[0] == Object.class && setup.getReturnType() == void.class) {
            setup.setAccessible(true);
            setup.invoke(model, args[modelAt + 1]);
          }
        }
      }
      // The submits that reach here (the collector's abstract ones) give
      // light, overlay, tint and outline, in that order.
      if (ints.size() != 4) throw new IllegalStateException("A model submitted with " + ints.size() + " ints: " + method);
      String texture = texture(sprite == null ? null : materials.get(sprite), renderType);
      Method render = renderMethod(model, poseType);
      List<float[]> vertices = new ArrayList<>();
      Object consumer = Proxy.newProxyInstance(client.loader, new Class<?>[] { render.getParameterTypes()[1] }, (inner, call, values) -> {
        // VertexConsumer.addVertex(x, y, z, color, u, v, overlay, light,
        // normal x, y, z), which ModelPart's cubes are drawn by.
        if (values == null || values.length != 11) throw new UnsupportedOperationException("A model asked its vertex consumer " + call);
        vertices.add(new float[] { (float) values[0], (float) values[1], (float) values[2], (float) values[4], (float) values[5], Float.intBitsToFloat((int) values[3]) });
        return null;
      });
      render.setAccessible(true);
      render.invoke(model, posed, consumer, ints.get(0), ints.get(1), ints.get(2));
      if (vertices.size() % 4 != 0) throw new IllegalStateException("A model drew " + vertices.size() + " vertices, not quads");
      for (int v = 0; v < vertices.size(); v += 4) {
        float[] position = new float[12], uv = new float[8];
        for (int k = 0; k < 4; k++) {
          float[] vertex = vertices.get(v + k);
          System.arraycopy(vertex, 0, position, k * 3, 3);
          uv[k * 2] = vertex[3];
          uv[k * 2 + 1] = vertex[4];
        }
        quads.add(new Quad(position, uv, Float.floatToRawIntBits(vertices.get(v)[5]), texture));
      }
      return null;
    });
    submit.invoke(renderer, renderState, pose, collector, submit.getParameterTypes()[3].getConstructor().newInstance());
  }

  // Text submitted (a sign's lines): FormattedCharSequence.accept(sink), the
  // method of an argument that hands a sink each character. A sign placed
  // holds no text, and nothing is drawn of it; any other submit is refused.
  static Object text(McbenchClient client, Method method, Object[] args) throws Exception {
    for (Object a : args) {
      if (a == null) continue;
      for (Class<?> face : a.getClass().getInterfaces()) for (Method accept : face.getMethods()) {
        if (!Modifier.isAbstract(accept.getModifiers()) || accept.getParameterCount() != 1 || !accept.getParameterTypes()[0].isInterface() || accept.getReturnType() != boolean.class) continue;
        int[] characters = { 0 };
        Object sink = Proxy.newProxyInstance(client.loader, new Class<?>[] { accept.getParameterTypes()[0] }, (proxy, call, values) -> {
          characters[0]++;
          return true;
        });
        accept.setAccessible(true);
        accept.invoke(a, sink);
        if (characters[0] > 0) throw new UnsupportedOperationException("A block entity renderer drew text of " + characters[0] + " characters");
        return null;
      }
    }
    throw new UnsupportedOperationException("A block entity renderer submitted " + method + " with " + Arrays.toString(args));
  }

  //   java McbenchBlockEntities client.jar blockTable <Minecraft's libraries> biome [state,...]
  // prints what is captured, and each quad of the states named.
  public static void main(String[] args) throws Exception {
    McbenchClient client = McbenchClient.read(java.nio.file.Path.of(args[0]), java.nio.file.Path.of(args[1]), args[2], args[3]);
    long started = System.nanoTime();
    McbenchBlockEntities captured = capture(client);
    long ms = (System.nanoTime() - started) / 1_000_000;
    int quads = captured.quads.values().stream().mapToInt(List::size).sum();
    System.out.println("captured " + captured.quads.size() + " states, " + quads + " quads in " + ms + " ms");
    captured.refused.forEach((type, why) -> System.out.println("refused " + type + ": " + why));
    System.out.println("other halves " + captured.partners.size() + " " + captured.partners.entrySet().stream().limit(4).map((e) -> e.getKey() + "=" + Arrays.toString(e.getValue())).toList());
    captured.asked.forEach((what) -> System.out.println("the level was asked " + what));
    for (String name : args.length > 4 ? args[4].split(";") : new String[0]) {
      for (var quad : captured.quads.getOrDefault(name, List.of())) System.out.println("quad " + name + " " + quad.texture() + " " + Integer.toHexString(quad.color()) + " " + Arrays.toString(quad.position()) + " " + Arrays.toString(quad.uv()));
    }
    System.exit(0);
  }
}
