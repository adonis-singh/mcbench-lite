//! The composer's files and selections (`web/app.js` 5413-5469, 6928-6957):
//! files come from the picker (`Attach file`, the platform's file dialog)
//! and from a paste (files or pictures on the clipboard, or a message
//! copied from this app, which brings its text and files back). A pick is
//! counted toward the limits (`state::upload`) at once, with every file the
//! draft holds or still reads; the draft then reads the batch (each file a
//! `Reading…` tile, and no send meanwhile) off the interface thread, a
//! picture redrawn as JPEG on white; a failed read ends the batch with no
//! files and says why. The read lands in the draft that holds the batch,
//! on screen or parked. Each attachment has `Remove attachment`; each
//! selection a chip, with `Remove <label>` in the composer.

use std::path::PathBuf;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::icons::{self, Icon};
use super::super::{Activate, colour, ring};
use super::ChatPane;
use super::message::{bytes_picture, file_tile, picture_element};
use crate::OutgoingFile;
use crate::state::draft::{self, Picked};
use crate::state::upload;

impl ChatPane {
    /// `Attach file`: the picker's files join the draft.
    pub(crate) fn pick_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = (self.options.picker)(cx);
        self.log("attach", serde_json::json!({}));
        cx.spawn_in(window, async move |this, cx| {
            let paths = picked.await.unwrap_or(Ok(Vec::new()));
            let _ = this.update_in(cx, |this, window, cx| match paths {
                Ok(paths) => this.add_paths(paths, window, cx),
                Err(error) => this.notice(error, window, cx),
            });
        })
        .detach();
    }

    /// Files by path join the draft (`addFiles`): their sizes are read,
    /// counted toward the limits, and the files read as one batch.
    pub fn add_paths(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        cx.spawn_in(window, async move |this, cx| {
            let sized: Result<Vec<(PathBuf, String, u64)>, String> = cx
                .background_executor()
                .spawn(async move {
                    paths
                        .into_iter()
                        .map(|path| {
                            let name = path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|| path.display().to_string());
                            let size = std::fs::metadata(&path)
                                .map_err(|e| format!("\"{name}\" could not be read: {e}"))?
                                .len();
                            Ok((path, name, size))
                        })
                        .collect()
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match sized {
                Ok(files) => this.read_batch(files, window, cx),
                Err(error) => this.notice(error, window, cx),
            });
        })
        .detach();
    }

    fn read_batch(
        &mut self,
        files: Vec<(PathBuf, String, u64)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let added: Vec<(String, u64)> = files.iter().map(|(_, n, s)| (n.clone(), *s)).collect();
        if let Err(error) =
            upload::check_attachment_limits(&draft::draft_files(&self.draft), &added)
        {
            return self.notice(error, window, cx);
        }
        let batch: Vec<Picked> = files
            .iter()
            .map(|(_, name, size)| {
                self.picks += 1;
                Picked {
                    id: self.picks,
                    name: name.clone(),
                    size: *size,
                }
            })
            .collect();
        self.draft = draft::begin_read(&self.draft, &batch);
        self.log(
            "reading",
            serde_json::json!({ "files": added.iter().map(|(n, _)| n).collect::<Vec<_>>() }),
        );
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let read: Result<Vec<OutgoingFile>, String> = cx
                .background_executor()
                .spawn(async move {
                    files
                        .iter()
                        .map(|(path, ..)| upload::read_path(path))
                        .collect()
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match read {
                Ok(files) => this.finish_read(&batch, files, cx),
                Err(error) => {
                    this.finish_read(&batch, Vec::new(), cx);
                    this.notice(error, window, cx);
                }
            });
        })
        .detach();
    }

    /// A batch's read is over (`finishRead`): its files take its place in
    /// the draft that holds it, on screen or parked; a draft that is gone
    /// took the batch with it.
    pub(crate) fn finish_read(
        &mut self,
        batch: &[Picked],
        files: Vec<OutgoingFile>,
        cx: &mut Context<Self>,
    ) {
        if draft::holds_read(&self.draft, batch) {
            self.draft = draft::end_read(&self.draft, batch, files.clone());
        }
        self.drafts.end_read(batch, &files);
        self.log(
            "read",
            serde_json::json!({ "files": files.iter().map(|f| &f.name).collect::<Vec<_>>() }),
        );
        cx.notify();
    }

    /// A paste in the composer (`onPaste`): a message copied from this app
    /// brings its text and files (after the limits); files or pictures on
    /// the clipboard join the draft; otherwise the composer pastes the text
    /// alone. Returns whether it took the paste.
    pub(crate) fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(item) = cx.read_from_clipboard() else {
            return false;
        };
        for entry in item.entries() {
            if let ClipboardEntry::String(string) = entry
                && let Some(metadata) = string.metadata_json::<serde_json::Value>()
            {
                match crate::state::clipboard::read_message(&metadata) {
                    Ok(Some((text, files))) => {
                        let added: Vec<(String, u64)> =
                            files.iter().map(|f| (f.name.clone(), f.size)).collect();
                        if let Err(error) = upload::check_attachment_limits(
                            &draft::draft_files(&self.draft),
                            &added,
                        ) {
                            self.notice(error, window, cx);
                            return true;
                        }
                        self.composer.update(cx, |c, cx| c.insert(text, window, cx));
                        self.draft.attachments.extend(files);
                        self.log(
                            "pasted message",
                            serde_json::json!({ "files": added.len() }),
                        );
                        cx.notify();
                        return true;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        self.notice(error, window, cx);
                        return true;
                    }
                }
            }
        }
        let mut paths = Vec::new();
        let mut pictures = Vec::new();
        for entry in item.entries() {
            match entry {
                ClipboardEntry::ExternalPaths(external) => {
                    paths.extend(external.paths().iter().cloned())
                }
                ClipboardEntry::Image(image) => pictures.push(image.clone()),
                ClipboardEntry::String(_) => {}
            }
        }
        if !paths.is_empty() {
            self.add_paths(paths, window, cx);
            return true;
        }
        if !pictures.is_empty() {
            self.add_pictures(pictures, window, cx);
            return true;
        }
        false
    }

    /// Pictures pasted from the clipboard join the draft as a browser hands
    /// them over: a PNG named `image.png` whatever the platform's format
    /// (`upload::pasted_picture`), then redrawn as any picked picture.
    fn add_pictures(&mut self, pictures: Vec<Image>, window: &mut Window, cx: &mut Context<Self>) {
        let added: Vec<(String, u64)> = pictures
            .iter()
            .map(|p| (upload::PASTED_NAME.to_string(), p.bytes.len() as u64))
            .collect();
        if let Err(error) =
            upload::check_attachment_limits(&draft::draft_files(&self.draft), &added)
        {
            return self.notice(error, window, cx);
        }
        let batch: Vec<Picked> = added
            .iter()
            .map(|(name, size)| {
                self.picks += 1;
                Picked {
                    id: self.picks,
                    name: name.clone(),
                    size: *size,
                }
            })
            .collect();
        self.draft = draft::begin_read(&self.draft, &batch);
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let read: Result<Vec<OutgoingFile>, String> = cx
                .background_executor()
                .spawn(async move {
                    pictures
                        .into_iter()
                        .zip(added)
                        .map(|(picture, _)| {
                            let (name, png) =
                                upload::pasted_picture(picture.format.mime_type(), picture.bytes)?;
                            upload::read_upload(&name, Some("image/png"), png)
                        })
                        .collect()
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match read {
                Ok(files) => this.finish_read(&batch, files, cx),
                Err(error) => {
                    this.finish_read(&batch, Vec::new(), cx);
                    this.notice(error, window, cx);
                }
            });
        })
        .detach();
    }

    /// `.composer-selections` and `.composer-attachments`: the draft's
    /// selection chips, then a tile per attachment and per file being read.
    pub(crate) fn composer_parts(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut parts = Vec::new();
        if !self.draft.selections.is_empty() {
            let mut row = div()
                .flex()
                .flex_wrap()
                .gap(px(5.))
                .pt(px(1.))
                .px(px(1.))
                .pb(px(4.));
            for selection in self.draft.selections.clone() {
                let letter = selection.letter.clone();
                let this = cx.entity().downgrade();
                let remove: super::super::Press = std::rc::Rc::new(move |_, cx| {
                    let letter = letter.clone();
                    let _ = this.update(cx, |this, cx| {
                        this.draft.selections.retain(|s| s.letter != letter);
                        this.log("removed selection", serde_json::json!({ "letter": letter }));
                        cx.notify();
                    });
                });
                let focus = self.part_focus(&format!("selection-{}", selection.letter), cx);
                row = row.child(selection_chip(&selection, Some((remove, focus)), cx));
            }
            parts.push(row.into_any_element());
        }
        if self.draft.attachments.is_empty() && self.draft.reading.is_empty() {
            return parts;
        }
        let mut tiles = div().flex().flex_wrap().gap(px(6.)).p(px(1.));
        for (index, file) in self.draft.attachments.clone().into_iter().enumerate() {
            let kind = file
                .content_type
                .rsplit('/')
                .next()
                .filter(|k| !k.is_empty())
                .unwrap_or("file")
                .to_string();
            let thumb = if file.content_type.starts_with("image/") {
                picture_element(bytes_picture(&file.content_type, &file.data))
                    .size(px(30.))
                    .rounded(px(6.))
                    .into_any_element()
            } else {
                file_tile(16.)
            };
            let remove = ring(
                div()
                    .id(("remove-attachment", index))
                    .role(Role::Button)
                    .aria_label("Remove attachment")
                    .track_focus(&self.part_focus(&format!("attachment{index}"), cx))
                    .key_context("Part")
                    .on_action(cx.listener(move |this, _: &Activate, _, cx| {
                        this.remove_attachment(index, cx)
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| this.remove_attachment(index, cx))),
            )
            .size(px(22.))
            .rounded(px(5.))
            .flex()
            .items_center()
            .justify_center()
            .text_color(colour::dim())
            .cursor_pointer()
            .hover(|s| s.text_color(colour::bone()).bg(colour::well()))
            .child(icons::icon(Icon::X, 12.));
            tiles = tiles.child(attachment_tile(
                ("attachment", index),
                format!("{}, {kind}", file.name),
                thumb,
                &file.name,
                &kind,
                Some(remove.into_any_element()),
                false,
                &self.fonts,
            ));
        }
        for (index, picked) in self.draft.reading.clone().into_iter().enumerate() {
            tiles = tiles.child(attachment_tile(
                ("reading", index),
                format!("{}, Reading\u{2026}", picked.name),
                div()
                    .size(px(30.))
                    .rounded(px(6.))
                    .bg(colour::well())
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(3.))
                    .children((0..3).map(|_| div().size(px(4.)).rounded_full().bg(colour::ash())))
                    .into_any_element(),
                &picked.name,
                "Reading\u{2026}",
                None,
                true,
                &self.fonts,
            ));
        }
        parts.push(tiles.into_any_element());
        parts
    }

    /// A composer part's focus handle: a Tab stop in the composer's order.
    pub(crate) fn part_focus(&mut self, name: &str, cx: &mut Context<Self>) -> FocusHandle {
        self.parts
            .entry(format!("composer/{name}"))
            .or_insert_with(|| cx.focus_handle())
            .clone()
            .tab_stop(true)
    }

    fn remove_attachment(&mut self, index: usize, cx: &mut Context<Self>) {
        let file = self.draft.attachments.remove(index);
        self.log(
            "removed attachment",
            serde_json::json!({ "name": file.name }),
        );
        cx.notify();
    }
}

