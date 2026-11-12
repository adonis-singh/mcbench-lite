// Server data that several views read (model catalogs, model capabilities):
// fetched once and shared, so a view that mounts after it landed renders it
// in its first frame instead of flashing a default. A reload keeps showing
// the old value until the new one arrives.
export class Shared {
  // `read(url)` -> a promise of the url's value.
  constructor(read) {
    this.read = read;
    this.entries = new Map(); // url -> { value, error, promise, listeners }
  }

  entry(url) {
    if (!this.entries.has(url)) this.entries.set(url, { value: undefined, error: null, promise: null, listeners: new Set() });
    return this.entries.get(url);
  }

  // `promise` is the entry's current request. Only it settles the entry: a
  // request that `fresh` superseded still answers whoever asked for it, and
  // writes nothing.
  load(url, { fresh = false } = {}) {
    const entry = this.entry(url);
    if (fresh) entry.promise = null;
    if (!entry.promise) {
      const settle = () => entry.listeners.forEach((listener) => listener());
      const request = this.read(url).then(
        (value) => {
          if (entry.promise === request) { entry.value = value; entry.error = null; settle(); }
          return value;
        },
        (error) => {
          if (entry.promise === request) { entry.error = error; entry.promise = null; settle(); }
          throw error;
        },
      );
      request.catch(() => {});
      entry.promise = request;
    }
    return entry.promise;
  }
}
