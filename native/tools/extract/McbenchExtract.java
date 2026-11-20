import java.io.IOException;
import java.io.PrintStream;
import java.lang.reflect.Method;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.concurrent.Callable;
import java.util.concurrent.FutureTask;
import java.util.zip.ZipFile;

// Every file today's first start makes by running Minecraft's code, in ONE
// JVM: the engine's tables (McbenchEngine's dump, the loot tables,
// DataFixReference's dump), the block table (McbenchBlockTable) and the
// viewer's bake (McbenchBake with McbenchClient, McbenchBlockEntities,
// McbenchEntities, McbenchEntityAssets inside BlueMap's jar), and
// Minecraft's night (McbenchNight). Nothing of those programs is changed:
// this calls their entry points.
//
// Today they are four JVMs (and javac runs) on two classpaths. Here the
// JVM's own classpath is BlueMap's jar and the bake (as today's bake run),
// and Paper's server classpath with the server-side programs is a class
// loader of its own (`server`) beside it, so Minecraft's server is
// bootstrapped once for the block table, the engine and the data fixer,
// which then run on threads: the data fixer beside the block table, the
// engine once the block table is written. The block table binds the static
// registries' tags itself (McbenchBlockTable.bindTags) before it asks
// skipRendering, and the engine's boot binds them again (WorldLoader,
// updateStaticRegistryTags), so the engine waits: no thread binds tags
// while the table asks. The client is read in its own loader by
// McbenchClient, as today.
//
// The JVM runs in the work folder (the bake's staging folder: BlueMap's
// configuration and the engine's world data pack, laid out by prep.js), as
// the bake and the engine each run in theirs today.
//
//   java -cp <bluemap jar>:<these classes> McbenchExtract <out> <server classes> <paper classpath file>
//     <client jar> <mappings> <minecraft version> <biome> <block table name>
//
// <out> gets blocks.txt, loot.json, datafix.txt, the block table and
// night.json; the bake's files are written in the work folder as today. Each step's start
// and end go to stderr as `[extract] <ms since the JVM started> ms <what>`.
//
// This is release tooling (native/tools/extract/README.md): it runs on the
// release machine, never on a user's, and what it writes is never committed.
public class McbenchExtract {
  static final long START = ProcessHandle.current().info().startInstant().orElseThrow().toEpochMilli();
  static final PrintStream console = System.err;

  static void mark(String what) {
    long ms = System.currentTimeMillis() - START;
    console.println("[extract] " + ms + " ms " + Thread.currentThread().getName() + " " + what);
  }

