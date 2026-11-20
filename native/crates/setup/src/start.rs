//! The first start: the shipped data unpacked, the textures fetched and
//! baked, and the model's shell prepared in the background from the moment
//! the window opens. A later start finds each in place and marks
//! its step done at once.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::bake::{Files, bake};
use crate::data::{Data, remove};
use crate::fetch::{self, Options};
use crate::jar::write_zip;
use crate::steps::{StepError, StepState, Steps};

/// The model's shell step's work, as the caller makes it: the files from
/// [`crate::shell::files`] given to the platform's sandbox, which is made of
/// them and prepared (`mcbench_sandbox`; on Windows WSL is checked first, and
/// without it the step answers [`StepError::Needs`]). It runs only when the
/// shell step is not done or under way.
pub type Shell = Pin<Box<dyn Future<Output = Result<(), StepError>> + Send>>;

/// The baked textures' folder (`textures/<the data's id>/` in the data
/// folder).
#[derive(Clone, Debug, PartialEq)]
pub struct Textures {
    folder: PathBuf,
}

impl Textures {
    /// The textures in `folder`.
    pub fn at(folder: &Path) -> Textures {
        Textures {
            folder: folder.to_path_buf(),
        }
    }

    /// The folder.
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// One of its files: `textures.json`, `atlas.png`, `atlas_map.json`,
    /// `client-entries.zip`.
    pub fn file(&self, name: &str) -> PathBuf {
        self.folder.join(name)
    }
}

/// What the renderer draws with: the shipped data's files and the bake's.
pub fn assets(data: &Data, textures: &Textures) -> mcbench_render::Assets {
    mcbench_render::Assets {
        pack: data.file("mcbench-pack.bin"),
        textures: textures.file("textures.json"),
        atlas: textures.file("atlas.png"),
        atlas_map: textures.file("atlas_map.json"),
        block_table: data.file("block-table.tsv"),
        render_layers: data.file("render-layers.tsv"),
        entities: data.file("mcbench-entities.json"),
        night: data.file("night.json"),
        software_vulkan: software_vulkan(),
        client_jar: textures.file("client-entries.zip"),
    }
}

/// The software renderer a machine without a graphics adapter draws on:
/// SwiftShader's Vulkan driver, which the Linux x86_64 payload holds beside
/// the program (`libvk_swiftshader.so`, the file today's renderer release
/// ships; `native/packaging`). Nowhere else: Windows has its own software
/// adapter, macOS has none, and no driver is built for ARM Linux. The file
/// is opened only where no hardware adapter is found; a payload without it
/// fails there with the file's name.
fn software_vulkan() -> Option<PathBuf> {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return None;
    }
    let program = std::env::current_exe().expect("this program's own path");
    Some(
        program
            .parent()
            .expect("a program is in a folder")
            .join("libvk_swiftshader.so"),
    )
}

/// Everything ready except, maybe, the model's shell.
pub struct Ready {
    /// The shipped data (the engine's tables are in its folder).
    pub data: Data,
    /// The baked textures.
    pub textures: Textures,
    /// The renderer's files.
    pub assets: mcbench_render::Assets,
    /// The shell's work, when this start began it (it was not done or under
    /// way): it ends with the shell step.
    pub shell: Option<tokio::task::JoinHandle<Result<(), String>>>,
}

/// Runs every step on `folder` (the data folder) and answers once every step
/// that is not optional is done; the model's shell goes on in the
/// background. `data` makes the shipped data (`Data::shipped` in the app);
/// `steps` tells the setup screen. `shell` is the shell step's work. Called
/// again after a step failed (the retry), it runs the steps again: what is
/// done stays done.
pub async fn start(
    steps: Arc<Steps>,
    folder: PathBuf,
    data: Box<dyn FnOnce() -> Result<Data, String> + Send>,
    client: reqwest::Client,
    shell: Shell,
    cancel: CancellationToken,
) -> Result<Ready, String> {
    let (_, list) = steps.view();
    let shell_state = list
        .iter()
        .find(|s| s.id == "shell")
        .map(|s| s.state.clone())
        .ok_or("No setup step shell")?;
    let shell = match shell_state {
        StepState::Running { .. } | StepState::Done { .. } => None,
        _ => {
            let steps = steps.clone();
            Some(tokio::spawn(
                async move { steps.track("shell", shell).await },
            ))
        }
    };
    let (data, textures) = data_and_textures(&steps, &folder, data, &client, cancel).await?;
    let assets = assets(&data, &textures);
    Ok(Ready {
        data,
        textures,
        assets,
        shell,
    })
}

