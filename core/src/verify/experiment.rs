// McbenchExperiment: one verifier experiment on a world that holds the
// placed build. At the end of each tick it applies its inputs, reads its
// probes and writes report.json, result.json and progress.json to its
// folder, byte for byte as the Java class writes them (Gson's output).
//
// What the Java class does that this port does not model refuses: saved
// data the data fixer's port does not upgrade (older than data version 1948)
// or that this port does not read, and a report that
// comes near its byte budget (Java's partial write depends on when its
// buffers flush). Use, page and book inputs are made by the experiment's
// input player (InputPlayer::make); a book a lectern's menu takes goes
// through its inventory and is held for the lectern.

use super::gson::{Writer, J};
use crate::blocks::container::{self, Cont, Stack};
use crate::blocks::{self, BlockEntity};
use crate::dir::{Dir, Pos};
use crate::level::{refuse, Level};
use crate::entity;
use crate::nbt::{self, Tag};
use crate::codec;
use crate::datafix;
use crate::components;
use crate::loot::ItemStack;
use crate::phys::Vec3;
use crate::registry::{Fluid, State};
use crate::ticks::Tick;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const MINECRAFT_VERSION: &str = "1.21.10";
// The Java engine's name is as long ("McbenchLevel 1.21.10"), so a report is as long.
pub const ENGINE: &str = "mcbench-core 1.21.10";
pub use crate::chunk_save::DATA_VERSION;

// The report stops short of its budget by this much before this port gives
// the experiment to the Java engine: the timings in the report are digits the
// two engines write differently.
const BUDGET_MARGIN: usize = 512;

// The preview of a report holds entries up to this many bytes; the shortest
// possible entry ({"probe":0,"tick":0,"block":"","signal":0}) is more than 40.
const PREVIEW_BYTES: usize = 8000;
const MIN_PREVIEW_ENTRY: usize = 40;

pub struct Experiment {
    request: J,
    folder: PathBuf,
    y_offset: i32,
    probes: Vec<Pos>,
    inventory_probes: Vec<bool>,
    report_missing_inventories: bool,
    observe_item_entities: bool,
    observe_world_inventories: bool,
    observe_living_entities: bool,
    samples: Vec<i32>,
    compact: bool,
    result: J,
    inputs: Vec<J>,
    input_failures: Vec<J>,
    preview_initial: Vec<J>,
    preview_changes: Vec<J>,
    preview_bytes: usize,
    report: Writer,
    imported_entities: Vec<J>,
    imported_block_entities: Vec<J>,
    report_limit: i64,
    start_time: i64,
    probe_nanos: u128,
    max_probe_nanos: u128,
    observation_count: i64,
    action_index: usize,
    sample_index: usize,
    failed_inputs: i64,
    last_tick: i64,
    pub running: bool,
    previous_blocks: Vec<Option<State>>,
    previous_signals: Vec<i32>,
    previous_inventories: Vec<Option<J>>,
    changed_probes: Vec<bool>,
    change_count: i64,
    pending: Option<Pending>,
    last_progress: Option<Instant>,
    // A state's block string, by state.
    names: Vec<Option<String>>,
    // Where a preview entry is written to measure it.
    scratch: Vec<u8>,
    // The detached input player, once an input has made it, and its
    // inventory (empty until a lectern's menu hands it a book).
    input_player: Option<blocks::InputPlayer>,
    input_inventory: blocks::inventory::Inventory,
    // McbenchExperiment.heldLecternBooks: the book each lectern's book input
    // took, until one puts it back.
    held_lectern_books: HashMap<Pos, ItemStack>,
}

// A run of equal observations the compact report has not written yet.
struct Pending {
    start: i32,
    end: i32,
    blocks: Vec<State>,
    signals: Vec<i32>,
    inventories: Vec<Option<J>>,
    item_entities: Option<J>,
    world_inventories: Option<J>,
    living_entities: Option<J>,
}

// What ends an experiment: an exception the Java class throws, as its
// toString.
pub type Failure = String;

impl Experiment {
    // McbenchExperiment's constructor.
    pub fn new(request: J, folder: &Path) -> Result<Experiment, Failure> {
        if request.has("block_data_version") && request.req("block_data_version").int_value() as i64 > DATA_VERSION {
            return Err("java.io.IOException: Cannot import future block data version".into());
        }
        let y_offset = request.req("yOffset").int_value();
        let position = |p: &J| position(p, y_offset);
        let probes: Vec<Pos> = request.req("probes").arr().iter().map(position).collect();
        let flag = |name: &str| request.has(name) && request.req(name).bool();
        let report_missing_inventories = request.has("inventory_missing") && request.req("inventory_missing").str() == "report";
        let mut inventory_probes = vec![false; probes.len()];
        if request.has("inventory_probes") {
            for entry in request.req("inventory_probes").arr() {
                let at = position(entry);
                for (i, probe) in probes.iter().enumerate() {
                    if *probe == at {
                        inventory_probes[i] = true;
                    }
                }
            }
        }
        let compact = request.has("reportEncoding") && request.req("reportEncoding").str() == "rle-v1";
        let samples: Vec<i32> = if request.has("sampleSequence") {
            let sequence = request.req("sampleSequence");
            let (start, step) = (sequence.req("start").long_value(), sequence.req("step").long_value());
            (0..sequence.req("count").int_value() as i64)
                .map(|i| i32::try_from(start + i * step).expect("java.lang.ArithmeticException: integer overflow"))
                .collect()
        } else {
            request.req("samples").arr().iter().map(|s| s.int_value()).collect()
        };
        let count = probes.len();
        Ok(Experiment {
            observe_item_entities: flag("item_entities"),
            observe_world_inventories: flag("world_inventories"),
            observe_living_entities: flag("living_entities"),
            report_limit: request.req("budgets").req("reportBytes").long_value(),
            request,
            folder: folder.to_path_buf(),
            y_offset,
            probes,
            inventory_probes,
            report_missing_inventories,
            samples,
            compact,
            result: J::obj(),
            inputs: Vec::new(),
            input_failures: Vec::new(),
            preview_initial: Vec::new(),
            preview_changes: Vec::new(),
            preview_bytes: 0,
            report: Writer::new(),
            imported_entities: Vec::new(),
            imported_block_entities: Vec::new(),
            start_time: 0,
            probe_nanos: 0,
            max_probe_nanos: 0,
            observation_count: 0,
            action_index: 0,
            sample_index: 0,
            failed_inputs: 0,
            last_tick: -1,
            running: false,
            previous_blocks: vec![None; count],
            previous_signals: vec![0; count],
            previous_inventories: vec![None; count],
            changed_probes: vec![false; count],
            change_count: 0,
            pending: None,
            last_progress: None,
            input_player: None,
            input_inventory: blocks::inventory::Inventory::default(),
            held_lectern_books: HashMap::new(),
            names: Vec::new(),
            scratch: Vec::new(),
        })
    }

    pub fn request(&self) -> &J {
        &self.request
    }

    // The refusals that depend on the request alone, before anything runs.
    pub fn check_ported(&self) {}

