//! A user message's parts (`web/app.js` 5827-5884): its files (a picture
//! is a button that opens the lightbox, any other file opens with the
//! system), its selections as chips, and its tools: the branch arrows with
//! their counter (`Previous branch`, `<i> / <n>`, `Next branch`), `Copy
//! message` (the page's clipboard form: the text with the message beside
//! it, `state::clipboard`) and `Edit and branch`. Each is a part of the
//! current message: Tab walks them, Enter and Space press them.

use std::sync::Arc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::icons::{self, Icon};
use super::super::{Activate, colour, ring};
use super::{ChatPane, Picture};
use crate::state::draft::{self, Picked};
use crate::state::rows::{MessageTools, Piece, Row};
use crate::state::transcript::{FileSource, Item, ShownFile};
use crate::{Command, MessageId, OutgoingFile};

/// How long a copied message's icon is a check (`setCopied`, 1200 ms).
pub const COPIED_FOR: std::time::Duration = std::time::Duration::from_millis(1200);

type ToolPress = fn(&mut ChatPane, &str, &mut Window, &mut Context<ChatPane>);

impl ChatPane {
    /// A user row's files, selections and tools.
    pub(crate) fn user_parts(
        &mut self,
        row: &Row,
        current: bool,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>, AnyElement) {
        let mut files = None;
        let mut selections = None;
        let mut tools = None;
        for piece in &row.pieces {
            match piece {
                Piece::Files(list) => files = Some(self.message_files(&row.key, list, current, cx)),
                Piece::Selections(list) => {
                    selections = Some(
                        div()
                            .mt(px(8.))
                            .flex()
                            .flex_wrap()
                            .gap(px(5.))
                            .children(
                                list.iter()
                                    .map(|s| super::attachments::selection_chip(s, None, cx)),
                            )
                            .into_any_element(),
                    )
                }
                Piece::Tools(state) => {
                    tools = Some(self.message_tools(&row.key, state, current, cx))
                }
                _ => {}
            }
        }
        (files, selections, tools.expect("a user row has its tools"))
    }

