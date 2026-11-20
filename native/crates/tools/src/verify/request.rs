//! A `test_build` call's experiments, checked as `minecraft-verify.js`
//! checks them (`callTests`, `validateRequest`, `verification-contract.js`
//! `entityCoordinates` and `validateEntities`), with today's error
//! sentences, and the request the engine reads (`Experiments.request` and
//! `transport`), written as `JSON.stringify` writes it.

use std::collections::HashSet;

use indexmap::IndexSet;
use mcbench_world::World;
use mcbench_world::dimension::{MAX_MC_Y, MIN_MC_Y, validate_world_height};
use serde_json::{Map, Value, json};

use super::budgets::Limits;
use super::error::VerifyError;
use super::js;

/// `DEFAULT_SAMPLES`.
pub const DEFAULT_SAMPLES: [i64; 3] = [0, 20, 100];
/// `DEFAULT_SETTLE_TICKS`.
pub const DEFAULT_SETTLE_TICKS: i64 = 40;
/// `Number.MAX_SAFE_INTEGER`, the open end of a list's length.
const MAX_SAFE: f64 = 9_007_199_254_740_991.0;

/// The keys a test may hold, in `validateRequest`'s order.
const TEST_KEYS: [&str; 13] = [
    "probes",
    "actions",
    "samples",
    "settle_ticks",
    "block_entities",
    "entities",
    "scheduled_ticks",
    "inventory_probes",
    "inventory_missing",
    "item_entities",
    "world_inventories",
    "living_entities",
    "block_data_version",
];

/// The action types (`validateRequest`'s list).
const ACTION_TYPES: [&str; 9] = [
    "press", "toggle", "insert", "extract", "drain", "use", "drop", "page", "book",
];

/// How a verifier places a test's world (`placementMode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// `updates`: blocks placed with their neighbours' updates (the app's).
    Updates,
    /// `strict`: placed as written, for saved schematics and their ticks.
    Strict,
}

impl Placement {
    /// The mode's name.
    pub fn name(self) -> &'static str {
        match self {
            Placement::Updates => "updates",
            Placement::Strict => "strict",
        }
    }

    /// The mode named `name`, or today's error.
    pub fn parse(name: &str) -> Result<Placement, String> {
        match name {
            "updates" => Ok(Placement::Updates),
            "strict" => Ok(Placement::Strict),
            _ => Err("Unsupported placement mode".into()),
        }
    }
}

/// How the engine writes a report (`reportEncoding`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// `full`: every observation (the app's).
    Full,
    /// `rle-v1`: observations as changes, for the benchmark's graders.
    RleV1,
}

impl Encoding {
    /// The encoding's name.
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Full => "full",
            Encoding::RleV1 => "rle-v1",
        }
    }

    /// The encoding named `name`, or today's error.
    pub fn parse(name: &str) -> Result<Encoding, String> {
        match name {
            "full" => Ok(Encoding::Full),
            "rle-v1" => Ok(Encoding::RleV1),
            _ => Err("Unsupported report encoding".into()),
        }
    }
}

/// What an experiment is run with: the verifier's settings when it was
/// queued (`budgets`, `placementMode`) and when it ran (`reportEncoding`).
/// The engine verifier's others are fixed: `sprint` and `asyncPreparation`
/// true, no seed.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// `budgets`.
    pub budgets: Limits,
    /// `placementMode`.
    pub placement: Placement,
    /// `reportEncoding`.
    pub encoding: Encoding,
}

/// A validated experiment (`Experiments.request`): the request object in
/// today's key order, and what the verifier reads of it.
#[derive(Clone, Debug)]
pub struct Experiment {
    /// The request, `validateRequest`'s object with `worldRevision`,
    /// `sprint` and `placementMode` after it.
    pub request: Map<String, Value>,
    /// `duration`: the last sample tick.
    pub duration: i64,
    /// How many probes, unique samples and actions it has.
    pub counts: [usize; 3],
    /// `block_data_version`, when given.
    pub block_data_version: Option<i64>,
    /// The budgets it was checked against.
    pub budgets: Limits,
}