/// `.composer-attachment`: 30 px thumbnail, name (`11px`, `520`, `--bone`,
/// ellipsized) and kind (data `9px`, uppercase, `--dim-text`), and its
/// remove button; while reading, a dashed border and no fill. Its node is
/// named by the file and its state.
#[allow(clippy::too_many_arguments)]
fn attachment_tile(
    id: impl Into<ElementId>,
    name: String,
    thumb: AnyElement,
    file: &str,
    kind: &str,
    remove: Option<AnyElement>,
    reading: bool,
    fonts: &super::super::markdown::Fonts,
) -> AnyElement {
    div()
        .id(id)
        .role(if reading { Role::Status } else { Role::Group })
        .aria_label(name)
        .w(px(220.))
        .min_h(px(42.))
        .p(px(5.))
        .flex()
        .items_center()
        .gap(px(7.))
        .rounded(px(8.))
        .border_1()
        .border_color(colour::line())
        .when(reading, |d| d.border_dashed())
        .when(!reading, |d| d.bg(colour::raised()))
        .child(thumb)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight(520.))
                        .text_color(colour::bone())
                        .truncate()
                        .child(file.to_string()),
                )
                .child(
                    div()
                        .text_size(px(9.))
                        .font_family(fonts.mono.clone())
                        .text_color(colour::dim())
                        .child(kind.to_uppercase()),
                ),
        )
        .children(remove)
        .into_any_element()
}

