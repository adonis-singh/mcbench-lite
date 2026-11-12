// The layers that open over the app, topmost first. style.css stacks them in
// this order (.cmdk-overlay, .lightbox, .overlay, .viewer.full, .share-panel).
// A modal layer takes the keyboard from the app under it; the viewer filling
// the window, and the chat's Share panel, are still the app.
export const LAYERS = [
  { id: 'command', modal: true },
  { id: 'lightbox', modal: true },
  { id: 'settings', modal: true },
  { id: 'viewer', modal: false },
  { id: 'share', modal: false },
];

// `open` says which layers are open, by id. The topmost open one is the one
// Escape closes; null when none is.
export function topLayer(open) {
  return LAYERS.find((layer) => open[layer.id])?.id ?? null;
}

export function modalOpen(open) {
  return LAYERS.some((layer) => layer.modal && open[layer.id]);
}

// Whether a bind's action runs now. The search bind opens and closes the
// command panel, the topmost layer, from anywhere; every other bind acts on
// the app, and waits while a modal layer covers it.
export function bindRuns(action, open) {
  return action === 'search' || !modalOpen(open);
}
