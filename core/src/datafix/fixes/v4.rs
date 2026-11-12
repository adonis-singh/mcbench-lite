// Fixes of 1.21 to 1.21.10 (data versions 3900 to 4556).

use super::super::gson::{self, Je};
use super::util::*;
use super::*;

pub fn defs() -> Vec<(i32, &'static str, Imp)> {
    vec![
        (39380, "ProjectileStoredWeaponFix", R(true, f312)), // 312
        (39390, "FeatureFlagRemoveFix", I), // 313
        (39430, "OptionsMenuBlurrinessFix", I), // 314
        (39450, "AttributeModifierIdFix", R(false, f315)), // 315
        (39450, "JukeboxTicksSinceSongStartedFix", R(false, f316)), // 316
        (40540, "OminousBannerRarityFix", R(false, f317)), // 317
        (40550, "AttributeIdPrefixFix", R(false, |fx| attributes_rename(fx, attribute_id_prefix))), // 318
        (40570, "CarvingStepRemoveFix", I), // 319
        (40590, "FoodToConsumableFix", R(true, f320)), // 320
        (40610, "TrialSpawnerConfigInRegistryFix", R(false, f321)), // 321
        (40640, "FireResistantToDamageResistantComponentFix", R(false, f322)), // 322
        (40670, "BoatSplitFix", R(true, f323)), // 323
        (40670, "FeatureFlagRemoveFix", I), // 324
        (40680, "LockComponentPredicateFix", R(false, |fx| component_remainder(fx, "minecraft:lock", "minecraft:lock", fix_lock))), // 325
        (40680, "ContainerBlockEntityLockPredicateFix", R(false, f326)), // 326
        (40700, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 327 Added Pale Oak Boat and Pale Oak Chest Boat
        (40710, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 328 Added Creaking
        (40710, "AddNewChoices", R(true, |fx| add_new_choices(fx, BLOCK_ENTITY))), // 329 Added Creaking Heart
        (40810, "EntitySalmonSizeFix", R(false, f330)), // 330
        (41730, "EntityFieldsRenameFix", R(false, |fx| entity_fields_rename(fx, "minecraft:tnt_minecart", &[("TNTFuse", "fuse")]))), // 331
        (41750, "EquippableAssetRenameFix", R(true, f332)), // 332
        (41750, "CustomModelDataExpandFix", R(false, f333)), // 333
        (41760, "InvalidBlockEntityLockFix", R(false, f334)), // 334
        (41760, "InvalidLockComponentFix", R(false, |fx| component_remainder(fx, "minecraft:lock", "minecraft:lock", fix_invalid_lock))), // 335
        (41800, "FeatureFlagRemoveFix", I), // 336
        (41810, "BlockEntityFurnaceBurnTimeFix", R(false, |fx| furnace_burn_time(fx, "minecraft:furnace"))), // 337
        (41810, "BlockEntityFurnaceBurnTimeFix", R(false, |fx| furnace_burn_time(fx, "minecraft:smoker"))), // 338
        (41810, "BlockEntityFurnaceBurnTimeFix", R(false, |fx| furnace_burn_time(fx, "minecraft:blast_furnace"))), // 339
        (41870, "EntityAttributeBaseValueFix", R(false, |fx| attribute_base_value(fx, "minecraft:villager", |d| if d == 48.0 { 16.0 } else { d }))), // 340
        (41870, "EntityAttributeBaseValueFix", R(false, |fx| attribute_base_value(fx, "minecraft:bee", |d| if d == 48.0 { 16.0 } else { d }))), // 341
        (41870, "EntityAttributeBaseValueFix", R(false, |fx| attribute_base_value(fx, "minecraft:allay", |d| if d == 48.0 { 16.0 } else { d }))), // 342
        (41870, "EntityAttributeBaseValueFix", R(false, |fx| attribute_base_value(fx, "minecraft:llama", |d| if d == 40.0 { 16.0 } else { d }))), // 343
        (41870, "EntityAttributeBaseValueFix", R(false, |fx| attribute_base_value(fx, "minecraft:piglin_brute", |d| if d == 16.0 { 12.0 } else { d }))), // 344
        (41870, "EntityAttributeBaseValueFix", R(false, |fx| attribute_base_value(fx, "minecraft:warden", |d| if d == 16.0 { 24.0 } else { d }))), // 345
        (42900, "UnflattenTextComponentFix", R(true, f346)), // 346
        (42910, "LegacyHoverEventFix", R(false, f347)), // 347
        (42910, "TextComponentStringifiedFlagsFix", R(false, f348)), // 348
        (42920, "TextComponentHoverAndClickEventFix", R(true, f349)), // 349
        (42930, "DropChancesFormatFix", R(false, f350)), // 350
        (42940, "BlockPropertyRenameAndFix", R(false, |fx| block_property_rename(fx, "minecraft:creaking_heart", "active", "creaking_heart_state", |s| if s == "true" { "awake".into() } else { "uprooted".into() }))), // 351
        (42950, "BlendingDataFix", I), // 352
        (42960, "AreaEffectCloudDurationScaleFix", R(false, f353)), // 353
        (42970, "ForcedChunkToTicketFix", I), // 354
        (42990, "EntitySpawnerItemVariantComponentFix", R(false, f355)), // 355
        (43000, "SaddleEquipmentSlotFix", R(true, f356)), // 356
        (43010, "EquipmentFormatFix", R(true, f357)), // 357
        (43020, "AddNewChoices", R(true, |fx| add_new_choices(fx, BLOCK_ENTITY))), // 358 Added Test and Test Instance Block Entities
        (43030, "EntityFallDistanceFloatToDoubleFix", R(false, f359)), // 359
        (43030, "EntityFallDistanceFloatToDoubleFix", I), // 360
        (43050, "BlockPropertyRenameAndFix", R(false, |fx| block_property_rename(fx, "minecraft:test_block", "test_block_mode", "mode", |s| s.to_string()))), // 361
        (43060, "ThrownPotionSplitFix", R(true, f362)), // 362
        (43070, "TooltipDisplayComponentFix", R(true, f363)), // 363
        (43090, "RaidRenamesDataFix", I), // 364
        (43090, "ChunkTicketUnpackPosFix", I), // 365
        (43110, "AdvancementsRenameFix", I), // 366
        (43120, "PlayerEquipmentFix", I), // 367
        (43140, "InlineBlockPosFormatFix", R(false, f368)), // 368
        (44200, "NamedEntityConvertUncheckedFix", R(true, |fx| named_entity_convert_unchecked(fx, ENTITY, "minecraft:area_effect_cloud"))), // 369 AreaEffectCloudCustomParticleFix
        (44210, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 370 Added Happy Ghast
        (44240, "FeatureFlagRemoveFix", I), // 371
        (44240, "AddFieldFix", I), // 372
        (44240, "AddFieldFix", R(false, f373)), // 373
        (45310, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 374 Added Copper Golem
        (45320, "AddNewChoices", R(true, |fx| add_new_choices(fx, BLOCK_ENTITY))), // 375 Added Copper Golem Statue Block Entity
        (45330, "AddNewChoices", R(true, |fx| add_new_choices(fx, BLOCK_ENTITY))), // 376 Added Shelf
        (45350, "CopperGolemWeatherStateFix", R(false, f377)), // 377
        (45370, "ChunkDeleteLightFix", I), // 378
        (45410, "BlockRenameFix$1", R(false, |fx| block_rename(fx, renamer(&[("minecraft:chain", "minecraft:iron_chain")])))), // 379
        (45410, "ItemRenameFix$1", R(false, |fx| item_rename(fx, renamer(&[("minecraft:chain", "minecraft:iron_chain")])))), // 380
        (45430, "AddNewChoices", R(true, |fx| add_new_choices(fx, ENTITY))), // 381 Added Mannequin
        (45440, "LegacyWorldBorderFix", I), // 382
        (45480, "WorldSpawnDataFix", I), // 383
        (45480, "PlayerRespawnDataFix", I), // 384
    ]
}

// ProjectileStoredWeaponFix: arrows and spectral arrows written and read as
// the output's choice types.
fn f312(fx: &Fx) -> RuleRef {
    let choice = |id: &'static str| {
        let finder = named_choice(id, fx.input().choice_type(ENTITY, id));
        let out = fx.output().choice_type(ENTITY, id);
        move |t: Typed| t.update_typed_to(&finder, out, |t1| write_and_read_typed(&t1, out, |d| d))
    };
    let (arrow, spectral) = (choice("minecraft:arrow"), choice("minecraft:spectral_arrow"));
    fix_typed(fx.input().ty(ENTITY), fx.output().ty(ENTITY), move |t| spectral(arrow(t)))
}

const MODIFIER_IDS: &[(&str, &str)] = &[
    ("736565d2-e1a7-403d-a3f8-1aeb3e302542", "minecraft:creative_mode_block_range"),
    ("98491ef6-97b1-4584-ae82-71a8cc85cf73", "minecraft:creative_mode_entity_range"),
    ("91AEAA56-376B-4498-935B-2F7F68070635", "minecraft:effect.speed"),
    ("7107DE5E-7CE8-4030-940E-514C1F160890", "minecraft:effect.slowness"),
    ("AF8B6E3F-3328-4C0A-AA36-5BA2BB9DBEF3", "minecraft:effect.haste"),
    ("55FCED67-E92A-486E-9800-B47F202C4386", "minecraft:effect.mining_fatigue"),
    ("648D7064-6A60-4F59-8ABE-C2C23A6DD7A9", "minecraft:effect.strength"),
    ("C0105BF3-AEF8-46B0-9EBC-92943757CCBE", "minecraft:effect.jump_boost"),
    ("22653B89-116E-49DC-9B6B-9971489B5BE5", "minecraft:effect.weakness"),
    ("5D6F0BA2-1186-46AC-B896-C61C5CEE99CC", "minecraft:effect.health_boost"),
    ("EAE29CF0-701E-4ED6-883A-96F798F3DAB5", "minecraft:effect.absorption"),
    ("03C3C89D-7037-4B42-869F-B146BCB64D2E", "minecraft:effect.luck"),
    ("CC5AF142-2BD2-4215-B636-2605AED11727", "minecraft:effect.unluck"),
    ("6555be74-63b3-41f1-a245-77833b3c2562", "minecraft:evil"),
    ("1eaf83ff-7207-4596-b37a-d7a07b3ec4ce", "minecraft:powder_snow"),
    ("662A6B8D-DA3E-4C1C-8813-96EA6097278D", "minecraft:sprinting"),
    ("020E0DFB-87AE-4653-9556-831010E291A0", "minecraft:attacking"),
    ("766bfa64-11f3-11ea-8d71-362b9e155667", "minecraft:baby"),
    ("7E0292F2-9434-48D5-A29F-9583AF7DF27F", "minecraft:covered"),
    ("9e362924-01de-4ddd-a2b2-d0f7a405a174", "minecraft:suffocating"),
    ("5CD17E52-A79A-43D3-A529-90FDE04B181E", "minecraft:drinking"),
    ("B9766B59-9566-4402-BC1F-2EE2A276D836", "minecraft:baby"),
    ("49455A49-7EC5-45BA-B886-3B90B23A1718", "minecraft:attacking"),
    ("845DB27C-C624-495F-8C9F-6020A9A58B6B", "minecraft:armor.boots"),
    ("D8499B04-0E66-4726-AB29-64469D734E0D", "minecraft:armor.leggings"),
    ("9F3D476D-C118-4544-8365-64846904B48E", "minecraft:armor.chestplate"),
    ("2AD3F246-FEE1-4E67-B886-69FD380BB150", "minecraft:armor.helmet"),
    ("C1C72771-8B8E-BA4A-ACE0-81A93C8928B2", "minecraft:armor.body"),
    ("b572ecd2-ac0c-4071-abde-9594af072a37", "minecraft:enchantment.fire_protection"),
    ("40a9968f-5c66-4e2f-b7f4-2ec2f4b3e450", "minecraft:enchantment.blast_protection"),
    ("07a65791-f64d-4e79-86c7-f83932f007ec", "minecraft:enchantment.respiration"),
    ("60b1b7db-fffd-4ad0-817c-d6c6a93d8a45", "minecraft:enchantment.aqua_affinity"),
    ("11dc269a-4476-46c0-aff3-9e17d7eb6801", "minecraft:enchantment.depth_strider"),
    ("87f46a96-686f-4796-b035-22e16ee9e038", "minecraft:enchantment.soul_speed"),
    ("b9716dbd-50df-4080-850e-70347d24e687", "minecraft:enchantment.soul_speed"),
    ("92437d00-c3a7-4f2e-8f6c-1f21585d5dd0", "minecraft:enchantment.swift_sneak"),
    ("5d3d087b-debe-4037-b53e-d84f3ff51f17", "minecraft:enchantment.sweeping_edge"),
    ("3ceb37c0-db62-46b5-bd02-785457b01d96", "minecraft:enchantment.efficiency"),
    ("CB3F55D3-645C-4F38-A497-9C13A33DB5CF", "minecraft:base_attack_damage"),
    ("FA233E1C-4180-4865-B01B-BCCE9785ACA3", "minecraft:base_attack_speed"),
];

const MODIFIER_NAMES: &[(&str, &str)] = &[
    ("Random spawn bonus", "minecraft:random_spawn_bonus"),
    ("Random zombie-spawn bonus", "minecraft:zombie_random_spawn_bonus"),
    ("Leader zombie bonus", "minecraft:leader_zombie_bonus"),
    ("Zombie reinforcement callee charge", "minecraft:reinforcement_callee_charge"),
    ("Zombie reinforcement caller charge", "minecraft:reinforcement_caller_charge"),
];

// AttributeModifierIdFix.fixModifiers: modifiers keyed by their new id (an
// array map, in the order the ids come), named ones summed.
fn fix_modifiers(modifiers: Vec<Dy>) -> Vec<Dy> {
    let mut map: Vec<(String, Dy)> = Vec::new();
    let put = |map: &mut Vec<(String, Dy)>, k: String, v: Dy| match map.iter_mut().find(|(key, _)| *key == k) {
        Some(e) => e.1 = v,
        None => map.push((k, v)),
    };
    for mut d in modifiers {
        let ints = d.get("uuid").as_int_stream_opt_or_empty();
        let uuid = uuid_from_ints(&ints);
        let name = d.get("name").as_string_or("");
        let by_id = uuid.and_then(|u| MODIFIER_IDS.iter().find(|(k, _)| uuid_parse(k) == u).map(|(_, v)| *v));
        let by_name = MODIFIER_NAMES.iter().find(|(k, _)| *k == name).map(|(_, v)| *v);
        if let Some(id) = by_id {
            d = d.clone().set("id", d.create_string(id));
            put(&mut map, id.to_string(), d.remove("uuid").remove("name"));
        } else if let Some(id) = by_name {
            match map.iter().find(|(k, _)| k == id).map(|(_, v)| v.clone()) {
                None => {
                    d = d.clone().set("id", d.create_string(id));
                    put(&mut map, id.to_string(), d.remove("uuid").remove("name"));
                }
                Some(existing) => {
                    let sum = existing.get("amount").as_double(0.0) + d.get("amount").as_double(0.0);
                    let amount = d.create_double(sum);
                    put(&mut map, id.to_string(), existing.set("amount", amount));
                }
            }
        } else {
            let id = format!("minecraft:{}", uuid.map_or("unknown".to_string(), uuid_string));
            d = d.clone().set("id", d.create_string(&id));
            put(&mut map, id, d.remove("uuid").remove("name"));
        }
    }
    map.into_iter().map(|(_, v)| v).collect()
}

fn convert_modifier_for_entity(m: Dy) -> Dy {
    m.rename_field("UUID", "uuid").rename_field("Name", "name").rename_field("Amount", "amount").rename_and_fix_field("Operation", "operation", |d| {
        let name = match d.as_int(0) {
            0 => "add_value",
            1 => "add_multiplied_base",
            2 => "add_multiplied_total",
            _ => "invalid",
        };
        d.create_string(name)
    })
}

fn f315(fx: &Fx) -> RuleRef {
    let item = fx.input().ty(ITEM_STACK);
    let components = find_field(item, "components");
    let items = fix_typed(item, item, move |t| {
        t.update_typed(&components, |c| {
            c.update_remainder(|tag| {
                tag.update("minecraft:attribute_modifiers", |d| {
                    d.update("modifiers", |list| match list.as_stream_opt().result() {
                        Some(items) => list.create_list(fix_modifiers(items)),
                        None => list,
                    })
                })
            })
        })
    });
    let entities = fix_remainder(fx.input().ty(ENTITY), |d| {
        d.rename_and_fix_field("Attributes", "attributes", |list| match list.as_stream_opt().result() {
            Some(items) => list.create_list(items.into_iter().map(|tag| {
                tag.clone().rename_field("Name", "id").rename_field("Base", "base").rename_and_fix_field("Modifiers", "modifiers", |m| match m.as_stream_opt().result() {
                    Some(ms) => tag.create_list(fix_modifiers(ms.into_iter().map(convert_modifier_for_entity).collect())),
                    None => m,
                })
            })),
            None => list,
        })
    });
    rewrite::seq(vec![items, entities])
}

// JukeboxTicksSinceSongStartedFix.
fn f316(fx: &Fx) -> RuleRef {
    named_entity_remainder(fx, BLOCK_ENTITY, "minecraft:jukebox", |tag| {
        let l = tag.get("TickCount").as_long(0).wrapping_sub(tag.get("RecordStartTick").as_long(0));
        let d = tag.clone().remove("IsPlaying").remove("TickCount").remove("RecordStartTick");
        if l > 0 {
            d.set("ticks_since_song_started", tag.create_long(l))
        } else {
            d
        }
    })
}

// OminousBannerRarityFix: an ominous banner's item name made the
// translatable component, and its rarity uncommon.
fn f317(fx: &Fx) -> RuleRef {
    let input = fx.input();
    let be = input.ty(BLOCK_ENTITY);
    let item = input.ty(ITEM_STACK);
    let choice = input.find_choice_type(BLOCK_ENTITY);
    let item_id = field_finder("id", t::named(ITEM_NAME, namespaced_string()));
    let be_components = find_field(be, "components");
    let item_components = find_field(item, "components");
    let item_name = find_field(be_components.ty(), "minecraft:item_name");
    let text = type_finder(input.ty(TEXT_COMPONENT));
    let fix = move |data: Typed, components: &Finder| {
        data.update_typed(components, |c| {
            let ominous = c
                .get_optional_typed(&item_name)
                .and_then(|n| n.get_optional(&text))
                .map(|v| v.into_pair().1.str().to_string())
                .and_then(|s| extract_translation_string(&s))
                .is_some_and(|s| s == "block.minecraft.ominous_banner");
            if !ominous {
                return c;
            }
            c.update_typed(&item_name, |n| n.set_value(&text, V::pair(V::Str(TEXT_COMPONENT.into()), V::Str(translatable_component_json("block.minecraft.ominous_banner")))))
                .update_remainder(|d| {
                    let rarity = d.create_string("uncommon");
                    d.set("minecraft:rarity", rarity)
                })
        })
    };
    let fix = Rc::new(fix);
    let f1 = fix.clone();
    let choice_finder = type_finder(choice);
    let blocks = fix_typed(be, be, move |t| {
        let (key, _) = t.get(&choice_finder).into_pair();
        if key.str() == "minecraft:banner" {
            f1(t, &be_components)
        } else {
            t
        }
    });
    let items = fix_typed(item, item, move |t| {
        let id = t.get_optional(&item_id).map(|v| v.into_pair().1.str().to_string()).unwrap_or_default();
        if id == "minecraft:white_banner" {
            fix(t, &item_components)
        } else {
            t
        }
    });
    rewrite::seq(vec![blocks, items])
}

// AttributeIdPrefixFix.replaceId.
fn attribute_id_prefix(id: &str) -> String {
    let s = t::ensure_namespaced(id);
    for prefix in ["generic.", "horse.", "player.", "zombie."] {
        let p = t::ensure_namespaced(prefix);
        if s.starts_with(&p) {
            return format!("minecraft:{}", &s[p.len()..]);
        }
    }
    id.to_string()
}

// FoodToConsumableFix.
fn f320(fx: &Fx) -> RuleRef {
    write_fix_and_read(fx.input().ty(DATA_COMPONENTS), fx.output().ty(DATA_COMPONENTS), |dynamic| {
        let Some(food) = dynamic.get("minecraft:food").result() else { return dynamic };
        let seconds = food.get("eat_seconds").as_float(1.6);
        let effects: Vec<Dy> = food
            .get("effects")
            .as_stream()
            .into_iter()
            .map(|e| {
                let list = e.create_list(e.get("effect").result());
                let p = e.create_float(e.get("probability").as_float(1.0));
                e.empty_map().set("type", e.create_string("minecraft:apply_effects")).set("effects", list).set("probability", p)
            })
            .collect();
        let mut dynamic = copy_field(&food, "using_converts_to", dynamic, "minecraft:use_remainder");
        dynamic = dynamic.set("minecraft:food", food.remove("eat_seconds").remove("effects").remove("using_converts_to"));
        let consumable = dynamic.empty_map().set("consume_seconds", dynamic.create_float(seconds)).set("on_consume_effects", dynamic.create_list(effects));
        dynamic.set("minecraft:consumable", consumable)
    })
}

// Dynamic.copyField.
pub fn copy_field(source: &Dy, from: &str, target: Dy, to: &str) -> Dy {
    match source.get(from).result() {
        Some(v) => target.set(to, v),
        None => target,
    }
}

// TrialSpawnerConfigInRegistryFix.VanillaTrialChambers: (normal, ominous)
// configs by the registry key of the vanilla config they are.
const TRIAL_CONFIGS: &[(&str, &str, &str)] = &[
    ("trial_chamber/breeze", "{simultaneous_mobs: 1.0f, simultaneous_mobs_added_per_player: 0.5f, spawn_potentials: [{data: {entity: {id: \"minecraft:breeze\"}}, weight: 1}], ticks_between_spawn: 20, total_mobs: 2.0f, total_mobs_added_per_player: 1.0f}", "{loot_tables_to_eject: [{data: \"minecraft:spawners/ominous/trial_chamber/key\", weight: 3}, {data: \"minecraft:spawners/ominous/trial_chamber/consumables\", weight: 7}], simultaneous_mobs: 2.0f, total_mobs: 4.0f}"),
    ("trial_chamber/melee/husk", "{simultaneous_mobs: 3.0f, simultaneous_mobs_added_per_player: 0.5f, spawn_potentials: [{data: {entity: {id: \"minecraft:husk\"}}, weight: 1}], ticks_between_spawn: 20}", "{loot_tables_to_eject: [{data: \"minecraft:spawners/ominous/trial_chamber/key\", weight: 3}, {data: \"minecraft:spawners/ominous/trial_chamber/consumables\", weight: 7}], spawn_potentials: [{data: {entity: {id: \"minecraft:husk\"}, equipment: {loot_table: \"minecraft:equipment/trial_chamber_melee\", slot_drop_chances: 0.0f}}, weight: 1}]}"),
    ("trial_chamber/melee/spider", "{simultaneous_mobs: 3.0f, simultaneous_mobs_added_per_player: 0.5f, spawn_potentials: [{data: {entity: {id: \"minecraft:spider\"}}, weight: 1}], ticks_between_spawn: 20}", "{loot_tables_to_eject: [{data: \"minecraft:spawners/ominous/trial_chamber/key\", weight: 3}, {data: \"minecraft:spawners/ominous/trial_chamber/consumables\", weight: 7}],simultaneous_mobs: 4.0f, total_mobs: 12.0f}"),
    ("trial_chamber/melee/zombie", "{simultaneous_mobs: 3.0f, simultaneous_mobs_added_per_player: 0.5f, spawn_potentials: [{data: {entity: {id: \"minecraft:zombie\"}}, weight: 1}], ticks_between_spawn: 20}", "{loot_tables_to_eject: [{data: \"minecraft:spawners/ominous/trial_chamber/key\", weight: 3}, {data: \"minecraft:spawners/ominous/trial_chamber/consumables\", weight: 7}],spawn_potentials: [{data: {entity: {id: \"minecraft:zombie\"}, equipment: {loot_table: \"minecraft:equipment/trial_chamber_melee\", slot_drop_chances: 0.0f}}, weight: 1}]}"),
    ("trial_chamber/ranged/poison_skeleton", "{simultaneous_mobs: 3.0f, simultaneous_mobs_added_per_player: 0.5f, spawn_potentials: [{data: {entity: {id: \"minecraft:bogged\"}}, weight: 1}], ticks_between_spawn: 20}", "{loot_tables_to_eject: [{data: \"minecraft:spawners/ominous/trial_chamber/key\", weight: 3}, {data: \"minecraft:spawners/ominous/trial_chamber/consumables\", weight: 7}],spawn_potentials: [{data: {entity: {id: \"minecraft:bogged\"}, equipment: {loot_table: \"minecraft:equipment/trial_chamber_ranged\", slot_drop_chances: 0.0f}}, weight: 1}]}"),
    ("trial_chamber/ranged/skeleton", "{simultaneous_mobs: 3.0f, simultaneous_mobs_added_per_player: 0.5f, spawn_potentials: [{data: {entity: {id: \"minecraft:skeleton\"}}, weight: 1}], ticks_between_spawn: 20}", "{loot_tables_to_eject: [{data: \"minecraft:spawners/ominous/trial_chamber/key\", weight: 3}, {data: \"minecraft:spawners/ominous/trial_chamber/consumables\", weight: 7}], spawn_potentials: [{data: {entity: {id: \"minecraft:skeleton\"}, equipment: {loot_table: \"minecraft:equipment/trial_chamber_ranged\", slot_drop_chances: 0.0f}}, weight: 1}]}"),
    ("trial_chamber/ranged/stray", "{simultaneous_mobs: 3.0f, simultaneous_mobs_added_per_player: 0.5f, spawn_potentials: [{data: {entity: {id: \"minecraft:stray\"}}, weight: 1}], ticks_between_spawn: 20}", "{loot_tables_to_eject: [{data: \"minecraft:spawners/ominous/trial_chamber/key\", weight: 3}, {data: \"minecraft:spawners/ominous/trial_chamber/consumables\", weight: 7}], spawn_potentials: [{data: {entity: {id: \"minecraft:stray\"}, equipment: {loot_table: \"minecraft:equipment/trial_chamber_ranged\", slot_drop_chances: 0.0f}}, weight: 1}]}"),
    ("trial_chamber/slow_ranged/poison_skeleton", "{simultaneous_mobs: 4.0f, simultaneous_mobs_added_per_player: 2.0f, spawn_potentials: [{data: {entity: {id: \"minecraft:bogged\"}}, weight: 1}], ticks_between_spawn: 160}", "{loot_tables_to_eject: [{data: \"minecraft:spawners/ominous/trial_chamber/key\", weight: 3}, {data: \"minecraft:spawners/ominous/trial_chamber/consumables\", weight: 7}], spawn_potentials: [{data: {entity: {id: \"minecraft:bogged\"}, equipment: {loot_table: \"minecraft:equipment/trial_chamber_ranged\", slot_drop_chances: 0.0f}}, weight: 1}]}"),
    ("trial_chamber/slow_ranged/skeleton", "{simultaneous_mobs: 4.0f, simultaneous_mobs_added_per_player: 2.0f, spawn_potentials: [{data: {entity: {id: \"minecraft:skeleton\"}}, weight: 1}], ticks_between_spawn: 160}", "{loot_tables_to_eject: [{data: \"minecraft:spawners/ominous/trial_chamber/key\", weight: 3}, {data: \"minecraft:spawners/ominous/trial_chamber/consumables\", weight: 7}], spawn_potentials: [{data: {entity: {id: \"minecraft:skeleton\"}, equipment: {loot_table: \"minecraft:equipment/trial_chamber_ranged\", slot_drop_chances: 0.0f}}, weight: 1}]}"),
    ("trial_chamber/slow_ranged/stray", "{simultaneous_mobs: 4.0f, simultaneous_mobs_added_per_player: 2.0f, spawn_potentials: [{data: {entity: {id: \"minecraft:stray\"}}, weight: 1}], ticks_between_spawn: 160}", "{loot_tables_to_eject: [{data: \"minecraft:spawners/ominous/trial_chamber/key\", weight: 3}, {data: \"minecraft:spawners/ominous/trial_chamber/consumables\", weight: 7}],spawn_potentials: [{data: {entity: {id: \"minecraft:stray\"}, equipment: {loot_table: \"minecraft:equipment/trial_chamber_ranged\", slot_drop_chances: 0.0f}}, weight: 1}]}"),
    ("trial_chamber/small_melee/baby_zombie", "{simultaneous_mobs: 2.0f, simultaneous_mobs_added_per_player: 0.5f, spawn_potentials: [{data: {entity: {IsBaby: 1b, id: \"minecraft:zombie\"}}, weight: 1}], ticks_between_spawn: 20}", "{loot_tables_to_eject: [{data: \"minecraft:spawners/ominous/trial_chamber/key\", weight: 3}, {data: \"minecraft:spawners/ominous/trial_chamber/consumables\", weight: 7}], spawn_potentials: [{data: {entity: {IsBaby: 1b, id: \"minecraft:zombie\"}, equipment: {loot_table: \"minecraft:equipment/trial_chamber_melee\", slot_drop_chances: 0.0f}}, weight: 1}]}"),
    ("trial_chamber/small_melee/cave_spider", "{simultaneous_mobs: 3.0f, simultaneous_mobs_added_per_player: 0.5f, spawn_potentials: [{data: {entity: {id: \"minecraft:cave_spider\"}}, weight: 1}], ticks_between_spawn: 20}", "{loot_tables_to_eject: [{data: \"minecraft:spawners/ominous/trial_chamber/key\", weight: 3}, {data: \"minecraft:spawners/ominous/trial_chamber/consumables\", weight: 7}], simultaneous_mobs: 4.0f, total_mobs: 12.0f}"),
    ("trial_chamber/small_melee/silverfish", "{simultaneous_mobs: 3.0f, simultaneous_mobs_added_per_player: 0.5f, spawn_potentials: [{data: {entity: {id: \"minecraft:silverfish\"}}, weight: 1}], ticks_between_spawn: 20}", "{loot_tables_to_eject: [{data: \"minecraft:spawners/ominous/trial_chamber/key\", weight: 3}, {data: \"minecraft:spawners/ominous/trial_chamber/consumables\", weight: 7}], simultaneous_mobs: 4.0f, total_mobs: 12.0f}"),
    ("trial_chamber/small_melee/slime", "{simultaneous_mobs: 3.0f, simultaneous_mobs_added_per_player: 0.5f, spawn_potentials: [{data: {entity: {Size: 1, id: \"minecraft:slime\"}}, weight: 3}, {data: {entity: {Size: 2, id: \"minecraft:slime\"}}, weight: 1}], ticks_between_spawn: 20}", "{loot_tables_to_eject: [{data: \"minecraft:spawners/ominous/trial_chamber/key\", weight: 3}, {data: \"minecraft:spawners/ominous/trial_chamber/consumables\", weight: 7}], simultaneous_mobs: 4.0f, total_mobs: 12.0f}"),
];

// TrialSpawnerConfigInRegistryFix.VanillaTrialChambers.removeDefaults.
fn remove_trial_defaults(mut c: Compound) -> Compound {
    let int_or = |c: &Compound, k: &str| c.get(k).and_then(|t| t.int_value()).unwrap_or(0);
    let float_or = |c: &Compound, k: &str| c.get(k).and_then(|t| t.float_value()).unwrap_or(0.0);
    if int_or(&c, "spawn_range") == 4 {
        c.remove("spawn_range");
    }
    if float_or(&c, "total_mobs") == 6.0 {
        c.remove("total_mobs");
    }
    if float_or(&c, "simultaneous_mobs") == 2.0 {
        c.remove("simultaneous_mobs");
    }
    if float_or(&c, "total_mobs_added_per_player") == 2.0 {
        c.remove("total_mobs_added_per_player");
    }
    if float_or(&c, "simultaneous_mobs_added_per_player") == 1.0 {
        c.remove("simultaneous_mobs_added_per_player");
    }
    if int_or(&c, "ticks_between_spawn") == 40 {
        c.remove("ticks_between_spawn");
    }
    c
}

fn f321(fx: &Fx) -> RuleRef {
    // CONFIGS_TO_KEY: three (normal, ominous) pairs for each config, the
    // later put of an equal pair winning.
    let mut keys: Vec<(Tag, Tag, &'static str)> = Vec::new();
    for (name, normal, ominous) in TRIAL_CONFIGS {
        let n = crate::nbt::parse_compound(normal).expect("a trial spawner config");
        let o = crate::nbt::parse_compound(ominous).expect("a trial spawner config");
        let merged = merge_compounds(&n, &o);
        let defaults = remove_trial_defaults(merged.clone());
        for second in [o, merged, defaults] {
            let pair = (Tag::Compound(n.clone()), Tag::Compound(second));
            keys.retain(|(a, b, _)| !(ops::tag_eq(a, &pair.0) && ops::tag_eq(b, &pair.1)));
            keys.push((pair.0, pair.1, name));
        }
    }
    named_entity_remainder(fx, BLOCK_ENTITY, "minecraft:trial_spawner", move |tag| {
        let Some(normal) = tag.get("normal_config").result() else { return tag };
        let Some(ominous) = tag.get("ominous_config").result() else { return tag };
        let found = keys.iter().find(|(a, b, _)| Dy::of(a.clone()) == normal && Dy::of(b.clone()) == ominous);
        match found {
            None => tag,
            Some((_, _, name)) => {
                let n = tag.create_string(&format!("minecraft:{name}/normal"));
                let o = tag.create_string(&format!("minecraft:{name}/ominous"));
                tag.set("normal_config", n).set("ominous_config", o)
            }
        }
    })
}

// FireResistantToDamageResistantComponentFix.
fn f322(fx: &Fx) -> RuleRef {
    component_remainder(fx, "minecraft:fire_resistant", "minecraft:damage_resistant", |c| {
        let types = c.create_string("#minecraft:is_fire");
        Some(c.empty_map().set("types", types))
    })
}

// BoatSplitFix: a boat or chest boat is the entity of its variant.
fn f323(fx: &Fx) -> RuleRef {
    let id = field_finder("id", namespaced_string());
    let output = fx.output().ty(ENTITY);
    fix_typed(fx.input().ty(ENTITY), output, move |t| {
        let Some(name) = t.get_optional(&id).map(|v| v.str().to_string()) else { return Typed::new(output, t.v) };
        if name != "minecraft:boat" && name != "minecraft:chest_boat" {
            return Typed::new(output, t.v);
        }
        let rem = t.get_or_create(&remainder_finder()).dy();
        let variant = rem.get("Type").as_string().result();
        let chest = name == "minecraft:chest_boat";
        let new = match variant.as_deref() {
            Some("spruce") => if chest { "minecraft:spruce_chest_boat" } else { "minecraft:spruce_boat" },
            Some("birch") => if chest { "minecraft:birch_chest_boat" } else { "minecraft:birch_boat" },
            Some("jungle") => if chest { "minecraft:jungle_chest_boat" } else { "minecraft:jungle_boat" },
            Some("acacia") => if chest { "minecraft:acacia_chest_boat" } else { "minecraft:acacia_boat" },
            Some("cherry") => if chest { "minecraft:cherry_chest_boat" } else { "minecraft:cherry_boat" },
            Some("dark_oak") => if chest { "minecraft:dark_oak_chest_boat" } else { "minecraft:dark_oak_boat" },
            Some("mangrove") => if chest { "minecraft:mangrove_chest_boat" } else { "minecraft:mangrove_boat" },
            Some("bamboo") => if chest { "minecraft:bamboo_chest_raft" } else { "minecraft:bamboo_raft" },
            _ => if chest { "minecraft:oak_chest_boat" } else { "minecraft:oak_boat" },
        };
        Typed::new(output, t.v).update_remainder(|d| d.remove("Type")).set_value(&id, V::Str(new.into()))
    })
}

// LockComponentPredicateFix.fixLock: a lock string as a predicate on the
// custom name ("" and non-strings are none).
fn fix_lock(tag: Dy) -> Option<Dy> {
    let s = tag.as_string().result()?;
    if s.is_empty() {
        return None;
    }
    let escaped: String = s.chars().flat_map(|c| match c {
        '"' => vec!['\\', '"'],
        '\\' => vec!['\\', '\\'],
        c => vec![c],
    }).collect();
    let name = tag.create_string(&format!("\"{escaped}\""));
    let components = tag.empty_map().set("minecraft:custom_name", name);
    Some(tag.empty_map().set("components", components))
}

// ContainerBlockEntityLockPredicateFix.
fn f326(fx: &Fx) -> RuleRef {
    fix_remainder(fx.input().find_choice_type(BLOCK_ENTITY), |tag| tag.rename_and_fix_field_opt("Lock", "lock", fix_lock))
}

// EntitySalmonSizeFix.
fn f330(fx: &Fx) -> RuleRef {
    named_entity_remainder(fx, ENTITY, "minecraft:salmon", |d| {
        if d.get("type").as_string_or("medium") == "large" {
            d
        } else {
            let medium = d.create_string("medium");
            d.set("type", medium)
        }
    })
}

// EquippableAssetRenameFix.
fn f332(fx: &Fx) -> RuleRef {
    let ty = fx.input().ty(DATA_COMPONENTS);
    let equippable = find_field(ty, "minecraft:equippable");
    fix_typed(ty, ty, move |t| t.update_typed(&equippable, |e| e.update_remainder(|d| d.rename_field("model", "asset_id"))))
}

// CustomModelDataExpandFix.
fn f333(fx: &Fx) -> RuleRef {
    fix_remainder(fx.input().ty(DATA_COMPONENTS), |d| {
        d.update("minecraft:custom_model_data", |c| {
            let f = c.as_number().result().map_or(0.0, |n| n.float_value());
            let list = c.create_list([c.create_float(f)]);
            c.create_map([(c.create_string("floats"), list)])
        })
    })
}

// InvalidLockComponentFix.fixLock: a lock on the name "" is none.
fn fix_invalid_lock(tag: Dy) -> Option<Dy> {
    let one_field = |d: &Dy, key: &str, p: &dyn Fn(&Dy) -> bool| d.as_map_opt().result().is_some_and(|m| m.len() == 1) && d.get(key).result().is_some_and(|v| p(&v));
    let broken = one_field(&tag, "components", &|c| one_field(c, "minecraft:custom_name", &|n| n.as_string().result().as_deref() == Some("\"\"")));
    if broken {
        None
    } else {
        Some(tag)
    }
}

// InvalidBlockEntityLockFix.
fn f334(fx: &Fx) -> RuleRef {
    fix_remainder(fx.input().ty(BLOCK_ENTITY), |tag| match tag.get("lock").result() {
        None => tag,
        Some(lock) => match fix_invalid_lock(lock) {
            Some(l) => tag.set("lock", l),
            None => tag.remove("lock"),
        },
    })
}

// BlockEntityFurnaceBurnTimeFix.
fn furnace_burn_time(fx: &Fx, entity: &'static str) -> RuleRef {
    named_entity_remainder(fx, BLOCK_ENTITY, entity, |tag| {
        let tag = tag.rename_field("CookTime", "cooking_time_spent").rename_field("CookTimeTotal", "cooking_total_time").rename_field("BurnTime", "lit_time_remaining");
        let lit = tag.get("lit_time_remaining").result();
        tag.set_field_if_present("lit_total_time", lit)
    })
}

// EntityAttributeBaseValueFix (follow range).
fn attribute_base_value(fx: &Fx, entity: &'static str, value: fn(f64) -> f64) -> RuleRef {
    named_entity_remainder(fx, ENTITY, entity, move |tag| {
        let t2 = tag.clone();
        tag.update("attributes", |list| {
            t2.create_list(list.as_stream().into_iter().map(|a| {
                if t::ensure_namespaced(&a.get("id").as_string_or("")) != "minecraft:follow_range" {
                    a
                } else {
                    let base = a.create_double(value(a.get("base").as_double(0.0)));
                    a.set("base", base)
                }
            }))
        })
    })
}

// UnflattenTextComponentFix: a component's JSON text read as NBT.
fn f346(fx: &Fx) -> RuleRef {
    let input = fx.input().ty(TEXT_COMPONENT);
    let output = fx.output().ty(TEXT_COMPONENT);
    fix_fn(input, output, move |v| {
        let (_, json) = v.into_pair();
        read_typed_or_throw(output, &unflatten_json(json.str()), true).v
    })
}

// UnflattenTextComponentFix.unflattenJson: the JSON as NBT, or the text as
// a string when it is not JSON (or JSON null).
fn unflatten_json(json: &str) -> Dy {
    if let Ok(element) = gson::parse_lenient(json) {
        if element != Je::Null {
            if let Ok(nbt) = gson::json_to_nbt(&element) {
                return Dy(nbt);
            }
        }
    }
    Dy::of(Tag::String(json.to_string()))
}

// The text component type the hover and stringified-flag fixes expect: a
// string or list, or {extra, separator, hoverEvent, rest}.
fn text_component_shape(component: TyId, hover: TyId) -> TyId {
    t::named(
        TEXT_COMPONENT,
        t::sum(
            t::sum(string(), t::list(component)),
            t::product(t::optional(t::field("extra", t::list(component))), t::product(t::optional(t::field("separator", component)), t::product(t::optional(t::field("hoverEvent", hover)), t::remainder()))),
        ),
    )
}

// The remainder of a text component of that shape, in place.
fn text_remainder(v: V, f: &dyn Fn(V, V) -> (V, V)) -> V {
    let (name, either) = v.into_pair();
    let either = match either {
        V::Right(p1) => {
            let (extra, rest1) = p1.into_pair();
            let (separator, rest2) = rest1.into_pair();
            let (hover, rem) = rest2.into_pair();
            let (hover, rem) = f(hover, rem);
            V::Right(Box::new(V::pair(extra, V::pair(separator, V::pair(hover, rem)))))
        }
        left => left,
    };
    V::pair(name, either)
}

// LegacyHoverEventFix: a hoverEvent of the old {action, value} form made a
// show_text event (or a placeholder showing its JSON).
fn f347(fx: &Fx) -> RuleRef {
    let input = fx.input();
    let hover = t::find_field_type(input.ty(TEXT_COMPONENT), "hoverEvent").unwrap_or_else(|| refuse("no hoverEvent field"));
    let ty = text_component_shape(input.type_raw(TEXT_COMPONENT), hover);
    if !t::same(ty, input.ty(TEXT_COMPONENT)) {
        refuse("Text component type did not match");
    }
    fix_fn(ty, ty, move |v| {
        text_remainder(v, &|hover_v, rem| {
            let d = rem.clone().dy();
            let Some(event) = d.get("hoverEvent").result() else { return (hover_v, rem) };
            if event.get("value").result().is_none() {
                return (hover_v, rem);
            }
            let action = match &hover_v {
                V::Left(h) => match &**h {
                    V::Pair(k, _) => k.str().to_string(),
                    _ => String::new(),
                },
                _ => String::new(),
            };
            let new = if action == "show_text" {
                read_typed_or_throw(hover, &event.rename_field("value", "contents"), false).v
            } else {
                let json = gson::to_stable_string(&gson::nbt_to_json(&event.0));
                let mut contents = Compound::new();
                contents.put("text", Tag::String(format!("Legacy hoverEvent: {json}")));
                let mut c = Compound::new();
                c.put("action", Tag::String("show_text".into()));
                c.put("contents", Tag::Compound(contents));
                read_typed_or_throw(hover, &Dy::of(Tag::Compound(c)), false).v
            };
            (V::Left(Box::new(new)), rem)
        })
    })
}

// TextComponentStringifiedFlagsFix: the style flags written as strings made
// booleans.
fn f348(fx: &Fx) -> RuleRef {
    let ty = fx.input().ty(TEXT_COMPONENT);
    fix_fn(ty, ty, move |v| {
        text_remainder(v, &|hover, rem| {
            let mut d = rem.dy();
            for flag in ["bold", "italic", "underlined", "strikethrough", "obfuscated"] {
                d = d.update(flag, |x| match x.as_string().result() {
                    Some(s) => x.create_boolean(s.eq_ignore_ascii_case("true")),
                    None => x,
                });
            }
            (hover, V::Dyn(d.0))
        })
    })
}

// TextComponentHoverAndClickEventFix: hoverEvent and clickEvent become
// hover_event and click_event of the new forms.
fn f349(fx: &Fx) -> RuleRef {
    let input = fx.input();
    let hover = t::find_field_type(input.ty(TEXT_COMPONENT), "hoverEvent").unwrap_or_else(|| refuse("no hoverEvent field"));
    let output = fx.output().ty(TEXT_COMPONENT);
    let ty = text_component_shape(input.type_raw(TEXT_COMPONENT), hover);
    if !t::same(ty, input.ty(TEXT_COMPONENT)) {
        refuse("Text component type did not match");
    }
    let patched = patch_sub_type(ty, ty, output);
    fix_fn(ty, output, move |v| {
        let flag = match &v {
            V::Pair(_, e) => match &**e {
                V::Right(p1) => {
                    let V::Pair(_, rest1) = &**p1 else { unreachable!() };
                    let V::Pair(_, rest2) = &**rest1 else { unreachable!() };
                    let V::Pair(h, rem) = &**rest2 else { unreachable!() };
                    let has_hover = matches!(**h, V::Left(_));
                    let has_click = match &**rem {
                        V::Dyn(n) => Dy(n.clone()).get("clickEvent").is_present(),
                        _ => false,
                    };
                    has_hover || has_click
                }
                _ => false,
            },
            _ => false,
        };
        if !flag {
            return v;
        }
        write_and_read_typed(&Typed::new(patched, v), output, |d| {
            d.rename_and_fix_field("hoverEvent", "hover_event", fix_hover_event).rename_and_fix_field_opt("clickEvent", "click_event", fix_click_event)
        })
        .v
    })
}

fn copy_fields(mut new: Dy, old: &Dy, fields: &[&str]) -> Dy {
    for f in fields {
        new = copy_field(old, f, new, f);
    }
    new
}

fn fix_hover_event(d: Dy) -> Dy {
    match d.get("action").as_string_or("").as_str() {
        "show_text" => d.rename_field("contents", "value"),
        "show_item" => {
            let contents = d.get("contents").or_else_empty_map();
            if contents.as_string().is_ok() {
                d.rename_field("contents", "id")
            } else {
                copy_fields(d.remove("contents"), &contents, &["id", "count", "components"])
            }
        }
        "show_entity" => {
            let contents = d.get("contents").or_else_empty_map();
            copy_fields(d.remove("contents"), &contents, &["id", "type", "name"]).rename_field("id", "uuid").rename_field("type", "id")
        }
        _ => d,
    }
}

fn fix_click_event(d: Dy) -> Option<Dy> {
    let action = d.get("action").as_string_or("");
    let value = d.get("value").as_string_or("");
    match action.as_str() {
        "open_url" => validate_uri(&value).then(|| d.rename_field("value", "url")),
        "open_file" => Some(d.rename_field("value", "path")),
        "run_command" | "suggest_command" => validate_chat(&value).then(|| d.rename_field("value", "command")),
        "change_page" => {
            let page = d.get("value").result().and_then(|v| parse_old_page(&v))?;
            let page = d.create_int(page.max(1));
            Some(d.remove("value").set("page", page))
        }
        _ => Some(d),
    }
}

fn parse_old_page(d: &Dy) -> Option<i32> {
    match d.as_number().result() {
        Some(n) => Some(n.int_value()),
        None => java_parse_int(&d.as_string_or("")),
    }
}

// Integer.parseInt.
pub fn java_parse_int(s: &str) -> Option<i32> {
    let (negative, digits) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut v: i64 = 0;
    for c in digits.bytes() {
        v = v * 10 + (c - b'0') as i64;
        if v > i32::MAX as i64 + 1 {
            return None;
        }
    }
    let v = if negative { -v } else { v };
    (v >= i32::MIN as i64 && v <= i32::MAX as i64).then_some(v as i32)
}

// TextComponentHoverAndClickEventFix.validateUri: java.net.URI parses it and
// its scheme is http or https. A string with no scheme (no ":" before "/",
// "?" or "#") is a relative reference, and a bad scheme fails the parse:
// both are false whatever the rest. For http(s), this port decides the
// hierarchical URIs whose parse it knows (an authority of letters, digits,
// dots and hyphens with a port, and path, query and fragment characters
// java.net.URI takes without escapes) and refuses the others.
fn validate_uri(uri: &str) -> bool {
    match uri.find(['/', '?', '#', ':']) {
        Some(i) if uri.as_bytes()[i] == b':' => {}
        _ => return false,
    }
    let colon = uri.find(':').unwrap();
    let scheme = &uri[..colon];
    let scheme_ok = scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !scheme_ok || !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")) {
        return false;
    }
    let Some(after) = uri[colon + 1..].strip_prefix("//") else { refuse("an opaque http URI (java.net.URI's parse)") };
    let end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let (authority, tail) = after.split_at(end);
    let tail_ok = tail.matches('#').count() <= 1 && tail.chars().all(|c| c.is_ascii_alphanumeric() || "-_.!~*'();/?:@&=+$,#".contains(c));
    if !tail_ok {
        refuse("an http URL java.net.URI would parse in a way this port does not model");
    }
    if authority.is_empty() {
        // An empty authority is allowed only before a path, query or fragment.
        return !tail.is_empty();
    }
    let mut parts = authority.splitn(2, ':');
    let host = parts.next().unwrap();
    let port_ok = parts.next().is_none_or(|p| p.chars().all(|c| c.is_ascii_digit()));
    if !port_ok || !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        refuse("an http URL authority java.net.URI would parse in a way this port does not model");
    }
    true
}

// TextComponentHoverAndClickEventFix.validateChat.
fn validate_chat(s: &str) -> bool {
    !s.chars().any(|c| c == '\u{a7}' || (c as u32) < 0x20 || c == '\u{7f}')
}

// DropChancesFormatFix.
fn f350(fx: &Fx) -> RuleRef {
    fix_remainder(fx.input().ty(ENTITY), |d| {
        let chances = |o: ODy| o.as_stream().into_iter().map(|x| x.as_float(0.085)).collect::<Vec<f32>>();
        let armor = chances(d.get("ArmorDropChances"));
        let hand = chances(d.get("HandDropChances"));
        let body = d.get("body_armor_drop_chance").as_number().result().map_or(0.085, |n| n.float_value());
        let d = d.remove("ArmorDropChances").remove("HandDropChances").remove("body_armor_drop_chance");
        let mut slots = d.empty_map();
        for (names, values) in [(&["feet", "legs", "chest", "head"][..], &armor), (&["mainhand", "offhand"][..], &hand)] {
            for (i, name) in names.iter().enumerate() {
                if i >= values.len() {
                    break;
                }
                if values[i] != 0.085 {
                    slots = slots.clone().set(name, slots.create_float(values[i]));
                }
            }
        }
        if body != 0.085 {
            slots = slots.clone().set("body", d.create_float(body));
        }
        if slots != d.empty_map() {
            d.set("drop_chances", slots)
        } else {
            d
        }
    })
}

// AreaEffectCloudDurationScaleFix.
fn f353(fx: &Fx) -> RuleRef {
    named_entity_remainder(fx, ENTITY, "minecraft:area_effect_cloud", |d| {
        let scale = d.create_float(0.25);
        d.set("potion_duration_scale", scale)
    })
}

// EntitySpawnerItemVariantComponentFix: bucketed mobs' and paintings'
// variants become components of their own.
fn f355(fx: &Fx) -> RuleRef {
    let item = fx.input().ty(ITEM_STACK);
    let id = field_finder("id", t::named(ITEM_NAME, namespaced_string()));
    let components = find_field(item, "components");
    fix_typed(item, item, move |t| {
        let name = t.get_optional(&id).map(|v| v.into_pair().1.str().to_string()).unwrap_or_default();
        let bucket = |f: fn(Dy, Dy) -> Dy| {
            move |c: Typed| {
                c.update_remainder(|d| match d.get("minecraft:bucket_entity_data").result() {
                    Some(data) => f(d, data),
                    None => d,
                })
            }
        };
        match name.as_str() {
            "minecraft:salmon_bucket" => t.update_typed(&components, bucket(fix_salmon_bucket)),
            "minecraft:axolotl_bucket" => t.update_typed(&components, bucket(fix_axolotl_bucket)),
            "minecraft:tropical_fish_bucket" => t.update_typed(&components, bucket(fix_tropical_fish_bucket)),
            "minecraft:painting" => t.update_typed(&components, |c| {
                let ty = c.ty;
                write_and_read_typed(&c, ty, fix_painting)
            }),
            _ => t,
        }
    })
}

fn fix_tropical_fish_bucket(data: Dy, entity: Dy) -> Dy {
    let Some(n) = entity.get("BucketVariantTag").as_number().result() else { return data };
    let i = n.int_value();
    let pattern = match i & 65535 {
        1 => "flopper",
        256 => "sunstreak",
        257 => "stripey",
        512 => "snooper",
        513 => "glitter",
        768 => "dasher",
        769 => "blockfish",
        1024 => "brinely",
        1025 => "betty",
        1280 => "spotty",
        1281 => "clayfish",
        _ => "kob",
    };
    let (p, b, c) = (data.create_string(pattern), data.create_string(dye_color_name(i >> 16 & 0xff)), data.create_string(dye_color_name(i >> 24 & 0xff)));
    data.update("minecraft:bucket_entity_data", |d| d.remove("BucketVariantTag")).set("minecraft:tropical_fish/pattern", p).set("minecraft:tropical_fish/base_color", b).set("minecraft:tropical_fish/pattern_color", c)
}

fn fix_axolotl_bucket(data: Dy, entity: Dy) -> Dy {
    let Some(n) = entity.get("Variant").as_number().result() else { return data };
    let name = match n.int_value() {
        1 => "wild",
        2 => "gold",
        3 => "cyan",
        4 => "blue",
        _ => "lucy",
    };
    let v = data.create_string(name);
    data.update("minecraft:bucket_entity_data", |d| d.remove("Variant")).set("minecraft:axolotl/variant", v)
}

fn fix_salmon_bucket(data: Dy, entity: Dy) -> Dy {
    match entity.get("type").result() {
        None => data,
        Some(size) => data.update("minecraft:bucket_entity_data", |d| d.remove("type")).set("minecraft:salmon/size", size),
    }
}

fn fix_painting(mut data: Dy) -> Dy {
    let Some(entity) = data.get("minecraft:entity_data").result() else { return data };
    if entity.get("id").as_string().result().as_deref() != Some("minecraft:painting") {
        return data;
    }
    let variant = entity.get("variant").result();
    let rest = entity.remove("variant");
    if rest.clone().remove("id") == rest.empty_map() {
        data = data.remove("minecraft:entity_data");
    } else {
        data = data.set("minecraft:entity_data", rest);
    }
    if let Some(v) = variant {
        data = data.set("minecraft:painting/variant", v);
    }
    data
}

// SaddleEquipmentSlotFix: a saddle item or flag becomes the saddle slot.
fn f356(fx: &Fx) -> RuleRef {
    let choice = type_finder(fx.input().find_choice_type(ENTITY));
    let input = fx.input().ty(ENTITY);
    let output = fx.output().ty(ENTITY);
    let patched = patch_sub_type(input, input, output);
    fix_typed(input, output, move |t| {
        let name = t.get_optional(&choice).map(|v| t::ensure_namespaced(v.into_pair().0.str())).unwrap_or_default();
        let cast = Typed::new(patched, t.v.clone());
        match name.as_str() {
            "minecraft:horse" | "minecraft:skeleton_horse" | "minecraft:zombie_horse" | "minecraft:donkey" | "minecraft:mule" | "minecraft:camel" | "minecraft:llama" | "minecraft:trader_llama" => write_and_read_typed(&cast, output, |d| {
                if d.get("SaddleItem").result().is_none() {
                    d
                } else {
                    saddle_drop_chances(d.rename_field("SaddleItem", "saddle"))
                }
            }),
            "minecraft:pig" | "minecraft:strider" => write_and_read_typed(&cast, output, |d| {
                let saddled = d.get("Saddle").as_boolean(false);
                let d = d.remove("Saddle");
                if !saddled {
                    return d;
                }
                let saddle = d.empty_map().set("id", d.create_string("minecraft:saddle")).set("count", d.create_int(1));
                saddle_drop_chances(d.set("saddle", saddle))
            }),
            _ => Typed::new(output, t.v),
        }
    })
}

fn saddle_drop_chances(d: Dy) -> Dy {
    let chances = d.get("drop_chances").or_else_empty_map().set("saddle", d.create_float(2.0));
    d.set("drop_chances", chances)
}

// EquipmentFormatFix: ArmorItems, HandItems, body_armor_item and saddle
// become the equipment map.
fn f357(fx: &Fx) -> RuleRef {
    let old_item = fx.input().type_raw(ITEM_STACK);
    let new_item = fx.output().type_raw(ITEM_STACK);
    let id = find_field(old_item, "id");
    let opt = |name: &str, ty: TyId| t::optional(t::field(name, ty));
    let old = t::named(
        "entity_equipment",
        t::product(opt("ArmorItems", t::list(old_item)), t::product(opt("HandItems", t::list(old_item)), t::product(opt("body_armor_item", old_item), opt("saddle", old_item)))),
    );
    let new = t::named(
        "entity_equipment",
        t::optional(t::field(
            "equipment",
            t::product(
                opt("mainhand", new_item),
                t::product(opt("offhand", new_item), t::product(opt("feet", new_item), t::product(opt("legs", new_item), t::product(opt("chest", new_item), t::product(opt("head", new_item), t::product(opt("body", new_item), t::product(opt("saddle", new_item), t::remainder()))))))),
            ),
        )),
    );
    if !t::same(old, fx.input().ty("entity_equipment")) {
        refuse("Input entity_equipment type does not match expected");
    }
    if !t::same(new, fx.output().ty("entity_equipment")) {
        refuse("Output entity_equipment type does not match expected");
    }
    fix_fn(old, new, move |v| {
        let (name, fields) = v.into_pair();
        let (armor, rest) = fields.into_pair();
        let (hand, rest) = rest.into_pair();
        let (body, saddle) = rest.into_pair();
        let list = |e: V| match e {
            V::Left(l) => match *l {
                V::List(items) => items,
                other => refuse(format!("a list that is no list {other:?}")),
            },
            _ => Vec::new(),
        };
        let (armor, hand) = (list(armor), list(hand));
        let empty = || V::Right(Box::new(V::Unit));
        let item = |items: &Vec<V>, i: usize| -> V {
            if i >= items.len() {
                return empty();
            }
            let x = items[i].clone();
            if Typed::new(old_item, x.clone()).get_optional(&id).is_none() {
                empty()
            } else {
                V::Left(Box::new(x))
            }
        };
        let slots = [item(&armor, 0), item(&armor, 1), item(&armor, 2), item(&armor, 3), item(&hand, 0), item(&hand, 1)];
        let all_empty = matches!(body, V::Right(_)) && matches!(saddle, V::Right(_)) && slots.iter().all(|s| matches!(s, V::Right(_)));
        if all_empty {
            return V::pair(name, empty());
        }
        let [feet, legs, chest, head, mainhand, offhand] = slots;
        let tail = V::pair(body, V::pair(saddle, V::Dyn(None)));
        let fields = V::pair(mainhand, V::pair(offhand, V::pair(feet, V::pair(legs, V::pair(chest, V::pair(head, tail))))));
        V::pair(name, V::Left(Box::new(fields)))
    })
}

// EntityFallDistanceFloatToDoubleFix (for entities).
fn f359(fx: &Fx) -> RuleRef {
    fix_remainder(fx.output().ty(ENTITY), |d| d.rename_and_fix_field("FallDistance", "fall_distance", |x| x.create_double(x.as_float(0.0) as f64)))
}

// ThrownPotionSplitFix: a thrown potion is a splash or lingering potion by
// its item.
fn f362(fx: &Fx) -> RuleRef {
    let input = fx.input();
    let output = fx.output();
    let potion = patch_sub_type(input.choice_type(ENTITY, "minecraft:potion"), input.ty(ENTITY), output.ty(ENTITY));
    let item = find_field(potion, "Item");
    let id = field_finder("id", t::named(ITEM_NAME, namespaced_string()));
    entity_rename(fx, move |name, typed| {
        if name != "minecraft:potion" {
            return (name.to_string(), typed);
        }
        let item_id = typed.get_optional_typed(&item).and_then(|i| i.get_optional(&id)).map(|v| t::ensure_namespaced(v.into_pair().1.str())).unwrap_or_default();
        let new = if item_id == "minecraft:lingering_potion" { "minecraft:lingering_potion" } else { "minecraft:splash_potion" };
        (new.to_string(), typed)
    })
}

const ADDITIONAL_TOOLTIP_TYPES: &[&str] = &[
    "minecraft:banner_patterns",
    "minecraft:bees",
    "minecraft:block_entity_data",
    "minecraft:block_state",
    "minecraft:bundle_contents",
    "minecraft:charged_projectiles",
    "minecraft:container",
    "minecraft:container_loot",
    "minecraft:firework_explosion",
    "minecraft:fireworks",
    "minecraft:instrument",
    "minecraft:map_id",
    "minecraft:painting/variant",
    "minecraft:pot_decorations",
    "minecraft:potion_contents",
    "minecraft:tropical_fish/pattern",
    "minecraft:written_book_content",
];

// TooltipDisplayComponentFix: show_in_tooltip flags and the hide components
// become tooltip_display.
fn f363(fx: &Fx) -> RuleRef {
    let ty = fx.input().ty(DATA_COMPONENTS);
    let out = fx.output().ty(DATA_COMPONENTS);
    let place = find_field(ty, "minecraft:can_place_on");
    let brk = find_field(ty, "minecraft:can_break");
    let place_ty = t::find_field_type(out, "minecraft:can_place_on").unwrap_or_else(|| refuse("no can_place_on"));
    let break_ty = t::find_field_type(out, "minecraft:can_break").unwrap_or_else(|| refuse("no can_break"));
    fix_typed(ty, out, move |data| {
        let set: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let predicate = |data: Typed, finder: &Finder, new_ty: TyId, name: &str| {
            data.update_typed_to(finder, new_ty, |t| {
                write_and_read_typed(&t, new_ty, |d| match d.get("predicates").result() {
                    None => d,
                    Some(p) => {
                        if !d.get("show_in_tooltip").as_boolean(true) {
                            set.borrow_mut().push(name.to_string());
                        }
                        p
                    }
                })
            })
        };
        let data = predicate(data, &place, place_ty, "minecraft:can_place_on");
        let data = predicate(data, &brk, break_ty, "minecraft:can_break");
        data.update_remainder(|mut d| {
            let component = |d: Dy, name: &str, inner: Option<&str>| {
                d.update(name, |c| {
                    if !c.get("show_in_tooltip").as_boolean(true) {
                        set.borrow_mut().push(name.to_string());
                    }
                    let c = c.remove("show_in_tooltip");
                    match inner {
                        Some(field) => c.get(field).result().unwrap_or(c),
                        None => c,
                    }
                })
            };
            d = component(d, "minecraft:trim", None);
            d = component(d, "minecraft:unbreakable", None);
            d = component(d, "minecraft:dyed_color", Some("rgb"));
            d = component(d, "minecraft:attribute_modifiers", Some("modifiers"));
            d = component(d, "minecraft:enchantments", Some("levels"));
            d = component(d, "minecraft:stored_enchantments", Some("levels"));
            d = component(d, "minecraft:jukebox_playable", Some("song"));
            let hide = d.get("minecraft:hide_tooltip").is_present();
            d = d.remove("minecraft:hide_tooltip");
            let hide_additional = d.get("minecraft:hide_additional_tooltip").is_present();
            d = d.remove("minecraft:hide_additional_tooltip");
            if hide_additional {
                for name in ADDITIONAL_TOOLTIP_TYPES {
                    if d.get(name).is_present() {
                        set.borrow_mut().push(name.to_string());
                    }
                }
            }
            let set = set.borrow();
            if set.is_empty() && !hide {
                return d;
            }
            let hidden = d.create_list(java_hash_set_order(&set).iter().map(|s| d.create_string(s)));
            let display = d.create_map([(d.create_string("hide_tooltip"), d.create_boolean(hide)), (d.create_string("hidden_components"), hidden)]);
            d.set("minecraft:tooltip_display", display)
        })
    })
}

// InlineBlockPosFormatFix (entities): inline block positions become arrays.
fn f368(fx: &Fx) -> RuleRef {
    let input = fx.input();
    let finder = |id: &'static str| named_choice(id, input.choice_type(ENTITY, id));
    let (vex, phantom, turtle) = (finder("minecraft:vex"), finder("minecraft:phantom"), finder("minecraft:turtle"));
    let attached = [finder("minecraft:item_frame"), finder("minecraft:glow_item_frame"), finder("minecraft:painting"), finder("minecraft:leash_knot")];
    fix_typed(input.ty(ENTITY), input.ty(ENTITY), move |t| {
        let mut t = t
            .update_remainder(|d| fix_inline_block_pos(d, "SleepingX", "SleepingY", "SleepingZ", "sleeping_pos"))
            .update_typed(&vex, |x| x.update_remainder(|d| fix_inline_block_pos(d.rename_field("LifeTicks", "life_ticks"), "BoundX", "BoundY", "BoundZ", "bound_pos")))
            .update_typed(&phantom, |x| x.update_remainder(|d| fix_inline_block_pos(d.rename_field("Size", "size"), "AX", "AY", "AZ", "anchor_pos")))
            .update_typed(&turtle, |x| {
                x.update_remainder(|d| {
                    let d = d.remove("TravelPosX").remove("TravelPosY").remove("TravelPosZ");
                    fix_inline_block_pos(d, "HomePosX", "HomePosY", "HomePosZ", "home_pos").rename_field("HasEgg", "has_egg")
                })
            });
        for f in &attached {
            t = t.update_typed(f, |x| x.update_remainder(|d| fix_inline_block_pos(d, "TileX", "TileY", "TileZ", "block_pos")));
        }
        t
    })
}

// AddFieldFix(ENTITY, "style", "minecraft:default", "locator_bar_icon"): the
// field is set in the remainder's locator_bar_icon, and the remainder
// becomes that compound (AddFieldFix.addField returns it).
fn f373(fx: &Fx) -> RuleRef {
    fix_typed(fx.input().ty(ENTITY), fx.output().ty(ENTITY), |typed| {
        typed.update_remainder(|d| match d.get("locator_bar_icon").result() {
            None => d,
            Some(icon) => {
                let style = icon.create_string("minecraft:default");
                icon.set("style", style)
            }
        })
    })
}

// CopperGolemWeatherStateFix.
fn f377(fx: &Fx) -> RuleRef {
    named_entity_remainder(fx, ENTITY, "minecraft:copper_golem", |d| {
        d.update("weather_state", |w| {
            let name = match w.as_int(0) {
                1 => "exposed",
                2 => "weathered",
                3 => "oxidized",
                _ => "unaffected",
            };
            w.create_string(name)
        })
    })
}
