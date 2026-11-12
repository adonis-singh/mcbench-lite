// Fixes of 1.14 to 1.18 (data versions 1953 to 2999).

use super::util::*;
use super::*;

pub fn defs() -> Vec<(i32, &'static str, Imp)> {
    vec![
        (19530, "OminousBannerBlockEntityRenameFix", R(false, f131)), // 131
        (19550, "VillagerRebuildLevelAndXpFix", R(false, f132)), // 132
        (19550, "ZombieVillagerRebuildXpFix", R(false, f133)), // 133
        (19610, "ChunkLightRemoveFix", I), // 134
        (19630, "RemoveGolemGossipFix", R(false, f135)), // 135
        (21000, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 136 Added Bee and Bee Stinger
        (21000, "AddNewChoices", R(true, |fx| add_new_choices(fx, BLOCK_ENTITY))), // 137 Add beehive
        (21000, "NamespacedTypeRenameFix", I), // 138
        (21000, "AdvancementsRenameFix", I), // 139
        (22020, "ChunkBiomeFix", I), // 140
        (22090, "ItemRenameFix$1", R(false, |fx| item_rename(fx, renamer(&[("minecraft:bee_hive", "minecraft:beehive")])))), // 141
        (22090, "PoiTypeRenameFix", I), // 142
        (22090, "BlockRenameFix$1", R(false, |fx| block_rename(fx, renamer(&[("minecraft:bee_hive", "minecraft:beehive")])))), // 143
        (22110, "StructureReferenceCountFix", I), // 144
        (22180, "ForcePoiRebuild", I), // 145
        (25010, "FurnaceRecipeFix", R(true, f146)), // 146
        (25020, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 147 Added Hoglin
        (25030, "WallPropertyFix", R(false, f148)), // 148
        (25030, "AdvancementsRenameFix", I), // 149
        (25050, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 150 Added Piglin
        (25050, "MemoryExpiryDataFix", R(false, f151)), // 151
        (25080, "ItemRenameFix$1", R(false, |fx| item_rename(fx, renamer(FUNGI)))), // 152
        (25080, "BlockRenameFix$1", R(false, |fx| block_rename(fx, renamer(FUNGI)))), // 153
        (25090, "EntityZombifiedPiglinRenameFix", R(true, |fx| simplest_entity_rename(fx, |s| if s == "minecraft:zombie_pigman" { "minecraft:zombified_piglin".into() } else { s.to_string() }))), // 154
        (25090, "ItemRenameFix$1", R(false, |fx| item_rename(fx, renamer(&[("minecraft:zombie_pigman_spawn_egg", "minecraft:zombified_piglin_spawn_egg")])))), // 155
        (25110, "EntityProjectileOwnerFix", R(false, f156)), // 156
        (25111, "NamedEntityConvertUncheckedFix", R(true, |fx| named_entity_convert_unchecked(fx, ENTITY, "minecraft:potion"))), // 157 SplashPotionItemFieldRenameFix
        (25140, "EntityUUIDFix", R(false, f158)), // 158
        (25140, "BlockEntityUUIDFix", R(false, f159)), // 159
        (25140, "PlayerUUIDFix", I), // 160
        (25140, "LevelUUIDFix", I), // 161
        (25140, "SavedDataUUIDFix", I), // 162
        (25140, "ItemStackUUIDFix", R(false, f163)), // 163
        (25160, "GossipUUIDFix", R(false, |fx| gossip_uuid(fx, "minecraft:villager"))), // 164
        (25160, "GossipUUIDFix", R(false, |fx| gossip_uuid(fx, "minecraft:zombie_villager"))), // 165
        (25180, "JigsawPropertiesFix", R(false, f166)), // 166
        (25180, "JigsawRotationFix", R(false, f167)), // 167
        (25190, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 168 Added Strider
        (25220, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 169 Added Zoglin
        (25230, "AttributesRenameLegacy", R(false, |fx| super::v3::attributes_rename_legacy(fx, renamer_no_namespace(ATTRIBUTE_RENAMES)))), // 170
        (25270, "BitStorageAlignFix", I), // 171
        (25280, "ItemRenameFix$1", R(false, |fx| item_rename(fx, renamer(&[("minecraft:soul_fire_torch", "minecraft:soul_torch"), ("minecraft:soul_fire_lantern", "minecraft:soul_lantern")])))), // 172
        (25280, "BlockRenameFix$1", R(false, |fx| block_rename(fx, renamer(&[("minecraft:soul_fire_torch", "minecraft:soul_torch"), ("minecraft:soul_fire_wall_torch", "minecraft:soul_wall_torch"), ("minecraft:soul_fire_lantern", "minecraft:soul_lantern")])))), // 173
        (25290, "StriderGravityFix", R(false, f174)), // 174
        (25310, "RedstoneWireConnectionsFix", R(false, f175)), // 175
        (25330, "VillagerFollowRangeFix", R(false, f176)), // 176
        (25350, "EntityShulkerRotationFix", R(false, f177)), // 177
        (25370, "LegacyDimensionIdFix", I), // 178
        (25380, "LevelLegacyWorldGenSettingsFix", I), // 179
        (25500, "WorldGenSettingsFix", I), // 180
        (25510, "WriteAndReadFix", I), // 181
        (25520, "NamespacedTypeRenameFix", I), // 182
        (25530, "NamespacedTypeRenameFix", I), // 183
        (25580, "MissingDimensionFix", I), // 184
        (25580, "OptionsRenameFieldFix", I), // 185
        (25680, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 186 Added Piglin Brute
        (25710, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 187 Added Goat
        (26790, "CauldronRenameFix", R(false, f188)), // 188
        (26800, "ItemRenameFix$1", R(false, |fx| item_rename(fx, renamer(&[("minecraft:grass_path", "minecraft:dirt_path")])))), // 189
        (26800, "BlockRenameFix$1", R(false, |fx| block_rename(fx, renamer(&[("minecraft:grass_path", "minecraft:dirt_path")])))), // 190
        (26840, "AddNewChoices", R(true, |fx| add_new_choices(fx, BLOCK_ENTITY))), // 191 Added Sculk Sensor
        (26860, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 192 Added Axolotl
        (26880, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 193 Added Glow Squid
        (26880, "AddNewChoices", I), // 194 Added Glow Item Frame (its input type is gone after 193)
        (26900, "ItemRenameFix$1", R(false, |fx| item_rename(fx, renamer(COPPER_OXIDIZED)))), // 195
        (26900, "BlockRenameFix$1", R(false, |fx| block_rename(fx, renamer(COPPER_OXIDIZED)))), // 196
        (26910, "ItemRenameFix$1", R(false, |fx| item_rename(fx, renamer(COPPER_SUFFIXES)))), // 197
        (26910, "BlockRenameFix$1", R(false, |fx| block_rename(fx, renamer(COPPER_SUFFIXES)))), // 198
        (26930, "AddFlagIfNotPresentFix", I), // 199
        (26960, "ItemRenameFix$1", R(false, |fx| item_rename(fx, renamer(GRIMSTONE)))), // 200
        (26960, "BlockRenameFix$1", R(false, |fx| block_rename(fx, renamer(GRIMSTONE)))), // 201
        (27000, "BlockRenameFix$1", R(false, |fx| block_rename(fx, renamer(&[("minecraft:cave_vines_head", "minecraft:cave_vines"), ("minecraft:cave_vines_body", "minecraft:cave_vines_plant")])))), // 202
        (27010, "SavedDataFeaturePoolElementFix", I), // 203
        (27020, "AbstractArrowPickupFix", R(false, f204)), // 204
        (27040, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 205 Added Goat
        (27070, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 206 Added Marker
        (27070, "AddFlagIfNotPresentFix", I), // 207
        (27100, "StatsRenameFix", I), // 208
        (27170, "ItemRenameFix$1", R(false, |fx| item_rename(fx, renamer(&[("minecraft:azalea_leaves_flowers", "minecraft:flowering_azalea_leaves")])))), // 209
        (27170, "BlockRenameFix$1", R(false, |fx| block_rename(fx, renamer(&[("minecraft:azalea_leaves_flowers", "minecraft:flowering_azalea_leaves")])))), // 210
        (28250, "AddFlagIfNotPresentFix", I), // 211
        (28310, "SpawnerDataFix", R(true, f212)), // 212
        (28320, "WorldGenSettingsHeightAndBiomeFix", I), // 213
        (28320, "ChunkHeightAndBiomeFix", I), // 214
        (28330, "WorldGenSettingsDisallowOldCustomWorldsFix", I), // 215
        (28380, "NamespacedTypeRenameFix", I), // 216
        (28410, "ChunkProtoTickListFix", I), // 217
        (28420, "ChunkRenamesFix", I), // 218
        (28430, "OverreachingTickFix", I), // 219
        (28430, "NamespacedTypeRenameFix", I), // 220
        (28460, "AdvancementsRenameFix", I), // 221
        (28520, "WorldGenSettingsDisallowOldCustomWorldsFix", I), // 222
        (29670, "StructureSettingsFlattenFix", I), // 223
        (29700, "StructuresBecomeConfiguredFix", I), // 224
    ]
}

const FUNGI: &[(&str, &str)] = &[("minecraft:warped_fungi", "minecraft:warped_fungus"), ("minecraft:crimson_fungi", "minecraft:crimson_fungus")];

const ATTRIBUTE_RENAMES: &[(&str, &str)] = &[
    ("generic.maxHealth", "minecraft:generic.max_health"),
    ("Max Health", "minecraft:generic.max_health"),
    ("zombie.spawnReinforcements", "minecraft:zombie.spawn_reinforcements"),
    ("Spawn Reinforcements Chance", "minecraft:zombie.spawn_reinforcements"),
    ("horse.jumpStrength", "minecraft:horse.jump_strength"),
    ("Jump Strength", "minecraft:horse.jump_strength"),
    ("generic.followRange", "minecraft:generic.follow_range"),
    ("Follow Range", "minecraft:generic.follow_range"),
    ("generic.knockbackResistance", "minecraft:generic.knockback_resistance"),
    ("Knockback Resistance", "minecraft:generic.knockback_resistance"),
    ("generic.movementSpeed", "minecraft:generic.movement_speed"),
    ("Movement Speed", "minecraft:generic.movement_speed"),
    ("generic.flyingSpeed", "minecraft:generic.flying_speed"),
    ("Flying Speed", "minecraft:generic.flying_speed"),
    ("generic.attackDamage", "minecraft:generic.attack_damage"),
    ("generic.attackKnockback", "minecraft:generic.attack_knockback"),
    ("generic.attackSpeed", "minecraft:generic.attack_speed"),
    ("generic.armorToughness", "minecraft:generic.armor_toughness"),
];

const COPPER_OXIDIZED: &[(&str, &str)] = &[
    ("minecraft:weathered_copper_block", "minecraft:oxidized_copper_block"),
    ("minecraft:semi_weathered_copper_block", "minecraft:weathered_copper_block"),
    ("minecraft:lightly_weathered_copper_block", "minecraft:exposed_copper_block"),
    ("minecraft:weathered_cut_copper", "minecraft:oxidized_cut_copper"),
    ("minecraft:semi_weathered_cut_copper", "minecraft:weathered_cut_copper"),
    ("minecraft:lightly_weathered_cut_copper", "minecraft:exposed_cut_copper"),
    ("minecraft:weathered_cut_copper_stairs", "minecraft:oxidized_cut_copper_stairs"),
    ("minecraft:semi_weathered_cut_copper_stairs", "minecraft:weathered_cut_copper_stairs"),
    ("minecraft:lightly_weathered_cut_copper_stairs", "minecraft:exposed_cut_copper_stairs"),
    ("minecraft:weathered_cut_copper_slab", "minecraft:oxidized_cut_copper_slab"),
    ("minecraft:semi_weathered_cut_copper_slab", "minecraft:weathered_cut_copper_slab"),
    ("minecraft:lightly_weathered_cut_copper_slab", "minecraft:exposed_cut_copper_slab"),
    ("minecraft:waxed_semi_weathered_copper", "minecraft:waxed_weathered_copper"),
    ("minecraft:waxed_lightly_weathered_copper", "minecraft:waxed_exposed_copper"),
    ("minecraft:waxed_semi_weathered_cut_copper", "minecraft:waxed_weathered_cut_copper"),
    ("minecraft:waxed_lightly_weathered_cut_copper", "minecraft:waxed_exposed_cut_copper"),
    ("minecraft:waxed_semi_weathered_cut_copper_stairs", "minecraft:waxed_weathered_cut_copper_stairs"),
    ("minecraft:waxed_lightly_weathered_cut_copper_stairs", "minecraft:waxed_exposed_cut_copper_stairs"),
    ("minecraft:waxed_semi_weathered_cut_copper_slab", "minecraft:waxed_weathered_cut_copper_slab"),
    ("minecraft:waxed_lightly_weathered_cut_copper_slab", "minecraft:waxed_exposed_cut_copper_slab"),
];

const COPPER_SUFFIXES: &[(&str, &str)] = &[
    ("minecraft:waxed_copper", "minecraft:waxed_copper_block"),
    ("minecraft:oxidized_copper_block", "minecraft:oxidized_copper"),
    ("minecraft:weathered_copper_block", "minecraft:weathered_copper"),
    ("minecraft:exposed_copper_block", "minecraft:exposed_copper"),
];

const GRIMSTONE: &[(&str, &str)] = &[
    ("minecraft:grimstone", "minecraft:deepslate"),
    ("minecraft:grimstone_slab", "minecraft:cobbled_deepslate_slab"),
    ("minecraft:grimstone_stairs", "minecraft:cobbled_deepslate_stairs"),
    ("minecraft:grimstone_wall", "minecraft:cobbled_deepslate_wall"),
    ("minecraft:polished_grimstone", "minecraft:polished_deepslate"),
    ("minecraft:polished_grimstone_slab", "minecraft:polished_deepslate_slab"),
    ("minecraft:polished_grimstone_stairs", "minecraft:polished_deepslate_stairs"),
    ("minecraft:polished_grimstone_wall", "minecraft:polished_deepslate_wall"),
    ("minecraft:grimstone_tiles", "minecraft:deepslate_tiles"),
    ("minecraft:grimstone_tile_slab", "minecraft:deepslate_tile_slab"),
    ("minecraft:grimstone_tile_stairs", "minecraft:deepslate_tile_stairs"),
    ("minecraft:grimstone_tile_wall", "minecraft:deepslate_tile_wall"),
    ("minecraft:grimstone_bricks", "minecraft:deepslate_bricks"),
    ("minecraft:grimstone_brick_slab", "minecraft:deepslate_brick_slab"),
    ("minecraft:grimstone_brick_stairs", "minecraft:deepslate_brick_stairs"),
    ("minecraft:grimstone_brick_wall", "minecraft:deepslate_brick_wall"),
    ("minecraft:chiseled_grimstone", "minecraft:chiseled_deepslate"),
];

// OminousBannerBlockEntityRenameFix: the banner's name's translation key.
fn f131(fx: &Fx) -> RuleRef {
    let text = type_finder(fx.input().ty(super::TEXT_COMPONENT));
    named_entity_fix(fx, BLOCK_ENTITY, "minecraft:banner", move |typed| {
        let name = find_field(typed.ty, "CustomName");
        typed.update_typed(&name, |t1| {
            t1.update(&text, |v| {
                let (n, s) = v.into_pair();
                V::pair(n, V::Str(s.str().replace("\"translate\":\"block.minecraft.illager_banner\"", "\"translate\":\"block.minecraft.ominous_banner\"")))
            })
        })
    })
}

// VillagerRebuildLevelAndXpFix.
fn f132(fx: &Fx) -> RuleRef {
    let input = fx.input();
    let choice = input.choice_type(ENTITY, "minecraft:villager");
    let finder = named_choice("minecraft:villager", choice);
    let offers = find_field(choice, "Offers");
    let recipes = find_field(offers.ty(), "Recipes");
    let Ty::List(element) = t::ty(recipes.ty()) else { refuse("Recipes is no list (ClassCastException)") };
    let recipe = type_finder(element);
    fix_typed(input.ty(ENTITY), input.ty(ENTITY), move |typed| {
        typed.update_typed_to(&finder, choice, |t1| {
            let d = t1.remainder();
            let mut level = d.get("VillagerData").get("level").as_int(0);
            let mut t2 = t1.clone();
            if level == 0 || level == 1 {
                let n = t1.get_optional_typed(&offers).and_then(|t3| t3.get_optional_typed(&recipes)).map_or(0, |t3| t3.get_all_typed(&recipe).len() as i32);
                level = (n / 2).clamp(1, 5);
                if level > 1 {
                    t2 = t1.update_remainder(|d| d.update("VillagerData", |v| v.clone().set("level", v.create_int(level))));
                }
            }
            if d.get("Xp").as_number().result().is_none() {
                let xp = min_xp_per_level(level);
                t2 = t2.update_remainder(|d| {
                    let x = d.create_int(xp);
                    d.set("Xp", x)
                });
            }
            t2
        })
    })
}

fn min_xp_per_level(level: i32) -> i32 {
    [0, 10, 50, 100, 150][(level - 1).clamp(0, 4) as usize]
}

// ZombieVillagerRebuildXpFix.
fn f133(fx: &Fx) -> RuleRef {
    named_entity_remainder(fx, ENTITY, "minecraft:zombie_villager", |d| {
        if d.get("Xp").as_number().result().is_none() {
            let level = d.get("VillagerData").get("level").as_int(1);
            let xp = d.create_int(min_xp_per_level(level));
            d.set("Xp", xp)
        } else {
            d
        }
    })
}

// RemoveGolemGossipFix.
fn f135(fx: &Fx) -> RuleRef {
    named_entity_remainder(fx, ENTITY, "minecraft:villager", |d| {
        let d2 = d.clone();
        d.update("Gossips", |g| d2.create_list(g.as_stream().into_iter().filter(|x| x.get("Type").as_string_or("") != "golem")))
    })
}

// FurnaceRecipeFix: RecipeLocation/RecipeAmount pairs become RecipesUsed.
fn f146(fx: &Fx) -> RuleRef {
    let input = fx.input();
    let output = fx.output();
    let recipe = output.type_raw("recipe");
    let used = t::product(t::optional(t::field("RecipesUsed", t::product(t::mk(Ty::CList(recipe, t::prim(t::Prim::Int))), t::remainder()))), t::remainder());
    let furnaces: Vec<(Finder, TyId)> = ["minecraft:furnace", "minecraft:blast_furnace", "minecraft:smoker"].into_iter().map(|id| (named_choice(id, input.choice_type(BLOCK_ENTITY, id)), output.choice_type(BLOCK_ENTITY, id))).collect();
    fix_typed(input.ty(BLOCK_ENTITY), output.ty(BLOCK_ENTITY), move |mut typed| {
        for (finder, out) in &furnaces {
            typed = typed.update_typed_to(finder, *out, |data| {
                let mut d = data.get_or_create(&remainder_finder()).dy();
                let size = d.get("RecipesUsedSize").as_int(0);
                d = d.remove("RecipesUsedSize");
                let mut list = Vec::new();
                for i in 0..size {
                    let (loc, amount) = (format!("RecipeLocation{i}"), format!("RecipeAmount{i}"));
                    let location = d.get(&loc).result();
                    let count = d.get(&amount).as_int(0);
                    if count > 0 {
                        if let Some(l) = location {
                            if let Some((v, _)) = t::decode(recipe, &l.0).result() {
                                list.push(V::pair(v, V::Int(count)));
                            }
                        }
                    }
                    d = d.remove(&loc).remove(&amount);
                }
                let value = V::pair(V::Left(Box::new(V::pair(V::List(list), V::Dyn(d.empty_map().0)))), V::Dyn(d.0));
                data.set(&remainder_finder(), Typed::new(used, value))
            });
        }
        typed
    })
}

const WALL_BLOCKS: &[&str] = &[
    "minecraft:andesite_wall",
    "minecraft:brick_wall",
    "minecraft:cobblestone_wall",
    "minecraft:diorite_wall",
    "minecraft:end_stone_brick_wall",
    "minecraft:granite_wall",
    "minecraft:mossy_cobblestone_wall",
    "minecraft:mossy_stone_brick_wall",
    "minecraft:nether_brick_wall",
    "minecraft:prismarine_wall",
    "minecraft:red_nether_brick_wall",
    "minecraft:red_sandstone_wall",
    "minecraft:sandstone_wall",
    "minecraft:stone_brick_wall",
];

// WallPropertyFix.
fn f148(fx: &Fx) -> RuleRef {
    fix_remainder(fx.input().ty(BLOCK_STATE), |d| {
        let wall = d.get("Name").as_string().result().is_some_and(|n| WALL_BLOCKS.contains(&n.as_str()));
        if !wall {
            return d;
        }
        d.update("Properties", |mut p| {
            for side in ["east", "west", "north", "south"] {
                p = p.update(side, |v| match v.as_string().result() {
                    Some(s) => v.create_string(if s == "true" { "low" } else { "none" }),
                    None => v,
                });
            }
            p
        })
    })
}

// SimplestEntityRenameFix: entity choices and entity names renamed.
fn simplest_entity_rename(fx: &Fx, rename: fn(&str) -> String) -> RuleRef {
    let input = fx.input().find_choice_type(ENTITY);
    let output = fx.output().find_choice_type(ENTITY);
    let name_ty = t::named("entity_name", namespaced_string());
    if !t::same(fx.output().ty("entity_name"), name_ty) {
        refuse("Entity name type is not what was expected.");
    }
    let Ty::Choice(_, _, in_types) = t::ty(input) else { unreachable!() };
    let Ty::Choice(_, _, out_types) = t::ty(output) else { unreachable!() };
    let choices = fix_fn(input, output, move |v| {
        let (k, x) = v.into_pair();
        let new = rename(k.str());
        match (t::choice_get(&in_types, k.str()), t::choice_get(&out_types, &new)) {
            (Some(a), Some(b)) if t::eq(b, a, true, true) => {}
            _ => refuse("an entity rename's dynamic type check failed (IllegalStateException)"),
        }
        V::pair(V::Str(new), x)
    });
    let names = fix_fn(name_ty, name_ty, move |v| {
        let (n, s) = v.into_pair();
        V::pair(n, V::Str(rename(s.str())))
    });
    rewrite::seq(vec![choices, names])
}

fn uuid_tag(d: &Dy, most: i64, least: i64) -> Dy {
    d.create_int_list([(most >> 32) as i32, most as i32, (least >> 32) as i32, least as i32])
}

// EntityProjectileOwnerFix.
fn f156(fx: &Fx) -> RuleRef {
    fn set_uuid(d: Dy, most: i64, least: i64) -> Dy {
        if most != 0 && least != 0 {
            let u = uuid_tag(&d, most, least);
            d.set("OwnerUUID", u)
        } else {
            d
        }
    }
    fn arrow(d: Dy) -> Dy {
        let (m, l) = (d.get("OwnerUUIDMost").as_long(0), d.get("OwnerUUIDLeast").as_long(0));
        set_uuid(d, m, l).remove("OwnerUUIDMost").remove("OwnerUUIDLeast")
    }
    fn spit(d: Dy) -> Dy {
        let o = d.get("Owner");
        let (m, l) = (o.clone().get("OwnerUUIDMost").as_long(0), o.get("OwnerUUIDLeast").as_long(0));
        set_uuid(d, m, l).remove("Owner")
    }
    fn throwable(d: Dy) -> Dy {
        let o = d.get("owner");
        let (m, l) = (o.clone().get("M").as_long(0), o.get("L").as_long(0));
        set_uuid(d, m, l).remove("owner")
    }
    let input = fx.input();
    let output = fx.output();
    let list: Vec<(Finder, TyId, fn(Dy) -> Dy)> = [
        ("minecraft:egg", throwable as fn(Dy) -> Dy),
        ("minecraft:ender_pearl", throwable),
        ("minecraft:experience_bottle", throwable),
        ("minecraft:snowball", throwable),
        ("minecraft:potion", throwable),
        ("minecraft:llama_spit", spit),
        ("minecraft:arrow", arrow),
        ("minecraft:spectral_arrow", arrow),
        ("minecraft:trident", arrow),
    ]
    .into_iter()
    .map(|(id, f)| (named_choice(id, input.choice_type(ENTITY, id)), output.choice_type(ENTITY, id), f))
    .collect();
    fix_typed(input.ty(ENTITY), input.ty(ENTITY), move |mut t| {
        for (finder, out, f) in &list {
            t = t.update_typed_to(finder, *out, |t1| t1.update_remainder(f));
        }
        t
    })
}

// UUID.fromString (as fromString1 parses it).
fn uuid_from_string(s: &str) -> Option<(i64, i64)> {
    let units: Vec<u16> = s.encode_utf16().collect();
    if units.len() > 36 {
        return None;
    }
    if s.chars().any(|c| !c.is_ascii()) {
        refuse("a UUID string of non-ASCII characters (Long.parseLong's Character.digit)");
    }
    let dashes: Vec<usize> = s.match_indices('-').map(|(i, _)| i).collect();
    // The first five dashes found from each one after the last.
    if dashes.len() < 4 || dashes.len() > 4 {
        // dash5 >= 0, or fewer than four.
        if dashes.len() < 4 {
            return None;
        }
        return None;
    }
    let parse = |from: usize, to: usize| -> Option<i64> {
        let part = &s[from..to];
        let (neg, digits) = match part.as_bytes().first() {
            Some(b'-') => (true, &part[1..]),
            Some(b'+') => (false, &part[1..]),
            _ => (false, part),
        };
        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let mut v: i128 = 0;
        for c in digits.chars() {
            v = v * 16 + c.to_digit(16).unwrap() as i128;
            if v > i64::MAX as i128 + 1 {
                return None;
            }
        }
        let v = if neg { -v } else { v };
        (v >= i64::MIN as i128 && v <= i64::MAX as i128).then_some(v as i64)
    };
    let (d1, d2, d3, d4) = (dashes[0], dashes[1], dashes[2], dashes[3]);
    let mut most = parse(0, d1)? & 0xffffffff;
    most <<= 16;
    most |= parse(d1 + 1, d2)? & 0xffff;
    most <<= 16;
    most |= parse(d2 + 1, d3)? & 0xffff;
    let mut least = parse(d3 + 1, d4)? & 0xffff;
    least <<= 48;
    least |= parse(d4 + 1, s.len())? & 0xffffffffffff;
    Some((most, least))
}

fn create_uuid_from_string(d: &Dy, key: &str) -> Option<Dy> {
    let v = d.get(key).result()?;
    let s = v.as_string().result()?;
    let (m, l) = uuid_from_string(&s)?;
    Some(uuid_tag(d, m, l))
}

fn replace_uuid_string(d: &Dy, old: &str, new: &str) -> Option<Dy> {
    create_uuid_from_string(d, old).map(|u| d.clone().remove(old).set(new, u))
}

fn create_uuid_from_longs(d: &Dy, most: &str, least: &str) -> Option<Dy> {
    let (m, l) = (d.get(most).as_long(0), d.get(least).as_long(0));
    (m != 0 && l != 0).then(|| uuid_tag(d, m, l))
}

fn replace_uuid_ml_tag(d: &Dy, old: &str, new: &str) -> Option<Dy> {
    let v = d.get(old).result()?;
    create_uuid_from_longs(&v, "M", "L").map(|u| d.clone().remove(old).set(new, u))
}

pub fn replace_uuid_least_most(d: &Dy, old: &str, new: &str) -> Option<Dy> {
    let (most, least) = (format!("{old}Most"), format!("{old}Least"));
    create_uuid_from_longs(d, &most, &least).map(|u| d.clone().remove(&most).remove(&least).set(new, u))
}

fn update_living_entity(d: Dy) -> Dy {
    let d2 = d.clone();
    d.update("Attributes", |a| {
        d2.create_list(a.as_stream().into_iter().map(|x| {
            let x2 = x.clone();
            x.update("Modifiers", |m| x2.create_list(m.as_stream().into_iter().map(|y| replace_uuid_least_most(&y, "UUID", "UUID").unwrap_or(y))))
        }))
    })
}

fn update_mob(d: Dy) -> Dy {
    update_living_entity(d).update("Leash", |l| replace_uuid_least_most(&l, "UUID", "UUID").unwrap_or(l))
}

fn update_animal(d: Dy) -> Dy {
    let d = update_mob(d);
    replace_uuid_least_most(&d, "LoveCause", "LoveCause").unwrap_or(d)
}

fn update_animal_owner(d: Dy) -> Dy {
    let d = update_animal(d);
    replace_uuid_string(&d, "OwnerUUID", "Owner").unwrap_or(d)
}

fn update_projectile(d: Dy) -> Dy {
    match d.get("OwnerUUID").result() {
        Some(v) => d.remove("OwnerUUID").set("Owner", v),
        None => d,
    }
}

// EntityUUIDFix: UUIDMost/UUIDLeast pairs and UUID strings become int arrays.
fn f158(fx: &Fx) -> RuleRef {
    const HORSES: &[&str] = &["minecraft:donkey", "minecraft:horse", "minecraft:llama", "minecraft:mule", "minecraft:skeleton_horse", "minecraft:trader_llama", "minecraft:zombie_horse"];
    const TAMEABLE: &[&str] = &["minecraft:cat", "minecraft:parrot", "minecraft:wolf"];
    const ANIMALS: &[&str] = &["minecraft:bee", "minecraft:chicken", "minecraft:cow", "minecraft:fox", "minecraft:mooshroom", "minecraft:ocelot", "minecraft:panda", "minecraft:pig", "minecraft:polar_bear", "minecraft:rabbit", "minecraft:sheep", "minecraft:turtle", "minecraft:hoglin"];
    const MOBS: &[&str] = &[
        "minecraft:bat", "minecraft:blaze", "minecraft:cave_spider", "minecraft:cod", "minecraft:creeper", "minecraft:dolphin", "minecraft:drowned", "minecraft:elder_guardian", "minecraft:ender_dragon", "minecraft:enderman", "minecraft:endermite", "minecraft:evoker", "minecraft:ghast", "minecraft:giant", "minecraft:guardian", "minecraft:husk", "minecraft:illusioner", "minecraft:magma_cube", "minecraft:pufferfish", "minecraft:zombified_piglin", "minecraft:salmon", "minecraft:shulker", "minecraft:silverfish", "minecraft:skeleton", "minecraft:slime", "minecraft:snow_golem", "minecraft:spider", "minecraft:squid", "minecraft:stray", "minecraft:tropical_fish", "minecraft:vex", "minecraft:villager", "minecraft:iron_golem", "minecraft:vindicator", "minecraft:pillager", "minecraft:wandering_trader", "minecraft:witch", "minecraft:wither", "minecraft:wither_skeleton", "minecraft:zombie", "minecraft:zombie_villager", "minecraft:phantom", "minecraft:ravager", "minecraft:piglin",
    ];
    const PROJECTILES: &[&str] = &["minecraft:arrow", "minecraft:dragon_fireball", "minecraft:firework_rocket", "minecraft:fireball", "minecraft:llama_spit", "minecraft:small_fireball", "minecraft:snowball", "minecraft:spectral_arrow", "minecraft:egg", "minecraft:ender_pearl", "minecraft:experience_bottle", "minecraft:potion", "minecraft:trident", "minecraft:wither_skull"];
    let input = fx.input();
    let output = fx.output();
    let mut updates: Vec<(Finder, TyId, fn(Dy) -> Dy)> = Vec::new();
    let mut add = |id: &str, f: fn(Dy) -> Dy| updates.push((named_choice(id, input.choice_type(ENTITY, id)), output.choice_type(ENTITY, id), f));
    for id in HORSES.iter().chain(TAMEABLE) {
        add(id, update_animal_owner);
    }
    for id in ANIMALS {
        add(id, update_animal);
    }
    for id in MOBS {
        add(id, update_mob);
    }
    add("minecraft:armor_stand", update_living_entity);
    for id in PROJECTILES {
        add(id, update_projectile);
    }
    fn hurt_by(d: Dy) -> Dy {
        replace_uuid_string(&d, "HurtBy", "HurtBy").unwrap_or(d)
    }
    add("minecraft:bee", hurt_by);
    add("minecraft:zombified_piglin", hurt_by);
    add("minecraft:fox", |d| {
        let list = d.get("TrustedUUIDs").result().map(|l| d.create_list(l.as_stream().into_iter().map(|x| create_uuid_from_longs(&x, "M", "L").unwrap_or(x))));
        match list {
            Some(l) => d.remove("TrustedUUIDs").set("Trusted", l),
            None => d,
        }
    });
    add("minecraft:item", |d| {
        let d = replace_uuid_ml_tag(&d, "Owner", "Owner").unwrap_or(d);
        replace_uuid_ml_tag(&d, "Thrower", "Thrower").unwrap_or(d)
    });
    add("minecraft:shulker_bullet", |d| {
        let d = replace_uuid_ml_tag(&d, "Owner", "Owner").unwrap_or(d);
        replace_uuid_ml_tag(&d, "Target", "Target").unwrap_or(d)
    });
    add("minecraft:area_effect_cloud", |d| replace_uuid_least_most(&d, "OwnerUUID", "Owner").unwrap_or(d));
    add("minecraft:zombie_villager", |d| replace_uuid_least_most(&d, "ConversionPlayer", "ConversionPlayer").unwrap_or(d));
    add("minecraft:evoker_fangs", |d| replace_uuid_least_most(&d, "OwnerUUID", "Owner").unwrap_or(d));
    add("minecraft:piglin", |d| {
        d.update("Brain", |b| b.update("memories", |m| m.update("minecraft:angry_at", |a| replace_uuid_string(&a, "value", "value").unwrap_or(a))))
    });
    fix_typed(input.ty(ENTITY), input.ty(ENTITY), move |mut t| {
        t = t.update_remainder(|d| replace_uuid_least_most(&d, "UUID", "UUID").unwrap_or(d));
        for (finder, out, f) in &updates {
            t = t.update_typed_to(finder, *out, |t1| t1.update_remainder(f));
        }
        t
    })
}

// BlockEntityUUIDFix.
fn f159(fx: &Fx) -> RuleRef {
    let input = fx.input();
    let output = fx.output();
    let conduit = (named_choice("minecraft:conduit", input.choice_type(BLOCK_ENTITY, "minecraft:conduit")), output.choice_type(BLOCK_ENTITY, "minecraft:conduit"));
    let skull = (named_choice("minecraft:skull", input.choice_type(BLOCK_ENTITY, "minecraft:skull")), output.choice_type(BLOCK_ENTITY, "minecraft:skull"));
    fix_typed(input.ty(BLOCK_ENTITY), input.ty(BLOCK_ENTITY), move |t| {
        let t = t.update_typed_to(&conduit.0, conduit.1, |t1| t1.update_remainder(|d| replace_uuid_ml_tag(&d, "target_uuid", "Target").unwrap_or(d)));
        t.update_typed_to(&skull.0, skull.1, |t1| {
            t1.update_remainder(|d| match d.get("Owner").result() {
                Some(o) => {
                    let o = replace_uuid_string(&o, "Id", "Id").unwrap_or(o);
                    d.remove("Owner").set("SkullOwner", o)
                }
                None => d,
            })
        })
    })
}

// ItemStackUUIDFix.
fn f163(fx: &Fx) -> RuleRef {
    let ty = fx.input().ty(ITEM_STACK);
    let id = field_finder("id", t::named(ITEM_NAME, namespaced_string()));
    fix_typed(ty, ty, move |typed| {
        let tag = find_field(typed.ty, "tag");
        let head = typed.get_optional(&id).is_some_and(|v| v.into_pair().1.str() == "minecraft:player_head");
        typed.update_typed(&tag, |t1| {
            t1.update_remainder(|d| {
                let d2 = d.clone();
                let mut d = d.update("AttributeModifiers", |m| d2.create_list(m.as_stream().into_iter().map(|x| replace_uuid_least_most(&x, "UUID", "UUID").unwrap_or(x))));
                if head {
                    d = d.update("SkullOwner", |o| replace_uuid_string(&o, "Id", "Id").unwrap_or(o));
                }
                d
            })
        })
    })
}

// GossipUUIDFix.
fn gossip_uuid(fx: &Fx, entity: &'static str) -> RuleRef {
    named_entity_remainder(fx, ENTITY, entity, |d| {
        d.update("Gossips", |g| match g.as_stream_opt().result() {
            Some(items) => g.create_list(items.into_iter().map(|x| replace_uuid_least_most(&x, "Target", "Target").unwrap_or(x))),
            None => g,
        })
    })
}

// JigsawPropertiesFix.
fn f166(fx: &Fx) -> RuleRef {
    named_entity_remainder(fx, BLOCK_ENTITY, "minecraft:jigsaw", |d| {
        let kind = d.get("attachement_type").as_string_or("minecraft:empty");
        let pool = d.get("target_pool").as_string_or("minecraft:empty");
        let (k1, k2, p) = (d.create_string(&kind), d.create_string(&kind), d.create_string(&pool));
        d.set("name", k1).set("target", k2).remove("attachement_type").set("pool", p).remove("target_pool")
    })
}

// JigsawRotationFix.
fn f167(fx: &Fx) -> RuleRef {
    block_property_fix(fx, |n| n == "minecraft:jigsaw", |_, p| {
        let facing = p.get("facing").as_string_or("north");
        let orientation = match facing.as_str() {
            "down" => "down_south",
            "up" => "up_north",
            "north" => "north_up",
            "south" => "south_up",
            "west" => "west_up",
            "east" => "east_up",
            other => other,
        };
        let o = p.create_string(orientation);
        p.remove("facing").set("orientation", o)
    })
}

// StriderGravityFix.
fn f174(fx: &Fx) -> RuleRef {
    named_entity_remainder(fx, ENTITY, "minecraft:strider", |d| {
        if d.get("NoGravity").as_boolean(false) {
            let f = d.create_boolean(false);
            d.set("NoGravity", f)
        } else {
            d
        }
    })
}

// RedstoneWireConnectionsFix.
fn f175(fx: &Fx) -> RuleRef {
    fix_remainder(fx.input().ty(BLOCK_STATE), |d| {
        if d.get("Name").as_string().result().as_deref() != Some("minecraft:redstone_wire") {
            return d;
        }
        d.update("Properties", |p| {
            let side = |k: &str| p.get(k).as_string_or("none");
            let (e, w, n, s) = (side("east"), side("west"), side("north"), side("south"));
            let c = |x: &str| x != "none";
            let x_axis = c(&e) || c(&w);
            let z_axis = c(&n) || c(&s);
            let e2 = if !c(&e) && !z_axis { "side".to_string() } else { e };
            let w2 = if !c(&w) && !z_axis { "side".to_string() } else { w };
            let n2 = if !c(&n) && !x_axis { "side".to_string() } else { n };
            let s2 = if !c(&s) && !x_axis { "side".to_string() } else { s };
            p.update("east", |x| x.create_string(&e2)).update("west", |x| x.create_string(&w2)).update("north", |x| x.create_string(&n2)).update("south", |x| x.create_string(&s2))
        })
    })
}

// VillagerFollowRangeFix.
fn f176(fx: &Fx) -> RuleRef {
    named_entity_remainder(fx, ENTITY, "minecraft:villager", |d| {
        let d2 = d.clone();
        d.update("Attributes", |a| {
            d2.create_list(a.as_stream().into_iter().map(|x| {
                if x.get("Name").as_string_or("") == "generic.follow_range" && x.get("Base").as_double(0.0) == 16.0 {
                    let b = x.create_double(48.0);
                    x.set("Base", b)
                } else {
                    x
                }
            }))
        })
    })
}

// EntityShulkerRotationFix.
fn f177(fx: &Fx) -> RuleRef {
    named_entity_remainder(fx, ENTITY, "minecraft:shulker", |d| {
        let mut list: Vec<f64> = d.get("Rotation").as_stream().into_iter().map(|x| x.as_double(180.0)).collect();
        if list.is_empty() {
            return d;
        }
        list[0] -= 180.0;
        let l = d.create_list(list.into_iter().map(|v| d.create_double(v)));
        d.set("Rotation", l)
    })
}

// CauldronRenameFix.
fn f188(fx: &Fx) -> RuleRef {
    fix_remainder(fx.input().ty(BLOCK_STATE), |d| {
        if d.get("Name").as_string().result().as_deref() != Some("minecraft:cauldron") {
            return d;
        }
        if d.get("Properties").or_else_empty_map().get("level").as_string_or("0") == "0" {
            d.remove("Properties")
        } else {
            let n = d.create_string("minecraft:water_cauldron");
            d.set("Name", n)
        }
    })
}

// AbstractArrowPickupFix.
fn f204(fx: &Fx) -> RuleRef {
    let input = fx.input();
    let output = fx.output();
    let list: Vec<(Finder, TyId)> = ["minecraft:arrow", "minecraft:spectral_arrow", "minecraft:trident"].into_iter().map(|id| (named_choice(id, input.choice_type(ENTITY, id)), output.choice_type(ENTITY, id))).collect();
    fix_typed(input.ty(ENTITY), input.ty(ENTITY), move |mut t| {
        for (finder, out) in &list {
            t = t.update_typed_to(finder, *out, |t1| {
                t1.update_remainder(|d| {
                    if d.get("pickup").is_present() {
                        d
                    } else {
                        let p = d.create_byte(d.get("player").as_boolean(true) as i8);
                        d.set("pickup", p).remove("player")
                    }
                })
            });
        }
        t
    })
}

// MemoryExpiryDataFix (villagers): each brain memory wrapped as {value}.
fn f151(fx: &Fx) -> RuleRef {
    named_entity_remainder(fx, ENTITY, "minecraft:villager", |d| {
        d.update("Brain", |b| b.update("memories", |m| m.update_map_values(|k, v| (k, v.create_map([(v.create_string("value"), v.clone())])))))
    })
}

// SpawnerDataFix: SpawnData wrapped as an entry, SpawnPotentials as weighted entries.
fn f212(fx: &Fx) -> RuleRef {
    let ty = fx.input().ty("untagged_spawner");
    let out = fx.output().ty("untagged_spawner");
    let data = find_field(ty, "SpawnData");
    let data_ty = find_field(out, "SpawnData").ty();
    let potentials = find_field(ty, "SpawnPotentials");
    let potentials_ty = find_field(out, "SpawnPotentials").ty();
    fix_typed(ty, out, move |typed| {
        typed
            .update_typed_to(&data, data_ty, |t| Typed::new(data_ty, V::pair(t.v, V::Dyn(None))))
            .update_typed_to(&potentials, potentials_ty, |t| {
                let V::List(items) = t.v else { refuse("SpawnPotentials that are no list") };
                let items = items
                    .into_iter()
                    .map(|x| {
                        let (first, rem) = x.into_pair();
                        let rem = rem.dy();
                        let weight = rem.get("Weight").as_number().result().map_or(1, |n| n.int_value());
                        let w = Dy(None).set("weight", rem.create_int(weight));
                        V::pair(V::pair(first, V::Dyn(rem.remove("Weight").remove("Entity").0)), V::Dyn(w.0))
                    })
                    .collect();
                Typed::new(potentials_ty, V::List(items))
            })
    })
}
