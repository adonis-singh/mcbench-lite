import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.awt.image.BufferedImage;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.FileSystem;
import java.nio.file.FileSystems;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import javax.imageio.ImageIO;

// The viewer's entity assets, made from Minecraft's client jar (the one
// BlueMap downloads): one atlas of every texture the entity layer draws
// (web/entities.js) and its index. Nothing here is committed or served from
// anywhere but the bake's own folder.
//
//   McbenchEntityAssets.write(clientJar, atlas.png, index.json)
//
// Index (JSON):
//   atlas      { width, height }
//   textures   texture path under assets/minecraft/textures (block/stone,
//              item/redstone, entity/minecart, painting/alban, ...) -> [x, y, w, h]
//              in the atlas. An animated texture is its first frame.
//   models     model name (block/oak_stairs) -> { elements }: Minecraft's
//              model elements with the parents merged, every face's uv
//              written out (0..16 over its texture) and its texture named.
//              A model with no elements (chests, beds, banners ...) is the
//              full cube of its particle texture; chests are drawn from
//              their entity texture.
//   blocks     block id -> { variants: { "prop=value,...": { model, x, y } },
//              multipart: [{ when, apply: { model, x, y } }], tint: [r, g, b] }
//              (a variant list is its first variant)
//   items      item id -> { sprite } (a texture name; layers are laid over
//              each other) or { model, tint } (a block model drawn small)
//   entities   entity type -> entity texture
class McbenchEntityAssets {
  static final String ASSETS = "assets/minecraft/";
  static final int ATLAS_WIDTH = 2048;
  static final String[] FACES = { "down", "up", "north", "south", "west", "east" };
  static final String[] WOODS = { "oak", "spruce", "birch", "jungle", "acacia", "dark_oak", "mangrove", "cherry", "pale_oak", "bamboo" };
  // Textures the entity layer draws that no block or item model names.
  static final String[] ENTITY_TEXTURES = {
    "entity/minecart", "entity/armorstand/wood", "entity/projectiles/arrow", "entity/projectiles/spectral_arrow",
    "entity/projectiles/tipped_arrow", "painting/back",
  };

  final FileSystem jar;
  final Map<String, Resolved> resolved = new HashMap<>();
  final Map<String, JsonObject> outModels = new TreeMap<>();
  final Map<String, BufferedImage> synthesized = new TreeMap<>();
  final Map<String, Boolean> needed = new TreeMap<>();

  McbenchEntityAssets(FileSystem jar) {
    this.jar = jar;
  }

  record Resolved(Map<String, String> textures, JsonArray elements, boolean generated) {}

  static String plain(String name) {
    return name.startsWith("minecraft:") ? name.substring("minecraft:".length()) : name;
  }

  byte[] read(String path) throws IOException {
    return Files.readAllBytes(jar.getPath(ASSETS + path));
  }

  boolean exists(String path) {
    return Files.exists(jar.getPath(ASSETS + path));
  }

  JsonObject json(String path) throws IOException {
    return JsonParser.parseString(new String(read(path), StandardCharsets.UTF_8)).getAsJsonObject();
  }

  // A model with its parents merged: the child's textures win, and the
  // nearest model with elements gives them.
  Resolved resolve(String name) throws IOException {
    name = plain(name);
    Resolved known = resolved.get(name);
    if (known != null) return known;
    JsonObject model = json("models/" + name + ".json");
    Map<String, String> textures = new LinkedHashMap<>();
    JsonArray elements = new JsonArray();
    boolean generated = false;
    if (model.has("parent")) {
      String parent = plain(model.get("parent").getAsString());
      if (parent.equals("builtin/generated")) generated = true;
      else if (!parent.startsWith("builtin/")) {
        Resolved up = resolve(parent);
        textures.putAll(up.textures);
        elements = up.elements;
        generated = up.generated;
      }
    }
    if (model.has("textures")) {
      for (var entry : model.getAsJsonObject("textures").entrySet()) textures.put(entry.getKey(), entry.getValue().getAsString());
    }
    if (model.has("elements")) elements = model.getAsJsonArray("elements");
    Resolved result = new Resolved(textures, elements, generated);
    resolved.put(name, result);
    return result;
  }