  public static void main(String[] args) throws Throwable {
    mark("main");
    Path out = Path.of(args[0]).toAbsolutePath();
    Path serverClasses = Path.of(args[1]).toAbsolutePath();
    List<Path> classpath = Files.readAllLines(Path.of(args[2])).stream().filter((line) -> !line.isBlank()).map(Path::of).toList();
    Path clientJar = Path.of(args[3]).toAbsolutePath();
    Path mappings = Path.of(args[4]).toAbsolutePath();
    String version = args[5];
    String biome = args[6];
    Path blockTable = out.resolve(args[7]);
    Files.createDirectories(out);

    List<URL> urls = new ArrayList<>();
    urls.add(serverClasses.toUri().toURL());
    for (Path file : classpath) urls.add(file.toUri().toURL());
    URLClassLoader server = new URLClassLoader("paper", urls.toArray(URL[]::new), ClassLoader.getPlatformClassLoader());

    // Minecraft's server, bootstrapped once, on a thread of the server's loader.
    FutureTask<Void> bootstrap = task(server, "server-bootstrap", 0, () -> {
      mark("server bootstrap start");
      server.loadClass("net.minecraft.SharedConstants").getMethod("tryDetectVersion").invoke(null);
      server.loadClass("net.minecraft.server.Bootstrap").getMethod("bootStrap").invoke(null);
      mark("server bootstrap end");
      return null;
    });
    // The block table: McbenchBlockTable's main (its bootstrap is then done).
    FutureTask<Void> table = task(server, "block-table", 0, () -> {
      bootstrap.get();
      mark("block table start");
      server.loadClass("McbenchBlockTable").getMethod("main", String[].class).invoke(null, (Object) new String[] { blockTable.toString() });
      mark("block table end");
      return null;
    });
    // The data fixer's table (DataFixReference --dump), on a 16 MB stack as
    // today's run (-Xss16m).
    FutureTask<Void> datafix = task(server, "datafix", 16L << 20, () -> {
      bootstrap.get();
      mark("datafix start");
      Path file = out.resolve("datafix.txt");
      server.loadClass("DataFixReference").getMethod("main", String[].class).invoke(null, (Object) new String[] { "--dump", file + ".part" });
      Files.move(Path.of(file + ".part"), file, java.nio.file.StandardCopyOption.REPLACE_EXISTING);
      mark("datafix end");
      return null;
    });
    // The engine's dump: McbenchEngine's boot and live level, then DUMP, on
    // Paper's tick thread as the engine's process runs them, after the block
    // table (see above).
    FutureTask<Void> engine = new FutureTask<>(() -> {
      table.get();
      mark("engine boot start");
      Class<?> engineClass = server.loadClass("McbenchEngine");
      call(engineClass, "boot");
      mark("engine boot end");
      Object level = call(engineClass, "liveLevel");
      var field = engineClass.getDeclaredField("level");
      field.setAccessible(true);
      field.set(null, level);
      Method newRequest = level.getClass().getDeclaredMethod("newRequest");
      newRequest.setAccessible(true);
      newRequest.invoke(level);
      mark("engine level made");
      Path dump = out.resolve("blocks.dump");
      Method dumpMethod = engineClass.getDeclaredMethod("dump", Path.class);
      dumpMethod.setAccessible(true);
      dumpMethod.invoke(null, dump);
      mark("engine dump end");
      return null;
    });
    Thread tick = (Thread) server.loadClass("ca.spottedleaf.moonrise.common.util.TickThread").getConstructor(Runnable.class, String.class).newInstance(engine, "mcbench-engine");
    tick.setContextClassLoader(server);
    tick.start();

    // The loot tables, read from Paper's server jar while Minecraft boots.
    FutureTask<Void> loot = task(McbenchExtract.class.getClassLoader(), "loot", 0, () -> {
      mark("loot start");
      Path jar = classpath.stream().filter((file) -> file.getFileName().toString().equals("paper-" + version + ".jar")).findFirst().orElseThrow();
      writeLoot(jar, out.resolve("loot.json"));
      mark("loot end");
      return null;
    });

    // The bake waits for the block table, as today's bake does.
    table.get();
    mark("bake start");
    String libraries = String.join(java.io.File.pathSeparator, classpath.stream().filter((file) -> file.toString().contains("/libraries/")).map(Path::toString).toList());
    McbenchBake.logFailures();
    McbenchBake.bake(new String[] { version, blockTable.toString(), libraries, biome, mappings.toString() });
    mark("bake end");

    engine.get();
    writeBlocks(out.resolve("blocks.dump"), out.resolve("blocks.txt"));
    datafix.get();
    // Minecraft's data generator's reports (net.minecraft.data.Main
    // --reports, as `java -DbundlerMainClass=net.minecraft.data.Main -jar
    // server.jar --reports` runs it), into <out>/generated/reports, once
    // everything above is written: its blocks.json is the block registry's
    // source (mcbench-data stage).
    task(server, "reports", 0, () -> {
      mark("reports start");
      server.loadClass("net.minecraft.data.Main").getMethod("main", String[].class)
        .invoke(null, (Object) new String[] { "--reports", "--output", out.resolve("generated").toString() });
      mark("reports end");
      return null;
    }).get();
    loot.get();
    // Minecraft's night (night.json), once everything above is written, in
    // a client of its own: McbenchNight's client loads its stand-in for
    // LWJGL's memory functions, which the bake's client has not.
    mark("night start");
    McbenchNight.write(clientJar, mappings, blockTable, libraries, biome, out.resolve("night.json"));
    mark("night end");
    mark("all done");
    console.flush();
    System.exit(0);
  }

