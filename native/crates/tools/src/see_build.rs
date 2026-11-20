//! `see_build`: `agent.js` `executeSeeBuild` and its branch of
//! `executeToolCall`. [`check`] (the empty world, the shot list, `attach`,
//! each shot an object and its tick checked, in today's order and
//! sentences); [`draw`] (the build as Minecraft makes it at each tick
//! asked, each shot resolved as `resolveShot` resolves it, the pictures
//! drawn by the process's one renderer, [`crate::shots::SharedRenderer`]);
//! the JPEGs saved with the chat's images; and [`finish`] (the note, and
//! the screenshots attached in the next message or only saved).
//!
//! The server's log line of a call's timings (`[see_build] <blocks> blocks,
//! <n> shots: ...`) is not kept, as `build`'s settle timings are not.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine as _;
use mcbench_store::ImageData;
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use crate::settle::{Settle, Settler};
use crate::shots::{Resolved, SharedRenderer, ToDraw};
use crate::{
    Chat, ImageRef, Progress, Screenshots, Services, Tool, ToolError, ToolOutcome, build, settle,
    shots,
};

/// `MAX_SHOTS`.
pub const MAX_SHOTS: usize = 6;

/// A shot as checked: its camera (the shot without `tick`) and its tick.
#[derive(Clone, Debug, PartialEq)]
pub struct CheckedShot {
    /// The shot's fields but `tick`, for `resolveShot`.
    pub camera: Map<String, Value>,
    /// The tick it is taken at.
    pub tick: i64,
}

/// The arguments as checked.
#[derive(Clone, Debug, PartialEq)]
pub struct Checked {
    /// The shots, in order.
    pub shots: Vec<CheckedShot>,
    /// `attach !== false`.
    pub attach: bool,
}

/// `executeSeeBuild`'s checks before anything is settled or drawn, or the
/// result a model reads.
pub fn check(
    world: &mcbench_world::World,
    args: &Map<String, Value>,
    max_ticks: u64,
) -> Result<Checked, Value> {
    let error = |message: String| json!({"ok": false, "error": message});
    if world.size() == 0 {
        return Err(error("The world is empty — nothing to render yet".into()));
    }
    let default = json!([{"view": "iso_se"}, {"view": "iso_nw"}]);
    let specs = match args.get("shots") {
        None | Some(Value::Null) => &default,
        Some(specs) => specs,
    };
    let specs = match specs.as_array() {
        Some(specs) if !specs.is_empty() && specs.len() <= MAX_SHOTS => specs,
        _ => {
            return Err(error(format!(
                "shots must be a list of 1 to {MAX_SHOTS} shots"
            )));
        }
    };
    let attach = match args.get("attach") {
        None => true,
        Some(Value::Bool(attach)) => *attach,
        Some(_) => return Err(error("attach must be true or false".into())),
    };
    let base = settle::current_ticks(world);
    let mut shots = Vec::new();
    for (index, spec) in specs.iter().enumerate() {
        let Value::Object(spec) = spec else {
            return Err(error(format!("shot {} must be an object", index + 1)));
        };
        let mut camera = spec.clone();
        let tick = match camera.shift_remove("tick") {
            None => base,
            Some(tick) => settle::check_ticks(&tick, "tick", max_ticks)
                .map_err(|message| error(format!("shot {}: {message}", index + 1)))?,
        };
        shots.push(CheckedShot { camera, tick });
    }
    Ok(Checked { shots, attach })
}

/// `executeToolCall`'s `see_build` branch around a drawn result (`result`
/// without `images` and `attach`): the note, and the images attached in
/// the next message or only saved.
pub fn finish(mut result: Map<String, Value>, images: Vec<ImageRef>, attach: bool) -> ToolOutcome {
    let note = if attach {
        format!(
            "{} screenshot(s) attached in the next message, in shot order",
            images.len()
        )
    } else {
        "Shots saved under /work/images only (attach: false)".to_string()
    };
    result.insert("note".into(), json!(note));
    if attach {
        ToolOutcome {
            result: Value::Object(result),
            screenshots: Some(Screenshots {
                text: "[see_build] Screenshots of the current build:".into(),
                images,
            }),
            images: Vec::new(),
        }
    } else {
        ToolOutcome {
            result: Value::Object(result),
            screenshots: None,
            images,
        }
    }
}

/// What [`draw`] made: the settles by tick, each shot resolved with its
/// tick, and each shot's JPEG, in shot order.
pub struct Drawn {
    /// The settles, by tick (`settled`, a `Map` in ascending ticks).
    pub settled: BTreeMap<i64, Settle>,
    /// Each shot as resolved, and its tick.
    pub shots: Vec<(Resolved, i64)>,
    /// Each shot's JPEG.
    pub jpegs: Vec<Vec<u8>>,
}

impl Drawn {
    /// `minecraft: [...settled.values()].map(minecraftReport)`.
    pub fn minecraft(&self) -> Value {
        Value::Array(
            self.settled
                .values()
                .map(|settle| build::minecraft_report(&settle.value))
                .collect(),
        )
    }

    /// Shot `index`'s entry of the result: `{ image, tick, ...described }`.
    pub fn shot(&self, index: usize, image: String) -> Value {
        let (resolved, tick) = &self.shots[index];
        let mut shot = Map::new();
        shot.insert("image".into(), json!(image));
        shot.insert("tick".into(), json!(tick));
        shot.extend(resolved.described.clone());
        Value::Object(shot)
    }
}

