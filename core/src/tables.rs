// The text of Minecraft's tables the engine reads (core/minecraft/<version>/,
// made on each machine by server/minecraft-tables.js; the repository and the
// built program hold none of it):
//   blocks.txt   every block, state, property, tag, shape and item
//                (Registry::parse, at boot)
//   loot.json    the blocks' loot tables (loot::tables, parsed when first read)
//   datafix.txt  the data fixer's schemas and rules (datafix, parsed when
//                first read; the native program alone has the data fixer)
// The native program reads them from the folder its caller names, once, at
// its start (read); the WebAssembly module is handed blocks.txt and loot.json
// by its host before mcb_init (set). Either way the text is kept for the
// process, as the parsed tables borrow from it.

use std::sync::OnceLock;

pub struct Text {
    pub blocks: String,
    pub loot: String,
    #[cfg(not(target_arch = "wasm32"))]
    pub datafix: String,
}

static TEXT: OnceLock<Text> = OnceLock::new();

// Keeps the tables' text for the process. Once only.
pub fn set(text: Text) {
    if TEXT.set(text).is_err() {
        panic!("the tables were given twice");
    }
}

// The tables of `folder`, each file read once; a file that cannot be read
// ends the process with a message naming it.
#[cfg(not(target_arch = "wasm32"))]
pub fn read(folder: &std::path::Path) -> Text {
    let file = |name: &str| {
        let path = folder.join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|error| {
            eprintln!("mcbench-core: cannot read Minecraft's table {}: {error}", path.display());
            std::process::exit(2);
        })
    };
    Text { blocks: file("blocks.txt"), loot: file("loot.json"), datafix: file("datafix.txt") }
}

// The text set at the start. The tests read the tables of the version the
// engine ports from core/minecraft, as the app makes them.
pub fn get() -> &'static Text {
    #[cfg(test)]
    TEXT.get_or_init(|| read(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("minecraft").join("1.21.10")));
    TEXT.get().expect("the tables are read before the engine starts")
}
