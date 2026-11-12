function unescapeJson(value) {
  return value.replace(/\\u([0-9a-fA-F]{4})|\\(.)/g, (match, unicode, escaped) => (
    unicode
      ? String.fromCharCode(parseInt(unicode, 16))
      : ({ n: '\n', t: '\t', r: '\r', b: '\b', f: '\f' }[escaped] || escaped)
  ));
}

// Where the JSON string starting at `start` (its opening quote) ends: the
// index of its closing quote, or -1 while it is still streaming.
function stringEnd(raw, start) {
  for (let index = start + 1; index < raw.length; index += 1) {
    if (raw[index] === '\\') index += 1;
    else if (raw[index] === '"') return index;
  }
  return -1;
}

// A top-level string field of a JSON object still streaming in: only keys of
// the outer object count, never one inside a nested object or a string.
function extractString(raw, key) {
  let depth = 0;
  for (let index = 0; index < raw.length; index += 1) {
    const char = raw[index];
    if (char === '{' || char === '[') depth += 1;
    else if (char === '}' || char === ']') depth -= 1;
    if (char !== '"') continue;
    const end = stringEnd(raw, index);
    if (end < 0) return undefined;
    const value = /^\s*:\s*"/.exec(raw.slice(end + 1));
    if (depth === 1 && value && raw.slice(index + 1, end) === key) return streamedString(raw.slice(end + 1 + value[0].length));
    index = end;
  }
  return undefined;
}

// A string value's text, closed or still streaming.
function streamedString(rest) {
  const end = stringEnd(`"${rest}`, 0);
  let body = end >= 0 ? rest.slice(0, end - 1) : rest;
  if (end < 0) {
    const trailing = body.match(/\\+$/);
    if (trailing && trailing[0].length % 2 === 1) body = body.slice(0, -1);
    body = body.replace(/\\u[0-9a-fA-F]{0,3}$/, '');
  }
  return unescapeJson(body);
}

function unwrapArguments(value) {
  let args = value;
  for (let depth = 0; depth < 3; depth += 1) {
    const nested = args && typeof args === 'object'
      ? (args.arguments ?? args.function?.arguments)
      : undefined;
    if (nested === undefined) break;
    if (typeof nested === 'string') {
      try {
        args = JSON.parse(nested);
      } catch {
        break;
      }
    } else if (nested && typeof nested === 'object') {
      args = nested;
    } else {
      break;
    }
  }
  return args;
}

export function parseToolArgs(raw) {
  if (!raw) return {};
  try {
    const args = unwrapArguments(JSON.parse(raw));
    return {
      summary: args?.summary,
      program: args?.program ?? args?.command,
      path: args?.path,
    };
  } catch {
    return {
      summary: extractString(raw, 'summary'),
      program: extractString(raw, 'program') ?? extractString(raw, 'command'),
      path: extractString(raw, 'path'),
    };
  }
}

// Keep the last successfully decoded fields on the item itself, so rendering
// never depends on reparsing one transient fragment correctly every frame.
export function mergeToolCallDelta(item, event) {
  const argsRaw = `${item.argsRaw || ''}${event.args || ''}`;
  const parsed = parseToolArgs(argsRaw);
  return {
    ...item,
    tool: event.name || item.tool,
    argsRaw,
    summary: parsed.summary ?? item.summary ?? '',
    program: parsed.program ?? item.program,
  };
}
