// The place on screen: one value, its kind and ids.
//   { kind: 'chat', id }                     a Build chat the list has
//   { kind: 'new-chat', id }                 Build's new chat. `id` is its row on
//                                            the server, null until it has one
//   { kind: 'new-bench' }                    a bench about to be run
//   { kind: 'compare', benchId }             a bench's comparison
//   { kind: 'member', benchId, memberId }    one model's chat in a bench
//   { kind: 'leaderboard' }
//   { kind: 'prompt' }                       the prompt library
// Everything else that says where the page is derives from it (placeKeys).

// The draft keys. A new chat's is the same before and after its row exists.
export const NEW_CHAT_DRAFT = 'chat:new';
export const NEW_BENCH_DRAFT = 'bench:new';
export const chatDraft = (id) => `chat:${id}`;
export const benchDraft = (benchId) => `bench:${benchId}`;

// What a place means for the page:
//   mode      'build' or 'bench'
//   chatId    the chat whose transcript it shows, or null
//   viewKey   the world the viewer shows: a chat id, a bench round, or null
//   draftKey  the draft its composer edits; null for a place with no composer
// `rounds` is the round each comparison shows, by bench id.
export function placeKeys(place, rounds) {
  switch (place.kind) {
    case 'chat':
      return { mode: 'build', chatId: place.id, viewKey: place.id, draftKey: chatDraft(place.id) };
    case 'new-chat':
      return { mode: 'build', chatId: place.id, viewKey: place.id, draftKey: NEW_CHAT_DRAFT };
    case 'new-bench':
      return { mode: 'bench', chatId: null, viewKey: null, draftKey: NEW_BENCH_DRAFT };
    case 'compare': {
      const round = rounds[place.benchId];
      if (!round) throw new Error(`The comparison of bench ${place.benchId} has no round to show`);
      return { mode: 'bench', chatId: null, viewKey: `bench:${place.benchId}:${round}`, draftKey: benchDraft(place.benchId) };
    }
    case 'member':
      return { mode: 'bench', chatId: place.memberId, viewKey: place.memberId, draftKey: null };
    case 'leaderboard':
    case 'prompt':
      return { mode: 'bench', chatId: null, viewKey: null, draftKey: null };
    default:
      throw new Error(`Unknown place: ${place.kind}`);
  }
}

// The new chat's row now exists on the server: the same place, and the same
// draft, with a world to show.
export function withRow(place, id) {
  if (place.kind !== 'new-chat' || place.id) throw new Error(`Only a new chat without a row takes one, not ${place.kind}`);
  return { kind: 'new-chat', id };
}

// The new chat got its first message: a chat like any other from here on.
export function asChat(place) {
  if (place.kind !== 'new-chat' || !place.id) throw new Error(`Only a new chat with a row becomes a chat, not ${place.kind}`);
  return { kind: 'chat', id: place.id };
}
