//! The header's Share button and its panel (share agent; `web/app.js`
//! `SharePanel` 2939-2998, `openShare`, `publishShare`, `removeShare`
//! 4313-4359, the button 6734-6760; inventory `components.md` N.4, `app.md`
//! 3.5 and 8.3; the seam is `ui-seam-share.md`).
//!
//! A subject is a Build chat whose world on show holds blocks, or a bench's
//! comparison ([`subject`]); a bench model's own chat has no Share button.
//! One share state per subject ([`crate::state::share`]), so a publish that
//! runs on while another place is on show lands on its own subject. The
//! panel open is remembered by the subject's key (`shareFor`): it shows
//! only while that subject is on screen, and comes back with it.
//!
//! The panel is the `share` layer (not modal): Escape closes it when it is
//! the topmost, as do a press outside it and its button, and `Open
//! Settings`. Focus goes into it when it opens (its first control, or the
//! panel itself while it has none) and back to the Share button when
//! Escape or a control closes it (`inventory/README.md`, share agent).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::*;

use super::controls::{Button, ConfirmButton, Kind, fonts};
use super::icons::Icon;
use super::shell::Shell;
use super::shell::announce::Announcer;
use super::theme::{colour, radius, shadow};
use super::{Opener, ring};
use crate::state::layers::Layer;
use crate::state::place::Place;
use crate::state::share::{self as rules, Phase, Share};
use crate::{ActionLog, Command, Core, PublishStep, Subject, Update};

/// How long `Copy link` reads `Copied` (`useCopy`, 1,500 ms).
pub const COPIED_FOR: Duration = Duration::from_millis(1500);

/// The button's text.
pub const SHARE: &str = "Share";
/// Its title on a chat.
pub const CHAT_TITLE: &str = "Publish this build's 3D view to a public page";
/// Its title on a bench.
pub const BENCH_TITLE: &str = "Publish this bench's builds to a public page";
/// Its title on a bench with a model running (disabled).
pub const BLOCKED_TITLE: &str = "Wait for the bench's running turns before sharing";
/// While the share or the repository is read.
pub const CHECKING: &str = "Checking…";
/// No repository set.
pub const NO_REPO: &str = "Set the GitHub repository for shared builds in Settings.";
/// The page shows it as published.
pub const AS_PUBLISHED: &str = "The page shows the {noun} as it was when published.";
/// A blind bench's note.
pub const BLIND_NOTE: &str = "Names are hidden: the page shows the models as A, B, C. Publish again after revealing to show their names.";

/// What the panel offers to publish.
pub fn offer(bench: bool, repo: &str) -> String {
    if bench {
        format!(
            "Publishes this bench's builds to a public page on {repo}, where anyone can switch between the models' builds. To post on X."
        )
    } else {
        format!("Publishes this build's 3D view to a public page on {repo}, to post on X.")
    }
}

/// What the header's Share button shares (`shareSubject`): the chat on
/// show in Build whose live world is its own, viewable and holding blocks;
/// a bench's comparison; none elsewhere.
pub fn subject(shell: &Shell, cx: &App) -> Option<Subject> {
    match shell.place() {
        Place::Chat { id } | Place::NewChat { id: Some(id) } => {
            let world = shell.world();
            let state = world.read(cx).state()?;
            (state.view.as_ref().is_some_and(|v| v.0 == *id)
                && state.revision.is_some()
                && state.blocks > 0)
                .then(|| Subject::Chat(id.clone()))
        }
        Place::Compare { bench } => Some(Subject::Bench(bench.clone())),
        _ => None,
    }
}

/// A bench with a model still running cannot be shared yet
/// (`shareBlocked`).
fn blocked(shell: &Shell, subject: &Subject, cx: &App) -> bool {
    let Subject::Bench(bench) = subject else {
        return false;
    };
    shell
        .lists_entity()
        .read(cx)
        .all()
        .iter()
        .any(|row| row.summary.bench_id.as_deref() == Some(bench) && row.running_since.is_some())
}