    // McbenchExperiment.start: the report's head, once the build is placed.
    pub fn start(&mut self, level: &mut Level, migrations: &[(String, String)]) -> Result<(), Failure> {
        self.import_block_entities(level)?;
        self.import_entities(level)?;
        self.import_scheduled_ticks(level)?;
        let mut result = J::obj();
        result.put("engine", J::string(ENGINE));
        result.put("minecraftVersion", J::string(MINECRAFT_VERSION));
        if self.request.has("block_data_version") {
            result.put("sourceBlockDataVersion", self.request.req("block_data_version").clone());
            let mut changes = J::Arr(Vec::new());
            for (before, after) in migrations {
                if before != after {
                    let mut change = J::obj();
                    change.put("before", J::string(before.as_str()));
                    change.put("after", J::string(after.as_str()));
                    changes.push(change);
                }
            }
            result.put("blockStateMigrations", changes);
        }
        result.put("placementMode", J::string(if self.request.has("placementMode") { self.request.req("placementMode").str() } else { "updates" }));
        // A seeded run records where the level's random stands.
        if self.request.has("seed") {
            result.put("randomState", J::int(level.random.state()));
        }
        let mut settings = J::obj();
        settings.put("redstoneImplementation", J::string("VANILLA"));
        settings.put("allowHeadlessPistons", J::Bool(false));
        settings.put("allowPermanentBlockBreakExploits", J::Bool(false));
        settings.put("allowPistonDuplication", J::Bool(false));
        let mut ranges = J::obj();
        for name in ["animals", "monsters", "raiders", "misc", "water", "villagers", "flying-monsters"] {
            ranges.put(name, J::int(0));
        }
        settings.put("entityActivationRanges", ranges);
        result.put("engineSettings", settings);
        result.put("physicsGuard", J::string("stock"));
        if self.compact {
            result.put("reportEncoding", J::string("rle-v1"));
        }
        self.result = result;
        self.open_report();
        Ok(())
    }

    // McbenchExperiment.importBlockEntities: each saved block entity is read
    // into the one the placement made (Level.getBlockEntity makes it if
    // missing), with the data upgraded by the data fixer from its data version.
    fn import_block_entities(&mut self, level: &mut Level) -> Result<(), Failure> {
        if !self.request.has("block_entities") {
            return Ok(());
        }
        let io = |message: String| format!("java.io.IOException: {message}");
        let reg = level.reg;
        for entry in self.request.req("block_entities").arr().to_vec() {
            let at = position(entry.req("at"), self.y_offset);
            let shown = entry.req("at").to_json();
            let Some(entity) = level.block_entity(at) else { return Err(io(format!("No block entity at {shown}"))) };
            let id = entity.type_key(reg);
            let mut tag = match nbt::parse_compound(entry.req("snbt").str()) {
                Ok(tag) => tag,
                Err(error) => refuse(format!("SNBT of a saved block entity ({error:?})")),
            };
            // The grammar returns one shared compound for any text "{}", and the
            // importer then puts the id and position in it: the Java process's
            // later parses of "{}" (this request's and others') hold them.
            if tag.is_empty() {
                refuse("a saved block entity of the text {} (the grammar's one shared empty compound, which the importer changes)");
            }
            if !tag.contains("id") {
                tag.put_string("id", id);
            }
            let version = entry.req("data_version").int_value();
            if version as i64 > DATA_VERSION {
                return Err(io(format!("Cannot import future block-entity data version {version}")));
            }
            let mut tag = datafix::update_compound(datafix::BLOCK_ENTITY, tag, version);
            if tag.get("id").and_then(|t| t.as_str()) != Some(id) {
                return Err(io(format!("Block entity ID mismatch at {shown}")));
            }
            tag.put_int("x", at.x());
            tag.put_int("y", at.y());
            tag.put_int("z", at.z());
            let entity = level.block_entities.get_mut(&at).expect("the block entity that was just made");
            match blocks::load::load(reg, entity, &tag) {
                Ok(extras) => level.set_block_entity_extras(at, extras),
                Err(fail) => refuse(format!("a saved block entity that Minecraft reports a problem with or this port does not read: {}", fail.0)),
            }
            blocks::loaded(level, at);
            // BlockEntity.setChanged (the entity's own).
            container::block_entity_changed(level, at);
            let mut audit = J::obj();
            audit.put("at", entry.req("at").clone());
            audit.put("sourceDataVersion", J::int(version as i64));
            audit.put("dataVersion", J::int(DATA_VERSION));
            audit.put("snbt", J::string(level.block_entity_saved_with_metadata(at)));
            self.imported_block_entities.push(audit);
        }
        Ok(())
    }

    // McbenchExperiment.importEntities: each saved entity is loaded and added
    // to the level. The server's importer takes boats, rafts, dropped items,
    // experience orbs, item frames, paintings, armor stands and ordinary
    // minecarts only.
    fn import_entities(&mut self, level: &mut Level) -> Result<(), Failure> {
        if !self.request.has("entities") {
            return Ok(());
        }
        let io = |message: String| format!("java.io.IOException: {message}");
        for entry in self.request.req("entities").arr().to_vec() {
            let tag = match nbt::parse_compound(entry.req("snbt").str()) {
                Ok(tag) => tag,
                Err(error) => refuse(format!("SNBT of a saved entity ({error:?})")),
            };
            let version = entry.req("data_version").int_value();
            if version as i64 > DATA_VERSION {
                return Err(io(format!("Cannot import future entity data version {version}")));
            }
            let reference = if tag.contains("Passengers") { datafix::ENTITY_TREE } else { datafix::ENTITY };
            let mut tag = datafix::update_compound(reference, tag, version);
            let id = tag.get("id").and_then(|t| t.as_str()).unwrap_or("").to_string();
            let frame = id == "minecraft:item_frame" || id == "minecraft:glow_item_frame" || id == "minecraft:painting";
            if !frame && !is_minecart(&id) && id != "minecraft:item" && id != "minecraft:experience_orb" && id != "minecraft:armor_stand" && !is_boat(&id) {
                return Err(io(format!("Native entity import currently supports boats, rafts, dropped items, experience orbs, item frames, paintings, armor stands and ordinary container/furnace/TNT minecarts only: {id}")));
            }
            if tag.contains("leash") || tag.contains("Leash") {
                return Err(io("Entity leash relationships require a faithful relationship importer".into()));
            }
            if tag.contains("Passengers") {
                let Some(Tag::List(passengers)) = tag.get("Passengers") else { return Err(io("Invalid passenger list".into())) };
                if id != "minecraft:minecart" || passengers.len() != 1 {
                    return Err(io("Unsupported passengers: only a single original dolphin riding an ordinary minecart is supported".into()));
                }
                refuse("a minecart with a passenger");
            }
            let Some(Tag::List(pos)) = tag.get("Pos") else { return Err(io("Entity needs Pos".into())) };
            if pos.len() != 3 || !tag.contains("UUID") {
                return Err(io("Entity needs three Pos coordinates and its source UUID".into()));
            }
            let Some(Tag::IntArray(uuid)) = tag.get("UUID") else { return Err(io("Entity needs a native UUID int array".into())) };
            if uuid.len() != 4 {
                return Err(io("Entity UUID must contain four ints".into()));
            }
            let source_uuid = entity::load::uuid(&Tag::IntArray(uuid.clone())).expect("a UUID of four ints");
            let at = entry.req("at").arr();
            let mut translated = Vec::new();
            for axis in 0..3 {
                let Some(local) = pos[axis].double_value() else { return Err(io("Invalid entity Pos".into())) };
                if !local.is_finite() || local.to_bits() != at[axis].double_value().to_bits() {
                    return Err(io("Entity Pos does not match declared at".into()));
                }
                translated.push(Tag::double(local + if axis == 1 { self.y_offset as f64 } else { 0.0 }));
            }
            // An item frame's attachment block, moved up with the build.
            let mut anchor = None;
            if frame {
                let Some(Tag::IntArray(block)) = tag.get("block_pos") else { return Err(io("Item frame needs native attachment block_pos".into())) };
                if block.len() != 3 {
                    return Err(io("Invalid frame attachment coordinates".into()));
                }
                let Some(y) = block[1].checked_add(self.y_offset) else { return Err("java.lang.ArithmeticException: integer overflow".into()) };
                anchor = Some(Pos::new(block[0], y, block[2]));
                tag.put("block_pos", Tag::IntArray(vec![block[0], y, block[2]]));
            }
            let translated_pos: Vec<f64> = translated.iter().map(|t| t.double_value().expect("a double")).collect();
            tag.put("Pos", Tag::List(translated));
            let loaded = match entity::load::load_entity(level, &id, &tag) {
                Ok(loaded) => loaded,
                Err(fail) => refuse(format!("a saved entity that Minecraft reports a problem with or this port does not read: {}", fail.0)),
            };
            if frame && entity::frame::attachment(&loaded.entity) != anchor.expect("a frame's anchor") {
                return Err(io("Native frame loader rejected the source attachment position".into()));
            }
            if frame && (0..3).any(|axis| [loaded.entity.pos.x, loaded.entity.pos.y, loaded.entity.pos.z][axis].to_bits() != translated_pos[axis].to_bits()) {
                return Err(io("Frame attachment/facing does not match source Pos".into()));
            }
            if loaded.entity.uuid != source_uuid {
                return Err(io("Native import changed source entity UUID".into()));
            }
            // tryAddFreshEntityWithPassengers: no entity of the level has this UUID.
            if level.entities.all.values().any(|e| e.uuid == source_uuid) {
                return Err(io(format!("Failed native entity insertion (possibly duplicate UUID): {}", uuid_string(source_uuid))));
            }
            // addFreshEntity ignores an entity that was discarded, which then
            // does not save either.
            if loaded.entity.removed {
                return Err(io(format!("Could not audit imported entity {id}")));
            }
            let (entity_id, position) = (loaded.entity.id, loaded.entity.pos);
            entity::add_fresh_entity(level, loaded.entity);
            // onTrackingStart sets the origin the entity lacks.
            let mut origin = loaded.origin;
            if origin.is_none() && level.entities.accessible.ids().contains(&entity_id) {
                origin = Some((position, level.world_uuid));
            }
            // The entity keeps them for the saves after this one.
            let added = level.entities.all.get_mut(&entity_id).expect("the entity that was just added");
            added.origin = origin;
            added.spawn_reason = loaded.spawn_reason;
            let saved = match entity::load::save_entity(level, &level.entities.all[&entity_id], origin, loaded.spawn_reason) {
                Ok(Some(saved)) => saved,
                Ok(None) => return Err(io(format!("Could not audit imported entity {id}"))),
                Err(fail) => refuse(format!("saving an imported entity: {}", fail.0)),
            };
            let mut audit = J::obj();
            audit.put("at", entry.req("at").clone());
            audit.put("sourceDataVersion", J::int(version as i64));
            audit.put("dataVersion", J::int(DATA_VERSION));
            audit.put("snbt", J::string(saved.to_string()));
            self.imported_entities.push(audit);
        }
        Ok(())
    }

