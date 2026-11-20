//! The prompt library's storage (`server/prompts.js` `list`, `save`,
//! `apply`): `prompts.json` in the data folder, the user's order, each
//! prompt's attachments as data URLs. The `prompt_library` tool's schema and
//! description are `mcbench-agent`'s; the results and errors a model reads
//! from `apply` are here, today's text.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::Error;
use crate::host::Host;
use crate::json::Truthy;
use crate::json::{self, Json, Object};
use crate::seed::SEED;
use crate::storage::js_space;

/// A prompt of the library, as `clean` leaves it (`{ id, title, text, tags,
/// favorite, attachments, usedAt?, addedBy? }`): `GET /api/prompts`' items
/// and `prompts.json`'s, [`LibraryPrompt::to_json`] being the stored form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryPrompt {
    /// `id`.
    pub id: String,
    /// `title`, trimmed.
    pub title: String,
    /// `text`.
    pub text: String,
    /// `tags`: trimmed, lower case, each once, in their order.
    pub tags: Vec<String>,
    /// `favorite`.
    pub favorite: bool,
    /// `attachments`.
    pub attachments: Vec<PromptAttachment>,
    /// `usedAt`, epoch ms: when the user last inserted it.
    pub used_at: Option<i64>,
    /// `addedBy`: the chat and model that added it through `prompt_library`.
    pub added_by: Option<PromptAddedBy>,
}

/// A prompt's attachment (`attachment(file)`): a file as the composer read
/// it, its bytes a base64 data URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptAttachment {
    /// `name`.
    pub name: String,
    /// `type`: the data URL's media type.
    pub content_type: String,
    /// `size`: the file's size in bytes as the page gave it (`0` when none).
    pub size: u64,
    /// `data`: `data:<type>;base64,<bytes>`.
    pub data: String,
}

/// The chat and model that added a prompt through `prompt_library`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptAddedBy {
    /// `chat`.
    pub chat: String,
    /// `model`.
    pub model: String,
}

impl LibraryPrompt {
    /// The prompt as `clean` writes it, key for key (`prompts.json`'s item,
    /// `GET /api/prompts`' `prompts[]`).
    pub fn to_json(&self) -> Json {
        let mut out = Object::new();
        out.insert("id".into(), self.id.as_str().into());
        out.insert("title".into(), self.title.as_str().into());
        out.insert("text".into(), self.text.as_str().into());
        out.insert("tags".into(), self.tags.clone().into());
        out.insert("favorite".into(), self.favorite.into());
        out.insert(
            "attachments".into(),
            Json::Array(
                self.attachments
                    .iter()
                    .map(|file| {
                        crate::object! {
                            "name" => file.name.as_str(),
                            "type" => file.content_type.as_str(),
                            "size" => json::number(file.size as f64),
                            "data" => file.data.as_str(),
                        }
                    })
                    .collect(),
            ),
        );
        if let Some(used) = self.used_at {
            out.insert("usedAt".into(), json::number(used as f64));
        }
        if let Some(by) = &self.added_by {
            out.insert(
                "addedBy".into(),
                crate::object! { "chat" => by.chat.as_str(), "model" => by.model.as_str() },
            );
        }
        Json::Object(out)
    }
}

/// `Number(value)` where today's data holds a whole number (the page writes
/// `Date.now()` and `File.size`): a fraction, `NaN`, a negative size or a
/// number past `i64` is refused, naming the field and the value.
fn whole(value: f64, field: &str, shown: &Json, negative: bool) -> Result<i64, Error> {
    if value.is_finite()
        && value.fract() == 0.0
        && value.abs() < 9.2e18
        && (negative || value >= 0.0)
    {
        Ok(value as i64)
    } else {
        Err(Error::refused(
            format!(
                "A prompt's {field} must be a whole number, not {}",
                json::to_js_string(Some(shown))
            ),
            400,
        ))
    }
}

/// The library of one data folder.
pub struct PromptLibrary {
    file: PathBuf,
    host: Arc<dyn Host>,
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(js_space)
}

/// `String(value || '')`.
fn string_or_empty(value: Option<&Json>) -> String {
    match value {
        Some(value) if value.truthy() => json::to_js_string(Some(value)),
        _ => String::new(),
    }
}

/// JavaScript's `Number(value)`.
fn js_number(value: &Json) -> f64 {
    match value {
        Json::Number(n) => n.as_f64().expect("a JSON number"),
        Json::Bool(b) => f64::from(u8::from(*b)),
        Json::Null => 0.0,
        Json::String(text) => crate::storage::js_number(text),
        Json::Array(items) if items.is_empty() => 0.0,
        Json::Array(items) if items.len() == 1 => js_number(&items[0]),
        _ => f64::NAN,
    }
}

