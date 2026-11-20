//! A place's unsent message (`web/draft.mjs`): what its composer holds, and
//! the drafts of the places not on screen. `reading` is the files picked for
//! it and still being read, one [`Picked`] each, added and taken out a
//! batch (one pick) at a time; a draft that is reading is not sent, its
//! message would go without them. `edit` is none, or the sent message the
//! draft is a new version of ([`begin_edit`]) with what the composer held
//! before, which Cancel brings back. The page's segments are text here:
//! prompt tokens and fill-ins are the prompt library's milestone.

use std::collections::HashMap;

use crate::{OutgoingFile, Selection};

/// A file picked and being read: its name and size, and the pick's own
/// number, so two files of the same name and size are two files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picked {
    /// Which pick this is, unique in the window.
    pub id: u64,
    /// The file's name.
    pub name: String,
    /// Its size in bytes, as picked.
    pub size: u64,
}

/// An edit: the message it is a new version of, and what the composer held.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    /// The sent message's id.
    pub of: String,
    /// What the composer held before the edit.
    pub before: Draft,
}

/// A place's unsent message.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Draft {
    /// The text.
    pub text: String,
    /// The files read.
    pub attachments: Vec<OutgoingFile>,
    /// The viewer's selections.
    pub selections: Vec<Selection>,
    /// The files still being read.
    pub reading: Vec<Picked>,
    /// The sent message this draft is a new version of.
    pub edit: Option<Box<Edit>>,
}

/// An empty draft (`EMPTY_DRAFT`).
pub fn empty() -> Draft {
    Draft::default()
}

/// Whether nothing is in it (`draftEmpty`): an edit is never empty, it is
/// there to be sent or cancelled.
pub fn draft_empty(draft: &Draft) -> bool {
    draft.edit.is_none()
        && draft.attachments.is_empty()
        && draft.selections.is_empty()
        && draft.reading.is_empty()
        && draft.text.trim().is_empty()
}

/// Every file of the draft, read or not, as name and size: what its limits
/// count (`draftFiles`).
pub fn draft_files(draft: &Draft) -> Vec<(String, u64)> {
    draft
        .attachments
        .iter()
        .map(|f| (f.name.clone(), f.size))
        .chain(draft.reading.iter().map(|p| (p.name.clone(), p.size)))
        .collect()
}

fn reads(draft: &Draft, batch: &[Picked]) -> bool {
    batch.iter().all(|entry| draft.reading.contains(entry))
}

/// Whether the draft reads the batch, itself or in what an edit put aside.
pub fn holds_read(draft: &Draft, batch: &[Picked]) -> bool {
    reads(draft, batch)
        || draft
            .edit
            .as_ref()
            .is_some_and(|edit| holds_read(&edit.before, batch))
}

/// The draft reading `batch` too.
pub fn begin_read(draft: &Draft, batch: &[Picked]) -> Draft {
    let mut next = draft.clone();
    next.reading.extend(batch.iter().cloned());
    next
}

/// The batch's read is over: the files read (none, when it failed) take its
/// place, in the draft or in what its edit put aside. A draft that does not
/// read the batch is a bug.
pub fn end_read(draft: &Draft, batch: &[Picked], files: Vec<OutgoingFile>) -> Draft {
    let mut next = draft.clone();
    if reads(draft, batch) {
        next.reading.retain(|entry| !batch.contains(entry));
        next.attachments.extend(files);
        return next;
    }
    assert!(
        holds_read(draft, batch),
        "This draft is not reading those files"
    );
    let edit = next.edit.as_mut().expect("an edit holds the read");
    edit.before = end_read(&edit.before, batch, files);
    next
}

/// An edit of the sent message `of`: its text and selections, and its
/// files as a batch still being read, take the composer's place, and what
/// the composer held is put aside for [`end_edit`].
pub fn begin_edit(
    draft: &Draft,
    of: &str,
    text: String,
    selections: Vec<Selection>,
    reading: Vec<Picked>,
) -> Draft {
    assert!(draft.edit.is_none(), "This draft is already an edit");
    Draft {
        text,
        attachments: Vec::new(),
        selections,
        reading,
        edit: Some(Box::new(Edit {
            of: of.to_string(),
            before: draft.clone(),
        })),
    }
}

/// The edit is cancelled: what the composer held before it.
pub fn end_edit(draft: &Draft) -> Draft {
    let edit = draft.edit.as_ref().expect("This draft is not an edit");
    edit.before.clone()
}