    // McbenchExperiment.importScheduledTicks: the saved queues of block and
    // fluid ticks replace the placement's inside their bounds.
    fn import_scheduled_ticks(&self, level: &mut Level) -> Result<(), Failure> {
        if !self.request.has("scheduled_ticks") {
            return Ok(());
        }
        let io = |message: &str| format!("java.io.IOException: {message}");
        if !self.request.has("placementMode") || self.request.req("placementMode").str() != "strict" {
            return Err(io("Saved tick restoration requires frozen strict placement"));
        }
        let saved = self.request.req("scheduled_ticks");
        if saved.req("data_version").int_value() as i64 > DATA_VERSION {
            return Err(io("Future scheduled ticks cannot be downgraded"));
        }
        let dy = self.y_offset;
        let bounds = saved.req("bounds").arr();
        let (lo, hi) = (bounds[0].arr(), bounds[1].arr());
        let min = Pos::new(lo[0].int_value(), lo[1].int_value() + dy, lo[2].int_value());
        let max = Pos::new(hi[0].int_value(), hi[1].int_value() + dy, hi[2].int_value());
        let mut ticks = Vec::new();
        let mut fluids = Vec::new();
        let mut largest_order = -1i64;
        for entry in saved.req("entries").arr() {
            let at = position(entry.req("at"), dy);
            let inside = (min.x()..=max.x()).contains(&at.x()) && (min.y()..=max.y()).contains(&at.y()) && (min.z()..=max.z()).contains(&at.z());
            if !inside {
                return Err(io("Scheduled tick outside saved bounds"));
            }
            let Ok(id) = codec::resource_location(entry.req("id").str()) else { refuse("a saved tick's id that is no resource location (an exception that is no IOException)") };
            let Some(due) = level.game_time.checked_add(entry.req("delay").long_value()) else { return Err("java.lang.ArithmeticException: long overflow".into()) };
            // TickPriority.byValue: the nearest of the seven.
            let priority = entry.req("priority").int_value().clamp(-3, 3);
            let order: i64 = entry.req("sub_tick").str().parse().expect("a sub_tick that is a decimal long");
            if order == i64::MAX {
                return Err(io("Saved tick order leaves no room for subsequent native updates"));
            }
            largest_order = largest_order.max(order);
            match entry.req("kind").str() {
                "block" => {
                    let Some(&block) = level.reg.by_key.get(&id) else { return Err(io(&format!("Unknown saved block tick: {id}"))) };
                    ticks.push(Tick { pos: at, kind: block, trigger: due, priority, sub: order });
                }
                "fluid" => {
                    let Some(fluid) = Fluid::from_key(&id) else { return Err(io(&format!("Unknown saved fluid tick: {id}"))) };
                    fluids.push(Tick { pos: at, kind: fluid, trigger: due, priority, sub: order });
                }
                _ => return Err(io("Unknown saved tick kind")),
            }
        }
        level.restore_ticks(min, max, ticks.clone(), fluids.clone(), largest_order);
        // The queues are asked for each tick restored: a missing one, or one
        // with other timing or order (an earlier tick of the same position and
        // type kept in its place), fails the import.
        for expected in &ticks {
            match level.block_ticks.queued(expected.pos, expected.kind) {
                None => return Err(io("Missing restored block tick")),
                Some(actual) if actual != *expected => return Err(io("Native queue changed saved block tick timing or order")),
                Some(_) => {}
            }
        }
        for expected in &fluids {
            match level.fluid_ticks.queued(expected.pos, expected.kind) {
                None => return Err(io("Missing restored fluid tick")),
                Some(actual) if actual != *expected => return Err(io("Native queue changed saved fluid tick timing or order")),
                Some(_) => {}
            }
        }
        Ok(())
    }

    fn open_report(&mut self) {
        let request = &self.request;
        let report = &mut self.report;
        report.begin_object();
        let J::Obj(head) = &self.result else { unreachable!() };
        for (key, value) in head {
            report.name(key);
            report.value_element(value);
        }
        report.name("worldRevision").value_int(request.req("worldRevision").long_value());
        report.name("probes");
        report.value_element(request.req("probes"));
        if request.has("scheduled_ticks") {
            report.name("importedScheduledTicks");
            report.value_element(request.req("scheduled_ticks"));
        }
        report.name("actions");
        report.value_element(request.req("actions"));
        if !self.imported_entities.is_empty() {
            report.name("importedEntities");
            report.value_element(&J::Arr(self.imported_entities.clone()));
        }
        if !self.imported_block_entities.is_empty() {
            report.name("importedBlockEntities");
            report.value_element(&J::Arr(self.imported_block_entities.clone()));
        }
        if self.compact && request.has("sampleSequence") {
            report.name("sampleSequence");
            report.value_element(request.req("sampleSequence"));
        } else {
            report.name("sampleTicks").begin_array();
            for &sample in &self.samples {
                report.value_int(sample as i64);
            }
            report.end_array();
        }
        report.name("settleTicks").value_int(request.req("settle_ticks").int_value() as i64);
        report.name("cost");
        report.value_element(request.req("cost"));
        report.name("budgets");
        report.value_element(request.req("budgets"));
    }

