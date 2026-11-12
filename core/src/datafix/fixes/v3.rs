// Fixes of 1.19 to 1.20.6 (data versions 3000 to 3899), with the item
// stack componentization of 1.20.5.

use super::super::gson;
use super::util::*;
use super::*;

pub fn defs() -> Vec<(i32, &'static str, Imp)> {
    vec![
        (30760, "AddNewChoices", R(true, |fx| add_new_choices(fx, BLOCK_ENTITY))), // 225 Added Sculk Catalyst
        (30770, "ChunkDeleteIgnoredLightDataFix", I), // 226
        (30780, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 227 Added Frog
        (30780, "AddNewChoices", I), // 228 Added Tadpole (its input type is gone after 227)
        (30780, "AddNewChoices", R(true, |fx| add_new_choices(fx, BLOCK_ENTITY))), // 229 Added Sculk Shrieker
        (30810, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 230 Added Warden
        (30820, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 231 Added Chest Boat
        (30830, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 232 Added Allay
        (30840, "NamespacedTypeRenameFix", R(false, |fx| namespaced_type_rename(fx, "game_event_name", renamer(GAME_EVENT_RENAMES)))), // 233
        (30860, "EntityVariantFix", R(false, |fx| entity_variant(fx, "minecraft:cat", "CatType", cat_variant))), // 234
        (30860, "CriteriaRenameFix", I), // 235
        (30870, "EntityVariantFix", R(false, |fx| entity_variant(fx, "minecraft:frog", "Variant", frog_variant))), // 236
        (30900, "EntityFieldsRenameFix", R(false, |fx| entity_fields_rename(fx, "minecraft:painting", &[("Motive", "variant"), ("Facing", "facing")]))), // 237
        (30930, "EntityGoatMissingStateFix", R(false, f238)), // 238
        (30940, "GoatHornIdFix", R(false, f239)), // 239
        (30970, "FilteredBooksFix", R(false, f240)), // 240
        (30970, "FilteredSignsFix", R(false, |fx| named_entity_write_read(fx, BLOCK_ENTITY, "minecraft:sign", |d| d.remove("FilteredText1").remove("FilteredText2").remove("FilteredText3").remove("FilteredText4")))), // 241
        (30970, "VariantRenameFix", R(false, f242)), // 242
        (30970, "CriteriaRenameFix", I), // 243
        (30970, "PoiTypeRemoveFix", I), // 244
        (31080, "BlendingDataRemoveFromNetherEndFix", I), // 245
        (32010, "OptionsProgrammerArtFix", I), // 246
        (32020, "AddNewChoices", R(true, |fx| add_new_choices(fx, BLOCK_ENTITY))), // 247 Added Hanging Sign
        (32030, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 248 Added Camel
        (32040, "AddNewChoices", R(true, |fx| add_new_choices(fx, BLOCK_ENTITY))), // 249 Added Chiseled Bookshelf
        (32090, "ItemStackSpawnEggFix", R(false, |fx| item_stack_spawn_egg(fx, "minecraft:pig_spawn_egg"))), // 250
        (32140, "OptionsAmbientOcclusionFix", I), // 251
        (33190, "OptionsAccessibilityOnboardFix", I), // 252
        (33220, "EffectDurationFix", R(false, f253)), // 253
        (33250, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 254 Added displays
        (33260, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 255 Added Sniffer
        (33270, "AddNewChoices", R(true, |fx| add_new_choices(fx, BLOCK_ENTITY))), // 256 Archaeology
        (33280, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 257 Added interaction
        (34380, "BlockEntityRenameFix", R(true, |fx| block_entity_rename(fx, renamer(&[("minecraft:suspicious_sand", "minecraft:brushable_block")])))), // 258
        (34380, "EntityBrushableBlockFieldsRenameFix", R(false, |fx| named_entity_remainder(fx, BLOCK_ENTITY, "minecraft:brushable_block", |d| d.rename_field("loot_table", "LootTable").rename_field("loot_table_seed", "LootTableSeed")))), // 259
        (34380, "ItemRenameFix$1", R(false, |fx| item_rename(fx, renamer(POTTERY_SHARDS)))), // 260
        (34380, "AddNewChoices", I), // 261 Added calibrated sculk sensor (its input type is gone after 258)
        (34390, "BlockEntitySignDoubleSidedEditableTextFix", R(true, |fx| named_entity_write_read(fx, BLOCK_ENTITY, "minecraft:sign", sign_double_sided))), // 262
        (34391, "BlockEntitySignDoubleSidedEditableTextFix", R(true, |fx| named_entity_write_read(fx, BLOCK_ENTITY, "minecraft:hanging_sign", sign_double_sided))), // 263
        (34400, "NamespacedTypeRenameFix", I), // 264
        (34400, "FeatureFlagRemoveFix", I), // 265
        (34470, "ItemRenameFix$1", R(false, |fx| item_rename(fx, pottery_sherd))), // 266
        (34480, "DecoratedPotFieldRenameFix", R(true, f267)), // 267
        (34500, "RemapChunkStatusFix", I), // 268
        (34510, "ChunkDeleteLightFix", I), // 269
        (34590, "LegacyDragonFightFix", I), // 270
        (35640, "DropInvalidSignDataFix", R(false, |fx| drop_invalid_sign_data(fx, "minecraft:sign"))), // 271
        (35641, "DropInvalidSignDataFix", R(false, |fx| drop_invalid_sign_data(fx, "minecraft:hanging_sign"))), // 272
        (35650, "RandomSequenceSettingsFix", I), // 273
        (35660, "ScoreboardDisplaySlotFix", I), // 274
        (35680, "MobEffectIdFix", R(false, f275)), // 275
        (36820, "AddNewChoices", R(true, |fx| add_new_choices(fx, BLOCK_ENTITY))), // 276 Added Crafter
        (36830, "PrimedTntBlockStateFixer", R(true, f277)), // 277
        (36850, "FixProjectileStoredItem", R(true, f278)), // 278
        (36890, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 279 Added Breeze
        (36890, "AddNewChoices", R(true, |fx| add_new_choices(fx, BLOCK_ENTITY))), // 280 Added Trial Spawner
        (36920, "BlockRenameFix$1", R(false, |fx| block_rename(fx, renamer(&[("minecraft:grass", "minecraft:short_grass")])))), // 281
        (36920, "ItemRenameFix$1", R(false, |fx| item_rename(fx, renamer(&[("minecraft:grass", "minecraft:short_grass")])))), // 282
        (37990, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 283 Added Armadillo
        (38000, "ItemRenameFix$1", R(false, |fx| item_rename(fx, renamer(&[("minecraft:scute", "minecraft:turtle_scute")])))), // 284
        (38030, "RenameEnchantmentsFix", R(false, f285)), // 285
        (38070, "AddNewChoices", R(true, |fx| add_new_choices(fx, BLOCK_ENTITY))), // 286 Added Vault
        (38071, "MapBannerBlockPosFormatFix", I), // 287
        (38080, "HorseBodyArmorItemFix", R(true, |fx| horse_body_armor(fx, "minecraft:horse", "ArmorItem", true))), // 288
        (38081, "HorseBodyArmorItemFix", R(true, |fx| horse_body_armor(fx, "minecraft:llama", "DecorItem", false))), // 289
        (38082, "HorseBodyArmorItemFix", R(true, |fx| horse_body_armor(fx, "minecraft:trader_llama", "DecorItem", false))), // 290
        (38090, "ChestedHorsesInventoryZeroIndexingFix", R(false, f291)), // 291
        (38120, "FixWolfHealth", R(false, f292)), // 292
        (38130, "BlockPosFormatAndRenamesFix", R(true, f293)), // 293
        (38140, "AttributesRenameLegacy", R(false, |fx| attributes_rename_legacy(fx, renamer(&[("minecraft:horse.jump_strength", "minecraft:generic.jump_strength")])))), // 294
        (38160, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 295 Added Bogged
        (38180, "BeehiveFieldRenameFix", R(true, f296)), // 296
        (38180, "EmptyItemInHotbarFix", I), // 297
        (38181, "BannerPatternFormatFix", R(false, f298)), // 298
        (38182, "TippedArrowPotionToItemFix", R(false, |fx| named_entity_write_read(fx, ENTITY, "minecraft:arrow", tipped_arrow_potion))), // 299
        (38183, "WriteAndReadFix", R(true, |fx| write_fix_and_read(fx.input().ty(DATA_COMPONENTS), fx.output().ty(DATA_COMPONENTS), |d| d))), // 300 Inject data component types
        (38184, "ParticleUnflatteningFix", R(true, |fx| write_fix_and_read(fx.input().ty("particle"), fx.output().ty("particle"), unflatten_particle))), // 301
        (38185, "ItemStackComponentizationFix", R(true, f302)), // 302
        (38186, "AreaEffectCloudPotionFix", R(false, f303)), // 303
        (38200, "PlayerHeadBlockProfileFix", R(false, f304)), // 304
        (38200, "LodestoneCompassComponentFix", R(false, f305)), // 305
        (38250, "ItemStackCustomNameToOverrideComponentFix", R(false, f306)), // 306
        (38250, "BannerEntityCustomNameToOverrideComponentFix", R(false, f307)), // 307
        (38250, "TrialSpawnerConfigFix", R(true, |fx| named_entity_write_read(fx, BLOCK_ENTITY, "minecraft:trial_spawner", trial_spawner_config))), // 308
        (38250, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 309 Added Ominous Item Spawner
        (38280, "EmptyItemInVillagerTradeFix", R(false, f310)), // 310
        (38330, "RemoveEmptyItemInBrushableBlockFix", R(false, |fx| named_entity_write_read(fx, BLOCK_ENTITY, "minecraft:brushable_block", remove_empty_brushable_item))), // 311
    ]
}

const GAME_EVENT_RENAMES: &[(&str, &str)] = &[
    ("minecraft:block_press", "minecraft:block_activate"),
    ("minecraft:block_switch", "minecraft:block_activate"),
    ("minecraft:block_unpress", "minecraft:block_deactivate"),
    ("minecraft:block_unswitch", "minecraft:block_deactivate"),
    ("minecraft:drinking_finish", "minecraft:drink"),
    ("minecraft:elytra_free_fall", "minecraft:elytra_glide"),
    ("minecraft:entity_damaged", "minecraft:entity_damage"),
    ("minecraft:entity_dying", "minecraft:entity_die"),
    ("minecraft:entity_killed", "minecraft:entity_die"),
    ("minecraft:mob_interact", "minecraft:entity_interact"),
    ("minecraft:ravager_roar", "minecraft:entity_roar"),
    ("minecraft:ring_bell", "minecraft:block_change"),
    ("minecraft:shulker_close", "minecraft:container_close"),
    ("minecraft:shulker_open", "minecraft:container_open"),
    ("minecraft:wolf_shaking", "minecraft:entity_shake"),
];

const POTTERY_SHARDS: &[(&str, &str)] = &[
    ("minecraft:pottery_shard_archer", "minecraft:archer_pottery_shard"),
    ("minecraft:pottery_shard_prize", "minecraft:prize_pottery_shard"),
    ("minecraft:pottery_shard_arms_up", "minecraft:arms_up_pottery_shard"),
    ("minecraft:pottery_shard_skull", "minecraft:skull_pottery_shard"),
];

const SHARD_NAMES: &[&str] = &[
    "minecraft:angler_pottery_shard",
    "minecraft:archer_pottery_shard",
    "minecraft:arms_up_pottery_shard",
    "minecraft:blade_pottery_shard",
    "minecraft:brewer_pottery_shard",
    "minecraft:burn_pottery_shard",
    "minecraft:danger_pottery_shard",
    "minecraft:explorer_pottery_shard",
    "minecraft:friend_pottery_shard",
    "minecraft:heart_pottery_shard",
    "minecraft:heartbreak_pottery_shard",
    "minecraft:howl_pottery_shard",
    "minecraft:miner_pottery_shard",
    "minecraft:mourner_pottery_shard",
    "minecraft:plenty_pottery_shard",
    "minecraft:prize_pottery_shard",
    "minecraft:sheaf_pottery_shard",
    "minecraft:shelter_pottery_shard",
    "minecraft:skull_pottery_shard",
    "minecraft:snort_pottery_shard",
];

fn pottery_sherd(name: &str) -> String {
    let key = t::ensure_namespaced(name);
    if SHARD_NAMES.contains(&key.as_str()) {
        key.replace("_pottery_shard", "_pottery_sherd")
    } else {
        name.to_string()
    }
}

// NamespacedTypeRenameFix.
fn namespaced_type_rename(fx: &Fx, reference: &'static str, rename: impl Fn(&str) -> String + 'static) -> RuleRef {
    let ty = t::named(reference, namespaced_string());
    if !t::same(ty, fx.input().ty(reference)) {
        refuse("a namespaced type is not what was expected (IllegalStateException)");
    }
    fix_fn(ty, ty, move |v| {
        let (n, s) = v.into_pair();
        V::pair(n, V::Str(rename(s.str())))
    })
}

fn cat_variant(id: i32) -> Option<&'static str> {
    Some(match id {
        1 => "minecraft:black",
        2 => "minecraft:red",
        3 => "minecraft:siamese",
        4 => "minecraft:british",
        5 => "minecraft:calico",
        6 => "minecraft:persian",
        7 => "minecraft:ragdoll",
        8 => "minecraft:white",
        9 => "minecraft:jellie",
        10 => "minecraft:all_black",
        _ => "minecraft:tabby",
    })
}

fn frog_variant(id: i32) -> Option<&'static str> {
    match id {
        0 => Some("minecraft:temperate"),
        1 => Some("minecraft:warm"),
        2 => Some("minecraft:cold"),
        _ => None,
    }
}

// EntityVariantFix: the numbered variant field becomes "variant" (the old
// field stays), the number its name.
fn entity_variant(fx: &Fx, entity: &'static str, field: &'static str, names: fn(i32) -> Option<&'static str>) -> RuleRef {
    named_entity_remainder(fx, ENTITY, entity, move |d| match d.get(field).result() {
        None => d,
        Some(v) => {
            let fixed = match v.as_number().result() {
                Some(n) => match names(n.int_value()) {
                    Some(name) => v.create_string(name),
                    // A string of null: Dynamic.createString(null) makes a StringTag of null.
                    None => refuse("an entity variant number with no name (createString of null)"),
                },
                None => v,
            };
            d.set("variant", fixed)
        }
    })
}

// EntityGoatMissingStateFix.
fn f238(fx: &Fx) -> RuleRef {
    named_entity_remainder(fx, ENTITY, "minecraft:goat", |d| {
        let t = d.create_boolean(true);
        d.set("HasLeftHorn", t.clone()).set("HasRightHorn", t)
    })
}

// ItemStackTagFix.createFixer: the tag of the stacks whose id passes.
pub fn item_stack_tag_fixer(ty: TyId, filter: impl Fn(&str) -> bool + 'static, fixer: impl Fn(Typed) -> Typed + 'static) -> impl Fn(Typed) -> Typed + 'static {
    let id = field_finder("id", t::named(ITEM_NAME, namespaced_string()));
    let tag = find_field(ty, "tag");
    move |typed: Typed| match typed.get_optional(&id) {
        Some(v) if filter(v.clone().into_pair().1.str()) => typed.update_typed(&tag, &fixer),
        _ => typed,
    }
}

// ItemStackTagFix.
fn item_stack_tag_fix(fx: &Fx, filter: impl Fn(&str) -> bool + 'static, fixer: impl Fn(Typed) -> Typed + 'static) -> RuleRef {
    let ty = fx.input().ty(ITEM_STACK);
    fix_typed(ty, ty, item_stack_tag_fixer(ty, filter, fixer))
}

const INSTRUMENTS: &[&str] = &[
    "minecraft:ponder_goat_horn",
    "minecraft:sing_goat_horn",
    "minecraft:seek_goat_horn",
    "minecraft:feel_goat_horn",
    "minecraft:admire_goat_horn",
    "minecraft:call_goat_horn",
    "minecraft:yearn_goat_horn",
    "minecraft:dream_goat_horn",
];

// GoatHornIdFix.
fn f239(fx: &Fx) -> RuleRef {
    item_stack_tag_fix(fx, |id| id == "minecraft:goat_horn", |t| {
        t.update_remainder(|d| {
            let i = d.get("SoundVariant").as_int(0);
            let name = INSTRUMENTS[if (0..INSTRUMENTS.len() as i32).contains(&i) { i as usize } else { 0 }];
            let instrument = d.create_string(name);
            d.remove("SoundVariant").set("instrument", instrument)
        })
    })
}

// FilteredBooksFix.
fn f240(fx: &Fx) -> RuleRef {
    item_stack_tag_fix(fx, |id| id == "minecraft:writable_book" || id == "minecraft:written_book", |t| {
        let ty = t.ty;
        write_and_read_typed(&t, ty, |d| d.remove("filtered_title").remove("filtered_pages"))
    })
}

// VariantRenameFix (British shorthair cats).
fn f242(fx: &Fx) -> RuleRef {
    named_entity_remainder(fx, ENTITY, "minecraft:cat", |d| {
        d.update("variant", |v| match v.as_string().result() {
            Some(s) => v.create_string(if s == "minecraft:british" { "minecraft:british_shorthair" } else { &s }),
            None => v,
        })
    })
}

const SPAWN_EGGS: &[(&str, &str)] = &[
    ("minecraft:bat", "minecraft:bat_spawn_egg"),
    ("minecraft:blaze", "minecraft:blaze_spawn_egg"),
    ("minecraft:cave_spider", "minecraft:cave_spider_spawn_egg"),
    ("minecraft:chicken", "minecraft:chicken_spawn_egg"),
    ("minecraft:cow", "minecraft:cow_spawn_egg"),
    ("minecraft:creeper", "minecraft:creeper_spawn_egg"),
    ("minecraft:donkey", "minecraft:donkey_spawn_egg"),
    ("minecraft:elder_guardian", "minecraft:elder_guardian_spawn_egg"),
    ("minecraft:ender_dragon", "minecraft:ender_dragon_spawn_egg"),
    ("minecraft:enderman", "minecraft:enderman_spawn_egg"),
    ("minecraft:endermite", "minecraft:endermite_spawn_egg"),
    ("minecraft:evocation_illager", "minecraft:evocation_illager_spawn_egg"),
    ("minecraft:ghast", "minecraft:ghast_spawn_egg"),
    ("minecraft:guardian", "minecraft:guardian_spawn_egg"),
    ("minecraft:horse", "minecraft:horse_spawn_egg"),
    ("minecraft:husk", "minecraft:husk_spawn_egg"),
    ("minecraft:iron_golem", "minecraft:iron_golem_spawn_egg"),
    ("minecraft:llama", "minecraft:llama_spawn_egg"),
    ("minecraft:magma_cube", "minecraft:magma_cube_spawn_egg"),
    ("minecraft:mooshroom", "minecraft:mooshroom_spawn_egg"),
    ("minecraft:mule", "minecraft:mule_spawn_egg"),
    ("minecraft:ocelot", "minecraft:ocelot_spawn_egg"),
    ("minecraft:pufferfish", "minecraft:pufferfish_spawn_egg"),
    ("minecraft:parrot", "minecraft:parrot_spawn_egg"),
    ("minecraft:pig", "minecraft:pig_spawn_egg"),
    ("minecraft:polar_bear", "minecraft:polar_bear_spawn_egg"),
    ("minecraft:rabbit", "minecraft:rabbit_spawn_egg"),
    ("minecraft:sheep", "minecraft:sheep_spawn_egg"),
    ("minecraft:shulker", "minecraft:shulker_spawn_egg"),
    ("minecraft:silverfish", "minecraft:silverfish_spawn_egg"),
    ("minecraft:skeleton", "minecraft:skeleton_spawn_egg"),
    ("minecraft:skeleton_horse", "minecraft:skeleton_horse_spawn_egg"),
    ("minecraft:slime", "minecraft:slime_spawn_egg"),
    ("minecraft:snow_golem", "minecraft:snow_golem_spawn_egg"),
    ("minecraft:spider", "minecraft:spider_spawn_egg"),
    ("minecraft:squid", "minecraft:squid_spawn_egg"),
    ("minecraft:stray", "minecraft:stray_spawn_egg"),
    ("minecraft:turtle", "minecraft:turtle_spawn_egg"),
    ("minecraft:vex", "minecraft:vex_spawn_egg"),
    ("minecraft:villager", "minecraft:villager_spawn_egg"),
    ("minecraft:vindication_illager", "minecraft:vindication_illager_spawn_egg"),
    ("minecraft:witch", "minecraft:witch_spawn_egg"),
    ("minecraft:wither", "minecraft:wither_spawn_egg"),
    ("minecraft:wither_skeleton", "minecraft:wither_skeleton_spawn_egg"),
    ("minecraft:wolf", "minecraft:wolf_spawn_egg"),
    ("minecraft:zombie", "minecraft:zombie_spawn_egg"),
    ("minecraft:zombie_horse", "minecraft:zombie_horse_spawn_egg"),
    ("minecraft:zombie_pigman", "minecraft:zombie_pigman_spawn_egg"),
    ("minecraft:zombie_villager", "minecraft:zombie_villager_spawn_egg"),
];

// ItemStackSpawnEggFix: a spawn egg of this item named by its entity.
pub fn item_stack_spawn_egg(fx: &Fx, item: &'static str) -> RuleRef {
    let ty = fx.input().ty(ITEM_STACK);
    let id = field_finder("id", t::named(ITEM_NAME, namespaced_string()));
    let entity_id = field_finder("id", namespaced_string());
    let tag = find_field(ty, "tag");
    let entity_tag = find_field(tag.ty(), "EntityTag");
    fix_typed(ty, ty, move |typed| {
        if let Some(v) = typed.get_optional(&id) {
            if v.into_pair().1.str() == item {
                let t1 = typed.get_or_create_typed(&tag);
                let t2 = t1.get_or_create_typed(&entity_tag);
                if let Some(e) = t2.get_optional(&entity_id) {
                    let egg = SPAWN_EGGS.iter().find(|(k, _)| *k == e.str()).map_or("minecraft:pig_spawn_egg", |(_, v)| *v);
                    return typed.set_value(&id, V::pair(V::Str(ITEM_NAME.into()), V::Str(egg.into())));
                }
            }
        }
        typed
    })
}

// EffectDurationFix: an effect's factor data counts ticks active.
fn f253(fx: &Fx) -> RuleRef {
    fn fix_effect(d: Dy) -> Dy {
        let duration = d.get("Duration").as_int(-1);
        d.update("FactorCalculationData", |f| {
            let stamp = f.get("effect_changed_timestamp").as_int(-1);
            let f = f.remove("effect_changed_timestamp");
            let ticks = f.create_int(stamp.wrapping_sub(duration));
            f.set("ticks_active", ticks)
        })
    }
    fn fix(d: Dy) -> Dy {
        d.create_list(d.as_stream().into_iter().map(fix_effect))
    }
    let input = fx.input();
    let entities = fix_remainder(input.ty(ENTITY), |d| d.update("Effects", fix).update("ActiveEffects", fix).update("CustomPotionEffects", fix));
    let ty = input.ty(ITEM_STACK);
    let id = field_finder("id", t::named(ITEM_NAME, namespaced_string()));
    let tag = find_field(ty, "tag");
    let items = fix_typed(ty, ty, move |typed| {
        let potion = typed.get_optional(&id).is_some_and(|v| matches!(v.into_pair().1.str(), "minecraft:potion" | "minecraft:splash_potion" | "minecraft:lingering_potion" | "minecraft:tipped_arrow"));
        if potion {
            if let Some(t1) = typed.get_optional_typed(&tag) {
                let d = t1.remainder();
                let t2 = t1.set_value(&remainder_finder(), V::Dyn(d.update("CustomPotionEffects", fix).0));
                return typed.set(&tag, t2);
            }
        }
        typed
    });
    rewrite::seq(vec![entities, items])
}

// BlockEntityRenameFix: the block entity's choice renamed.
fn block_entity_rename(fx: &Fx, rename: impl Fn(&str) -> String + 'static) -> RuleRef {
    let input = fx.input().find_choice_type(BLOCK_ENTITY);
    let output = fx.output().find_choice_type(BLOCK_ENTITY);
    fix_fn(input, output, move |v| {
        let (k, x) = v.into_pair();
        V::pair(V::Str(rename(k.str())), x)
    })
}

// BlockEntitySignDoubleSidedEditableTextFix.fix.
fn sign_double_sided(tag: Dy) -> Dy {
    let empty = empty_component();
    let lines = |prefix: &str| (1..=4).map(|i| tag.get(&format!("{prefix}{i}")).result()).collect::<Vec<Option<Dy>>>();
    let messages: Vec<Dy> = lines("Text").into_iter().map(|o| o.unwrap_or_else(|| empty.clone())).collect();
    let color = tag.get("Color").result().unwrap_or_else(|| tag.create_string("black"));
    let glowing = tag.get("GlowingText").result().unwrap_or_else(|| tag.create_boolean(false));
    let mut front = tag.empty_map().set("messages", tag.create_list(messages.clone())).set("color", color).set("has_glowing_text", glowing);
    let filtered = lines("FilteredText");
    if filtered.iter().any(Option::is_some) {
        let list = filtered.into_iter().enumerate().map(|(i, o)| o.unwrap_or_else(|| messages[i].clone()));
        front = front.clone().set("filtered_messages", tag.create_list(list));
    }
    let empty_lines = tag.create_list((0..4).map(|_| empty.clone()));
    let back = tag.empty_map().set("messages", empty_lines).set("color", tag.create_string("black")).set("has_glowing_text", tag.create_boolean(false));
    let mut tag = tag.clone().set("front_text", front).set("back_text", back).set("is_waxed", tag.create_boolean(false)).set("_filtered_correct", tag.create_boolean(true));
    for f in SIGN_FIELDS_TO_DROP {
        tag = tag.remove(f);
    }
    tag
}

const SIGN_FIELDS_TO_DROP: &[&str] = &["Text1", "Text2", "Text3", "Text4", "FilteredText1", "FilteredText2", "FilteredText3", "FilteredText4", "Color", "GlowingText"];

// DecoratedPotFieldRenameFix: convertUnchecked of the pot's choice.
fn f267(fx: &Fx) -> RuleRef {
    convert_unchecked(fx.input().choice_type(BLOCK_ENTITY, "minecraft:decorated_pot"), fx.output().choice_type(BLOCK_ENTITY, "minecraft:decorated_pot"))
}

// DropInvalidSignDataFix.
fn drop_invalid_sign_data(fx: &Fx, entity: &'static str) -> RuleRef {
    let ty = fx.input().ty(BLOCK_ENTITY);
    let choice = fx.input().choice_type(BLOCK_ENTITY, entity);
    let finder = named_choice(entity, choice);
    fn fix_text(text: Dy) -> Dy {
        let Some(filtered) = text.get("filtered_messages").as_stream_opt().result() else { return text };
        let empty = empty_component();
        let messages = text.get("messages").as_stream_opt().result().unwrap_or_default();
        let fixed: Vec<Dy> = filtered.into_iter().enumerate().map(|(i, f)| if f == empty { messages.get(i).cloned().unwrap_or_else(|| empty.clone()) } else { f }).collect();
        if fixed == messages {
            text.remove("filtered_messages")
        } else {
            let list = text.create_list(fixed);
            text.set("filtered_messages", list)
        }
    }
    fix_typed(ty, ty, move |typed| {
        typed.update_typed_to(&finder, choice, |t1| {
            if t1.remainder().get("_filtered_correct").as_boolean(false) {
                t1.update_remainder(|d| d.remove("_filtered_correct"))
            } else {
                write_and_read_typed(&t1, choice, |mut d| {
                    d = d.update("front_text", fix_text).update("back_text", fix_text);
                    for f in SIGN_FIELDS_TO_DROP {
                        d = d.remove(f);
                    }
                    d
                })
            }
        })
    })
}

const MOB_EFFECT_IDS: &[&str] = &[
    "minecraft:speed",
    "minecraft:slowness",
    "minecraft:haste",
    "minecraft:mining_fatigue",
    "minecraft:strength",
    "minecraft:instant_health",
    "minecraft:instant_damage",
    "minecraft:jump_boost",
    "minecraft:nausea",
    "minecraft:regeneration",
    "minecraft:resistance",
    "minecraft:fire_resistance",
    "minecraft:water_breathing",
    "minecraft:invisibility",
    "minecraft:blindness",
    "minecraft:night_vision",
    "minecraft:hunger",
    "minecraft:weakness",
    "minecraft:poison",
    "minecraft:wither",
    "minecraft:health_boost",
    "minecraft:absorption",
    "minecraft:saturation",
    "minecraft:glowing",
    "minecraft:levitation",
    "minecraft:luck",
    "minecraft:unluck",
    "minecraft:slow_falling",
    "minecraft:conduit_power",
    "minecraft:dolphins_grace",
    "minecraft:bad_omen",
    "minecraft:hero_of_the_village",
    "minecraft:darkness",
];

fn mob_effect_id(d: &Dy, key: &str) -> Option<Dy> {
    let n = d.get(key).as_number().result()?.int_value();
    if (1..=MOB_EFFECT_IDS.len() as i32).contains(&n) {
        Some(d.create_string(MOB_EFFECT_IDS[n as usize - 1]))
    } else {
        None
    }
}

fn mob_effect_id_field(old: &Dy, old_name: &str, new: Dy, new_name: &str) -> Dy {
    let id = mob_effect_id(old, old_name);
    new.replace_field(old_name, new_name, id)
}

fn mob_effect_instance(d: Dy) -> Dy {
    let mut d = mob_effect_id_field(&d.clone(), "Id", d, "id");
    d = d.rename_field("Ambient", "ambient").rename_field("Amplifier", "amplifier").rename_field("Duration", "duration").rename_field("ShowParticles", "show_particles").rename_field("ShowIcon", "show_icon");
    let hidden = d.get("HiddenEffect").result().map(mob_effect_instance);
    d.replace_field("HiddenEffect", "hidden_effect", hidden)
}

fn mob_effect_instance_list(tag: Dy, old: &str, new: &str) -> Dy {
    let list = tag.get(old).as_stream_opt().result().map(|items| tag.create_list(items.into_iter().map(mob_effect_instance)));
    tag.replace_field(old, new, list)
}

fn suspicious_stew_entry(old: &Dy, new: Dy) -> Dy {
    let new = mob_effect_id_field(old, "EffectId", new, "id");
    let duration = old.get("EffectDuration").result();
    new.replace_field("EffectDuration", "duration", duration)
}

// MobEffectIdFix: numeric effect ids named.
fn f275(fx: &Fx) -> RuleRef {
    let input = fx.input();
    let output = fx.output();
    let choice = |reference: &'static str, id: &'static str| (named_choice(id, input.choice_type(reference, id)), output.choice_type(reference, id));
    let beacon = choice(BLOCK_ENTITY, "minecraft:beacon");
    let blocks = fix_typed(input.ty(BLOCK_ENTITY), input.ty(BLOCK_ENTITY), move |t| {
        t.update_typed_to(&beacon.0, beacon.1, |t1| {
            t1.update_remainder(|d| {
                let d = mob_effect_id_field(&d.clone(), "Primary", d, "primary_effect");
                mob_effect_id_field(&d.clone(), "Secondary", d, "secondary_effect")
            })
        })
    });
    let (mooshroom, arrow, cloud) = (choice(ENTITY, "minecraft:mooshroom"), choice(ENTITY, "minecraft:arrow"), choice(ENTITY, "minecraft:area_effect_cloud"));
    let entities = fix_typed(input.ty(ENTITY), input.ty(ENTITY), move |t| {
        let t = t.update_typed_to(&mooshroom.0, mooshroom.1, |t1| {
            t1.update_remainder(|tag| {
                let empty = tag.empty_map();
                let entry = suspicious_stew_entry(&tag, empty.clone());
                let mut tag = tag;
                if entry != empty {
                    let list = tag.create_list([entry]);
                    tag = tag.set("stew_effects", list);
                }
                tag.remove("EffectId").remove("EffectDuration")
            })
        });
        let t = t.update_typed_to(&arrow.0, arrow.1, |t1| t1.update_remainder(|d| mob_effect_instance_list(d, "CustomPotionEffects", "custom_potion_effects")));
        let t = t.update_typed_to(&cloud.0, cloud.1, |t1| t1.update_remainder(|d| mob_effect_instance_list(d, "Effects", "effects")));
        t.update_remainder(|d| mob_effect_instance_list(d, "ActiveEffects", "active_effects"))
    });
    let ty = input.ty(ITEM_STACK);
    let id = field_finder("id", t::named(ITEM_NAME, namespaced_string()));
    let tag = find_field(ty, "tag");
    let items = fix_typed(ty, ty, move |typed| {
        let Some(v) = typed.get_optional(&id) else { return typed };
        let item = v.into_pair().1.str().to_string();
        if item == "minecraft:suspicious_stew" {
            return typed.update_typed(&tag, |t1| {
                t1.update_remainder(|d| {
                    let list = d.get("Effects").as_stream_opt().result().map(|items| d.create_list(items.into_iter().map(|e| suspicious_stew_entry(&e.clone(), e))));
                    d.replace_field("Effects", "effects", list)
                })
            });
        }
        if matches!(item.as_str(), "minecraft:potion" | "minecraft:splash_potion" | "minecraft:lingering_potion" | "minecraft:tipped_arrow") {
            return typed.update_typed(&tag, |t1| t1.update_remainder(|d| mob_effect_instance_list(d, "CustomPotionEffects", "custom_potion_effects")));
        }
        typed
    });
    rewrite::seq(vec![blocks, entities, items])
}

// PrimedTntBlockStateFixer.
fn f277(fx: &Fx) -> RuleRef {
    named_entity_write_read(fx, ENTITY, "minecraft:tnt", |tag| {
        let state = tag.create_map([(tag.create_string("Name"), tag.create_string("minecraft:tnt"))]);
        let tag = tag.clone().set("block_state", state);
        match tag.get("Fuse").result() {
            Some(f) => tag.set("fuse", f),
            None => tag,
        }
    })
}

// FixProjectileStoredItem: arrows hold the item they are.
fn f278(fx: &Fx) -> RuleRef {
    let choice = |id: &'static str, fix: fn(&Typed, TyId) -> Typed| {
        let finder = named_choice(id, fx.input().choice_type(ENTITY, id));
        let out = fx.output().choice_type(ENTITY, id);
        move |t: Typed| t.update_typed_to(&finder, out, |t1| fix(&t1, out))
    };
    fn item(d: &Dy, id: &str) -> Dy {
        d.create_map([(d.create_string("id"), d.create_string(id)), (d.create_string("Count"), d.create_int(1))])
    }
    let trident = choice("minecraft:trident", |t, out| Typed::new(out, t.v.clone()));
    let arrow = choice("minecraft:arrow", |t, out| {
        write_and_read_typed(t, out, |d| {
            let kind = if d.get("Potion").as_string_or("minecraft:empty") == "minecraft:empty" { "minecraft:arrow" } else { "minecraft:tipped_arrow" };
            let i = item(&d, kind);
            d.set("item", i)
        })
    });
    let spectral = choice("minecraft:spectral_arrow", |t, out| {
        write_and_read_typed(t, out, |d| {
            let i = item(&d, "minecraft:spectral_arrow");
            d.set("item", i)
        })
    });
    fix_typed(fx.input().ty(ENTITY), fx.output().ty(ENTITY), move |t| spectral(arrow(trident(t))))
}

// RenameEnchantmentsFix (sweeping to sweeping_edge).
fn f285(fx: &Fx) -> RuleRef {
    let ty = fx.input().ty(ITEM_STACK);
    let tag = find_field(ty, "tag");
    fn fix_list(tag: Dy, key: &str) -> Dy {
        tag.update(key, |list| match list.as_stream_opt().result() {
            Some(items) => list.create_list(items.into_iter().map(|e| {
                let e2 = e.clone();
                e.update("id", |id| match id.as_string().result() {
                    Some(s) => e2.create_string(if t::ensure_namespaced(&s) == "minecraft:sweeping" { "minecraft:sweeping_edge" } else { &s }),
                    None => id,
                })
            })),
            None => list,
        })
    }
    fix_typed(ty, ty, move |t| t.update_typed(&tag, |t1| t1.update_remainder(|d| fix_list(fix_list(d, "Enchantments"), "StoredEnchantments"))))
}

// HorseBodyArmorItemFix.
fn horse_body_armor(fx: &Fx, entity: &'static str, field: &'static str, clear: bool) -> RuleRef {
    named_entity_write_read(fx, ENTITY, entity, move |tag| {
        let Some(armor) = tag.get(field).result() else { return tag };
        let mut d = tag.clone().remove(field);
        if clear {
            d = d.update("ArmorItems", |l| l.create_list(l.as_stream().into_iter().enumerate().map(|(i, x)| if i == 2 { x.empty_map() } else { x })));
            d = d.update("ArmorDropChances", |l| l.create_list(l.as_stream().into_iter().enumerate().map(|(i, x)| if i == 2 { x.create_float(0.085) } else { x })));
        }
        d.set("body_armor_item", armor).set("body_armor_drop_chance", tag.create_float(2.0))
    })
}

// ChestedHorsesInventoryZeroIndexingFix.
fn f291(fx: &Fx) -> RuleRef {
    let input = fx.input();
    let item = type_finder(input.ty(ITEM_STACK));
    let ty = input.ty(ENTITY);
    let rules = ["minecraft:llama", "minecraft:trader_llama", "minecraft:mule", "minecraft:donkey"]
        .into_iter()
        .map(|id| {
            let choice = input.choice_type(ENTITY, id);
            let finder = named_choice(id, choice);
            let items = find_field(choice, "Items");
            let item = item.clone();
            fix_typed(ty, ty, move |t| {
                t.update_typed(&finder, |t1| {
                    t1.update_typed(&items, |t2| {
                        t2.update(&item, |v| {
                            let (n, p1) = v.into_pair();
                            let (id, p2) = p1.into_pair();
                            let (tag, rem) = p2.into_pair();
                            let d = rem.dy().update("Slot", |s| s.create_byte(s.as_int(2).wrapping_sub(2) as i8));
                            V::pair(n, V::pair(id, V::pair(tag, V::Dyn(d.0))))
                        })
                    })
                })
            })
        })
        .collect();
    rewrite::seq(rules)
}

// FixWolfHealth.
fn f292(fx: &Fx) -> RuleRef {
    named_entity_remainder(fx, ENTITY, "minecraft:wolf", |d| {
        let doubled = std::cell::Cell::new(false);
        let d = d.update("Attributes", |list| {
            list.create_list(list.as_stream().into_iter().map(|a| {
                if t::ensure_namespaced(&a.get("Name").as_string_or("")) == "minecraft:generic.max_health" {
                    a.update("Base", |b| {
                        if b.as_double(0.0) == 20.0 {
                            doubled.set(true);
                            b.create_double(40.0)
                        } else {
                            b
                        }
                    })
                } else {
                    a
                }
            }))
        });
        if doubled.get() {
            d.update("Health", |h| h.create_float(h.as_float(0.0) * 2.0))
        } else {
            d
        }
    })
}

// BlockPosFormatAndRenamesFix: {X, Y, Z} positions become arrays, renamed.
fn f293(fx: &Fx) -> RuleRef {
    let input = fx.input();
    let fixer = |reference: &'static str, id: &'static str, renames: &'static [(&'static str, &'static str)]| {
        let finder = named_choice(id, input.choice_type(reference, id));
        let ty = input.ty(reference);
        fix_typed(ty, ty, move |t| {
            t.update_typed(&finder, |t1| {
                t1.update_remainder(|mut d| {
                    for (old, new) in renames {
                        d = d.rename_and_fix_field(old, new, fix_block_pos);
                    }
                    d
                })
            })
        })
    };
    let mut rules = vec![
        fixer(ENTITY, "minecraft:bee", &[("HivePos", "hive_pos"), ("FlowerPos", "flower_pos")]),
        fixer(ENTITY, "minecraft:end_crystal", &[("BeamTarget", "beam_target")]),
        fixer(ENTITY, "minecraft:wandering_trader", &[("WanderTarget", "wander_target")]),
    ];
    for id in ["minecraft:witch", "minecraft:ravager", "minecraft:pillager", "minecraft:illusioner", "minecraft:evoker", "minecraft:vindicator"] {
        rules.push(fixer(ENTITY, id, &[("PatrolTarget", "patrol_target")]));
    }
    rules.push(fix_remainder(input.ty(ENTITY), |d| d.rename_and_fix_field("Leash", "leash", fix_block_pos)));
    rules.push(fixer(BLOCK_ENTITY, "minecraft:beehive", &[("FlowerPos", "flower_pos")]));
    rules.push(fixer(BLOCK_ENTITY, "minecraft:end_gateway", &[("ExitPortal", "exit_portal")]));
    let ty = input.ty(ITEM_STACK);
    rules.push(fix_typed(ty, ty, item_stack_tag_fixer(ty, |id| id == "minecraft:compass", |t| t.update_remainder(|d| d.update("LodestonePos", fix_block_pos)))));
    rewrite::seq(rules)
}

// AttributesRenameLegacy.
pub fn attributes_rename_legacy(fx: &Fx, rename: impl Fn(&str) -> String + 'static) -> RuleRef {
    let rename = Rc::new(rename);
    let input = fx.input();
    let ty = input.ty(ITEM_STACK);
    let tag = find_field(ty, "tag");
    let fix_name = |rename: Rc<dyn Fn(&str) -> String>| move |n: Dy| match n.as_string().result() {
        Some(s) => n.create_string(&rename(&s)),
        None => n,
    };
    let r1: Rc<dyn Fn(&str) -> String> = rename.clone();
    let items = fix_typed(ty, ty, move |t| {
        let f = fix_name(r1.clone());
        t.update_typed(&tag, |t1| {
            t1.update_remainder(|d| {
                d.update("AttributeModifiers", |list| match list.as_stream_opt().result() {
                    Some(items) => list.create_list(items.into_iter().map(|m| m.update("AttributeName", &f))),
                    None => list,
                })
            })
        })
    });
    let r2: Rc<dyn Fn(&str) -> String> = rename.clone();
    let entities = fix_remainder(input.ty(ENTITY), move |d| {
        let f = fix_name(r2.clone());
        d.update("Attributes", |list| match list.as_stream_opt().result() {
            Some(items) => list.create_list(items.into_iter().map(|a| a.update("Name", &f))),
            None => list,
        })
    });
    rewrite::seq(vec![items, entities])
}

// BeehiveFieldRenameFix.
fn f296(fx: &Fx) -> RuleRef {
    let input = fx.input();
    let choice = input.choice_type(BLOCK_ENTITY, "minecraft:beehive");
    let finder = named_choice("minecraft:beehive", choice);
    let bees_ty = t::find_field_type(choice, "Bees").unwrap_or_else(|| refuse("no Bees field"));
    let Ty::List(element) = t::ty(bees_ty) else { refuse("Bees is no list (ClassCastException)") };
    let bees = field_finder("Bees", bees_ty);
    let bee = type_finder(element);
    let output = fx.output().ty(BLOCK_ENTITY);
    fix_typed(input.ty(BLOCK_ENTITY), output, move |t| {
        let t = t.update_typed(&finder, |t1| {
            t1.update_remainder(|d| d.remove("Bees")).update_typed(&bees, |t2| {
                t2.update_typed(&bee, |t3| t3.update_remainder(|d| d.remove("EntityData").rename_field("TicksInHive", "ticks_in_hive").rename_field("MinOccupationTicks", "min_ticks_in_hive")))
            })
        });
        Typed::new(output, t.v)
    })
}

const PATTERN_IDS: &[(&str, &str)] = &[
    ("b", "minecraft:base"),
    ("bl", "minecraft:square_bottom_left"),
    ("br", "minecraft:square_bottom_right"),
    ("tl", "minecraft:square_top_left"),
    ("tr", "minecraft:square_top_right"),
    ("bs", "minecraft:stripe_bottom"),
    ("ts", "minecraft:stripe_top"),
    ("ls", "minecraft:stripe_left"),
    ("rs", "minecraft:stripe_right"),
    ("cs", "minecraft:stripe_center"),
    ("ms", "minecraft:stripe_middle"),
    ("drs", "minecraft:stripe_downright"),
    ("dls", "minecraft:stripe_downleft"),
    ("ss", "minecraft:small_stripes"),
    ("cr", "minecraft:cross"),
    ("sc", "minecraft:straight_cross"),
    ("bt", "minecraft:triangle_bottom"),
    ("tt", "minecraft:triangle_top"),
    ("bts", "minecraft:triangles_bottom"),
    ("tts", "minecraft:triangles_top"),
    ("ld", "minecraft:diagonal_left"),
    ("rd", "minecraft:diagonal_up_right"),
    ("lud", "minecraft:diagonal_up_left"),
    ("rud", "minecraft:diagonal_right"),
    ("mc", "minecraft:circle"),
    ("mr", "minecraft:rhombus"),
    ("vh", "minecraft:half_vertical"),
    ("hh", "minecraft:half_horizontal"),
    ("vhr", "minecraft:half_vertical_right"),
    ("hhb", "minecraft:half_horizontal_bottom"),
    ("bo", "minecraft:border"),
    ("cbo", "minecraft:curly_border"),
    ("gra", "minecraft:gradient"),
    ("gru", "minecraft:gradient_up"),
    ("bri", "minecraft:bricks"),
    ("glb", "minecraft:globe"),
    ("cre", "minecraft:creeper"),
    ("sku", "minecraft:skull"),
    ("flo", "minecraft:flower"),
    ("moj", "minecraft:mojang"),
    ("pig", "minecraft:piglin"),
];

// BannerPatternFormatFix.
fn f298(fx: &Fx) -> RuleRef {
    named_entity_remainder(fx, BLOCK_ENTITY, "minecraft:banner", |d| {
        d.rename_and_fix_field("Patterns", "patterns", |list| {
            list.create_list(list.as_stream().into_iter().map(|layer| {
                let layer = layer.rename_and_fix_field("Pattern", "pattern", |p| match p.as_string().result() {
                    Some(s) => p.create_string(PATTERN_IDS.iter().find(|(k, _)| *k == s).map_or(s.as_str(), |(_, v)| *v)),
                    None => p,
                });
                let color = layer.create_string(dye_color_name(layer.get("Color").as_int(0)));
                layer.set("color", color).remove("Color")
            }))
        })
    })
}

// TippedArrowPotionToItemFix.fix.
fn tipped_arrow_potion(tag: Dy) -> Dy {
    let potion = tag.get("Potion").result();
    let effects = tag.get("custom_potion_effects").result();
    let color = tag.get("Color").result();
    if potion.is_none() && effects.is_none() && color.is_none() {
        return tag;
    }
    tag.remove("Potion").remove("custom_potion_effects").remove("Color").update("item", |item| {
        let mut t = item.get("tag").or_else_empty_map();
        if let Some(p) = potion {
            t = t.set("Potion", p);
        }
        if let Some(e) = effects {
            t = t.set("custom_potion_effects", e);
        }
        if let Some(c) = color {
            t = t.set("CustomPotionColor", c);
        }
        item.set("tag", t)
    })
}

// Character.isWhitespace.
fn java_whitespace(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | '\u{1c}'..='\u{1f}' | ' ' | '\u{1680}' | '\u{2000}'..='\u{2006}' | '\u{2008}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{205f}' | '\u{3000}')
}

// Brigadier's StringReader, over UTF-16 units.
struct Reader {
    s: Vec<char>,
    cursor: usize,
}

impl Reader {
    fn new(s: &str) -> Reader {
        Reader { s: s.chars().collect(), cursor: 0 }
    }
    fn can_read(&self) -> bool {
        self.cursor < self.s.len()
    }
    fn peek(&self) -> char {
        self.s[self.cursor]
    }
    fn skip_whitespace(&mut self) {
        while self.can_read() && java_whitespace(self.peek()) {
            self.cursor += 1;
        }
    }
    fn expect(&mut self, c: char) -> Option<()> {
        if self.can_read() && self.peek() == c {
            self.cursor += 1;
            Some(())
        } else {
            None
        }
    }
    fn number(&mut self) -> Option<String> {
        let start = self.cursor;
        while self.can_read() && (self.peek().is_ascii_digit() || self.peek() == '.' || self.peek() == '-') {
            self.cursor += 1;
        }
        let n: String = self.s[start..self.cursor].iter().collect();
        (!n.is_empty()).then_some(n)
    }
    fn read_float(&mut self) -> Option<f32> {
        self.number()?.parse().ok()
    }
    fn read_double(&mut self) -> Option<f64> {
        self.number()?.parse().ok()
    }
    fn read_int(&mut self) -> Option<i32> {
        super::v4::java_parse_int(&self.number()?)
    }
    fn read_string(&mut self) -> Option<String> {
        if !self.can_read() {
            return Some(String::new());
        }
        let next = self.peek();
        if next == '"' || next == '\'' {
            self.cursor += 1;
            let mut out = String::new();
            let mut escaped = false;
            while self.can_read() {
                let c = self.peek();
                self.cursor += 1;
                if escaped {
                    if c != next && c != '\\' {
                        return None;
                    }
                    out.push(c);
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == next {
                    return Some(out);
                } else {
                    out.push(c);
                }
            }
            None
        } else {
            let start = self.cursor;
            while self.can_read() && (self.peek().is_ascii_alphanumeric() || matches!(self.peek(), '_' | '-' | '.' | '+')) {
                self.cursor += 1;
            }
            Some(self.s[start..self.cursor].iter().collect())
        }
    }
}

// ParticleUnflatteningFix.fix: a particle's command string becomes its
// options map.
fn unflatten_particle(tag: Dy) -> Dy {
    let Some(s) = tag.as_string().result() else { return tag };
    let (first, rest) = match s.find(' ') {
        Some(at) => (&s[..at], Some(&s[at + 1..])),
        None => (s.as_str(), None),
    };
    let ty = t::ensure_namespaced(first);
    let dynamic = tag.create_map([(tag.create_string("type"), tag.create_string(&ty))]);
    let Some(rest) = rest else { return dynamic };
    match ty.as_str() {
        "minecraft:item" => {
            let mut item = tag.create_map([(tag.create_string("Count"), tag.create_int(1))]);
            match rest.find('{') {
                None => item = item.clone().set("id", tag.create_string(rest)),
                Some(at) => {
                    item = item.clone().set("id", tag.create_string(&rest[..at]));
                    match crate::nbt::parse(&rest[at..]) {
                        Ok(t) => item = item.set("tag", Dy::of(t)),
                        Err(crate::nbt::ParseError::Unsupported(why)) => refuse(format!("a particle's item tag of SNBT this port does not read ({why})")),
                        Err(_) => {}
                    }
                }
            }
            dynamic.set("item", item)
        }
        "minecraft:block" | "minecraft:block_marker" | "minecraft:falling_dust" | "minecraft:dust_pillar" => {
            let mut state = tag.empty_map();
            match rest.find('[') {
                None => state = state.set("Name", tag.create_string(&t::ensure_namespaced(rest))),
                Some(at) => {
                    state = state.set("Name", tag.create_string(&t::ensure_namespaced(&rest[..at])));
                    let props = parse_block_properties(&rest[at..]);
                    if !props.is_empty() {
                        let map = tag.create_map(props.into_iter().map(|(k, v)| (tag.create_string(&k), tag.create_string(&v))));
                        state = state.set("Properties", map);
                    }
                }
            }
            dynamic.set("block_state", state)
        }
        "minecraft:dust" => {
            let mut r = Reader::new(rest);
            (|| {
                let v = read_vector(&tag, &mut r)?;
                r.expect(' ')?;
                let f = r.read_float()?;
                Some(dynamic.clone().set("color", v).set("scale", tag.create_float(f)))
            })()
            .unwrap_or(dynamic)
        }
        "minecraft:dust_color_transition" => {
            let mut r = Reader::new(rest);
            (|| {
                let v = read_vector(&tag, &mut r)?;
                r.expect(' ')?;
                let f = r.read_float()?;
                r.expect(' ')?;
                let v1 = read_vector(&tag, &mut r)?;
                Some(dynamic.clone().set("from_color", v).set("to_color", v1).set("scale", tag.create_float(f)))
            })()
            .unwrap_or(dynamic)
        }
        "minecraft:sculk_charge" => {
            let mut r = Reader::new(rest);
            match r.read_float() {
                Some(f) => dynamic.set("roll", tag.create_float(f)),
                None => dynamic,
            }
        }
        "minecraft:vibration" => {
            let mut r = Reader::new(rest);
            (|| {
                let x = r.read_double()? as f32;
                r.expect(' ')?;
                let y = r.read_double()? as f32;
                r.expect(' ')?;
                let z = r.read_double()? as f32;
                r.expect(' ')?;
                let ticks = r.read_int()?;
                let floor = |v: f32| v.floor() as i32;
                let pos = tag.create_int_list([floor(x), floor(y), floor(z)]);
                let destination = tag.create_map([(tag.create_string("type"), tag.create_string("minecraft:block")), (tag.create_string("pos"), pos)]);
                Some(dynamic.clone().set("destination", destination).set("arrival_in_ticks", tag.create_int(ticks)))
            })()
            .unwrap_or(dynamic)
        }
        "minecraft:shriek" => {
            let mut r = Reader::new(rest);
            match r.read_int() {
                Some(i) => dynamic.set("delay", tag.create_int(i)),
                None => dynamic,
            }
        }
        _ => dynamic,
    }
}

fn read_vector(tag: &Dy, r: &mut Reader) -> Option<Dy> {
    let a = r.read_float()?;
    r.expect(' ')?;
    let b = r.read_float()?;
    r.expect(' ')?;
    let c = r.read_float()?;
    Some(tag.create_list([tag.create_float(a), tag.create_float(b), tag.create_float(c)]))
}

// ParticleUnflatteningFix.parseBlockProperties (empty on any error).
fn parse_block_properties(text: &str) -> Vec<(String, String)> {
    let mut r = Reader::new(text);
    let mut out: Vec<(String, String)> = Vec::new();
    let result = (|| {
        r.expect('[')?;
        r.skip_whitespace();
        while r.can_read() && r.peek() != ']' {
            r.skip_whitespace();
            let k = r.read_string()?;
            r.skip_whitespace();
            r.expect('=')?;
            r.skip_whitespace();
            let v = r.read_string()?;
            r.skip_whitespace();
            match out.iter_mut().find(|(key, _)| *key == k) {
                Some(e) => e.1 = v,
                None => out.push((k, v)),
            }
            if r.can_read() {
                if r.peek() != ',' {
                    break;
                }
                r.cursor += 1;
            }
        }
        r.expect(']')?;
        Some(())
    })();
    if result.is_some() {
        out
    } else {
        Vec::new()
    }
}

const BOOLEAN_PROPERTIES: &[&str] = &[
    "attached", "bottom", "conditional", "disarmed", "drag", "enabled", "extended", "eye", "falling", "hanging", "has_bottle_0", "has_bottle_1", "has_bottle_2", "has_record", "has_book", "inverted", "in_wall", "lit", "locked", "occupied", "open", "persistent", "powered", "short", "signal_fire", "snowy", "triggered", "unstable", "waterlogged", "berries", "bloom", "shrieking", "can_summon", "up", "down", "north", "east", "south", "west", "slot_0_occupied", "slot_1_occupied", "slot_2_occupied", "slot_3_occupied", "slot_4_occupied", "slot_5_occupied", "cracked", "crafting",
];

// ItemStackComponentizationFix.ItemStackData.
struct Stack {
    item: String,
    count: i32,
    components: Dy,
    remainder: Dy,
    tag: Dy,
}

impl Stack {
    fn read(tag: &Dy) -> Option<Stack> {
        let id = tag.get("id").as_string().result()?;
        let count = tag.get("Count").as_number().result()?;
        let rest = tag.clone().remove("id").remove("Count");
        Some(Stack { item: t::ensure_namespaced(&id), count: count.int_value(), components: rest.empty_map(), tag: rest.get("tag").or_else_empty_map(), remainder: rest.remove("tag") })
    }
    fn remove_tag(&mut self, key: &str) -> ODy {
        let v = self.tag.get(key);
        self.tag = self.tag.clone().remove(key);
        v
    }
    fn set(&mut self, component: &str, value: Dy) {
        self.components = self.components.clone().set(component, value);
    }
    fn set_opt(&mut self, component: &str, value: ODy) {
        if let Some(v) = value.result() {
            self.set(component, v);
        }
    }
    fn move_tag_into(&mut self, old: &str, tag: Dy, new: &str) -> Dy {
        match self.remove_tag(old).result() {
            Some(v) => tag.set(new, v),
            None => tag,
        }
    }
    fn move_to_component_unless(&mut self, key: &str, component: &str, default: Dy) {
        if let Some(v) = self.remove_tag(key).result() {
            if v != default {
                self.set(component, v);
            }
        }
    }
    fn move_to_component(&mut self, key: &str, component: &str) {
        if let Some(v) = self.remove_tag(key).result() {
            self.set(component, v);
        }
    }
    fn fix_sub_tag(&mut self, key: &str, skip_if_empty: bool, f: impl FnOnce(&mut Stack, Dy) -> Dy) {
        let o = self.tag.get(key);
        if skip_if_empty && !o.is_present() {
            return;
        }
        let d = f(self, o.or_else_empty_map());
        if d == d.empty_map() {
            self.tag = self.tag.clone().remove(key);
        } else {
            self.tag = self.tag.clone().set(key, d);
        }
    }
    fn is(&self, item: &str) -> bool {
        self.item == item
    }
    fn write(self) -> Dy {
        let mut d = self.tag.empty_map().set("id", self.tag.create_string(&self.item)).set("count", self.tag.create_int(self.count));
        let mut components = self.components;
        if self.tag != self.tag.empty_map() {
            components = components.set("minecraft:custom_data", self.tag.clone());
        }
        if components != self.tag.empty_map() {
            d = d.set("components", components);
        }
        // mergeRemainder: the stack's entries over a copy of the remainder.
        match (&d.0, &self.remainder.0) {
            (Some(Tag::Compound(c)), r @ (None | Some(Tag::Compound(_)))) => Dy(ops::merge_to_map_all(r, c.0.clone()).result().expect("a map")),
            _ => d,
        }
    }
}

fn fix_item_stack(s: &mut Stack, tag: &Dy) {
    let hide = s.remove_tag("HideFlags").as_int(0);
    s.move_to_component_unless("Damage", "minecraft:damage", tag.create_int(0));
    s.move_to_component_unless("RepairCost", "minecraft:repair_cost", tag.create_int(0));
    s.move_to_component("CustomModelData", "minecraft:custom_model_data");
    if let Some(state) = s.remove_tag("BlockStateTag").result() {
        s.set("minecraft:block_state", fix_block_state_tag(state));
    }
    s.move_to_component("EntityTag", "minecraft:entity_data");
    s.fix_sub_tag("BlockEntityTag", false, |s, d| {
        let id = t::ensure_namespaced(&d.get("id").as_string_or(""));
        let d = fix_block_entity_tag(s, d, &id);
        let without = d.clone().remove("id");
        if without == d.empty_map() {
            without
        } else {
            d
        }
    });
    s.move_to_component("BlockEntityTag", "minecraft:block_entity_data");
    if s.remove_tag("Unbreakable").as_boolean(false) {
        let mut d = tag.empty_map();
        if hide & 4 != 0 {
            d = d.set("show_in_tooltip", tag.create_boolean(false));
        }
        s.set("minecraft:unbreakable", d);
    }
    fix_enchantments(s, tag, "Enchantments", "minecraft:enchantments", hide & 1 != 0);
    if s.is("minecraft:enchanted_book") {
        fix_enchantments(s, tag, "StoredEnchantments", "minecraft:stored_enchantments", hide & 32 != 0);
    }
    s.fix_sub_tag("display", false, |s, d| fix_display(s, d, hide));
    fix_block_state_predicates(s, tag, "CanDestroy", "minecraft:can_break", hide & 8 != 0);
    fix_block_state_predicates(s, tag, "CanPlaceOn", "minecraft:can_place_on", hide & 16 != 0);
    fix_attribute_modifiers(s, tag, hide);
    if let Some(mut trim) = s.remove_tag("Trim").result() {
        if hide & 128 != 0 {
            trim = trim.clone().set("show_in_tooltip", trim.create_boolean(false));
        }
        s.set("minecraft:trim", trim);
    }
    if hide & 32 != 0 {
        s.set("minecraft:hide_additional_tooltip", tag.empty_map());
    }
    if s.is("minecraft:crossbow") {
        s.remove_tag("Charged");
        s.move_to_component_unless("ChargedProjectiles", "minecraft:charged_projectiles", tag.create_list([]));
    }
    if s.is("minecraft:bundle") {
        s.move_to_component_unless("Items", "minecraft:bundle_contents", tag.create_list([]));
    }
    if s.is("minecraft:filled_map") {
        s.move_to_component("map", "minecraft:map_id");
        let mut decorations: Vec<(Dy, Dy)> = Vec::new();
        for d in s.remove_tag("Decorations").as_stream() {
            let (k, v) = fix_map_decoration(&d);
            if !decorations.iter().any(|(key, _)| *key == k) {
                decorations.push((k, v));
            }
        }
        if !decorations.is_empty() {
            s.set("minecraft:map_decorations", tag.create_map(decorations));
        }
    }
    if matches!(s.item.as_str(), "minecraft:potion" | "minecraft:splash_potion" | "minecraft:lingering_potion" | "minecraft:tipped_arrow") {
        let mut d = tag.empty_map();
        if let Some(p) = s.remove_tag("Potion").as_string().result().filter(|p| p != "minecraft:empty") {
            d = d.set("potion", tag.create_string(&p));
        }
        d = s.move_tag_into("CustomPotionColor", d, "custom_color");
        d = s.move_tag_into("custom_potion_effects", d, "custom_effects");
        if d != tag.empty_map() {
            s.set("minecraft:potion_contents", d);
        }
    }
    if s.is("minecraft:writable_book") {
        if let Some(pages) = fix_book_pages(s, tag) {
            s.set("minecraft:writable_book_content", tag.empty_map().set("pages", pages));
        }
    }
    if s.is("minecraft:written_book") {
        let pages = fix_book_pages(s, tag);
        let title = s.remove_tag("title").as_string_or("");
        let filtered = s.remove_tag("filtered_title").as_string().result();
        let mut d = tag.empty_map().set("title", filtered_text(tag, &title, filtered));
        d = s.move_tag_into("author", d, "author");
        d = s.move_tag_into("resolved", d, "resolved");
        d = s.move_tag_into("generation", d, "generation");
        if let Some(p) = pages {
            d = d.set("pages", p);
        }
        s.set("minecraft:written_book_content", d);
    }
    if s.is("minecraft:suspicious_stew") {
        s.move_to_component("effects", "minecraft:suspicious_stew_effects");
    }
    if s.is("minecraft:debug_stick") {
        s.move_to_component("DebugProperty", "minecraft:debug_stick_state");
    }
    if matches!(s.item.as_str(), "minecraft:pufferfish_bucket" | "minecraft:salmon_bucket" | "minecraft:cod_bucket" | "minecraft:tropical_fish_bucket" | "minecraft:axolotl_bucket" | "minecraft:tadpole_bucket") {
        let mut d = tag.empty_map();
        for name in ["NoAI", "Silent", "NoGravity", "Glowing", "Invulnerable", "Health", "Age", "Variant", "HuntingCooldown", "BucketVariantTag"] {
            d = s.move_tag_into(name, d, name);
        }
        if d != tag.empty_map() {
            s.set("minecraft:bucket_entity_data", d);
        }
    }
    if s.is("minecraft:goat_horn") {
        s.move_to_component("instrument", "minecraft:instrument");
    }
    if s.is("minecraft:knowledge_book") {
        s.move_to_component("Recipes", "minecraft:recipes");
    }
    if s.is("minecraft:compass") {
        let pos = s.remove_tag("LodestonePos").result();
        let dimension = s.remove_tag("LodestoneDimension").result();
        if pos.is_some() || dimension.is_some() {
            let tracked = s.remove_tag("LodestoneTracked").as_boolean(true);
            let mut d = tag.empty_map();
            if let (Some(p), Some(dim)) = (pos, dimension) {
                d = d.set("target", tag.empty_map().set("pos", p).set("dimension", dim));
            }
            if !tracked {
                d = d.set("tracked", tag.create_boolean(false));
            }
            s.set("minecraft:lodestone_tracker", d);
        }
    }
    if s.is("minecraft:firework_rocket") {
        s.fix_sub_tag("Fireworks", true, |s, d| {
            let explosions: Vec<Dy> = d.get("Explosions").as_stream().into_iter().map(fix_firework_explosion).collect();
            let flight = d.get("Flight").as_int(0);
            s.set("minecraft:fireworks", d.empty_map().set("explosions", d.create_list(explosions)).set("flight_duration", d.create_byte(flight as i8)));
            d.remove("Explosions").remove("Flight")
        });
    }
    if s.is("minecraft:firework_star") {
        s.fix_sub_tag("Explosion", true, |s, d| {
            s.set("minecraft:firework_explosion", fix_firework_explosion(d.clone()));
            d.remove("Type").remove("Colors").remove("FadeColors").remove("Trail").remove("Flicker")
        });
    }
    if s.is("minecraft:player_head") {
        if let Some(owner) = s.remove_tag("SkullOwner").result() {
            s.set("minecraft:profile", fix_profile(owner));
        }
    }
}

// The boxed Number's toString (Byte, Short, Integer, Long, Float, Double).
fn number_string(n: ops::Num) -> String {
    match n {
        ops::Num::B(v) => v.to_string(),
        ops::Num::S(v) => v.to_string(),
        ops::Num::I(v) => v.to_string(),
        ops::Num::L(v) => v.to_string(),
        ops::Num::F(v) => crate::jfmt::float_to_string(v),
        ops::Num::D(v) => crate::jfmt::double_to_string(v),
    }
}

fn fix_block_state_tag(tag: Dy) -> Dy {
    match tag.as_map_opt().result() {
        None => tag,
        Some(entries) => tag.create_map(entries.into_iter().map(|(k, v)| {
            let name = k.as_string_or("");
            if BOOLEAN_PROPERTIES.contains(&name.as_str()) {
                if let Some(b) = v.as_boolean_dr().result() {
                    let s = v.create_string(if b { "true" } else { "false" });
                    return (k, s);
                }
            }
            match v.as_number().result() {
                Some(n) => {
                    let s = v.create_string(&number_string(n));
                    (k, s)
                }
                None => (k, v),
            }
        })),
    }
}

fn fix_display(s: &mut Stack, tag: Dy, hide: i32) -> Dy {
    if let Some(name) = tag.get("Name").result().filter(strictly_valid_json) {
        s.set("minecraft:custom_name", name);
    }
    if tag.get("Lore").is_present() {
        let lore = tag.create_list(tag.get("Lore").as_stream().into_iter().filter(strictly_valid_json));
        s.set("minecraft:lore", lore);
    }
    let color = tag.get("color").as_number().result().map(|n| n.int_value());
    let dyed = hide & 64 != 0;
    if color.is_some() || dyed {
        let mut d = tag.empty_map().set("rgb", tag.create_int(color.unwrap_or(10511680)));
        if dyed {
            d = d.set("show_in_tooltip", tag.create_boolean(false));
        }
        s.set("minecraft:dyed_color", d);
    }
    if let Some(loc) = tag.get("LocName").as_string().result() {
        s.set("minecraft:item_name", translatable_component(&loc));
    }
    let mut tag = tag;
    if s.is("minecraft:filled_map") {
        s.set_opt("minecraft:map_color", tag.get("MapColor"));
        tag = tag.remove("MapColor");
    }
    tag.remove("Name").remove("Lore").remove("color").remove("LocName")
}

// LegacyComponentDataFixUtils.isStrictlyValidJson.
fn strictly_valid_json(d: &Dy) -> bool {
    d.as_string().result().is_some_and(|s| gson::is_strict_json(&s))
}

fn fix_block_entity_tag(s: &mut Stack, tag: Dy, id: &str) -> Dy {
    s.set_opt("minecraft:lock", tag.get("Lock"));
    let mut tag = tag.remove("Lock");
    if let Some(table) = tag.get("LootTable").result() {
        let mut d = tag.empty_map().set("loot_table", table);
        let seed = tag.get("LootTableSeed").as_long(0);
        if seed != 0 {
            d = d.set("seed", tag.create_long(seed));
        }
        s.set("minecraft:container_loot", d);
        tag = tag.remove("LootTable").remove("LootTableSeed");
    }
    match id {
        "minecraft:skull" => {
            s.set_opt("minecraft:note_block_sound", tag.get("note_block_sound"));
            tag.remove("note_block_sound")
        }
        "minecraft:decorated_pot" => {
            s.set_opt("minecraft:pot_decorations", tag.get("sherds"));
            if let Some(item) = tag.get("item").result() {
                let entry = tag.empty_map().set("slot", tag.create_int(0)).set("item", item);
                s.set("minecraft:container", tag.create_list([entry]));
            }
            tag.remove("sherds").remove("item")
        }
        "minecraft:banner" => {
            s.set_opt("minecraft:banner_patterns", tag.get("patterns"));
            if let Some(base) = tag.get("Base").as_number().result() {
                s.set("minecraft:base_color", tag.create_string(dye_color_name(base.int_value())));
            }
            tag.remove("patterns").remove("Base")
        }
        "minecraft:shulker_box" | "minecraft:chest" | "minecraft:trapped_chest" | "minecraft:furnace" | "minecraft:ender_chest" | "minecraft:dispenser" | "minecraft:dropper" | "minecraft:brewing_stand" | "minecraft:hopper" | "minecraft:barrel" | "minecraft:smoker" | "minecraft:blast_furnace" | "minecraft:campfire" | "minecraft:chiseled_bookshelf" | "minecraft:crafter" => {
            let items: Vec<Dy> = tag
                .get("Items")
                .as_stream()
                .into_iter()
                .map(|i| {
                    let slot = i.create_int((i.get("Slot").as_byte(0) as i32) & 255);
                    i.empty_map().set("slot", slot).set("item", i.remove("Slot"))
                })
                .collect();
            if !items.is_empty() {
                s.set("minecraft:container", tag.create_list(items));
            }
            tag.remove("Items")
        }
        "minecraft:beehive" => {
            s.set_opt("minecraft:bees", tag.get("bees"));
            tag.remove("bees")
        }
        _ => tag,
    }
}

fn fix_enchantments(s: &mut Stack, tag: &Dy, key: &str, component: &str, hide: bool) {
    let o = s.remove_tag(key);
    let list: Vec<(String, i32)> = o
        .as_stream_opt()
        .result()
        .map(|items| items.into_iter().filter_map(|e| Some((e.get("id").as_string().result()?, e.get("lvl").as_number().result()?.int_value().clamp(0, 255)))).filter(|(_, l)| *l > 0).collect())
        .unwrap_or_default();
    if !list.is_empty() || hide {
        let mut levels = tag.empty_map();
        for (id, level) in &list {
            levels = levels.set(id, tag.create_int(*level));
        }
        let mut d = tag.empty_map().set("levels", levels);
        if hide {
            d = d.set("show_in_tooltip", tag.create_boolean(false));
        }
        s.set(component, d);
    }
    if o.is_present() && list.is_empty() {
        s.set("minecraft:enchantment_glint_override", tag.create_boolean(true));
    }
}

fn fix_block_state_predicates(s: &mut Stack, tag: &Dy, key: &str, component: &str, hide: bool) {
    let Some(list) = s.remove_tag(key).result() else { return };
    let predicates = tag.create_list(list.as_stream().into_iter().map(|p| match p.as_string().result() {
        Some(id) => fix_block_state_predicate(&p, &id),
        None => p,
    }));
    let mut d = tag.empty_map().set("predicates", predicates);
    if hide {
        d = d.set("show_in_tooltip", tag.create_boolean(false));
    }
    s.set(component, d);
}

// String.trim: code units up to space removed from both ends.
fn java_trim(s: &str) -> &str {
    s.trim_matches(|c: char| (c as u32) <= 0x20)
}

fn fix_block_state_predicate(tag: &Dy, id: &str) -> Dy {
    let index = id.find('[');
    let index1 = id.find('{');
    let mut len = id.len();
    if let Some(i) = index {
        len = i;
    }
    if let Some(i) = index1 {
        len = len.min(i);
    }
    let mut d = tag.empty_map().set("blocks", tag.create_string(java_trim(&id[..len])));
    let index2 = id.find(']');
    if let (Some(i), Some(j)) = (index, index2) {
        let mut state = tag.empty_map();
        if i + 1 > j {
            refuse("a block predicate whose ']' comes before its '[' (StringIndexOutOfBoundsException)");
        }
        for part in id[i + 1..j].split(',') {
            if let Some(eq) = part.find('=') {
                state = state.set(java_trim(&part[..eq]), tag.create_string(java_trim(&part[eq + 1..])));
            }
        }
        d = d.set("state", state);
    }
    let index4 = id.find('}');
    if let (Some(i), Some(j)) = (index1, index4) {
        if i > j + 1 {
            refuse("a block predicate whose '}' comes before its '{' (StringIndexOutOfBoundsException)");
        }
        d = d.set("nbt", tag.create_string(&id[i..j + 1]));
    }
    d
}

fn fix_attribute_modifiers(s: &mut Stack, tag: &Dy, hide: i32) {
    let o = s.remove_tag("AttributeModifiers");
    if !o.is_present() {
        return;
    }
    let list: Vec<Dy> = o.as_stream().into_iter().map(|m| {
        let mut d = m.empty_map().set("name", m.create_string("")).set("amount", m.create_double(0.0)).set("operation", m.create_string("add_value"));
        for (from, to) in [("AttributeName", "type"), ("Slot", "slot"), ("UUID", "uuid"), ("Name", "name"), ("Amount", "amount")] {
            d = super::v4::copy_field(&m, from, d, to);
        }
        match m.get("Operation").result() {
            Some(op) => {
                let name = match op.as_int(0) {
                    1 => "add_multiplied_base",
                    2 => "add_multiplied_total",
                    _ => "add_value",
                };
                d.set("operation", op.create_string(name))
            }
            None => d,
        }
    }).collect();
    let mut d = tag.empty_map().set("modifiers", tag.create_list(list));
    if hide & 2 != 0 {
        d = d.set("show_in_tooltip", tag.create_boolean(false));
    }
    s.set("minecraft:attribute_modifiers", d);
}

fn fix_map_decoration(tag: &Dy) -> (Dy, Dy) {
    let id = tag.get("id").result().unwrap_or_else(|| tag.create_string(""));
    let kind = match tag.get("type").as_int(0) {
        1 => "frame",
        2 => "red_marker",
        3 => "blue_marker",
        4 => "target_x",
        5 => "target_point",
        6 => "player_off_map",
        7 => "player_off_limits",
        8 => "mansion",
        9 => "monument",
        10 => "banner_white",
        11 => "banner_orange",
        12 => "banner_magenta",
        13 => "banner_light_blue",
        14 => "banner_yellow",
        15 => "banner_lime",
        16 => "banner_pink",
        17 => "banner_gray",
        18 => "banner_light_gray",
        19 => "banner_cyan",
        20 => "banner_purple",
        21 => "banner_blue",
        22 => "banner_brown",
        23 => "banner_green",
        24 => "banner_red",
        25 => "banner_black",
        26 => "red_x",
        27 => "village_desert",
        28 => "village_plains",
        29 => "village_savanna",
        30 => "village_snowy",
        31 => "village_taiga",
        32 => "jungle_temple",
        33 => "swamp_hut",
        _ => "player",
    };
    let d = tag.empty_map().set("type", tag.create_string(kind)).set("x", tag.create_double(tag.get("x").as_double(0.0))).set("z", tag.create_double(tag.get("z").as_double(0.0))).set("rotation", tag.create_float(tag.get("rot").as_double(0.0) as f32));
    (id, d)
}

fn fix_book_pages(s: &mut Stack, tag: &Dy) -> Option<Dy> {
    let pages: Vec<String> = s.remove_tag("pages").as_stream_opt().result().map(|l| l.into_iter().map(|p| p.as_string_or("")).collect()).unwrap_or_default();
    let filtered: Vec<(String, String)> = s.remove_tag("filtered_pages").as_map_opt().result().map(|m| m.into_iter().map(|(k, v)| (k.as_string_or("0"), v.as_string_or(""))).collect()).unwrap_or_default();
    if pages.is_empty() {
        return None;
    }
    let list = pages.iter().enumerate().map(|(i, p)| {
        let f = filtered.iter().find(|(k, _)| *k == i.to_string()).map(|(_, v)| v.clone());
        filtered_text(tag, p, f)
    });
    Some(tag.create_list(list.collect::<Vec<_>>()))
}

fn filtered_text(tag: &Dy, raw: &str, filtered: Option<String>) -> Dy {
    let mut d = tag.empty_map().set("raw", tag.create_string(raw));
    if let Some(f) = filtered {
        d = d.set("filtered", tag.create_string(&f));
    }
    d
}

fn fix_firework_explosion(tag: Dy) -> Dy {
    let shape = match tag.get("Type").as_int(0) {
        1 => "large_ball",
        2 => "star",
        3 => "creeper",
        4 => "burst",
        _ => "small_ball",
    };
    let shape = tag.create_string(shape);
    tag.set("shape", shape).remove("Type").rename_field("Colors", "colors").rename_field("FadeColors", "fade_colors").rename_field("Trail", "has_trail").rename_field("Flicker", "has_twinkle")
}

fn valid_player_name(name: &str) -> bool {
    name.encode_utf16().count() <= 16 && !name.encode_utf16().any(|c| c <= 32 || c >= 127)
}

// ItemStackComponentizationFix.fixProfile.
pub fn fix_profile(tag: Dy) -> Dy {
    if let Some(name) = tag.as_string().result() {
        return if valid_player_name(&name) { tag.empty_map().set("name", tag.create_string(&name)) } else { tag.empty_map() };
    }
    let name = tag.get("Name").as_string_or("");
    let id = tag.get("Id").result();
    let properties = fix_profile_properties(&tag, tag.get("Properties"));
    let mut d = tag.empty_map();
    if valid_player_name(&name) {
        d = d.set("name", tag.create_string(&name));
    }
    if let Some(i) = id {
        d = d.set("id", i);
    }
    if let Some(p) = properties {
        d = d.set("properties", p);
    }
    d
}

// fixProfileProperties: a list made from a hash map of the property names,
// in its order (refused where two names share a table index).
fn fix_profile_properties(tag: &Dy, o: ODy) -> Option<Dy> {
    let entries = o.as_map_opt().result().unwrap_or_default();
    if entries.is_empty() {
        return None;
    }
    let mut map: Vec<(String, Vec<(String, Option<String>)>)> = Vec::new();
    for (k, v) in entries {
        let key = k.as_string_or("");
        let values = v.as_stream_opt().result().map(|l| l.into_iter().map(|x| (x.get("Value").as_string_or(""), x.get("Signature").as_string().result())).collect()).unwrap_or_default();
        match map.iter_mut().find(|(n, _)| *n == key) {
            Some(e) => e.1 = values,
            None => map.push((key, values)),
        }
    }
    let names: Vec<String> = map.iter().map(|(k, _)| k.clone()).collect();
    let mut seen = std::collections::HashSet::new();
    for n in &names {
        let h = string_hash(n);
        let index = ((h ^ ((h as u32) >> 16) as i32) as u32) & 15;
        if names.len() > 12 || !seen.insert(index) {
            refuse("profile property names whose hash map order this port does not decide");
        }
    }
    let order = java_hash_set_order(&names);
    let mut list = Vec::new();
    for n in order {
        let values = &map.iter().find(|(k, _)| *k == n).unwrap().1;
        for (value, signature) in values {
            let mut d = tag.empty_map().set("name", tag.create_string(&n)).set("value", tag.create_string(value));
            if let Some(s) = signature {
                d = d.set("signature", tag.create_string(s));
            }
            list.push(d);
        }
    }
    Some(tag.create_list(list))
}

// ItemStackComponentizationFix: an item stack's tag becomes components.
fn f302(fx: &Fx) -> RuleRef {
    write_fix_and_read(fx.input().ty(ITEM_STACK), fx.output().ty(ITEM_STACK), |d| match Stack::read(&d) {
        Some(mut s) => {
            let tag = s.tag.clone();
            fix_item_stack(&mut s, &tag);
            s.write()
        }
        None => d,
    })
}

// AreaEffectCloudPotionFix.
fn f303(fx: &Fx) -> RuleRef {
    named_entity_remainder(fx, ENTITY, "minecraft:area_effect_cloud", |tag| {
        let color = tag.get("Color").result();
        let effects = tag.get("effects").result();
        let potion = tag.get("Potion").result();
        let tag = tag.remove("Color").remove("effects").remove("Potion");
        if color.is_none() && effects.is_none() && potion.is_none() {
            return tag;
        }
        let mut d = tag.empty_map();
        if let Some(c) = color {
            d = d.set("custom_color", c);
        }
        if let Some(e) = effects {
            d = d.set("custom_effects", e);
        }
        if let Some(p) = potion {
            d = d.set("potion", p);
        }
        tag.set("potion_contents", d)
    })
}

// PlayerHeadBlockProfileFix.
fn f304(fx: &Fx) -> RuleRef {
    named_entity_remainder(fx, BLOCK_ENTITY, "minecraft:skull", |tag| {
        let owner = tag.get("SkullOwner").result().or_else(|| tag.get("ExtraType").result());
        match owner {
            None => tag,
            Some(o) => tag.remove("SkullOwner").remove("ExtraType").set("profile", fix_profile(o)),
        }
    })
}

// LodestoneCompassComponentFix.
fn f305(fx: &Fx) -> RuleRef {
    component_remainder(fx, "minecraft:lodestone_target", "minecraft:lodestone_tracker", |c| {
        let pos = c.get("pos").result();
        let dimension = c.get("dimension").result();
        let mut c = c.remove("pos").remove("dimension");
        if let (Some(p), Some(d)) = (pos, dimension) {
            let target = c.empty_map().set("pos", p).set("dimension", d);
            c = c.set("target", target);
        }
        Some(c)
    })
}

const MAP_NAMES: &[&str] = &[
    "filled_map.buried_treasure",
    "filled_map.explorer_jungle",
    "filled_map.explorer_swamp",
    "filled_map.mansion",
    "filled_map.monument",
    "filled_map.trial_chambers",
    "filled_map.village_desert",
    "filled_map.village_plains",
    "filled_map.village_savanna",
    "filled_map.village_snowy",
    "filled_map.village_taiga",
];

// ItemStackCustomNameToOverrideComponentFix.
fn f306(fx: &Fx) -> RuleRef {
    let ty = fx.input().ty(ITEM_STACK);
    let id = field_finder("id", t::named(ITEM_NAME, namespaced_string()));
    let components = find_field(ty, "components");
    fn fix_custom_name(t: Typed, pass: fn(&str) -> bool) -> Typed {
        let ty = t.ty;
        write_and_read_typed(&t, ty, |d| {
            let name = d.get("minecraft:custom_name").as_string().result().and_then(|s| extract_translation_string(&s)).filter(|s| pass(s));
            if name.is_some() {
                d.rename_field("minecraft:custom_name", "minecraft:item_name")
            } else {
                d
            }
        })
    }
    fix_typed(ty, ty, move |typed| {
        let item = typed.get_optional(&id).map(|v| v.into_pair().1.str().to_string());
        match item.as_deref() {
            Some("minecraft:white_banner") => typed.update_typed(&components, |c| fix_custom_name(c, |s| s == "block.minecraft.ominous_banner")),
            Some("minecraft:filled_map") => typed.update_typed(&components, |c| fix_custom_name(c, |s| MAP_NAMES.contains(&s))),
            _ => typed,
        }
    })
}

// BannerEntityCustomNameToOverrideComponentFix.
fn f307(fx: &Fx) -> RuleRef {
    let input = fx.input();
    let ty = input.ty(BLOCK_ENTITY);
    let choice = type_finder(input.find_choice_type(BLOCK_ENTITY));
    let custom_name = find_field(ty, "CustomName");
    let text = type_finder(input.ty(TEXT_COMPONENT));
    fix_typed(ty, ty, move |typed| {
        let (key, _) = typed.get(&choice).into_pair();
        if key.str() != "minecraft:banner" {
            return typed;
        }
        let name = typed.get_optional_typed(&custom_name).and_then(|t| t.get_optional(&text)).map(|v| v.into_pair().1.str().to_string());
        let ominous = name.as_deref().and_then(extract_translation_string).is_some_and(|s| s == "block.minecraft.ominous_banner");
        if !ominous {
            return typed;
        }
        let name = name.unwrap();
        let ty = typed.ty;
        write_and_read_typed(&typed, ty, |d| {
            let components = d.create_map([(d.create_string("minecraft:item_name"), d.create_string(&name)), (d.create_string("minecraft:hide_additional_tooltip"), d.empty_map())]);
            d.set("components", components).remove("CustomName")
        })
    })
}

// TrialSpawnerConfigFix.moveToConfigTag.
fn trial_spawner_config(mut data: Dy) -> Dy {
    let mut map: Vec<(Dy, Dy)> = Vec::new();
    for key in ["spawn_range", "total_mobs", "simultaneous_mobs", "total_mobs_added_per_player", "simultaneous_mobs_added_per_player", "ticks_between_spawn", "spawn_potentials", "loot_tables_to_eject", "items_to_drop_when_ominous"] {
        if let Some(v) = data.get(key).result() {
            map.push((data.create_string(key), v));
            data = data.remove(key);
        }
    }
    if map.is_empty() {
        data
    } else {
        let config = data.create_map(map);
        data.set("normal_config", config)
    }
}

// EmptyItemInVillagerTradeFix.
fn f310(fx: &Fx) -> RuleRef {
    let ty = fx.input().ty("villager_trade");
    write_fix_and_read(ty, ty, |d| {
        let buy = d.get("buyB").or_else_empty_map();
        let id = t::ensure_namespaced(&buy.get("id").as_string_or("minecraft:air"));
        let count = buy.get("count").as_int(0);
        if id != "minecraft:air" && count != 0 {
            d
        } else {
            d.remove("buyB")
        }
    })
}

// RemoveEmptyItemInBrushableBlockFix.fix.
fn remove_empty_brushable_item(tag: Dy) -> Dy {
    match tag.get("item").result() {
        Some(item) if t::ensure_namespaced(&item.get("id").as_string_or("minecraft:air")) == "minecraft:air" || item.get("count").as_int(0) == 0 => tag.remove("item"),
        _ => tag,
    }
}
