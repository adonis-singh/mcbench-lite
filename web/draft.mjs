// A place's unsent message (a chat, a new chat, a bench): what its composer
// holds. `reading` is the files picked for it and still being read, one
// { name, size } each, added and taken out a batch (one pick) at a time. A
// draft that is reading is not sent: its message would go without them.
// `edit` is null, or the sent prompt or round the draft is a new version of
// (beginEdit), with what the composer held before, which Cancel brings back.
export const EMPTY_DRAFT = Object.freeze({ segments: [], attachments: [], selections: [], reading: [], choice: null, edit: null });

// An edit is never empty: it is there to be sent or cancelled.
export function draftEmpty(draft) {
  return !draft.edit && !draft.attachments.length && !draft.selections.length && !draft.reading.length
    && !draft.segments.some((segment) => segment.type !== 'text' || segment.text.trim());
}

// Every file of the draft, read or not: what its limits count.
export const draftFiles = (draft) => [...draft.attachments, ...draft.reading];

const reads = (draft, batch) => batch.every((entry) => draft.reading.includes(entry));

// Whether the draft is reading the batch, itself or in what an edit put aside.
export const holdsRead = (draft, batch) => reads(draft, batch) || (draft.edit !== null && holdsRead(draft.edit.before, batch));

export function beginRead(draft, batch) {
  return { ...draft, reading: [...draft.reading, ...batch] };
}

// The batch's read is over: the files read (none, when it failed) take its
// place, in the draft or in what its edit put aside.
export function endRead(draft, batch, files) {
  if (reads(draft, batch)) {
    return {
      ...draft,
      reading: draft.reading.filter((entry) => !batch.includes(entry)),
      attachments: [...draft.attachments, ...files],
    };
  }
  if (!holdsRead(draft, batch)) throw new Error('This draft is not reading those files');
  return { ...draft, edit: { ...draft.edit, before: endRead(draft.edit.before, batch, files) } };
}

// An edit of the sent prompt or round `of`: the prompt (its segments and
// selections, and its files as a batch still `reading`, which endRead fills
// in) takes the composer's place, and what the composer held is put aside
// for endEdit. The model picked stays.
export function beginEdit(draft, of, { segments, selections, reading }) {
  if (draft.edit) throw new Error('This draft is already an edit');
  return { segments, attachments: [], selections, reading, choice: draft.choice, edit: { of, before: draft } };
}

// The edit is cancelled: what the composer held before it, with the model
// picked since.
export function endEdit(draft) {
  if (!draft.edit) throw new Error('This draft is not an edit');
  return { ...draft.edit.before, choice: draft.choice };
}

// What a place holds once its message `sent` goes: nothing, or after an
// edit what it held before the edit.
export const afterSend = (sent) => (sent.edit ? endEdit(sent) : EMPTY_DRAFT);

// What a place holds once its message `sent` did not go out, `now` being
// what it holds meanwhile (with the model picked there): the message back,
// unless something new was written there since. An edit always comes back,
// with what is there now put aside again.
export function sentBack(sent, now) {
  if (sent.edit) {
    if (now.edit) throw new Error('A place holds one edit at a time');
    return { ...sent, choice: now.choice, edit: { ...sent.edit, before: now } };
  }
  return draftEmpty(now) ? { ...sent, choice: now.choice } : now;
}

// The drafts of the places not on screen, by draft key (place.mjs). A place's
// draft is either parked here or in the composer, never both: leaving a place
// parks what its composer holds, and coming to it takes its draft out.
export class Drafts {
  constructor() {
    this.parked = new Map();
  }

  park(key, draft) {
    if (this.parked.has(key)) throw new Error(`The draft of ${key} is parked: it cannot be in the composer too`);
    this.parked.set(key, draft);
  }

  // The place's draft, for its composer: no longer parked.
  take(key) {
    const draft = this.peek(key);
    this.parked.delete(key);
    return draft;
  }

  peek(key) {
    return this.parked.get(key) || EMPTY_DRAFT;
  }

  // What a place not on screen is left to come back to: a prompt put there
  // from elsewhere. It keeps the model picked for the place.
  put(key, draft) {
    this.parked.set(key, { ...draft, choice: this.peek(key).choice });
  }

  // A message that did not go out goes back where it was written (sentBack).
  putBack(key, draft) {
    this.parked.set(key, sentBack(draft, this.peek(key)));
  }

  // The place is gone (its chat or bench archived), and its draft with it.
  drop(key) {
    this.parked.delete(key);
  }

  // A batch's read is over in whichever parked draft holds it.
  endRead(batch, files) {
    for (const [key, draft] of this.parked) {
      if (holdsRead(draft, batch)) this.parked.set(key, endRead(draft, batch, files));
    }
  }
}