    // The first tick: settle ticks count from here.
    pub fn begin(&mut self, level: &Level) {
        self.start_time = level.game_time;
        self.running = true;
        self.result.put("sprinting", J::Bool(!self.request.has("sprint") || self.request.req("sprint").bool()));
    }

    fn check_budget(&self) {
        if self.report.len() + BUDGET_MARGIN > self.report_limit as usize {
            refuse("a report within reach of its byte budget (Java's partial write depends on its buffers' flushes)");
        }
    }

    // The end of each server tick. An Err ends the experiment, which then
    // writes its result with the error.
    pub fn end_of_tick(&mut self, level: &mut Level) -> Result<(), Failure> {
        if !self.running {
            return Ok(());
        }
        let elapsed = level.game_time - self.start_time;
        let settle = self.request.req("settle_ticks").int_value() as i64;
        if elapsed < settle && elapsed % 20 == 0 {
            self.progress("settling", elapsed, settle);
        }
        if elapsed < settle {
            return Ok(());
        }
        let tick = i32::try_from(elapsed - settle).expect("java.lang.ArithmeticException: integer overflow");
        if tick as i64 == self.last_tick {
            return Ok(());
        }
        assert!(tick as i64 == self.last_tick + 1, "Missed game tick: {tick}");
        self.last_tick = tick as i64;
        let duration = self.request.req("duration").int_value();
        if tick == 0 {
            self.report.name("initial");
            self.observe(level, 0, true)?;
            self.report.name("observations").begin_array();
        }
        // Requests are sorted once by the host.
        loop {
            let actions = self.request.req("actions").arr();
            if self.action_index >= actions.len() || actions[self.action_index].req("tick").int_value() != tick {
                break;
            }
            let action = actions[self.action_index].clone();
            self.action_index += 1;
            self.interact(level, &action)?;
        }
        if self.sample_index < self.samples.len() && self.samples[self.sample_index] == tick {
            self.observe(level, tick, false)?;
            self.sample_index += 1;
        }
        if tick % 20 == 0 || tick == duration {
            self.progress("running", tick as i64, duration as i64);
        }
        if tick == duration {
            self.finish(level, None);
        }
        Ok(())
    }

    // McbenchExperiment.progress: progress.json, at most every 100 ms.
    fn progress(&mut self, phase: &str, tick: i64, total: i64) {
        let now = Instant::now();
        if tick != total && self.last_progress.map_or(false, |last| now.duration_since(last).as_millis() < 100) {
            return;
        }
        self.last_progress = Some(now);
        let mut status = J::obj();
        status.put("phase", J::string(phase));
        status.put("tick", J::int(tick));
        status.put("totalTicks", J::int(total));
        status.put("observations", J::int(self.observation_count));
        status.put("reportBytes", J::int(self.report.len() as i64));
        write_json(&self.folder, "progress", &status);
    }

    // McbenchExperiment.observe.
    fn observe(&mut self, level: &mut Level, tick: i32, initial_observation: bool) -> Result<(), Failure> {
        let started = Instant::now();
        let count = self.probes.len();
        let mut states: Vec<State> = Vec::with_capacity(count);
        let mut signals = vec![0i32; count];
        // The inventories read, by probe: none are made where no probe is an inventory probe.
        let any_inventory = self.inventory_probes.iter().any(|&probe| probe);
        let mut inventories: Vec<Option<J>> = if any_inventory { vec![None; count] } else { Vec::new() };
        for i in 0..count {
            let pos = self.probes[i];
            let state = level.get(pos);
            self.name(level, state);
            let signal = block_power(level, pos);
            signals[i] = signal;
            if self.inventory_probes[i] {
                inventories[i] = self.read_inventory(level, pos)?;
            }
            let initial = self.previous_blocks[i].is_none();
            let changed = !initial && (self.previous_blocks[i] != Some(state) || signal != self.previous_signals[i] || (self.inventory_probes[i] && inventories[i] != self.previous_inventories[i]));
            if changed {
                self.changed_probes[i] = true;
                self.change_count += 1;
            }
            // An entry is longer than MIN_PREVIEW_ENTRY bytes: once the preview
            // has less room than that none is made.
            if ((initial && i < 32) || changed) && self.preview_bytes + MIN_PREVIEW_ENTRY <= PREVIEW_BYTES {
                let mut entry = J::obj();
                entry.put("probe", J::int(i as i64));
                if initial {
                    entry.put("at", self.request.req("probes").arr()[i].clone());
                } else {
                    entry.put("tick", J::int(tick as i64));
                }
                entry.put("block", J::string(self.names[state as usize].as_deref().expect("a state's name")));
                entry.put("signal", J::int(signal as i64));
                if self.inventory_probes[i] {
                    if let Some(items) = &inventories[i] {
                        entry.put("inventory", items.clone());
                    }
                }
                self.scratch.clear();
                super::gson::write_element(&mut self.scratch, &entry, true);
                let bytes = self.scratch.len();
                // One bounded preview, in observation order. Full report is lossless.
                if self.preview_bytes + bytes <= PREVIEW_BYTES {
                    if initial {
                        self.preview_initial.push(entry);
                    } else {
                        self.preview_changes.push(entry);
                    }
                    self.preview_bytes += bytes;
                }
            }
            self.previous_blocks[i] = Some(state);
            self.previous_signals[i] = signal;
            if self.inventory_probes[i] {
                self.previous_inventories[i] = inventories[i].clone();
            }
            self.observation_count += 1;
            states.push(state);
        }
        let item_entities = if self.observe_item_entities { Some(self.read_item_entities(level)) } else { None };
        let world_inventories = if self.observe_world_inventories { Some(self.read_world_inventories(level)) } else { None };
        let living_entities = if self.observe_living_entities { Some(self.read_living_entities(level)) } else { None };
        if !self.compact || initial_observation {
            self.write_observation(tick, tick, &states, &signals, &inventories, &item_entities, &world_inventories, &living_entities, false);
        } else if self.pending.as_ref().map_or(false, |p| {
            tick == p.end + 1 && states == p.blocks && signals == p.signals && inventories == p.inventories && item_entities == p.item_entities && world_inventories == p.world_inventories && living_entities == p.living_entities
        }) {
            self.pending.as_mut().unwrap().end = tick;
        } else {
            self.flush_pending();
            self.pending = Some(Pending { start: tick, end: tick, blocks: states, signals, inventories, item_entities, world_inventories, living_entities });
        }
        self.check_budget();
        let elapsed = started.elapsed().as_nanos();
        self.probe_nanos += elapsed;
        self.max_probe_nanos = self.max_probe_nanos.max(elapsed);
        Ok(())
    }

