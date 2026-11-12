// The prompt library in the page. It lives on the server, and the builders
// can change it too. Local edits show at once and reach the server shortly
// after. Each edit is a function of the list, kept in `unsent` until a save
// that carried it succeeds: a reload (a builder changed the library) lands
// the server's list with those edits made again on it, so neither side's
// change is lost, and a save that failed goes again with the next one.
//
// The phase, and the phases each can go to:
//   unloaded   nothing read yet
//   loading    the first read on its way: there is no list to edit
//   failed     that read failed (`error` says why); load() reads again
//   ready      the list is the server's with the unsent edits on it
//   saving     the list on its way to the server
//   reloading  the server's list being read again
// One request at a time: what is asked for meanwhile happens when it ends.
const NEXT = {
  unloaded: ['loading'],
  loading: ['ready', 'failed'],
  failed: ['loading'],
  ready: ['saving', 'reloading'],
  saving: ['ready'],
  reloading: ['ready'],
};

// The phases with a list read from the server: only then can it be edited,
// and so saved.
export const promptsLoaded = (phase) => phase === 'ready' || phase === 'saving' || phase === 'reloading';

export class PromptSync {
  // What the page provides:
  //   read() -> a promise of the server's list
  //   write(list) -> a promise, settled when the server has taken the list
  //   onChange(snapshot) -> called whenever the phase or the list changes
  //   onError(error) -> a load, a save or a reload failed
  //   delay -> how long edits gather before they are saved, in ms
  constructor({ read, write, onChange, onError, delay = 300 }) {
    this.host = { read, write, onChange, onError };
    this.delay = delay;
    this.phase = 'unloaded';
    this.error = null; // why the first read failed, in phase failed
    this.list = [];
    this.unsent = [];
    // The server's list changed since it was read: it is read again before
    // anything is saved over it.
    this.stale = false;
    this.timer = null; // the save that waits for more edits, in phase ready
  }

  to(phase, error = null) {
    if (!NEXT[this.phase].includes(phase)) throw new Error(`The prompt library cannot go from ${this.phase} to ${phase}`);
    if ((phase === 'failed') !== (error !== null)) throw new Error('A failed load, and only one, has an error');
    this.phase = phase;
    this.error = error;
    this.host.onChange(this.snapshot());
  }

  snapshot() {
    return { phase: this.phase, list: this.list, error: this.error };
  }

  // The first read, or another after one that failed.
  load() {
    this.to('loading');
    this.stale = false;
    this.host.read().then((list) => {
      this.list = list;
      this.to('ready');
      this.next();
    }, (error) => {
      this.to('failed', error);
      this.host.onError(error);
    });
  }

  // A change to the library. Before the list is read there is nothing to
  // make it on, and saving it would replace the server's library with it.
  edit(change) {
    if (!promptsLoaded(this.phase)) throw new Error('The prompt library has not loaded, so it cannot be changed yet');
    this.list = change(this.list);
    this.unsent.push(change);
    this.host.onChange(this.snapshot());
    if (this.phase === 'ready') this.next();
  }

  // The server's list changed.
  reload() {
    this.stale = true;
    if (this.phase === 'ready') this.next();
  }

  // Ready, with nothing on its way: a stale list is read again, and
  // otherwise the unsent edits are saved once no more come for a moment.
  next() {
    clearTimeout(this.timer);
    this.timer = null;
    if (this.stale) this.reread();
    else if (this.unsent.length) this.timer = setTimeout(() => this.save(), this.delay);
  }

  // A save that fails keeps its edits unsent and is not tried again by
  // itself: the next edit, or reload, sends them.
  save() {
    this.timer = null;
    const sent = this.unsent.length;
    this.to('saving');
    this.host.write(this.list).then(() => {
      this.unsent.splice(0, sent); // edits made since are still to go
      this.to('ready');
      this.next();
    }, (error) => {
      this.to('ready');
      this.host.onError(error);
      if (this.stale) this.reread();
    });
  }

  // A read that fails leaves the list stale, so the next edit reads it again
  // before saving.
  reread() {
    this.to('reloading');
    this.stale = false;
    this.host.read().then((list) => {
      this.list = this.unsent.reduce((made, change) => change(made), list);
      this.to('ready');
      this.next();
    }, (error) => {
      this.stale = true;
      this.to('ready');
      this.host.onError(error);
    });
  }
}
