//! A port of `test/helpers/block-table.js`: block tables in
//! `McbenchBlockTable.java`'s format and render layers in
//! `McbenchClient.java`'s, of states written for a test (what is under test
//! is the code's handling of a table, not Minecraft's facts).

#![allow(dead_code)]

use mcbench_world::block_states::BlockStates;

/// One test state (`{ name, emission, lightBlock, full, faces, fluid, model,
/// cull, shades, blended, fluidBlended, renderer }`).
#[derive(Clone)]
pub struct TestState {
    pub name: &'static str,
    pub emission: u8,
    pub light_block: u8,
    pub full: u8,
    pub faces: [u8; 6],
    pub fluid: Option<&'static str>,
    pub model: bool,
    pub cull: Option<[u8; 6]>,
    pub shades: Option<bool>,
    pub blended: bool,
    pub fluid_blended: bool,
    pub renderer: &'static str,
}

impl TestState {
    pub fn new(name: &'static str) -> TestState {
        TestState {
            name,
            emission: 0,
            light_block: 0,
            full: 0,
            faces: [0; 6],
            fluid: None,
            model: true,
            cull: None,
            shades: None,
            blended: false,
            fluid_blended: false,
            renderer: "-",
        }
    }

    /// A cube with six full faces that blocks all light.
    pub fn cube(name: &'static str) -> TestState {
        TestState {
            full: 63,
            light_block: 15,
            ..TestState::new(name)
        }
    }
}

/// The table's options (`{ shapes, pairs, skips, cullShapes, hidePairs }`).
pub struct Options {
    pub shapes: usize,
    pub pairs: Vec<(usize, usize)>,
    pub skips: Vec<(&'static str, &'static str, u8)>,
    pub cull_shapes: usize,
    pub hide_pairs: Vec<(usize, usize)>,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            shapes: 2,
            pairs: Vec::new(),
            skips: Vec::new(),
            cull_shapes: 2,
            hide_pairs: Vec::new(),
        }
    }
}

fn unique(list: impl Iterator<Item = &'static str>) -> Vec<&'static str> {
    let mut out = Vec::new();
    for item in list {
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

/// `blockTable(states, options)`.
pub fn block_table(states: &[TestState], options: &Options) -> String {
    let names: Vec<&str> = states.iter().map(|s| s.name).collect();
    let id_of = |name: &str| {
        names
            .iter()
            .position(|n| *n == name)
            .unwrap_or_else(|| panic!("No test state {name}"))
    };
    let row_names = unique(options.skips.iter().map(|s| s.0));
    let column_names = unique(options.skips.iter().map(|s| s.1));
    let mut lines = Vec::new();
    let mut block = "";
    for s in states {
        let id = s.name.split('[').next().expect("a name");
        if id != block {
            lines.push(format!("default\t{id}\t{}\tTest {id}", s.name));
        }
        block = id;
        let index = |list: &Vec<&str>| list.iter().position(|n| *n == s.name).map_or(0, |i| i + 1);
        let skip = format!("{},{}", index(&row_names), index(&column_names));
        let cull = s
            .cull
            .unwrap_or([0, 1, 2, 3, 4, 5].map(|d| (s.full >> d) & 1));
        let shades = s.shades.unwrap_or(s.full == 63);
        let join = |v: &[u8]| v.iter().map(u8::to_string).collect::<Vec<_>>().join(",");
        lines.push(
            [
                s.name.to_string(),
                s.emission.to_string(),
                s.light_block.to_string(),
                s.full.to_string(),
                join(&s.faces),
                s.fluid.map_or(0, id_of).to_string(),
                u8::from(s.model).to_string(),
                skip,
                join(&cull),
                u8::from(shades).to_string(),
            ]
            .join("\t"),
        );
    }
    let hides = |a: usize, b: usize| b == 1 || options.hide_pairs.contains(&(a, b));
    let mut hidden = String::new();
    for a in 0..options.cull_shapes {
        for b in 0..options.cull_shapes {
            hidden.push(if hides(a, b) { '1' } else { '0' });
        }
    }
    lines.push(format!("facehides\t{}\t{hidden}", options.cull_shapes));
    let closes = |a: usize, b: usize| a == 1 || b == 1 || options.pairs.contains(&(a, b));
    let mut bits = String::new();
    for a in 0..options.shapes {
        for b in 0..options.shapes {
            bits.push(if closes(a, b) { '1' } else { '0' });
        }
    }
    lines.push(format!("faceoccludes\t{}\t{bits}", options.shapes));
    let rows: Vec<String> = row_names
        .iter()
        .map(|a| {
            let masks = column_names.iter().map(|b| {
                options
                    .skips
                    .iter()
                    .filter(|(p, q, _)| p == a && q == b)
                    .fold(0u8, |m, (_, _, d)| m | d)
            });
            std::iter::once(1u8)
                .chain(masks)
                .map(|m| m.to_string())
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect();
    let mut skip_line = vec![
        "skiprendering".to_string(),
        (row_names.len() + 1).to_string(),
        (column_names.len() + 1).to_string(),
        "0".to_string(),
    ];
    skip_line.extend(rows);
    lines.push(skip_line.join("\t"));
    format!("{}\n", lines.join("\n"))
}

/// `renderLayers(states)`.
pub fn render_layers(states: &[TestState]) -> String {
    let mut lines = vec![
        "layer\tOPAQUE\t0".to_string(),
        "layer\tBLENDED\t1".to_string(),
    ];
    for s in states {
        lines.push(format!(
            "{}\t{}\t{}\t{}",
            s.name,
            u8::from(s.blended),
            u8::from(s.fluid_blended),
            s.renderer
        ));
    }
    format!("{}\n", lines.join("\n"))
}

/// `testBlockStates(states, options)`: air first.
pub fn test_block_states(states: &[TestState], options: &Options) -> BlockStates {
    let mut all = vec![TestState {
        model: false,
        ..TestState::new("minecraft:air")
    }];
    all.extend_from_slice(states);
    BlockStates::new(&block_table(&all, options), &render_layers(&all))
        .expect("the test table reads")
}