  // A face's texture: a variable of the model ("#side", or "side" as heavy_core
  // writes it) or a texture path.
  static String texture(String reference, Map<String, String> textures, String context) {
    if (!reference.startsWith("#") && textures.containsKey(reference)) reference = "#" + reference;
    for (int hops = 0; reference.startsWith("#"); hops++) {
      String next = textures.get(reference.substring(1));
      if (next == null || hops > 16) throw new IllegalStateException("Unresolved texture " + reference + " in " + context);
      reference = next;
    }
    return plain(reference);
  }

  String use(String texture) {
    if (!exists("textures/" + texture + ".png")) throw new IllegalStateException("No texture " + texture);
    needed.put(texture, true);
    return texture;
  }

  static JsonArray numbers(double... values) {
    JsonArray array = new JsonArray();
    for (double value : values) array.add(value);
    return array;
  }

  static double[] doubles(JsonArray array) {
    double[] values = new double[array.size()];
    for (int i = 0; i < values.length; i++) values[i] = array.get(i).getAsDouble();
    return values;
  }

  // Minecraft's uv for a face with none given.
  static double[] defaultUv(String face, double[] f, double[] t) {
    return switch (face) {
      case "down" -> new double[] { f[0], 16 - t[2], t[0], 16 - f[2] };
      case "up" -> new double[] { f[0], f[2], t[0], t[2] };
      case "north" -> new double[] { 16 - t[0], 16 - t[1], 16 - f[0], 16 - f[1] };
      case "south" -> new double[] { f[0], 16 - t[1], t[0], 16 - f[1] };
      case "west" -> new double[] { f[2], 16 - t[1], t[2], 16 - f[1] };
      case "east" -> new double[] { 16 - t[2], 16 - t[1], 16 - f[2], 16 - f[1] };
      default -> throw new IllegalStateException(face);
    };
  }

  // The model as the index holds it, or null for a model that draws nothing.
  JsonObject convert(String name, Resolved model) {
    JsonObject out = new JsonObject();
    JsonArray elements = new JsonArray();
    for (JsonElement item : model.elements) {
      JsonObject element = item.getAsJsonObject();
      double[] from = doubles(element.getAsJsonArray("from"));
      double[] to = doubles(element.getAsJsonArray("to"));
      JsonObject converted = new JsonObject();
      converted.add("from", numbers(from));
      converted.add("to", numbers(to));
      if (element.has("rotation")) {
        JsonObject rotation = element.getAsJsonObject("rotation");
        JsonObject copy = new JsonObject();
        copy.add("origin", rotation.get("origin"));
        copy.add("axis", rotation.get("axis"));
        copy.add("angle", rotation.get("angle"));
        if (rotation.has("rescale")) copy.add("rescale", rotation.get("rescale"));
        converted.add("rotation", copy);
      }
      JsonObject faces = new JsonObject();
      for (var entry : element.getAsJsonObject("faces").entrySet()) {
        JsonObject face = entry.getValue().getAsJsonObject();
        JsonObject copy = new JsonObject();
        copy.add("uv", face.has("uv") ? face.get("uv") : numbers(defaultUv(entry.getKey(), from, to)));
        String texture = texture(face.get("texture").getAsString(), model.textures, name);
        if (!exists("textures/" + texture + ".png")) throw new IllegalStateException("No texture " + texture + " in " + name);
        copy.addProperty("texture", use(texture));
        if (face.has("rotation") && face.get("rotation").getAsInt() != 0) copy.add("rotation", face.get("rotation"));
        if (face.has("tintindex")) copy.addProperty("tinted", true);
        faces.add(entry.getKey(), copy);
      }
      converted.add("faces", faces);
      elements.add(converted);
    }
    out.add("elements", elements);
    return out;
  }