  static Object call(Class<?> type, String name) throws Exception {
    Method method = type.getDeclaredMethod(name);
    method.setAccessible(true);
    return method.invoke(null);
  }

  // `work` on a thread of its own with `loader` as its context loader
  // (`stack` bytes of stack, 0 for the default), started now.
  static FutureTask<Void> task(ClassLoader loader, String name, long stack, Callable<Void> work) {
    FutureTask<Void> task = new FutureTask<>(work);
    Thread thread = new Thread(null, task, name, stack);
    thread.setContextClassLoader(loader);
    thread.start();
    return task;
  }

  // blocks.txt as minecraft-tables.js writeRegistry makes it from the dump:
  // each run of lines of the types dumped from maps keyed by identity hash
  // (compost, itemcomponent) sorted (JavaScript's default sort: UTF-16 code
  // units, which String.compareTo is).
  static void writeBlocks(Path dump, Path target) throws IOException {
    String[] lines = Files.readString(dump, StandardCharsets.UTF_8).split("\n", -1);
    List<String> result = new ArrayList<>(Arrays.asList(lines));
    for (int start = 0; start < result.size();) {
      String type = result.get(start).split("\t", -1)[0];
      int end = start + 1;
      while (end < result.size() && result.get(end).split("\t", -1)[0].equals(type)) end++;
      if (type.equals("compost") || type.equals("itemcomponent")) {
        List<String> run = new ArrayList<>(result.subList(start, end));
        java.util.Collections.sort(run);
        for (int i = 0; i < run.size(); i++) result.set(start + i, run.get(i));
      }
      start = end;
    }
    Files.writeString(target, String.join("\n", result), StandardCharsets.UTF_8);
    Files.delete(dump);
  }

  // loot.json as minecraft-tables.js writes it: every block loot table of the
  // jar by key, keys sorted, each table as JSON.parse then JSON.stringify
  // leave it.
  static void writeLoot(Path jar, Path target) throws IOException {
    String prefix = "data/minecraft/loot_table/blocks/";
    Map<String, String> tables = new TreeMap<>();
    try (ZipFile zip = new ZipFile(jar.toFile())) {
      var entries = zip.entries();
      while (entries.hasMoreElements()) {
        var entry = entries.nextElement();
        String name = entry.getName();
        if (name.startsWith(prefix) && name.endsWith(".json")) {
          tables.put(name, new String(zip.getInputStream(entry).readAllBytes(), StandardCharsets.UTF_8));
        }
      }
    }
    StringBuilder json = new StringBuilder("{");
    boolean first = true;
    for (var entry : tables.entrySet()) {
      if (!first) json.append(',');
      first = false;
      Js.string(json, "minecraft:" + entry.getKey().substring(prefix.length(), entry.getKey().length() - ".json".length()));
      json.append(':');
      new Js(entry.getValue()).value(json);
    }
    json.append("}\n");
    Files.writeString(target, json, StandardCharsets.UTF_8);
  }

  // JSON.parse then JSON.stringify of one document, as JavaScript does them:
  // keys in order (a repeated key keeps its first place and last value; an
  // integer-like key, which JavaScript orders first, is refused), numbers as
  // JavaScript prints a double.
  static final class Js {
    final String text;
    int at;

    Js(String text) {
      this.text = text;
    }

    void space() {
      while (at < text.length() && " \t\n\r".indexOf(text.charAt(at)) >= 0) at++;
    }

