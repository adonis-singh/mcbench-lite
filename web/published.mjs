// Settings' list of published builds (GET /api/shares), read each time
// Settings open. One value:
//   unread   { phase }: not read since Settings last opened
//   loading  { phase, data, removing, errors }: being read. `data` is what an
//            earlier read gave, still shown meanwhile, or null
//   loaded   { phase, data, removing, errors }: `data` is the server's
//            { repo, builds }, each build as publishedSubject reads it
//   failed   { phase, data, removing, errors, error }: the read failed
//            (`error` says why), and it can be read again
// `removing` lists the shareIds whose removal is on its way, and `errors`
// maps a shareId to why its last removal failed. Any read can be followed by
// another, which supersedes it.
export const PUBLISHED_UNREAD = Object.freeze({ phase: 'unread' });

const NEXT = {
  unread: ['loading'],
  loading: ['loading', 'loaded', 'failed', 'unread'],
  loaded: ['loading', 'unread'],
  failed: ['loading', 'unread'],
};

function publishedTo(published, next) {
  if (!NEXT[published.phase].includes(next.phase)) throw new Error(`The published builds cannot go from ${published.phase} to ${next.phase}`);
  return next;
}

export const publishedReading = (published) => publishedTo(published, published.phase === 'unread'
  ? { phase: 'loading', data: null, removing: [], errors: {} }
  : { phase: 'loading', data: published.data, removing: published.removing, errors: published.errors });
export const publishedRead = (published, data) => publishedTo(published, { ...published, phase: 'loaded', data });
export const publishedFailed = (published, error) => publishedTo(published, { ...published, phase: 'failed', error: String(error.message || error) });
export const publishedClosed = (published) => publishedTo(published, PUBLISHED_UNREAD);

// What the list shows: 'loading' before the first read lands, 'failed',
// 'no-repo' when no repository is set, 'empty', or 'list'.
export function publishedView(published) {
  if (published.phase === 'unread') throw new Error('Unread published builds show nothing');
  if (published.phase === 'failed') return 'failed';
  if (!published.data) return 'loading';
  if (published.data.repo === null) return 'no-repo';
  return published.data.builds.length ? 'list' : 'empty';
}

function shownBuild(published, shareId) {
  if (!published.data?.builds.some((build) => build.shareId === shareId)) throw new Error(`${shareId} is not a published build on show`);
}

// A removal of build `shareId` starts, letting go of its last error.
export function publishedRemoving(published, shareId) {
  shownBuild(published, shareId);
  if (published.removing.includes(shareId)) throw new Error(`${shareId} is already being removed`);
  const { [shareId]: _, ...errors } = published.errors;
  return { ...published, removing: [...published.removing, shareId], errors };
}

// The removal landed: the build leaves the list before the read again.
export function publishedRemoved(published, shareId) {
  if (!published.removing.includes(shareId)) throw new Error(`${shareId} is not being removed`);
  return {
    ...published,
    data: { ...published.data, builds: published.data.builds.filter((build) => build.shareId !== shareId) },
    removing: published.removing.filter((id) => id !== shareId),
  };
}

// The removal failed: the build stays, with why.
export function publishedKept(published, shareId, error) {
  if (!published.removing.includes(shareId)) throw new Error(`${shareId} is not being removed`);
  return {
    ...published,
    removing: published.removing.filter((id) => id !== shareId),
    errors: { ...published.errors, [shareId]: String(error.message || error) },
  };
}

// The chat or bench a published build was made from, as a share subject
// (share.mjs), or null when it was deleted. A build is { kind: 'chat' |
// 'bench', conversationId, benchId, labels, ... }: a chat's labels are its
// model, a bench's what its page shows per build.
export function publishedSubject(build) {
  if (build.kind === 'chat') return build.conversationId === null ? null : { kind: 'chat', id: build.conversationId };
  if (build.kind === 'bench') return build.benchId === null ? null : { kind: 'bench', id: build.benchId };
  throw new Error(`${JSON.stringify(build.kind)} is not a kind of published build (chat, bench)`);
}

const UNITS = [
  ['year', 365 * 24 * 60 * 60],
  ['month', 30 * 24 * 60 * 60],
  ['day', 24 * 60 * 60],
  ['hour', 60 * 60],
  ['minute', 60],
];

// How long ago `iso` was, in its largest whole unit: "2 hours ago".
export function publishedAgo(iso, now) {
  const seconds = Math.max(0, Math.floor((now - Date.parse(iso)) / 1000));
  for (const [unit, size] of UNITS) {
    const count = Math.floor(seconds / size);
    if (count >= 1) return `${count} ${unit}${count === 1 ? '' : 's'} ago`;
  }
  return 'just now';
}