/// The data step and the textures step of [`start`], in that order: the
/// shipped data `data` makes (on the blocking pool), then its textures in
/// `folder`, fetched and baked unless they are there. `mcbench-lite
/// headless setup` runs these two alone: the model's shell is the app's.
pub async fn data_and_textures(
    steps: &Steps,
    folder: &Path,
    data: Box<dyn FnOnce() -> Result<Data, String> + Send>,
    client: &reqwest::Client,
    cancel: CancellationToken,
) -> Result<(Data, Textures), String> {
    let data = steps
        .track("data", async {
            tokio::task::spawn_blocking(data)
                .await
                .map_err(|e| format!("Unpacking the data failed: {e}"))?
                .map_err(StepError::Failed)
        })
        .await?;
    let textures = steps
        .track("textures", textures(steps, folder, &data, client, cancel))
        .await?;
    Ok((data, textures))
}

/// The textures of `data` in `folder`, fetched and baked unless they are
/// there.
pub async fn textures(
    steps: &Steps,
    folder: &Path,
    data: &Data,
    client: &reqwest::Client,
    cancel: CancellationToken,
) -> Result<Textures, StepError> {
    let options = Options::of(&data.manifest()?);
    make_textures(
        folder,
        data,
        client,
        &options,
        &|bytes| {
            let _ = steps.progress("textures", bytes);
        },
        &cancel,
    )
    .await
    .map_err(StepError::Failed)
}

/// [`textures`] with the fetch's options and a progress callback.
pub async fn make_textures(
    folder: &Path,
    data: &Data,
    client: &reqwest::Client,
    options: &Options,
    progress: &(dyn Fn(crate::steps::Bytes) + Send + Sync),
    cancel: &CancellationToken,
) -> Result<Textures, String> {
    let id = data.id();
    let parent = folder.join("textures");
    let target = parent.join(&id);
    if !target.is_dir() {
        let manifest = data.manifest()?;
        let fetching = parent.join(format!("{id}.fetch"));
        let fetched = fetch::fetch(client, &manifest, options, &fetching, progress, cancel).await?;
        let staging = parent.join(format!("{id}.{}.partial", std::process::id()));
        let target_ = target.clone();
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let threads = std::thread::available_parallelism()
                .map_or(4, |n| n.get())
                .min(8);
            let files = Files::decode(fetched.bytes, threads)?;
            let baked = bake(&manifest, &files)?;
            let entries: Vec<(&crate::jar::Entry, &[u8])> = manifest
                .entries
                .iter()
                .map(|e| (e, fetched.stored[&e.name].as_slice()))
                .collect();
            let zip = write_zip(&entries);
            remove(&staging)?;
            std::fs::create_dir_all(&staging).map_err(|e| format!("{}: {e}", staging.display()))?;
            for (name, bytes) in [
                ("textures.json", &baked.textures_json),
                ("atlas.png", &baked.atlas_png),
                ("atlas_map.json", &baked.atlas_map),
                ("client-entries.zip", &zip),
            ] {
                let file = staging.join(name);
                std::fs::write(&file, bytes).map_err(|e| format!("{}: {e}", file.display()))?;
            }
            std::fs::rename(&staging, &target_).map_err(|e| format!("{}: {e}", target_.display()))
        })
        .await
        .map_err(|e| format!("The texture bake failed: {e}"))??;
    }
    for entry in std::fs::read_dir(&parent).map_err(|e| format!("{}: {e}", parent.display()))? {
        let entry = entry.map_err(|e| format!("{}: {e}", parent.display()))?;
        if entry.file_name() != id.as_str() {
            remove(&entry.path())?;
        }
    }
    Ok(Textures { folder: target })
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_software_renderer_is_named_beside_the_program_on_linux_x86_64_only() {
        let named = super::software_vulkan();
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            let program = std::env::current_exe().unwrap();
            assert_eq!(
                named,
                Some(program.parent().unwrap().join("libvk_swiftshader.so"))
            );
        } else {
            assert_eq!(named, None);
        }
    }
}
