// A bench's details (its rounds, stats and votes) as the page has them, one
// value per bench:
//   loading  being read. `data` is what an earlier read gave, still shown
//            meanwhile, or null
//   loaded   `data` is the server's
//   failed   the read failed (`error` says why): `data` is the earlier
//            read's, or null, and it can be read again
// Any read can be followed by another, which supersedes it.
const NEXT = {
  loading: ['loading', 'loaded', 'failed'],
  loaded: ['loading'],
  failed: ['loading'],
};

function detailsTo(details, next) {
  if (details && !NEXT[details.phase].includes(next.phase)) throw new Error(`Bench details cannot go from ${details.phase} to ${next.phase}`);
  return next;
}

// `details` is the bench's value so far: undefined before its first read.
export const detailsReading = (details) => detailsTo(details, { phase: 'loading', data: details?.data ?? null, error: null });

export function detailsRead(details, data) {
  if (!details) throw new Error('Bench details land only for a read that was begun');
  return detailsTo(details, { phase: 'loaded', data, error: null });
}

export function detailsFailed(details, error) {
  if (!details) throw new Error('Bench details fail only for a read that was begun');
  return detailsTo(details, { phase: 'failed', data: details.data, error: String(error.message || error) });
}

// The votes changed here (a vote cast, every vote cleared): the data the
// page holds takes them, whatever read is on its way.
export function detailsVotes(details, votes) {
  if (!details?.data) throw new Error('Bench details that were never read hold no votes');
  return { ...details, data: { ...details.data, votes } };
}
