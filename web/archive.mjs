// The sidebar's archived items, shown for one mode at a time. One value:
//   closed   { phase }
//   loading  { phase, mode, data, error }: being read. `data` is what an
//            earlier read gave, still shown meanwhile, or null
//   loaded   { phase, mode, data, error }: `data` is the server's
//            { conversations, benches }
//   failed   { phase, mode, data, error }: the read failed (`error` says
//            why), and it can be read again
// Any read can be followed by another, which supersedes it.
export const ARCHIVE_CLOSED = Object.freeze({ phase: 'closed' });

const NEXT = {
  closed: ['loading'],
  loading: ['loading', 'loaded', 'failed', 'closed'],
  loaded: ['loading', 'closed'],
  failed: ['loading', 'closed'],
};

function archiveTo(archive, next) {
  if (!NEXT[archive.phase].includes(next.phase)) throw new Error(`The archive cannot go from ${archive.phase} to ${next.phase}`);
  return next;
}

// A read for `mode`. What was read before stays on show only in the same mode.
export const archiveReading = (archive, mode) => archiveTo(archive, {
  phase: 'loading', mode, data: archive.phase !== 'closed' && archive.mode === mode ? archive.data : null, error: null,
});
export const archiveRead = (archive, data) => archiveTo(archive, { ...archive, phase: 'loaded', data, error: null });
export const archiveFailed = (archive, error) => archiveTo(archive, { ...archive, phase: 'failed', error: String(error.message || error) });
export const archiveClosed = (archive) => archiveTo(archive, ARCHIVE_CLOSED);

// The archived rows of the archive's mode, or null before the first read.
export function archivedRows(archive) {
  if (archive.phase === 'closed') throw new Error('A closed archive shows no rows');
  if (!archive.data) return null;
  return archive.mode === 'bench' ? archive.data.benches : archive.data.conversations;
}

const DAY = 24 * 60 * 60 * 1000;

// How long an archived item has before it is deleted, in whole days.
export function timeLeft(expiresAt, now) {
  const days = Math.floor((expiresAt - now) / DAY);
  if (days < 1) return 'less than a day left';
  return days === 1 ? '1 day left' : `${days} days left`;
}
