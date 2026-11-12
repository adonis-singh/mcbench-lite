// A place's unsent message (a chat, a new chat, a bench): what its composer
// holds. `reading` is the files picked for it and still being read, one
// { name, size } each, added and taken out a batch (one pick) at a time. A
// draft that is reading is not sent: its message would go without them.
export const EMPTY_DRAFT = Object.freeze({ segments: [], attachments: [], selections: [], reading: [], choice: null });

export function draftEmpty(draft) {
  return !draft.attachments.length && !draft.selections.length && !draft.reading.length
    && !draft.segments.some((segment) => segment.type !== 'text' || segment.text.trim());
}

// Every file of the draft, read or not: what its limits count.
export const draftFiles = (draft) => [...draft.attachments, ...draft.reading];

export const holdsRead = (draft, batch) => batch.every((entry) => draft.reading.includes(entry));

export function beginRead(draft, batch) {
  return { ...draft, reading: [...draft.reading, ...batch] };
}

// The batch's read is over: the files read (none, when it failed) take its
// place.
export function endRead(draft, batch, files) {
  if (!holdsRead(draft, batch)) throw new Error('This draft is not reading those files');
  return {
    ...draft,
    reading: draft.reading.filter((entry) => !batch.includes(entry)),
    attachments: [...draft.attachments, ...files],
  };
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

  // A message that did not go out goes back where it was written, unless
  // something new was written there since.
  putBack(key, draft) {
    if (draftEmpty(this.peek(key))) this.put(key, draft);
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