/// What a place holds once its message `sent` goes: nothing, or after an
/// edit what it held before the edit (`afterSend`).
pub fn after_send(sent: &Draft) -> Draft {
    if sent.edit.is_some() {
        end_edit(sent)
    } else {
        empty()
    }
}

/// What a place holds once its message `sent` did not go out, `now` being
/// what it holds meanwhile (`sentBack`): the message back, unless something
/// new was written there since. An edit always comes back, with what is
/// there now put aside again.
pub fn sent_back(sent: &Draft, now: &Draft) -> Draft {
    if let Some(edit) = &sent.edit {
        assert!(now.edit.is_none(), "A place holds one edit at a time");
        return Draft {
            edit: Some(Box::new(Edit {
                of: edit.of.clone(),
                before: now.clone(),
            })),
            ..sent.clone()
        };
    }
    if draft_empty(now) {
        sent.clone()
    } else {
        now.clone()
    }
}

/// The drafts of the places not on screen, by draft key (`Drafts`). A
/// place's draft is parked here or in the composer, never both.
#[derive(Default, Debug)]
pub struct Drafts {
    /// The parked drafts.
    pub parked: HashMap<String, Draft>,
}

impl Drafts {
    /// Parks the composer's draft as the place is left; parking over a
    /// parked draft means it was in the composer too, a bug.
    pub fn park(&mut self, key: &str, draft: Draft) {
        assert!(
            !self.parked.contains_key(key),
            "The draft of {key} is parked: it cannot be in the composer too"
        );
        self.parked.insert(key.to_string(), draft);
    }

    /// The place's draft, for its composer: no longer parked.
    pub fn take(&mut self, key: &str) -> Draft {
        self.parked.remove(key).unwrap_or_default()
    }

    /// The place's parked draft, or an empty one.
    pub fn peek(&self, key: &str) -> Draft {
        self.parked.get(key).cloned().unwrap_or_default()
    }

    /// A message put in a place not on screen from elsewhere. (The
    /// place's model choice is the pickers' `state::choice::Parked`.)
    pub fn put(&mut self, key: &str, draft: Draft) {
        self.parked.insert(key.to_string(), draft);
    }

    /// A message that did not go out goes back where it was written
    /// ([`sent_back`]).
    pub fn put_back(&mut self, key: &str, draft: &Draft) {
        let back = sent_back(draft, &self.peek(key));
        self.parked.insert(key.to_string(), back);
    }

    /// The place is gone, and its draft with it.
    pub fn drop_place(&mut self, key: &str) {
        self.parked.remove(key);
    }