  JsonObject cubeOf(String texture) {
    JsonObject element = new JsonObject();
    element.add("from", numbers(0, 0, 0));
    element.add("to", numbers(16, 16, 16));
    JsonObject faces = new JsonObject();
    for (String face : FACES) {
      JsonObject entry = new JsonObject();
      entry.add("uv", numbers(0, 0, 16, 16));
      entry.addProperty("texture", use(texture));
      faces.add(face, entry);
    }
    element.add("faces", faces);
    JsonArray elements = new JsonArray();
    elements.add(element);
    JsonObject model = new JsonObject();
    model.add("elements", elements);
    return model;
  }

  // The model name the index uses for `name`: its own, or the particle cube
  // when it has no elements. Null when it draws nothing.
  String emit(String name) throws IOException {
    name = plain(name);
    if (outModels.containsKey(name)) return name;
    Resolved model = resolve(name);
    if (model.elements.size() > 0) {
      outModels.put(name, convert(name, model));
      return name;
    }
    if (model.generated || !model.textures.containsKey("particle")) return null;
    String particle = texture("#particle", model.textures, name);
    if (particle.equals("missingno")) return null; // air's models
    if (!exists("textures/" + particle + ".png")) throw new IllegalStateException("No texture " + particle + " in " + name);
    outModels.put(name, cubeOf(particle));
    return name;
  }

  // One box of a 64x64 entity texture laid out as Minecraft's cuboids are,
  // as a model element 16 over the whole texture: u, v the box's offset,
  // w x h x d its size, at (x, y, z) in block pixels.
  static JsonObject box(String texture, int size, double u, double v, double w, double h, double d, double x, double y, double z) {
    double k = 16.0 / size;
    JsonObject element = new JsonObject();
    element.add("from", numbers(x, y, z));
    element.add("to", numbers(x + w, y + h, z + d));
    JsonObject faces = new JsonObject();
    double[][] regions = {
      { u + d + w, v + d, u + d + 2 * w, v },                  // down
      { u + d, v, u + d + w, v + d },                          // up
      { u + d, v + d, u + d + w, v + d + h },                  // north
      { u + 2 * d + w, v + d, u + 2 * d + 2 * w, v + d + h },  // south
      { u, v + d, u + d, v + d + h },                          // west
      { u + d + w, v + d, u + 2 * d + w, v + d + h },          // east
    };
    for (int i = 0; i < FACES.length; i++) {
      JsonObject face = new JsonObject();
      double[] r = regions[i];
      face.add("uv", numbers(r[0] * k, r[1] * k, r[2] * k, r[3] * k));
      face.addProperty("texture", texture);
      faces.add(FACES[i], face);
    }
    element.add("faces", faces);
    return element;
  }

  // The chest block, which has no model of its own: its lid and body from
  // the chest entity texture.
  String chestModel(String id) {
    String base = id.replaceFirst("^waxed_", "");
    String name = switch (base) {
      case "chest" -> "normal";
      case "trapped_chest" -> "trapped";
      case "ender_chest" -> "ender";
      case "copper_chest" -> "copper";
      case "exposed_copper_chest" -> "copper_exposed";
      case "weathered_copper_chest" -> "copper_weathered";
      case "oxidized_copper_chest" -> "copper_oxidized";
      default -> null;
    };
    if (name == null) return null;
    String texture = use("entity/chest/" + name);
    String key = "special/chest/" + name;
    if (!outModels.containsKey(key)) {
      JsonArray elements = new JsonArray();
      elements.add(box(texture, 64, 0, 19, 14, 10, 14, 1, 0, 1));
      elements.add(box(texture, 64, 0, 0, 14, 5, 14, 1, 10, 1));
      JsonObject model = new JsonObject();
      model.add("elements", elements);
      outModels.put(key, model);
    }
    return key;
  }