/// A bench that hides its models' names (`hiddenNames`).
fn hides_names(shell: &Shell, subject: &Subject, cx: &App) -> bool {
    let Subject::Bench(bench) = subject else {
        return false;
    };
    shell
        .lists_entity()
        .read(cx)
        .benches()
        .iter()
        .find(|b| b.id == *bench)
        .is_some_and(|b| crate::state::bench::hides_names(b.blind, b.revealed))
}

/// The shares and the open panel.
pub struct SharePanel {
    core: Arc<dyn Core>,
    opener: Opener,
    log: Option<ActionLog>,
    shell: WeakEntity<Shell>,
    announcer: Entity<Announcer>,
    shares: HashMap<String, Share>,
    /// The key of the subject whose panel is open (`shareFor`).
    open: Option<String>,
    /// The saved repository: none until the first read lands
    /// (`undefined`), then the read's (`null` or `owner/name`).
    repo: Option<Option<String>>,
    /// The `share` layer is registered.
    layered: bool,
    /// Focus goes into the panel at its next frame.
    focus_in: bool,
    button: FocusHandle,
    dialog: FocusHandle,
    first: FocusHandle,
    link: Option<(String, Entity<InputState>)>,
    copied: Option<(String, Instant)>,
    button_hovered: bool,
}

struct Installed(Entity<SharePanel>);
impl Global for Installed {}

/// The panel, made the first time the header asks for it.
pub fn installed(
    core: Arc<dyn Core>,
    opener: Opener,
    log: Option<ActionLog>,
    announcer: Entity<Announcer>,
    cx: &mut Context<Shell>,
) -> Entity<SharePanel> {
    if let Some(Installed(panel)) = cx.try_global::<Installed>() {
        return panel.clone();
    }
    let weak = cx.entity().downgrade();
    let panel = cx.new(|cx| SharePanel {
        core,
        opener,
        log,
        shell: weak,
        announcer,
        shares: HashMap::new(),
        open: None,
        repo: None,
        layered: false,
        focus_in: false,
        button: cx.focus_handle(),
        dialog: cx.focus_handle(),
        first: cx.focus_handle(),
        link: None,
        copied: None,
        button_hovered: false,
    });
    cx.set_global(Installed(panel.clone()));
    panel
}

/// The panel when it has been made (the shell's updates go to it).
pub fn panel(cx: &App) -> Option<Entity<SharePanel>> {
    cx.try_global::<Installed>().map(|Installed(p)| p.clone())
}

impl SharePanel {
    /// A subject's share (`shareOf`).
    pub fn share_of(&self, subject: &Subject) -> Share {
        self.shares
            .get(&rules::share_key(subject))
            .cloned()
            .unwrap_or(rules::UNKNOWN)
    }

    /// The key of the subject whose panel is open.
    pub fn open_key(&self) -> Option<&str> {
        self.open.as_deref()
    }

    fn log(&self, action: &str, fields: serde_json::Value) {
        if let Some(log) = &self.log {
            log.record(action, fields);
        }
    }

    fn notice(&self, text: String, cx: &mut App) {
        let _ = self.shell.update(cx, |s, cx| s.notice(text, cx));
    }

    /// Moves `subject`'s share; a move the page's rules refuse is the
    /// rules' sentence as a notice (it is a fault, never a state).
    fn set(&mut self, subject: &Subject, moved: rules::Moved, cx: &mut Context<Self>) {
        match moved {
            Ok(next) => {
                let shown = self.open.as_deref() == Some(rules::share_key(subject).as_str());
                let before = self.share_of(subject);
                if before.phase != next.phase {
                    self.log(
                        "share state",
                        serde_json::json!({ "subject": rules::share_key(subject), "phase": next.phase.name() }),
                    );
                    if shown {
                        self.announce(&next, cx);
                    }
                }
                self.shares.insert(rules::share_key(subject), next);
            }
            Err(text) => self.notice(text, cx),
        }
        cx.notify();
    }

