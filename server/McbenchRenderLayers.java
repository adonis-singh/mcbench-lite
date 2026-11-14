import java.lang.classfile.ClassFile;
import java.lang.classfile.ClassModel;
import java.lang.classfile.MethodModel;
import java.lang.constant.ClassDesc;
import java.lang.constant.MethodTypeDesc;
import java.lang.reflect.AccessFlag;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Enumeration;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.jar.JarEntry;
import java.util.jar.JarFile;

// Writes the chunk render layer Minecraft's client draws each block state
// and its fluid in, asked of the client itself: ItemBlockRenderTypes's
// getChunkRenderType(BlockState) and getRenderLayer(FluidState), which the
// client's section compiler draws every block and fluid by. One line per
// layer, in the order of its enum (ChunkSectionLayer), then one per state,
// in the order of Minecraft's state ids:
//   layer  name  blended
//   state  blockLayer  fluidLayer
// blended is whether the layer's render pipeline has a blend function: the
// layers drawn blended, after the others, sorted (translucent and tripwire).
// The layers are the client's under its default graphics (Fancy), under
// which leaves are drawn cut out: ItemBlockRenderTypes.setCutoutLeaves.
//
//   java McbenchRenderLayers client.jar blockTable target <Minecraft's libraries, a class path>
//
// The client jar's classes are obfuscated, so the program finds what it
// asks by its shape: the layer enum is the one enum whose constants hold a
// RenderPipeline (com.mojang.blaze3d, not obfuscated); ItemBlockRenderTypes
// is the one class with two public static methods of one argument that give
// a layer, one for block states and one for fluid states, and one public
// static method that takes a boolean, the leaves' setting. Minecraft is
// bootstrapped by its own server entry point, which with --initSettings
// writes server.properties and eula.txt in the working directory and stops.
// The block states are the client's state registry (Block.BLOCK_STATE_REGISTRY:
// the one public static iterable among the block classes'), in order, each
// named as the block table names it, line for line.
public class McbenchRenderLayers {
  public static void main(String[] args) throws Exception {
    Path client = Path.of(args[0]);
    List<String> names = new ArrayList<>();
    for (String line : Files.readAllLines(Path.of(args[1]))) {
      String first = line.substring(0, line.indexOf('\t'));
      if (!first.equals("default") && !first.equals("faceoccludes") && !first.equals("skiprendering")) names.add(first);
    }
    Path target = Path.of(args[2]);
    Path partial = target.resolveSibling(target.getFileName() + ".part");

    ClassDesc pipeline = ClassDesc.of("com.mojang.blaze3d.pipeline.RenderPipeline");
    List<ClassModel> classes = new ArrayList<>();
    try (JarFile jar = new JarFile(client.toFile())) {
      for (Enumeration<JarEntry> entries = jar.entries(); entries.hasMoreElements();) {
        JarEntry entry = entries.nextElement();
        if (!entry.getName().endsWith(".class") || entry.getName().contains("/")) continue;
        classes.add(ClassFile.of().parse(jar.getInputStream(entry).readAllBytes()));
      }
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
    for (String library : args[3].split(java.io.File.pathSeparator)) urls.add(Path.of(library).toUri().toURL());
    ClassLoader loader = new URLClassLoader(urls.toArray(URL[]::new), ClassLoader.getPlatformClassLoader());
    Class.forName("net.minecraft.server.Main", true, loader).getMethod("main", String[].class).invoke(null, (Object) new String[] { "--initSettings" });

    Class<?> layer = Class.forName(layerClass.thisClass().asInternalName().replace('/', '.'), true, loader);
    Class<?> types = Class.forName(typesClass.thisClass().asInternalName().replace('/', '.'), true, loader);
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
    Iterable<?> states = (Iterable<?>) only("the block state registry", registries.stream().filter((registry) -> named((Iterable<?>) registry, names)).toList());
    Class<?> blockState = states.iterator().next().getClass();
    Method blockLayer = only("getChunkRenderType", byState.stream().filter((method) -> method.getParameterTypes()[0].isAssignableFrom(blockState)).toList());
    Method fluidLayer = only("getRenderLayer", byState.stream().filter((method) -> method != blockLayer).toList());
    Class<?> fluidState = fluidLayer.getParameterTypes()[0];
    Method getFluidState = only("BlockState.getFluidState", List.of(blockState.getMethods()).stream()
      .filter((method) -> method.getParameterCount() == 0 && method.getReturnType() == fluidState && !Modifier.isStatic(method.getModifiers())).toList());
    Field pipelineField = only("the layer's pipeline", List.of(layer.getDeclaredFields()).stream().filter((field) -> field.getType().getName().equals(pipeline.packageName() + "." + pipeline.displayName())).toList());
    pipelineField.setAccessible(true);
    Method blendFunction = pipelineField.getType().getMethod("getBlendFunction");

    try (var out = Files.newBufferedWriter(partial)) {
      for (Object constant : layer.getEnumConstants()) {
        boolean blended = ((Optional<?>) blendFunction.invoke(pipelineField.get(constant))).isPresent();
        out.write("layer\t" + ((Enum<?>) constant).name() + "\t" + (blended ? 1 : 0) + "\n");
      }
      for (Object state : states) {
        Enum<?> own = (Enum<?>) blockLayer.invoke(null, state);
        Enum<?> fluid = (Enum<?>) fluidLayer.invoke(null, getFluidState.invoke(state));
        out.write(name(state) + "\t" + own.ordinal() + "\t" + fluid.ordinal() + "\n");
      }
    }
    Files.move(partial, target);
    // Minecraft's entry point leaves threads behind.
    System.exit(0);
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

  static <T> T only(String what, List<T> found) {
    if (found.size() != 1) throw new IllegalStateException("Found " + found.size() + " candidates for " + what + " in the client jar, not one");
    return found.get(0);
  }
}