  static int[] tint(String id) {
    return switch (id) {
      case "grass_block", "short_grass", "tall_grass", "fern", "large_fern", "sugar_cane", "bush" -> new int[] { 0x91, 0xBD, 0x59 };
      case "birch_leaves" -> new int[] { 0x80, 0xA7, 0x55 };
      case "spruce_leaves" -> new int[] { 0x61, 0x99, 0x61 };
      case "mangrove_leaves" -> new int[] { 0x8D, 0xB1, 0x27 };
      case "oak_leaves", "jungle_leaves", "acacia_leaves", "dark_oak_leaves", "vine" -> new int[] { 0x77, 0xAB, 0x2F };
      case "lily_pad" -> new int[] { 0x20, 0x80, 0x30 };
      case "water" -> new int[] { 0x3F, 0x76, 0xE4 };
      default -> null;
    };
  }

  static void setTint(JsonObject target, String id) {
    int[] color = tint(id);
    if (color != null) target.add("tint", numbers(color[0], color[1], color[2]));
  }

  static JsonObject placement(JsonObject variant, String model) {
    JsonObject out = new JsonObject();
    out.addProperty("model", model);
    if (variant.has("x") && variant.get("x").getAsInt() != 0) out.add("x", variant.get("x"));
    if (variant.has("y") && variant.get("y").getAsInt() != 0) out.add("y", variant.get("y"));
    return out;
  }

  static JsonObject first(JsonElement variant) {
    return variant.isJsonArray() ? variant.getAsJsonArray().get(0).getAsJsonObject() : variant.getAsJsonObject();
  }

  // The model a blockstate entry draws, or null when it draws nothing.
  String modelOf(String id, JsonObject variant) throws IOException {
    String chest = chestModel(id);
    return chest != null ? chest : emit(variant.get("model").getAsString());
  }

  JsonObject blocks() throws IOException {
    JsonObject blocks = new JsonObject();
    List<Path> files = new ArrayList<>();
    try (var stream = Files.list(jar.getPath(ASSETS + "blockstates"))) {
      stream.sorted().forEach(files::add);
    }
    for (Path file : files) {
      String id = file.getFileName().toString().replaceFirst("\\.json$", "");
      JsonObject state = json("blockstates/" + id + ".json");
      JsonObject block = new JsonObject();
      if (state.has("variants")) {
        JsonObject variants = new JsonObject();
        for (var entry : state.getAsJsonObject("variants").entrySet()) {
          JsonObject variant = first(entry.getValue());
          String model = modelOf(id, variant);
          if (model != null) variants.add(entry.getKey(), placement(variant, model));
        }
        block.add("variants", variants);
      }
      if (state.has("multipart")) {
        JsonArray parts = new JsonArray();
        for (JsonElement item : state.getAsJsonArray("multipart")) {
          JsonObject part = item.getAsJsonObject();
          JsonObject apply = first(part.get("apply"));
          String model = modelOf(id, apply);
          if (model == null) continue;
          JsonObject out = new JsonObject();
          if (part.has("when")) out.add("when", part.get("when"));
          out.add("apply", placement(apply, model));
          parts.add(out);
        }
        block.add("multipart", parts);
      }
      setTint(block, id);
      blocks.add("minecraft:" + id, block);
    }
    return blocks;
  }

  // The first model an item definition (items/<id>.json) shows.
  static String leaf(JsonObject node) {
    String type = plain(node.get("type").getAsString());
    return switch (type) {
      case "model" -> plain(node.get("model").getAsString());
      case "special" -> plain(node.get("base").getAsString());
      case "condition" -> leaf(node.getAsJsonObject("on_false"));
      case "select" -> leaf(node.has("fallback") ? node.getAsJsonObject("fallback") : node.getAsJsonArray("cases").get(0).getAsJsonObject().getAsJsonObject("model"));
      case "range_dispatch" -> leaf(node.has("fallback") ? node.getAsJsonObject("fallback") : node.getAsJsonArray("entries").get(0).getAsJsonObject().getAsJsonObject("model"));
      case "composite" -> leaf(node.getAsJsonArray("models").get(0).getAsJsonObject());
      case "empty" -> null;
      default -> throw new IllegalStateException("Unknown item model type " + type);
    };
  }