    /// The steps politely, a failure at once (the page's `role="status"`
    /// and `role="alert"`).
    fn announce(&self, share: &Share, cx: &mut Context<Self>) {
        let (text, assertive) = match &share.phase {
            Phase::Failed(error) => (error.clone(), true),
            _ if rules::publishing_now(share) => (
                rules::step_text(share).expect("publishing").to_string(),
                false,
            ),
            _ => return,
        };
        self.announcer.update(cx, |a, cx| {
            if assertive {
                a.assertive(text, cx)
            } else {
                a.polite(text, cx)
            }
        });
    }

    /// Opens `subject`'s panel (`openShare`): the repository is read at
    /// each opening, the share only the first time.
    pub fn open(&mut self, subject: &Subject, cx: &mut Context<Self>) {
        self.open = Some(rules::share_key(subject));
        self.focus_in = true;
        self.log(
            "share opened",
            serde_json::json!({ "subject": rules::share_key(subject) }),
        );
        let share = self.share_of(subject);
        if share.phase == Phase::Unknown {
            self.set(subject, rules::reading(&share), cx);
        }
        self.core.send(Command::ReadShare {
            subject: subject.clone(),
        });
        cx.notify();
    }

    /// Closes the panel (`closeShare`); `refocus` puts focus back on the
    /// Share button.
    pub fn close(&mut self, refocus: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.open.take().is_some() {
            self.log("share closed", serde_json::json!({}));
        }
        self.focus_in = false;
        if refocus {
            window.focus(&self.button, cx);
        }
        cx.notify();
    }

    /// Publish, Try again, Publish again (`publishShare`).
    pub fn publish(&mut self, subject: &Subject, window: &mut Window, cx: &mut Context<Self>) {
        let share = self.share_of(subject);
        self.set(subject, rules::publishing(&share), cx);
        self.log(
            "share publish",
            serde_json::json!({ "subject": rules::share_key(subject) }),
        );
        self.core.send(Command::Publish {
            subject: subject.clone(),
        });
        window.focus(&self.dialog, cx);
    }

    /// Remove, confirmed (`removeShare`).
    pub fn remove(&mut self, subject: &Subject, window: &mut Window, cx: &mut Context<Self>) {
        let share = self.share_of(subject);
        self.set(subject, rules::removing(&share), cx);
        self.log(
            "share remove",
            serde_json::json!({ "subject": rules::share_key(subject) }),
        );
        self.core.send(Command::Unshare {
            subject: subject.clone(),
        });
        window.focus(&self.dialog, cx);
    }

    /// The core's share updates.
    pub fn apply(&mut self, update: Update, cx: &mut Context<Self>) {
        match update {
            Update::ShareRead { subject, read } => {
                let share = self.share_of(&subject);
                match read {
                    Ok(read) => {
                        self.repo = Some(read.repo);
                        // The share is read once; later reads give the
                        // repository only.
                        if share.phase == Phase::Loading {
                            self.set(&subject, rules::read(&share, read.url), cx);
                        }
                    }
                    Err(error) => {
                        if share.phase == Phase::Loading {
                            self.set(&subject, rules::unread(&share), cx);
                        }
                        self.notice(error, cx);
                    }
                }
            }
            Update::PublishStep { subject, step } => {
                let share = self.share_of(&subject);
                // A publish the window did not start here moves the share
                // as a press of Publish would have.
                let share = if step == PublishStep::Exporting
                    && matches!(share.phase, Phase::None | Phase::Shared | Phase::Failed(_))
                {
                    rules::publishing(&share).expect("a publish may start")
                } else {
                    share
                };
                self.set(&subject, rules::step(&share, &step), cx);
            }
            Update::ShareChanged { subject, url } => {
                let share = self.share_of(&subject);
                self.set(&subject, rules::changed(&share, url.as_deref()), cx);
            }
            other => unreachable!("not a share update: {other:?}"),
        }
        cx.notify();
    }