/// `SelectionChip`: the block's icon
/// ([`crate::view::block_icons::chip_square`]: an empty square for empty
/// space or when the core has none), its label and its size
/// (`<x>×<y>×<z>`), titled `<blocks> blocks`; with `remove`, the composer's
/// chip and its `Remove <label>` button. Named by its label and size.
pub fn selection_chip(
    selection: &crate::Selection,
    remove: Option<(super::super::Press, FocusHandle)>,
    cx: &mut App,
) -> AnyElement {
    let size = (0..3)
        .map(|axis| (selection.max[axis] - selection.min[axis] + 1).to_string())
        .collect::<Vec<_>>()
        .join("\u{d7}");
    let fonts = super::super::controls::fonts(cx);
    let name = format!("{}, {size}", selection.label);
    let chip = div()
        .id(SharedString::from(format!(
            "selection-{}",
            selection.letter
        )))
        .role(Role::Group)
        .aria_label(name)
        .aria_description(format!("{} blocks", selection.blocks))
        // pane agent: hovering a chip lights its box in the 3D pane.
        .on_hover({
            let (min, max) = (selection.min, selection.max);
            move |hovered: &bool, _, cx: &mut App| {
                crate::view::world::chip_hovered(hovered.then_some((min, max)), cx)
            }
        })
        .h(px(28.))
        .pl(px(4.))
        .pr(px(9.))
        .flex()
        .items_center()
        .gap(px(7.))
        .rounded(px(8.))
        .bg(colour::well())
        .border_1()
        .border_color(colour::line())
        .text_size(px(12.))
        .font_family(fonts.ui.clone())
        .text_color(colour::ash())
        // pane agent: the block's icon, or the empty square.
        .child(crate::view::block_icons::chip_square(
            &format!("selection-{}", selection.letter),
            selection.block.as_deref(),
            cx,
        ))
        .child(
            div()
                .text_color(colour::bone())
                .truncate()
                .child(selection.label.clone()),
        )
        .child(div().text_color(colour::dim()).child(size));
    // par-chat: the chip's `title` (`<blocks> blocks`, `web/app.js` 2537)
    // is also its pointer tip, on a box around it (the chip's own hover
    // lights its box in the pane); the description stays the chip's.
    let tip_id = SharedString::from(format!("selection-{}-tip", selection.letter));
    let title = format!("{} blocks", selection.blocks);
    let with_tip = move |chip: Stateful<Div>| {
        super::super::controls::pointer_tip(div().id(tip_id).child(chip), "tip", title)
            .into_any_element()
    };
    match remove {
        None => with_tip(chip),
        Some((remove, focus)) => {
            let on_key = remove.clone();
            // par-chat: a right-click removes the draft's chip
            // (`onContextMenu`, the draft's only).
            let on_right = remove.clone();
            let chip = chip
                .on_mouse_down(MouseButton::Right, move |_, window, cx| {
                    cx.stop_propagation();
                    on_right(window, cx)
                })
                .child(
                    ring(
                        div()
                            .id("remove")
                            .role(Role::Button)
                            .aria_label(format!("Remove {}", selection.label))
                            .track_focus(&focus)
                            .key_context("Part")
                            .on_action(move |_: &Activate, window, cx| on_key(window, cx))
                            .on_click(move |_, window, cx| remove(window, cx)),
                    )
                    .size(px(20.))
                    .mr(px(-5.))
                    .rounded(px(5.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|s| s.text_color(colour::bone()).bg(colour::hover()))
                    .child(icons::icon(Icon::X, 12.)),
                );
            with_tip(chip)
        }
    }
}