    /// `.msg-attachments`: a picture's thumbnail and name (a button that
    /// opens the lightbox), any other file's icon and name (opens it).
    fn message_files(
        &mut self,
        key: &str,
        files: &[ShownFile],
        current: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut strip = div().mt(px(8.)).flex().flex_wrap().gap(px(6.));
        for (index, file) in files.iter().enumerate() {
            let focus = self
                .parts
                .entry(format!("{key}/file{index}"))
                .or_insert_with(|| cx.focus_handle())
                .clone()
                .tab_stop(current);
            let picture = file_picture(file, &*self.options.core);
            let open = {
                let file = file.clone();
                move |this: &mut ChatPane, window: &mut Window, cx: &mut Context<ChatPane>| {
                    this.open_file(&file, window, cx)
                }
            };
            let open_key = open.clone();
            let tile: AnyElement = match (&picture, file.is_image()) {
                (Ok(picture), true) => picture_element(picture.clone())
                    .size(px(30.))
                    .rounded(px(5.))
                    .into_any_element(),
                (Err(sentence), true) => div()
                    .size(px(30.))
                    .rounded(px(5.))
                    .bg(colour::raised())
                    .id(("missing", index))
                    .aria_description(sentence.clone())
                    .into_any_element(),
                _ => file_tile(15.),
            };
            let name = match &picture {
                Err(sentence) => format!("{}, {sentence}", file.name),
                Ok(_) => file.name.clone(),
            };
            strip = strip.child(
                ring(
                    div()
                        .id(SharedString::from(format!("{key}-file-{index}")))
                        .role(if file.is_image() {
                            Role::Button
                        } else {
                            Role::Link
                        })
                        .aria_label(name)
                        .track_focus(&focus)
                        .key_context("Part")
                        .on_action(cx.listener(move |this, _: &Activate, window, cx| {
                            open_key(this, window, cx)
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| open(this, window, cx))),
                )
                .flex()
                .items_center()
                .gap(px(7.))
                .max_w(px(220.))
                .min_h(px(38.))
                .pl(px(4.))
                .pr(px(8.))
                .py(px(4.))
                .rounded(px(8.))
                .bg(colour::well())
                .border_color(colour::line())
                .text_size(px(11.))
                .text_color(colour::ash())
                .cursor_pointer()
                .hover(|s| s.text_color(colour::bone()))
                .child(tile)
                .child(div().min_w_0().truncate().child(file.name.clone())),
            );
        }
        strip.into_any_element()
    }

    /// A message's file pressed: a picture opens in the lightbox, any other
    /// file with the system (the page opened its address in a new tab).
    fn open_file(&mut self, file: &ShownFile, window: &mut Window, cx: &mut Context<Self>) {
        self.log("file activated", serde_json::json!({ "name": file.name }));
        match file_picture(file, &*self.options.core) {
            Ok(picture) if file.is_image() => self.open_lightbox(picture, window, cx),
            Ok(Picture::File(path)) => {
                let url = format!("file://{}", path.display());
                (self.options.opener)(&url, cx);
            }
            Ok(Picture::Bytes(_)) => {}
            Err(sentence) => self.notice(sentence, window, cx),
        }
    }

    /// `.msg-tools`: the branch picker when the message has versions, `Copy
    /// message`, and `Edit and branch` when it can be edited. Shown on hover
    /// or while the message is current.
    fn message_tools(
        &mut self,
        key: &str,
        tools: &MessageTools,
        current: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut row = div()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(2.))
            .opacity(if current { 1. } else { 0. })
            .group_hover(SharedString::from(format!("row-{key}")), |s| s.opacity(1.));
        if let Some(branch) = &tools.branch {
            let previous = self.tool_button(
                key,
                "previous",
                Icon::Back,
                "Previous branch",
                tools.arrows_disabled || branch.previous.is_none(),
                current,
                |this, key, window, cx| this.step_branch(key, false, window, cx),
                cx,
            );
            let next = self.tool_button(
                key,
                "next",
                Icon::Forward,
                "Next branch",
                tools.arrows_disabled || branch.next.is_none(),
                current,
                |this, key, window, cx| this.step_branch(key, true, window, cx),
                cx,
            );
            row = row.child(
                div()
                    .id(SharedString::from(format!("{key}-branch")))
                    .role(Role::Group)
                    .aria_label(format!(
                        "Conversation branch, {} of {}",
                        branch.index + 1,
                        branch.count
                    ))
                    .mr(px(3.))
                    .flex()
                    .items_center()
                    .gap(px(1.))
                    .child(previous)
                    .child(
                        div()
                            .text_size(px(10.))
                            .line_height(px(10.))
                            .text_color(colour::dim())
                            .font_family(self.fonts.mono.clone())
                            .child(format!("{} / {}", branch.index + 1, branch.count)),
                    )
                    .child(next),
            );
        }
        let copy = self.tool_button(
            key,
            "copy",
            if tools.copied {
                Icon::Check
            } else {
                Icon::Copy
            },
            "Copy message",
            false,
            current,
            |this, key, window, cx| this.copy_message(key, window, cx),
            cx,
        );
        row = row.child(copy);
        if tools.can_edit {
            let edit = self.tool_button(
                key,
                "edit",
                Icon::Compose,
                "Edit and branch",
                tools.edit_disabled,
                current,
                |this, key, window, cx| this.edit_message(key, window, cx),
                cx,
            );
            row = row.child(edit);
        }
        row.into_any_element()
    }