  JsonObject items() throws IOException {
    JsonObject items = new JsonObject();
    List<Path> files = new ArrayList<>();
    try (var stream = Files.list(jar.getPath(ASSETS + "items"))) {
      stream.sorted().forEach(files::add);
    }
    for (Path file : files) {
      String id = file.getFileName().toString().replaceFirst("\\.json$", "");
      String name = leaf(json("items/" + id + ".json").getAsJsonObject("model"));
      if (name == null) continue;
      Resolved model = resolve(name);
      JsonObject item = new JsonObject();
      if (model.generated) {
        List<String> layers = new ArrayList<>();
        for (int i = 0; model.textures.containsKey("layer" + i); i++) layers.add(texture(model.textures.get("layer" + i), model.textures, name));
        if (layers.isEmpty()) continue;
        if (layers.size() == 1) item.addProperty("sprite", use(layers.get(0)));
        else {
          String key = "composite/" + id;
          synthesized.put(key, layered(layers));
          needed.put(key, true);
          item.addProperty("sprite", key);
        }
      } else {
        String emitted = emit(name);
        if (emitted == null) continue;
        item.addProperty("model", emitted);
        setTint(item, id);
      }
      items.add("minecraft:" + id, item);
    }
    return items;
  }

  // The layers laid over each other, first at the bottom.
  BufferedImage layered(List<String> layers) throws IOException {
    BufferedImage out = null;
    for (String layer : layers) {
      BufferedImage image = load(layer);
      if (out == null) out = new BufferedImage(image.getWidth(), image.getHeight(), BufferedImage.TYPE_INT_ARGB);
      var graphics = out.createGraphics();
      graphics.drawImage(image, 0, 0, null);
      graphics.dispose();
    }
    return out;
  }

  // A texture; an animated one is its first frame.
  BufferedImage load(String texture) throws IOException {
    BufferedImage image;
    try (InputStream stream = Files.newInputStream(jar.getPath(ASSETS + "textures/" + texture + ".png"))) {
      image = ImageIO.read(stream);
    }
    String meta = "textures/" + texture + ".png.mcmeta";
    if (exists(meta) && json(meta).has("animation")) {
      JsonObject animation = json(meta).getAsJsonObject("animation");
      int w = animation.has("width") ? animation.get("width").getAsInt() : image.getWidth();
      int h = animation.has("height") ? animation.get("height").getAsInt() : (animation.has("width") ? image.getHeight() : image.getWidth());
      image = image.getSubimage(0, 0, w, h);
    }
    BufferedImage argb = new BufferedImage(image.getWidth(), image.getHeight(), BufferedImage.TYPE_INT_ARGB);
    var graphics = argb.createGraphics();
    graphics.drawImage(image, 0, 0, null);
    graphics.dispose();
    return argb;
  }