    void value(StringBuilder out) {
      space();
      char c = text.charAt(at);
      if (c == '{') {
        at++;
        Map<String, StringBuilder> members = new java.util.LinkedHashMap<>();
        space();
        if (text.charAt(at) == '}') {
          at++;
        } else {
          while (true) {
            space();
            String key = parseString();
            if (!key.isEmpty() && key.chars().allMatch(Character::isDigit)) throw new IllegalStateException("integer-like key " + key);
            space();
            if (text.charAt(at++) != ':') throw new IllegalStateException("expected : at " + at);
            StringBuilder member = new StringBuilder();
            value(member);
            members.put(key, member);
            space();
            char next = text.charAt(at++);
            if (next == '}') break;
            if (next != ',') throw new IllegalStateException("expected , at " + at);
          }
        }
        out.append('{');
        boolean first = true;
        for (var member : members.entrySet()) {
          if (!first) out.append(',');
          first = false;
          string(out, member.getKey());
          out.append(':').append(member.getValue());
        }
        out.append('}');
      } else if (c == '[') {
        at++;
        out.append('[');
        space();
        if (text.charAt(at) == ']') {
          at++;
        } else {
          boolean first = true;
          while (true) {
            if (!first) out.append(',');
            first = false;
            value(out);
            space();
            char next = text.charAt(at++);
            if (next == ']') break;
            if (next != ',') throw new IllegalStateException("expected , at " + at);
          }
        }
        out.append(']');
      } else if (c == '"') {
        string(out, parseString());
      } else if (text.startsWith("true", at)) {
        at += 4;
        out.append("true");
      } else if (text.startsWith("false", at)) {
        at += 5;
        out.append("false");
      } else if (text.startsWith("null", at)) {
        at += 4;
        out.append("null");
      } else {
        int start = at;
        while (at < text.length() && "+-0123456789.eE".indexOf(text.charAt(at)) >= 0) at++;
        out.append(number(Double.parseDouble(text.substring(start, at))));
      }
    }

    String parseString() {
      if (text.charAt(at++) != '"') throw new IllegalStateException("expected string at " + at);
      StringBuilder s = new StringBuilder();
      while (true) {
        char c = text.charAt(at++);
        if (c == '"') return s.toString();
        if (c != '\\') {
          s.append(c);
          continue;
        }
        char e = text.charAt(at++);
        switch (e) {
          case 'b' -> s.append('\b');
          case 'f' -> s.append('\f');
          case 'n' -> s.append('\n');
          case 'r' -> s.append('\r');
          case 't' -> s.append('\t');
          case 'u' -> {
            s.append((char) Integer.parseInt(text.substring(at, at + 4), 16));
            at += 4;
          }
          default -> s.append(e);
        }
      }
    }

    // JSON.stringify of a string: ", \ and control characters escaped (lone
    // surrogates too, as \\uXXXX).
    static void string(StringBuilder out, String s) {
      out.append('"');
      for (int i = 0; i < s.length(); i++) {
        char c = s.charAt(i);
        switch (c) {
          case '"' -> out.append("\\\"");
          case '\\' -> out.append("\\\\");
          case '\b' -> out.append("\\b");
          case '\f' -> out.append("\\f");
          case '\n' -> out.append("\\n");
          case '\r' -> out.append("\\r");
          case '\t' -> out.append("\\t");
          default -> {
            boolean lone = Character.isSurrogate(c) && !(Character.isHighSurrogate(c) && i + 1 < s.length() && Character.isLowSurrogate(s.charAt(i + 1)))
              && !(Character.isLowSurrogate(c) && i > 0 && Character.isHighSurrogate(s.charAt(i - 1)));
            if (c < 0x20 || lone) out.append(String.format("\\u%04x", (int) c));
            else out.append(c);
          }
        }
      }
      out.append('"');
    }

    // Number.prototype.toString of a double (ECMA-262 Number::toString):
    // the shortest digits that round-trip (Double.toString's since JDK 19),
    // laid out by JavaScript's rules.
    static String number(double value) {
      if (value == 0) return "0";
      if (Double.isNaN(value) || Double.isInfinite(value)) return "null";
      if (value < 0) return "-" + number(-value);
      java.math.BigDecimal decimal = new java.math.BigDecimal(Double.toString(value)).stripTrailingZeros();
      String digits = decimal.unscaledValue().toString();
      int k = digits.length();
      int n = k - decimal.scale();
      if (k <= n && n <= 21) return digits + "0".repeat(n - k);
      if (0 < n && n <= 21) return digits.substring(0, n) + "." + digits.substring(n);
      if (-6 < n && n <= 0) return "0." + "0".repeat(-n) + digits;
      String exponent = (n - 1 >= 0 ? "+" : "-") + Math.abs(n - 1);
      return k == 1 ? digits + "e" + exponent : digits.charAt(0) + "." + digits.substring(1) + "e" + exponent;
    }
  }
}