    fn copy(&mut self, url: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(url.to_string()));
        self.log("copied", serde_json::json!({ "text": url }));
        self.copied = Some((url.to_string(), Instant::now()));
        cx.spawn(async |this, cx| {
            cx.background_executor().timer(COPIED_FOR).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        cx.notify();
    }

    fn post(&self, url: &str, cx: &mut App) {
        let post = rules::post_url(url);
        self.log("link activated", serde_json::json!({ "url": post }));
        (self.opener)(&post, cx);
    }

    /// The link field's state for `url`, made once per address.
    fn link_field(
        &mut self,
        url: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some((at, input)) = &self.link
            && at == url
        {
            return input.clone();
        }
        let value = url.to_string();
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(value, window, cx);
            state.set_readonly(true, cx);
            state
        });
        // `onFocus`: selects all.
        let focus = input.read(cx).focus_handle(cx);
        let selected = input.clone();
        cx.on_focus(&focus, window, move |_, window, cx| {
            selected.update(cx, |s, cx| s.select_all(window, cx));
        })
        .detach();
        self.link = Some((url.to_string(), input.clone()));
        input
    }
}

/// The header's Share button with its panel under it, or none when there
/// is nothing to share here.
pub fn button(
    shell: &Shell,
    panel: Entity<SharePanel>,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    let subject = subject(shell, cx);
    let key = subject.as_ref().map(rules::share_key);
    let shown = key.is_some() && panel.read(cx).open.as_deref() == key.as_deref();
    // The `share` layer follows what shows (`layers.share: shareShown`).
    let layered = panel.read(cx).layered;
    if shown && !layered {
        let closer = panel.clone();
        super::keys::open_layer(cx, Layer::Share, move |window, cx| {
            closer.update(cx, |p, cx| p.close(true, window, cx));
        });
        panel.update(cx, |p, _| p.layered = true);
    } else if !shown && layered {
        super::keys::close_layer(cx, Layer::Share);
        panel.update(cx, |p, _| p.layered = false);
    }
    let subject = subject?;
    let bench = matches!(subject, Subject::Bench(_));
    let blocked = blocked(shell, &subject, cx);
    let title = if blocked {
        BLOCKED_TITLE
    } else if bench {
        BENCH_TITLE
    } else {
        CHAT_TITLE
    };
    let toggle = panel.clone();
    let pressed = subject.clone();
    let focus = panel.read(cx).button.clone();
    let share_button = Button::new("share-button", SHARE)
        .kind(Kind::Quiet)
        .with_icon(Icon::Share, 14.)
        .pressed(shown)
        .expanded(shown)
        .disabled(blocked)
        .tip(title)
        .focus(focus)
        .on_press(move |window, cx| {
            let subject = pressed.clone();
            toggle.update(cx, |p, cx| {
                if shown {
                    p.close(false, window, cx)
                } else {
                    p.open(&subject, cx)
                }
            });
        });
    let hover = panel.clone();
    let mut anchor = div()
        .id("share-anchor")
        .relative()
        .flex_none()
        .on_hover(move |hovered, _, cx| {
            hover.update(cx, |p, _| p.button_hovered = *hovered);
        })
        .child(share_button);
    if shown {
        let hidden = hides_names(shell, &subject, cx);
        let body = panel.update(cx, |p, cx| p.body(&subject, hidden, window, cx));
        anchor = anchor.child(body);
    }
    Some(anchor.into_any_element())
}