/// `executeSeeBuild`'s try block, for checked arguments: the settles of the
/// chat's world at each tick asked (the world's settle ticks become its
/// current ones), each shot resolved on its settle, and the pictures drawn.
/// A failure is the sentence today's catch answers.
pub async fn draw(
    settler: &Settler,
    renderer: &SharedRenderer,
    conversation: &str,
    world: &mut mcbench_world::World,
    checked: &Checked,
    cancel: &CancellationToken,
) -> Result<Drawn, String> {
    let base = settle::current_ticks(world);
    let ticks: Vec<i64> = checked.shots.iter().map(|shot| shot.tick).collect();
    let settled = settler
        .settle(conversation, world, &ticks, Some(base))
        .await?;
    let mut shots = Vec::new();
    for (index, shot) in checked.shots.iter().enumerate() {
        let settle = &settled[&shot.tick].value;
        if settle.world.size() == 0 {
            return Err(format!(
                "shot {}: Minecraft keeps none of the blocks of this build at tick {}",
                index + 1,
                shot.tick
            ));
        }
        let resolved = shots::resolve_shot(&shot.camera, &settle.world, &settle.entities)
            .map_err(|error| format!("shot {}: {error}", index + 1))?;
        shots.push((resolved, shot.tick));
    }
    let to_draw: Vec<ToDraw<'_>> = shots
        .iter()
        .map(|(resolved, tick)| ToDraw {
            shot: &resolved.shot,
            tick: *tick,
            settled: &settled[tick].value,
        })
        .collect();
    let jpegs = renderer.render(conversation, &to_draw, cancel).await?;
    Ok(Drawn {
        settled,
        shots,
        jpegs,
    })
}

/// The `see_build` tool.
pub struct SeeBuild(pub Arc<Services>);

#[async_trait]
impl Tool for SeeBuild {
    fn name(&self) -> &'static str {
        "see_build"
    }

    async fn call(
        &self,
        chat: &mut Chat,
        arguments: &Map<String, Value>,
        _progress: &mut (dyn FnMut(Progress) + Send),
        cancel: &CancellationToken,
    ) -> Result<ToolOutcome, ToolError> {
        let checked = match check(&chat.world, arguments, self.0.budgets.simulation_ticks) {
            Ok(checked) => checked,
            Err(result) => return Ok(ToolOutcome::result(result)),
        };
        let drawn = match draw(
            &self.0.settler,
            &self.0.renderer,
            &chat.id,
            &mut chat.world,
            &checked,
            cancel,
        )
        .await
        {
            Ok(drawn) => drawn,
            Err(error) => {
                let mut result = Map::new();
                result.insert("ok".into(), json!(false));
                if cancel.is_cancelled() {
                    result.insert("cancelled".into(), json!(true));
                }
                result.insert("error".into(), json!(error));
                return Ok(ToolOutcome::result(Value::Object(result)));
            }
        };
        let urls: Vec<ImageData> = drawn
            .jpegs
            .iter()
            .map(|jpeg| {
                ImageData::DataUrl(format!(
                    "data:image/jpeg;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(jpeg)
                ))
            })
            .collect();
        let id = chat.id.clone();
        let saved = self
            .0
            .blocking(move |store| store.save_images(&id, &urls, ""))
            .await;
        let urls = match saved {
            Ok(urls) => urls,
            Err(error) => {
                return Ok(ToolOutcome::result(
                    json!({"ok": false, "error": error.message}),
                ));
            }
        };
        let shots: Vec<Value> = urls
            .iter()
            .enumerate()
            .map(|(index, url)| {
                let name = url.rsplit('/').next().expect("a split has a part");
                drawn.shot(index, format!("/work/images/{name}"))
            })
            .collect();
        let mut result = Map::new();
        result.insert("ok".into(), json!(true));
        result.insert("worldRevision".into(), json!(chat.world.revision()));
        result.insert("minecraft".into(), drawn.minecraft());
        result.insert("shots".into(), Value::Array(shots));
        let images = urls.into_iter().map(|url| ImageRef { url }).collect();
        Ok(finish(result, images, checked.attach))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcbench_world::{BlockState, World};

    fn world() -> World {
        let mut world = World::new();
        world
            .set([0, 0, 0], BlockState::new("minecraft:stone", &[]))
            .unwrap();
        world
    }

    fn args(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn the_arguments_are_checked_in_todays_order_and_words() {
        let error = |world: &World, value: Value| {
            check(world, &args(value), 72_000).unwrap_err()["error"].clone()
        };
        assert_eq!(
            error(&World::new(), json!({"shots": 5})),
            "The world is empty — nothing to render yet"
        );
        assert_eq!(
            error(&world(), json!({"shots": []})),
            "shots must be a list of 1 to 6 shots"
        );
        assert_eq!(
            error(&world(), json!({"shots": [{}, {}, {}, {}, {}, {}, {}]})),
            "shots must be a list of 1 to 6 shots"
        );
        assert_eq!(
            error(&world(), json!({"attach": "no"})),
            "attach must be true or false"
        );
        assert_eq!(
            error(&world(), json!({"shots": [{}, 3]})),
            "shot 2 must be an object"
        );
        assert_eq!(
            error(&world(), json!({"shots": [{"tick": 1.5}]})),
            "shot 1: tick must be a whole number of ticks, 0 or more"
        );
        let checked = check(
            &world(),
            &args(json!({"shots": [{"view": "top", "tick": 0}, {}]})),
            72_000,
        )
        .unwrap();
        assert_eq!(checked.shots[0].tick, 0);
        assert_eq!(checked.shots[1].tick, 210);
        assert_eq!(checked.shots[0].camera, args(json!({"view": "top"})));
        assert!(
            check(&world(), &args(json!({})), 72_000)
                .unwrap()
                .shots
                .len()
                == 2
        );
    }
}
