// The share of one subject: a chat's build, or a bench's builds on one page
// where a visitor switches between the models, published to a public GitHub
// Pages repository (server/share.js). A subject is { kind: 'chat' | 'bench',
// id }. One value per subject:
//   unknown    { phase }: not read yet
//   loading    { phase, url }: the server being asked whether it is shared
//   none       { phase, url }: not shared; `url` is null
//   shared     { phase, url }: live at `url`
//   exporting, publishing, waiting   { phase, url }: a publish on its way,
//              the step the server last said. `url` is the one it had
//              before, or null
//   failed     { phase, url, error }: the publish failed (`error` is the
//              server's message); `url` is still the one it had, if any
//   removing   { phase, url }: the page being taken down
export const SHARE_UNKNOWN = Object.freeze({ phase: 'unknown' });

const SUBJECT_PATHS = { chat: 'conversations', bench: 'benches' };

function subjectPath(subject) {
  const path = SUBJECT_PATHS[subject.kind];
  if (!path) throw new Error(`${JSON.stringify(subject.kind)} is not a kind of share (chat, bench)`);
  return path;
}

// The page's key for a subject's share, and its endpoint on the server.
export function shareKey(subject) {
  subjectPath(subject);
  return `${subject.kind}:${subject.id}`;
}
export const shareUrl = (subject) => `/api/${subjectPath(subject)}/${subject.id}/share`;

// A click starts a publish on exporting, and the server's first line says
// exporting again.
const NEXT = {
  unknown: ['loading'],
  loading: ['none', 'shared', 'unknown'],
  none: ['exporting'],
  shared: ['exporting', 'removing'],
  exporting: ['exporting', 'publishing', 'failed'],
  publishing: ['waiting', 'failed'],
  waiting: ['shared', 'failed'],
  failed: ['exporting'],
  removing: ['none', 'shared'],
};

const PUBLISHING = ['exporting', 'publishing', 'waiting'];

function shareTo(share, next) {
  if (!NEXT[share.phase].includes(next.phase)) throw new Error(`The share cannot go from ${share.phase} to ${next.phase}`);
  return next;
}

export const shareReading = (share) => shareTo(share, { phase: 'loading', url: null });
// `read` is the server's { url }.
export const shareRead = (share, read) => shareTo(share, { phase: read.url ? 'shared' : 'none', url: read.url });
// The read failed: unknown again, to be read on the next opening.
export const shareUnread = (share) => shareTo(share, SHARE_UNKNOWN);

export const sharePublishing = (share) => shareTo(share, { phase: 'exporting', url: share.url });

// One line of the publish stream.
export function shareStep(share, message) {
  if (message.state === 'live') return shareTo(share, { phase: 'shared', url: message.url });
  if (message.state === 'failed') return shareTo(share, { phase: 'failed', url: share.url, error: message.error });
  return shareTo(share, { phase: message.state, url: share.url });
}

// The publish was refused, or its request broke.
export const shareFailed = (share, error) => shareTo(share, { phase: 'failed', url: share.url, error: String(error.message || error) });

// The stream ended. One that ended before saying live or failed failed.
export function shareStoppedEarly(share) {
  if (!PUBLISHING.includes(share.phase)) return share;
  return shareFailed(share, 'The publish stopped before the page was live');
}

export const shareRemoving = (share) => shareTo(share, { phase: 'removing', url: share.url });
export const shareRemoved = (share) => shareTo(share, { phase: 'none', url: null });
// The removal failed: the page is still up.
export const shareKept = (share) => shareTo(share, { phase: 'shared', url: share.url });

export const sharePublishingNow = (share) => PUBLISHING.includes(share.phase);

const STEP_TEXT = {
  exporting: 'Preparing the build…',
  publishing: 'Uploading…',
  waiting: 'Waiting for GitHub Pages…',
};

export function shareStepText(share) {
  if (!PUBLISHING.includes(share.phase)) throw new Error(`A share that is ${share.phase} is not publishing`);
  return STEP_TEXT[share.phase];
}

export const postUrl = (url) => `https://x.com/intent/post?url=${encodeURIComponent(url)}`;

// Reads the publish's NDJSON body, one parsed message per line, until it ends.
export async function readShareStream(body, onMessage) {
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  for (;;) {
    const { done, value } = await reader.read();
    if (done) return;
    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split('\n');
    buffer = lines.pop();
    for (const line of lines) if (line.trim()) onMessage(JSON.parse(line));
  }
}