/// `attachment(file)`: a prompt's attachment keeps its bytes as a data URL
/// and nothing else.
fn attachment(file: &Json) -> Result<PromptAttachment, Error> {
    let pattern = crate::regex!(r"^data:[^;,]+;base64,");
    let (Some(Json::String(name)), Some(Json::String(data))) = (file.get("name"), file.get("data"))
    else {
        return Err(Error::refused(
            "A prompt attachment needs a name and its bytes as a base64 data URL",
            400,
        ));
    };
    if !pattern.is_match(data) {
        return Err(Error::refused(
            "A prompt attachment needs a name and its bytes as a base64 data URL",
            400,
        ));
    }
    // `Number(file.size) || 0`.
    let size = file.get("size").map_or(0.0, js_number);
    let size = if size.is_nan() || size == 0.0 {
        0
    } else {
        let shown = file.get("size").expect("a size that is not 0");
        whole(size, "attachment size", shown, false)? as u64
    };
    Ok(PromptAttachment {
        name: name.clone(),
        content_type: data[5..data.find(';').expect("matched")].to_string(),
        size,
        data: data.clone(),
    })
}

impl PromptLibrary {
    /// The library in `file` (`prompts.json` of the data folder).
    pub fn new(file: PathBuf, host: Arc<dyn Host>) -> PromptLibrary {
        PromptLibrary { file, host }
    }

    /// The file it is kept in.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// `clean(prompt)`.
    fn clean(&self, prompt: &Json) -> Result<LibraryPrompt, Error> {
        let id = match prompt.get("id") {
            Some(id) if id.truthy() => json::to_js_string(Some(id)),
            _ => self.host.uuid(),
        };
        let mut tags: Vec<String> = Vec::new();
        if let Some(Json::Array(items)) = prompt.get("tags") {
            for tag in items {
                let tag = js_trim(&json::to_js_string(Some(tag))).to_lowercase();
                if !tag.is_empty() && !tags.contains(&tag) {
                    tags.push(tag);
                }
            }
        }
        let attachments = match prompt.get("attachments") {
            Some(Json::Array(items)) => items
                .iter()
                .map(attachment)
                .collect::<Result<Vec<_>, _>>()?,
            _ => Vec::new(),
        };
        let used_at = match prompt.get("usedAt").filter(|used| used.truthy()) {
            Some(used) => Some(whole(js_number(used), "usedAt", used, true)?),
            None => None,
        };
        let added_by = prompt
            .get("addedBy")
            .filter(|by| by.truthy())
            .map(|by| PromptAddedBy {
                chat: json::to_js_string(by.get("chat")),
                model: json::to_js_string(by.get("model")),
            });
        Ok(LibraryPrompt {
            id,
            title: js_trim(&string_or_empty(prompt.get("title"))).to_string(),
            text: string_or_empty(prompt.get("text")),
            tags,
            favorite: prompt.get("favorite") == Some(&Json::Bool(true)),
            attachments,
            used_at,
            added_by,
        })
    }

    /// `list()`: the library, or the seed prompts before it was saved.
    pub fn list(&self) -> Result<Vec<LibraryPrompt>, Error> {
        if !self.file.exists() {
            return SEED
                .iter()
                .map(|(id, title, text)| {
                    self.clean(&crate::object! { "id" => *id, "title" => *title, "text" => *text })
                })
                .collect();
        }
        let stored = json::parse(&std::fs::read_to_string(&self.file)?)?;
        let items = stored.as_array().ok_or("prompts.map is not a function")?;
        items.iter().map(|prompt| self.clean(prompt)).collect()
    }

    /// `save(prompts)`: the library in the order given, written whole and
    /// renamed into place.
    pub fn save(&self, prompts: &[Json]) -> Result<Vec<LibraryPrompt>, Error> {
        let next = prompts
            .iter()
            .map(|prompt| self.clean(prompt))
            .collect::<Result<Vec<_>, _>>()?;
        std::fs::create_dir_all(self.file.parent().expect("a folder"))?;
        let temporary = PathBuf::from(format!(
            "{}.{}.tmp",
            self.file.display(),
            std::process::id()
        ));
        std::fs::write(
            &temporary,
            json::stringify_indented(&Json::Array(
                next.iter().map(LibraryPrompt::to_json).collect(),
            )),
        )?;
        std::fs::rename(&temporary, &self.file)?;
        Ok(next)
    }

