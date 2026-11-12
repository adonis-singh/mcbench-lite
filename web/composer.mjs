// The message composer's content: plain text with two kinds of inline atoms.
// A prompt token (/Medieval house) stands for a saved prompt and sends its
// text; a field is a small box to type into, made from a prompt's
// {{fill-in}}. The DOM is the source of truth while typing; segments are its
// serializable form for drafts:
//   { type: 'text', text } | { type: 'prompt', id, title, text } | { type: 'field', name, value }

export const FILL_IN = /\{\{([^{}\n]*)\}\}/g;

// A prompt with fill-ins arrives as its text with boxes in place of them;
// one without is a single token.
export function promptSegments(prompt) {
  const text = prompt.text || '';
  FILL_IN.lastIndex = 0;
  if (!FILL_IN.test(text)) return [{ type: 'prompt', id: prompt.id, title: prompt.title || 'Untitled prompt', text }];
  const segments = [];
  let last = 0;
  FILL_IN.lastIndex = 0;
  for (const match of text.matchAll(FILL_IN)) {
    if (match.index > last) segments.push({ type: 'text', text: text.slice(last, match.index) });
    segments.push({ type: 'field', name: match[1].trim(), value: '' });
    last = match.index + match[0].length;
  }
  if (last < text.length) segments.push({ type: 'text', text: text.slice(last) });
  return segments;
}

function sizeField(input) {
  input.style.width = `${Math.max(input.value.length, input.placeholder.length, 2) + 1}ch`;
}

function atom(segment) {
  const node = document.createElement('span');
  node.contentEditable = 'false';
  if (segment.type === 'prompt') {
    node.className = 'ce-prompt';
    node.dataset.id = segment.id;
    node.dataset.title = segment.title;
    node.dataset.text = segment.text;
    node.title = segment.text;
    const remove = document.createElement('button');
    remove.type = 'button';
    remove.tabIndex = -1;
    remove.className = 'ce-remove';
    remove.setAttribute('aria-label', `Remove /${segment.title}`);
    remove.innerHTML = '<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18"/></svg>';
    const slash = document.createElement('span');
    slash.className = 'ce-slash';
    slash.textContent = '/';
    slash.append(remove);
    node.append(slash, segment.title);
  } else {
    node.className = 'ce-field';
    const input = document.createElement('input');
    input.className = 'ce-field-input';
    input.placeholder = segment.name;
    input.value = segment.value;
    input.spellcheck = false;
    sizeField(input);
    input.addEventListener('input', () => sizeField(input));
    node.append(input);
  }
  return node;
}

export function nodesFor(segments) {
  return segments.map((segment) => (segment.type === 'text' ? document.createTextNode(segment.text) : atom(segment)));
}

export function write(root, segments) {
  root.replaceChildren(...nodesFor(segments));
}

// Segments, the text a send carries (tokens expanded, boxes filled), and the
// first box left empty.
export function read(root) {
  const segments = [];
  let text = '';
  let emptyField = null;
  const pushText = (raw) => {
    const value = raw.replace(/\u200b/g, '');
    if (!value) return;
    const last = segments[segments.length - 1];
    if (last?.type === 'text') last.text += value;
    else segments.push({ type: 'text', text: value });
    text += value;
  };
  const walk = (parent) => {
    for (const node of parent.childNodes) {
      if (node.nodeType === Node.TEXT_NODE) pushText(node.data);
      else if (node.nodeName === 'BR') pushText('\n');
      else if (node.classList?.contains('ce-prompt')) {
        segments.push({ type: 'prompt', id: node.dataset.id, title: node.dataset.title, text: node.dataset.text });
        text += node.dataset.text;
      } else if (node.classList?.contains('ce-field')) {
        const input = node.querySelector('input');
        segments.push({ type: 'field', name: input.placeholder, value: input.value });
        if (!input.value.trim()) emptyField ??= input;
        text += input.value;
      } else {
        if (/^(DIV|P)$/.test(node.nodeName) && text && !text.endsWith('\n')) pushText('\n');
        walk(node);
      }
    }
  };
  walk(root);
  return { segments, text, emptyField };
}

// "/query" just before the caret, at the start or after whitespace.
export function slashAtCaret(root) {
  const selection = window.getSelection();
  if (!selection.rangeCount || !selection.isCollapsed) return null;
  const { startContainer: node, startOffset: offset } = selection.getRangeAt(0);
  if (node.nodeType !== Node.TEXT_NODE || !root.contains(node)) return null;
  const match = /(?:^|\s)\/([^\s/]*)$/.exec(node.data.slice(0, offset));
  if (!match) return null;
  return { node, start: offset - match[1].length - 1, query: match[1] };
}

// Puts segments where `range` is (replacing what it covers), leaves the caret
// after them, and returns the first empty box so the caller can focus it.
export function insert(root, segments, range) {
  range.deleteContents();
  const nodes = nodesFor(segments);
  const tail = document.createTextNode('​');
  const fragment = document.createDocumentFragment();
  fragment.append(...nodes, tail);
  range.insertNode(fragment);
  // The zero-width space gives the caret a text position after a trailing atom.
  const selection = window.getSelection();
  const after = document.createRange();
  after.setStart(tail, 1);
  after.collapse(true);
  selection.removeAllRanges();
  selection.addRange(after);
  root.normalize();
  return nodes.find((node) => node.classList?.contains('ce-field'))?.querySelector('input') || null;
}

export function rangeAtEnd(root) {
  const range = document.createRange();
  range.selectNodeContents(root);
  range.collapse(false);
  return range;
}

// The caret's range if it is inside the composer.
export function caretRange(root) {
  const selection = window.getSelection();
  if (!selection.rangeCount) return null;
  const range = selection.getRangeAt(0);
  return root.contains(range.startContainer) ? range.cloneRange() : null;
}

export function fields(root) {
  return [...root.querySelectorAll('.ce-field-input')];
}