    // Makes sure a state's block string is in `names`. Two states have two
    // strings, so states compare as their strings do.
    fn name(&mut self, level: &Level, state: u16) {
        if self.names.len() <= state as usize {
            self.names.resize(level.reg.states.len(), None);
        }
        if self.names[state as usize].is_none() {
            self.names[state as usize] = Some(level.reg.describe(state));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn write_observation(&mut self, tick: i32, end_tick: i32, states: &[State], signals: &[i32], inventories: &[Option<J>], item_entities: &Option<J>, world_inventories: &Option<J>, living_entities: &Option<J>, range: bool) {
        let report = &mut self.report;
        let names = &self.names;
        report.begin_object().name("tick").value_int(tick as i64);
        if range {
            report.name("endTick").value_int(end_tick as i64);
        }
        report.name("blocks").begin_array();
        for i in 0..states.len() {
            report.begin_object().name("block").value_str(names[states[i] as usize].as_deref().expect("a state's name")).name("signal").value_int(signals[i] as i64);
            if self.inventory_probes[i] {
                report.name("inventory");
                match &inventories[i] {
                    None => report.value_null(),
                    Some(items) => report.value_element(items),
                };
            }
            report.end_object();
        }
        report.end_array();
        for (name, value) in [("item_entities", item_entities), ("world_inventories", world_inventories), ("living_entities", living_entities)] {
            if let Some(value) = value {
                report.name(name);
                report.value_element(value);
            }
        }
        report.end_object();
    }

    fn flush_pending(&mut self) {
        let Some(p) = self.pending.take() else { return };
        self.write_observation(p.start, p.end, &p.blocks, &p.signals, &p.inventories, &p.item_entities, &p.world_inventories, &p.living_entities, true);
    }

    // McbenchExperiment.readInventory: the items of a probed container, or
    // none for a block that holds no inventory (reported, or an error).
    fn read_inventory(&mut self, level: &mut Level, at: Pos) -> Result<Option<J>, Failure> {
        match container_items(level, at) {
            Some(items) => Ok(Some(J::Arr(items))),
            None if self.report_missing_inventories => Ok(None),
            None => Err(format!("java.io.IOException: Inventory probe is not a container: BlockPos{{x={}, y={}, z={}}}", at.x(), at.y(), at.z())),
        }
    }

    // Item entities in the order of their UUIDs.
    fn read_item_entities(&self, level: &Level) -> J {
        let mut items: Vec<(String, J)> = Vec::new();
        for &id in level.entities.accessible.ids() {
            let e = &level.entities.all[&id];
            // EntityLookup/world.getEntitiesByClass(Item.class): item entities only.
            let crate::entity::Kind::Item(data) = &e.kind else { continue };
            let uuid = uuid_string(e.uuid);
            let mut value = J::obj();
            value.put("uuid", J::string(uuid.as_str()));
            value.put("at", J::Arr(vec![J::double(e.pos.x), J::double(e.pos.y - self.y_offset as f64), J::double(e.pos.z)]));
            describe_stack(level, &data.stack, &mut value);
            add_contained_items(level, &data.stack, &mut value, 0);
            items.push((uuid, value));
        }
        items.sort_by(|a, b| a.0.cmp(&b.0));
        J::Arr(items.into_iter().map(|(_, v)| v).collect())
    }

    // Living entities (the armor stands that are alive) in the order of their UUIDs:
    // the world's getLivingEntities holds the valid ones.
    fn read_living_entities(&self, level: &Level) -> J {
        let mut values: Vec<(String, J)> = Vec::new();
        for &id in level.entities.accessible.ids() {
            let e = &level.entities.all[&id];
            if let crate::entity::Kind::Golem(_) = e.kind {
                crate::entity::golem::unported(e, "is observed");
            }
            let crate::entity::Kind::ArmorStand(stand) = &e.kind else { continue };
            if !crate::entity::armor_stand::is_alive(e) {
                continue;
            }
            let uuid = uuid_string(e.uuid);
            let mut value = J::obj();
            value.put("uuid", J::string(uuid.as_str()));
            value.put("type", J::string("minecraft:armor_stand"));
            value.put("at", J::Arr(vec![J::double(e.pos.x), J::double(e.pos.y - self.y_offset as f64), J::double(e.pos.z)]));
            value.put("health", J::double(stand.health as f64));
            value.put("max_health", J::double(20.0));
            value.put("dead", J::Bool(false));
            values.push((uuid, value));
        }
        values.sort_by(|a, b| a.0.cmp(&b.0));
        J::Arr(values.into_iter().map(|(_, v)| v).collect())
    }

    // The loaded chunks' block entities that hold items, in chunk then
    // position order. The entities this port has hold none of their own.
    fn read_world_inventories(&self, level: &mut Level) -> J {
        // Block entities of the loaded chunks, chunk by chunk in (x, z) order
        // and by position within a chunk.
        let mut positions: Vec<(i32, i32, i64, Pos)> = level
            .block_entities
            .keys()
            .filter(|pos| level.world.has_chunk(pos.x() >> 4, pos.z() >> 4))
            .map(|&pos| (pos.x() >> 4, pos.z() >> 4, pos.as_long(), pos))
            .collect();
        positions.sort_by_key(|&(x, z, long, _)| (x, z, long));
        let mut values = Vec::new();
        for (_, _, _, pos) in positions {
            let items = match &level.block_entities[&pos] {
                BlockEntity::Container(c) if c.is_container() => encode_items(level, &c.items),
                BlockEntity::Entity(e) if e.container_size().is_some() => encode_items(level, e.items().map(|items| items.as_slice()).unwrap_or(&[])),
                // The lectern holds its book, which is observed when it is not empty.
                BlockEntity::Entity(blocks::entities::Entity::Lectern(lectern)) => encode_items(level, std::slice::from_ref(&lectern.book)),
                // The campfire's four slots, empty here.
                BlockEntity::Special(blocks::special::Special::Campfire) => Vec::new(),
                _ => continue,
            };
            let state = level.world.get(pos);
            let mut value = J::obj();
            value.put("kind", J::string("block"));
            value.put("type", J::string(level.reg.blocks[level.block(state) as usize].key.as_str()));
            value.put("at", J::Arr(vec![J::int(pos.x() as i64), J::int((pos.y() - self.y_offset) as i64), J::int(pos.z() as i64)]));
            value.put("inventory", J::Arr(items));
            values.push(value);
        }
        // The entities, by UUID string: an item frame holds its item (and is
        // observed with none), a container entity its stacks, and a minecart or a
        // boat is observed with the item that picks it (vehicle_item).
        let mut listed: Vec<(String, entity::EntityId)> = level.entities.all.values().filter(|e| matches!(e.kind, entity::Kind::Frame(_) | entity::Kind::Minecart(_) | entity::Kind::Boat(_))).map(|e| (uuid_string(e.uuid), e.id)).collect();
        listed.sort();
        for (uuid, id) in listed {
            let e = &level.entities.all[&id];
            let (type_key, items, vehicle_item) = match &e.kind {
                entity::Kind::Frame(data) => (
                    if data.glow { "minecraft:glow_item_frame" } else { "minecraft:item_frame" },
                    if data.item.count > 0 { vec![encode_item(level, &data.item, 0, 0)] } else { Vec::new() },
                    None,
                ),
                entity::Kind::Minecart(cart) => {
                    let items = if cart.is_container() { encode_items(level, &level.entities.containers[&id]) } else { Vec::new() };
                    let pick = ItemStack::new(level.reg.item_by_key[cart.class.key()], 1);
                    (cart.class.key(), items, Some(encode_item(level, &pick, 0, 0)))
                }
                entity::Kind::Boat(boat) => {
                    let items = if boat.chest { encode_items(level, &level.entities.containers[&id]) } else { Vec::new() };
                    let pick = ItemStack::new(level.reg.item_by_key[boat.key.as_str()], 1);
                    (boat.key.as_str(), items, Some(encode_item(level, &pick, 0, 0)))
                }
                _ => unreachable!(),
            };
            let mut value = J::obj();
            value.put("kind", J::string("entity"));
            value.put("uuid", J::string(uuid));
            value.put("type", J::string(type_key));
            if let Some(pick) = vehicle_item {
                value.put("vehicle_item", pick);
            }
            value.put("at", J::Arr(vec![J::double(e.pos.x), J::double(e.pos.y - self.y_offset as f64), J::double(e.pos.z)]));
            value.put("inventory", J::Arr(items));
            values.push(value);
        }
        J::Arr(values)
    }

    // McbenchExperiment.inputPlayer: the detached input player, made by the
    // first input that needs it and kept for the experiment.
    fn input_player(&mut self, level: &mut Level) -> blocks::InputPlayer {
        *self.input_player.get_or_insert_with(|| blocks::InputPlayer::make(level))
    }

    // McbenchExperiment.interact.
    fn interact(&mut self, level: &mut Level, input: &J) -> Result<(), Failure> {
        let mut outcome = input.clone();
        let pos = position(input.req("at"), self.y_offset);
        let state = level.get(pos);
        let kind = input.req("type").str().to_string();
        let class = level.reg.blocks[level.block(state) as usize].class.clone();
        let applied: Result<(), String> = if kind == "drop" {
            self.drop_item(level, input, &mut outcome)
        } else if kind == "book" {
            match level.block_entity(pos) {
                Some(BlockEntity::Entity(blocks::entities::Entity::Lectern(_))) => self.book_input(level, pos, state, input),
                _ => Err("Book input requires a lectern".into()),
            }
        } else if kind == "page" {
            let reg = level.reg;
            match level.block_entity(pos) {
                Some(BlockEntity::Entity(blocks::entities::Entity::Lectern(lectern))) if blocks::lectern::has_book_content(reg, lectern) => {
                    let pages = blocks::load::page_count(reg, &lectern.book.borrow());
                    let page = input.req("page").int_value();
                    if page < 0 || page >= pages {
                        Err("Page is outside this book; nothing changed".into())
                    } else {
                        self.input_player(level);
                        // The lectern's menu (nothing of the world changes as it
                        // opens) and its page-jump button: LecternMenu.clickMenuButton
                        // sets the lectern's page through its container data.
                        let previous = lectern_page(level, pos);
                        blocks::lectern::set_page(level, pos, page);
                        if lectern_page(level, pos) != page {
                            Err("Native page selection failed".into())
                        } else {
                            outcome.put("previousPage", J::int(previous as i64));
                            Ok(())
                        }
                    }
                }
                _ => Err("Page input requires a lectern with a book".into()),
            }
        } else if kind == "use" && blocks::takes_use(&class) {
            // A fence gate opens away from the player, so its use needs the
            // yaw; no other usable block reads it.
            if !input.has("yaw") && class == "FenceGateBlock" {
                Err("Using a fence gate requires yaw, the direction the player faces (it opens away from the player)".into())
            } else {
                let mut player = self.input_player(level);
                if input.has("yaw") {
                    // JsonElement.getAsFloat: Float.parseFloat of the number's text.
                    let J::Num(text) = input.req("yaw") else { panic!("yaw is not a number") };
                    player.yaw = text.parse().unwrap_or_else(|_| panic!("bad yaw {text}"));
                    self.input_player = Some(player);
                }
                if blocks::use_without_item(level, state, pos, player) {
                    Ok(())
                } else {
                    Err(format!("Use does nothing to {}", level.reg.describe(level.get(pos))))
                }
            }
        } else if kind == "press" && class == "ButtonBlock" {
            if level.reg.get_bool(state, level.reg.property_of(state, "powered")) {
                Err("Button is already pressed".into())
            } else {
                assert!(blocks::input(level, "press", state, pos, 0.0));
                Ok(())
            }
        } else if kind == "insert" || kind == "extract" {
            inventory_input(level, pos, input)
        } else if kind == "drain" {
            match drain_inventory(level, pos) {
                Ok(removed) => {
                    outcome.put("removed", J::Arr(removed));
                    Ok(())
                }
                Err(message) => Err(message),
            }
        } else if kind == "toggle" && class == "LeverBlock" {
            assert!(blocks::input(level, "toggle", state, pos, 0.0));
            Ok(())
        } else {
            let wanted = if kind == "press" {
                "a button"
            } else if kind == "use" {
                "a door, trapdoor, fence gate, repeater, comparator, note block, daylight detector, redstone dust or decorated pot"
            } else {
                "a lever"
            };
            let found = level.get(pos);
            Err(format!("Input requires {wanted}; found {}", level.reg.describe(found)))
        };
        match applied {
            Ok(()) => outcome.put("applied", J::Bool(true)),
            Err(message) => {
                outcome.put("applied", J::Bool(false));
                outcome.put("error", J::string(message));
            }
        }
        if outcome.req("applied") == &J::Bool(false) {
            self.failed_inputs += 1;
            if self.input_failures.len() < 8 {
                self.input_failures.push(outcome.clone());
            }
        }
        self.inputs.push(outcome);
        Ok(())
    }

    // The "book" input on a lectern (McbenchExperiment.interact): a book
    // supplied (its stack) or the one an earlier input took is placed by the
    // input player (LecternBlock.tryPlaceBook), or the lectern's book is
    // taken through its menu into the player's inventory and held for the
    // lectern.
    fn book_input(&mut self, level: &mut Level, pos: Pos, state: State, input: &J) -> Result<(), String> {
        let player = self.input_player(level);
        let present = input.req("present").bool();
        let supplied = input.has("stack");
        let has_book = lectern_has_book(level, pos);
        if supplied && (!present || has_book || self.held_lectern_books.contains_key(&pos)) {
            return Err("Book supply requires an empty lectern with no previously removed book".into());
        }
        if present && !has_book {
            let mut carried = if supplied { supplied_stack(level, input)? } else { self.held_lectern_books.get(&pos).cloned().unwrap_or_else(ItemStack::empty) };
            if carried.item == 0 || carried.count <= 0 {
                return Err("No previously removed source book is available for this lectern".into());
            }
            let reg = level.reg;
            if carried.count != 1 || (carried.item != reg.item_by_key["minecraft:written_book"] && carried.item != reg.item_by_key["minecraft:writable_book"]) {
                return Err("Lectern supply requires exactly one native book".into());
            }
            let placed = blocks::lectern::try_place_book(level, player, pos, state, &mut carried);
            // A held book is the stack the lectern took from: what it took
            // leaves it there too.
            if !supplied {
                self.held_lectern_books.insert(pos, carried.clone());
            }
            if !placed || !lectern_has_book(level, pos) {
                return Err("Native book replacement failed".into());
            }
            if carried.item != 0 && carried.count > 0 {
                return Err("Native book replacement did not consume the carried book".into());
            }
            self.held_lectern_books.remove(&pos);
        } else if !present && has_book {
            if self.held_lectern_books.contains_key(&pos) {
                return Err("A source book is already held for this lectern".into());
            }
            if !self.input_inventory.is_empty() {
                return Err("Input actor inventory is not empty".into());
            }
            let original = match level.block_entities.get(&pos) {
                Some(BlockEntity::Entity(blocks::entities::Entity::Lectern(lectern))) => lectern.book.borrow().clone(),
                _ => unreachable!("the lectern's block entity was checked"),
            };
            // The lectern's menu, and its take-book button (the input player
            // may build): the book goes into the player's inventory, which
            // has room for it.
            let mut book = blocks::lectern::take_book(level, pos);
            assert!(self.input_inventory.add(level.reg, &mut book), "an empty inventory took nothing of the lectern's book");
            if lectern_has_book(level, pos) {
                return Err("Native book removal failed".into());
            }
            let mut slot = None;
            for (i, item) in self.input_inventory.items().iter().enumerate() {
                if item.item == 0 || item.count <= 0 {
                    continue;
                }
                // ItemStack.matches: the count, the item and its components.
                if slot.is_some() || !(original.count == item.count && blocks::container::same_item_same_components(&original, item)) {
                    return Err("Native removed book does not match the source book".into());
                }
                slot = Some(i);
            }
            let Some(slot) = slot else { return Err("Native source book was not transferred to the input actor".into()) };
            let held = self.input_inventory.remove_item_no_update(slot);
            self.held_lectern_books.insert(pos, held);
        }
        Ok(())
    }

    // The "drop" input: an item entity of the supplied stack with the velocity
    // given (suppliedStack and the ItemEntity constructor).
    fn drop_item(&mut self, level: &mut Level, input: &J, outcome: &mut J) -> Result<(), String> {
        let stack = supplied_stack(level, input)?;
        let at = input.req("at").arr();
        let velocity = input.req("velocity").arr();
        let uuid = crate::entity::item::supply_item(
            level,
            at[0].double_value(),
            at[1].double_value() + self.y_offset as f64,
            at[2].double_value(),
            stack,
            Vec3::new(velocity[0].double_value(), velocity[1].double_value(), velocity[2].double_value()),
        );
        outcome.put("entityUUID", J::string(uuid_string(uuid)));
        Ok(())
    }

    // McbenchExperiment.finish.
    pub fn finish(&mut self, level: &mut Level, error: Option<Failure>) {
        self.running = false;
        // McbenchEngine.stopped: what the ticks changed is lit, and a seeded
        // run's columns are lit again from their blocks; it then adds the
        // saved chunks of its columns (chunks).
        level.flush_light();
        if self.request.has("seed") {
            let columns: Vec<(i32, i32)> = self.request.req("columns").arr().iter().map(|column| (column.arr()[0].int_value(), column.arr()[1].int_value())).collect();
            let relit = level.relight(&columns);
            if relit != columns.len() {
                panic!("java.lang.IllegalStateException: Relit {relit} of {} columns", columns.len());
            }
            let mut chunks = J::obj();
            for column in self.request.req("columns").arr() {
                let column = column.arr();
                let (x, z) = (column[0].int_value(), column[1].int_value());
                chunks.put(&format!("{x},{z}"), J::string(level.chunk_snbt(x, z)));
            }
            self.result.put("chunks", chunks);
        }
        let mut error = error;
        self.result.put("elapsedTicks", J::int(self.last_tick));
        self.result.put("probeCount", J::int(self.probes.len() as i64));
        self.result.put("sampleCount", J::int(self.sample_index as i64));
        self.result.put("inputCount", J::int(self.inputs.len() as i64));
        self.result.put("failedInputs", J::int(self.failed_inputs));
        self.result.put("observationCount", J::int(self.observation_count));
        let mut metrics = J::obj();
        metrics.put("probeReadMs", J::double(self.probe_nanos as f64 / 1e6));
        metrics.put("maxSampleReadMs", J::double(self.max_probe_nanos as f64 / 1e6));
        // This engine has no Java heap to read.
        metrics.put("maxObservedHeapBytes", J::int(0));
        metrics.put("maxHeapBytes", J::int(0));
        self.result.put("metrics", metrics);
        let mut preview = J::obj();
        preview.put("initial", J::Arr(self.preview_initial.clone()));
        preview.put("changes", J::Arr(self.preview_changes.clone()));
        preview.put("inputFailures", J::Arr(self.input_failures.clone()));
        preview.put("changeCount", J::int(self.change_count));
        preview.put("changedProbeCount", J::int(self.changed_probes.iter().filter(|c| **c).count() as i64));
        preview.put(
            "truncated",
            J::Bool((self.preview_initial.len() as i64) < self.probes.len() as i64 || (self.preview_changes.len() as i64) < self.change_count || (self.input_failures.len() as i64) < self.failed_inputs),
        );
        preview.put(
            "note",
            J::string("Probe indexes are zero-based. Changes compare adjacent requested samples, starting with initial; unsampled transitions are not counted. signal is Bukkit getBlockPower (received neighbor power), not a component's own power/powered/lit state; read those in block."),
        );
        self.result.put("preview", preview);
        if error.is_none() {
            self.flush_pending();
            self.report.end_array().name("inputs");
            self.report.value_element(&J::Arr(self.inputs.clone()));
            self.report.name("ok").value_bool(true);
            let J::Obj(entries) = &self.result else { unreachable!() };
            for (key, value) in entries {
                if key != "engine" && key != "minecraftVersion" && key != "reportEncoding" {
                    self.report.name(key);
                    self.report.value_element(value);
                }
            }
            self.report.end_object();
            self.check_budget();
        }
        let temporary = self.folder.join("report.tmp");
        std::fs::write(&temporary, &self.report.out).expect("write the report");
        if error.is_none() {
            std::fs::rename(&temporary, self.folder.join("report.json")).expect("move the report");
        }
        self.result.put("ok", J::Bool(error.is_none()));
        self.result.put("reportBytes", J::int(self.report.len() as i64));
        if let Some(error) = error.take() {
            self.result.put("error", J::string(error));
        }
        write_json(&self.folder, "result", &self.result);
    }
}

// McbenchExperiment.encodeInventoryItem for each non-empty slot.
fn encode_items(level: &Level, items: &[Stack]) -> Vec<J> {
    let mut values = Vec::new();
    for (slot, stack) in items.iter().enumerate() {
        let stack = stack.borrow().clone();
        if stack.count <= 0 {
            continue;
        }
        values.push(encode_item(level, &stack, slot, 0));
    }
    values
}

// A stack's id, count, whether it has no component patch, and its SNBT as
// ItemStack.CODEC writes it.
fn describe_stack(level: &Level, stack: &ItemStack, value: &mut J) {
    value.put("id", J::string(level.reg.items[stack.item as usize].key.as_str()));
    value.put("count", J::int(stack.count as i64));
    value.put("plain", J::Bool(stack.patch.is_empty()));
    value.put("snbt", J::string(stack_snbt(level, stack)));
}

// McbenchExperiment.encodeInventoryItem: the slot, the stack, and the items it
// holds (containedItems).
fn encode_item(level: &Level, stack: &ItemStack, slot: usize, depth: usize) -> J {
    if depth > 64 {
        refuse("an inventory nested deeper than the observation allows");
    }
    let mut value = J::obj();
    value.put("slot", J::int(slot as i64));
    describe_stack(level, stack, &mut value);
    add_contained_items(level, stack, &mut value, depth);
    value
}

// McbenchExperiment.addContainedItems: the non-empty items of the stack's
// container, bundle contents and charged projectiles.
fn add_contained_items(level: &Level, stack: &ItemStack, value: &mut J, depth: usize) {
    let contained = stack.contained(level.reg).unwrap_or_else(|fail| refuse(format!("the items a stack holds: {}", fail.0)));
    let mut children: Vec<J> = Vec::new();
    for child in contained.iter().filter(|c| c.count > 0) {
        let at = children.len();
        children.push(encode_item(level, child, at, depth + 1));
    }
    if !children.is_empty() {
        value.put("containedItems", J::Arr(children));
    }
}

// The slots' items of the container block entity at a position
// (Level.getBlockEntity makes it if missing), or none if there is no
// Container there.
fn container_items(level: &mut Level, at: Pos) -> Option<Vec<J>> {
    match level.block_entity(at)? {
        BlockEntity::Container(c) if c.is_container() => {
            let items = c.items.clone();
            Some(encode_items(level, &items))
        }
        BlockEntity::Entity(e) if e.container_size().is_some() => {
            let items: Vec<Stack> = e.items().map(|items| items.to_vec()).unwrap_or_default();
            Some(encode_items(level, &items))
        }
        _ => None,
    }
}

// The block entity at a position as a container block entity, made if
// missing: none if it is no Container.
fn container_at(level: &mut Level, at: Pos) -> Option<()> {
    match level.block_entity(at)? {
        BlockEntity::Container(c) if c.is_container() => Some(()),
        BlockEntity::Entity(e) if e.container_size().is_some() => Some(()),
        _ => None,
    }
}

// McbenchExperiment.inventoryInput: an insert or extract by the slot.
fn inventory_input(level: &mut Level, at: Pos, input: &J) -> Result<(), String> {
    if container_at(level, at).is_none() {
        return Err("Input is not a container".into());
    }
    let (slot, count) = (input.req("slot").int_value(), input.req("count").int_value());
    let mut container = Cont::Be(at);
    if slot < 0 || slot as usize >= container.size(level) {
        return Err("Invalid inventory slot".into());
    }
    let slot = slot as usize;
    let old = container.get(level, slot);
    if input.req("type").str() == "extract" {
        if container::count(&old) < count {
            return Err("Insufficient items; nothing extracted".into());
        }
        container.remove_item(level, slot, count);
    } else {
        let item = container::stack_of(supplied_stack(level, input)?);
        if !container::is_empty(&old) && !container::same_item(&old, &item) {
            return Err("Slot contains a different item; nothing inserted".into());
        }
        // Math.min(item.getMaxStackSize(), container.getMaxStackSize(item)).
        let max = container.max_stack_for(level, &item);
        if container::count(&old) + count > max || !container.can_place_item(level, slot, &item) {
            return Err("Items do not fit; nothing inserted".into());
        }
        container::set_count(&item, container::count(&old) + count);
        container.set_item(level, slot, item);
    }
    container.set_changed(level);
    Ok(())
}

// McbenchExperiment.suppliedStack: the stack an input names, with the saved
// data (of the server's own data version) it supplies, checked against the
// item and count it names. An Err is the message of the IOException the
// server throws (an input that fails).
fn supplied_stack(level: &Level, input: &J) -> Result<ItemStack, String> {
    let Ok(key) = codec::resource_location(input.req("item").str()) else { refuse("an item key that is no resource location (an exception that no input handles)") };
    let item = match level.reg.item_by_key.get(&key) {
        Some(&item) if key != "minecraft:air" => item,
        _ => return Err("Unknown or empty item".into()),
    };
    let count = input.req("count").int_value();
    let mut stack = ItemStack::new(item, count);
    if input.has("stack") {
        let saved = input.req("stack");
        let version = saved.req("data_version").int_value();
        let invalid = |message: String| format!("Invalid supplied stack: {message}");
        if version as i64 > DATA_VERSION {
            return Err(invalid(format!("Cannot supply future item data version {version}")));
        }
        let tag = match nbt::parse_compound(saved.req("snbt").str()) {
            Ok(tag) => tag,
            Err(error) => refuse(format!("SNBT of a supplied stack ({error:?})")),
        };
        let fixed = datafix::update(datafix::ITEM_STACK, nbt::Tag::Compound(tag), version);
        stack = match components::decode_stack(level.reg, &fixed) {
            Ok(stack) => stack,
            Err(fail) => refuse(format!("a supplied stack that Minecraft's codec rejects or this port does not read: {}", fail.0)),
        };
        if stack.count <= 0 || stack.item != item || stack.count != count {
            return Err(invalid("Supplied stack ID/count does not match input".into()));
        }
    }
    if count < 1 || count > stack.max_stack_size(level.reg) {
        return Err("Supplied item count exceeds native stack size".into());
    }
    Ok(stack)
}

// McbenchExperiment.drainInventory: every stack out through removeItem,
// each encoded as it was before.
fn drain_inventory(level: &mut Level, at: Pos) -> Result<Vec<J>, String> {
    if container_at(level, at).is_none() {
        return Err("Input is not a container".into());
    }
    let mut container = Cont::Be(at);
    let mut removed = Vec::new();
    for slot in 0..container.size(level) {
        let item = container.get(level, slot);
        if container::is_empty(&item) {
            continue;
        }
        let stack = item.borrow().clone();
        removed.push(encode_item(level, &stack, slot, 0));
        container.remove_item(level, slot, stack.count);
    }
    container.set_changed(level);
    Ok(removed)
}

// id.matches("minecraft:(chest_|hopper_|furnace_)?minecart").
fn is_minecart(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("minecraft:") else { return false };
    let rest = ["chest_", "hopper_", "furnace_", "tnt_"].iter().find_map(|prefix| rest.strip_prefix(prefix)).unwrap_or(rest);
    rest == "minecart"
}

// id.matches("minecraft:[a-z_]+_(boat|raft)").
fn is_boat(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("minecraft:") else { return false };
    ["_boat", "_raft"].iter().any(|suffix| rest.strip_suffix(suffix).is_some_and(|name| !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '_')))
}

// BlockPos from a request's position, with the build's height offset.
pub fn position(p: &J, y_offset: i32) -> Pos {
    let p = p.arr();
    Pos::new(p[0].int_value(), p[1].int_value() + y_offset, p[2].int_value())
}

// UUID.toString.
pub fn uuid_string((hi, lo): (i64, i64)) -> String {
    let (hi, lo) = (hi as u64, lo as u64);
    format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}", hi >> 32, (hi >> 16) & 0xffff, hi & 0xffff, lo >> 48, lo & 0xffff_ffff_ffff)
}

