import com.mojang.datafixers.DSL;
import com.mojang.datafixers.DataFixUtils;
import com.mojang.datafixers.DataFixerUpper;
import com.mojang.datafixers.schemas.Schema;
import com.mojang.datafixers.types.Type;
import com.mojang.datafixers.types.templates.*;
import com.mojang.serialization.Dynamic;
import java.io.*;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;

// The Java side of scripts/datafix-parity.js: Minecraft's data fixer
// (DataFixers.getDataFixer(), Mojang's DataFixerUpper with the server's
// schemas and fixes) as the pinned Paper server runs it.
//
//   --dump <file>   writes the schemas the Rust port builds its types from
//                   (core/minecraft/<version>/datafix.txt): every schema's
//                   type templates as an interned graph, by version key.
//   --upgrade       reads lines "<type> <version> <snbt>" and writes, for
//                   each, the line "ok <snbt>" (DataFixer.update of the
//                   parsed tag to the server's version, as the experiment
//                   calls it) or "error <exception>".
public class DataFixReference {
  public static void main(String[] args) throws Exception {
    net.minecraft.SharedConstants.tryDetectVersion();
    net.minecraft.server.Bootstrap.bootStrap();
    if (args[0].equals("--dump")) dump(Path.of(args[1]));
    else if (args[0].equals("--upgrade")) upgrade();
    else if (args[0].equals("--block-entity-types")) blockEntityTypes();
    else if (args[0].equals("--vocabulary")) vocabulary();
    else throw new IllegalArgumentException(args[0]);
  }

  // Template graph: one line per distinct node, children before parents.
  static final Map<String, Integer> nodes = new HashMap<>();
  static final StringBuilder out = new StringBuilder();
  static final IdentityHashMap<Object, String> names = new IdentityHashMap<>();
  static final Map<String, Integer> nameCounts = new HashMap<>();

  // A primitive type or hook function: its class (or codec) name, with a
  // number when two distinct objects share one (they are unequal in DFU).
  static String name(Object object, String base) {
    return names.computeIfAbsent(object, o -> {
      int n = nameCounts.merge(base, 1, Integer::sum);
      return n == 1 ? base : base + "#" + n;
    });
  }

  static String constName(Type<?> type) {
    String cls = type.getClass().getName();
    if (type instanceof Const.PrimitiveType<?>) return name(type, "prim:" + type.toString());
    if (cls.equals("com.mojang.datafixers.types.constant.EmptyPart")) return name(type, "EmptyPart");
    if (cls.equals("com.mojang.datafixers.types.constant.EmptyPartPassthrough")) return name(type, "EmptyPartPassthrough");
    throw new IllegalStateException("Unknown constant type " + cls + " " + type);
  }

  static String hookName(Hook.HookFunction function) {
    if (function == Hook.HookFunction.IDENTITY) return "IDENTITY";
    return name(function, function.getClass().getName());
  }

  static int node(String line) {
    Integer id = nodes.get(line);
    if (id != null) return id;
    id = nodes.size();
    nodes.put(line, id);
    out.append("T ").append(id).append(' ').append(line).append('\n');
    return id;
  }

  static int template(TypeTemplate template) {
    if (template instanceof Const c) return node("const " + constName(c.type()));
    if (template instanceof Named n) return node("named " + n.name() + " " + template(n.element()));
    if (template instanceof Tag t) return node("field " + t.name() + " " + template(t.element()));
    if (template instanceof Product p) { int a = template(p.f()), b = template(p.g()); return node("and " + a + " " + b); }
    if (template instanceof Sum s) { int a = template(s.f()), b = template(s.g()); return node("or " + a + " " + b); }
    if (template instanceof com.mojang.datafixers.types.templates.List l) return node("list " + template(l.element()));
    if (template instanceof CompoundList c) { int a = template(c.key()), b = template(c.element()); return node("clist " + a + " " + b); }
    if (template instanceof Hook h) return node("hook " + template(h.element()) + " " + hookName(h.preRead()) + " " + hookName(h.postWrite()));
    if (template instanceof Check c) return node("check " + c.name() + " " + c.index() + " " + template(c.element()));
    if (template instanceof RecursivePoint r) return node("id " + r.index());
    if (template instanceof TaggedChoice<?> c) return choice(c);
    throw new IllegalStateException("Unknown template " + template.getClass());
  }

  static int choice(TaggedChoice<?> choice) {
    try {
      Field nameField = TaggedChoice.class.getDeclaredField("name"); nameField.setAccessible(true);
      Field keyField = TaggedChoice.class.getDeclaredField("keyType"); keyField.setAccessible(true);
      Field templatesField = TaggedChoice.class.getDeclaredField("templates"); templatesField.setAccessible(true);
      @SuppressWarnings("unchecked") Map<Object, TypeTemplate> templates = (Map<Object, TypeTemplate>) templatesField.get(choice);
      var keys = new ArrayList<String>();
      for (Object key : templates.keySet()) keys.add((String) key);
      Collections.sort(keys);
      var line = new StringBuilder("choice ").append(nameField.get(choice)).append(' ').append(constName((Type<?>) keyField.get(choice))).append(' ').append(keys.size());
      for (String key : keys) line.append(' ').append(key).append(' ').append(template(templates.get(key)));
      return node(line.toString());
    } catch (ReflectiveOperationException e) { throw new RuntimeException(e); }
  }

