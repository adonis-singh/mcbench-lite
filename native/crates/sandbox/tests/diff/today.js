'use strict';
// node today.js <cases.json> <scratch> <chat id> [only chat]
//
// The differential test's Node side: the cases of one chat ("main" or
// "tight") through today's server/sandbox.js on Docker, in order, one JSON
// line each: { id, exitCode, formatted, storage, error, png }. The tight
// chat runs in a process of its own with MCBENCH_CHAT_MAX_BYTES of 1 MiB
// (storage.js reads it at load). Every container is removed at the end.

const fs = require('node:fs');
const path = require('node:path');

// Today's names are fixed in server/sandbox.js (IMAGE, containerName):
// never on the owner's machines (tests/diff/RESULTS.md keeps the results).
if (process.env.MCBENCH_I_OWN_THIS_DOCKER !== '1') {
  console.error('today.js refused: it runs today\'s server/sandbox.js on Docker, which makes containers named as the app\'s own (mcb-v3-..., mcb-net-...) from the image localhost/mcbench-sandbox:<hash>, names that cannot be changed without editing server/; it is for a machine that is not the owner\'s (CI): set MCBENCH_I_OWN_THIS_DOCKER=1 there');
  process.exit(2);
}

const [casesFile, scratch, chat, only] = process.argv.slice(2);
process.env.MCBENCH_DATA_DIR = scratch;
const repo = path.resolve(__dirname, '../../../../..');
const sandbox = require(path.join(repo, 'server/sandbox'));
const storage = require(path.join(repo, 'server/storage'));
const cases = JSON.parse(fs.readFileSync(casesFile, 'utf8')).filter((c) => (c.chat || 'main') === only);
const big = 'x'.repeat(60000);

(async () => {
  for (const c of cases) {
    const line = { id: c.id };
    try {
      if (c.measure) await storage.measure(chat);
      if (c.op === 'view_image') {
        const png = await sandbox.viewImage(chat, c.path);
        line.png = `${png.length} bytes, sha256 ${require('node:crypto').createHash('sha256').update(png).digest('hex')}`;
      }
      else if (c.op === 'remove') await sandbox.removeWorkFiles(chat, c.paths);
      else if (c.op === 'disable_network') sandbox.disableNetwork();
      else {
        const result = await sandbox.exec(chat, c.command.replace('REPLACED_BY_BIG', big), c.timeout_ms ?? 10_000, { network: c.network === true });
        // A cut through a surrogate pair leaves a lone surrogate, which JSON
        // here (and Rust's String there) holds as U+FFFD; it is flagged.
        const formatted = result.formatted.toWellFormed();
        Object.assign(line, { exitCode: result.exitCode, formatted, storage: result.storage ?? null });
        if (formatted !== result.formatted) line.loneSurrogates = 'true';
      }
    } catch (error) {
      line.error = error.message;
    }
    process.stdout.write(JSON.stringify(line) + '\n');
  }
  await sandbox.destroy(chat);
  process.exit(0);
})();