impl SharePanel {
    /// The panel under its button (`.share-panel`).
    fn body(
        &mut self,
        subject: &Subject,
        hidden: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fonts = fonts(cx);
        let bench = matches!(subject, Subject::Bench(_));
        let noun = if bench { "bench" } else { "build" };
        let share = self.share_of(subject);
        let this = cx.entity().downgrade();
        let text = |t: String, colour: Hsla, id: &'static str| {
            div()
                .id(id)
                .role(Role::Label)
                .aria_label(t.clone())
                .aria_value(t.clone())
                .text_color(colour)
                .whitespace_normal()
                .child(t)
        };
        let actions = || div().flex().flex_wrap().gap(px(8.));
        let mut body: Vec<AnyElement> = Vec::new();
        let mut first: Option<FocusHandle> = None;
        let first_handle = self.first.clone();
        let blind =
            || text(BLIND_NOTE.to_string(), colour::dim(), "share-blind-note").into_any_element();
        match (&share.phase, &self.repo) {
            (Phase::Unknown | Phase::Loading, _) | (Phase::None, None) => {
                body.push(
                    text(CHECKING.into(), colour::dim(), "share-checking").into_any_element(),
                );
            }
            (Phase::None, Some(None)) => {
                body.push(text(NO_REPO.into(), colour::bone(), "share-no-repo").into_any_element());
                let this = this.clone();
                body.push(
                    actions()
                        .child(
                            Button::new("share-settings", "Open Settings")
                                .focus(first_handle.clone())
                                .on_press(move |window, cx| {
                                    let _ = this.update(cx, |p, cx| {
                                        p.close(false, window, cx);
                                        let _ = p.shell.update(cx, |s, cx| {
                                            s.open_settings_on(
                                                Some(crate::state::settings::Pane::General),
                                                None,
                                                window,
                                                cx,
                                            )
                                        });
                                    });
                                }),
                        )
                        .into_any_element(),
                );
                first = Some(first_handle.clone());
            }
            (Phase::None, Some(Some(repo))) => {
                body.push(
                    text(offer(bench, repo), colour::bone(), "share-offer").into_any_element(),
                );
                if hidden {
                    body.push(blind());
                }
                let (this, subject) = (this.clone(), subject.clone());
                body.push(
                    actions()
                        .child(
                            Button::new("share-publish", "Publish")
                                .kind(Kind::Primary)
                                .focus(first_handle.clone())
                                .on_press(move |window, cx| {
                                    let _ =
                                        this.update(cx, |p, cx| p.publish(&subject, window, cx));
                                }),
                        )
                        .into_any_element(),
                );
                first = Some(first_handle.clone());
            }
            (Phase::Exporting | Phase::Publishing | Phase::Waiting, _) => {
                let step = rules::step_text(&share).expect("publishing");
                body.push(
                    text(step.into(), colour::ash(), "share-step")
                        .role(Role::Status)
                        .into_any_element(),
                );
            }
            (Phase::Failed(error), _) => {
                body.push(
                    text(error.clone(), colour::err(), "share-error")
                        .role(Role::Alert)
                        .into_any_element(),
                );
                let (this, subject) = (this.clone(), subject.clone());
                body.push(
                    actions()
                        .child(
                            Button::new("share-retry", "Try again")
                                .focus(first_handle.clone())
                                .on_press(move |window, cx| {
                                    let _ =
                                        this.update(cx, |p, cx| p.publish(&subject, window, cx));
                                }),
                        )
                        .into_any_element(),
                );
                first = Some(first_handle.clone());
            }
            (Phase::Shared | Phase::Removing, _) => {
                let removing = share.phase == Phase::Removing;
                let url = share.url.clone().expect("a shared page has its address");
                let input = self.link_field(&url, window, cx);
                let focused = input.read(cx).focus_handle(cx).is_focused(window);
                first = Some(input.read(cx).focus_handle(cx));
                body.push(
                    div()
                        .id("share-url")
                        .w_full()
                        .h(px(32.))
                        .flex()
                        .items_center()
                        .rounded(px(radius::SM))
                        .bg(colour::raised())
                        .shadow(if focused {
                            super::theme::settings::input_focus(true)
                        } else {
                            shadow::field_face()
                        })
                        .font_family(fonts.data.clone())
                        .text_size(px(12.))
                        .text_color(colour::bone())
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(&input)
                                    .aria_label("Shared page link")
                                    .appearance(false)
                                    .text_size(px(12.)),
                            ),
                        )
                        .into_any_element(),
                );
                let copied = self
                    .copied
                    .as_ref()
                    .is_some_and(|(u, at)| *u == url && at.elapsed() < COPIED_FOR);
                let (copy_this, copy_url) = (this.clone(), url.clone());
                let (post_this, post_url) = (this.clone(), url.clone());
                body.push(
                    actions()
                        .child(
                            Button::new("share-copy", if copied { "Copied" } else { "Copy link" })
                                .disabled(removing)
                                .on_press(move |_, cx| {
                                    let _ = copy_this.update(cx, |p, cx| p.copy(&copy_url, cx));
                                }),
                        )
                        .child(
                            Button::new("share-post", "Post on X").on_press(move |_, cx| {
                                let _ = post_this.update(cx, |p, cx| p.post(&post_url, cx));
                            }),
                        )
                        .into_any_element(),
                );
                body.push(
                    text(
                        AS_PUBLISHED.replace("{noun}", noun),
                        colour::dim(),
                        "share-as-published",
                    )
                    .into_any_element(),
                );
                if hidden {
                    body.push(blind());
                }
                let (again, again_subject) = (this.clone(), subject.clone());
                let (remove, remove_subject) = (this.clone(), subject.clone());
                body.push(
                    actions()
                        .child(
                            Button::new("share-again", "Publish again")
                                .disabled(removing)
                                .on_press(move |window, cx| {
                                    let _ = again
                                        .update(cx, |p, cx| p.publish(&again_subject, window, cx));
                                }),
                        )
                        .child(
                            ConfirmButton::new(
                                "share-remove",
                                format!("unshare:{}", rules::share_key(subject)),
                                Icon::X,
                                if removing { "Removing…" } else { "Remove" },
                                "Remove the page",
                                move |window, cx| {
                                    let _ = remove
                                        .update(cx, |p, cx| p.remove(&remove_subject, window, cx));
                                },
                            )
                            .title("Take the public page down")
                            .disabled(removing),
                        )
                        .into_any_element(),
                );
            }
        }
        // While it has no control yet (`Checking…`), the panel itself
        // holds focus until its first control comes.
        if self.focus_in {
            match first {
                Some(to) => {
                    self.focus_in = false;
                    window.focus(&to, cx);
                }
                None => window.focus(&self.dialog, cx),
            }
        }
        let outside = cx.entity().downgrade();
        let name = format!("Share this {noun}");
        let panel = ring(
            div()
                .id("share-panel")
                .role(Role::Dialog)
                .aria_label(name)
                .track_focus(&self.dialog)
                .occlude()
                .on_mouse_down_out(move |_, window, cx| {
                    // A press on the button is its own: its press closes it.
                    let _ = outside.update(cx, |p, cx| {
                        if !p.button_hovered {
                            p.close(false, window, cx);
                        }
                    });
                }),
        )
        .w(px(300.))
        .flex()
        .flex_col()
        .gap(px(10.))
        .p(px(12.))
        .rounded(px(10.))
        .bg(colour::popover())
        .shadow(shadow::panel())
        .font_family(fonts.ui.clone())
        .text_size(px(12.5))
        .line_height(px(12.5 * 1.45))
        .text_color(colour::bone())
        .children(body);
        // `top: calc(100% + 7px); right: 0`, over the pane (z 10).
        div()
            .absolute()
            .bottom_0()
            .right_0()
            .size_0()
            .child(
                deferred(
                    anchored()
                        .position_mode(AnchoredPositionMode::Local)
                        .anchor(Anchor::TopRight)
                        .position(point(px(0.), px(7.)))
                        .snap_to_window_with_margin(px(12.))
                        .child(panel),
                )
                .priority(1),
            )
            .into_any_element()
    }
}

/// The shell's share updates go to the panel. The header makes it before
/// any share command can be sent, so none comes before it.
pub fn apply(update: Update, cx: &mut App) {
    if let Some(panel) = panel(cx) {
        panel.update(cx, |p, cx| p.apply(update, cx));
    }
}