impl Experiment {
    /// The request as the engine reads it (`transport`): with the report's
    /// encoding and the preparation flag after it, and the samples as a
    /// sequence when they are evenly spaced.
    pub fn transport(&self, encoding: Encoding) -> String {
        let mut transport = self.request.clone();
        transport.insert("reportEncoding".into(), Value::from(encoding.name()));
        transport.insert("asyncPreparation".into(), Value::Bool(true));
        let samples: Vec<i64> = self.request["samples"]
            .as_array()
            .expect("samples")
            .iter()
            .map(|s| s.as_i64().expect("an integer"))
            .collect();
        if samples.len() > 1 {
            let step = samples[1] - samples[0];
            if samples
                .iter()
                .enumerate()
                .all(|(i, &tick)| tick == samples[0] + i as i64 * step)
            {
                transport.shift_remove("samples");
                transport.insert(
                    "sampleSequence".into(),
                    json!({ "start": samples[0], "step": step, "count": samples.len() }),
                );
            }
        }
        mcbench_json::stringify(&Value::Object(transport))
    }

    /// `request.cost`.
    pub fn cost(&self) -> &Value {
        &self.request["cost"]
    }
}

/// The experiments of a `test_build` call (`callTests`): `{tests: [one
/// experiment, ...]}` within the tests budget.
pub fn call_tests<'a>(
    args: &'a Map<String, Value>,
    budgets: &Limits,
) -> Result<&'a [Value], VerifyError> {
    let refused = || VerifyError::message("test_build requires {tests: [one experiment, ...]}");
    if js::keys(args).join(",") != "tests" {
        return Err(refused());
    }
    let tests = args["tests"]
        .as_array()
        .filter(|tests| !tests.is_empty())
        .ok_or_else(refused)?;
    check_budget("tests", tests.len() as f64, budgets)?;
    Ok(tests)
}

/// `checkBudget`.
pub(crate) fn check_budget(
    resource: &str,
    requested: f64,
    budgets: &Limits,
) -> Result<(), VerifyError> {
    match budgets.get(resource) {
        Some(allowed) if requested > allowed => Err(VerifyError::budget(
            resource,
            Some(requested),
            Some(allowed),
        )),
        _ => Ok(()),
    }
}

/// A number of a JSON value (`typeof value === 'number'`).
fn as_number(value: Option<&Value>) -> Option<f64> {
    value.and_then(Value::as_f64)
}

/// `integer(value, min, max, label)`.
fn integer(value: Option<&Value>, min: f64, max: f64, label: &str) -> Result<i64, VerifyError> {
    match as_number(value) {
        Some(x) if x.fract() == 0.0 && x >= min && x <= max => Ok(x as i64),
        _ => Err(VerifyError::message(format!(
            "{label} must be an integer within {}..{}",
            js::text(min),
            js::text(max)
        ))),
    }
}

/// `list(value, min, max, label)`.
fn list<'a>(
    value: Option<&'a Value>,
    min: f64,
    max: f64,
    label: &str,
) -> Result<&'a [Value], VerifyError> {
    match value.and_then(Value::as_array) {
        Some(items) if items.len() as f64 >= min && items.len() as f64 <= max => Ok(items),
        _ => Err(VerifyError::message(format!(
            "{label} requires {}..{} entries",
            js::text(min),
            js::text(max)
        ))),
    }
}

/// A block position (`position`).
fn position(value: Option<&Value>, y_offset: i32) -> Result<[i64; 3], VerifyError> {
    let items = list(value, 3.0, 3.0, "position")?;
    let mut out = [0; 3];
    for (axis, n) in items.iter().enumerate() {
        let (lo, hi) = if axis == 1 {
            (
                f64::from(MIN_MC_Y - y_offset),
                f64::from(MAX_MC_Y - y_offset),
            )
        } else {
            (-4096.0, 4096.0)
        };
        out[axis] = integer(Some(n), lo, hi, "coordinate")?;
    }
    Ok(out)
}