  JsonObject entities() throws IOException {
    JsonObject entities = new JsonObject();
    for (String type : new String[] { "minecart", "chest_minecart", "furnace_minecart", "hopper_minecart", "tnt_minecart", "spawner_minecart", "command_block_minecart" }) {
      entities.addProperty("minecraft:" + type, "entity/minecart");
    }
    for (String wood : WOODS) {
      boolean raft = wood.equals("bamboo");
      entities.addProperty("minecraft:" + wood + (raft ? "_raft" : "_boat"), "entity/boat/" + wood);
      entities.addProperty("minecraft:" + wood + (raft ? "_chest_raft" : "_chest_boat"), "entity/chest_boat/" + wood);
    }
    entities.addProperty("minecraft:armor_stand", "entity/armorstand/wood");
    entities.addProperty("minecraft:arrow", "entity/projectiles/arrow");
    entities.addProperty("minecraft:spectral_arrow", "entity/projectiles/spectral_arrow");
    for (var entry : entities.entrySet()) use(entry.getValue().getAsString());
    for (String texture : ENTITY_TEXTURES) use(texture);
    var paintings = Pattern.compile("^" + Pattern.quote(ASSETS + "textures/painting/") + "(.+)\\.png$");
    try (var stream = Files.list(jar.getPath(ASSETS + "textures/painting"))) {
      for (Path file : (Iterable<Path>) stream.sorted()::iterator) {
        Matcher match = paintings.matcher(ASSETS + "textures/painting/" + file.getFileName());
        if (match.matches()) use("painting/" + match.group(1));
      }
    }
    return entities;
  }

  record Placed(String name, BufferedImage image, int x, int y) {}

  // Shelf packing, tallest first, ATLAS_WIDTH wide.
  static Object[] pack(Map<String, BufferedImage> images, JsonObject rects) {
    List<Map.Entry<String, BufferedImage>> order = new ArrayList<>(images.entrySet());
    order.sort(Comparator.<Map.Entry<String, BufferedImage>>comparingInt(e -> -e.getValue().getHeight()).thenComparing(Map.Entry::getKey));
    List<Placed> placed = new ArrayList<>();
    int x = 0, y = 0, shelf = 0;
    for (var entry : order) {
      BufferedImage image = entry.getValue();
      if (image.getWidth() > ATLAS_WIDTH) throw new IllegalStateException(entry.getKey() + " is wider than the atlas");
      if (x + image.getWidth() > ATLAS_WIDTH) {
        y += shelf;
        x = 0;
        shelf = 0;
      }
      placed.add(new Placed(entry.getKey(), image, x, y));
      rects.add(entry.getKey(), numbers(x, y, image.getWidth(), image.getHeight()));
      x += image.getWidth();
      shelf = Math.max(shelf, image.getHeight());
    }
    int height = y + shelf;
    BufferedImage atlas = new BufferedImage(ATLAS_WIDTH, height, BufferedImage.TYPE_INT_ARGB);
    for (Placed p : placed) atlas.getGraphics().drawImage(p.image, p.x, p.y, null);
    return new Object[] { atlas, height };
  }

  static void write(Path clientJar, Path atlasPath, Path indexPath) throws Exception {
    try (FileSystem jar = FileSystems.newFileSystem(clientJar)) {
      McbenchEntityAssets assets = new McbenchEntityAssets(jar);
      JsonObject blocks = assets.blocks();
      JsonObject items = assets.items();
      JsonObject entities = assets.entities();
      Map<String, BufferedImage> images = new TreeMap<>();
      for (String texture : assets.needed.keySet()) {
        images.put(texture, assets.synthesized.containsKey(texture) ? assets.synthesized.get(texture) : assets.load(texture));
      }
      JsonObject rects = new JsonObject();
      Object[] packed = pack(images, rects);
      JsonObject atlas = new JsonObject();
      atlas.addProperty("width", ATLAS_WIDTH);
      atlas.addProperty("height", (Integer) packed[1]);
      JsonObject models = new JsonObject();
      for (var entry : assets.outModels.entrySet()) models.add(entry.getKey(), entry.getValue());
      JsonObject index = new JsonObject();
      index.add("atlas", atlas);
      index.add("textures", rects);
      index.add("models", models);
      index.add("blocks", blocks);
      index.add("items", items);
      index.add("entities", entities);
      ImageIO.write((BufferedImage) packed[0], "png", atlasPath.toFile());
      Files.writeString(indexPath, index.toString());
    }
  }

  public static void main(String[] args) throws Exception {
    write(Path.of(args[0]), Path.of(args[1]), Path.of(args[2]));
  }
}