// An item stack as ItemStack.CODEC writes it to NBT, as text.
fn stack_snbt(level: &Level, stack: &ItemStack) -> String {
    match components::encode_stack(level.reg, stack) {
        Ok(tag) => tag.to_string(),
        Err(fail) => refuse(format!("a stack that does not encode: {}", fail.0)),
    }
}

// CraftBlock.getBlockPower(): the power that the six neighbours give.
fn block_power(level: &mut Level, pos: Pos) -> i32 {
    let mut power = 0;
    for dir in [Dir::Down, Dir::Up, Dir::East, Dir::West, Dir::North, Dir::South] {
        let neighbour = pos.relative(dir);
        if level.has_signal(neighbour, dir) {
            let state = level.get(neighbour);
            if level.block(state) == level.known.redstone_wire {
                power = power.max(level.reg.get_int(state, level.reg.property_of(state, "power")));
            }
        }
    }
    if power > 0 {
        power
    } else if level.has_neighbor_signal(pos) {
        15
    } else {
        0
    }
}

// McbenchExperiment.writeJson.
fn write_json(folder: &Path, name: &str, value: &J) {
    let temporary = folder.join(format!("{name}.tmp"));
    std::fs::write(&temporary, value.to_json()).expect("write a file");
    std::fs::rename(&temporary, folder.join(format!("{name}.json"))).expect("move a file");
}

// LecternBlockEntity.hasBook of the lectern at a position.
fn lectern_has_book(level: &Level, pos: Pos) -> bool {
    match level.block_entities.get(&pos) {
        Some(BlockEntity::Entity(blocks::entities::Entity::Lectern(lectern))) => blocks::lectern::has_book_content(level.reg, lectern),
        _ => panic!("no lectern at {pos}"),
    }
}

// LecternBlockEntity.getPage of the lectern at the position.
fn lectern_page(level: &mut Level, pos: Pos) -> i32 {
    match level.block_entity(pos) {
        Some(BlockEntity::Entity(blocks::entities::Entity::Lectern(lectern))) => lectern.page,
        _ => panic!("no lectern at {pos}"),
    }
}