    /// `.mtool`: 27 by 27, the icon at 13 px, `--dim-text`; hover `--bone` on
    /// `--well`; disabled it takes no press and is no Tab stop.
    #[allow(clippy::too_many_arguments)]
    fn tool_button(
        &mut self,
        key: &str,
        part: &str,
        icon: Icon,
        label: &str,
        disabled: bool,
        current: bool,
        press: ToolPress,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let focus = self
            .parts
            .entry(format!("{key}/{part}"))
            .or_insert_with(|| cx.focus_handle())
            .clone()
            .tab_stop(current && !disabled);
        let (key_for_action, key_for_click) = (key.to_string(), key.to_string());
        ring(
            div()
                .id(SharedString::from(format!("{key}-{part}")))
                .role(Role::Button)
                .aria_label(label.to_string())
                .track_focus(&focus)
                .key_context("Part")
                .a11y_synthetic_children(move |builder| {
                    if disabled {
                        builder.parent_node().set_disabled();
                    }
                })
                .when(!disabled, |d| {
                    d.on_action(cx.listener(move |this, _: &Activate, window, cx| {
                        press(this, &key_for_action, window, cx)
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        press(this, &key_for_click, window, cx)
                    }))
                    .cursor_pointer()
                    .hover(|s| s.text_color(colour::bone()).bg(colour::well()))
                }),
        )
        .size(px(27.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .text_color(colour::dim())
        .when(disabled, |d| d.opacity(0.28))
        .child(icons::icon(icon, 13.))
        .into_any_element()
    }

    /// The user message of row `key`, from its pieces.
    fn user_item(&self, key: &str) -> Item {
        let row = self
            .rows
            .iter()
            .find(|r| r.key == key)
            .unwrap_or_else(|| panic!("no row {key}"));
        let (mut text, mut message_id, mut files, mut selections) =
            (String::new(), None, Vec::new(), Vec::new());
        for piece in &row.pieces {
            match piece {
                Piece::Text(t) => text = t.clone(),
                Piece::Files(f) => files = f.clone(),
                Piece::Selections(s) => selections = s.clone(),
                Piece::Tools(tools) => message_id = tools.message_id.clone(),
                other => panic!("row {key} is not a user message: {other:?}"),
            }
        }
        Item::User {
            text,
            message_id,
            files,
            selections,
        }
    }

    /// `Copy message` (`copyMessage`): the text, with the message and its
    /// files beside it for a paste into this app; the icon is a check for
    /// 1.2 s. A file that cannot be read is a notice and nothing is copied.
    pub(crate) fn copy_message(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Item::User { text, files, .. } = self.user_item(key) else {
            unreachable!("a user item")
        };
        let core = self.options.core.clone();
        let key = key.to_string();
        cx.spawn_in(window, async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move {
                    files
                        .iter()
                        .map(|file| {
                            read_back(file, &*core)
                                .map_err(|why| format!("Could not copy \"{}\" ({why})", file.name))
                        })
                        .collect::<Result<Vec<OutgoingFile>, String>>()
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match read {
                Ok(files) => {
                    let metadata = crate::state::clipboard::message_metadata(&text, &files);
                    cx.write_to_clipboard(ClipboardItem::new_string_with_json_metadata(
                        text.clone(),
                        metadata,
                    ));
                    this.log(
                        "copied message",
                        serde_json::json!({ "text": text, "files": files.len() }),
                    );
                    this.copies += 1;
                    let copy = this.copies;
                    this.copied = Some((key, copy));
                    this.rebuild(cx);
                    cx.notify();
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(COPIED_FOR).await;
                        let _ = this.update(cx, |this, cx| {
                            if this.copied.as_ref().is_some_and(|(_, n)| *n == copy) {
                                this.copied = None;
                                this.rebuild(cx);
                                cx.notify();
                            }
                        });
                    })
                    .detach();
                }
                Err(error) => this.notice(error, window, cx),
            });
        })
        .detach();
    }

    /// `Edit and branch` (`editPrompt`): the message comes into the
    /// composer with its selections and its files, read back (each a batch
    /// entry until read); nothing else changes until it is sent. A file that
    /// cannot be read back is left out and named in a notice; the others
    /// stay (today's page dropped them all: a defect).
    pub(crate) fn edit_message(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Item::User {
            text,
            message_id,
            files,
            selections,
        } = self.user_item(key)
        else {
            unreachable!("a user item")
        };
        let of = message_id.expect("Edit and branch is offered for a stored message");
        let batch: Vec<Picked> = files
            .iter()
            .map(|file| {
                self.picks += 1;
                Picked {
                    id: self.picks,
                    name: file.name.clone(),
                    size: file.size,
                }
            })
            .collect();
        let now = self.current_draft(cx);
        let edit = draft::begin_edit(&now, &of, text, selections, batch.clone());
        self.log("edit", serde_json::json!({ "of": of }));
        self.put_draft(edit, window, cx);
        self.caret_at_end(window, cx);
        self.rebuild(cx);
        cx.notify();
        let core = self.options.core.clone();
        cx.spawn_in(window, async move |this, cx| {
            let read: Vec<Result<OutgoingFile, String>> = cx
                .background_executor()
                .spawn(async move {
                    files
                        .iter()
                        .map(|file| {
                            read_back(file, &*core)
                                .map_err(|why| format!("Could not load \"{}\" ({why})", file.name))
                        })
                        .collect()
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                let mut kept = Vec::new();
                for result in read {
                    match result {
                        Ok(file) => kept.push(file),
                        Err(error) => this.notice(error, window, cx),
                    }
                }
                this.finish_read(&batch, kept, cx);
            });
        })
        .detach();
    }

    /// Cancel (or Escape) of an edit (`cancelEdit`): what the composer held
    /// before, the caret at its end.
    pub(crate) fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let back = draft::end_edit(&self.current_draft(cx));
        self.log("cancel edit", serde_json::json!({}));
        self.put_draft(back, window, cx);
        self.caret_at_end(window, cx);
        self.rebuild(cx);
        cx.notify();
    }

    /// An arrow of the branch picker (`switchBranch`): only with a chat on
    /// screen and no turn running; the arrows wait until the branch's
    /// transcript arrives, which ends at its newest message.
    fn step_branch(&mut self, key: &str, next: bool, _: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.open.clone() else {
            return;
        };
        if self.busy(cx) || self.branching {
            return;
        }
        let Item::User { message_id, .. } = self.user_item(key) else {
            unreachable!("a user item")
        };
        let branch = message_id
            .as_ref()
            .and_then(|id| self.branches.get(id))
            .expect("the arrows are on a message with versions");
        let Some(target) = (if next { &branch.next } else { &branch.previous }).clone() else {
            return;
        };
        self.log("branch", serde_json::json!({ "message": target }));
        self.branching = true;
        self.pin_next = true;
        self.options.core.send(Command::Branch {
            chat,
            message: MessageId(target),
        });
        self.rebuild(cx);
        cx.notify();
    }
}

