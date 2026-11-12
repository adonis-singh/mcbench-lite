'use strict';

// Converts Mojang's ProGuard mapping file (client.txt: Mojang names -> official
// obfuscated names) into a Tiny v1 mapping (official -> named) for
// tiny-remapper. Usage: node proguard-to-tiny.js <client.txt> <out.tiny>

const fs = require('fs');

const [input, output] = process.argv.slice(2);
const lines = fs.readFileSync(input, 'utf8').split('\n');

const classToObf = new Map();
for (const line of lines) {
  const match = line.match(/^(\S+) -> (\S+):$/);
  if (match) classToObf.set(match[1], match[2]);
}

const PRIMITIVES = { int: 'I', long: 'J', double: 'D', float: 'F', boolean: 'Z', byte: 'B', short: 'S', char: 'C', void: 'V' };
function descriptor(type) {
  let dims = 0;
  while (type.endsWith('[]')) {
    dims += 1;
    type = type.slice(0, -2);
  }
  const base = PRIMITIVES[type] || `L${(classToObf.get(type) || type).replaceAll('.', '/')};`;
  return '['.repeat(dims) + base;
}
const methodDescriptor = (ret, args) => `(${args ? args.split(',').map(descriptor).join('') : ''})${descriptor(ret)}`;

const out = ['v1\tofficial\tnamed'];
let owner = null;
for (const line of lines) {
  if (line.startsWith('#') || !line.trim()) continue;
  let match = line.match(/^(\S+) -> (\S+):$/);
  if (match) {
    owner = classToObf.get(match[1]).replaceAll('.', '/');
    out.push(`CLASS\t${owner}\t${match[1].replaceAll('.', '/')}`);
    continue;
  }
  match = line.match(/^\s+(?:\d+:\d+:)?(\S+) (\S+)\((.*)\)(?::\d+(?::\d+)?)? -> (\S+)$/);
  if (match) {
    if (match[2] === '<init>' || match[2] === '<clinit>') continue;
    out.push(`METHOD\t${owner}\t${methodDescriptor(match[1], match[3])}\t${match[4]}\t${match[2]}`);
    continue;
  }
  match = line.match(/^\s+(\S+) (\S+) -> (\S+)$/);
  if (match) {
    out.push(`FIELD\t${owner}\t${descriptor(match[1])}\t${match[3]}\t${match[2]}`);
    continue;
  }
  throw new Error(`unparsed mapping line: ${line}`);
}
fs.writeFileSync(output, `${out.join('\n')}\n`);
