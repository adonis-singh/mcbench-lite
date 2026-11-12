'use strict';

// The corpus of builds the parity and benchmark scripts read in their corpus
// modes: the directories MCBENCH_CORPUS names, separated by the path
// delimiter (':' on macOS and Linux). A build is an artifact.json in a folder
// under an imports/ folder, named by that folder. A corpus mode without
// MCBENCH_CORPUS stops the script with this message.

const fs = require('fs');
const path = require('path');

function corpusDirs() {
  if (!process.env.MCBENCH_CORPUS) {
    console.error('This mode reads a corpus of builds: set MCBENCH_CORPUS to its directory (every <dir>/**/imports/<build>/artifact.json is one build; several directories are separated by ":").');
    process.exit(2);
  }
  return process.env.MCBENCH_CORPUS.split(path.delimiter).map((dir) => path.resolve(dir));
}

// Every build of the corpus as [build name, artifact.json path], in the
// order the directories are walked.
function corpusArtifacts() {
  const builds = [];
  const walk = (folder) => {
    for (const entry of fs.readdirSync(folder, { withFileTypes: true })) {
      const file = path.join(folder, entry.name);
      if (entry.isDirectory()) walk(file);
      else if (entry.name === 'artifact.json' && folder.includes(`${path.sep}imports${path.sep}`)) builds.push([path.basename(folder), file]);
    }
  };
  for (const dir of corpusDirs()) walk(dir);
  return builds;
}

// The artifact of the build named `name`. Every copy of it in the corpus
// must be the same file, or the script stops.
function corpusArtifact(name) {
  const files = corpusArtifacts().filter(([build]) => build === name).map(([, file]) => file);
  if (!files.length) throw new Error(`No build ${name} in MCBENCH_CORPUS (${process.env.MCBENCH_CORPUS})`);
  const texts = new Set(files.map((file) => fs.readFileSync(file, 'utf8')));
  if (texts.size > 1) throw new Error(`MCBENCH_CORPUS holds different builds named ${name}: ${files.join(', ')}`);
  return JSON.parse([...texts][0]);
}

module.exports = { corpusDirs, corpusArtifacts, corpusArtifact };
