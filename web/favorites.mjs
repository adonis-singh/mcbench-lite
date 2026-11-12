// The starred models in the page. The list lives on the server. A star shows
// at once and is saved after: each toggle goes on its own, in order, as the
// server's list with that toggle made, so one that fails is undone (its star
// goes back) and takes no other with it.
//
// The phase, and the phases each can go to:
//   unloaded  the server's list is not known: nothing can be starred, since
//             saving would put this page's list over the server's
//   idle      the list is the server's with the toggles still to save on it
//   saving    the first of those toggles on its way to the server
const NEXT = {
  unloaded: ['idle'],
  idle: ['saving'],
  saving: ['idle'],
};

const toggled = (list, key) => (list.includes(key) ? list.filter((entry) => entry !== key) : [...list, key]);

export class Favorites {
  // What the page provides:
  //   write(list) -> a promise of the list the server stored
  //   onChange(list) -> called whenever the list shown changes
  //   onError(error) -> a save failed, and its star went back
  constructor({ write, onChange, onError }) {
    this.host = { write, onChange, onError };
    this.phase = 'unloaded';
    this.saved = []; // the server's list
    this.unsent = []; // the keys toggled and not saved yet, oldest first
  }

  to(phase) {
    if (!NEXT[this.phase].includes(phase)) throw new Error(`The favorites cannot go from ${this.phase} to ${phase}`);
    this.phase = phase;
  }

  // The list shown: the server's with the unsent toggles made on it.
  get list() {
    return this.unsent.reduce(toggled, this.saved);
  }

  // The server's list, read once with the config.
  load(list) {
    if (this.phase !== 'unloaded') throw new Error('The favorites have loaded already');
    this.to('idle');
    this.saved = list;
    this.host.onChange(this.list);
  }

  toggle(key) {
    if (this.phase === 'unloaded') throw new Error('The favorites have not loaded, so they cannot be changed yet');
    this.unsent.push(key);
    this.host.onChange(this.list);
    if (this.phase === 'idle') this.save();
  }

  save() {
    this.to('saving');
    this.host.write(toggled(this.saved, this.unsent[0])).then((list) => {
      this.saved = list;
      this.saveEnded();
    }, (error) => {
      this.saveEnded();
      this.host.onError(error);
    });
  }

  // The toggle is the server's now, or dropped: either way no longer unsent.
  saveEnded() {
    this.unsent.shift();
    this.to('idle');
    this.host.onChange(this.list);
    if (this.unsent.length) this.save();
  }
}
