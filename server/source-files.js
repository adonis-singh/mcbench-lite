'use strict';

// The source files a version is the hash of (engine.js and draw.js
// sourcesHash): in a git checkout the files git tracks, so a file an editor
// or the operating system leaves beside them changes no version; in a copy
// without git (an archive) every file but the operating system's droppings.

const fs = require('fs');
const path = require('path');
const { execFile } = require('child_process');
const { promisify } = require('util');

const execFileAsync = promisify(execFile);
const REPOSITORY = path.join(__dirname, '..');

// Files an operating system or an editor leaves beside sources, never
// sources: macOS's folder settings and resource forks (._*), Windows'
// thumbnails and folder settings, and editors' backups (*~), Vim's swap files
// (*.swp) and Emacs's autosaves (#*#).
const DROPPINGS = ['.DS_Store', 'Thumbs.db', 'desktop.ini'];
const dropping = (name) => DROPPINGS.includes(name) || name.startsWith('._') || name.endsWith('~') || name.endsWith('.swp') || (name.startsWith('#') && name.endsWith('#'));

// The files of `paths` (folders and files, relative to `root`, a folder of
// this repository), relative to `root` and sorted. `skip`: folders (relative
// to `root`) a copy without git leaves out, as git ignores them.
async function trackedFiles(root, paths, { skip = [] } = {}) {
  if (fs.existsSync(path.join(REPOSITORY, '.git'))) {
    const { stdout } = await execFileAsync('git', ['ls-files', '-z', '--', ...paths], { cwd: root, maxBuffer: 1 << 26 });
    return stdout.split('\0').filter(Boolean).sort();
  }
  const files = [];
  const walk = async (relative) => {
    const stat = await fs.promises.stat(path.join(root, relative));
    if (!stat.isDirectory()) return files.push(relative);
    for (const entry of await fs.promises.readdir(path.join(root, relative))) {
      const name = path.posix.join(relative, entry);
      if (!dropping(entry) && !skip.includes(name)) await walk(name);
    }
  };
  for (const relative of paths) await walk(relative);
  return files.sort();
}

module.exports = { trackedFiles, DROPPINGS };