    /// `forModel(prompts)`: what a model sees, everything but attachment
    /// bytes.
    pub fn for_model(prompts: &[LibraryPrompt]) -> Vec<Json> {
        prompts
            .iter()
            .map(|prompt| {
                let Json::Object(mut object) = prompt.to_json() else {
                    unreachable!("a prompt is an object")
                };
                let attachments = object.shift_remove("attachments");
                if let Some(Json::Array(files)) = attachments
                    && !files.is_empty()
                {
                    let names = files
                        .iter()
                        .map(|file| file.get("name").cloned().unwrap_or(Json::Null))
                        .collect();
                    object.insert("attachments".into(), Json::Array(names));
                }
                Json::Object(object)
            })
            .collect()
    }

    /// `apply(args, by)`: one `prompt_library` call: reads the library, adds
    /// prompts, and changes or removes those this chat's model added. `by`
    /// is `{ chat, model }`. Returns what the model sees.
    pub fn apply(&self, args: &Json, by: &Json) -> Result<Vec<Json>, Error> {
        let empty = Object::new();
        let args = match args {
            Json::Object(object) => object,
            _ => &empty,
        };
        let rest: Vec<&str> = args
            .keys()
            .map(String::as_str)
            .filter(|key| *key != "save" && *key != "remove")
            .collect();
        if !rest.is_empty() {
            return Err(Error::plain(format!(
                "prompt_library takes only save and remove; not {}",
                rest.join(", ")
            )));
        }
        let none = Json::Array(Vec::new());
        let entries = args.get("save").unwrap_or(&none);
        let remove = args.get("remove").unwrap_or(&none);
        for (name, value) in [("save", entries), ("remove", remove)] {
            if value.as_array().is_none() {
                return Err(Error::plain(format!("{name} must be an array")));
            }
        }
        // `new Map(prompts.map((prompt) => [prompt.id, prompt]))`.
        let mut by_id: Vec<(String, Json)> = Vec::new();
        for prompt in self.list()? {
            let id = prompt.id.clone();
            let prompt = prompt.to_json();
            match by_id.iter_mut().find(|(k, _)| *k == id) {
                Some(slot) => slot.1 = prompt,
                None => by_id.push((id, prompt)),
            }
        }
        let own = |id: &Json, by_id: &[(String, Json)]| -> Result<Json, Error> {
            let text = json::to_js_string(Some(id));
            let prompt = match id {
                Json::String(key) => by_id
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, prompt)| prompt.clone()),
                _ => None,
            }
            .ok_or_else(|| Error::plain(format!("No prompt with id {text}")))?;
            let added = prompt.get("addedBy");
            let field = |name: &str| added.and_then(|added| added.get(name));
            if added.is_none()
                || field("chat") != by.get("chat")
                || field("model") != by.get("model")
            {
                return Err(Error::plain(format!(
                    "Prompt {text} was not added by you in this chat, so you cannot change or remove it. Add a new prompt instead."
                )));
            }
            Ok(prompt)
        };
        for id in remove.as_array().expect("checked") {
            own(id, &by_id)?;
            if let Json::String(key) = id {
                by_id.retain(|(k, _)| k != key);
            }
        }
        const MODEL_FIELDS: [&str; 3] = ["title", "text", "tags"];
        for entry in entries.as_array().expect("checked") {
            let fields = entry.as_object().ok_or_else(|| {
                Error::plain("Cannot destructure a save entry that is not an object")
            })?;
            let unknown: Vec<&str> = fields
                .keys()
                .map(String::as_str)
                .filter(|key| *key != "id" && !MODEL_FIELDS.contains(key))
                .collect();
            if !unknown.is_empty() {
                return Err(Error::plain(format!(
                    "A prompt has only the fields id, {}; not {}",
                    MODEL_FIELDS.join(", "),
                    unknown.join(", ")
                )));
            }
            let id = fields.get("id");
            if id.is_none() && js_trim(&string_or_empty(fields.get("text"))).is_empty() {
                return Err(Error::plain("A new prompt needs text"));
            }
            let mut merged = match id {
                None => crate::object! {
                    "id" => self.host.uuid(),
                    "attachments" => Json::Array(Vec::new()),
                    "addedBy" => by.clone(),
                },
                Some(id) => own(id, &by_id)?,
            };
            let object = merged.as_object_mut().expect("an object");
            for (key, value) in fields.iter().filter(|(key, _)| *key != "id") {
                object.insert((key).to_string(), value.clone());
            }
            let next = self.clean(&merged)?;
            let next_id = next.id.clone();
            let next = next.to_json();
            match by_id.iter_mut().find(|(k, _)| *k == next_id) {
                Some(slot) => slot.1 = next,
                None => by_id.push((next_id, next)),
            }
        }
        let saved = self.save(
            &by_id
                .into_iter()
                .map(|(_, prompt)| prompt)
                .collect::<Vec<_>>(),
        )?;
        Ok(Self::for_model(&saved))
    }
}
