//! `mcbench-lite headless shots`: `see_build`'s pictures of a world
//! (`PROTOCOL.md`, family `shots`). A request names a world as `grade`'s
//! do (a manifest file and a folder of chunk files) and `see_build`'s
//! arguments; the world is settled, framed and drawn as the tool does it
//! (`mcbench_tools::see_build::check` and `draw`, this process's engines
//! and its one renderer, `crate::renderer::process`), and the reply carries
//! the result the tool would give a model, each shot's `image` the JPEG
//! file written. Requests are served one at a time, in order.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use mcbench_world::{World, WorldManifest};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

/// The conversation the settles and the renderer's worlds are kept under.
const CONVERSATION: &str = "headless shots";

fn refuse(message: &str) -> ! {
    eprintln!("mcbench-lite headless shots: {message}");
    std::process::exit(2)
}

fn stop(message: &str) -> ! {
    eprintln!("mcbench-lite headless shots: {message}");
    std::process::exit(1)
}

/// One request's reply fields, or the frame's error.
async fn answer(request: &Map<String, Value>) -> Result<Map<String, Value>, String> {
    let field = |key: &str| {
        request
            .get(key)
            .unwrap_or_else(|| refuse(&format!("a request needs {key}")))
    };
    let Value::Object(world) = field("world") else {
        refuse("world is {\"manifest\": \"<file>\", \"chunks\": \"<folder>\"}")
    };
    let path = |key: &str| {
        world
            .get(key)
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .unwrap_or_else(|| refuse(&format!("world.{key} is a string")))
    };
    let (manifest, chunks) = (path("manifest"), path("chunks"));
    let Value::Object(arguments) = field("arguments") else {
        refuse("arguments is an object")
    };
    let out = PathBuf::from(
        field("out")
            .as_str()
            .unwrap_or_else(|| refuse("out is a string")),
    );
    let text = std::fs::read_to_string(&manifest)
        .map_err(|error| format!("{}: {error}", manifest.display()))?;
    let mut world = World::from_manifest(&WorldManifest(text), &mut |hash: &str| {
        std::fs::read(chunks.join(format!("{hash}.json"))).map_err(|e| format!("{hash}: {e}"))
    })?;
    let engines = crate::turn::adapter::engines()?;
    let renderer = crate::renderer::process()?;
    let result = match mcbench_tools::see_build::check(
        &world,
        arguments,
        engines.budgets.simulation_ticks,
    ) {
        Err(result) => result,
        Ok(checked) => match mcbench_tools::see_build::draw(
            &engines.settler,
            &renderer,
            CONVERSATION,
            &mut world,
            &checked,
            &CancellationToken::new(),
        )
        .await
        {
            Err(error) => json!({"ok": false, "error": error}),
            Ok(drawn) => {
                let shots = drawn
                    .jpegs
                    .iter()
                    .enumerate()
                    .map(|(index, jpeg)| {
                        let file = write(&out, index, jpeg)?;
                        Ok(drawn.shot(index, file))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                json!({
                    "ok": true,
                    "worldRevision": world.revision(),
                    "minecraft": drawn.minecraft(),
                    "shots": shots,
                })
            }
        },
    };
    let mut fields = Map::new();
    fields.insert("result".into(), result);
    Ok(fields)
}

/// Shot `index`'s JPEG as `<out>/<index + 1>.jpg`.
fn write(out: &Path, index: usize, jpeg: &[u8]) -> Result<String, String> {
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let file = out.join(format!("{}.jpg", index + 1));
    std::fs::write(&file, jpeg).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(file.display().to_string())
}

/// Serves requests on stdin until it ends, one at a time, then exits 0.
pub(crate) fn run(arguments: &[String]) -> ! {
    if !arguments.is_empty() {
        refuse(&format!("takes no arguments, not {}", arguments.join(" ")));
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a tokio runtime");
    // The renderer starts now, while the first request is read.
    runtime.block_on(async {
        crate::renderer::process().unwrap_or_else(|error| stop(&error));
    });
    let mut out = std::io::stdout();
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap_or_else(|e| refuse(&e.to_string()));
        let Ok(Value::Object(request)) = serde_json::from_str::<Value>(&line) else {
            refuse("a request line is a JSON object")
        };
        let id = request
            .get("id")
            .cloned()
            .unwrap_or_else(|| refuse("a request needs id"));
        for key in request.keys() {
            if !["id", "world", "arguments", "out"].contains(&key.as_str()) {
                refuse(&format!("a request takes no {key}"));
            }
        }
        let mut reply = Map::new();
        reply.insert("id".into(), id);
        match runtime.block_on(answer(&request)) {
            Ok(fields) => {
                reply.insert("ok".into(), Value::Bool(true));
                reply.extend(fields);
            }
            Err(error) => {
                reply.insert("error".into(), Value::String(error));
            }
        }
        writeln!(out, "{}", mcbench_json::stringify(&Value::Object(reply))).expect("stdout");
        out.flush().expect("stdout");
    }
    std::process::exit(0)
}