/// Fractional entity coordinates in blueprint space (`entityCoordinates`):
/// the numbers as given, and as doubles.
pub fn entity_coordinates(
    value: Option<&Value>,
    y_offset: i32,
) -> Result<(Value, [f64; 3]), VerifyError> {
    let items = match value.and_then(Value::as_array) {
        Some(items) if items.len() == 3 => items,
        _ => {
            return Err(VerifyError::message(
                "entity position requires 3..3 entries",
            ));
        }
    };
    let mut out = [0.0; 3];
    for (axis, n) in items.iter().enumerate() {
        let (lo, hi) = if axis == 1 {
            (
                f64::from(MIN_MC_Y - y_offset),
                f64::from(MAX_MC_Y + 1 - y_offset),
            )
        } else {
            (-4096.0, 4096.0)
        };
        match n.as_f64() {
            Some(x) if x.is_finite() && x >= lo && x < hi => out[axis] = x,
            _ => {
                return Err(VerifyError::message(
                    "Entity coordinate outside supported world bounds",
                ));
            }
        }
    }
    Ok((numbers(&out), out))
}

/// Numbers as JSON, as JavaScript holds them.
fn numbers(values: &[f64]) -> Value {
    Value::Array(values.iter().map(|&x| js::number(x)).collect())
}

fn ints(values: &[i64]) -> Value {
    Value::Array(values.iter().map(|&x| Value::from(x)).collect())
}

/// Entries `{at, data_version, snbt}` (`validateEntities`), in order: the
/// entries as checked and their positions.
pub fn validate_entities(
    value: &Value,
    y_offset: i32,
) -> Result<Vec<(Value, [f64; 3])>, VerifyError> {
    let Some(items) = value.as_array() else {
        return Err(VerifyError::message(
            "entities requires 0..9007199254740991 entries",
        ));
    };
    items
        .iter()
        .map(|entry| {
            if !js::has_exactly(entry, &["at", "data_version", "snbt"])
                || !nonblank(entry.get("snbt"))
            {
                return Err(VerifyError::message(
                    "entities require at, data_version and nonempty snbt",
                ));
            }
            let (at, numbers) = entity_coordinates(entry.get("at"), y_offset)?;
            let version = integer(entry.get("data_version"), 0.0, 2147483647.0, "data_version")?;
            Ok((
                json!({ "at": at, "data_version": version, "snbt": entry["snbt"] }),
                numbers,
            ))
        })
        .collect()
}

/// `typeof value === 'string' && value.trim()`.
fn nonblank(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_str)
        .is_some_and(|text| !js::trim(text).is_empty())
}

/// `/^minecraft:[a-z0-9_]+$/`.
fn minecraft_id(value: Option<&Value>) -> bool {
    value.and_then(Value::as_str).is_some_and(|id| {
        id.strip_prefix("minecraft:").is_some_and(|name| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
    })
}

/// `sub_tick`: `/^(-?[0-9]+)$/` within a signed 64-bit integer.
fn signed_64(value: Option<&Value>) -> bool {
    let Some(text) = value.and_then(Value::as_str) else {
        return false;
    };
    let (negative, digits) = match text.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, text),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let significant = digits.trim_start_matches('0');
    if significant.len() > 19 {
        return false;
    }
    let magnitude: u128 = if significant.is_empty() {
        0
    } else {
        significant.parse().expect("at most 19 digits")
    };
    if negative {
        magnitude <= 1 << 63
    } else {
        magnitude < 1 << 63
    }
}

/// Blocks the test reads as Minecraft does: the item tag
/// `minecraft:lectern_books`, which a supplied book must be in
/// (`LecternBlock.useItemOn`), from Minecraft's table.
#[derive(Clone, Debug)]
pub struct ItemTags {
    /// `minecraft:lectern_books`.
    pub lectern_books: HashSet<String>,
}

impl ItemTags {
    /// The tags of the engine's table (`blocks.txt`, its `itemtag` lines).
    pub fn from_table(text: &str) -> Result<ItemTags, String> {
        for line in text.split('\n') {
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() >= 3 && fields[0] == "itemtag" && fields[1] == "minecraft:lectern_books"
            {
                let members = if fields[2] == "-" {
                    HashSet::new()
                } else {
                    fields[2].split(',').map(str::to_string).collect()
                };
                return Ok(ItemTags {
                    lectern_books: members,
                });
            }
        }
        Err("Minecraft has no item tag minecraft:lectern_books".into())
    }
}