    /// A batch's read is over in whichever parked draft holds it.
    pub fn end_read(&mut self, batch: &[Picked], files: &[OutgoingFile]) {
        for draft in self.parked.values_mut() {
            if holds_read(draft, batch) {
                *draft = end_read(draft, batch, files.to_vec());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! `test/draft.test.js`, case for case.
    use super::*;
    use std::sync::Arc;

    fn picked(id: u64, name: &str, size: u64) -> Picked {
        Picked {
            id,
            name: name.into(),
            size,
        }
    }

    fn read(name: &str, size: u64) -> OutgoingFile {
        OutgoingFile {
            name: name.into(),
            content_type: "text/plain".into(),
            size,
            data: Arc::from(&b""[..]),
        }
    }

    fn text(value: &str) -> Draft {
        Draft {
            text: value.into(),
            ..Draft::default()
        }
    }

    #[test]
    fn files_being_read_are_part_of_the_draft_until_their_read_ends() {
        assert!(draft_empty(&empty()));
        let batch = [picked(1, "a.png", 10), picked(2, "b.txt", 10)];
        let reading = begin_read(&empty(), &batch);
        assert_eq!(reading.reading, batch);
        // A draft that only holds files being read is not empty.
        assert!(!draft_empty(&reading));
        assert!(holds_read(&reading, &batch));
        assert!(!holds_read(&empty(), &batch));
        assert_eq!(
            draft_files(&reading),
            vec![("a.png".into(), 10), ("b.txt".into(), 10)]
        );
        let files = vec![read("a.png", 10), read("b.txt", 10)];
        let done = end_read(&reading, &batch, files.clone());
        assert!(done.reading.is_empty());
        assert_eq!(done.attachments, files);
        assert!(!holds_read(&done, &batch));
        let again = std::panic::catch_unwind(|| end_read(&done, &batch, files.clone()));
        assert!(again.is_err(), "not reading those files");
        assert_eq!(empty(), Draft::default());
    }

    #[test]
    fn a_failed_read_leaves_the_draft_with_what_it_had() {
        let kept = read("kept.txt", 10);
        let batch = [picked(1, "big.bin", 10)];
        let start = Draft {
            attachments: vec![kept.clone()],
            ..Draft::default()
        };
        let done = end_read(&begin_read(&start, &batch), &batch, Vec::new());
        assert_eq!(done.attachments, vec![kept]);
        assert!(done.reading.is_empty());
    }

    #[test]
    fn batches_end_on_their_own_and_files_still_being_read_count_toward_the_limits() {
        let first = [picked(1, "one", 5)];
        let second = [picked(2, "two", 7), picked(3, "three", 9)];
        let start = Draft {
            attachments: vec![read("zero", 1)],
            ..Draft::default()
        };
        let mut draft = begin_read(&begin_read(&start, &first), &second);
        assert_eq!(
            draft_files(&draft)
                .iter()
                .map(|(_, size)| *size)
                .collect::<Vec<_>>(),
            vec![1, 5, 7, 9]
        );
        draft = end_read(&draft, &second, vec![read("two", 7), read("three", 9)]);
        assert!(holds_read(&draft, &first));
        assert_eq!(draft.reading, first);
        assert_eq!(
            draft
                .attachments
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>(),
            ["zero", "two", "three"]
        );
        // Two files of the same name and size are still two files.
        let twin = [picked(4, "one", 5)];
        assert!(!holds_read(&draft, &twin));
    }

    #[test]
    fn a_draft_is_parked_or_in_the_composer_never_both() {
        let mut drafts = Drafts::default();
        // A place never left has nothing parked.
        assert_eq!(drafts.take("chat:new"), empty());
        drafts.park("chat:new", text("a tower"));
        assert_eq!(drafts.peek("chat:new"), text("a tower"));
        // Coming back takes it out: the composer holds it now.
        assert_eq!(drafts.take("chat:new"), text("a tower"));
        assert_eq!(drafts.peek("chat:new"), empty());
        assert!(drafts.parked.is_empty());
        // Parking over a parked draft means it was in the composer too.
        drafts.park("chat:a", text("one"));
        let twice = std::panic::catch_unwind(move || {
            let mut drafts = drafts;
            drafts.park("chat:a", text("two"));
        });
        assert!(
            twice.is_err(),
            "is parked: it cannot be in the composer too"
        );
        let mut drafts = Drafts::default();
        drafts.park("chat:a", text("one"));
        drafts.drop_place("chat:a");
        assert_eq!(drafts.peek("chat:a"), empty());
    }

    #[test]
    fn a_prompt_put_in_a_parked_draft_keeps_the_model_picked_there_and_an_unsent_message_does_not_replace_newer_text()
     {
        let mut drafts = Drafts::default();
        drafts.park("chat:a", text(""));
        drafts.put_back("chat:a", &text("not sent"));
        assert_eq!(drafts.peek("chat:a"), text("not sent"));
        drafts.put_back("chat:a", &text("older"));
        assert_eq!(drafts.peek("chat:a"), text("not sent"));
        // A prompt moved there replaces what was parked.
        drafts.put("chat:a", text("edited prompt"));
        assert_eq!(drafts.peek("chat:a"), text("edited prompt"));
        drafts.put_back("chat:never-left", &text("kept"));
        assert_eq!(drafts.take("chat:never-left"), text("kept"));
    }

    #[test]
    fn a_read_ends_in_the_parked_draft_that_holds_it() {
        let mut drafts = Drafts::default();
        let batch = [picked(1, "a.png", 10)];
        drafts.park("chat:a", begin_read(&empty(), &batch));
        drafts.park("chat:b", text("other"));
        drafts.end_read(&batch, &[read("a.png", 10)]);
        assert_eq!(drafts.peek("chat:a").attachments, vec![read("a.png", 10)]);
        assert!(drafts.peek("chat:a").reading.is_empty());
        assert_eq!(drafts.peek("chat:b"), text("other"));
    }

    #[test]
    fn an_edit_puts_the_prompt_in_the_composer_and_its_cancel_brings_back_what_was_there_files_read_meanwhile_and_all()
     {
        let typed = Draft {
            text: "half a thought".into(),
            attachments: vec![read("kept.txt", 10)],
            ..Draft::default()
        };
        let pending = [picked(1, "late.png", 10)];
        let before = begin_read(&typed, &pending);
        let files = vec![picked(2, "door.png", 4)];
        let selection = Selection {
            letter: "A".into(),
            label: "Stone".into(),
            min: [0, 0, 0],
            max: [1, 1, 1],
            blocks: 8,
            block: Some("minecraft:stone".into()),
        };
        let edit = begin_edit(
            &before,
            "m2",
            "Add a door".into(),
            vec![selection.clone()],
            files.clone(),
        );
        assert_eq!(
            edit,
            Draft {
                text: "Add a door".into(),
                attachments: Vec::new(),
                selections: vec![selection],
                reading: files.clone(),
                edit: Some(Box::new(Edit {
                    of: "m2".into(),
                    before: before.clone(),
                })),
            }
        );
        let again = std::panic::catch_unwind(|| {
            begin_edit(&edit, "m1", String::new(), Vec::new(), Vec::new())
        });
        assert!(again.is_err(), "already an edit");
        // An edit with its text deleted is still an edit.
        assert!(!draft_empty(&Draft {
            text: String::new(),
            selections: Vec::new(),
            reading: Vec::new(),
            ..edit.clone()
        }));
        // Each read lands in the draft that is reading it.
        assert!(holds_read(&edit, &files));
        assert!(holds_read(&edit, &pending));
        let mut draft = end_read(&edit, &files, vec![read("door.png", 4)]);
        draft = end_read(&draft, &pending, vec![read("late.png", 10)]);
        assert_eq!(draft.attachments, vec![read("door.png", 4)]);
        assert_eq!(
            draft.edit.as_ref().expect("an edit").before.attachments,
            vec![read("kept.txt", 10), read("late.png", 10)]
        );
        let after = draft.clone();
        assert!(std::panic::catch_unwind(|| end_read(&after, &pending, Vec::new())).is_err());
        // Cancel: what was there before, with the model picked during the edit.
        let back = end_edit(&Draft { ..draft });
        assert_eq!(
            back,
            Draft {
                attachments: vec![read("kept.txt", 10), read("late.png", 10)],
                reading: Vec::new(),
                ..typed
            }
        );
        let cancelled = back.clone();
        assert!(std::panic::catch_unwind(|| end_edit(&cancelled)).is_err());
        // A parked edit is still one, and its reads end in it.
        let mut parked = Drafts::default();
        parked.park(
            "chat:c1",
            begin_edit(&empty(), "m2", String::new(), Vec::new(), files.clone()),
        );
        parked.end_read(&files, &[read("door.png", 4)]);
        assert_eq!(
            parked.peek("chat:c1").attachments,
            vec![read("door.png", 4)]
        );
        assert_eq!(parked.peek("chat:c1").edit.expect("an edit").of, "m2");
    }

    #[test]
    fn a_sent_edit_leaves_what_the_composer_held_before_it_and_one_that_did_not_go_out_comes_back_with_what_is_there_now()
     {
        assert_eq!(after_send(&text("plain")), empty());
        let edit = Draft {
            ..begin_edit(
                &text("half a thought"),
                "m2",
                "Add a door".into(),
                Vec::new(),
                Vec::new(),
            )
        };
        let left = after_send(&edit);
        assert_eq!(left, text("half a thought"));
        // Not sent: the edit is back, and what was written meanwhile is put
        // aside in it.
        let meanwhile = text("a new line");
        assert_eq!(
            sent_back(&edit, &meanwhile),
            Draft {
                edit: Some(Box::new(Edit {
                    of: "m2".into(),
                    before: meanwhile.clone(),
                })),
                ..edit.clone()
            }
        );
        assert_eq!(sent_back(&edit, &left).edit.expect("an edit").before, left);
        let twice = edit.clone();
        assert!(std::panic::catch_unwind(|| sent_back(&twice, &twice)).is_err());
        // A plain message comes back only where nothing new was written.
        assert_eq!(sent_back(&text("plain"), &empty()), text("plain"));
        assert_eq!(sent_back(&text("plain"), &meanwhile), meanwhile);
        let mut drafts = Drafts::default();
        drafts.park("chat:a", left.clone());
        drafts.put_back("chat:a", &edit);
        assert_eq!(
            drafts.peek("chat:a"),
            Draft {
                edit: Some(Box::new(Edit {
                    of: "m2".into(),
                    before: left,
                })),
                ..edit
            }
        );
    }
}
