import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.awt.image.BufferedImage;
import java.io.InputStream;
import java.nio.file.FileSystem;
import java.nio.file.FileSystems;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collection;
import java.util.Comparator;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import javax.imageio.ImageIO;

// The atlas the live viewer's entity layer (web/entities.mjs) and
// see_build's entities (web/viewer.js) draw entities with, made from
// Minecraft's client jar (the one BlueMap downloads): every texture named, by
// its name under textures/ ("minecraft:entity/minecart"), packed in shelves.
// An animated texture is its first frame (as the capture's sprites are,
// McbenchEntities.sprite). Nothing here is committed or served from anywhere
// but the bake's own folder.
//
//   McbenchEntityAssets.atlas(clientJar, textures, atlas.png) ->
//     { width, height, rects: { texture: [x, y, w, h] } }
class McbenchEntityAssets {
  static final int ATLAS_WIDTH = 2048;

  record Placed(String name, BufferedImage image, int x, int y) {}

  static BufferedImage load(FileSystem jar, String texture) throws Exception {
    String[] id = texture.split(":", 2);
    String path = "assets/" + id[0] + "/textures/" + id[1] + ".png";
    BufferedImage image;
    try (InputStream stream = Files.newInputStream(jar.getPath(path))) {
      image = ImageIO.read(stream);
    }
    Path meta = jar.getPath(path + ".mcmeta");
    if (Files.exists(meta) && JsonParser.parseString(Files.readString(meta)).getAsJsonObject().has("animation")) image = image.getSubimage(0, 0, image.getWidth(), image.getWidth());
    BufferedImage argb = new BufferedImage(image.getWidth(), image.getHeight(), BufferedImage.TYPE_INT_ARGB);
    var graphics = argb.createGraphics();
    graphics.drawImage(image, 0, 0, null);
    graphics.dispose();
    return argb;
  }

  static JsonObject atlas(Path clientJar, Collection<String> textures, Path atlasPath) throws Exception {
    Map<String, BufferedImage> images = new TreeMap<>();
    try (FileSystem jar = FileSystems.newFileSystem(clientJar)) {
      for (String texture : textures) images.put(texture, load(jar, texture));
    }
    // Shelf packing, tallest first.
    List<Map.Entry<String, BufferedImage>> order = new ArrayList<>(images.entrySet());
    order.sort(Comparator.<Map.Entry<String, BufferedImage>>comparingInt((entry) -> -entry.getValue().getHeight()).thenComparing(Map.Entry::getKey));
    List<Placed> placed = new ArrayList<>();
    JsonObject rects = new JsonObject();
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
      JsonArray rect = new JsonArray();
      for (int v : new int[] { x, y, image.getWidth(), image.getHeight() }) rect.add(v);
      rects.add(entry.getKey(), rect);
      x += image.getWidth();
      shelf = Math.max(shelf, image.getHeight());
    }
    int height = y + shelf;
    BufferedImage atlas = new BufferedImage(ATLAS_WIDTH, height, BufferedImage.TYPE_INT_ARGB);
    var graphics = atlas.createGraphics();
    for (Placed p : placed) graphics.drawImage(p.image(), p.x(), p.y(), null);
    graphics.dispose();
    ImageIO.write(atlas, "png", atlasPath.toFile());
    JsonObject out = new JsonObject();
    out.addProperty("width", ATLAS_WIDTH);
    out.addProperty("height", height);
    out.add("rects", rects);
    return out;
  }
}
