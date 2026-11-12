'use strict';
// Native API adapters. Every operation calls the selected server's own
// implementation; unsupported import modes fail explicitly.
// The plugin's sources for `version`: McbenchExperiment.java holds the
// version-sensitive code; class renames apply to both files.
function verifierSourcesForVersion(sources,version){
 return {'McbenchVerify.java':renamed(sources['McbenchVerify.java'],version),'McbenchExperiment.java':experimentSourceForVersion(sources['McbenchExperiment.java'],version)};
}
function experimentSourceForVersion(source,version){
 if(version==='1.21.4'){
  // 1.21.4 predates Optional NBT getters and ValueInput. Keep missing/wrong
  // field checks explicit; use that server's own loader and data fixer.
  source=source.replaceAll('.dataVersion().version()', '.getDataVersion().getVersion()')
   .replaceAll('TagParser.parseCompoundFully(', 'TagParser.parseTag(')
   .replaceAll('tag.getString("Name")', 'legacyString(tag,"Name")')
   .replaceAll('tag.getString("id")', 'legacyString(tag,"id")')
   .replaceAll('tag.getCompound("Properties")', 'legacyCompound(tag,"Properties")')
   .replaceAll('properties.getString(key)', 'legacyString(properties,key)')
   .replaceAll('properties.keySet()', 'properties.getAllKeys()')
   .replace('var problems = new net.minecraft.util.ProblemReporter.Collector();\n            entity.loadWithComponents(net.minecraft.world.level.storage.TagValueInput.create(problems, level.registryAccess(), tag));\n            if (!problems.isEmpty()) throw new IOException("Invalid block entity at " + entry.get("at") + ": " + problems.getReport());',
    '// The legacy loader has no structured ProblemReporter; retain its native\n            // exceptions and the saved-NBT audit below, rather than a dummy collector.\n            entity.loadWithComponents(tag, level.registryAccess());');
  const start=source.indexOf('    // BEGIN_VERSIONED_ENTITY_IMPORT');
  const end=source.indexOf('    // END_VERSIONED_ENTITY_IMPORT',start);
  if(start<0||end<0)throw new Error('Cannot locate legacy entity import boundary');
  source=source.slice(0,start)+`    private void importEntities() throws Exception {
        importedEntities = new JsonArray();
        if (request.has("entities") && !request.getAsJsonArray("entities").isEmpty())
            throw new IOException("Original entity import is not yet validated on Minecraft 1.21.4; no entities were omitted");
    }
    private static java.util.Optional<String> legacyString(net.minecraft.nbt.CompoundTag tag, String key) {
        return tag.contains(key,8) ? java.util.Optional.of(tag.getString(key)) : java.util.Optional.empty();
    }
    private static java.util.Optional<net.minecraft.nbt.CompoundTag> legacyCompound(net.minecraft.nbt.CompoundTag tag, String key) {
        return tag.contains(key,10) ? java.util.Optional.of(tag.getCompound(key)) : java.util.Optional.empty();
    }

`+source.slice(end+'    // END_VERSIONED_ENTITY_IMPORT'.length);
  return source;
 }
 return renamed(source,version);
}
function renamed(source,version){
 if(version==='1.21.4'||version==='1.21.8'||version==='1.21.10')return source;
 if(version!=='1.21.11')throw new Error('Unsupported Minecraft runtime version');
 const names={
  'net.minecraft.resources.ResourceLocation':'net.minecraft.resources.Identifier',
  'net.minecraft.world.entity.animal.Dolphin':'net.minecraft.world.entity.animal.dolphin.Dolphin',
  'net.minecraft.world.entity.vehicle.AbstractBoat':'net.minecraft.world.entity.vehicle.boat.AbstractBoat',
  'net.minecraft.world.entity.vehicle.AbstractMinecart':'net.minecraft.world.entity.vehicle.minecart.AbstractMinecart',
 };
 for(const[from,to]of Object.entries(names))source=source.replaceAll(from,to);
 return source;
}
module.exports={verifierSourcesForVersion};