  static final DSL.TypeReference[] ROOTS = {
    net.minecraft.util.datafix.fixes.References.BLOCK_ENTITY, net.minecraft.util.datafix.fixes.References.ENTITY,
    net.minecraft.util.datafix.fixes.References.ENTITY_TREE, net.minecraft.util.datafix.fixes.References.ITEM_STACK,
    net.minecraft.util.datafix.fixes.References.BLOCK_STATE,
  };

  // Static tables (class, field) the port reads: maps of strings and sets of strings.
  static final String[][] TABLES = {
    { "net.minecraft.util.datafix.schemas.V99", "ITEM_TO_BLOCKENTITY" },
    { "net.minecraft.util.datafix.schemas.V99", "ITEM_TO_ENTITY" },
    { "net.minecraft.util.datafix.schemas.V704", "ITEM_TO_BLOCKENTITY" },
    { "net.minecraft.util.datafix.schemas.V705", "ITEM_TO_ENTITY" },
  };

  static void dump(Path file) throws Exception {
    var fixer = (DataFixerUpper) net.minecraft.util.datafix.DataFixers.getDataFixer();
    Field schemasField = DataFixerUpper.class.getDeclaredField("schemas"); schemasField.setAccessible(true);
    @SuppressWarnings("unchecked") var schemas = (it.unimi.dsi.fastutil.ints.Int2ObjectSortedMap<Schema>) schemasField.get(fixer);
    Field recursiveField = Schema.class.getDeclaredField("RECURSIVE_TYPES"); recursiveField.setAccessible(true);
    Field templatesField = Schema.class.getDeclaredField("TYPE_TEMPLATES"); templatesField.setAccessible(true);
    var schemaLines = new StringBuilder();
    for (var entry : schemas.int2ObjectEntrySet()) {
      Schema schema = entry.getValue();
      // In the order Schema.buildTypes reduces them into the family's template.
      var recursive = (it.unimi.dsi.fastutil.objects.Object2IntMap<String>) recursiveField.get(schema);
      @SuppressWarnings("unchecked") var templates = (Map<String, ?>) templatesField.get(schema);
      var typeNames = new ArrayList<>(templates.keySet());
      Collections.sort(typeNames);
      schemaLines.append("S ").append(schema.getVersionKey()).append(' ').append(schema.getClass().getSimpleName())
        .append(' ').append(schema instanceof net.minecraft.util.datafix.schemas.NamespacedSchema ? "namespaced" : "plain");
      schemaLines.append(" recursive");
      for (var e : recursive.object2IntEntrySet()) schemaLines.append(' ').append(e.getKey()).append('=').append(e.getIntValue());
      schemaLines.append(" types");
      for (String name : typeNames) schemaLines.append(' ').append(name).append('=').append(template(schema.resolveTemplate(name)));
      schemaLines.append('\n');
    }
    out.append(schemaLines);
    // The tables the schemas' hooks and the fixes read, as "X <table> <key> <value>" lines.
    for (String[] table : TABLES) {
      Field field = Class.forName(table[0]).getDeclaredField(table[1]);
      field.setAccessible(true);
      Object value = field.get(null);
      String name = table[0].substring(table[0].lastIndexOf('.') + 1) + "." + table[1];
      var map = (Map<?, ?>) value;
      var keys = new ArrayList<String>();
      for (Object key : map.keySet()) keys.add((String) key);
      Collections.sort(keys);
      for (String key : keys) out.append("X ").append(name).append(' ').append(key).append(' ').append(map.get(key)).append('\n');
    }
    // The default value (Type.point) of each schema's entity and block entity
    // choice types: the first choice of its hash map's order that has one,
    // "P <first key> <the choices' keys, sorted, joined by commas>".
    var points = new TreeSet<String>();
    for (Schema schema : schemas.values()) {
      for (var ref : new DSL.TypeReference[] { net.minecraft.util.datafix.fixes.References.ENTITY, net.minecraft.util.datafix.fixes.References.BLOCK_ENTITY }) {
        var choice = schema.getType(ref).findChoiceType("id", -1);
        if (choice.isEmpty()) continue;
        var point = choice.get().point(net.minecraft.nbt.NbtOps.INSTANCE);
        if (point.isEmpty()) continue;
        var keys = new ArrayList<String>();
        for (Object key : choice.get().types().keySet()) keys.add((String) key);
        Collections.sort(keys);
        points.add("P " + ((com.mojang.datafixers.util.Pair<?, ?>) point.get()).getFirst() + " " + String.join(",", keys) + "\n");
      }
    }
    for (String p : points) out.append(p);
    // The fixes in order: index, version key, class, and whether the rule
    // changes any of the types the engines upgrade (as the schema before the
    // fix has them).
    Field listField = DataFixerUpper.class.getDeclaredField("globalList"); listField.setAccessible(true);
    @SuppressWarnings("unchecked") var list = (java.util.List<com.mojang.datafixers.DataFix>) listField.get(fixer);
    // Each root type as the chain of fixes from the first schema rewrites it.
    Type<?>[] current = new Type<?>[ROOTS.length];
    for (int i = 0; i < ROOTS.length; i++) current[i] = schemas.get(schemas.firstIntKey()).getTypeRaw(ROOTS[i]);
    int index = 0;
    for (var fix : list) {
      boolean changes = false;
      for (int i = 0; i < ROOTS.length; i++) {
        var result = fix.getRule().rewrite(current[i]);
        if (result.isPresent() && !result.get().view().isNop()) {
          changes = true;
          current[i] = result.get().view().newType();
        }
      }
      String name = fix.getClass().getName();
      out.append("F ").append(index++).append(' ').append(fix.getVersionKey()).append(' ').append(name.substring(name.lastIndexOf('.') + 1)).append(' ').append(changes ? 1 : 0).append('\n');
    }
    Files.writeString(file, out.toString(), StandardCharsets.UTF_8);
    System.out.println(nodes.size() + " template nodes, " + schemas.size() + " schemas, " + list.size() + " fixes");
  }