/// One experiment of a call (`validateRequest(world, args, yOffset,
/// budgets)`), then `Experiments.request`'s fields.
pub fn validate_request(
    world: &World,
    args: &Value,
    y_offset: i32,
    settings: &Settings,
    tags: &ItemTags,
) -> Result<Experiment, VerifyError> {
    let budgets = &settings.budgets;
    if args.get("scheduled_ticks").is_some() && settings.placement != Placement::Strict {
        return Err(VerifyError::message(
            "scheduled_ticks is unavailable in updates placement mode. Omit scheduled_ticks when testing a newly authored blueprint. Restoring an authoritative saved tick queue requires a verifier configured for strict placement; this test was not started.",
        ));
    }
    let Some(object) = args.as_object() else {
        return Err(VerifyError::message("A test requires an object"));
    };
    for key in js::keys(object) {
        if !TEST_KEYS.contains(&key) {
            return Err(VerifyError::message(format!(
                "Unknown test argument: {key}"
            )));
        }
    }
    let get = |name: &str| object.get(name);
    let request_bytes = mcbench_json::stringify(args).len() as f64;
    check_budget("requestBytes", request_bytes, budgets)?;
    validate_world_height(world, y_offset).map_err(VerifyError::Message)?;
    if world.size() == 0 {
        return Err(VerifyError::message("The blueprint is empty"));
    }
    let probes = list(get("probes"), 1.0, MAX_SAFE, "probes")?
        .iter()
        .map(|p| position(Some(p), y_offset))
        .collect::<Result<Vec<_>, _>>()?;
    let default_samples = ints(&DEFAULT_SAMPLES);
    let mut samples = list(
        Some(js::or_default(get("samples"), &default_samples)),
        1.0,
        MAX_SAFE,
        "samples",
    )?
    .iter()
    .map(|t| integer(Some(t), 0.0, 2_147_483_647.0, "sample tick"))
    .collect::<Result<Vec<_>, _>>()?;
    samples.sort();
    samples.dedup();
    let empty = Value::Array(Vec::new());
    let inventory_probes = list(
        Some(js::or_default(get("inventory_probes"), &empty)),
        0.0,
        MAX_SAFE,
        "inventory_probes",
    )?
    .iter()
    .map(|p| position(Some(p), y_offset))
    .collect::<Result<Vec<_>, _>>()?;
    let probe_keys: HashSet<[i64; 3]> = probes.iter().copied().collect();
    let distinct: HashSet<[i64; 3]> = inventory_probes.iter().copied().collect();
    if inventory_probes.iter().any(|p| !probe_keys.contains(p))
        || distinct.len() != inventory_probes.len()
    {
        return Err(VerifyError::message(
            "inventory_probes must be distinct positions included in probes",
        ));
    }
    if let Some(missing) = get("inventory_missing")
        && !matches!(missing.as_str(), Some("error" | "report"))
    {
        return Err(VerifyError::message(
            "inventory_missing must be error or report",
        ));
    }
    for name in ["world_inventories", "living_entities", "item_entities"] {
        if let Some(value) = get(name)
            && !value.is_boolean()
        {
            return Err(VerifyError::message(format!("{name} must be a boolean")));
        }
    }
    let block_entities = list(
        Some(js::or_default(get("block_entities"), &empty)),
        0.0,
        MAX_SAFE,
        "block_entities",
    )?
    .iter()
    .map(|entry| {
        if !js::has_exactly(entry, &["at", "data_version", "snbt"]) || !nonblank(entry.get("snbt"))
        {
            return Err(VerifyError::message(
                "block_entities require at, data_version and nonempty snbt",
            ));
        }
        let at = position(entry.get("at"), y_offset)?;
        let version = integer(entry.get("data_version"), 0.0, 2147483647.0, "data_version")?;
        Ok((
            at,
            json!({ "at": at, "data_version": version, "snbt": entry["snbt"] }),
        ))
    })
    .collect::<Result<Vec<_>, _>>()?;
    if block_entities
        .iter()
        .map(|(at, _)| *at)
        .collect::<HashSet<_>>()
        .len()
        != block_entities.len()
    {
        return Err(VerifyError::message("Duplicate block entity position"));
    }
    let entities = validate_entities(js::or_default(get("entities"), &empty), y_offset)?;
    let mut scheduled: Option<Scheduled> = None;
    if let Some(saved) = get("scheduled_ticks") {
        if !js::has_exactly(saved, &["bounds", "data_version", "entries"]) {
            return Err(VerifyError::message(
                "scheduled_ticks require bounds, data_version and entries",
            ));
        }
        let bounds = list(saved.get("bounds"), 2.0, 2.0, "tick bounds")?
            .iter()
            .map(|p| position(Some(p), y_offset))
            .collect::<Result<Vec<_>, _>>()?;
        let bounds = [bounds[0], bounds[1]];
        if (0..3).any(|i| bounds[0][i] > bounds[1][i]) {
            return Err(VerifyError::message("Reversed scheduled tick bounds"));
        }
        let mut entries = Vec::new();
        let mut ats = Vec::new();
        let mut seen = HashSet::new();
        let items = list(
            saved.get("entries"),
            0.0,
            MAX_SAFE,
            "scheduled tick entries",
        )?;
        for e in items {
            let kind = e.get("kind").and_then(Value::as_str);
            if !js::has_exactly(e, &["at", "delay", "id", "kind", "priority", "sub_tick"])
                || !matches!(kind, Some("block" | "fluid"))
                || !minecraft_id(e.get("id"))
            {
                return Err(VerifyError::message("Invalid scheduled tick entry"));
            }
            let at = position(e.get("at"), y_offset)?;
            if (0..3).any(|i| at[i] < bounds[0][i] || at[i] > bounds[1][i]) {
                return Err(VerifyError::message("Scheduled tick outside saved region"));
            }
            if !signed_64(e.get("sub_tick")) {
                return Err(VerifyError::message(
                    "sub_tick must be a signed 64-bit decimal string",
                ));
            }
            let delay = integer(e.get("delay"), -2147483648.0, 2147483647.0, "tick delay")?;
            let priority = integer(e.get("priority"), -3.0, 3.0, "tick priority")?;
            let id = e["id"].as_str().expect("an id");
            entries.push(json!({ "kind": kind, "id": id, "at": at, "delay": delay, "priority": priority, "sub_tick": e["sub_tick"] }));
            seen.insert((kind.expect("a kind").to_string(), id.to_string(), at));
            ats.push(at);
        }
        if seen.len() != entries.len() {
            return Err(VerifyError::message("Duplicate scheduled tick"));
        }
        let version = integer(saved.get("data_version"), 0.0, 2147483647.0, "data_version")?;
        scheduled = Some((
            json!({ "bounds": bounds, "data_version": version, "entries": entries }),
            bounds,
            ats,
        ));
    }
    let last_sample = *samples.last().expect("one sample at least");
    let mut actions = list(
        Some(js::or_default(get("actions"), &empty)),
        0.0,
        MAX_SAFE,
        "actions",
    )?
    .iter()
    .map(|action| validate_action(action, last_sample, y_offset, tags))
    .collect::<Result<Vec<_>, _>>()?;
    actions.sort_by_key(|(tick, _, _)| *tick);
    let settle_ticks = integer(
        Some(js::or_default(
            get("settle_ticks"),
            &Value::from(DEFAULT_SETTLE_TICKS),
        )),
        1.0,
        2_147_483_647.0,
        "settle_ticks",
    )?;
    let operations = (probes.len() as f64 + inventory_probes.len() as f64 * 54.0)
        * (samples.len() as f64 + 1.0)
        + actions.len() as f64
        + block_entities.len() as f64
        + entities.len() as f64
        + scheduled
            .as_ref()
            .map_or(0.0, |(_, _, ats)| ats.len() as f64);
    let simulation_ticks = settle_ticks as f64 + last_sample as f64;
    check_budget("operations", operations, budgets)?;
    check_budget("simulationTicks", simulation_ticks, budgets)?;
    let mut columns: IndexSet<(i64, i64)> = IndexSet::new();
    let mut include = |cx: i64, cz: i64| -> Result<(), VerifyError> {
        for dx in -1..=1 {
            for dz in -1..=1 {
                columns.insert((cx + dx, cz + dz));
            }
        }
        check_budget("columns", columns.len() as f64, budgets)
    };
    let column = |x: f64| (x / 16.0).floor() as i64;
    if let Some((_, bounds, _)) = &scheduled {
        for cx in column(bounds[0][0] as f64)..=column(bounds[1][0] as f64) {
            for cz in column(bounds[0][2] as f64)..=column(bounds[1][2] as f64) {
                include(cx, cz)?;
            }
        }
    }
    for (key, _) in world.chunks() {
        let [cx, _, cz] = mcbench_world::coords_of(key);
        include(i64::from(cx), i64::from(cz))?;
    }
    let block = |p: &[i64; 3]| [p[0] as f64, p[1] as f64, p[2] as f64];
    let mut points: Vec<[f64; 3]> = probes.iter().map(block).collect();
    points.extend(actions.iter().map(|(_, _, at)| *at));
    points.extend(block_entities.iter().map(|(at, _)| block(at)));
    points.extend(entities.iter().map(|(_, at)| *at));
    if let Some((_, bounds, ats)) = &scheduled {
        points.extend(bounds.iter().map(block));
        points.extend(ats.iter().map(block));
    }
    for p in points {
        include(column(p[0]), column(p[2]))?;
    }
    let mut request = Map::new();
    let flag = |name: &str, request: &mut Map<String, Value>| {
        if let Some(value) = get(name) {
            request.insert(name.into(), value.clone());
        }
    };
    flag("living_entities", &mut request);
    let block_data_version = match get("block_data_version") {
        Some(value) => {
            let version = integer(Some(value), 0.0, 2147483647.0, "block_data_version")?;
            request.insert("block_data_version".into(), Value::from(version));
            Some(version)
        }
        None => None,
    };
    flag("world_inventories", &mut request);
    if let Some((value, _, _)) = &scheduled {
        request.insert("scheduled_ticks".into(), value.clone());
    }
    flag("inventory_missing", &mut request);
    flag("item_entities", &mut request);
    request.insert("probes".into(), json!(probes));
    request.insert("samples".into(), ints(&samples));
    let counts = [probes.len(), samples.len(), actions.len()];
    request.insert(
        "actions".into(),
        Value::Array(actions.into_iter().map(|(_, action, _)| action).collect()),
    );
    request.insert(
        "entities".into(),
        Value::Array(entities.into_iter().map(|(entry, _)| entry).collect()),
    );
    request.insert(
        "block_entities".into(),
        Value::Array(block_entities.into_iter().map(|(_, entry)| entry).collect()),
    );
    request.insert("inventory_probes".into(), json!(inventory_probes));
    request.insert("settle_ticks".into(), Value::from(settle_ticks));
    request.insert("duration".into(), Value::from(last_sample));
    request.insert("yOffset".into(), Value::from(y_offset));
    request.insert(
        "columns".into(),
        Value::Array(columns.iter().map(|(x, z)| json!([x, z])).collect()),
    );
    request.insert(
        "cost".into(),
        json!({
            "operations": js::number(operations),
            "simulationTicks": js::number(simulation_ticks),
            "requestBytes": js::number(request_bytes),
            "columns": columns.len(),
        }),
    );
    request.insert("budgets".into(), budgets.to_json());
    request.insert("worldRevision".into(), Value::from(world.revision()));
    request.insert("sprint".into(), Value::Bool(true));
    request.insert(
        "placementMode".into(),
        Value::from(settings.placement.name()),
    );
    Ok(Experiment {
        request,
        duration: last_sample,
        counts,
        block_data_version,
        budgets: budgets.clone(),
    })
}

