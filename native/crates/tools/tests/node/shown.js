'use strict';

// What today's server and today's page show of every stored tool call and
// tool result of a data folder, for tests/shown.rs to compare with
// mcbench-tools' call_shown and tool_shown.
//
//   node shown.js <data folder clone> <out.jsonl>
//
// The page's reading is web/app.js itemsFromMessages, evaluated from its
// source text as it is. The server's is agent.js runCall's argument parse,
// its tool_start summary and program, and its tool_result output, each
// evaluated from the source text of those lines. One line per stored call
// ({kind: 'call'}) and per stored tool result ({kind: 'result'}). The output
// holds real chat text: it is written under the scratch folder, never in the
// repository.

const fs = require('fs');
const path = require('path');
const Database = require('better-sqlite3');

const root = path.resolve(__dirname, '../../../../..');
const [dataDir, outFile] = process.argv.slice(2);
if (!dataDir || !outFile) throw new Error('usage: node shown.js <data folder clone> <out.jsonl>');

// The page's itemsFromMessages, from app.js's own text.
const app = fs.readFileSync(path.join(root, 'web/app.js'), 'utf8');
const pageStart = app.indexOf('function itemsFromMessages(messages) {');
const pageEnd = app.indexOf('\n}\n', pageStart) + 2;
if (pageStart < 0 || pageEnd < pageStart) throw new Error('itemsFromMessages not found in web/app.js');
const itemsFromMessages = new Function(`${app.slice(pageStart, pageEnd)}\nreturn itemsFromMessages;`)();

// The server's lines, from agent.js's own text.
const agent = fs.readFileSync(path.join(root, 'server/agent.js'), 'utf8');
const between = (from, to) => {
  const start = agent.indexOf(from);
  const end = agent.indexOf(to, start);
  if (start < 0 || end < 0) throw new Error(`agent.js no longer has ${JSON.stringify(from)} .. ${JSON.stringify(to)}`);
  return agent.slice(start, end);
};
const defaultShots = between('const DEFAULT_SHOTS = ', ';\n');
const parseAndSummary = between('const name = call.function && call.function.name;', '      emit({\n        type: \'tool_start\'');
const programLine = between('program: args && (', '\n');
const outputLine = between("output: name === 'bash'", '\n');
const serverStart = new Function('call', `${defaultShots};\n${parseAndSummary}\nreturn { summary, ${programLine.replace(/,$/, '')} };`);
const serverResult = new Function('name', 'result', `return { ok: result.ok, cancelled: result.cancelled, error: result.error, diff: result.diff, ${outputLine.replace(/,$/, '')} };`);

const db = new Database(path.join(dataDir, 'mcbench.sqlite3'), { readonly: true, fileMustExist: true });
const rows = db.prepare('SELECT id, conversation_id, parent_id, position, payload_json FROM messages ORDER BY conversation_id, position').all();
const out = fs.openSync(outFile, 'w');
const write = (value) => fs.writeSync(out, `${JSON.stringify(value)}\n`);

// Each message with the assistant message whose call it answers: the page
// reads a tool result only after its call (itemsFromMessages over the pair).
const calls = new Map();
let callCount = 0;
let resultCount = 0;
for (const row of rows) {
  const msg = { ...JSON.parse(row.payload_json), id: row.id };
  if (msg.role === 'assistant') {
    for (const call of msg.tool_calls || []) {
      calls.set(call.id, call);
      const items = itemsFromMessages([{ role: 'assistant', content: '', tool_calls: [call] }]);
      const item = items.find((entry) => entry.kind === 'tool');
      write({
        kind: 'call',
        name: call.function.name,
        arguments: call.function.arguments,
        page: { summary: item.summary, program: item.program ?? null },
        server: (({ summary, program }) => ({ summary, program: program ?? null }))(serverStart(call)),
      });
      callCount += 1;
    }
  } else if (msg.role === 'tool') {
    const call = calls.get(msg.tool_call_id);
    if (!call) continue;
    const name = call.function.name;
    const items = itemsFromMessages([{ role: 'assistant', content: '', tool_calls: [call] }, msg]);
    const item = items.find((entry) => entry.kind === 'tool');
    let result = null;
    try {
      result = JSON.parse(msg.content);
    } catch {
      if (name === 'bash') result = { ok: true, text: msg.content };
    }
    const server = result && typeof result === 'object' ? serverResult(name, result) : null;
    write({
      kind: 'result',
      name,
      content: msg.content,
      page: { status: item.status, diff: item.diff ?? null, output: item.output ?? null, detail: item.detail },
      server: server && {
        ok: server.ok ?? null,
        cancelled: server.cancelled ?? null,
        error: server.error ?? null,
        diff: server.diff ?? null,
        output: server.output ?? null,
      },
    });
    resultCount += 1;
  }
}
fs.closeSync(out);
console.log(JSON.stringify({ calls: callCount, results: resultCount }));
