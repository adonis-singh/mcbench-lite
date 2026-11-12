// The command panel's highlight. It belongs to an item, by its key, not to a
// position in the list: groups that arrive later (the server's matches)
// leave it on the item it was on.

// The key highlighted among `keys`, the items listed in order: `active` (the
// key last highlighted) while it is listed, else the first.
export function highlighted(keys, active) {
  return keys.includes(active) ? active : keys[0] ?? null;
}

// The key `step` items on from the highlighted one, wrapping round.
export function stepped(keys, active, step) {
  const index = keys.indexOf(highlighted(keys, active));
  return keys[(index + step + keys.length) % keys.length];
}
