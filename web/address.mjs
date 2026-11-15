// The page's addresses. Each place (place.mjs) has one, kept in the address
// bar, so a reload comes back to it, Back and Forward move between the
// places opened, and a chat or bench can be linked and bookmarked:
//   /                         the landing: the newest chat, or the mode last used
//   /new                      Build's new chat, before and after it has a row
//   /chat/<id>                a Build chat
//   /bench                    a new bench
//   /bench/<id>               a bench's comparison
//   /bench/<id>/<memberId>    one model's chat in a bench
//   /leaderboard
//   /prompts                  the prompt library
// The server serves the page at each (isPageAddress), and the sign-in link
// keeps the path it was opened on (server/auth.js signIn), so /chat/<id>
// opened with ?token= lands on that chat. Settings, the search panel and the
// sidebar's archived section open over a place and have no address.

const segment = (id) => encodeURIComponent(id);

export function placeAddress(place) {
  switch (place.kind) {
    case 'chat':
      return `/chat/${segment(place.id)}`;
    case 'new-chat':
      return '/new';
    case 'new-bench':
      return '/bench';
    case 'compare':
      return `/bench/${segment(place.benchId)}`;
    case 'member':
      return `/bench/${segment(place.benchId)}/${segment(place.memberId)}`;
    case 'leaderboard':
      return '/leaderboard';
    case 'prompt':
      return '/prompts';
    // A chat or bench the address names that is not there keeps its address.
    case 'missing':
      return place.address;
    default:
      throw new Error(`Unknown place: ${place.kind}`);
  }
}

// What `pathname` names: { kind: 'landing' }, or a place (a chat's or a
// bench's not yet looked up in the lists), or null when it is not one of the
// page's addresses.
export function addressPlace(pathname) {
  if (pathname === '/') return { kind: 'landing' };
  const fixed = { '/new': { kind: 'new-chat', id: null }, '/bench': { kind: 'new-bench' }, '/leaderboard': { kind: 'leaderboard' }, '/prompts': { kind: 'prompt' } };
  if (Object.hasOwn(fixed, pathname)) return fixed[pathname];
  const parts = pathname.split('/').slice(1);
  if (parts.some((part) => !part)) return null;
  let ids;
  try {
    ids = parts.slice(1).map(decodeURIComponent);
  } catch {
    return null; // a malformed escape names nothing
  }
  if (parts[0] === 'chat' && ids.length === 1) return { kind: 'chat', id: ids[0] };
  if (parts[0] === 'bench' && ids.length === 1) return { kind: 'compare', benchId: ids[0] };
  if (parts[0] === 'bench' && ids.length === 2) return { kind: 'member', benchId: ids[0], memberId: ids[1] };
  return null;
}

export const isPageAddress = (pathname) => addressPlace(pathname) !== null;

// The place the page shows while its lists load: what the address names
// when it needs no list; Build's new chat or a new bench while the chat or
// bench it names is looked up; and on the landing, the mode last used.
export function startPlace(target, mode) {
  switch (target.kind) {
    case 'landing':
      return mode === 'bench' ? { kind: 'new-bench' } : { kind: 'new-chat', id: null };
    case 'chat':
      return { kind: 'new-chat', id: null };
    case 'compare':
    case 'member':
      return { kind: 'new-bench' };
    default:
      return target;
  }
}

// The place an address names, from the lists the server sent: `chats` and
// `benches` are the listed rows. A chat or bench they do not have (deleted,
// archived, or never there) is { kind: 'missing', mode, id, address }, `id`
// being the chat's or the bench's: never another chat or bench in its place.
export function listedPlace(target, address, chats, benches) {
  const missing = (mode, id) => ({ kind: 'missing', mode, id, address });
  switch (target.kind) {
    case 'chat':
      return chats.some((row) => row.id === target.id && !row.benchId) ? target : missing('build', target.id);
    case 'compare':
      return benches.some((bench) => bench.id === target.benchId) ? target : missing('bench', target.benchId);
    case 'member':
      return benches.some((bench) => bench.id === target.benchId) && chats.some((row) => row.id === target.memberId && row.benchId === target.benchId)
        ? target
        : missing('bench', target.benchId);
    case 'landing':
      throw new Error('The landing names no place of its own');
    default:
      return target;
  }
}
