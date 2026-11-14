// The sidebar's filters: chats (Build) or benches (Bench) narrowed by size,
// by the library prompt they came from, and by model, beside the search
// text. Each group holds the values chosen in it; a row is listed when it
// matches every group with a choice, and within a group any chosen value.
// The page holds the whole list, and each row says what it is in each group
// (its facets), so filtering takes no request: only the text is searched on
// the server, and its results are narrowed here.
//
// A row's facets, one list per group:
//   size    a chat's current resolution, the tag its row shows; a bench's
//   prompt  the ids of the library prompts any of its messages (a bench: its
//           rounds) was sent from (server/store.js)
//   model   a chat's current model; a bench's models, any of which matches
// Models are keyed `provider::model`, as favorites are.

import { RESOLUTIONS } from './resolution.mjs';

export const GROUPS = ['size', 'prompt', 'model'];

export const NO_FILTERS = Object.freeze({ size: [], prompt: [], model: [] });

export const modelKey = (provider, model) => `${provider}::${model}`;

export const filtering = (filters) => GROUPS.some((group) => filters[group].length > 0);

// The filters with `value` chosen in `group`, or no longer chosen.
export function toggled(filters, group, value) {
  const chosen = filters[group];
  return { ...filters, [group]: chosen.includes(value) ? chosen.filter((entry) => entry !== value) : [...chosen, value] };
}

export const chatFacets = (row) => ({ size: [row.resolution], prompt: row.prompts, model: [modelKey(row.provider, row.model)] });

// `members` is the bench's chats, as the list has them.
export const benchFacets = (bench, members) => ({
  size: [bench.resolution],
  prompt: bench.prompts,
  model: members.map((row) => modelKey(row.provider, row.model)),
});

export function matches(filters, facets) {
  return GROUPS.every((group) => filters[group].length === 0 || filters[group].some((value) => facets[group].includes(value)));
}

// The rows the sidebar lists: the search's results in their rank order
// (`found`, [{ id }], when text is searched; null otherwise) or every row,
// then those the filters match. `facetsOf(row)` is the row's facets.
export function listed(rows, found, filters, facetsOf) {
  let shown = rows;
  if (found) {
    const byId = new Map(rows.map((row) => [row.id, row]));
    shown = found.map((result) => byId.get(result.id)).filter(Boolean);
  }
  return filtering(filters) ? shown.filter((row) => matches(filters, facetsOf(row))) : shown;
}

// What the menu offers, from the facets of every row the mode lists (not
// only those shown, so one choice never takes the others away):
//   size    every resolution stop
//   prompt  the prompts the rows came from, in the library's order, then
//           those deleted from it, by title: [{ id, title, deleted }]
//   model   the rows' models, by model id, then provider
export function offered(facetsList, library, titles) {
  const used = (group) => new Set(facetsList.flatMap((facets) => facets[group]));
  const prompts = used('prompt');
  const inLibrary = new Set(library.map((prompt) => prompt.id));
  const kept = library.filter((prompt) => prompts.has(prompt.id)).map((prompt) => ({ id: prompt.id, title: prompt.title, deleted: false }));
  const deleted = [...prompts].filter((id) => !inLibrary.has(id))
    .map((id) => ({ id, title: titles[id], deleted: true }))
    .sort((a, b) => a.title.localeCompare(b.title));
  const model = (key) => key.slice(key.indexOf('::') + 2);
  return {
    size: RESOLUTIONS.map((stop) => stop.value),
    prompt: [...kept, ...deleted],
    model: [...used('model')].sort((a, b) => model(a).localeCompare(model(b)) || a.localeCompare(b)),
  };
}

// The chosen values the list still offers: a prompt or model no listed row
// has any more (its last chat archived) is no choice, and its chip goes. It
// comes back with a row that has it.
export function inEffect(filters, offer) {
  const values = { size: offer.size, prompt: offer.prompt.map((entry) => entry.id), model: offer.model };
  return Object.fromEntries(GROUPS.map((group) => [group, filters[group].filter((value) => values[group].includes(value))]));
}
