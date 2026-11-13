// What the page says of the first start (server/setup.js, GET /api/setup):
// the steps as rows, how many of the steps the app needs are done, and the
// line under the composer for a build sent before the app is ready. The
// view is the server's, read as it is: a step's state is waiting, running,
// done, failed or unavailable, and nothing else.

// Bytes in MB, as the downloads are sized (tools.js PINS: about 130 MB).
const mb = (bytes) => Math.round(bytes / 1e6);

// A download's progress ({ done, total } bytes, total null when the server
// did not say): "84 of 136 MB", or "84 MB".
export const megabytes = ({ done, total }) => (total ? `${mb(done)} of ${mb(total)} MB` : `${mb(done)} MB`);

// One step as a row: `label` what it makes, or what its work does while it
// runs; `value` the short word at its end (the download's MB while one
// runs); `note` the line under it: what a running step does now, or why a
// failed or unavailable one is not done.
export function stepRow(step) {
  const running = step.state === 'running';
  const value = {
    waiting: 'Waiting',
    running: step.progress ? megabytes(step.progress) : '',
    done: 'Done',
    failed: 'Failed',
    unavailable: 'Not available',
  }[step.state];
  if (value === undefined) throw new Error(`A setup step cannot be ${step.state}`);
  const note = running ? step.detail || null : step.error || null;
  return { id: step.id, state: step.state, optional: step.optional, label: running ? step.doing : step.title, value, note };
}

// The setup screen: the rows, and of the steps the app needs (optional ones
// are not counted) how many are done, and whether one failed.
export function setupStatus(view) {
  const needed = view.steps.filter((step) => !step.optional);
  return {
    ready: view.ready,
    done: needed.filter((step) => step.state === 'done').length,
    total: needed.length,
    failed: needed.some((step) => step.state === 'failed'),
    rows: view.steps.map(stepRow),
  };
}

// The line for a build sent before the app is ready: it is sent once the
// app is, which a failed step keeps from happening until it is tried again.
export function heldLine(status) {
  if (status.failed) return 'Not sent yet: a step failed. Try it again above.';
  return `Sends when the app is ready. ${status.done} of ${status.total} done.`;
}

// Text with its web addresses as links: [{ text }, { url }, …] in order.
// An address ends at a space or a closing bracket, and a full stop or comma
// after it is the sentence's.
export function linkParts(text) {
  const parts = [];
  let at = 0;
  for (const match of text.matchAll(/https?:\/\/[^\s)]+[^\s).,]/g)) {
    if (match.index > at) parts.push({ text: text.slice(at, match.index) });
    parts.push({ url: match[0] });
    at = match.index + match[0].length;
  }
  if (at < text.length) parts.push({ text: text.slice(at) });
  return parts;
}
