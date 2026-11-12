'use strict';

// The prompt library: reusable build requests the user inserts with "/" in
// the composer. It lives on the server so Bench's prompt library and the builder
// models (through the prompt_library tool) work on the same list. Order is
// the user's order. Attachments are data URLs kept with their prompt.

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const file = path.join(path.resolve(process.env.MCBENCH_DATA_DIR || path.join(__dirname, '..', 'data')), 'prompts.json');

const SEED = [
  {
    id: 'medieval-house',
    title: 'Medieval house',
    text: 'Build a cozy medieval house with a stone ground floor, timber-framed upper floor, steep dark-oak roof, working chimney, furnished interior, and a compact kitchen garden. Make every side intentional and inspect the finished build before stopping.',
  },
  {
    id: 'cliff-lighthouse',
    title: 'Cliff lighthouse',
    text: 'Build a weathered lighthouse on a dramatic rocky cliff. Include a tapered tower, lantern room with a visible light source, keeper’s cottage, winding path, rough shoreline, and waves breaking against the rocks. Inspect it from at least two angles before stopping.',
  },
  {
    id: 'glass-cathedral',
    title: 'Glass cathedral',
    text: 'Build a compact Gothic cathedral centered on a long nave. Use flying buttresses, two distinct front towers, layered arches, a rose window, stained-glass side windows, an altar, pews, and deliberate interior lighting. Avoid flat walls and inspect both exterior and interior before stopping.',
  },
  {
    id: 'floating-island',
    title: 'Floating island',
    text: 'Build a floating island with an irregular stone underside, grassy top, a large windswept tree, a small ruined shrine, and a waterfall spilling into the void. Make the underside convincing from below and the top composition readable from a distant isometric view.',
  },
  {
    id: 'market-street',
    title: 'Night market',
    text: 'Build a lively medieval night market along a narrow street. Include at least six distinct stalls, cloth awnings, hanging signs, warm lantern lighting, varied goods, upper-floor windows, alley details, and a clear path through the scene. Keep it dense but readable and inspect the street from both ends.',
  },
];

// A prompt's attachment is a file as the composer reads one: its bytes are a
// base64 data URL, and nothing else the page could link to or load is kept.
function attachment(file) {
  if (typeof file?.name !== 'string' || typeof file.data !== 'string' || !/^data:[^;,]+;base64,/.test(file.data)) {
    throw Object.assign(new Error('A prompt attachment needs a name and its bytes as a base64 data URL'), { statusCode: 400 });
  }
  return { name: file.name, type: file.data.slice(5, file.data.indexOf(';')), size: Number(file.size) || 0, data: file.data };
}

const clean = (prompt) => ({
  id: String(prompt.id || crypto.randomUUID()),
  title: String(prompt.title || '').trim(),
  text: String(prompt.text || ''),
  tags: [...new Set((Array.isArray(prompt.tags) ? prompt.tags : []).map((tag) => String(tag).trim().toLowerCase()).filter(Boolean))],
  favorite: prompt.favorite === true,
  attachments: (Array.isArray(prompt.attachments) ? prompt.attachments : []).map(attachment),
  ...(prompt.usedAt ? { usedAt: Number(prompt.usedAt) } : {}),
  // A prompt a model added names the chat and the model it came from, so the
  // page can mark it and the model can tell its own prompts from the user's.
  ...(prompt.addedBy ? { addedBy: { chat: String(prompt.addedBy.chat), model: String(prompt.addedBy.model) } } : {}),
});

function list() {
  if (!fs.existsSync(file)) return SEED.map(clean);
  return JSON.parse(fs.readFileSync(file, 'utf8')).map(clean);
}

function save(prompts) {
  const next = prompts.map(clean);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  // Written whole and renamed into place, so a crash never leaves half a library.
  const temp = `${file}.${process.pid}.tmp`;
  fs.writeFileSync(temp, JSON.stringify(next, null, 2));
  fs.renameSync(temp, file);
  return next;
}

// What a model sees: everything but attachment bytes.
const forModel = (prompts) => prompts.map(({ attachments, ...prompt }) => ({
  ...prompt,
  ...(attachments.length ? { attachments: attachments.map((file) => file.name) } : {}),
}));

// One call reads the library, and can add prompts and change or remove the
// ones this chat's model added. `save` adds a prompt (no id) or changes the
// fields it passes of one by id; `remove` deletes by id. The user's own
// prompts, and those another chat or model added, are only read: a model
// that rewrote one would have its text sent under the user's title to every
// model of a later bench. `by` is the chat and model making the call.
const MODEL_FIELDS = ['title', 'text', 'tags'];

function apply({ save: entries = [], remove = [], ...rest } = {}, by) {
  if (Object.keys(rest).length) throw new Error(`prompt_library takes only save and remove; not ${Object.keys(rest).join(', ')}`);
  for (const [name, value] of [['save', entries], ['remove', remove]]) {
    if (!Array.isArray(value)) throw new Error(`${name} must be an array`);
  }
  const prompts = list();
  const byId = new Map(prompts.map((prompt) => [prompt.id, prompt]));
  const own = (id) => {
    const prompt = byId.get(id);
    if (!prompt) throw new Error(`No prompt with id ${id}`);
    if (prompt.addedBy?.chat !== by.chat || prompt.addedBy.model !== by.model) {
      throw new Error(`Prompt ${id} was not added by you in this chat, so you cannot change or remove it. Add a new prompt instead.`);
    }
    return prompt;
  };
  for (const id of remove) {
    own(id);
    byId.delete(id);
  }
  for (const { id, ...fields } of entries) {
    // Only these fields are the model's to write: the mark, favorites and
    // attachments stay as the server and the user set them.
    const unknown = Object.keys(fields).filter((key) => !MODEL_FIELDS.includes(key));
    if (unknown.length) throw new Error(`A prompt has only the fields id, ${MODEL_FIELDS.join(', ')}; not ${unknown.join(', ')}`);
    if (id === undefined && !String(fields.text || '').trim()) throw new Error('A new prompt needs text');
    const existing = id === undefined ? { id: crypto.randomUUID(), attachments: [], addedBy: by } : own(id);
    const next = clean({ ...existing, ...fields });
    byId.set(next.id, next);
  }
  return forModel(save([...byId.values()]));
}

const TOOL = {
  type: 'function',
  function: {
    name: 'prompt_library',
    description: `The user's library of reusable build prompts, shared across all chats. The user inserts them by typing / in the message box or from the prompt library in Bench. Parts written as {{like this}} are fill-ins the user types over each time.

Call with no arguments to read the whole library (id, title, text, tags, favorite, attachment names, and addedBy for a prompt a model added). You can add prompts, and change or remove only the prompts you added in this chat. The user's own prompts, and prompts added in other chats, are read only. Pass any combination of:
- save: prompts to add (omit id) or prompts you added to update (give id; only the fields you pass change)
- remove: ids of prompts you added, to delete
A prompt you add goes at the end of the library, and the user sees that a model added it. It returns the full library afterwards. Reuse existing tags, avoid near-duplicates, and add only what the user asked for or clearly expects.`,
    parameters: {
      type: 'object',
      properties: {
        save: {
          type: 'array',
          items: {
            type: 'object',
            properties: {
              id: { type: 'string' },
              title: { type: 'string' },
              text: { type: 'string' },
              tags: { type: 'array', items: { type: 'string' } },
            },
            additionalProperties: false,
          },
        },
        remove: { type: 'array', items: { type: 'string' } },
      },
      additionalProperties: false,
    },
  },
};

module.exports = { list, save, apply, TOOL };