  // "<block> <block entity type>" for every block whose default state has a
  // block entity: the id the experiment gives saved data without one.
  static void blockEntityTypes() {
    var output = new PrintStream(new FileOutputStream(FileDescriptor.out), false, StandardCharsets.UTF_8);
    for (var block : net.minecraft.core.registries.BuiltInRegistries.BLOCK) {
      if (block instanceof net.minecraft.world.level.block.EntityBlock entityBlock) {
        var entity = entityBlock.newBlockEntity(net.minecraft.core.BlockPos.ZERO, block.defaultBlockState());
        if (entity == null) continue;
        output.println("@@" + net.minecraft.core.registries.BuiltInRegistries.BLOCK.getKey(block) + " " + net.minecraft.world.level.block.entity.BlockEntityType.getKey(entity.getType()));
      }
    }
    output.flush();
  }

  // Every string constant of the data fixer's classes (net.minecraft.util.datafix
  // and below), a line each: the fuzzer's field names, ids and keys.
  static void vocabulary() throws Exception {
    var output = new PrintStream(new FileOutputStream(FileDescriptor.out), false, StandardCharsets.UTF_8);
    var jar = Path.of(net.minecraft.util.datafix.DataFixers.class.getProtectionDomain().getCodeSource().getLocation().toURI());
    var strings = new TreeSet<String>();
    try (var zip = new java.util.zip.ZipFile(jar.toFile())) {
      for (var entries = zip.entries(); entries.hasMoreElements();) {
        var entry = entries.nextElement();
        if (!entry.getName().startsWith("net/minecraft/util/datafix/") || !entry.getName().endsWith(".class")) continue;
        var model = java.lang.classfile.ClassFile.of().parse(zip.getInputStream(entry).readAllBytes());
        for (var constant : model.constantPool()) {
          if (constant instanceof java.lang.classfile.constantpool.StringEntry s) strings.add(s.stringValue());
        }
      }
    }
    for (String s : strings) if (!s.contains("\n")) output.println("@@" + s);
    output.flush();
  }

  static void upgrade() throws Exception {
    var fixer = net.minecraft.util.datafix.DataFixers.getDataFixer();
    int current = net.minecraft.SharedConstants.getCurrentVersion().dataVersion().version();
    var in = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
    var output = new PrintStream(new FileOutputStream(FileDescriptor.out), false, StandardCharsets.UTF_8);
    String line;
    while ((line = in.readLine()) != null) {
      int a = line.indexOf(' '), b = line.indexOf(' ', a + 1);
      String type = line.substring(0, a);
      int version = Integer.parseInt(line.substring(a + 1, b));
      String snbt = line.substring(b + 1);
      String result;
      try {
        var reference = switch (type) {
          case "block_entity" -> net.minecraft.util.datafix.fixes.References.BLOCK_ENTITY;
          case "entity" -> net.minecraft.util.datafix.fixes.References.ENTITY;
          case "entity_tree" -> net.minecraft.util.datafix.fixes.References.ENTITY_TREE;
          case "item_stack" -> net.minecraft.util.datafix.fixes.References.ITEM_STACK;
          case "block_state" -> net.minecraft.util.datafix.fixes.References.BLOCK_STATE;
          default -> throw new IllegalArgumentException(type);
        };
        var tag = net.minecraft.nbt.TagParser.parseCompoundFully(snbt);
        var fixed = fixer.update(reference, new Dynamic<>(net.minecraft.nbt.NbtOps.INSTANCE, tag), version, current).getValue();
        result = "@@ok " + fixed;
      } catch (Throwable error) {
        result = "@@error " + error.toString().replace('\n', ' ');
      }
      output.println(result);
      output.flush();
    }
  }
}