/// A file's bytes for sending again or copying: a sent one's own, a stored
/// one's through the core; `Err` is why not.
pub fn read_back(file: &ShownFile, core: &dyn crate::Core) -> Result<OutgoingFile, String> {
    let data: Arc<[u8]> = match &file.source {
        FileSource::Sent(bytes) => bytes.clone(),
        FileSource::Stored(address) => {
            let found = crate::state::files::attachment(address, core)?;
            Arc::from(std::fs::read(&found.path).map_err(|e| e.to_string())?)
        }
    };
    Ok(OutgoingFile {
        name: file.name.clone(),
        content_type: file.content_type.clone(),
        size: file.size,
        data,
    })
}

/// What draws a message's file: a sent one's bytes, a stored one's file.
pub fn file_picture(file: &ShownFile, core: &dyn crate::Core) -> Result<Picture, String> {
    match &file.source {
        FileSource::Sent(bytes) => Ok(bytes_picture(&file.content_type, bytes)),
        FileSource::Stored(address) => {
            crate::state::files::attachment(address, core).map(|found| Picture::File(found.path))
        }
    }
}

/// A picture of bytes of type `content_type` (a JPEG when unknown: every
/// redrawn attachment is one).
/// Each set of bytes makes its image once (a redraw does not copy and
/// hash the bytes again); the cache keeps the bytes it is keyed by.
pub fn bytes_picture(content_type: &str, bytes: &Arc<[u8]>) -> Picture {
    type Cache = std::collections::HashMap<usize, (Arc<[u8]>, Arc<Image>)>;
    thread_local! {
        static IMAGES: std::cell::RefCell<Cache> = Default::default();
    }
    let key = bytes.as_ptr() as usize;
    IMAGES.with(|images| {
        let mut images = images.borrow_mut();
        if let Some((kept, image)) = images.get(&key)
            && Arc::ptr_eq(kept, bytes)
        {
            return Picture::Bytes(image.clone());
        }
        // The pictures of a draft and of the messages just sent: few.
        if images.len() >= 64 {
            images.clear();
        }
        let format = ImageFormat::from_mime_type(content_type).unwrap_or(ImageFormat::Jpeg);
        let image = Arc::new(Image::from_bytes(format, bytes.to_vec()));
        images.insert(key, (bytes.clone(), image.clone()));
        Picture::Bytes(image)
    })
}

/// A picture as an element.
pub fn picture_element(picture: Picture) -> Img {
    match picture {
        Picture::File(path) => img(path),
        Picture::Bytes(image) => img(image),
    }
}

/// `.file-tile`: a file's icon in a 30 by 30 tile.
pub fn file_tile(size: f32) -> AnyElement {
    div()
        .size(px(30.))
        .flex_none()
        .rounded(px(6.))
        .bg(colour::well())
        .text_color(colour::ash())
        .flex()
        .items_center()
        .justify_center()
        .child(icons::icon(Icon::File, size))
        .into_any_element()
}