/// A validated saved tick queue: as the request holds it, its bounds and
/// its entries' positions.
type Scheduled = (Value, [[i64; 3]; 2], Vec<[i64; 3]>);

/// One input action: its tick, the action as the request holds it, and its
/// position.
fn validate_action(
    action: &Value,
    last_sample: i64,
    y_offset: i32,
    tags: &ItemTags,
) -> Result<(i64, Value, [f64; 3]), VerifyError> {
    let object = action.as_object();
    let has = |name: &str| object.is_some_and(|o| o.contains_key(name));
    let kind = action.get("type").and_then(Value::as_str);
    let inventory = matches!(kind, Some("insert" | "extract"));
    let drop = kind == Some("drop");
    let mut fields: Vec<&str> = match kind {
        Some("book") => vec!["tick", "type", "at", "present"],
        Some("page") => vec!["tick", "type", "at", "page"],
        _ if drop => vec!["tick", "type", "at", "item", "count", "velocity"],
        _ if inventory => {
            let mut fields = vec!["tick", "type", "at", "slot", "count"];
            if kind == Some("insert") {
                fields.push("item");
            }
            fields
        }
        _ => vec!["tick", "type", "at"],
    };
    if kind == Some("use") && has("yaw") {
        fields.push("yaw");
    }
    let supplied_book = kind == Some("book") && has("stack");
    if supplied_book {
        fields.extend(["item", "count", "stack"]);
    } else if (drop || kind == Some("insert")) && has("stack") {
        fields.push("stack");
    }
    let well_formed = object
        .is_some_and(|o| o.len() == fields.len() && fields.iter().all(|k| o.contains_key(*k)))
        && kind.is_some_and(|k| ACTION_TYPES.contains(&k));
    if !well_formed {
        return Err(VerifyError::message("Malformed input action"));
    }
    let kind = kind.expect("a type");
    let get = |name: &str| action.get(name);
    let tick = integer(get("tick"), 0.0, last_sample as f64, "action tick")?;
    let mut result = Map::new();
    result.insert("tick".into(), Value::from(tick));
    result.insert("type".into(), Value::from(kind));
    let at = if drop {
        let (value, numbers) = entity_coordinates(get("at"), y_offset)?;
        result.insert("at".into(), value);
        numbers
    } else {
        let at = position(get("at"), y_offset)?;
        result.insert("at".into(), json!(at));
        [at[0] as f64, at[1] as f64, at[2] as f64]
    };
    if kind == "book" {
        let Some(present) = get("present").and_then(Value::as_bool) else {
            return Err(VerifyError::message("book present must be boolean"));
        };
        result.insert("present".into(), Value::Bool(present));
    }
    if supplied_book {
        let item = get("item").and_then(Value::as_str);
        if !js::truthy(get("present"))
            || !item.is_some_and(|item| tags.lectern_books.contains(item))
            || as_number(get("count")) != Some(1.0)
        {
            return Err(VerifyError::message(
                "Book supply requires present:true and exactly one written or writable book",
            ));
        }
        result.insert("item".into(), get("item").expect("an item").clone());
        result.insert("count".into(), Value::from(1));
    }
    if kind == "page" {
        result.insert(
            "page".into(),
            Value::from(integer(get("page"), 0.0, 2147483547.0, "page")?),
        );
    }
    if has("yaw") {
        match as_number(get("yaw")) {
            Some(yaw) if yaw.is_finite() => {
                result.insert("yaw".into(), js::number(yaw));
            }
            _ => {
                return Err(VerifyError::message(
                    "yaw must be a finite number of degrees",
                ));
            }
        }
    }
    if drop {
        result.insert(
            "count".into(),
            Value::from(integer(get("count"), 1.0, 64.0, "count")?),
        );
        if !minecraft_id(get("item")) {
            return Err(VerifyError::message("item must be a Minecraft item ID"));
        }
        result.insert("item".into(), get("item").expect("an item").clone());
        let velocity = list(get("velocity"), 3.0, 3.0, "velocity")?
            .iter()
            .map(|n| match n.as_f64() {
                Some(x) if x.is_finite() && x.abs() <= 10.0 => Ok(js::number(x)),
                _ => Err(VerifyError::message(
                    "Velocity must be finite and within native entity load range -10..10",
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        result.insert("velocity".into(), Value::Array(velocity));
    }
    if has("stack") {
        let stack = get("stack").expect("a stack");
        if !js::has_exactly(stack, &["data_version", "snbt"]) || !nonblank(stack.get("snbt")) {
            return Err(VerifyError::message(
                "stack requires data_version and nonempty SNBT",
            ));
        }
        let version = integer(
            stack.get("data_version"),
            0.0,
            2147483647.0,
            "stack data_version",
        )?;
        result.insert(
            "stack".into(),
            json!({ "data_version": version, "snbt": stack["snbt"] }),
        );
    }
    if inventory {
        result.insert(
            "slot".into(),
            Value::from(integer(get("slot"), 0.0, 2147483647.0, "slot")?),
        );
        result.insert(
            "count".into(),
            Value::from(integer(get("count"), 1.0, 64.0, "count")?),
        );
        if kind == "insert" {
            if !minecraft_id(get("item")) {
                return Err(VerifyError::message("item must be a Minecraft item ID"));
            }
            result.insert("item".into(), get("item").expect("an item").clone());
        }
    }
    Ok((tick, Value::Object(result), at))
}
