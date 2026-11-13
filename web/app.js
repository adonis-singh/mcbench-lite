import { h, html, render, useState, useEffect, useLayoutEffect, useMemo, useRef, useCallback, useErrorBoundary } from '/vendor/preact-htm.mjs';
import { groupItems, entryKeys, applyEvent, IDLE, turnTo, turnRunning, turnToJoin, turnOver, LOADING, SHOWN, readFailed, dismissError } from '/transcript.mjs';
import { Shared } from '/shared.mjs';
import { topLayer, modalOpen, bindRuns } from '/layers.mjs';
import { EMPTY_DRAFT, draftEmpty, draftFiles, holdsRead, beginRead, endRead, Drafts } from '/draft.mjs';
import { placeKeys, withRow, asChat, chatDraft, benchDraft, NEW_CHAT_DRAFT, NEW_BENCH_DRAFT } from '/place.mjs';
import { PromptSync, promptsLoaded } from '/prompt-sync.mjs';
import { RESOLUTIONS, resolutionStop, resolutionHelp } from '/resolution.mjs';
import { Favorites } from '/favorites.mjs';
import { CLOSED, panelTo, panelEdit, panelDraft, withTitleModel, withShareRepo, usable, CONNECT, firstPane, connectWays } from '/settings-panel.mjs';
import { megabytes, setupStatus, heldLine, linkParts } from '/setup-status.mjs';
import { SHARE_UNKNOWN, shareKey, shareUrl, shareReading, shareRead, shareUnread, sharePublishing, shareStep, shareFailed, shareStoppedEarly, shareRemoving, shareRemoved, shareKept, sharePublishingNow, shareStepText, postUrl, readShareStream } from '/share.mjs';
import { detailsReading, detailsRead, detailsFailed, detailsVotes } from '/bench-details.mjs';
import { paintUpload } from '/upload-image.mjs';
import { logoName, vendorIcon } from '/vendor-icon.mjs';
import { ARCHIVE_CLOSED, archiveReading, archiveRead, archiveFailed, archiveClosed, archivedRows, timeLeft } from '/archive.mjs';
import { PUBLISHED_UNREAD, publishedReading, publishedRead, publishedFailed, publishedClosed, publishedView, publishedRemoving, publishedRemoved, publishedKept, publishedAgo, publishedSubject } from '/published.mjs';
import { highlighted, stepped } from '/command.mjs';
import { writeMessageClipboard, readMessageClipboard, insertClipboardText, readBlobAsDataUrl } from '/clipboard.mjs';
import * as composer from '/composer.mjs';
import { LiveWorld } from '/live-world.mjs';
import { browserHost } from '/live-host.mjs';

async function api(path, options) {
  const res = await fetch(path, options);
  const body = await res.json();
  if (!res.ok) throw new Error(body.error || `HTTP ${res.status}`);
  return body;
}

// Reads an NDJSON response, one parsed message per line, until it ends or
// the signal aborts.
async function readLines(res, signal, onMessage) {
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  for (;;) {
    const { done, value } = await reader.read();
    if (signal.aborted || done) return;
    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split('\n');
    buffer = lines.pop();
    for (const line of lines) if (line.trim()) onMessage(JSON.parse(line));
  }
}

// Server data that several views read (shared.mjs).
const shared = new Shared(api);
const load = (url, options) => shared.load(url, options);

// Subscribes to every url in the list; null entries are skipped.
function useShared(urls) {
  const [, rerender] = useState(0);
  const live = urls.filter(Boolean);
  useEffect(() => {
    const listener = () => rerender((n) => n + 1);
    const entries = live.map((url) => shared.entry(url));
    for (const entry of entries) entry.listeners.add(listener);
    for (const url of live) load(url).catch(() => {});
    return () => { for (const entry of entries) entry.listeners.delete(listener); };
  }, [live.join('\n')]);
  return urls.map((url) => (url ? shared.entries.get(url) || { value: undefined, error: null } : null));
}

const modelsUrl = (provider) => `/api/models?provider=${encodeURIComponent(provider)}`;
const modelInfoUrl = (provider, model) => (provider && model
  ? `/api/model-info?provider=${encodeURIComponent(provider)}&model=${encodeURIComponent(model)}` : null);

function useModelInfo(provider, model) {
  return useShared([modelInfoUrl(provider, model)])[0]?.value || null;
}

// The effort a request actually uses: the chosen one if this model offers
// it, otherwise the model's default ('').
const effortFor = (caps, effort) => (caps?.reasoning && caps.efforts.includes(effort) ? effort : '');

// Minimal markdown for LLM text: **bold**, *italic*, `code`, bullet and
// numbered lists. Renders to nodes, never HTML strings.
function renderInline(text) {
  const nodes = [];
  // Emphasis must hug its text (no "a * b") and _ only works at word edges.
  const re = /(`[^`\n]+`)|(\*\*[^*\n]+\*\*)|(\*(?!\s)(?:[^*\n]*?\S)?\*)|((?<![\w_])_(?!\s)(?:[^_\n]*?\S)?_(?![\w_]))/g;
  let last = 0;
  let match;
  while ((match = re.exec(text))) {
    if (match.index > last) nodes.push(text.slice(last, match.index));
    const token = match[0];
    if (token.startsWith('`')) nodes.push(html`<code>${token.slice(1, -1)}</code>`);
    else if (token.startsWith('**')) nodes.push(html`<strong>${token.slice(2, -2)}</strong>`);
    else nodes.push(html`<em>${token.slice(1, -1)}</em>`);
    last = match.index + token.length;
  }
  if (last < text.length) nodes.push(text.slice(last));
  return nodes;
}

// Parsed once per distinct text: a streaming turn re-renders the whole
// transcript per token, and Preact skips subtrees whose vnodes are reused.
function Markdown({ text }) {
  return useMemo(() => renderMarkdown(text), [text]);
}

function renderMarkdown(text) {
  // Models pad turns with stray newlines; trim the edges and collapse runs
  // of blank lines so paragraphs breathe evenly instead of gaping.
  const lines = [];
  for (const line of String(text || '').split('\n')) {
    if (line.trim() === '' && (lines.length === 0 || lines[lines.length - 1].trim() === '')) continue;
    lines.push(line);
  }
  while (lines.length && lines[lines.length - 1].trim() === '') lines.pop();

  const blocks = [];
  let list = null;
  const flush = () => { if (list) { blocks.push(list); list = null; } };
  for (const line of lines) {
    const bullet = line.match(/^\s*[-*•]\s+(.*)/);
    const numbered = line.match(/^\s*\d+[.)]\s+(.*)/);
    const heading = line.match(/^\s*#{1,4}\s+(.*)/);
    if (bullet) {
      if (!list || list.ordered) { flush(); list = { ordered: false, items: [] }; }
      list.items.push(bullet[1]);
    } else if (numbered) {
      if (!list || !list.ordered) { flush(); list = { ordered: true, items: [] }; }
      list.items.push(numbered[1]);
    } else {
      flush();
      blocks.push({ line, heading: heading ? heading[1] : null });
    }
  }
  flush();
  return html`<div class="md">${blocks.map((block) => {
    if (block.items) {
      const items = block.items.map((item) => html`<li>${renderInline(item)}</li>`);
      return block.ordered ? html`<ol>${items}</ol>` : html`<ul>${items}</ul>`;
    }
    if (block.heading !== null) return html`<div class="md-h">${renderInline(block.heading)}</div>`;
    return html`<div class="md-line">${renderInline(block.line)}</div>`;
  })}</div>`;
}

// Elapsed time for the working ticker: "42s", "3m 5s".
function formatElapsed(ms) {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  return `${minutes}m ${seconds % 60}s`;
}

// A harness's task list (Claude Code's task tools, Codex's plan): what is
// done, what is being worked on, and what is left.
function PlanCard({ items }) {
  const done = items.filter((item) => item.status === 'done').length;
  return html`
    <div class="plan-card">
      <div class="plan-head"><span>Tasks</span><span class="plan-count">${done} of ${items.length}</span></div>
      <ol>
        ${items.map((item) => html`
          <li class=${item.status}>
            <span class="plan-mark" aria-hidden="true">${item.status === 'done' ? html`<${Icon} name="check" size="11" />` : ''}</span>
            <span>${item.text}</span>
          </li>
        `)}
      </ol>
    </div>
  `;
}

// Something the harness reported outside the conversation: a model fallback,
// context compaction, a warning.
function NoticeLine({ level, text }) {
  return html`<div class="notice-line ${level}"><span class="notice-dot" aria-hidden="true"></span><span>${text}</span></div>`;
}

const LIMIT_WINDOWS = { five_hour: '5-hour', seven_day: 'weekly', seven_day_opus: 'weekly Opus', seven_day_sonnet: 'weekly Sonnet' };

function formatTokens(count) {
  return count >= 1e6 ? `${(count / 1e6).toFixed(count % 1e6 ? 1 : 0)}M` : count >= 1e3 ? `${Math.round(count / 1e3)}k` : String(count);
}

// How full the model's context was on the last request, as a small ring.
function ContextMeter({ used, window }) {
  const share = Math.min(used / window, 1);
  const r = 6.5;
  const length = 2 * Math.PI * r;
  return html`
    <span class="context-meter ${share > 0.85 ? 'high' : ''}" data-tip=${`${Math.round(share * 100)}% of context · ${formatTokens(used)} of ${formatTokens(window)} tokens`}>
      <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
        <circle cx="8" cy="8" r=${r} class="track" />
        <circle cx="8" cy="8" r=${r} class="fill" stroke-dasharray=${`${share * length} ${length}`} transform="rotate(-90 8 8)" />
      </svg>
    </span>
  `;
}

// The subscription's usage limit, shown only once it matters.
// The build resolution (web/resolution.mjs): a trigger beside the model that
// opens a slider over its four stops, so the arrow keys step one stop. The
// popover stays open while the slider moves. Read-only on a bench's
// comparison, where the trigger only shows the bench's stop.
function ResolutionPicker({ value, readOnly, onChange }) {
  const { open, setOpen, closing, closePicker, rootRef } = usePopover(110);
  const sliderRef = useRef(null);
  const stop = resolutionStop(value);

  useEffect(() => {
    if (open) sliderRef.current.focus();
  }, [open]);

  if (readOnly) {
    return html`
      <div class="resolution-picker">
        <button
          type="button"
          class="resolution-picker-trigger ${value > 1 ? 'raised' : ''}"
          title=${`Resolution: ${stop.label} (set when the bench was made)`}
          aria-label=${`Resolution: ${stop.label} (set when the bench was made)`}
          disabled
        ><span>${stop.short}</span></button>
      </div>
    `;
  }

  return html`
    <div class="resolution-picker" ref=${rootRef}>
      <button
        type="button"
        class="resolution-picker-trigger ${value > 1 ? 'raised' : ''} ${open ? 'open' : ''}"
        title=${`Resolution: ${stop.label}`}
        aria-label=${`Resolution: ${stop.label}`}
        aria-haspopup="dialog"
        aria-expanded=${open}
        onClick=${() => { if (open) closePicker(); else setOpen(true); }}
      >
        <span>${stop.short}</span>
        <span class="picker-chevron"><${Icon} name="chevron-down" size="12" /></span>
      </button>
      ${open && html`
        <div class="resolution-picker-menu ${closing ? 'closing' : ''}" role="dialog" aria-label="Resolution">
          <div class="resolution-picker-head">
            <label for="composer-resolution">Resolution</label>
            <span>${stop.label}</span>
          </div>
          <input
            ref=${sliderRef}
            id="composer-resolution"
            type="range"
            min="0"
            max=${RESOLUTIONS.length - 1}
            step="1"
            value=${RESOLUTIONS.indexOf(stop)}
            aria-valuetext=${stop.label}
            aria-describedby="composer-resolution-help"
            onInput=${(evt) => onChange(RESOLUTIONS[Number(evt.target.value)].value)}
          />
          <div class="resolution-picker-ticks">
            ${RESOLUTIONS.map((entry) => html`
              <button
                type="button"
                class=${entry === stop ? 'selected' : ''}
                onClick=${() => onChange(entry.value)}
              >${entry.value === 1 ? entry.label : entry.short}</button>
            `)}
          </div>
          <div class="resolution-picker-help">
            ${RESOLUTIONS.map((entry) => html`
              <p
                id=${entry === stop ? 'composer-resolution-help' : undefined}
                class=${entry === stop ? '' : 'hidden'}
                aria-hidden=${entry === stop ? undefined : 'true'}
              >${resolutionHelp(entry)}</p>
            `)}
          </div>
        </div>
      `}
    </div>
  `;
}

// A list row's resolution, above Standard.
function ResolutionTag({ value }) {
  if (value === 1) return null;
  const stop = resolutionStop(value);
  return html`<span class="resolution-tag" title=${`Resolution: ${stop.label}`}>${stop.short}</span>`;
}

function LimitChip({ info }) {
  if (!info || info.status === 'allowed') return null;
  const reset = info.resetsAt ? new Date(info.resetsAt * 1000).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' }) : null;
  const window = LIMIT_WINDOWS[info.rateLimitType] || 'usage';
  const text = info.status === 'rejected' ? `${window} limit reached` : `Near ${window} limit`;
  return html`<span class="limit-chip ${info.status === 'rejected' ? 'rejected' : ''}" data-tip=${reset ? `Resets at ${reset}` : ''}>${text}</span>`;
}

function Ticker({ startedAt, status }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [startedAt]);
  return html`<span class="ticker">
    <span class="ticker-icon-slot" aria-hidden="true">
      <span class="working-dots"><i></i><i></i><i></i></span>
    </span>
    <span>${status ? html`${status} · ` : 'Working for '}<span>${formatElapsed(now - startedAt)}</span></span>
  </span>`;
}

// Tiny JS highlighter: comments, strings, numbers, keywords, calls. One
// regex pass, span nodes, no library.
const CODE_TOKEN = /(\/\/[^\n]*|\/\*[\s\S]*?\*\/)|('(?:[^'\\\n]|\\.)*'?|"(?:[^"\\\n]|\\.)*"?|`(?:[^`\\]|\\.)*`?)|(\b0x[0-9a-fA-F]+\b|\b\d+(?:\.\d+)?\b)|(\b(?:const|let|var|function|return|if|else|for|while|of|in|new|typeof|module|exports|true|false|null|undefined|break|continue|throw|try|catch)\b)|([A-Za-z_$][\w$]*(?=\s*\())/g;

function Code({ text }) {
  return useMemo(() => highlightCode(text), [text]);
}

function highlightCode(text) {
  const nodes = [];
  let last = 0;
  let match;
  CODE_TOKEN.lastIndex = 0;
  while ((match = CODE_TOKEN.exec(text))) {
    if (match.index > last) nodes.push(text.slice(last, match.index));
    const cls = match[1] ? 'c' : match[2] ? 's' : match[3] ? 'n' : match[4] ? 'k' : 'f';
    nodes.push(html`<span class=${`tk-${cls}`}>${match[0]}</span>`);
    last = match.index + match[0].length;
  }
  if (last < text.length) nodes.push(text.slice(last));
  return nodes;
}

// Rebuild displayable chat items from stored OpenAI-format messages.
function itemsFromMessages(messages) {
  const items = [];
  const toolsById = new Map();
  for (const msg of messages) {
    if (msg.role === 'user') {
      if (Array.isArray(msg.content)) {
        const text = msg.content.filter((p) => p.type === 'text').map((p) => p.text).join('\n');
        const images = msg.content.filter((p) => p.type === 'image_url').map((p) => p.image_url.url);
        const attachments = msg.content.filter((p) => p.type === 'attachment').map((p) => p.attachment);
        const selections = msg.content.filter((p) => p.type === 'selection').map((p) => p.selection);
        if (msg.seeBuild) {
          items.push({ kind: 'shots', images });
        } else {
          items.push({ kind: 'user', text, attachments, selections, messageId: msg.id });
        }
      } else {
        items.push({ kind: 'user', text: msg.content, messageId: msg.id });
      }
    } else if (msg.role === 'error') {
      if (!msg.dismissed) items.push({ kind: 'error', text: msg.content, messageId: msg.id });
    } else if (msg.role === 'assistant') {
      const final = !(msg.tool_calls || []).length;
      // Usage drives the composer's context meter and limit chip; it never renders inline.
      if (msg.telemetry) items.push({ kind: 'usage', telemetry: msg.telemetry, rateLimit: msg.rateLimit || null });
      for (const notice of msg.notices || []) items.push({ kind: 'notice', level: notice.level, text: notice.text });
      if (msg.plan) items.push({ kind: 'plan', items: msg.plan });
      if (msg.reasoning) items.push({ kind: 'reasoning', text: msg.reasoning });
      if (msg.content) items.push({ kind: 'assistant', text: msg.content, final });
      for (const call of msg.tool_calls || []) {
        const name = call.function.name;
        let summary = 'build';
        let program;
        try {
          const args = JSON.parse(call.function.arguments);
          program = typeof args.program === 'string' ? args.program
            : typeof args.command === 'string' ? args.command : name === 'test_build' ? JSON.stringify(args, null, 2) : undefined;
          summary = args.summary
            || (name === 'see_build' ? `Look at build (${(args.shots || []).map((shot) => shot?.view || (shot?.eye ? 'camera' : 'iso_se')).join(', ') || 'default'})`
              : name === 'bash' ? `$ ${(args.command || '').split('\n')[0].slice(0, 80)}`
                : name === 'test_build' ? `Test build in Minecraft${Array.isArray(args.tests) && args.tests.length > 1 ? ` (${args.tests.length} tests)` : ''}`
                : name === 'view_image' ? `Look at /work/${args.path || ''}`
                  : name);
        } catch { /* keep default */ }
        const item = { kind: 'tool', callId: call.id, tool: name, summary, program, status: 'ok', detail: '' };
        items.push(item);
        toolsById.set(call.id, item);
      }
    } else if (msg.role === 'tool') {
      const item = toolsById.get(msg.tool_call_id);
      if (item) {
        try {
          const result = JSON.parse(msg.content);
          item.status = result.cancelled ? 'cancelled' : result.ok ? 'ok' : 'failed';
          if (result.ok && result.diff) item.diff = result.diff;
          if (typeof result.output === 'string') item.output = result.output;
          if (item.tool === 'test_build') item.output = JSON.stringify(result, null, 2);
          item.detail = result.ok ? '' : (result.error || (item.output ? '' : 'failed'));
        } catch {
          // bash transcripts are stored as plain terminal text, not JSON.
          if (item.tool === 'bash') item.output = msg.content;
        }
      }
    }
  }
  return items;
}

function branchNavigation(messageTree) {
  const siblingsByParent = new Map();
  for (const message of messageTree.messages) {
    if (!message.branchable) continue;
    const siblings = siblingsByParent.get(message.parentId) || [];
    siblings.push(message);
    siblingsByParent.set(message.parentId, siblings);
  }

  const navigation = new Map();
  for (const siblings of siblingsByParent.values()) {
    if (siblings.length < 2) continue;
    siblings.sort((left, right) => left.position - right.position);
    siblings.forEach((message, index) => navigation.set(message.id, {
      index,
      count: siblings.length,
      previousId: siblings[index - 1]?.id || null,
      nextId: siblings[index + 1]?.id || null,
    }));
  }
  return navigation;
}

// Git-style stat: a changed block counts as one addition and one removal.
function DiffStat({ diff }) {
  const plus = (diff.added || 0) + (diff.changed || 0);
  const minus = (diff.removed || 0) + (diff.changed || 0);
  if (!plus && !minus) return null;
  return html`<span class="diffstat">
    ${plus > 0 && html`<em class="p">+${plus}</em>`}
    ${minus > 0 && html`<em class="m">-${minus}</em>`}
  </span>`;
}

// Vision APIs cap image size (Anthropic: 5 MB, ~1568px useful resolution), so
// supported bitmap images are downscaled and re-encoded as JPEG. Preserve
// other files (including SVG source) for the agent's workspace tools.
const MAX_IMAGE_EDGE = 1568;
const MAX_ATTACHMENT_BYTES = 100 * 1024 * 1024;
const MAX_ATTACHMENTS_BYTES = 200 * 1024 * 1024;

function checkAttachmentLimits(current, added) {
  if (current.length + added.length > 8) throw new Error('A message supports at most 8 attachments');
  const totalBytes = [...current, ...added].reduce((sum, file) => sum + file.size, 0);
  if (totalBytes > MAX_ATTACHMENTS_BYTES) throw new Error('Attachments exceed 200 MB total');
}

async function readImageAsDataUrl(file) {
  const bitmap = await createImageBitmap(file);
  const canvas = document.createElement('canvas');
  paintUpload(canvas, bitmap, MAX_IMAGE_EDGE);
  bitmap.close();
  return canvas.toDataURL('image/jpeg', 0.88);
}

async function readUpload(file) {
  if (file.size === 0) throw new Error(`"${file.name}" is empty`);
  if (file.size > MAX_ATTACHMENT_BYTES) throw new Error(`"${file.name}" exceeds 100 MB`);
  const image = ['image/jpeg', 'image/png', 'image/gif', 'image/webp'].includes(file.type);
  return {
    name: file.name,
    type: image ? 'image/jpeg' : (file.type || 'application/octet-stream'),
    size: file.size,
    data: image
      ? await readImageAsDataUrl(file)
      : await readBlobAsDataUrl(file.type ? file : new Blob([file], { type: 'application/octet-stream' })),
  };
}

// Quiet, consistent line icons. The grass block carries the pixel identity;
// controls use a modern 1.75px stroke so the interface stays legible at 12–16px.
function Icon({ name, size }) {
  let glyph;
  if (name === 'send') glyph = html`<path d="m5 12 7-7 7 7" /><path d="M12 19V5" />`;
  else if (name === 'eye') glyph = html`<path d="M2.06 12.35a1 1 0 0 1 0-.7 10.75 10.75 0 0 1 19.88 0 1 1 0 0 1 0 .7 10.75 10.75 0 0 1-19.88 0" /><circle cx="12" cy="12" r="3" />`;
  else if (name === 'eye-off') glyph = html`<path d="M10.73 5.08A10.43 10.43 0 0 1 12 5c7 0 10 7 10 7a13.16 13.16 0 0 1-1.67 2.68" /><path d="M6.61 6.61A13.53 13.53 0 0 0 2 12s3 7 10 7a9.74 9.74 0 0 0 5.39-1.61" /><path d="M14.08 14.08a3 3 0 1 1-4.16-4.16" /><path d="m2 2 20 20" />`;
  else if (name === 'trophy') glyph = html`<path d="M6 9H4.5a2.5 2.5 0 0 1 0-5H6" /><path d="M18 9h1.5a2.5 2.5 0 0 0 0-5H18" /><path d="M4 22h16" /><path d="M10 14.66V17c0 .55-.47.98-.97 1.21C7.85 18.75 7 20.24 7 22" /><path d="M14 14.66V17c0 .55.47.98.97 1.21C16.15 18.75 17 20.24 17 22" /><path d="M18 2H6v7a6 6 0 0 0 12 0V2Z" />`;
  else if (name === 'play') glyph = html`<path d="M7 4.5v15l12-7.5z" fill="currentColor" stroke="none" />`;
  else if (name === 'pause') glyph = html`<rect x="6.5" y="5" width="3.5" height="14" rx="1" fill="currentColor" stroke="none" /><rect x="14" y="5" width="3.5" height="14" rx="1" fill="currentColor" stroke="none" />`;
  else if (name === 'step') glyph = html`<path d="M5 5v14l10-7z" fill="currentColor" stroke="none" /><path d="M18.5 5v14" />`;
  else if (name === 'restart') glyph = html`<path d="M3 12a9 9 0 1 0 3-6.7L3 8" /><path d="M3 3v5h5" />`;
  else if (name === 'stop') glyph = html`<rect x="7" y="7" width="10" height="10" rx="1.5" fill="currentColor" stroke="none" />`;
  else if (name === 'settings') glyph = html`<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" /><circle cx="12" cy="12" r="3" />`;
  else if (name === 'copy') glyph = html`<rect x="8" y="8" width="12" height="12" rx="2" /><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" />`;
  else if (name === 'check') glyph = html`<path d="m5 12 4 4L19 6" />`;
  else if (name === 'compose') glyph = html`<path d="M12 3.5H6.5a3 3 0 0 0-3 3v11a3 3 0 0 0 3 3h11a3 3 0 0 0 3-3V12" /><path d="M18.4 3.1a1.9 1.9 0 0 1 2.7 2.7l-7.6 7.6a2 2 0 0 1-.9.5l-2.6.7.7-2.6a2 2 0 0 1 .5-.9z" />`;
  else if (name === 'panel') glyph = html`<rect x="3" y="3" width="18" height="18" rx="2" /><path d="M9 3v18" />`;
  else if (name === 'archive') glyph = html`<path d="M4 7h16" /><path d="M5 7v12h14V7" /><path d="M3 3h18v4H3z" /><path d="M10 11h4" />`;
  else if (name === 'star') glyph = html`<path d="m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2-5.6-2.9-5.6 2.9 1.1-6.2L3 9.6l6.2-.9z" />`;
  else if (name === 'sun') glyph = html`<circle cx="12" cy="12" r="4" /><path d="M12 2v2M12 20v2M4.93 4.93l1.42 1.42M17.66 17.66l1.41 1.41M2 12h2M20 12h2M4.93 19.07l1.42-1.42M17.66 6.34l1.41-1.41" />`;
  else if (name === 'moon') glyph = html`<path d="M21 12.8A9 9 0 1 1 11.2 3 7 7 0 0 0 21 12.8Z" />`;
  else if (name === 'search') glyph = html`<circle cx="11" cy="11" r="7" /><path d="m20 20-4-4" />`;
  else if (name === 'x') glyph = html`<path d="m6 6 12 12M18 6 6 18" />`;
  else if (name === 'equal') glyph = html`<path d="M5 9h14" /><path d="M5 15h14" />`;
  else if (name === 'trash') glyph = html`<path d="M3 6h18" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" /><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" /><path d="M10 11v6" /><path d="M14 11v6" />`;
  else if (name === 'share') glyph = html`<circle cx="18" cy="5" r="2.5" /><circle cx="6" cy="12" r="2.5" /><circle cx="18" cy="19" r="2.5" /><path d="m8.2 10.8 7.6-4.4M8.2 13.2l7.6 4.4" />`;
  else if (name === 'build') glyph = html`<path d="M4 8.5 12 4l8 4.5-8 4.5z" /><path d="m4 8.5 8 4.5 8-4.5M4 8.5V16l8 4 8-4V8.5M12 13v7" />`;
  else if (name === 'plus') glyph = html`<path d="M12 5v14M5 12h14" />`;
  else if (name === 'grip') glyph = html`${[4.5, 9.5, 14.5, 19.5].flatMap((y) => [9, 15].map((x) => html`<circle cx=${x} cy=${y} r="1.6" fill="currentColor" stroke="none" />`))}`;
  else if (name === 'back') glyph = html`<path d="m14 6-6 6 6 6" />`;
  else if (name === 'forward') glyph = html`<path d="m10 6 6 6-6 6" />`;
  else if (name === 'chevron-right') glyph = html`<path d="m9 6 6 6-6 6" />`;
  else if (name === 'chevron-down') glyph = html`<path d="m6 9 6 6 6-6" />`;
  else if (name === 'terminal') glyph = html`<path d="m5 7 4.5 5L5 17" /><path d="M12 17h7" />`;
  else if (name === 'eye') glyph = html`<path d="M2.5 12s3.5-6 9.5-6 9.5 6 9.5 6-3.5 6-9.5 6-9.5-6-9.5-6Z" /><circle cx="12" cy="12" r="2.5" />`;
  else if (name === 'hammer') glyph = html`<path d="m14 5 5 5" /><path d="m12 7 4-4 5 5-4 4" /><path d="M14.5 9.5 6 18l-2 2" />`;
  else if (name === 'refresh') glyph = html`<path d="M20 11a8 8 0 0 0-14.6-4.5L4 8" /><path d="M4 3v5h5" /><path d="M4 13a8 8 0 0 0 14.6 4.5L20 16" /><path d="M20 21v-5h-5" />`;
  else if (name === 'up') glyph = html`<path d="M12 19V5M6 11l6-6 6 6" />`;
  else if (name === 'down') glyph = html`<path d="M12 5v14M6 13l6 6 6-6" />`;
  else if (name === 'prompts') glyph = html`<path d="M12 7v14" /><path d="M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z" /><path d="M16 8h2M16 12h2M6 8h2M6 12h2" />`;
  else if (name === 'plug') glyph = html`<path d="M9 2v5M15 2v5" /><path d="M6 7h12v4a6 6 0 0 1-12 0z" /><path d="M12 17v5" />`;
  else if (name === 'wrench') glyph = html`<path d="M14.5 6.5a4 4 0 0 0-5-5l2.2 2.2-2.8 2.8-2.2-2.2a4 4 0 0 0 5 5L20 17.6 17.6 20l-8.3-8.3" />`;
  else if (name === 'file') glyph = html`<path d="M6 3h8l4 4v14H6z" /><path d="M14 3v5h5M9 13h6M9 17h6" />`;
  else if (name === 'bench') glyph = html`<path d="M3.5 20.5h17" /><path d="M5 20.5V12h4.5v8.5M9.5 20.5V6h5v14.5M14.5 20.5V14H19v6.5" />`;
  else if (name === 'select') glyph = html`<path d="M4 8V5a1 1 0 0 1 1-1h3M16 4h3a1 1 0 0 1 1 1v3M20 16v3a1 1 0 0 1-1 1h-3M8 20H5a1 1 0 0 1-1-1v-3" /><path d="M11 4h2M11 20h2M4 11v2M20 11v2" />`;
  else if (name === 'swap') glyph = html`<path d="M5 8h13.5M15 4.5 18.5 8 15 11.5" /><path d="M19 16H5.5M9 12.5 5.5 16 9 19.5" />`;
  else if (name === 'edit') glyph = html`<path d="M4 20h4L19 9l-4-4L4 16z" /><path d="m13.5 6.5 4 4" />`;
  else if (name === 'open') glyph = html`<path d="M14 4h6v6M20 4l-8.5 8.5" /><path d="M18.5 13.5V18a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V7.5a2 2 0 0 1 2-2h4.5" />`;
  else if (name === 'filter') glyph = html`<path d="M4 7h16M7 12h10M10 17h4" />`;
  else if (name === 'fork') glyph = html`<circle cx="6" cy="5.5" r="2" /><circle cx="18" cy="5.5" r="2" /><circle cx="12" cy="18.5" r="2" /><path d="M6 7.5v1a3 3 0 0 0 3 3h6a3 3 0 0 0 3-3v-1M12 11.5v5" />`;
  else if (name === 'sparkles') glyph = html`<path d="m12 3 1.2 3.3L16.5 7.5l-3.3 1.2L12 12l-1.2-3.3-3.3-1.2 3.3-1.2z" /><path d="m18 13 .8 2.2L21 16l-2.2.8L18 19l-.8-2.2L15 16l2.2-.8zM6 14l.6 1.4L8 16l-1.4.6L6 18l-.6-1.4L4 16l1.4-.6z" />`;
  return html`
    <svg
      width=${size} height=${size} viewBox="0 0 24 24"
      fill="none" stroke="currentColor" stroke-width="1.75"
      stroke-linecap="round" stroke-linejoin="round"
      aria-hidden="true"
    >${glyph}</svg>
  `;
}

const favoriteKey = (provider, model) => `${provider}::${model}`;

// Real vendor logos (monochrome SVGs in /icons), painted with currentColor
// via a CSS mask so they follow the theme.
// Each vendor's own brand color; monochrome brands (OpenAI, Codex, Cursor,
// xAI, Moonshot) follow the text color so they read in both themes.
const BRAND = {
  claude: '#d97757',
  openrouter: '#7072f5',
  groq: '#f55036',
  qwen: 'linear-gradient(135deg, #7b6cf6, #5b4fe0)',
  gemini: 'linear-gradient(135deg, #4285f4 15%, #9b72cb 55%, #d96570 90%)',
  meta: 'linear-gradient(135deg, #0081fb, #0064e1)',
  mistral: 'linear-gradient(180deg, #ffd800, #ff8205 50%, #e10500)',
  deepseek: '#4d6bfe',
};

function Logo({ name: given, size }) {
  const name = logoName(given);
  return html`<span
    class="vlogo"
    style=${{ width: `${size}px`, height: `${size}px`, '--icon': `url(/icons/${name}.svg)`, ...(BRAND[name] ? { background: BRAND[name] } : {}) }}
    aria-hidden="true"
  ></span>`;
}

// A provider's models in its saved order; models not in the order keep
// their catalog position after the ordered ones.
function orderModels(models, order) {
  if (!order?.length) return models;
  const rank = new Map(order.map((id, index) => [id, index]));
  return models.map((model, index) => ({ model, index })).sort((a, b) => (rank.get(a.model.id) ?? order.length + a.index) - (rank.get(b.model.id) ?? order.length + b.index)).map((entry) => entry.model);
}

// OpenRouter names read "Vendor: Model"; the logo already says the vendor.
const displayName = (model) => model.name.replace(/^[^:]+:\s*/, '');
const modelLabel = (id) => id.split('/').pop();

// The unqueried OpenRouter list would be 400+ entries of noise. Default view:
// your favorites, then recent releases from the big four labs. Everything
// else is a search away.
const CURATED_VENDORS = ['openai/', 'anthropic/', 'x-ai/', 'google/'];
const CURATED_LIMIT = 30;

// The app's mark: a wire cube with a lit block inside (web/icons/block.svg is
// the same drawing for the browser tab). The wire takes the text colour, so
// it follows the theme.
function AppMark({ size }) {
  return html`
    <svg class="block-icon app-mark" width=${size} height=${size} viewBox="0 0 64 64" fill="none" aria-hidden="true">
      <defs>
        <filter id="app-mark-glow" x="-60%" y="-60%" width="220%" height="220%">
          <feGaussianBlur stdDeviation="2.2" result="b" />
          <feMerge><feMergeNode in="b" /><feMergeNode in="SourceGraphic" /></feMerge>
        </filter>
      </defs>
      <path d="M32 8V32M32 32 52.78 44M32 32 11.22 44" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" opacity=".32" />
      <g filter="url(#app-mark-glow)" stroke="#0b3d1c" stroke-width=".7" stroke-linejoin="round">
        <path d="M22.47 26.5 32 32V43L22.47 37.5Z" fill="#3ecf6b" />
        <path d="M41.53 26.5 32 32V43L41.53 37.5Z" fill="#229a4b" />
        <path d="M32 21 41.53 26.5 32 32 22.47 26.5Z" fill="#7df0a0" />
      </g>
      <path d="M32 8 52.78 20V44L32 56 11.22 44V20ZM52.78 20 32 32 11.22 20M32 32V56" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" />
    </svg>
  `;
}

function storedJson(key, fallback) {
  try {
    const value = JSON.parse(localStorage.getItem(key));
    return value ?? fallback;
  } catch {
    return fallback;
  }
}

// The resolution stop this browser saved under `key` (the last one chosen
// for a new chat, or for a new bench), or Standard.
function storedResolution(key) {
  const value = storedJson(key, 1);
  return RESOLUTIONS.some((stop) => stop.value === value) ? value : 1;
}

// Settings: providers and harnesses listed on the left with their version,
// state and an on/off switch; the selection's account, runtime and models on
// the right.
function ago(iso) {
  const seconds = Math.max(0, Math.round((Date.now() - Date.parse(iso)) / 1000));
  if (seconds < 10) return 'just now';
  if (seconds < 60) return `${seconds}s ago`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  return `${Math.floor(seconds / 3600)}h ago`;
}

function Toggle({ on, onChange, label }) {
  return html`<button
    type="button"
    role="switch"
    aria-checked=${on}
    aria-label=${label}
    class="toggle ${on ? 'on' : ''}"
    onClick=${(evt) => { evt.stopPropagation(); onChange(!on); }}
  ><span></span></button>`;
}

function CopyButton({ text, label }) {
  const [copied, setCopied] = useState(false);
  const copy = () => navigator.clipboard.writeText(text)
    .then(() => { setCopied(true); setTimeout(() => setCopied(false), 1200); })
    .catch(() => {});
  return html`
    <button type="button" class="copy-btn" title=${copied ? 'Copied' : label} aria-label=${label} onClick=${copy}>
      <${Icon} name=${copied ? 'check' : 'copy'} size="13" />
    </button>
  `;
}

function SettingsSection({ title, action, children }) {
  return html`
    <section class="s-section">
      <div class="s-section-head"><h4>${title}</h4>${action}</div>
      <div class="s-card">${children}</div>
    </section>
  `;
}

// A row is found by its title (data-setting) when search jumps to it.
function SettingsRow({ title, help, tip, children }) {
  return html`
    <div class="s-row" data-setting=${typeof title === 'string' ? title : undefined}>
      <div class="s-row-text">
        <div class="s-row-title">${title}</div>
        ${help && html`<div class="s-row-help">${help}</div>`}
      </div>
      ${children && html`<div class="s-row-control" data-tip=${tip}>${children}</div>`}
    </div>
  `;
}

// Clears the leaderboard by deleting every vote, after asking once more.
function DangerZone({ onClearVotes, onError }) {
  // null while counting, then the count, or the error that kept it unknown.
  const [votes, setVotes] = useState(null);
  const [confirming, setConfirming] = useState(false);
  const [clearing, setClearing] = useState(false); // the delete is on its way
  useEffect(() => {
    api('/api/leaderboard').then((data) => setVotes(data.votes), (error) => { setVotes(error); onError(error); });
  }, []);
  const clear = async () => {
    setClearing(true);
    try {
      await onClearVotes();
      setConfirming(false);
      setVotes(0);
    } finally {
      setClearing(false);
    }
  };
  const question = votes instanceof Error ? 'Delete every vote?' : votes === 1 ? 'Delete the only vote?' : `Delete all ${votes} votes?`;
  return html`
    <${SettingsSection} title="Danger zone">
      <${SettingsRow} title="Clear leaderboard" help="Deletes every vote, blind or not, so the leaderboard starts over. Benches and their builds stay.">
        ${confirming ? html`
          <div class="danger-confirm">
            <span>${question}</span>
            <button type="button" class="btn" disabled=${clearing} onClick=${() => setConfirming(false)}>Cancel</button>
            <button type="button" class="btn danger" disabled=${clearing} onClick=${() => clear().catch(onError)}>Delete votes</button>
          </div>
        ` : html`
          <button type="button" class="btn danger" disabled=${votes === null || votes === 0} onClick=${() => setConfirming(true)}>
            ${votes === 0 ? 'No votes' : 'Clear leaderboard'}
          </button>
        `}
      <//>
    <//>
  `;
}

function SettingsNavItem({ id, connection, active, onSelect, onToggle }) {
  return html`
    <div
      role="button"
      tabindex="0"
      class="settings-nav-item provider ${active ? 'active' : ''} ${connection.enabled ? '' : 'off'}"
      onClick=${() => onSelect(id)}
      onKeyDown=${(evt) => { if (evt.key === 'Enter' || evt.key === ' ') { evt.preventDefault(); onSelect(id); } }}
    >
      <${Logo} name=${id} size="18" />
      <span class="nav-text">
        <span class="name">${connection.label}${connection.version && html`<span class="ver">v${connection.version}</span>`}</span>
        <span class="sub ${connection.connected && connection.enabled ? '' : 'muted'}">${connection.summary}</span>
      </span>
      <${Toggle} on=${connection.enabled} label=${`Offer ${connection.label} in the model picker`} onChange=${(on) => onToggle(id, on)} />
    </div>
  `;
}

const MODEL_ROWS = 150;

function ModelsSection({ id, connection, favorites, onToggleFavorite, onChange, checkedAt }) {
  const [query, setQuery] = useState('');
  // The model dragged by its grip: its row (from), the row it would land on
  // (to), how far it has moved (dy) and the row pitch (height), in pixels.
  const [drag, setDrag] = useState(null);
  // The render that applies a drop moves rows without animating them.
  const [settling, setSettling] = useState(false);
  const listRef = useRef(null);
  // A drag's listeners, let go of if Settings closes mid-drag.
  const dragEndRef = useRef(null);
  useEffect(() => () => dragEndRef.current?.(), []);
  useEffect(() => { setQuery(''); }, [id]);
  const [entry] = useShared([connection.connected ? modelsUrl(id) : null]);
  // "Check again" re-reads the catalog; the current list stays up meanwhile.
  const checkedRef = useRef(checkedAt);
  useEffect(() => {
    if (checkedRef.current === checkedAt) return;
    checkedRef.current = checkedAt;
    if (connection.connected) load(modelsUrl(id), { fresh: true }).catch(() => {});
  }, [checkedAt]);
  const catalog = { models: entry?.value?.models || (entry?.error ? [] : null), error: entry?.error ? String(entry.error.message || entry.error) : '' };

  if (!connection.connected) {
    return html`<${SettingsSection} title="Models"><div class="s-empty">${connection.envKey ? 'Add a key' : 'Sign in'} to list ${connection.label}'s models.</div><//>`;
  }
  const hidden = new Set(connection.hiddenModels);
  const ordered = orderModels(catalog.models || [], connection.modelOrder);
  const needle = query.trim().toLowerCase();
  const matches = ordered.filter((model) => !needle || model.name.toLowerCase().includes(needle) || model.id.toLowerCase().includes(needle));
  const allMatchesHidden = matches.length > 0 && matches.every((model) => hidden.has(model.id));
  const setHidden = (ids, hide) => {
    const next = new Set(hidden);
    for (const modelId of ids) { if (hide) next.add(modelId); else next.delete(modelId); }
    onChange('hiddenModels', [...next]);
  };
  const shown = matches.slice(0, MODEL_ROWS);
  // Puts a model at row `to` of the rows shown (the rest of the order, filtered
  // out or not, keeps its place).
  const place = (modelId, to) => {
    const others = shown.map((model) => model.id).filter((other) => other !== modelId);
    const ids = ordered.map((model) => model.id).filter((other) => other !== modelId);
    const at = to < others.length ? ids.indexOf(others[to]) : ids.indexOf(others[others.length - 1]) + 1;
    ids.splice(at, 0, modelId);
    onChange('modelOrder', ids);
  };
  // Dragging a model by its grip: its row follows the pointer, the rows it
  // passes slide aside, and letting go saves the order. The rows keep their
  // order in the page until then.
  const startDrag = (evt, modelId) => {
    if (evt.button !== 0) return;
    evt.preventDefault();
    // The grip holds the pointer until it is let go, wherever it moves.
    const grip = evt.currentTarget;
    grip.setPointerCapture(evt.pointerId);
    const rows = listRef.current.querySelectorAll('.s-model');
    const height = rows.length > 1 ? rows[1].offsetTop - rows[0].offsetTop : rows[0].offsetHeight;
    const from = shown.findIndex((model) => model.id === modelId);
    const startY = evt.clientY;
    const at = (y) => {
      const dy = Math.max(-from * height, Math.min((shown.length - 1 - from) * height, y - startY));
      return { id: modelId, from, height, dy, to: from + Math.round(dy / height) };
    };
    setDrag(at(startY));
    const move = (event) => setDrag(at(event.clientY));
    const detach = () => {
      grip.removeEventListener('pointermove', move);
      grip.removeEventListener('pointerup', end);
      grip.removeEventListener('pointercancel', end);
      dragEndRef.current = null;
    };
    const end = (event) => {
      detach();
      const { to } = at(event.clientY);
      setSettling(true);
      setDrag(null);
      if (to !== from) place(modelId, to);
      requestAnimationFrame(() => requestAnimationFrame(() => setSettling(false)));
    };
    grip.addEventListener('pointermove', move);
    grip.addEventListener('pointerup', end);
    grip.addEventListener('pointercancel', end);
    dragEndRef.current = detach;
  };
  // Where a row sits while a model is dragged: the dragged row under the
  // pointer, the rows between its row and the one it would land on moved a
  // row towards where it came from.
  const offset = (row) => {
    if (!drag) return 0;
    if (row === drag.from) return drag.dy;
    if (drag.from < drag.to && row > drag.from && row <= drag.to) return -drag.height;
    if (drag.from > drag.to && row >= drag.to && row < drag.from) return drag.height;
    return 0;
  };
  return html`
    <${SettingsSection} title="Models">
      <div class="s-models-intro">Hidden models stay out of the model picker. Order and favorites apply wherever this provider is listed.</div>
      <div class="s-models-toolbar">
        <input id=${`model-filter-${id}`} class="s-filter" placeholder="Filter models" value=${query} onInput=${(evt) => setQuery(evt.target.value)} />
        <button type="button" class="s-link" disabled=${!matches.length} onClick=${() => setHidden(matches.map((model) => model.id), !allMatchesHidden)}>
          ${allMatchesHidden ? 'Show' : 'Hide'} ${needle ? 'matches' : 'all'}
        </button>
        <span class="s-count">${catalog.models ? `${ordered.filter((model) => !hidden.has(model.id)).length} of ${ordered.length} shown` : ''}</span>
      </div>
      ${!catalog.models && html`<div class="s-empty">Loading models…</div>`}
      ${catalog.error && html`<div class="s-empty error">${catalog.error}</div>`}
      <div class="s-models ${drag ? 'dragging' : ''} ${settling ? 'settling' : ''}" ref=${listRef}>
        ${shown.map((model, row) => {
          const key = favoriteKey(id, model.id);
          return html`
            <div
              class="s-model ${hidden.has(model.id) ? 'hidden' : ''} ${drag?.id === model.id ? 'dragged' : ''}"
              key=${model.id}
              style=${drag ? `transform: translateY(${offset(row)}px)` : ''}
            >
              <button
                type="button"
                class="s-grip"
                title="Drag to reorder"
                aria-label=${`Reorder ${model.name}: drag, or use the up and down arrow keys`}
                onPointerDown=${(evt) => startDrag(evt, model.id)}
                onKeyDown=${(evt) => {
                  const to = row + (evt.key === 'ArrowUp' ? -1 : evt.key === 'ArrowDown' ? 1 : 0);
                  if (to === row || to < 0 || to >= shown.length) return;
                  evt.preventDefault();
                  place(model.id, to);
                }}
              ><${Icon} name="grip" size="14" /></button>
              <button type="button" class="s-star ${favorites.includes(key) ? 'on' : ''}" title=${favorites.includes(key) ? 'Remove favorite' : 'Add favorite'} onClick=${() => onToggleFavorite(key)}>
                <${Icon} name="star" size="14" />
              </button>
              <span class="s-model-name">${displayName(model)}</span>
              <span class="s-model-id">${model.id}</span>
              <span class="s-model-caps">${model.fast && html`<span class="s-tag" title=${model.fastNote || 'Fast mode available'}>Fast</span>`}</span>
              <${Toggle} on=${!hidden.has(model.id)} label=${`Show ${model.name} in the model picker`} onChange=${(on) => setHidden([model.id], !on)} />
            </div>
          `;
        })}
        ${matches.length > MODEL_ROWS && html`<div class="s-empty">${matches.length - MODEL_ROWS} more. Filter to find them.</div>`}
        ${catalog.models && !matches.length && html`<div class="s-empty">No models match.</div>`}
      </div>
    <//>
  `;
}

const RUNTIME_NOTES = {
  'claude-code': [
    ['Tools', 'Built-in tools removed; mcbench\'s tools over MCP are the only ones.'],
    ['Sessions', 'One Claude Code session per chat, resumed on later turns.'],
  ],
  'codex-cli': [
    ['Tools', 'Shell, file, web and agent features off; mcbench\'s tools over MCP.'],
    ['Isolation', 'Linux: a container with no network but the model API. macOS: on the host, read-only sandbox.'],
    ['Sessions', 'One Codex thread per chat, resumed on later turns.'],
  ],
  cursor: [
    ['Tools', 'mcbench\'s tools are the agent\'s only custom tools; no rules, skills or MCP servers load.'],
    ['Sessions', 'A fresh agent each turn, given the conversation so far.'],
  ],
  codex: [['Requests', 'Sent with your Codex login from inside the mcbench server; nothing listens on a port.']],
};

// The sign-in methods' names on their buttons (server/provider-login.js).
const SIGN_IN_METHODS = { claudeai: 'Claude subscription', console: 'Anthropic Console' };
const SIGN_IN_HELP = {
  'claude-code': 'Sign in with your Claude subscription or with an Anthropic Console account. The sign-in page opens in a new tab.',
  codex: 'Sign in with your ChatGPT account. The sign-in page opens in a new tab and asks for a one-time code shown here.',
  'codex-cli': 'Sign in with your ChatGPT account. The sign-in page opens in a new tab and asks for a one-time code shown here.',
  cursor: 'Sign in with your Cursor account instead of a key. The sign-in page opens in a new tab.',
};
const SIGN_IN_NEXT = {
  'claude-code': 'Sign in there. The page then shows a code: paste it below.',
  codex: 'Sign in there and enter the one-time code below. This pane updates when you are done.',
  'codex-cli': 'Sign in there and enter the one-time code below. This pane updates when you are done.',
  cursor: 'Sign in there. This pane updates when you are done.',
};

// Signing in from Settings: the server runs the provider's own login
// (server/provider-login.js), and this shows what the login printed, the
// address to open and the one-time code, and passes on the code the person
// pastes when the CLI asks for one. `signIn` is the latest sign-in as the
// server reports it, or null; it is read when the pane opens (ChatGPT and
// Codex CLI share one), again each second while it runs, and one that is
// done reads Settings again, which then show the account.
//
// A login whose CLI is not on this machine installs it first, from the same
// button (server/cli-install.js): `install` is the latest install as the
// server reports it, read each second while it runs, and `after` the
// sign-in that starts once Settings, read again, find the CLI installed
// (`{ method }`), or null. `startWith`, when set, is a button pressed from
// Connect an AI: `{ method }`, started once when the pane opens.
function SignInRows({ id, connection, startWith, onStarted, onInstalled, onSignedIn, onError }) {
  const { methods, program, installed } = connection.signIn;
  const [signIn, setSignIn] = useState(connection.signIn.current);
  const [code, setCode] = useState('');
  const [install, setInstall] = useState(null);
  const [after, setAfter] = useState(null);
  useEffect(() => {
    setSignIn(connection.signIn.current);
    setCode('');
    api(`/api/provider-logins/${id}`).then(setSignIn).catch(onError);
    if (program) api(`/api/cli-installs/${program.name}`).then(setInstall).catch(onError);
  }, [id]);
  const running = signIn?.state === 'starting' || signIn?.state === 'waiting';
  useEffect(() => {
    if (!running) return undefined;
    let live = true;
    const timer = setInterval(() => {
      api(`/api/provider-logins/${id}`).then((next) => {
        if (!live) return;
        setSignIn(next);
        if (next?.state === 'done') onSignedIn();
      }).catch(onError);
    }, 1000);
    return () => { live = false; clearInterval(timer); };
  }, [id, running]);
  const installing = install?.state === 'running';
  useEffect(() => {
    if (!installing) return undefined;
    let live = true;
    const timer = setInterval(() => {
      api(`/api/cli-installs/${program.name}`).then((next) => {
        if (!live) return;
        setInstall(next);
        if (next.state === 'done') onInstalled();
        if (next.state === 'failed') setAfter(null);
      }).catch(onError);
    }, 1000);
    return () => { live = false; clearInterval(timer); };
  }, [id, installing]);
  const post = (path, body) => api(path, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });
  const start = (method) => post('/api/provider-logins', { provider: id, method }).then(setSignIn).catch(onError);
  useEffect(() => {
    if (!installed || !after) return;
    setAfter(null);
    start(after.method);
  }, [installed, after]);
  // The one button: the sign-in, after installing the CLI when it is missing.
  const begin = (method) => {
    if (installed) return start(method);
    setAfter({ method });
    return post('/api/cli-installs', { program: program.name }).then(setInstall).catch((error) => { setAfter(null); onError(error); });
  };
  useEffect(() => {
    if (!startWith) return;
    onStarted();
    begin(startWith.method);
  }, [startWith]);
  const cancel = () => api(`/api/provider-logins/${id}`, { method: 'DELETE' }).then(setSignIn).catch(onError);
  const send = (evt) => {
    evt.preventDefault();
    post(`/api/provider-logins/${id}/input`, { text: code }).then((next) => { setCode(''); setSignIn(next); }).catch(onError);
  };

  if (installing || (after && install?.state === 'done')) {
    return html`
      <${SettingsRow}
        title=${`Installing ${program.title}`}
        help=${`${install.state === 'done' ? 'Installed.' : install.progress ? `Downloading, ${megabytes(install.progress)}.` : 'Starting the download.'}${after ? ' The sign-in starts when it is done.' : ''}`}
      >
        <span class="working-dots" aria-hidden="true"><i></i><i></i><i></i></span>
      <//>
    `;
  }
  if (signIn?.state === 'starting') {
    return html`
      <${SettingsRow} title="Starting the sign-in" help="Waiting for the sign-in address.">
        <button type="button" class="btn" onClick=${cancel}>Cancel</button>
      <//>
    `;
  }
  if (signIn?.state === 'waiting') {
    return html`
      <div class="s-row s-signin">
        <div class="s-row-text">
          <div class="s-row-title">Open the sign-in page</div>
          <div class="s-row-help">${SIGN_IN_NEXT[id]}</div>
          <div class="s-signin-link">
            <a href=${signIn.url} target="_blank" rel="noreferrer">${signIn.url}</a>
            <${CopyButton} text=${signIn.url} label="Copy address" />
          </div>
        </div>
        <div class="s-row-control">
          <button type="button" class="btn" onClick=${cancel}>Cancel</button>
        </div>
      </div>
      ${signIn.userCode && html`
        <${SettingsRow} title="One-time code" help="Enter this code on the sign-in page.">
          <span class="cmd-chip s-signin-code"><code>${signIn.userCode}</code><${CopyButton} text=${signIn.userCode} label="Copy code" /></span>
        <//>
      `}
      ${signIn.wantsInput && html`
        <${SettingsRow} title="Code from the sign-in page" help=${signIn.message || 'Paste the code the page shows after you sign in.'}>
          <form class="s-signin-form" onSubmit=${send}>
            <input
              id=${`sign-in-code-${id}`}
              class="s-input mono"
              autocomplete="off"
              spellcheck="false"
              placeholder="Paste the code"
              value=${code}
              onInput=${(evt) => setCode(evt.target.value)}
            />
            <button type="submit" class="btn gold" disabled=${!code.trim()}>Submit</button>
          </form>
        <//>
      `}
    `;
  }
  const failure = install?.state === 'failed' ? `${program.title} was not installed: ${install.error}`
    : signIn?.state === 'failed' ? signIn.error : null;
  return html`
    <${SettingsRow}
      title=${connection.envKey ? 'Cursor login' : 'Not signed in'}
      help=${html`${SIGN_IN_HELP[id]}${!installed && ` The app installs ${program.title} first, about ${program.mb} MB.`}${failure && html`<span class="s-signin-error">${failure}</span>`}`}
    >
      <div class="s-signin-start">
        ${methods
          ? methods.map((method, i) => html`<button type="button" class="btn ${i === 0 ? 'gold' : ''}" onClick=${() => begin(method)}>${SIGN_IN_METHODS[method]}</button>`)
          : html`<button type="button" class="btn gold" onClick=${() => begin()}>Sign in</button>`}
      </div>
    <//>
  `;
}

function ProviderPane({ id, connection, favorites, onToggleFavorite, onChange, checkedAt, onRecheck, startWith, onStarted, onError }) {
  const runtime = connection.runtime;
  // Cursor's sign-in is beside its key; the others are their only connection.
  const signedIn = connection.envKey ? connection.signedIn : connection.connected;
  const signOut = () => api('/api/provider-logouts', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ provider: id }) })
    .then(onRecheck).catch(onError);
  return html`
    <div class="pane-head">
      <${Logo} name=${id} size="22" />
      <h3>${connection.label}</h3>
      ${connection.version && html`<span class="pane-version">v${connection.version}</span>`}
      <button type="button" class="s-recheck" onClick=${onRecheck} title="Check logins, keys and versions again">
        <${Icon} name="refresh" size="13" /> Checked ${ago(checkedAt)}
      </button>
    </div>

    <${SettingsSection} title="Account">
      ${signedIn && !connection.envKey && html`
        <${SettingsRow} title=${connection.account || 'Signed in'} help=${[connection.plan, id === 'codex' || id === 'codex-cli' ? 'ChatGPT login through the Codex CLI' : 'Claude login through Claude Code'].filter(Boolean).join(' · ')}>
          <button type="button" class="btn" onClick=${signOut}>Sign out</button>
        <//>
      `}
      ${!signedIn && !connection.envKey && html`<${SignInRows} id=${id} connection=${connection} startWith=${startWith} onStarted=${onStarted} onInstalled=${onRecheck} onSignedIn=${onRecheck} onError=${onError} />`}
      ${connection.envKey && html`
        <${SettingsRow}
          title="API key"
          help=${connection.keySource === 'saved' ? 'Saved in data/settings.json. Paste a new key to replace it.'
            : connection.keySource === 'env' ? `Read from ${connection.envKey}. A key saved here takes precedence.`
              : `Stored in data/settings.json and never sent to the browser. ${connection.envKey} works too.`}
        >
          <input
            id=${`key-${id}`}
            class="s-input mono"
            type="password"
            autocomplete="off"
            value=${connection.apiKey}
            placeholder=${connection.keySource === 'saved' ? '••••••••••••' : 'Paste your key'}
            onInput=${(evt) => onChange('apiKey', evt.target.value)}
          />
        <//>
      `}
      ${connection.envKey && connection.signIn && (signedIn ? html`
        <${SettingsRow} title="Signed in with Cursor" help="Used when no key is set.">
          <button type="button" class="btn" onClick=${signOut}>Sign out</button>
        <//>
      ` : html`<${SignInRows} id=${id} connection=${connection} startWith=${startWith} onStarted=${onStarted} onInstalled=${onRecheck} onSignedIn=${onRecheck} onError=${onError} />`)}
    <//>

    <${ModelsSection} id=${id} connection=${connection} favorites=${favorites} onToggleFavorite=${onToggleFavorite} onChange=${onChange} checkedAt=${checkedAt} />

    <${SettingsSection} title="Runtime">
      ${runtime && html`
        <${SettingsRow} title="Binary" help=${`Set by ${runtime.binaryEnv} in the server's environment; unset, ${runtime.binary} on PATH, else the one the app installed.`}>
          <span class="s-value mono">${runtime.binaryValue || runtime.binary}</span>
        <//>
        <${SettingsRow} title="Config directory" help=${`Set by ${runtime.homeEnv} in the server's environment. Login and sessions live here.`}>
          <span class="s-value mono">${runtime.resolvedHome}</span>
        <//>
      `}
      ${connection.endpoint && html`<${SettingsRow} title="Endpoint"><span class="s-value mono">${connection.endpoint}</span><//>`}
      <${SettingsRow} title="Protocol"><span class="s-value">${connection.protocol}</span><//>
      ${!connection.harness && id !== 'codex' && html`<${SettingsRow} title="Loop" help="mcbench's own turn loop calls this API with mcbench's tools." />`}
      ${(RUNTIME_NOTES[id] || []).map(([title, help]) => html`<${SettingsRow} title=${title} help=${help} />`)}
    <//>
  `;
}

// Settings' Connect an AI pane: each way to give mcbench a model, what it
// needs and who bills it, with a button to each provider pane that sets it
// up. A subscription not signed in (a CLI's login) is one button: it opens
// its pane on its sign-in, installing the CLI first when it is missing.
function ConnectPane({ providers, onSelect, onConnect }) {
  return html`
    <div class="pane-head"><${Icon} name="plug" size="20" /><h3>Connect an AI</h3></div>
    <p class="connect-intro">mcbench builds with a model you connect. Choose one way in; you can add others later.</p>
    <div class="connect-ways">
      ${connectWays(providers).map((way) => html`
        <section class="connect-way" key=${way.key} data-way=${way.key}>
          <h4 class="connect-way-title">${way.title}</h4>
          <p class="connect-way-need">${way.need.split('`').map((part, i) => (i % 2 ? html`<code>${part}</code>` : part))}</p>
          <div class="connect-way-panes">
            ${way.panes.map(({ id, connection }) => html`
              <button type="button" class="connect-pane ${usable(connection) ? 'connected' : ''}" data-pane=${id}
                onClick=${() => (connection.auth === 'cli-login' && !connection.connected ? onConnect(id) : onSelect(id))}>
                <${Logo} name=${id} size="16" />
                <span class="connect-pane-name">${connection.label}</span>
                <span class="connect-pane-state">${connection.auth === 'cli-login' && !connection.connected ? 'Sign in' : connection.summary}</span>
                <${Icon} name="forward" size="13" />
              </button>
            `)}
          </div>
        </section>
      `)}
    </div>
  `;
}

// The first start on the new build's screen (setup-status.mjs): each step
// and the one under way, how many of the steps the app needs are done, and
// what to do meanwhile: connect an AI, or write the first build, which is
// sent once the app is ready. A failed step is tried again from here
// (POST /api/setup/retry).
function SetupStatus({ status, noAI, onConnect, onRetry }) {
  const mark = {
    waiting: html`<span class="setup-dot"></span>`,
    running: html`<span class="working-dots"><i></i><i></i><i></i></span>`,
    done: html`<${Icon} name="check" size="13" />`,
    failed: html`<${Icon} name="x" size="13" />`,
    unavailable: html`<span class="setup-dash"></span>`,
  };
  return html`
    <div class="setup">
      <div class="setup-head">
        <div class="big">Getting ready</div>
        <div class="setup-sub">The first start downloads and builds what the app needs. This happens once.</div>
      </div>
      <div class="setup-overall">
        <div class="setup-bar"><span style=${`width: ${(100 * status.done) / status.total}%`}></span></div>
        <span class="setup-count" role="status">${status.done} of ${status.total} done</span>
      </div>
      ${status.failed && html`
        <div class="setup-failed">
          <span>A step failed.</span>
          <button type="button" class="btn" onClick=${onRetry}>Try again</button>
        </div>
      `}
      <ol class="setup-steps">
        ${status.rows.map((row) => html`
          <li class="setup-step ${row.state}" key=${row.id}>
            <span class="setup-mark" aria-hidden="true">${mark[row.state]}</span>
            <span class="setup-label">${row.label}${row.optional && html`<span class="setup-optional">Optional</span>`}</span>
            <span class="setup-value">${row.value}</span>
            ${row.note && html`<span class="setup-note">${linkParts(row.note).map((part) => (part.url
              ? html`<a href=${part.url} target="_blank" rel="noreferrer">${part.url}</a>`
              : part.text))}</span>`}
          </li>
        `)}
      </ol>
      ${noAI ? html`
        <div class="connect-hint">
          <div class="connect-hint-text">Connect an AI while this runs</div>
          <button type="button" class="btn gold connect-hint-button" onClick=${onConnect}>Connect an AI</button>
        </div>
      ` : html`<div class="empty-sub">You can send your first build now. It starts when the app is ready.</div>`}
    </div>
  `;
}

// Sidebar width in px: its default, its resize range, and the pull past which
// it collapses instead.
const SIDEBAR = { width: 236, min: 200, max: 420, collapseBelow: 140 };

const initialModelSelection = storedJson('mcb-last-model', { provider: '', model: '' });
const initialPromptModelSelection = storedJson('mcb-prompt-ai-model', initialModelSelection);

// An anchored picker: Escape or a click outside closes it once its exit
// animation (exitMs) has played.
function usePopover(exitMs) {
  const [open, setOpenState] = useState(false);
  const [closing, setClosing] = useState(false);
  const rootRef = useRef(null);
  const openRef = useRef(false);
  const closeTimerRef = useRef(null); // set while the exit animation plays

  // Opening again cancels a close still animating.
  const setOpen = (next) => {
    clearTimeout(closeTimerRef.current);
    closeTimerRef.current = null;
    openRef.current = next;
    setOpenState(next);
    setClosing(false);
  };

  // A second Escape or click during the exit animation is the same close.
  const closePicker = () => {
    if (!openRef.current || closeTimerRef.current) return;
    setClosing(true);
    closeTimerRef.current = setTimeout(() => {
      closeTimerRef.current = null;
      openRef.current = false;
      setOpenState(false);
      setClosing(false);
    }, exitMs);
  };

  useEffect(() => () => clearTimeout(closeTimerRef.current), []);

  useEffect(() => {
    if (!open) return undefined;
    const close = (evt) => {
      if (evt.key === 'Escape' || (evt.type === 'mousedown' && !rootRef.current?.contains(evt.target))) {
        if (evt.key === 'Escape') evt.stopPropagation();
        closePicker();
      }
    };
    document.addEventListener('mousedown', close);
    document.addEventListener('keydown', close);
    return () => {
      document.removeEventListener('mousedown', close);
      document.removeEventListener('keydown', close);
    };
  }, [open]);

  return { open, setOpen, closing, closePicker, rootRef };
}

function ModelPicker({
  providers,
  value,
  onChange,
  favorites,
  onToggleFavorite,
  requireTools,
  label,
  emptyLabel = `Choose ${label.toLowerCase()}`,
  // Choosing several: the picker stays open and each click toggles a model.
  selected = null,
  onToggle = null,
  // A last row that leaves the list: { label, icon, onSelect }.
  footer = null,
  // Opens Settings on a pane: the rail's set-up button and, while no AI is
  // connected, the picker's Connect an AI button open Connect an AI.
  onSetup,
  // Set, the picker opens by itself as its button would open it, then
  // `onOpened` is called.
  openNow = false,
  onOpened = null,
}) {
  const { open, setOpen, closing, closePicker, rootRef } = usePopover(120);
  const usableIds = Object.keys(providers).filter((id) => usable(providers[id]));
  const usableFavorites = favorites.filter((key) => usable(providers[key.split('::')[0]]));
  // The tab the picker opens on: the chosen model's provider, else Favorites
  // when there are any, else the first provider that can be used; null when
  // none can.
  const openingTab = () => {
    if (value?.provider && usable(providers[value.provider])) return value.provider;
    if (usableFavorites.length) return 'favorites';
    return usableIds[0] ?? null;
  };
  const [activeProvider, setActiveProvider] = useState(openingTab);
  // A tab whose provider can no longer be used (turned off in Settings,
  // signed out), or no tab once one can, gives way to the opening tab.
  useEffect(() => {
    if (activeProvider === 'favorites' || usable(providers[activeProvider])) return;
    const next = openingTab();
    if (next !== activeProvider) setActiveProvider(next);
  }, [activeProvider, providers]);
  const [query, setQuery] = useState('');
  const needle = query.trim().toLowerCase();
  useEffect(() => {
    if (!openNow) return;
    onOpened();
    setActiveProvider(openingTab());
    setOpen(true);
  }, [openNow]);

  // An open popover moves sideways, if it must, to stay 8px inside the window
  // (an anchored one would hang off a phone's edge). Measured from its centre
  // and laid-out width, as it opens scaled.
  useLayoutEffect(() => {
    if (!open) return;
    const popover = rootRef.current.querySelector('.model-picker-popover');
    popover.style.setProperty('--nudge', '0px');
    const box = popover.getBoundingClientRect();
    const half = popover.offsetWidth / 2;
    const left = (box.left + box.right) / 2 - half;
    const right = left + 2 * half;
    const nudge = left < 8 ? 8 - left : right > window.innerWidth - 8 ? window.innerWidth - 8 - right : 0;
    popover.style.setProperty('--nudge', `${nudge}px`);
    // The rail scrolls when it holds more providers than fit: it centres the
    // open tab (the popover is the tabs' offset parent, the rail at its edge).
    const rail = popover.querySelector('.model-provider-rail');
    const tab = rail.querySelector('.active');
    if (tab) {
      rail.scrollTop = tab.offsetTop - (rail.clientHeight - tab.offsetHeight) / 2;
      rail.scrollLeft = tab.offsetLeft - (rail.clientWidth - tab.offsetWidth) / 2;
    }
  }, [open]);

  // The providers listed: with a query, every provider that can be used (each
  // catalog loads as a tab click would load it); otherwise the tab's,
  // Favorites' providers or the one provider. Catalogs are shared and
  // prefetched, so lists are there the moment the picker opens.
  const shownProviders = (needle ? usableIds
    : activeProvider === 'favorites' ? [...new Set(usableFavorites.map((key) => key.split('::')[0]))]
      : [activeProvider]).filter((provider) => usable(providers[provider]));
  const entries = useShared(shownProviders.map(modelsUrl));
  const loading = entries.some((entry) => !entry.value && !entry.error);
  const error = entries.map((entry, i) => entry.error && (needle ? `${providers[shownProviders[i]].label}: ${entry.error.message}` : entry.error.message)).filter(Boolean)[0] || '';

  // Settings decide which models each provider offers, and in what order.
  let models = shownProviders.flatMap((provider, i) => orderModels(
    (entries[i].value?.models || []).filter((model) => !providers[provider].hiddenModels.includes(model.id)),
    providers[provider].modelOrder,
  ).map((model) => ({ ...model, provider })));
  if (!needle && activeProvider === 'favorites') models = models.filter((model) => favorites.includes(favoriteKey(model.provider, model.id)));
  models = models.filter((model) => {
    if (requireTools && model.tools !== true) return false;
    return !needle || model.name.toLowerCase().includes(needle) || model.id.toLowerCase().includes(needle);
  });
  if (activeProvider === 'openrouter' && !needle) {
    const starred = models.filter((model) => favorites.includes(favoriteKey('openrouter', model.id)));
    const curated = models
      .filter((model) => !starred.includes(model) && CURATED_VENDORS.some((prefix) => model.id.startsWith(prefix)))
      .slice(0, CURATED_LIMIT);
    models = [...starred, ...curated];
  }

  const chooseProvider = (id) => {
    if (id !== 'favorites' && !usable(providers[id])) return;
    setActiveProvider(id);
    setQuery('');
  };
  const selectedLabel = value?.model ? modelLabel(value.model) : emptyLabel;
  const connectAI = () => { closePicker(); onSetup(CONNECT); };

  return html`
    <div class="model-picker" ref=${rootRef}>
      <button
        type="button"
        class="model-picker-trigger ${value?.model ? '' : 'empty'}"
        title=${value?.model || emptyLabel}
        onClick=${() => {
          if (open) return closePicker();
          // Opens on the current model's provider, which may have changed with the chat.
          setActiveProvider(openingTab());
          setOpen(true);
        }}
      >
        ${value?.model && html`<${Logo} name=${vendorIcon(value.provider, value.model)} size="14" />`}
        <span>${selectedLabel}</span><span class="picker-chevron"><${Icon} name="chevron-down" size="12" /></span>
      </button>
      ${open && html`
        <div class="model-picker-popover ${closing ? 'closing' : ''}">
          <nav class="model-provider-rail" aria-label="Providers">
            <button
              type="button"
              class=${activeProvider === 'favorites' && !needle ? 'active' : ''}
              title="Favorites"
              onClick=${() => chooseProvider('favorites')}
            ><${Icon} name="star" size="16" /></button>
            ${Object.entries(providers).filter(([, provider]) => provider.enabled).map(([id, provider], index, entries) => html`
              ${provider.harness && !entries[index - 1]?.[1].harness && html`
                <span class="rail-divider" role="separator" title="Harnesses"></span>
              `}
              <button
                type="button"
                class="${activeProvider === id && !needle ? 'active' : ''} ${usable(provider) ? '' : 'disabled'} ${provider.harness ? 'harness' : ''}"
                title="${provider.label}${provider.harness ? ' harness: its own agent loop with mcbench tools attached' : ''}${!provider.enabled ? ' — disabled in Settings' : provider.connected ? '' : (provider.auth === 'cli-login' ? ' — CLI not signed in' : ' — not configured')}"
                disabled=${!usable(provider)}
                onClick=${() => chooseProvider(id)}
              >
                <${Logo} name=${id} size="18" />
                ${provider.harness && html`<span class="harness-badge" aria-hidden="true">›_</span>`}
              </button>
            `)}
            <button
              type="button"
              class="rail-setup"
              title="Set up providers"
              aria-label="Set up providers"
              onClick=${connectAI}
            ><${Icon} name="plus" size="16" /></button>
          </nav>
          ${usableIds.length === 0 ? html`
            <div class="model-picker-main model-picker-no-ai">
              <div class="model-picker-no-ai-text">No AI connected yet</div>
              <button type="button" class="btn gold model-picker-connect" onClick=${connectAI}>Connect an AI</button>
            </div>
          ` : html`
            <div class="model-picker-main">
              <div class="model-picker-search">
                <${Icon} name="search" size="15" />
                <input
                  autofocus
                  value=${query}
                  placeholder="Search models..."
                  onInput=${(evt) => setQuery(evt.target.value)}
                />
              </div>
              <div class="model-picker-list">
                ${loading && !models.length && html`<div class="model-picker-empty">Loading models...</div>`}
                ${error && html`<div class="model-picker-error"><span>${error}</span></div>`}
                ${!loading && !error && !models.length && html`
                  <div class="model-picker-empty">${needle ? 'No model matches' : activeProvider === 'favorites' ? 'No favorite models yet.' : 'No matching models.'}</div>
                `}
                ${models.map((model) => {
                  const key = favoriteKey(model.provider, model.id);
                  const favorite = favorites.includes(key);
                  const chosen = selected
                    ? selected.some((entry) => entry.provider === model.provider && entry.model === model.id)
                    : value?.provider === model.provider && value?.model === model.id;
                  return html`
                    <div class="model-picker-row ${chosen ? 'selected' : ''}" key=${key}>
                      <button
                        type="button"
                        class="model-choice"
                        title=${model.id}
                        aria-pressed=${selected ? chosen : undefined}
                        onClick=${() => {
                          if (onToggle) return onToggle({ provider: model.provider, model: model.id });
                          onChange({ provider: model.provider, model: model.id });
                          closePicker();
                        }}
                      >
                        <${Logo} name=${vendorIcon(model.provider, model.id)} size="16" />
                        <span class="model-name">${displayName(model)}</span>
                        ${needle && html`<span class="model-provider">${providers[model.provider].label}</span>`}
                        ${selected && html`<span class="model-check ${chosen ? 'on' : ''}"><${Icon} name="check" size="13" /></span>`}
                      </button>
                      <button
                        type="button"
                        class="model-star ${favorite ? 'favorite' : ''}"
                        title=${favorite ? 'Remove favorite' : 'Add favorite'}
                        onClick=${() => onToggleFavorite(key)}
                      ><${Icon} name="star" size="14" /></button>
                    </div>
                  `;
                })}
              </div>
              ${footer && html`
                <button type="button" class="model-picker-footer" onClick=${() => { closePicker(); footer.onSelect(); }}>
                  <${Icon} name=${footer.icon} size="15" /><span>${footer.label}</span><${Icon} name="forward" size="13" />
                </button>
              `}
            </div>
          `}
        </div>
      `}
    </div>
  `;
}

function EffortPicker({ caps, value, onChange, label = 'Reasoning effort' }) {
  const { open, setOpen, closing, closePicker, rootRef } = usePopover(110);

  const options = caps.efforts.map((effort) => ({ value: effort, label: effort }));
  // '' means "not explicitly set": show the model's default effort as current.
  const selected = options.find((option) => option.value === value)
    || options.find((option) => option.value === caps.defaultEffort)
    || options[0];

  return html`
    <div class="effort-picker" ref=${rootRef}>
      <button
        type="button"
        class="effort-picker-trigger ${open ? 'open' : ''}"
        aria-label=${label}
        aria-haspopup="listbox"
        aria-expanded=${open}
        onClick=${() => { if (open) closePicker(); else setOpen(true); }}
      >
        <span>${selected.label}</span>
        <span class="picker-chevron"><${Icon} name="chevron-down" size="12" /></span>
      </button>
      ${open && html`
        <div class="effort-picker-menu ${closing ? 'closing' : ''}" role="listbox" aria-label=${label}>
          ${options.map((option) => html`
            <button
              type="button"
              role="option"
              aria-selected=${option === selected}
              class=${option === selected ? 'selected' : ''}
              onClick=${() => {
                onChange(option.value);
                closePicker();
              }}
            >
              <span class="effort-option-label">${option.label}</span>
              ${option === selected && html`<${Icon} name="check" size="14" />`}
            </button>
          `)}
        </div>
      `}
    </div>
  `;
}

// Random composer placeholder: a rotating invitation instead of a form label,
// shown only in a chat with no prompt yet.
const IDEAS = [
  'Add a cat',
  'A cozy hobbit hole',
  'A lighthouse on a stormy cliff',
  'A giant rubber duck',
  'A wizard tower, slightly crooked',
  'A ramen stand at midnight',
  'A pirate ship run aground',
  'A treehouse village with rope bridges',
  'Saturn, with rings',
  'A snow globe with a cabin inside',
  'A dragon skeleton in the desert',
  'A floating sky island with a waterfall',
  'The Moai heads of Easter Island',
  'A medieval tavern with a secret basement',
  'A whale breaching out of the ocean',
  'A cathedral of stained glass',
  'A tiny red barn with fat sheep',
  'An overgrown greenhouse',
  'A castle with an excessive moat',
  'A UFO abducting a cow',
  'A giant chess board mid-game',
  'A creeper topiary garden',
  'A Japanese pagoda in autumn',
  'A desert oasis with palm trees',
  'A steampunk airship docked at a tower',
  'A volcano with a lava river',
  'A cherry blossom bridge over a koi pond',
  'An abandoned mineshaft entrance',
  'A cyberpunk alley with neon signs',
  'A sunken temple, half flooded',
  'A windmill on a hill of wheat',
  'A bonsai tree, but huge',
  'A space station orbiting nothing in particular',
  'An igloo village under the northern lights',
  'A lunar base with a rover',
  'A mushroom forest with glowing caps',
  'A Greek temple on a cliff',
  'A cozy library with a reading nook',
  'A coral reef with a shipwreck',
  'A haunted Victorian house',
  'A giant clock tower',
  'A beach hut with a surfboard',
  'A Viking longhouse',
  'A dam holding back a reservoir',
  'A roller coaster that loops',
  'A Mayan step pyramid in the jungle',
  'A fishing village on stilts',
  'A potted cactus the size of a building',
  'The Eiffel Tower',
  'The Colosseum',
  'The Golden Gate Bridge in fog',
  'A Dutch canal house row',
  'A Moroccan riad with a courtyard fountain',
  'A train station with a steam engine',
  'A hot air balloon over a valley',
  'A giant teapot cottage',
  'A burger the size of a house',
  'A bowl of fruit, enormous',
  'A sleeping fox curled up',
  'An axolotl, pixel art',
  'A parrot on a pirate hat',
  'A guitar lying on its side',
  'A grand piano',
  'A pixel art sunset',
  'A rocket on the launch pad',
  'A submarine surfacing',
  'A medieval market square',
  'An observatory with an open dome',
  'A desert canyon with a rope bridge',
  'A frozen waterfall',
  'A hedge maze with a fountain in the middle',
  'A tiny planet with one house on it',
  'A sandcastle, very serious',
  'A working 4-bit adder with lamps',
  'A piston door with a combination lock',
  'A redstone clock that ticks on a display',
  'A seven-segment display that counts up',
  'An automatic farm with hoppers',
  'A flying machine that goes and comes back',
  'A secret staircase behind a bookshelf',
  'A marble run made of rails',
  'A vending machine that dispenses snacks',
];

// Rarer ones: about one empty chat in forty gets one of these.
const EGGS = [
  "Herobrine's summer house",
  'A redstone computer that runs Minecraft (good luck)',
  'The cake, which is not a lie',
  'A statue of the person reading this',
  'A to-scale replica of this text box',
  'Something that would confuse an archaeologist',
  'Whatever you were going to build anyway',
  'A monument to the first dirt hut',
  'A house that is definitely not haunted',
  'The Far Lands, but cozy',
];

// On a few days of the year, half the empty chats get the day's idea.
const SEASONAL = {
  '1-1': 'Fireworks over a city at midnight',
  '2-14': 'A heart made of roses in a garden',
  '3-17': 'A pot of gold at the end of a rainbow',
  '4-1': 'A perfectly normal house, upside down',
  '10-31': 'A haunted manor with a pumpkin patch',
  '12-25': 'A gingerbread village under snow',
};

function pickIdea() {
  const today = new Date();
  const seasonal = SEASONAL[`${today.getMonth() + 1}-${today.getDate()}`];
  if (seasonal && Math.random() < 0.5) return seasonal;
  const pool = Math.random() < 1 / 40 ? EGGS : IDEAS;
  return pool[(Math.random() * pool.length) | 0];
}

// Muted, theme-adjacent hues. A tag's color is a stable hash of its name, so
// "terrain" is the same color everywhere without storing anything.
const TAG_COLORS = ['#7da869', '#c9a25e', '#6f9aa8', '#9a7fae', '#a8785f', '#a86f7e', '#8fa86f', '#6f7fa8'];

function tagColor(tag) {
  let hash = 0;
  for (const ch of tag) hash = (hash * 31 + ch.codePointAt(0)) >>> 0;
  return TAG_COLORS[hash % TAG_COLORS.length];
}

// Prompts for the palette: favorites, then the five most recently used, then
// the rest in library order. Search covers title, text and tags.
function promptSections(prompts, query) {
  const needle = query.trim().toLowerCase();
  const matches = prompts.filter((prompt) => (
    !needle
    || (prompt.title || '').toLowerCase().includes(needle)
    || (prompt.text || '').toLowerCase().includes(needle)
    || (prompt.tags || []).some((tag) => tag.toLowerCase().includes(needle))
  ));
  const favorites = matches.filter((prompt) => prompt.favorite);
  const recent = matches.filter((prompt) => !prompt.favorite && prompt.usedAt)
    .sort((a, b) => b.usedAt - a.usedAt).slice(0, 5);
  const rest = matches.filter((prompt) => !favorites.includes(prompt) && !recent.includes(prompt));
  return [['Favorites', favorites], ['Recent', recent], [needle || favorites.length || recent.length ? 'All' : '', rest]]
    .filter(([, list]) => list.length);
}

// The mark on a prompt a model added through its prompt_library tool: the
// server stores the chat and the model (prompts.js), so the user can tell it
// from a prompt of their own. `compact` is the icon alone, for list rows.
function AddedByMark({ addedBy, compact = false }) {
  const label = `Added by ${modelLabel(addedBy.model)} in a chat`;
  return html`<span class="prompt-added-by ${compact ? 'compact' : ''}" title=${label} aria-label=${label}>
    <${Icon} name="sparkles" size=${compact ? 12 : 13} />${!compact && html`<span>${label}</span>`}
  </span>`;
}

function PromptText({ text }) {
  const parts = [];
  let last = 0;
  for (const match of text.matchAll(composer.FILL_IN)) {
    parts.push(text.slice(last, match.index), html`<mark>${match[0].slice(2, -2) || '…'}</mark>`);
    last = match.index + match[0].length;
  }
  parts.push(text.slice(last));
  return html`<p class="pp-text">${parts}</p>`;
}

// The prompt palette over the composer. Opened by typing "/" (the text after
// it filters) or by the prompts button (which adds its own search field).
// Arrow keys move, Enter or Tab inserts, Escape closes. The right side
// previews the highlighted prompt with its fill-ins and attachments.
// `onRetry` is set while the library has failed to load: it reads it again.
function PromptPalette({ prompts, query, index, withSearch, onQuery, onIndex, onPick, onEdit, onToggleFavorite, canSaveDraft, onSaveDraft, onRetry, onKey }) {
  const sections = promptSections(prompts, query);
  const flat = sections.flatMap(([, list]) => list);
  const current = flat[Math.min(index, flat.length - 1)] || null;
  const listRef = useRef(null);
  useEffect(() => {
    listRef.current?.querySelector('.pp-row.active')?.scrollIntoView({ block: 'nearest' });
  }, [index, query]);
  let position = -1;
  return html`
    <div class="prompt-palette" onMouseDown=${(evt) => { if (!evt.target.closest('input')) evt.preventDefault(); }}>
      <div class="pp-left">
        ${withSearch && html`
          <label class="pp-search">
            <${Icon} name="search" size="14" />
            <input id="prompt-palette-search" autofocus value=${query} placeholder="Search prompts"
              onInput=${(evt) => onQuery(evt.target.value)} onKeyDown=${onKey} />
          </label>
        `}
        <div class="pp-list" ref=${listRef}>
          ${sections.map(([label, list]) => html`
            ${label && sections.length > 1 && html`<div class="pp-label">${label}</div>`}
            ${list.map((prompt) => {
              position += 1;
              const at = position;
              return html`
                <div class="pp-row ${at === Math.min(index, flat.length - 1) ? 'active' : ''}" onMouseEnter=${() => onIndex(at)} onClick=${() => onPick(prompt)}>
                  <span class="pp-title">${prompt.title || 'Untitled prompt'}</span>
                  ${prompt.addedBy && html`<${AddedByMark} addedBy=${prompt.addedBy} compact />`}
                  ${prompt.attachments?.length > 0 && html`<span class="pp-meta"><${Icon} name="file" size="12" />${prompt.attachments.length}</span>`}
                  <button type="button" class="pp-edit" title="Edit prompt" aria-label=${`Edit ${prompt.title || 'prompt'}`}
                    onClick=${(evt) => { evt.stopPropagation(); onEdit(prompt); }}><${Icon} name="edit" size="13" /></button>
                  <button type="button" class="model-star ${prompt.favorite ? 'favorite' : ''}" title=${prompt.favorite ? 'Unfavorite' : 'Favorite'}
                    onClick=${(evt) => { evt.stopPropagation(); onToggleFavorite(prompt.id); }}><${Icon} name="star" size="13" /></button>
                </div>
              `;
            })}
          `)}
          ${!flat.length && (onRetry
            ? html`<div class="pp-empty" role="alert">Prompts failed to load. <button type="button" class="text-link" onClick=${onRetry}>Try again</button></div>`
            : html`<div class="pp-empty">${prompts.length ? 'No prompts match.' : 'No saved prompts yet.'}</div>`)}
        </div>
        ${canSaveDraft && html`<div class="pp-foot"><button type="button" onClick=${onSaveDraft}>Save message as prompt</button></div>`}
      </div>
      ${current && html`
        <div class="pp-preview">
          <div class="pp-preview-title">${current.title || 'Untitled prompt'}</div>
          ${current.addedBy && html`<${AddedByMark} addedBy=${current.addedBy} />`}
          ${current.tags?.length > 0 && html`
            <div class="pp-tags">${current.tags.map((tag) => html`<span style=${`--tag:${tagColor(tag)}`}>${tag}</span>`)}</div>
          `}
          <${PromptText} text=${current.text || ''} />
          ${current.attachments?.length > 0 && html`
            <div class="pp-files">
              ${current.attachments.map((file) => file.type.startsWith('image/')
                ? html`<img src=${file.data} alt=${file.name} title=${file.name} />`
                : html`<span title=${file.name}><${Icon} name="file" size="14" />${file.name}</span>`)}
            </div>
          `}
        </div>
      `}
    </div>
  `;
}

// Prompt library in the app sidebar: search, drag to reorder, favorite, and
// delete. Selection opens the prompt in Bench's editor pane.
function PromptList({ prompts, query, tags, activeId, onSwitch, onReorder, onToggleFavorite, onDelete }) {
  // The first click on × arms it; a second click within 3 seconds deletes.
  const [armed, setArmed] = useState(null);
  useEffect(() => {
    if (!armed) return undefined;
    const timer = setTimeout(() => setArmed(null), 3000);
    return () => clearTimeout(timer);
  }, [armed]);

  const needle = query.trim().toLowerCase();
  const visible = prompts.filter((prompt) => (
    (!needle
      || (prompt.title || '').toLowerCase().includes(needle)
      || (prompt.text || '').toLowerCase().includes(needle)
      || (prompt.tags || []).some((tag) => tag.toLowerCase().includes(needle)))
    && (!tags.length || (prompt.tags || []).some((tag) => tags.includes(tag)))
  ));
  // Reordering a filtered list is ambiguous, so dragging only works unfiltered.
  const sortable = !needle && !tags.length;

  // Dragging moves the row itself: it follows the pointer, and the rows it
  // passes slide out of its way, so where it will land is always what shows.
  const listRef = useRef(null);
  const pressRef = useRef(null); // pointer down on a row; becomes a drag past 4px
  const clickRef = useRef(true); // false right after a drag, so the drop is not a click
  const [drag, setDragState] = useState(null); // { from, to, dy, slot, dropping }
  const dragRef = useRef(null);
  const setDrag = (next) => { dragRef.current = next; setDragState(next); };
  // The list going away mid-drag (Build, a search) lets go of the cursor.
  useEffect(() => () => document.body.classList.remove('grabbing'), []);

  function lift(evt, index) {
    clickRef.current = true;
    if (!sortable || evt.button !== 0 || evt.target.closest('button')) return;
    pressRef.current = { index, y: evt.clientY, x: evt.clientX, row: evt.currentTarget, started: false };
  }

  function follow(evt) {
    const press = pressRef.current;
    if (!press) return;
    const list = listRef.current;
    if (!press.started) {
      if (Math.hypot(evt.clientX - press.x, evt.clientY - press.y) < 4) return;
      const rows = [...list.querySelectorAll('.pe-item')];
      press.started = true;
      press.tops = rows.map((row) => row.offsetTop);
      press.heights = rows.map((row) => row.offsetHeight);
      press.scroll = list.scrollTop;
      press.row.setPointerCapture(evt.pointerId);
      document.body.classList.add('grabbing');
    }
    const box = list.getBoundingClientRect();
    if (evt.clientY < box.top + 28) list.scrollTop -= 8;
    else if (evt.clientY > box.bottom - 28) list.scrollTop += 8;
    const { index: from, tops, heights } = press;
    const last = tops.length - 1;
    const dy = Math.min(
      Math.max(evt.clientY - press.y + list.scrollTop - press.scroll, tops[0] - tops[from]),
      tops[last] + heights[last] - tops[from] - heights[from],
    );
    // A row gives way once the lifted row's leading edge passes its middle.
    const top = tops[from] + dy;
    const below = tops.filter((t, j) => j > from && top + heights[from] > t + heights[j] / 2).length;
    const above = tops.filter((t, j) => j < from && top < t + heights[j] / 2).length;
    const to = from + below - above;
    const gap = last > 0 ? tops[1] - tops[0] - heights[0] : 0;
    setDrag({ from, to, dy, slot: heights[from] + gap, dropping: false });
  }

  function drop() {
    const press = pressRef.current;
    pressRef.current = null;
    if (!press?.started) return;
    clickRef.current = false;
    document.body.classList.remove('grabbing');
    const current = dragRef.current;
    if (!current) return;
    const { from, to } = current;
    const { tops, heights } = press;
    // Settle into the gap the other rows opened, then commit the order.
    setDrag({ ...current, dy: to > from ? tops[to] + heights[to] - tops[from] - heights[from] : tops[to] - tops[from], dropping: true });
    setTimeout(() => {
      if (from !== to) onReorder(from, to);
      setDrag(null);
    }, 150);
  }

  const offset = (index) => {
    if (!drag) return 0;
    const { from, to, dy, slot } = drag;
    if (index === from) return dy;
    if (from < to && index > from && index <= to) return -slot;
    if (to < from && index >= to && index < from) return slot;
    return 0;
  };

  return html`
    <div class="prompt-side">
      <div class="pe-list ${drag ? 'sorting' : ''}" ref=${listRef}>
        ${visible.map((prompt, index) => html`
          <div
            key=${prompt.id}
            class="pe-item ${prompt.id === activeId ? 'active' : ''} ${drag?.from === index ? `lifted ${drag.dropping ? 'dropping' : ''}` : ''}"
            style=${drag ? `transform: translateY(${offset(index)}px)` : ''}
            onPointerDown=${(evt) => lift(evt, index)}
            onPointerMove=${follow}
            onPointerUp=${drop}
            onPointerCancel=${drop}
            onClick=${() => { if (clickRef.current) onSwitch(prompt.id); }}
          >
            <span class="pe-item-title">${prompt.title || 'Untitled prompt'}</span>
            ${prompt.addedBy && html`<${AddedByMark} addedBy=${prompt.addedBy} compact />`}
            ${prompt.tags?.length > 0 && html`
              <span class="pe-item-dots">
                ${prompt.tags.map((tag) => html`<i style=${`background:${tagColor(tag)}`}></i>`)}
              </span>
            `}
            <span class="pe-item-actions">
              <button
                class="model-star ${prompt.favorite ? 'favorite' : ''}"
                title=${prompt.favorite ? 'Unfavorite prompt' : 'Favorite prompt'}
                aria-label=${prompt.favorite ? 'Unfavorite prompt' : 'Favorite prompt'}
                onClick=${(evt) => { evt.stopPropagation(); onToggleFavorite(prompt.id); }}
              ><${Icon} name="star" size="13" /></button>
              <button
                class="pe-item-delete ${armed === prompt.id ? 'armed' : ''}"
                title=${armed === prompt.id ? 'Click again to delete' : 'Delete prompt'}
                aria-label=${armed === prompt.id ? 'Confirm delete' : 'Delete prompt'}
                onClick=${(evt) => {
                  evt.stopPropagation();
                  if (armed === prompt.id) { setArmed(null); onDelete(prompt.id); } else setArmed(prompt.id);
                }}
              >${armed === prompt.id ? 'Delete' : html`<${Icon} name="x" size="13" />`}</button>
            </span>
          </div>
        `)}
        ${prompts.length > 0 && visible.length === 0 && html`<span class="pe-empty">No prompts match.</span>`}
      </div>
    </div>
  `;
}

// The library's sidebar section: its header folds it, the search button opens
// into a bar in the same row, and the filter inside that bar narrows by tag.
function PromptsSection({ prompts, onCreate, ...list }) {
  const [open, setOpen] = useState(() => localStorage.getItem('mcb-prompts-open') !== 'closed');
  const [searching, setSearching] = useState(false);
  const [query, setQuery] = useState('');
  const [tags, setTags] = useState([]);
  const filter = usePopover(110);
  const inputRef = useRef(null);
  const searchRef = useRef(null);
  useEffect(() => { localStorage.setItem('mcb-prompts-open', open ? 'open' : 'closed'); }, [open]);
  // A click anywhere else closes the search.
  useEffect(() => {
    if (!searching) return undefined;
    const close = (evt) => { if (!searchRef.current.contains(evt.target)) endSearch(); };
    document.addEventListener('mousedown', close);
    return () => document.removeEventListener('mousedown', close);
  }, [searching]);
  const allTags = [...new Set(prompts.flatMap((prompt) => prompt.tags || []))].sort();
  const shownTags = tags.filter((tag) => allTags.includes(tag));

  function startSearch() {
    setOpen(true);
    setSearching(true);
    requestAnimationFrame(() => inputRef.current?.focus());
  }
  function endSearch() {
    setSearching(false);
    setQuery('');
    filter.closePicker();
  }

  return html`
    <div class="side-section ${searching ? 'searching' : ''}">
      <button type="button" class="side-title" aria-expanded=${open} onClick=${() => setOpen((current) => !current)}>
        <${Icon} name="chevron-right" size="12" />
        <span>Prompts</span>
        ${shownTags.length > 0 && html`<span class="side-tags" aria-label="Filtered by tag">${shownTags.map((tag) => html`<i style=${`background:${tagColor(tag)}`}></i>`)}</span>`}
      </button>
      <div class="side-search" ref=${searchRef}>
        <button type="button" class="side-search-icon" aria-label="Search prompts" tabIndex=${searching ? -1 : 0} onClick=${startSearch}>
          <${Icon} name="search" size="14" />
        </button>
        <input
          ref=${inputRef}
          value=${query}
          aria-label="Search prompts"
          tabIndex=${searching ? 0 : -1}
          onInput=${(evt) => setQuery(evt.target.value)}
          onKeyDown=${(evt) => { if (evt.key === 'Escape') endSearch(); }}
        />
        ${allTags.length > 0 && html`
          <div class="side-filter" ref=${filter.rootRef}>
            <button
              type="button"
              class="side-filter-button ${shownTags.length ? 'active' : ''}"
              aria-label="Filter by tag"
              aria-expanded=${filter.open}
              tabIndex=${searching ? 0 : -1}
              onClick=${() => (filter.open ? filter.closePicker() : filter.setOpen(true))}
            ><${Icon} name="filter" size="14" /></button>
            ${filter.open && html`
              <div class="tag-menu ${filter.closing ? 'closing' : ''}" role="menu">
                ${allTags.map((tag) => {
                  const on = shownTags.includes(tag);
                  return html`
                    <button
                      type="button"
                      role="menuitemcheckbox"
                      aria-checked=${on}
                      class=${on ? 'selected' : ''}
                      onClick=${() => setTags((current) => (on ? current.filter((entry) => entry !== tag) : [...current, tag]))}
                    >
                      <i style=${`background:${tagColor(tag)}`}></i>
                      <span>${tag}</span>
                      ${on && html`<${Icon} name="check" size="13" />`}
                    </button>
                  `;
                })}
              </div>
            `}
          </div>
        `}
      </div>
      <button class="icon-btn side-add" title="New prompt" aria-label="New prompt" onClick=${onCreate}>
        <${Icon} name="plus" size="14" />
      </button>
    </div>
    ${open && html`<${PromptList} prompts=${prompts} query=${query} tags=${shownTags} ...${list} />`}
  `;
}

// Obsidian-style prompt editor pane, opened from Bench. The prompt list
// itself lives in the app sidebar (PromptList); this renders the active one.
function PromptEditor({
  prompts,
  activeId,
  providers,
  favorites,
  onToggleFavorite,
  onSetup,
  onCreate,
  onUpdate,
  onError,
}) {
  const active = prompts.find((prompt) => prompt.id === activeId);
  const fileRef = useRef(null);
  const activeRef = useRef(active);
  const requestRef = useRef(null);
  activeRef.current = active;
  const [tagDraft, setTagDraft] = useState('');
  const knownTags = [...new Set(prompts.flatMap((prompt) => prompt.tags || []))].filter((tag) => !(active?.tags || []).includes(tag));
  const addTag = (raw) => {
    const tag = raw.trim().toLowerCase();
    setTagDraft('');
    if (tag && !(active.tags || []).includes(tag)) onUpdate(activeId, { tags: [...(active.tags || []), tag] });
  };
  const [aiModel, setAiModel] = useState(() => ({
    provider: String(initialPromptModelSelection?.provider || ''),
    model: String(initialPromptModelSelection?.model || ''),
    effort: String(initialPromptModelSelection?.effort || ''),
  }));
  const aiCaps = useModelInfo(aiModel.provider, aiModel.model);
  const [aiBusy, setAiBusy] = useState(false);
  const [aiError, setAiError] = useState('');

  useEffect(() => {
    setTagDraft('');
    requestRef.current?.abort();
    requestRef.current = null;
    setAiBusy(false);
    setAiError('');
  }, [activeId]);

  useEffect(() => () => requestRef.current?.abort(), []);

  useEffect(() => {
    if (aiModel.provider && aiModel.model) {
      localStorage.setItem('mcb-prompt-ai-model', JSON.stringify(aiModel));
    }
  }, [aiModel.provider, aiModel.model, aiModel.effort]);

  // The files join the prompt they were added to, as it is once they are read.
  async function addFiles(files) {
    const id = activeId;
    checkAttachmentLimits(active.attachments || [], [...files]);
    const read = await Promise.all([...files].map(readUpload));
    onUpdate(id, (prompt) => ({ attachments: [...(prompt.attachments || []), ...read] }));
  }

  function onPaste(evt) {
    try {
      const message = readMessageClipboard(evt.clipboardData);
      if (message) {
        evt.preventDefault();
        checkAttachmentLimits(active.attachments || [], message.attachments);
        onUpdate(activeId, {
          text: insertClipboardText(evt.currentTarget, message.text),
          attachments: [...(active.attachments || []), ...message.attachments],
        });
        return;
      }
    } catch (error) {
      evt.preventDefault();
      onError(error);
      return;
    }
    const files = [...(evt.clipboardData?.items || [])]
      .filter((item) => item.kind === 'file')
      .map((item) => item.getAsFile())
      .filter(Boolean);
    if (files.length === 0) return;
    evt.preventDefault();
    addFiles(files).catch(onError);
  }

  // "text": improve what's in the editor. "note": write the prompt from a
  // short description typed into a small box, leaving the editor's text out.
  const [aiSource, setAiSource] = useState(() => (localStorage.getItem('mcb-improve-source') === 'note' ? 'note' : 'text'));
  const [aiMenu, setAiMenu] = useState(false);
  const [noteOpen, setNoteOpen] = useState(false);
  const [note, setNote] = useState('');
  const aiRef = useRef(null);
  useEffect(() => { localStorage.setItem('mcb-improve-source', aiSource); }, [aiSource]);
  useEffect(() => {
    if (!aiMenu && !noteOpen) return undefined;
    const close = (evt) => {
      if (evt.key === 'Escape' || (evt.type === 'mousedown' && !aiRef.current?.contains(evt.target))) { setAiMenu(false); setNoteOpen(false); }
    };
    document.addEventListener('mousedown', close);
    document.addEventListener('keydown', close);
    return () => { document.removeEventListener('mousedown', close); document.removeEventListener('keydown', close); };
  }, [aiMenu, noteOpen]);

  async function improveWithAi(sourceText) {
    const source = activeRef.current;
    if (!source || !aiModel.provider || !aiModel.model || aiBusy || !sourceText.trim()) return;
    const controller = new AbortController();
    requestRef.current?.abort();
    requestRef.current = controller;
    setAiBusy(true);
    setAiError('');
    setNoteOpen(false);
    try {
      const result = await api('/api/prompts/generate', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        signal: controller.signal,
        body: JSON.stringify({ provider: aiModel.provider, model: aiModel.model, effort: effortFor(aiCaps, aiModel.effort), text: sourceText }),
      });
      if (controller.signal.aborted) return;
      if (activeRef.current?.id !== source.id || activeRef.current.text !== source.text) {
        setAiError('The prompt changed while the model was writing. Try again.');
        return;
      }
      onUpdate(source.id, { text: result.text });
      setNote('');
    } catch (err) {
      if (!controller.signal.aborted) setAiError(String(err.message || err));
    } finally {
      if (requestRef.current === controller) {
        requestRef.current = null;
        setAiBusy(false);
      }
    }
  }

  const modelConnected = Boolean(providers[aiModel.provider]?.connected);
  const aiReady = Boolean(aiModel.provider && aiModel.model && modelConnected) && !aiBusy;

  return html`
    <div class="prompt-editor">
        ${!active && html`
          <section class="pe-main pe-blank">
            <div class="pe-empty-state">
              <${AppMark} size=${44} />
              <h2>No prompts yet</h2>
              <p>Save build requests you reuse. Type <kbd>/</kbd> in a message to insert one, and write <code>{{style}}</code> for parts you fill in each time.</p>
              <button class="btn gold" onClick=${onCreate}><${Icon} name="plus" size="14" /> New prompt</button>
            </div>
          </section>
        `}
        ${active && html`<section class="pe-main ${active.addedBy ? 'added-by' : ''}">
          <input
            class="pe-title"
            type="text"
            value=${active.title || ''}
            placeholder="Untitled prompt"
            onInput=${(evt) => onUpdate(activeId, { title: evt.target.value })}
          />
          ${active.addedBy && html`<div class="pe-added-by"><${AddedByMark} addedBy=${active.addedBy} /></div>`}
          <div class="pe-tags">
            ${(active.tags || []).map((tag) => html`
              <span class="pe-tag" style=${`--tag:${tagColor(tag)}`}>
                ${tag}
                <button type="button" aria-label=${`Remove tag ${tag}`} onClick=${() => onUpdate(activeId, { tags: active.tags.filter((t) => t !== tag) })}>
                  <${Icon} name="x" size="10" />
                </button>
              </span>
            `)}
            <input
              id="prompt-tag-input"
              list="prompt-tag-suggestions"
              value=${tagDraft}
              placeholder=${active.tags?.length ? '' : 'Add tag'}
              onInput=${(evt) => {
                if (evt.target.value.includes(',')) addTag(evt.target.value.replace(',', ''));
                else setTagDraft(evt.target.value);
              }}
              onKeyDown=${(evt) => {
                if (evt.key === 'Enter') { evt.preventDefault(); addTag(tagDraft); }
                if (evt.key === 'Backspace' && !tagDraft && active.tags?.length) onUpdate(activeId, { tags: active.tags.slice(0, -1) });
              }}
              onBlur=${() => addTag(tagDraft)}
            />
            <datalist id="prompt-tag-suggestions">${knownTags.map((tag) => html`<option value=${tag} />`)}</datalist>
          </div>
          <textarea
            class="pe-body"
            value=${active.text || ''}
            placeholder="Describe the build. Write {{like this}} for parts to fill in when you insert it."
            onInput=${(evt) => onUpdate(activeId, { text: evt.target.value })}
            onPaste=${onPaste}
          ></textarea>
          <div class="pe-foot">
            <button class="attach" title="Attach file" aria-label="Attach file" onClick=${() => fileRef.current?.click()}>
              <${Icon} name="plus" size="16" />
            </button>
            <input
              ref=${fileRef}
              type="file"
              multiple
              style="display:none"
              onChange=${(evt) => {
                addFiles([...evt.target.files]).catch(onError);
                evt.target.value = '';
              }}
            />
            <div class="pe-attachments">
              ${(active.attachments || []).map((file, index) => html`
                <div class="composer-attachment">
                  ${file.type.startsWith('image/')
                    ? html`<img src=${file.data} alt="" />`
                    : html`<span class="file-tile"><${Icon} name="file" size="16" /></span>`}
                  <span class="attachment-copy">
                    <b title=${file.name}>${file.name}</b>
                    <small>${file.type.split('/').pop() || 'file'}</small>
                  </span>
                  <button
                    title="Remove attachment"
                    aria-label="Remove attachment"
                    onClick=${() => onUpdate(activeId, {
                      attachments: active.attachments.filter((_, i) => i !== index),
                    })}
                  ><${Icon} name="x" size="12" /></button>
                </div>
              `)}
            </div>
            <div class="pe-ai-controls">
              ${aiError && html`<span class="pe-ai-error" title=${aiError}>${aiError}</span>`}
              <div class="pe-ai-model">
                <${ModelPicker}
                  providers=${providers}
                  value=${aiModel}
                  onChange=${(selection) => setAiModel({ ...selection, effort: '' })}
                  favorites=${favorites}
                  onToggleFavorite=${onToggleFavorite}
                  onSetup=${onSetup}
                  label="Generation model"
                />
              </div>
              ${aiCaps?.reasoning && aiCaps.efforts.length > 0 && html`
                <${EffortPicker}
                  caps=${aiCaps}
                  value=${aiModel.effort}
                  onChange=${(effort) => setAiModel((selection) => ({ ...selection, effort }))}
                  label="Generation reasoning effort"
                />
              `}
              <div class="pe-ai-split" ref=${aiRef}>
                <button
                  type="button"
                  class="pe-ai-generate"
                  disabled=${!aiReady || (aiSource === 'text' && !active.text?.trim())}
                  data-tip=${aiSource === 'text' ? 'Rewrites the text above' : 'Writes the prompt from a short description'}
                  onClick=${() => (aiSource === 'text' ? improveWithAi(active.text || '') : setNoteOpen((open) => !open))}
                >
                  <${Icon} name="sparkles" size="14" />
                  ${aiBusy ? 'Improving…' : aiSource === 'text' ? 'Improve with AI' : 'Write with AI'}
                </button>
                <button type="button" class="pe-ai-mode" aria-label="Choose what the AI works from" onClick=${() => setAiMenu((open) => !open)}>
                  <${Icon} name="chevron-down" size="12" />
                </button>
                ${aiMenu && html`
                  <div class="pe-ai-menu" role="menu">
                    ${[['text', 'Improve the text', 'Rewrites what is in the editor'], ['note', 'Write from a note', 'Opens a box for a short description']].map(([value, label, hint]) => html`
                      <button type="button" role="menuitemradio" aria-checked=${aiSource === value} class=${aiSource === value ? 'selected' : ''}
                        onClick=${() => { setAiSource(value); setAiMenu(false); }}>
                        <span><b>${label}</b><small>${hint}</small></span>
                        ${aiSource === value && html`<${Icon} name="check" size="14" />`}
                      </button>
                    `)}
                  </div>
                `}
                ${noteOpen && html`
                  <form class="pe-ai-note" onSubmit=${(evt) => { evt.preventDefault(); improveWithAi(note); }}>
                    <textarea
                      id="improve-note"
                      autofocus
                      rows="3"
                      value=${note}
                      placeholder="e.g. a lighthouse on a cliff, stormy"
                      onInput=${(evt) => setNote(evt.target.value)}
                      onKeyDown=${(evt) => { if (evt.key === 'Enter' && !evt.shiftKey && !evt.isComposing) { evt.preventDefault(); improveWithAi(note); } }}
                    ></textarea>
                    <button type="submit" class="btn gold" disabled=${!aiReady || !note.trim()}>Write</button>
                  </form>
                `}
              </div>
            </div>
          </div>
        </section>`}
    </div>
  `;
}

// A fold whose body exists only while it is open: a long transcript's closed
// folds (tool code, outputs, reasoning) cost no DOM until someone looks.
function Fold({ className, summaryClass, title, summary, body }) {
  const [open, setOpen] = useState(false);
  return html`
    <details class=${className} open=${open} onToggle=${(evt) => setOpen(evt.currentTarget.open)}>
      <summary class=${summaryClass} title=${title}>${summary}</summary>
      ${open && body()}
    </details>
  `;
}

// A keyed slot for a transcript entry's cached vnode (see entryKeys).
function Keyed({ children }) {
  return children;
}

// A box picked in the viewer, as a chip: hovering it lights its box up, and a
// chip that goes away while hovered (sent, removed, another chat's) puts the
// light out.
function SelectionChip({ selection, draft = false, onHover, onRemove }) {
  const hoveredRef = useRef(false);
  useEffect(() => () => { if (hoveredRef.current) onHover(null); }, []);
  const size = selection.min.map((value, axis) => selection.max[axis] - value + 1).join('×');
  return html`
    <span
      class="selection-chip"
      title=${`${selection.blocks} blocks`}
      onMouseEnter=${() => { hoveredRef.current = true; onHover({ ...selection, draft }); }}
      onMouseLeave=${() => { hoveredRef.current = false; onHover(null); }}
      onContextMenu=${draft ? (evt) => { evt.preventDefault(); onRemove(selection.letter); } : null}
    >
      ${selection.block
        ? html`<img class="selection-icon" src=${blockIconUrl(selection.block)} alt="" />`
        : html`<span class="selection-icon empty"></span>`}
      <span class="selection-label">${selection.label}</span>
      <span class="selection-size">${size}</span>
      ${draft && html`<button
        type="button"
        aria-label=${`Remove ${selection.label}`}
        onClick=${() => onRemove(selection.letter)}
      ><${Icon} name="x" size="11" /></button>`}
    </span>
  `;
}

// The composer's editable surface. Preact never renders inside it: the
// element is built once, rewritten when `value.version` changes, and reports
// typing through onChange (see composer.mjs for what it holds).
function ComposerInput({ inputRef, value, placeholder, onChange, onKeyDown, onPaste, onCaret }) {
  const hostRef = useRef(null);
  const editorRef = useRef(null);
  const handlers = useRef(null);
  handlers.current = { onChange, onKeyDown, onPaste, onCaret };
  const [empty, setEmpty] = useState(true);
  const blank = (segments) => !segments.some((segment) => segment.type !== 'text' || segment.text.trim());

  // A layout effect, so the editor exists before the rewrite below first runs.
  useLayoutEffect(() => {
    const editor = document.createElement('div');
    editor.className = 'composer-input';
    editor.contentEditable = 'true';
    editor.spellcheck = true;
    editor.setAttribute('role', 'textbox');
    editor.setAttribute('aria-multiline', 'true');
    const report = () => {
      const state = composer.read(editor);
      setEmpty(blank(state.segments));
      handlers.current.onChange(state);
    };
    const track = () => {
      const range = composer.caretRange(editor);
      if (range) handlers.current.onCaret(range);
    };
    editor.addEventListener('input', report);
    editor.addEventListener('keydown', (evt) => handlers.current.onKeyDown(evt));
    editor.addEventListener('paste', (evt) => handlers.current.onPaste(evt));
    editor.addEventListener('click', (evt) => {
      const remove = evt.target.closest('.ce-remove');
      if (!remove) return;
      remove.closest('.ce-prompt').remove();
      editor.focus();
      report();
    });
    document.addEventListener('selectionchange', track);
    hostRef.current.append(editor);
    editorRef.current = editor;
    inputRef.current = editor;
    return () => {
      document.removeEventListener('selectionchange', track);
      editor.remove();
    };
  }, []);

  // Before the next event: a key pressed right after a rewrite reads the new content.
  useLayoutEffect(() => {
    composer.write(editorRef.current, value.segments);
    setEmpty(blank(value.segments));
  }, [value.version]);

  useEffect(() => { editorRef.current.dataset.placeholder = placeholder; }, [placeholder]);

  return html`<div class="composer-input-host ${empty ? 'empty' : ''}" ref=${hostRef}></div>`;
}

// Key binds: each action has one combination, "Mod" being Cmd on a Mac and
// Ctrl elsewhere. A combination needs a modifier (or is an F key), so typing
// never triggers one.
const MAC = /Mac|iPhone|iPad/.test(navigator.platform);
const BIND_ACTIONS = [
  ['build', 'Switch to Build', 'Mod+1'],
  ['bench', 'Switch to Bench', 'Mod+2'],
  ['search', 'Search everything', 'Mod+K'],
  ['new', 'New chat or bench', 'Mod+Shift+O'],
  ['sidebar', 'Show or hide the sidebar', 'Mod+B'],
  ['select', 'Select blocks in the viewer', 'Alt+S'],
];
const DEFAULT_BINDS = Object.fromEntries(BIND_ACTIONS.map(([id, , combo]) => [id, combo]));

// The combination a key press makes, or null for a bare modifier or a key
// without one.
function comboOf(evt) {
  if (['Meta', 'Control', 'Alt', 'Shift'].includes(evt.key)) return null;
  const key = /^Key[A-Z]$/.test(evt.code) ? evt.code.slice(3)
    : /^Digit\d$/.test(evt.code) ? evt.code.slice(5)
      : evt.key.length === 1 ? evt.key.toUpperCase() : evt.key;
  const mod = MAC ? evt.metaKey : evt.ctrlKey;
  if (!mod && !evt.altKey && !(MAC && evt.ctrlKey) && !/^F\d{1,2}$/.test(key)) return null;
  return [mod && 'Mod', MAC && evt.ctrlKey && 'Ctrl', evt.altKey && 'Alt', evt.shiftKey && 'Shift', key].filter(Boolean).join('+');
}

function formatBind(combo) {
  if (!combo) return 'Not set';
  const names = MAC ? { Mod: '⌘', Ctrl: '⌃', Alt: '⌥', Shift: '⇧' } : { Mod: 'Ctrl', Alt: 'Alt', Shift: 'Shift' };
  return combo.split('+').map((part) => names[part] || part).join(MAC ? '' : '+');
}

// Settings rows for the binds. Clicking a key records the next combination;
// Escape or a click elsewhere cancels and keeps what was there.
function BindsSection({ binds, onChange, onReset }) {
  const [recording, setRecordingState] = useState(null);
  // Listening from mount, not from the click's render, so even a key pressed
  // right after the click is recorded rather than run.
  const recordingRef = useRef(null);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const setRecording = (id) => { recordingRef.current = id; setRecordingState(id); };
  useEffect(() => {
    const onKey = (evt) => {
      const id = recordingRef.current;
      if (!id) return;
      evt.preventDefault();
      evt.stopPropagation();
      if (evt.key === 'Escape') return setRecording(null);
      const combo = comboOf(evt);
      if (!combo) return;
      onChangeRef.current(id, combo);
      setRecording(null);
    };
    const onDown = (evt) => { if (recordingRef.current && !evt.target.closest('.bind-key.recording')) setRecording(null); };
    window.addEventListener('keydown', onKey, true);
    document.addEventListener('mousedown', onDown);
    return () => {
      window.removeEventListener('keydown', onKey, true);
      document.removeEventListener('mousedown', onDown);
    };
  }, []);
  const changed = BIND_ACTIONS.some(([id, , combo]) => binds[id] !== combo);
  return html`
    <${SettingsSection}
      title="Shortcuts"
      action=${changed && html`<button type="button" class="s-section-action" onClick=${() => { setRecording(null); onReset(); }}>Reset all</button>`}
    >
      ${BIND_ACTIONS.map(([id, label]) => html`
        <${SettingsRow} title=${label}>
          <button
            type="button"
            class="bind-key ${recording === id ? 'recording' : ''} ${binds[id] ? '' : 'unset'}"
            title=${recording === id ? 'Press a shortcut, or Escape to cancel' : 'Click to change'}
            onClick=${() => setRecording(recording === id ? null : id)}
          >${recording === id ? 'Press a shortcut' : formatBind(binds[id])}</button>
        <//>
      `)}
    <//>
  `;
}

// The wordmark is the switch: mcbench or mcbuild. Its second half swaps the
// workspace and flashes white as it changes.
function ModeSwitcher({ mode, binds, dot, waiting, onChange }) {
  const [swapped, setSwapped] = useState(false);
  const other = mode === 'build' ? 'bench' : 'build';
  return html`
    <div class="wordmark mode-switch">
      <span>mc</span><button
        type="button"
        key=${mode}
        class="mode-word ${swapped ? 'swapped' : ''}"
        title=${`Switch to ${other[0].toUpperCase()}${other.slice(1)}${waiting ? ` · ${waiting} finished` : dot ? ' · working' : ''}${binds[other] ? ` (${formatBind(binds[other])})` : ''}`}
        onClick=${() => { setSwapped(true); onChange(other); }}
      >${mode}<span class="mode-swap"><${Icon} name="swap" size="12" />${dot}</span></button>
    </div>
  `;
}

function Elapsed({ since }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [since]);
  return html`<span class="elapsed">${formatElapsed(now - since)}</span>`;
}

// One model chosen for a new bench. Its name removes it (the logo turns into
// an x on hover); its effort opens a menu that sets the effort or, with the
// plus on an effort it lacks, adds the same model again at that effort.
function BenchModelChip({ entry, taken, onEffort, onCopy, onRemove }) {
  const caps = useModelInfo(entry.provider, entry.model);
  const { open, setOpen, closing, closePicker, rootRef } = usePopover(110);
  const name = modelLabel(entry.model);
  const effort = entry.effort || caps?.defaultEffort || '';
  return html`
    <span class="bench-chip" ref=${rootRef}>
      <button type="button" class="bench-chip-model" title="Remove" aria-label=${`Remove ${name}`} onClick=${onRemove}>
        <span class="bench-chip-icon">
          <${Logo} name=${vendorIcon(entry.provider, entry.model)} size="13" />
          <${Icon} name="x" size="12" />
        </span>
        <span class="bench-chip-name" title=${entry.model}>${name}</span>
      </button>
      ${caps?.reasoning && caps.efforts.length > 0 && html`
        <button type="button" class="bench-chip-effort ${open ? 'open' : ''}" aria-haspopup="menu" aria-expanded=${open}
          onClick=${() => (open ? closePicker() : setOpen(true))}>
          <span>${effort}</span><${Icon} name="chevron-down" size="10" />
        </button>
        ${open && html`
          <div class="effort-picker-menu bench-effort-menu ${closing ? 'closing' : ''}" role="menu" aria-label=${`${name} reasoning effort`}>
            ${caps.efforts.map((level) => html`
              <div class="bench-effort-row ${level === effort ? 'selected' : ''}">
                <button type="button" role="menuitemradio" aria-checked=${level === effort} onClick=${() => { onEffort(level); closePicker(); }}>
                  <span class="effort-option-label">${level}</span>
                  ${level === effort && html`<${Icon} name="check" size="14" />`}
                </button>
                ${!taken.map((used) => used || caps.defaultEffort).includes(level) && html`
                  <button type="button" class="bench-effort-add" title=${`Also run ${name} at ${level}`} aria-label=${`Also run ${name} at ${level}`}
                    onClick=${() => { onCopy(level, effort); closePicker(); }}>
                    <${Icon} name="plus" size="13" />
                  </button>
                `}
              </div>
            `)}
          </div>
        `}
      `}
    </span>
  `;
}

// A bench chat's name: its model, and its effort when the bench runs that
// model more than once.
function memberName(row, rows) {
  const twin = rows.some((other) => other !== row && other.benchId === row.benchId && other.provider === row.provider && other.model === row.model);
  return twin ? `${modelLabel(row.model)} · ${row.effort || 'default'}` : modelLabel(row.model);
}

// Where one model of a bench stands on a round (1-based). `round` on the
// member is how many rounds it was sent; a failed or stopped turn holds the rest.
function roundState(member, round) {
  if (round < member.round) return 'done';
  if (round === member.round) return member.runningSince ? 'running' : member.halted || 'done';
  return member.halted ? 'held' : 'queued';
}

// One model on one round: where it stands, its effort, time and tokens, and
// the way into its chat. The status slot shows where it stands (retry after a
// failure or stop); a working model has a Stop button where the vote goes.
// A blind bench hides which model is which until it is revealed; each model
// goes by its letter, and the letters run in a random order.
const hidesNames = (bench) => bench.blind && !bench.revealed;
const benchLetter = (bench, memberId) => String.fromCharCode(65 + bench.order.indexOf(memberId));

// An action that cannot be undone: the first click asks, the second acts.
// `subject` names what it acts on (a bench, a bench chat). The button is
// armed for the subject it was clicked on: shown for another, it asks again.
// An archived chat or bench in the sidebar: its title, the time it has left
// and two icon buttons, restore and delete. Delete asks first: one click arms
// it for four seconds, which the row says in place of the time left, and a
// second click deletes.
function ArchivedRow({ title, meta, onRestore, onDelete }) {
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    if (!armed) return undefined;
    const timer = setTimeout(() => setArmed(false), 4000);
    return () => clearTimeout(timer);
  }, [armed]);
  return html`
    <div class="archived-row">
      <span class="title" title=${title}>${title}</span>
      <span class="archived-meta ${armed ? 'armed' : ''}" role=${armed ? 'status' : null}>${armed ? 'Delete for good?' : meta}</span>
      <button type="button" class="row-icon archived-restore" title="Restore to the list and open it" aria-label="Restore" onClick=${onRestore}>
        <${Icon} name="restart" size="14" />
      </button>
      <button
        type="button"
        class="row-icon archived-delete ${armed ? 'armed' : ''}"
        title=${armed ? 'Click again to delete it for good' : 'Delete permanently: this cannot be undone'}
        aria-label=${armed ? 'Delete for good' : 'Delete permanently'}
        onClick=${() => { if (!armed) return setArmed(true); setArmed(false); onDelete(); }}
      ><${Icon} name="trash" size="14" /></button>
    </div>
  `;
}

function ConfirmButton({ subject, icon, label, confirmLabel, title, disabled = false, onConfirm }) {
  const [armedFor, setArmedFor] = useState(null);
  const armed = armedFor === subject;
  useEffect(() => {
    if (armedFor === null) return undefined;
    const timer = setTimeout(() => setArmedFor(null), 4000);
    return () => clearTimeout(timer);
  }, [armedFor]);
  return html`
    <button
      type="button"
      class="header-action quiet ${armed ? 'armed' : ''}"
      title=${title}
      disabled=${disabled}
      onClick=${() => { if (!armed) return setArmedFor(subject); setArmedFor(null); onConfirm(); }}
    ><${Icon} name=${icon} size="14" /> ${armed ? confirmLabel : label}</button>
  `;
}

// Settings' published builds (published.mjs), under the repository field:
// each with its page, its link to copy, its chat or bench, and a two-click
// Remove.
function PublishedBuilds({ published, onRetry, onRemove, onGoTo, onError }) {
  const [copied, copy] = useCopy(onError);
  const view = publishedView(published);
  let body;
  if (view === 'loading') {
    body = html`<p class="published-note" role="status">Loading published builds…</p>`;
  } else if (view === 'failed') {
    body = html`
      <p class="published-note published-error" role="alert">
        <span>${published.error}</span>
        <button type="button" class="btn" onClick=${onRetry}>Retry</button>
      </p>
    `;
  } else if (view === 'no-repo') {
    body = html`<p class="published-note">Set a repository to publish builds.</p>`;
  } else if (view === 'empty') {
    body = html`<p class="published-note">No builds are published.</p>`;
  } else {
    const { repo, builds } = published.data;
    const now = Date.now();
    body = html`
      <p class="published-note published-count">
        ${builds.length} ${builds.length === 1 ? 'build' : 'builds'} at <a href=${`https://github.com/${repo}`} target="_blank" rel="noopener">${repo}</a>
      </p>
      <ul class="published-list">
        ${builds.map((build) => {
          const removing = published.removing.includes(build.shareId);
          const error = published.errors[build.shareId];
          const bench = build.kind === 'bench';
          const subject = publishedSubject(build);
          return html`
            <li class="published-row ${bench ? 'bench' : 'chat'} ${removing ? 'removing' : ''}" key=${build.shareId}>
              <div class="published-text">
                <span class="published-heading">
                  ${bench && html`<span class="published-tag">Bench</span>`}
                  <span class="published-title" title=${build.title}>${build.title}</span>
                </span>
                <span class="published-meta">
                  ${bench
                    ? html`<span title=${build.labels.join(', ')}>${build.labels.length} ${build.labels.length === 1 ? 'model' : 'models'}</span>`
                    : build.labels[0]} · published <span title=${new Date(build.publishedAt).toLocaleString()}>${publishedAgo(build.publishedAt, now)}</span>${subject === null ? ` · ${build.kind} deleted` : build.archived ? ' · archived' : ''}
                </span>
                ${error && html`<span class="published-row-error" role="alert">${error}</span>`}
              </div>
              <div class="published-actions">
                <a class="header-action quiet published-open" href=${build.url} target="_blank" rel="noopener"><${Icon} name="open" size="14" /> Open</a>
                <button type="button" class="header-action quiet published-copy" onClick=${() => copy(build.shareId, build.url)}>
                  <${Icon} name="copy" size="14" /> ${copied === build.shareId ? 'Copied' : 'Copy link'}
                </button>
                ${subject !== null && !build.archived && html`
                  <button type="button" class="header-action quiet ${bench ? 'published-bench' : 'published-chat'}" onClick=${() => onGoTo(subject)}>
                    <${Icon} name="forward" size="14" /> ${bench ? 'Go to bench' : 'Go to chat'}
                  </button>
                `}
                <span class="published-remove">
                  <${ConfirmButton}
                    subject=${`published:${build.shareId}`}
                    icon="x"
                    label=${removing ? 'Removing…' : 'Remove'}
                    confirmLabel="Remove from the web?"
                    title="Take the public page down"
                    disabled=${removing}
                    onConfirm=${() => onRemove(build)}
                  />
                </span>
              </div>
            </li>
          `;
        })}
      </ul>
    `;
  }
  return html`<div class="published" id="published-builds">${body}</div>`;
}

// Copies `text` to the clipboard. `copied` is the key of the copy that
// landed, for 1.5 seconds, so its button can say "Copied".
function useCopy(onError) {
  const [copied, setCopied] = useState(null);
  const copy = (key, text) => navigator.clipboard.writeText(text).then(() => {
    setCopied(key);
    setTimeout(() => setCopied((current) => (current === key ? null : current)), 1500);
  }, onError);
  return [copied, copy];
}

// The Share panel, under its button in the header: what the share
// (share.mjs) of `subject`, a chat or a bench, is, and what can be done with
// it. `repo` is the saved shareRepo, null when none is set, or undefined
// until read. `hiddenNames` is set for a bench that hides its models' names,
// which its page then shows as letters. A press outside the panel and its
// button closes it.
function SharePanel({ subject, hiddenNames, share, repo, onPublish, onRemove, onSettings, onClose, onError }) {
  const noun = subject.kind === 'bench' ? 'bench' : 'build';
  const panelRef = useRef(null);
  const [copied, copy] = useCopy(onError);
  useEffect(() => {
    const onDown = (evt) => { if (!panelRef.current.parentElement.contains(evt.target)) onClose(); };
    document.addEventListener('pointerdown', onDown);
    return () => document.removeEventListener('pointerdown', onDown);
  }, [onClose]);
  const blindNote = html`<p class="share-text muted share-blind-note">Names are hidden: the page shows the models as A, B, C. Publish again after revealing to show their names.</p>`;
  let body;
  if (share.phase === 'unknown' || share.phase === 'loading' || (share.phase === 'none' && repo === undefined)) {
    body = html`<p class="share-text muted">Checking…</p>`;
  } else if (share.phase === 'none' && repo === null) {
    body = html`
      <p class="share-text">Set the GitHub repository for shared builds in Settings.</p>
      <div class="share-actions"><button type="button" class="btn" onClick=${onSettings}>Open Settings</button></div>
    `;
  } else if (share.phase === 'none') {
    body = html`
      <p class="share-text">${subject.kind === 'bench'
        ? `Publishes this bench's builds to a public page on ${repo}, where anyone can switch between the models' builds. To post on X.`
        : `Publishes this build's 3D view to a public page on ${repo}, to post on X.`}</p>
      ${hiddenNames && blindNote}
      <div class="share-actions"><button type="button" class="btn gold" onClick=${onPublish}>Publish</button></div>
    `;
  } else if (sharePublishingNow(share)) {
    body = html`<p class="share-text share-step" role="status">${shareStepText(share)}</p>`;
  } else if (share.phase === 'failed') {
    body = html`
      <p class="share-text share-error" role="alert">${share.error}</p>
      <div class="share-actions"><button type="button" class="btn" onClick=${onPublish}>Try again</button></div>
    `;
  } else {
    // shared, or being removed
    const removing = share.phase === 'removing';
    body = html`
      <input class="share-url" readonly value=${share.url} aria-label="Shared page link" onFocus=${(evt) => evt.target.select()} />
      <div class="share-actions">
        <button type="button" class="btn" disabled=${removing} onClick=${() => copy(share.url, share.url)}>${copied === share.url ? 'Copied' : 'Copy link'}</button>
        <a class="btn" href=${postUrl(share.url)} target="_blank" rel="noopener">Post on X</a>
      </div>
      <p class="share-text muted">The page shows the ${noun} as it was when published.</p>
      ${hiddenNames && blindNote}
      <div class="share-actions">
        <button type="button" class="btn" disabled=${removing} onClick=${onPublish}>Publish again</button>
        <${ConfirmButton}
          subject=${`unshare:${shareKey(subject)}`}
          icon="x"
          label=${removing ? 'Removing…' : 'Remove'}
          confirmLabel="Remove the page"
          title="Take the public page down"
          disabled=${removing}
          onConfirm=${onRemove}
        />
      </div>
    `;
  }
  return html`<div class="share-panel" ref=${panelRef} role="dialog" aria-label=${`Share this ${noun}`}>${body}</div>`;
}

// `letter` is set while the bench hides names: the row then shows only it,
// with no logo, effort, time or tokens, which would give the model away.
// `onVote` is set once the round can be voted on: picking this build as the
// best, or taking that pick back.
function BenchResultRow({ member, letter, picked, onVote, state, stats, onFocus, onOpen, onStop, onRetry }) {
  const caps = useModelInfo(member.provider, member.model);
  // A stop or retry on its way: its button waits for the answer.
  const [pending, setPending] = useState(false);
  const request = (handler) => (evt) => {
    evt.stopPropagation();
    setPending(true);
    handler().finally(() => setPending(false));
  };
  const effort = letter ? '' : member.effort || caps?.defaultEffort || '';
  const tokens = !letter && stats && (stats.inputTokens ?? 0) + (stats.outputTokens ?? 0);
  const finished = !letter && stats?.ms != null && state !== 'queued' && state !== 'held';
  const name = letter ? `Build ${letter}` : modelLabel(member.model);
  const act = (handler) => (evt) => { evt.stopPropagation(); handler(); };
  let status;
  if (state === 'done') status = html`<span class="bench-status" role="img" aria-label="Done"><${Icon} name="check" size="14" /></span>`;
  else if (state === 'running') {
    status = html`<span class="bench-status" role="img" aria-label="Working"><span class="conv-dot running bench-working"></span></span>`;
  } else if (state === 'failed' || state === 'stopped') {
    status = html`<button type="button" class="bench-status" title=${`${state === 'failed' ? 'Failed' : 'Stopped'}. Retry this round`} aria-label=${`Retry ${name}`} disabled=${pending} onClick=${request(onRetry)}>
      <${Icon} name="refresh" size="13" />
    </button>`;
  } else status = html`<span class="bench-status" role="img" aria-label=${state === 'held' ? 'Waiting for a retry' : 'Queued'}><span class="bench-waiting"></span></span>`;
  return html`
    <div role="listitem" class="bench-result ${state} ${picked ? 'picked' : ''}" onClick=${onFocus}>
      ${status}
      ${letter ? html`<span class="bench-letter">${letter}</span>` : html`<${Logo} name=${vendorIcon(member.provider, member.model)} size="14" />`}
      <span class="bench-model" title=${letter ? '' : member.model}>${name}${effort && html`<span class="bench-effort">${effort}</span>`}</span>
      <span class="bench-stat" title=${tokens ? `${formatTokens(stats.inputTokens ?? 0)} in · ${formatTokens(stats.outputTokens ?? 0)} out` : ''}>
        ${tokens && state !== 'running' ? `${formatTokens(tokens)} tok` : ''}
      </span>
      <span class="bench-stat">${state === 'running' && !letter ? html`<${Elapsed} since=${member.runningSince} />` : finished ? formatElapsed(stats.ms) : ''}</span>
      ${onVote ? html`
        <button
          type="button"
          class="bench-vote ${picked ? 'picked' : ''}"
          title=${picked ? 'Your pick for this round. Click to take it back' : 'Pick as the best build'}
          aria-label=${picked ? `Take back the pick of ${name}` : `Pick ${name} as the best build`}
          aria-pressed=${picked}
          onClick=${act(onVote)}
        ><${Icon} name="trophy" size="13" />${picked && html`<span>Best</span>`}</button>
      ` : state === 'running' ? html`
        <button type="button" class="bench-vote bench-stop" title=${`Stop ${name}`} aria-label=${`Stop ${name}`} disabled=${pending} onClick=${request(onStop)}><${Icon} name="stop" size="18" /></button>
      ` : picked ? html`
        <span class="bench-vote picked bench-mark" role="img" aria-label=${`Your blind pick for this round: ${name}`}><${Icon} name="trophy" size="13" /><span>Best</span></span>
      ` : html`<span></span>`}
      <button class="bench-open" title="Open chat" aria-label=${`Open ${name}'s chat`} onClick=${act(onOpen)}><${Icon} name="open" size="14" /></button>
    </div>
  `;
}

function memberDot(member, activeId) {
  if (member.runningSince) return 'running';
  if (member.halted) return 'halted';
  return member.unread && member.id !== activeId ? 'unread' : 'idle';
}

// The texture a selection's main block breaks into, or null for empty space.
const blockIconUrl = (block) => (block ? `/api/blocks/${block.slice('minecraft:'.length)}/icon.png` : null);

// A view the viewer can show: the world Minecraft saved, or, while it is
// still preparing it, a preview (the build's snapshot, or the build or an
// edit placed by the engine). The viewer meshes its blueprint itself.
const viewable = (live) => live.viewerRevision != null && live.mesher != null;

function BlueMapViewer({ live, world, exportName, onError, activeId, labels, focus, emptyText, selections, onSelect, onDeselect, binds, onBind, selectToggle, typeAhead, onType, full, onFull, onEscape }) {
  const frameRef = useRef(null);
  const [paintedFrame, setPaintedFrame] = useState(null);
  // Why the iframe could not show a view: { key, text }, for that key only.
  const [viewerError, setViewerError] = useState(null);
  // Why the iframe's viewer could not start (bluemap-bridge.js, its boot): it
  // shows nothing until the iframe is loaded again.
  const [bootError, setBootError] = useState(null);
  const activeRef = useRef(activeId);
  activeRef.current = activeId;
  const liveRef = useRef(live);
  const uiRef = useRef(null);
  const onSelectRef = useRef(onSelect);
  onSelectRef.current = onSelect;
  const onDeselectRef = useRef(onDeselect);
  onDeselectRef.current = onDeselect;
  const onBindRef = useRef(onBind);
  onBindRef.current = onBind;
  const onTypeRef = useRef(onType);
  onTypeRef.current = onType;
  const onFullRef = useRef(onFull);
  onFullRef.current = onFull;
  const onEscapeRef = useRef(onEscape);
  onEscapeRef.current = onEscape;
  const readyRef = useRef(false);
  const worldRef = useRef(world);
  worldRef.current = world;
  const exportNameRef = useRef(exportName);
  exportNameRef.current = exportName;
  const onErrorRef = useRef(onError);
  onErrorRef.current = onError;
  // The key of the base the iframe was last sent a refresh for, while it may
  // still show it: ticks on that base follow it.
  const sentRef = useRef(null);
  const lastKeyRef = useRef(null);
  const lastConversationRef = useRef(null);
  liveRef.current = live;
  const shown = viewable(live) && live.conversationId === activeId;
  const chat = shown && !activeId.startsWith('bench:');
  uiRef.current = {
    type: 'mcbench-lite:ui',
    exportable: shown && live.live != null,
    playable: shown && live.live != null,
    download: chat && live.blockCount > 0 ? `/api/conversations/${activeId}/schematic` : null,
    labels: shown ? labels : [],
    selectable: chat && Boolean(onSelect),
    selections: chat ? selections : [],
    binds: Object.values(binds).filter(Boolean),
    typeAhead,
    full,
    bounds: shown && live.bounds ? { min: [live.bounds.min[0], live.bounds.min[1] + live.yOffset, live.bounds.min[2]], max: [live.bounds.max[0], live.bounds.max[1] + live.yOffset, live.bounds.max[2]] } : null,
  };

  const postPlayer = (message) => frameRef.current?.contentWindow?.postMessage({ type: 'mcbench-lite:player', ...message }, window.location.origin);

  const syncUi = useCallback(() => {
    if (!readyRef.current || !frameRef.current?.contentWindow) return;
    frameRef.current.contentWindow.postMessage(uiRef.current, window.location.origin);
  }, []);

  const refresh = useCallback((force = false) => {
    const current = liveRef.current;
    const frame = frameRef.current;
    if (!viewable(current) || current.conversationId !== activeRef.current) return;
    if (!readyRef.current || !frame?.contentWindow || current.viewerRevision == null) return;
    const key = `${current.conversationId || ''}:${current.viewerRevision}`;
    // Delivery is confirmed by the iframe answering for the key, shown or
    // failed. Until that lands, every status poll re-sends, so a lost
    // refresh can never strand the viewer on stale tiles.
    if (!force && lastKeyRef.current === key) return;
    // Another chat's world: move the camera to it and stream its tiles fresh
    // instead of swapping them in as one atomic edit.
    const focus = lastConversationRef.current !== current.conversationId;
    frame.contentWindow.postMessage({
      type: 'mcbench-lite:refresh',
      key,
      blueprint: current.viewerBlueprint,
      mask: current.viewerMask,
      wasm: current.mesher.wasm,
      pack: current.mesher.pack,
      cameraHash: focus ? current.cameraHash : null,
      fresh: focus,
    }, window.location.origin);
    // What the world did since this base goes first, then each tick as it comes.
    sentRef.current = key;
    const behind = worldRef.current.accumulated();
    if (behind && `${current.conversationId}:${behind.base}` === key) {
      frame.contentWindow.postMessage({ type: 'mcbench-lite:frame', key, patch: behind.patch, entities: behind.entities }, window.location.origin);
    }
  }, []);

  // Every tick of the world goes to the viewer, once it has been told the base it applies to.
  // With the player in it, a tick carries the player's view too.
  useEffect(() => world.onFrame(({ base, patch, entities, view, ticks }) => {
    const target = frameRef.current?.contentWindow;
    if (!target) return;
    // The player's view goes every tick: the camera keeps time by it.
    if (view) target.postMessage({ type: 'mcbench-lite:player', view, tick: ticks }, window.location.origin);
    const key = `${liveRef.current.conversationId}:${base}`;
    if (sentRef.current !== key) return;
    target.postMessage({ type: 'mcbench-lite:frame', key, patch, entities }, window.location.origin);
  }), [world]);

  useEffect(() => {
    const onMessage = (event) => {
      if (event.origin !== window.location.origin || event.source !== frameRef.current?.contentWindow) return;
      if (event.data?.type === 'mcbench-lite:bluemap-ready') {
        readyRef.current = true;
        refresh(true);
        syncUi();
      }
      if (event.data?.type === 'mcbench-lite:refreshed' && typeof event.data.key === 'string') {
        const current = liveRef.current;
        if (!viewable(current) || current.conversationId !== activeRef.current
          || event.data.key !== `${current.conversationId}:${current.viewerRevision}`) return;
        lastKeyRef.current = event.data.key;
        lastConversationRef.current = event.data.key.slice(0, event.data.key.lastIndexOf(':')) || null;
        // The bridge acknowledges after visible tiles have had a frame to
        // render. Keep the loading surface until then, including empty maps.
        setPaintedFrame({ frame: frameRef.current, key: event.data.key });
        setViewerError(null);
      }
      // Player mode (bluemap-bridge.js): the creative player in this page's
      // world. Every spawn is answered once, in order (the bridge counts
      // them); one answered after another chat came on show stood no one in
      // its world, and says so.
      if (event.data?.type === 'mcbench-lite:player-spawn') {
        const conversationId = activeRef.current;
        const { x, y, z, yaw, pitch, flying } = event.data;
        worldRef.current.spawnPlayer({ x, y, z, yaw, pitch, flying }).then(
          (view) => postPlayer(activeRef.current === conversationId ? { view } : { error: 'Another world came on show before the player stood in this one' }),
          (error) => postPlayer({ error: String(error.message || error) }),
        );
      }
      if (event.data?.type === 'mcbench-lite:player-input') {
        const { keys, taps, yaw, pitch } = event.data;
        worldRef.current.setPlayerInput({ keys, taps, yaw, pitch }).catch((error) => onErrorRef.current(error));
      }
      // Every leave reaches the world, a spawn still on its way included.
      if (event.data?.type === 'mcbench-lite:player-leave') {
        worldRef.current.removePlayer().catch((error) => onErrorRef.current(error));
      }
      if (event.data?.type === 'mcbench-lite:interact') {
        const { x, y, z, yaw } = event.data;
        worldRef.current.input(x, y, z, yaw).catch((error) => onErrorRef.current(error));
      }
      if (event.data?.type === 'mcbench-lite:export') {
        worldRef.current.exportWorld(exportNameRef.current).then(({ folder, bytes }) => {
          const link = document.createElement('a');
          link.href = URL.createObjectURL(new Blob([bytes], { type: 'application/zip' }));
          link.download = `${folder}.zip`;
          link.click();
          setTimeout(() => URL.revokeObjectURL(link.href), 10_000);
        }).catch((error) => onErrorRef.current(error));
      }
      if (event.data?.type === 'mcbench-lite:selection' && onSelectRef.current) {
        onSelectRef.current({ min: event.data.min, max: event.data.max });
      }
      if (event.data?.type === 'mcbench-lite:deselect') onDeselectRef.current(event.data.key);
      if (event.data?.type === 'mcbench-lite:bind') onBindRef.current(event.data.combo);
      if (event.data?.type === 'mcbench-lite:type') onTypeRef.current(event.data.key);
      if (event.data?.type === 'mcbench-lite:full') onFullRef.current(event.data.on === true);
      if (event.data?.type === 'mcbench-lite:escape') onEscapeRef.current();
      if (event.data?.type === 'mcbench-lite:boot-failed') setBootError(String(event.data.error));
      // The iframe could not show the view: it answers for the key, so the
      // refresh is not sent again, and says why.
      // No ticks follow a base it could not show.
      if (event.data?.type === 'mcbench-lite:refresh-error') {
        const current = liveRef.current;
        if (current.conversationId === activeRef.current && event.data.key === `${current.conversationId}:${current.viewerRevision}`) {
          lastKeyRef.current = event.data.key;
          if (sentRef.current === event.data.key) sentRef.current = null;
          setViewerError({ key: event.data.key, text: event.data.error });
        }
      }
    };
    window.addEventListener('message', onMessage);
    return () => window.removeEventListener('message', onMessage);
  }, [refresh, syncUi]);

  // The world dropped the player itself (another engine module, a dead
  // worker, a fresh placement that failed): the viewer leaves player mode
  // too. A fresh placement keeps the player standing where it was.
  useEffect(() => world.onPlayerGone(() => postPlayer({ gone: true })), [world]);

  // Unconfirmed refreshes retry once a second until the iframe answers for
  // the key, so a lost one can never strand the viewer on stale tiles.
  useEffect(() => refresh(), [live.viewerRevision, live.conversationId, live.viewerBlueprint, refresh]);
  useEffect(() => {
    const retry = setInterval(() => refresh(), 1000);
    return () => clearInterval(retry);
  }, [refresh]);
  useEffect(() => syncUi(), [
    activeId,
    live.address,
    live.blockCount,
    live.conversationId,
    live.status,
    JSON.stringify(labels),
    JSON.stringify(selections),
    JSON.stringify(live.bounds),
    JSON.stringify(binds),
    typeAhead,
    full,
    Boolean(onSelect),
    syncUi,
  ]);
  // Frames one build in the view.
  useEffect(() => {
    if (focus && readyRef.current) frameRef.current?.contentWindow?.postMessage({ type: 'mcbench-lite:fly', min: focus.min, max: focus.max }, window.location.origin);
  }, [focus]);
  // The select shortcut, pressed anywhere in the app.
  useEffect(() => {
    if (selectToggle && readyRef.current) frameRef.current?.contentWindow?.postMessage({ type: 'mcbench-lite:toggle-select' }, window.location.origin);
  }, [selectToggle]);

  // The server advances `frame` when a finished turn changed what is worth
  // looking at. The first value seen for a world is where it already stands;
  // a later one flies to the whole world once its new version has painted.
  const framedRef = useRef({ id: null, frame: null });
  useEffect(() => {
    if (!readyRef.current || live.status !== 'running' || live.conversationId !== activeId
      || paintedFrame?.key !== `${live.conversationId}:${live.viewerRevision}`) return;
    const framed = framedRef.current;
    framedRef.current = { id: live.conversationId, frame: live.frame };
    if (framed.id !== live.conversationId || framed.frame === live.frame || !live.bounds) return;
    const lift = (corner) => [corner[0], corner[1] + live.yOffset, corner[2]];
    frameRef.current.contentWindow.postMessage({ type: 'mcbench-lite:fly', min: lift(live.bounds.min), max: lift(live.bounds.max) }, window.location.origin);
  }, [live, activeId, paintedFrame]);

  // One BlueMap document serves every chat: once loaded it stays, behind the
  // loading surface while another chat's world is prepared, and only its
  // tiles and camera change. Rebooting BlueMap per chat cost a third of a second.
  const [viewerUrl, setViewerUrl] = useState(null);
  if (live.viewerUrl && viewerUrl !== live.viewerUrl) setViewerUrl(live.viewerUrl);
  // A sync that failed with no world on show has no live controls to restart
  // it from: the error offers the restart, which syncs again (live-world.mjs).
  const retrySync = live.syncFailed && !live.live && live.conversationId === activeId && html`
    <button type="button" class="btn" onClick=${() => world.control('restart').catch(onError)}>Try again</button>
  `;
  if (!viewerUrl) {
    return html`
      <div class="bluemap-placeholder">
        <${Icon} name="build" size="20" />
        <span>${live.viewerError || live.syncError || live.error || (activeId ? 'Preparing the live world…' : emptyText)}</span>
        ${retrySync}
      </div>
    `;
  }
  // The iframe could not show this view, so what it holds is unknown: asked
  // again, it loads the view afresh (bluemap-bridge.js), and what the world
  // did since its base follows.
  const retryView = () => {
    setViewerError(null);
    setPaintedFrame(null);
    refresh(true);
  };

  // Painted means the frame confirmed exactly this chat's current world.
  const framePainted = paintedFrame?.frame === frameRef.current && viewable(live) && live.conversationId === activeId
    && paintedFrame.key === `${live.conversationId}:${live.viewerRevision}`;
  // The cover hides another chat's world, or none yet. A newer version of
  // the world on screen, while it syncs or its tiles render, replaces the old
  // one in a single swap, so the old one stays in view meanwhile.
  const showsThisChat = paintedFrame?.frame === frameRef.current
    && paintedFrame.key.slice(0, paintedFrame.key.lastIndexOf(':')) === activeId;
  const shownError = bootError ?? (viewerError && live.conversationId === activeId && viewerError.key === `${live.conversationId}:${live.viewerRevision}` ? viewerError.text : null);
  const reloadViewer = () => frameRef.current.contentWindow.location.reload();
  const covered = !showsThisChat || shownError !== null;
  return html`
    <div class="bluemap-shell" aria-busy=${!framePainted}>
      <iframe
        ref=${frameRef}
        class="bluemap-frame"
        src=${viewerUrl}
        title="Live Minecraft world in BlueMap"
        onLoad=${() => {
          readyRef.current = false;
          setPaintedFrame(null);
          setViewerError(null);
          setBootError(null);
          // A fresh document lost its camera hash; the first refresh after
          // ready must resend it even for the same conversation.
          lastKeyRef.current = null;
          lastConversationRef.current = null;
          frameRef.current?.contentWindow?.postMessage(
            { type: 'mcbench-lite:ping' },
            window.location.origin,
          );
        }}
      ></iframe>
      ${covered && html`
        <div class="bluemap-placeholder bluemap-loading" role="status">
          <${Icon} name="build" size="20" />
          <span>${shownError ?? (!activeId ? emptyText : activeId.startsWith('bench:') ? 'Placing every build…' : 'Preparing this chat’s world…')}</span>
          ${shownError !== null && html`<button type="button" class="btn" onClick=${bootError !== null ? reloadViewer : retryView}>${bootError !== null ? 'Reload the viewer' : 'Try again'}</button>`}
        </div>
      `}
      ${(live.syncError || live.error) && html`
        <div class="bluemap-mismatch" role="status">${live.syncError || live.error}${retrySync}</div>
      `}
      ${!covered && live.live && html`<${LiveControls} live=${live.live} world=${world} onError=${onError} />`}
    </div>
  `;
}

// The live world in this page's engine: it plays at Minecraft's 20 ticks a
// second; paused, it steps one tick at a time; restart places the build
// afresh, which is all a world the engine stopped takes.
function LiveControls({ live, world, onError }) {
  const act = (action) => world.control(action).catch(onError);
  return html`
    <div class="live-controls" role="toolbar" aria-label="Live world">
      <button type="button" title=${live.playing ? 'Pause' : 'Play'} disabled=${live.stopped} onClick=${() => act(live.playing ? 'pause' : 'play')}>
        <${Icon} name=${live.playing ? 'pause' : 'play'} size="14" />
      </button>
      <button type="button" title="Step one tick" disabled=${live.playing || live.stopped} onClick=${() => act('step')}>
        <${Icon} name="step" size="14" />
      </button>
      <button type="button" title="Restart from the build" onClick=${() => act('restart')}>
        <${Icon} name="restart" size="14" />
      </button>
      <span class="live-ticks" title="Ticks since the build was placed">${live.ticks.toLocaleString()} ticks</span>
    </div>
  `;
}

// Every running dot fades on one clock (style.css, conv-pulse): an animation
// starts when its dot starts running, so each would be at its own point of
// the fade. Each is moved to start at the page's time zero, which puts them
// all at the same point, whenever they began.
document.addEventListener('animationstart', (event) => {
  if (event.animationName !== 'conv-pulse') return;
  for (const animation of event.target.getAnimations()) {
    if (animation.animationName === 'conv-pulse') animation.startTime = 0;
  }
});

function App() {
  const [conversations, setConversations] = useState([]);
  // The transcript's items. The ref is current between renders, so a burst
  // of events in one task applies each on top of the last.
  const [items, setItemsState] = useState([]);
  const itemsRef = useRef(items);
  function setItems(update) {
    itemsRef.current = typeof update === 'function' ? update(itemsRef.current) : update;
    setItemsState(itemsRef.current);
  }
  const [messageTree, setMessageTree] = useState({ headId: null, messages: [] });
  const [provider, setProvider] = useState(initialModelSelection.provider);
  const [model, setModel] = useState(initialModelSelection.model);
  const [effort, setEffort] = useState('');
  // The build resolution the next message asks for (web/resolution.mjs).
  // Chosen per chat, as the model is; a new chat starts at the last one
  // chosen in the picker, which this browser keeps.
  const [resolution, setResolution] = useState(() => storedResolution('mcb-last-resolution'));
  function chooseResolution(value) {
    setResolution(value);
    localStorage.setItem('mcb-last-resolution', JSON.stringify(value));
  }
  const modelCaps = useModelInfo(provider, model); // live capabilities for the current slug
  function setModelChoice(choice) {
    setProvider(choice.provider);
    setModel(choice.model);
    setEffort(choice.effort || '');
    setResolution(choice.resolution);
  }
  const [providers, setProviders] = useState({});
  // The providers and favorites as the server has them, read at boot:
  // loading, loaded, or failed (`error` says why), when they can be read again.
  const [config, setConfig] = useState({ phase: 'loading', error: null });
  // An instance behind the hosted sign-in gateway, whose /account page signs
  // out of the website and deletes the workspace (/api/config).
  const [hosted, setHosted] = useState(false);
  // The providers are read and none can be used: the empty screens, the
  // sidebar and the pickers point to Settings' Connect an AI.
  const noAI = config.phase === 'loaded' && !Object.values(providers).some(usable);
  const connectAI = () => openSettings(CONNECT).catch(showError);
  // The starred models (favorites.mjs): the server's list with this page's
  // stars on it. One that could not be saved goes back, and says so.
  const favoriteSyncRef = useRef(null);
  if (!favoriteSyncRef.current) {
    favoriteSyncRef.current = new Favorites({
      write: (list) => api('/api/model-favorites', { method: 'PUT', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ favorites: list }) }).then((data) => data.favorites),
      onChange: (list) => setFavorites(list),
      onError: (error) => showError(new Error(`The star was not saved, and went back: ${error.message}`)),
    });
  }
  const [favorites, setFavorites] = useState([]);
  // The chat turn on screen (transcript.mjs): what this page holds of it.
  const [turn, setTurnState] = useState(IDLE);
  const turnRef = useRef(IDLE);
  // Every change of it goes through the phase table: an illegal one throws.
  function setTurn(next) {
    turnRef.current = turnTo(turnRef.current, next);
    setTurnState(next);
  }
  // The controller of a send whose Stop came before the server took it.
  const stopAskedRef = useRef(null);
  // An edit's prompt on its way back to its draft (the revert, then the
  // prompt's files): that draft's key, or null. It belongs to the draft, not
  // to the screen, so leaving the place does not end it. One at a time; a
  // send from that draft waits for it.
  const restoringRef = useRef(null);
  const [restoringKey, setRestoringKey] = useState(null);
  function beginRestore(key) {
    if (restoringRef.current) throw new Error('An edit is already bringing its prompt back');
    restoringRef.current = key;
    setRestoringKey(key);
  }
  // The restore is over. `draft` is the prompt the server reverted to, which
  // lands in its draft: the composer if that draft is on screen, parked if
  // not. Null when the revert itself failed.
  function endRestore(draft) {
    const key = restoringRef.current;
    restoringRef.current = null;
    setRestoringKey(null);
    if (!draft) return;
    if (where().draftKey === key) applyDraft(draft);
    else draftsRef.current.put(key, draft);
  }
  // Whether the composer holds text. Only this is state: a keystroke that
  // does not change it re-renders nothing.
  const [hasText, setHasText] = useState(false);
  const [attachments, setAttachments] = useState([]);
  const [reading, setReading] = useState([]); // the draft's files still being read (draft.mjs)
  const [copied, setCopied] = useState(null); // the message item just copied
  const [minecraftLive, setMinecraftLive] = useState({
    status: 'idle', conversationId: null, address: null, error: null, syncError: null,
    viewerUrl: null, viewerRevision: null, cameraHash: null, mesher: null,
  });
  const [lightbox, setLightbox] = useState(null);
  // The search-everything panel: { query, active } while open, `active`
  // being the key of the item highlighted (command.mjs).
  const [command, setCommand] = useState(null);
  // The Settings panel (settings-panel.mjs): closed, opening, open or saving.
  // The ref is current between renders, for async results.
  const [panel, setPanelState] = useState(CLOSED);
  // A sign-in asked for from Connect an AI, { id, method }, until its
  // provider pane has started it.
  const [signInStart, setSignInStart] = useState(null);
  const panelRef = useRef(CLOSED);
  // Every change of it goes through the phase table: an illegal one throws.
  function setPanel(next) {
    panelRef.current = panelTo(panelRef.current, next);
    setPanelState(next);
  }
  // The draft on show, or null.
  const settings = panelDraft(panel);
  // Each subject's share (share.mjs: a chat or a bench), by its shareKey,
  // so a publish that runs on while another place is on show lands on its
  // own subject. The ref is current between renders, for the publish stream.
  const [shares, setSharesState] = useState({});
  const sharesRef = useRef({});
  const shareOf = (subject) => sharesRef.current[shareKey(subject)] ?? SHARE_UNKNOWN;
  function setShare(subject, next) {
    sharesRef.current = { ...sharesRef.current, [shareKey(subject)]: next };
    setSharesState(sharesRef.current);
  }
  // The shareKey of the subject whose Share panel is open, or null; the
  // saved shareRepo, read at each opening (undefined until the first read
  // lands).
  const [shareFor, setShareFor] = useState(null);
  const [shareRepo, setShareRepo] = useState(undefined);
  // Stable, as the panel's press-outside listener depends on it.
  const closeShare = useCallback(() => setShareFor(null), []);
  // Settings' list of published builds (published.mjs), read each time
  // Settings open. Each read is numbered: only the latest lands, and none
  // after Settings close.
  const [published, setPublishedState] = useState(PUBLISHED_UNREAD);
  const publishedRef = useRef(PUBLISHED_UNREAD);
  const publishedReadsRef = useRef(0);
  function setPublished(next) {
    publishedRef.current = next;
    setPublishedState(next);
  }
  // Each "Check again" is numbered. Only the latest lands, and none that a
  // save or an opening came after.
  const checkRef = useRef(0);
  const titleModelCaps = useModelInfo(settings?.titleModel?.provider, settings?.titleModel?.model);
  const [theme, setTheme] = useState(() => localStorage.getItem('mcb-theme') || 'dark');
  const [sidebarOpen, setSidebarOpen] = useState(() => localStorage.getItem('mcb-sidebar') !== 'closed');
  const [chatWidth, setChatWidth] = useState(() => Number(localStorage.getItem('mcb-chat-width')) || null);
  const [sidebarWidth, setSidebarWidth] = useState(() => Number(localStorage.getItem('mcb-sidebar-width')) || SIDEBAR.width);
  const [viewerFull, setViewerFull] = useState(false);
  // The place on screen (place.mjs): one value. Build shows a chat or the
  // new chat; Bench a new bench, a bench's comparison, one model's chat in
  // it, the leaderboard or the prompt library. The mode, the chat shown, the
  // viewer's world and the composer's draft all derive from it.
  const [place, setPlaceState] = useState(() => (localStorage.getItem('mcb-app-mode') === 'bench' ? { kind: 'new-bench' } : { kind: 'new-chat', id: null }));
  const placeRef = useRef(place);
  // The place each mode was last on: Bench keeps its own, and Build comes
  // back to its chat.
  const leftRef = useRef({ build: { kind: 'new-chat', id: null }, bench: { kind: 'new-bench' } });
  const [benches, setBenches] = useState([]);
  const benchesRef = useRef(benches);
  const [benchRounds, setBenchRounds] = useState({}); // bench id -> round on screen
  const benchRoundsRef = useRef(benchRounds);
  // Every change of place ends here: a navigation (which parks and restores
  // the drafts around it), or the new chat taking its row or its first message.
  function setPlace(next) {
    leftRef.current[placeKeys(next, benchRoundsRef.current).mode] = next;
    placeRef.current = next;
    setPlaceState(next);
  }
  // What the place means (its mode, chat, world and draft): `where()` is
  // current between renders, for handlers and async results.
  const where = () => placeKeys(placeRef.current, benchRoundsRef.current);
  const { mode: appMode, chatId: activeId, viewKey, draftKey } = placeKeys(place, benchRounds);
  // Each bench's details (bench-details.mjs): loading, loaded or failed. The
  // ref is current between renders, for async results.
  const [benchDetails, setBenchDetailsState] = useState({});
  const benchDetailsRef = useRef(benchDetails);
  function setBenchDetails(benchId, details) {
    benchDetailsRef.current = { ...benchDetailsRef.current, [benchId]: details };
    setBenchDetailsState(benchDetailsRef.current);
  }
  // Each details read is numbered; only the latest one asked for a bench lands.
  const detailsRef = useRef({ next: 0, latest: {} });
  // The archived items the sidebar shows for one mode (archive.mjs). Each
  // read is numbered: only the latest lands, and none after a close.
  const [archive, setArchiveState] = useState(ARCHIVE_CLOSED);
  const archiveRef = useRef(ARCHIVE_CLOSED);
  const archiveReadsRef = useRef(0);
  function setArchive(next) {
    archiveRef.current = next;
    setArchiveState(next);
  }
  // How many items each mode has archived, from the list stream: the
  // sidebar's archived section shows only for a mode that has some.
  const [archivedCounts, setArchivedCounts] = useState({ conversations: 0, benches: 0 });
  // A restored item to open once the list stream brings it back.
  const restoredRef = useRef(null);
  // The sidebar search: what is typed, and the server's ranked matches for
  // the current mode (null while nothing is typed).
  // Actions added since the binds were saved start on their defaults.
  const [binds, setBinds] = useState(() => ({ ...DEFAULT_BINDS, ...storedJson('mcb-binds', {}) }));
  useEffect(() => { localStorage.setItem('mcb-binds', JSON.stringify(binds)); }, [binds]);
  const [find, setFind] = useState('');
  const [found, setFound] = useState(null);
  const findRef = useRef(null);
  const [openBenches, setOpenBenches] = useState(() => new Set(storedJson('mcb-open-benches', [])));
  const [benchModels, setBenchModels] = useState(() => storedJson('mcb-bench-models', []));
  const [benchBlind, setBenchBlind] = useState(() => storedJson('mcb-bench-blind', true));
  // A new bench's resolution, for every model in it (web/resolution.mjs).
  const [benchResolution, setBenchResolution] = useState(() => storedResolution('mcb-bench-resolution'));
  useEffect(() => { localStorage.setItem('mcb-bench-blind', JSON.stringify(benchBlind)); }, [benchBlind]);
  useEffect(() => { localStorage.setItem('mcb-bench-resolution', JSON.stringify(benchResolution)); }, [benchResolution]);
  const [leaderboard, setLeaderboard] = useState(null);
  const [viewerFocus, setViewerFocus] = useState(null);
  const [selectToggle, setSelectToggle] = useState(0);
  // The prompt library (prompt-sync.mjs): the server's list with this page's
  // edits on it, and where its sync with the server stands.
  const promptSyncRef = useRef(null);
  if (!promptSyncRef.current) {
    promptSyncRef.current = new PromptSync({
      read: () => api('/api/prompts').then((data) => data.prompts),
      write: (list) => api('/api/prompts', { method: 'PUT', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ prompts: list }) }),
      onChange: (state) => setPromptLibrary(state),
      onError: (error) => showError(error),
    });
  }
  const [promptLibrary, setPromptLibrary] = useState(() => promptSyncRef.current.snapshot());
  const prompts = promptLibrary.list;
  const promptsReady = promptsLoaded(promptLibrary.phase);
  // An edit is a function of the list. One made before the library has
  // loaded throws: there is no list to make it on.
  const setPrompts = (edit) => promptSyncRef.current.edit(edit);
  const reloadPrompts = () => promptSyncRef.current.reload();
  const loadPrompts = () => promptSyncRef.current.load();
  useEffect(loadPrompts, []);
  const [promptsActiveId, setPromptsActiveId] = useState(null); // the prompt open in Bench
  const shownPromptId = prompts.some((prompt) => prompt.id === promptsActiveId) ? promptsActiveId : prompts[0]?.id ?? null;
  const shownPrompt = prompts.find((prompt) => prompt.id === shownPromptId) || null;

  const messagesRef = useRef(null);
  const fileInputRef = useRef(null);
  // Bumped by every navigation (leaveConversation): work begun before it
  // checks it after each await and lands nowhere it no longer belongs.
  const navigationRef = useRef(0);
  const conversationsRef = useRef(conversations);
  // Each place with a composer (a chat, the new chat, a bench) keeps its own
  // unsent draft (draft.mjs): { segments, attachments, selections, reading,
  // choice }, `choice` being a model picked for the chat and not yet sent
  // with. The draft of the place on screen is in the composer; the others
  // are parked here.
  const draftsRef = useRef(null);
  if (!draftsRef.current) draftsRef.current = new Drafts();
  const draftRef = useRef(null);
  const [selections, setSelections] = useState([]); // blocks picked in the viewer for this message
  const [hoveredSelection, setHoveredSelection] = useState(null);
  // The composer's content as segments (see composer.mjs). `version` changes
  // when the app rewrites it; typing only updates the segments and text.
  const [composerValue, setComposerValue] = useState({ segments: [], version: 0 });
  const segmentsRef = useRef([]);
  function setComposer(segments) {
    segmentsRef.current = segments;
    setComposerValue((current) => ({ segments, version: current.version + 1 }));
    setHasText(Boolean(segments.map((segment) => (segment.type === 'field' ? segment.value : segment.text)).join('').trim()));
    // A "/query" the palette followed is gone with the text it was in.
    setPalette(null);
  }
  draftRef.current = { attachments, selections, reading, choice: { provider, model, effort, resolution } };
  // What the composer holds now: typing updates the segments without a render.
  const currentDraft = () => ({ ...draftRef.current, segments: segmentsRef.current });
  function applyDraft(draft) {
    setComposer(draft.segments);
    setAttachments(draft.attachments);
    setSelections(draft.selections);
    setReading(draft.reading);
  }
  // Transcripts already fetched, by chat: going back to a chat shows it at
  // once, and hovering a chat fetches it ahead of the click.
  const transcriptsRef = useRef(new Map()); // id -> request promise; `.data` once it lands
  // The chat list has arrived (the composer's model is known).
  const [listReady, setListReady] = useState(false);
  // The transcript of the place on screen (transcript.mjs): loading, shown or
  // failed. The ref is current between renders, for async results.
  const [transcriptState, setTranscriptStateValue] = useState(LOADING);
  const transcriptStateRef = useRef(LOADING);
  function setTranscriptState(next) {
    transcriptStateRef.current = next;
    setTranscriptStateValue(next);
  }
  const branches = useMemo(() => branchNavigation(messageTree), [messageTree]);

  // Failures of what the page does (a click's request, a save, the chat
  // list, the viewer) show at the top of the chat column, under its header,
  // in whatever view: one notice per distinct text, kept until dismissed.
  const [notices, setNotices] = useState([]); // { id, text }
  const noticeIdRef = useRef(0);
  const showError = (err) => {
    const text = String(err.message || err);
    noticeIdRef.current += 1;
    const notice = { id: noticeIdRef.current, text };
    setNotices((current) => [...current.filter((entry) => entry.text !== text), notice]);
  };

  // What a world saved from the viewer is called: the chat's title, or the bench's with its round.
  const exportName = (() => {
    if (!viewKey) return 'mcbench world';
    if (viewKey.startsWith('bench:')) {
      const [, benchId, round] = viewKey.split(':');
      return `${benches.find((bench) => bench.id === benchId)?.title || 'Bench'} round ${round}`;
    }
    return conversations.find((conversation) => conversation.id === viewKey)?.title || 'mcbench world';
  })();

  function fetchTranscript(id, { fresh = false } = {}) {
    const cache = transcriptsRef.current;
    if (!fresh && cache.has(id)) return cache.get(id);
    const request = api(`/api/conversations/${id}`);
    request.then((data) => { request.data = data; }, () => { if (cache.get(id) === request) cache.delete(id); });
    cache.set(id, request);
    return request;
  }

  // A stored error dismissed here is dismissed in the fetched copy of its
  // chat too: the cache holds what the page last knew, so coming back to the
  // chat does not show the error again. A copy still on its way may be from
  // before the dismissal, and is let go of.
  function forgetError(id, messageId) {
    const cache = transcriptsRef.current;
    const cached = cache.get(id);
    if (!cached) return;
    if (!cached.data) {
      cache.delete(id);
      return;
    }
    const data = dismissError(cached.data, messageId);
    cache.set(id, Object.assign(Promise.resolve(data), { data }));
  }

  // Items are built once per fetched transcript, so a chat shown again keeps
  // its item objects and the transcript's cached entries with them.
  const itemsByTranscript = useRef(new WeakMap());
  function showTranscript(data) {
    if (!itemsByTranscript.current.has(data)) itemsByTranscript.current.set(data, itemsFromMessages(data.messages));
    setItems(itemsByTranscript.current.get(data));
    setMessageTree(data.messageTree);
    setTranscriptState(SHOWN);
  }

  // Messages never change once written, except an error being dismissed.
  const sameTranscript = (a, b) => a.messages.length === b.messages.length
    && a.messages.at(-1)?.id === b.messages.at(-1)?.id
    && a.messageTree.headId === b.messageTree.headId
    && a.messageTree.messages.length === b.messageTree.messages.length
    && a.messages.filter((message) => message.dismissed).length === b.messages.filter((message) => message.dismissed).length;

  // Pull the canonical state from the server; it, or why it could not be
  // read, is applied only if the user is still looking at that conversation
  // and no turn stream began meanwhile.
  async function resync(id) {
    const navigation = navigationRef.current;
    const turn = turnRef.current;
    const here = () => where().chatId === id && navigationRef.current === navigation && turnRef.current === turn;
    let data;
    try {
      data = await fetchTranscript(id, { fresh: true });
    } catch (error) {
      if (here()) setTranscriptState(readFailed(error));
      return;
    }
    if (here()) showTranscript(data);
  }

  // A transcript that failed to load is read again: from its pane, or by
  // clicking its row. What is on screen stays until the read lands.
  function readAgain() {
    setTranscriptState(LOADING);
    resync(where().chatId).catch(showError);
  }

  // What takes the place of a row that left the list: the next one below it
  // still listed, or the nearest above.
  function successor(before, listed, id) {
    const index = before.findIndex((row) => row.id === id);
    return before.slice(index + 1).find((row) => listed.has(row.id))
      || before.slice(0, index).reverse().find((row) => listed.has(row.id))
      || null;
  }

  // The chat list from the server. A chat that left it (archived here or in
  // another window) takes its draft and cached transcript with it, and the
  // view on it moves to its neighbour.
  function receiveConversations(rows) {
    const before = conversationsRef.current;
    const old = new Map(before.map((row) => [row.id, row]));
    const listed = new Set(rows.map((row) => row.id));
    for (const row of rows) {
      const previous = old.get(row.id);
      // A turn started or ended: a fetched transcript of that chat is stale.
      if (previous?.runningSince !== row.runningSince) transcriptsRef.current.delete(row.id);
      // A message stored outside a turn (a failed title) reloads an open chat.
      else if (previous && previous.messageCount !== row.messageCount) {
        transcriptsRef.current.delete(row.id);
        if (row.id === where().chatId && !turnRunning(turnRef.current, row)) resync(row.id).catch(showError);
      }
      load(modelInfoUrl(row.provider, row.model)).catch(() => {});
    }
    conversationsRef.current = rows;
    setConversations(rows);
    // The list shows a chat once it has a message: the new chat's row in it
    // means a send the page gave up on reached the server all the same.
    if (listed.has(newChatRef.current?.id)) settleNewChat(newChatRef.current.id);
    const gone = before.filter((row) => !listed.has(row.id));
    for (const row of gone) transcriptsRef.current.delete(row.id);
    const at = placeRef.current;
    if (at.kind === 'chat' && gone.some((row) => row.id === at.id)) {
      const next = successor(before.filter((row) => !row.benchId), listed, at.id);
      if (next) openConversation(next.id).catch(showError);
      else showNewChat();
    }
    for (const row of gone) draftsRef.current.drop(chatDraft(row.id));
  }

  // The bench list: a bench that left it closes in the sidebar, and the view
  // on it moves to its neighbour; a round on screen past the bench's last
  // (reverted elsewhere) comes back to it.
  function receiveBenches(rows, first) {
    const before = benchesRef.current;
    const listed = new Set(rows.map((bench) => bench.id));
    benchesRef.current = rows;
    setBenches(rows);
    const gone = before.filter((bench) => !listed.has(bench.id)).map((bench) => bench.id);
    // The first list also lets go of benches archived while the page was away.
    if (first || gone.length) setOpenBenches((current) => new Set([...current].filter((id) => (first ? listed.has(id) : !gone.includes(id)))));
    // Bench's place, on screen or the one it comes back to.
    const view = leftRef.current.bench;
    if (view.benchId && !listed.has(view.benchId) && before.some((bench) => bench.id === view.benchId)) {
      const next = successor(before, listed, view.benchId);
      const moved = next ? { kind: 'compare', benchId: next.id } : { kind: 'new-bench' };
      if (where().mode === 'bench') showBenchView(moved);
      else leftRef.current.bench = moved;
    }
    for (const id of gone) draftsRef.current.drop(benchDraft(id));
    for (const bench of rows) {
      if (benchRoundsRef.current[bench.id] > bench.rounds) showRound(bench.id, bench.rounds);
    }
  }

  // The chat list is pushed by the server whenever something the sidebar
  // shows changes: a turn starting or ending, a title, a chat seen or archived.
  useEffect(() => {
    let stopped = false;
    let controller = null;
    let first = true;
    // A hidden page lets go of its stream (browsers allow few open requests per
    // host); it reconnects, and gets the whole list again, when shown.
    let shown = null;
    const onVisibility = () => {
      if (document.visibilityState === 'visible') shown?.();
      else controller?.abort();
    };
    document.addEventListener('visibilitychange', onVisibility);
    // One notice per outage, not one per retry.
    let failing = false;
    // A list from the server, and on the first one, where the page opens.
    const receiveLists = ({ conversations: rows, benches: benchRows, archived }) => {
      receiveConversations(rows);
      receiveBenches(benchRows, first);
      receiveArchivedCounts(archived);
      openRestored();
      if (!first) return;
      first = false;
      setListReady(true);
      if (navigationRef.current !== 0) return; // the user already moved
      if (where().mode === 'bench') {
        // A prompt typed before the list came stays on the new bench it was typed for.
        if (benchRows.length && draftEmpty(currentDraft())) showBenchView({ kind: 'compare', benchId: benchRows[0].id });
        setTranscriptState(SHOWN);
        return;
      }
      const latest = rows.find((row) => !row.benchId);
      if (!latest) {
        setTranscriptState(SHOWN);
        return;
      }
      openConversation(latest.id).catch(showError);
      // What was typed before the list came goes with the chat that opens.
      const typed = draftsRef.current.take(NEW_CHAT_DRAFT);
      if (!draftEmpty(typed)) applyDraft(typed);
    };
    (async () => {
      while (!stopped) {
        if (document.visibilityState !== 'visible') await new Promise((resolve) => { shown = resolve; });
        if (stopped) return;
        controller = new AbortController();
        try {
          const res = await fetch('/api/conversations/stream', { signal: controller.signal });
          if (!res.ok) throw new Error(`The chat list failed to load (HTTP ${res.status})`);
          await readLines(res, controller.signal, (lists) => {
            failing = false;
            // A list this page cannot apply is a bug here, not a dropped
            // connection: it shows, and the stream reads on. Only a failure
            // of the request or the stream reaches the catch below.
            try {
              receiveLists(lists);
            } catch (error) {
              console.error(error);
              showError(error);
            }
          });
        } catch (error) {
          // Aborts (hidden page, unmount) and dropped connections just reconnect.
          if (!controller.signal.aborted && error.name !== 'TypeError' && !failing) {
            failing = true;
            showError(error);
          }
        }
        if (!stopped && document.visibilityState === 'visible') await new Promise((resolve) => setTimeout(resolve, 1000));
      }
    })();
    return () => {
      stopped = true;
      document.removeEventListener('visibilitychange', onVisibility);
      shown?.();
      controller?.abort();
    };
  }, []);

  // The first read, or another after one that failed.
  function loadConfig() {
    setConfig({ phase: 'loading', error: null });
    api('/api/config').then((data) => {
      setProviders(data.providers);
      setHosted(data.hosted === true);
      favoriteSyncRef.current.load(data.modelFavorites);
      setConfig({ phase: 'loaded', error: null });
      // Warm every model catalog so pickers open already filled.
      for (const [id, entry] of Object.entries(data.providers)) {
        if (usable(entry)) load(modelsUrl(id)).catch(() => {});
      }
    }, (error) => {
      setConfig({ phase: 'failed', error: String(error.message || error) });
      showError(error);
    }).catch(showError);
  }
  useEffect(loadConfig, []);

  // The first start's steps (server/setup.js), read each second until the
  // app is ready: a later start is ready at the first read, and the page
  // never shows them. Null until that read.
  const [setup, setSetup] = useState(null);
  useEffect(() => {
    let live = true;
    let timer = null;
    const read = () => api('/api/setup').then((view) => {
      if (!live) return;
      setSetup(view);
      if (!view.ready) timer = setTimeout(read, 1000);
    }, showError);
    read();
    return () => { live = false; clearTimeout(timer); };
  }, []);
  const preparing = setup && !setup.ready ? setupStatus(setup) : null;
  // A send before the app is ready is held (submit) until it is: the draft
  // it came from, or null. A held draft not on screen when the app is ready
  // stays unsent.
  const [held, setHeld] = useState(null);
  useEffect(() => {
    if (!held || !setup?.ready) return;
    setHeld(null);
    if (where().draftKey === held) submit().catch(showError);
  }, [held, setup?.ready]);
  // Settings closing with an AI connected and no chat model chosen for the
  // composer (`provider` and `model`, as send() needs them) open the chat
  // model picker by itself, so that choice is the next thing in view. A
  // chosen model is kept (mcb-last-model), so this happens only until one is.
  const [askModel, setAskModel] = useState(false);
  const askedModelRef = useRef(false); // the picker open now opened by itself
  const settingsWereOpenRef = useRef(false);
  useEffect(() => {
    const closed = panel.phase === 'closed';
    if (closed && settingsWereOpenRef.current && !noAI && !(provider && model) && where().mode === 'build') setAskModel(true);
    settingsWereOpenRef.current = !closed;
  }, [panel.phase]);
  // The first build: the app is ready, an AI is connected, a chat model is
  // chosen and there is no chat yet, so the prompt box is where to start.
  // Again each time Settings close on that screen, as after connecting, and
  // once the model picker that opened then has a model chosen.
  const firstBuild = setup?.ready === true && config.phase === 'loaded' && !noAI && Boolean(provider && model)
    && conversations.length === 0 && place.kind === 'new-chat' && panel.phase === 'closed';
  useEffect(() => {
    if (firstBuild) composerRef.current?.focus();
  }, [firstBuild]);

  // What is on screen counts as seen. A turn this page is not following
  // (started from another window) is joined as soon as the list shows it.
  const activeRow = conversations.find((row) => row.id === activeId) || null;
  const [pageVisible, setPageVisible] = useState(() => document.visibilityState === 'visible');
  useEffect(() => {
    const update = () => setPageVisible(document.visibilityState === 'visible');
    document.addEventListener('visibilitychange', update);
    return () => document.removeEventListener('visibilitychange', update);
  }, []);
  const markSeen = (id) => api(`/api/conversations/${id}/seen`, { method: 'POST' }).catch(() => {});
  useEffect(() => {
    if (activeRow?.unread && pageVisible) markSeen(activeRow.id);
  }, [activeRow?.id, activeRow?.unread, pageVisible]);
  // Busy while the page holds a turn's stream or send, or the server runs one.
  const busy = turnRunning(turn, activeRow);
  // A turn the server runs and this page does not read is joined: one begun
  // in another window, or one whose stream broke.
  useEffect(() => {
    const startedAt = turnToJoin(turn, activeRow);
    if (startedAt) attachToRun(activeRow.id, startedAt);
  }, [activeRow?.id, activeRow?.runningSince, turn]);

  // The live world is this tab's own (web/live-world.mjs): an engine in a
  // worker of this page ticks the build on show, with nothing shared with
  // other tabs, and the server only says what the build is. Its status has
  // the shape the viewer draws from.
  const liveWorldRef = useRef(null);
  if (!liveWorldRef.current) liveWorldRef.current = new LiveWorld(browserHost());
  const minecraftLiveRef = useRef(minecraftLive);
  useEffect(() => {
    const world = liveWorldRef.current;
    const stop = world.subscribe((status) => {
      minecraftLiveRef.current = status;
      setMinecraftLive(status);
    });
    world.begin();
    return () => {
      stop();
      world.dispose();
    };
  }, []);
  useEffect(() => {
    liveWorldRef.current.show(viewKey).catch(showError);
  }, [viewKey]);

  // Sticky autoscroll, race-free: follow the stream only if the user was at
  // the bottom of the PREVIOUS content (height recorded on the last update).
  // Scroll events are too async to trust with deltas landing every ~40ms —
  // position vs. previous height needs no events and cannot misfire.
  const prevScrollHeightRef = useRef(0);

  useEffect(() => {
    const el = messagesRef.current;
    if (!el) return;
    const pinned = el.scrollTop + el.clientHeight >= prevScrollHeightRef.current - 48;
    prevScrollHeightRef.current = el.scrollHeight;
    if (pinned) el.scrollTop = el.scrollHeight;
  }, [items, busy, appMode]);

  // The layers open over the app (layers.mjs). Escape closes the topmost one
  // only, wherever the focus is. Popovers stop their own Escape.
  const settingsOpen = Boolean(settings);
  // What the header's Share button shares: a chat in Build whose world on
  // show holds a build, or a bench's comparison; null elsewhere. Its panel
  // shows only on that subject.
  const shareSubject = appMode === 'build' && activeId !== null && viewable(minecraftLive)
    && minecraftLive.conversationId === activeId && minecraftLive.blockCount > 0
    ? { kind: 'chat', id: activeId }
    : place.kind === 'compare' ? { kind: 'bench', id: place.benchId } : null;
  const shareShown = shareSubject !== null && shareFor === shareKey(shareSubject);
  // A bench with a model still running cannot be shared yet.
  const shareBlocked = shareSubject?.kind === 'bench' && conversations.some((row) => row.benchId === shareSubject.id && row.runningSince);
  const layers = { command: Boolean(command), lightbox: Boolean(lightbox), settings: settingsOpen, viewer: viewerFull, share: shareShown };
  const layersRef = useRef(layers);
  layersRef.current = layers;
  // Escape closes the topmost layer: the key pressed here, or passed on by
  // the viewer (bluemap-bridge.js), whose frame keeps its own keys. It reads
  // the layers as last rendered, not as they were when an effect last ran: an
  // Escape right after a layer opens closes that one.
  const closeLayer = {
    command: () => setCommand(null),
    lightbox: () => setLightbox(null),
    settings: closeSettings,
    viewer: () => setViewerFull(false),
    share: closeShare,
  };
  const closeTopLayer = () => {
    const top = topLayer(layersRef.current);
    if (top) closeLayer[top]();
  };
  const closeTopLayerRef = useRef(closeTopLayer);
  closeTopLayerRef.current = closeTopLayer;
  useEffect(() => {
    const onKey = (evt) => { if (evt.key === 'Escape') closeTopLayerRef.current(); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem('mcb-theme', theme);
  }, [theme]);

  useEffect(() => {
    localStorage.setItem('mcb-app-mode', appMode);
  }, [appMode]);

  useEffect(() => {
    localStorage.setItem('mcb-sidebar', sidebarOpen ? 'open' : 'closed');
  }, [sidebarOpen]);

  useEffect(() => {
    localStorage.setItem('mcb-sidebar-width', String(sidebarWidth));
  }, [sidebarWidth]);

  // The sidebar's edge resizes it; pulled far enough left it collapses, and
  // pulled back out it opens again. Collapsed, the window's left edge is the
  // handle: a click brings the sidebar back, a drag brings it back sized.
  function startSidebarDrag(evt) {
    if (evt.button !== 0) return;
    evt.preventDefault();
    const left = evt.currentTarget.closest('.layout').getBoundingClientRect().left;
    const startX = evt.clientX;
    const wasOpen = sidebarOpen;
    const startWidth = sidebarWidth;
    let moved = false;
    let collapsed = !sidebarOpen;
    const move = (e) => {
      if (!moved && Math.abs(e.clientX - startX) < 3) return;
      moved = true;
      const width = e.clientX - left;
      collapsed = width < SIDEBAR.collapseBelow;
      if (collapsed) {
        setSidebarOpen(false);
        return;
      }
      setSidebarOpen(true);
      setSidebarWidth(Math.round(Math.min(Math.max(width, SIDEBAR.min), SIDEBAR.max)));
    };
    const up = () => {
      window.removeEventListener('mousemove', move);
      window.removeEventListener('mouseup', up);
      document.body.classList.remove('dragging', 'sizing-sidebar');
      if (!moved && !wasOpen) setSidebarOpen(true);
      // Collapsing keeps the width it had, not the minimum it passed on the way.
      if (moved && collapsed) setSidebarWidth(startWidth);
    };
    document.body.classList.add('dragging', 'sizing-sidebar');
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
  }

  // Drag the strata divider to trade chat width for world width.
  function startDividerDrag(evt) {
    evt.preventDefault();
    const layout = evt.currentTarget.parentElement;
    const chat = layout.querySelector('.chat').getBoundingClientRect();
    const room = layout.getBoundingClientRect().right - chat.left - evt.currentTarget.offsetWidth;
    const move = (e) => {
      const width = Math.min(Math.max(e.clientX - chat.left, 320), Math.max(room - 360, 320));
      setChatWidth(width);
      localStorage.setItem('mcb-chat-width', String(Math.round(width)));
    };
    const up = () => {
      window.removeEventListener('mousemove', move);
      window.removeEventListener('mouseup', up);
      document.body.classList.remove('dragging');
    };
    document.body.classList.add('dragging');
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
  }

  // The published builds are read as Settings open (readSettings), so the
  // list never renders unread, and let go of when they close.
  useEffect(() => {
    if (!settingsOpen && publishedRef.current.phase !== 'unread') {
      publishedReadsRef.current += 1;
      setPublished(publishedClosed(publishedRef.current));
    }
  }, [settingsOpen]);

  function loadPublished() {
    publishedReadsRef.current += 1;
    const read = publishedReadsRef.current;
    setPublished(publishedReading(publishedRef.current));
    api('/api/shares').then((data) => {
      if (publishedReadsRef.current === read) setPublished(publishedRead(publishedRef.current, data));
    }, (error) => {
      if (publishedReadsRef.current === read) setPublished(publishedFailed(publishedRef.current, error));
    }).catch(showError);
  }

  // Takes a published build down from Settings, then reads the list again.
  // A chat or bench whose share this page holds as shared goes through the
  // same removal moves as its Share panel's Remove, so its header reads
  // right; one not read yet is read when its panel next opens. Settings closed
  // meanwhile let go of the list, and with it this removal's row.
  async function removePublished(build) {
    const shown = () => publishedRef.current.phase !== 'unread' && publishedRef.current.removing.includes(build.shareId);
    const subject = publishedSubject(build);
    const chat = subject !== null && shareOf(subject).phase === 'shared' ? subject : null;
    setPublished(publishedRemoving(publishedRef.current, build.shareId));
    if (chat) setShare(chat, shareRemoving(shareOf(chat)));
    try {
      await api(`/api/shares/${encodeURIComponent(build.shareId)}`, { method: 'DELETE' });
    } catch (error) {
      if (shown()) setPublished(publishedKept(publishedRef.current, build.shareId, error));
      if (chat) setShare(chat, shareKept(shareOf(chat)));
      return;
    }
    if (chat) setShare(chat, shareRemoved(shareOf(chat)));
    if (!shown()) return;
    setPublished(publishedRemoved(publishedRef.current, build.shareId));
    loadPublished();
  }

  // Keeps "Checked … ago" current while Settings is open.
  const [, setSettingsClock] = useState(0);
  useEffect(() => {
    if (!settingsOpen) return undefined;
    const timer = setInterval(() => setSettingsClock((tick) => tick + 1), 15000);
    return () => clearInterval(timer);
  }, [settingsOpen]);

  // Opens Settings on the pane `view` (by default Connect an AI while no AI
  // is connected, General once one is), from a fresh read of what is saved
  // (with the title model's efforts, so the panel opens complete). Settings
  // already open keep their unsaved draft and only turn to `view`; ones
  // still opening open on it; ones being saved cannot be opened anew.
  const openingRef = useRef(null); // the read an opening waits for
  async function openSettings(view = firstPane(providers)) {
    const { phase } = panelRef.current;
    if (phase === 'open') return showSettingsPane(view);
    if (phase === 'saving') throw new Error('Settings are being saved');
    setPanel({ phase: 'opening', view });
    if (phase === 'closed') openingRef.current = readSettings();
    await openingRef.current;
  }

  async function readSettings() {
    let saved;
    try {
      saved = await api('/api/settings');
      // null: titles are written by each chat's own model.
      if (saved.titleModel) await load(modelInfoUrl(saved.titleModel.provider, saved.titleModel.model)).catch(() => {});
    } catch (error) {
      setPanel(CLOSED);
      throw error;
    }
    checkRef.current += 1;
    loadPublished();
    setPanel({
      phase: 'open',
      view: panelRef.current.view,
      draft: { ...saved, providers: Object.fromEntries(Object.entries(saved.providers).map(([id, connection]) => [id, editable(connection)])) },
      error: '',
    });
  }

  // Closes open Settings, dropping the draft. Ones being saved close, or
  // not, with the save's answer.
  function closeSettings() {
    if (panelRef.current.phase === 'open') setPanel(CLOSED);
  }

  // Opens the Share panel of `subject` (a chat or a bench). The saved
  // repository is read at each opening, as Settings may have changed it; the
  // share is read once, the first time, and this page keeps it from then on.
  async function openShare(subject) {
    setShareFor(shareKey(subject));
    const repo = api('/api/settings').then((saved) => setShareRepo(saved.shareRepo));
    if (shareOf(subject).phase === 'unknown') {
      setShare(subject, shareReading(shareOf(subject)));
      let read;
      try {
        read = await api(shareUrl(subject));
      } catch (error) {
        setShare(subject, shareUnread(shareOf(subject)));
        throw error;
      }
      setShare(subject, shareRead(shareOf(subject), read));
    }
    await repo;
  }

  // Publishes `subject`, following the server's stream to the end. A
  // refusal (400, or 409 while a turn runs) fails the publish with the
  // server's message.
  async function publishShare(subject) {
    setShare(subject, sharePublishing(shareOf(subject)));
    try {
      const res = await fetch(shareUrl(subject), { method: 'POST' });
      if (!res.ok) throw new Error((await res.json()).error);
      await readShareStream(res.body, (message) => setShare(subject, shareStep(shareOf(subject), message)));
    } catch (error) {
      setShare(subject, shareFailed(shareOf(subject), error));
      return;
    }
    setShare(subject, shareStoppedEarly(shareOf(subject)));
  }

  // Takes `subject`'s page down. One that fails stays shared, and says why.
  async function removeShare(subject) {
    setShare(subject, shareRemoving(shareOf(subject)));
    try {
      await api(shareUrl(subject), { method: 'DELETE' });
    } catch (error) {
      setShare(subject, shareKept(shareOf(subject)));
      throw error;
    }
    setShare(subject, shareRemoved(shareOf(subject)));
  }

  // A change to the draft: only open Settings take one (panelEdit throws).
  const editSettings = (change) => setPanel(panelEdit(panelRef.current, change));
  // Another pane of the open Settings: not an edit, so a save's error stays.
  const showSettingsPane = (view) => setPanel({ ...panelRef.current, view });

  // The fields Settings edits, seeded from what the server reports.
  function editable(connection) {
    return {
      ...connection,
      ...(connection.envKey ? { apiKey: '' } : {}),
    };
  }

  // Re-reads logins, keys and CLI versions without discarding unsaved edits.
  // What it finds is the saved state, so the pickers take it at once. A
  // check that a later check, a save or another opening came after read
  // older settings than theirs, and lands nowhere.
  async function recheckSettings() {
    checkRef.current += 1;
    const check = checkRef.current;
    const fresh = await api('/api/settings?refresh=1');
    if (checkRef.current !== check) return;
    setProviders(fresh.providers);
    if (panelRef.current.phase !== 'open') return;
    editSettings((draft) => ({
      ...draft,
      checkedAt: fresh.checkedAt,
      providers: Object.fromEntries(Object.entries(fresh.providers).map(([id, connection]) => {
        const kept = draft.providers[id];
        return [id, { ...editable(connection), apiKey: kept.apiKey, enabled: kept.enabled,
          hiddenModels: kept.hiddenModels, modelOrder: kept.modelOrder }];
      })),
    }));
  }

  // The draft goes to the server as it is now: until the answer, Settings
  // are `saving`, and take no edit, no second save and no closing. A save
  // that lands closes them; one that fails opens them again with its error.
  async function saveSettings() {
    const { view, draft } = panelRef.current;
    setPanel({ phase: 'saving', view, draft });
    checkRef.current += 1;
    let result;
    try {
      result = await api('/api/settings', {
        method: 'PUT',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(draft),
      });
    } catch (err) {
      setPanel({ phase: 'open', view, draft, error: String(err.message || err) });
      return;
    }
    setProviders(result.providers);
    setPanel(CLOSED);
  }

  function updateConnection(id, field, value) {
    editSettings((draft) => ({
      ...draft,
      providers: {
        ...draft.providers,
        [id]: { ...draft.providers[id], [field]: value },
      },
    }));
  }

  // A star before the favorites loaded has no list to go on: it says so.
  function toggleFavorite(key) {
    try {
      favoriteSyncRef.current.toggle(key);
    } catch (error) {
      showError(error);
    }
  }

  // Switching chats is synchronous: the sidebar row already carries the
  // chat's model, effort and running state, and a transcript fetched before
  // shows at once. The fresh copy replaces it when it lands.
  // `next` is the chat's place: a Build chat, or a bench model's.
  async function openConversation(id, next = { kind: 'chat', id }) {
    if (id === where().chatId) {
      if (transcriptStateRef.current.phase === 'failed') readAgain();
      return;
    }
    const row = conversationsRef.current.find((entry) => entry.id === id);
    leaveConversation();
    const navigation = navigationRef.current;
    const draft = arrive(next);
    // The picker is Build's: a bench model's chat never sets it.
    if (next.kind === 'chat') setModelChoice(draft.choice ?? row);
    if (row.runningSince) {
      // The join effect reads the turn, whose stream replays it from its start.
      clearTranscript();
      return;
    }
    // What is on screen now: a cached copy, or nothing while its first fetch
    // (perhaps a prefetch still in flight) is awaited below.
    const onScreen = transcriptsRef.current.get(id)?.data ?? null;
    if (onScreen) showTranscript(onScreen);
    else clearTranscript();
    const here = () => where().chatId === id && navigationRef.current === navigation && turnRef.current.phase === 'idle';
    let data;
    try {
      data = await fetchTranscript(id, { fresh: Boolean(onScreen) });
    } catch (error) {
      if (here()) setTranscriptState(readFailed(error));
      return;
    }
    if (!here()) return;
    // The fresh copy usually matches what is already on screen.
    if (onScreen && sameTranscript(onScreen, data)) return;
    showTranscript(data);
  }

  // The new chat: empty, with the draft (and model) it was left with, and
  // its row on the server once it has one (createdChat).
  function showNewChat() {
    leaveConversation();
    const { choice } = arrive({ kind: 'new-chat', id: newChatRef.current?.id ?? null });
    clearTranscript();
    setTranscriptState(SHOWN);
    // A new chat with no choice of its own yet starts at the last resolution
    // chosen, not at the one of the chat just left.
    if (choice) setModelChoice(choice);
    else setResolution(storedResolution('mcb-last-resolution'));
  }

  // Shared by every navigation: park the draft, drop the turn's stream (never
  // the turn) and anything else that belonged to the place being left.
  function leaveConversation() {
    parkDraft();
    turnRef.current.controller?.abort();
    setTurn(IDLE);
    navigationRef.current += 1;
    setPalette(null);
    prevScrollHeightRef.current = 0; // a fresh view starts pinned to the newest message
  }

  // The other half of every navigation: the place on screen is `next`, and
  // its draft, returned, leaves the parked ones for the composer. A place
  // with no composer has no draft.
  function arrive(next) {
    setPlace(next);
    const key = where().draftKey;
    const draft = key ? draftsRef.current.take(key) : EMPTY_DRAFT;
    applyDraft(draft);
    return draft;
  }

  // What the composer holds is parked under the place's draft key. The
  // picker's model is part of a Build draft only: the new chat's always, a
  // chat's while it differs from the chat's own.
  function parkDraft() {
    const key = where().draftKey;
    if (!key) return;
    const draft = currentDraft();
    const at = placeRef.current;
    const row = at.kind === 'chat' && conversationsRef.current.find((entry) => entry.id === at.id);
    const { choice } = draft;
    const kept = at.kind === 'new-chat' || (row && (choice.provider !== row.provider || choice.model !== row.model || choice.effort !== (row.effort || '') || choice.resolution !== row.resolution));
    draftsRef.current.park(key, { ...draft, choice: kept ? choice : null });
  }

  // A message that did not go out goes back where it was written (the draft
  // `key`, in the composer or parked), unless something new was written
  // there since.
  function putBack(key, draft) {
    if (where().draftKey !== key) draftsRef.current.putBack(key, draft);
    else if (draftEmpty(currentDraft())) applyDraft(draft);
  }

  function clearTranscript() {
    setItems([]);
    setMessageTree({ headId: null, messages: [] });
    setTranscriptState(LOADING);
  }

  // Joins the turn the server runs for chat `id`, begun at `startedAt`: its
  // stream replays the turn from its start. A 404 is the server saying no
  // turn runs: it already ended. Any other refusal says nothing about the
  // turn, which is left running, unread.
  async function attachToRun(id, startedAt) {
    const controller = new AbortController();
    setTurn({ phase: 'reading', id, controller, status: null });
    let res;
    try {
      res = await fetch(`/api/conversations/${id}/stream`, { signal: controller.signal });
    } catch (error) {
      if (!controller.signal.aborted) lostStream(controller, id);
      return;
    }
    if (res.status === 404) return finishTurn(controller, id, startedAt);
    if (!res.ok) {
      const body = await res.json().catch(() => ({})); // a refusal may not be JSON
      if (turnRef.current.controller !== controller) return;
      showError(new Error(body.error || `HTTP ${res.status}`));
      return detachTurn(controller, id, startedAt);
    }
    await readTurn(res, id, controller, startedAt);
  }

  // Reads a turn's events until its stream ends: finished (the turn sent its
  // done or error), lost (the stream broke first, so join again), or broken
  // (an event this page cannot apply: a bug, shown, and the turn, which the
  // server still runs, left unread). `startedAt` is the turn's start, null
  // for one this page sent.
  async function readTurn(res, id, controller, startedAt) {
    let finished = false;
    let broken = null;
    try {
      await readLines(res, controller.signal, (event) => {
        if (turnRef.current.controller !== controller) return;
        try {
          receiveTurnEvent(event, controller);
        } catch (error) {
          broken = error;
          throw error;
        }
        if (event.type === 'done' || event.type === 'error') finished = true;
      });
    } catch (error) {
      if (error instanceof SyntaxError) broken = error;
    }
    if (turnRef.current.controller !== controller) return;
    if (!broken && !finished) return lostStream(controller, id);
    if (broken) showError(broken);
    // A sent turn's start is the one the list shows for it.
    const began = startedAt ?? conversationsRef.current.find((row) => row.id === id)?.runningSince ?? null;
    if (finished) finishTurn(controller, id, began);
    else detachTurn(controller, id, began);
  }

  // The page found the turn over: the list's running mark for it is stale
  // from here on, and the transcript is read again.
  function finishTurn(controller, id, startedAt) {
    if (turnRef.current.controller !== controller) return;
    controller.abort(); // an answer left unread is let go of
    turnEnded(id, startedAt);
  }

  function turnEnded(id, startedAt) {
    setTurn({ phase: 'idle', ended: startedAt });
    // It finished in front of the user, even if they leave before the list says so.
    if (document.visibilityState === 'visible') markSeen(id);
    resync(id).catch(showError);
  }

  // The page stops reading a turn the server still runs, and lets go of its
  // stream. The chat stays busy, so Stop works and nothing is sent into it,
  // until the list no longer shows the turn running (the effect below).
  function detachTurn(controller, id, startedAt) {
    if (turnRef.current.controller !== controller) return;
    controller.abort();
    setTurn({ phase: 'detached', id, startedAt });
  }
  // The list stopped showing a detached turn running: it is over.
  useEffect(() => {
    if (turnRef.current === turn && turnOver(turn, activeRow)) turnEnded(turn.id, turn.startedAt);
  }, [activeRow?.runningSince, turn]);

  // The stream broke while the turn may still run. After a second the join
  // effect reads it again if the list still shows it running; if not, the
  // transcript is read again.
  function lostStream(controller, id) {
    if (turnRef.current.controller !== controller) return;
    const waiting = { phase: 'waiting' };
    setTurn(waiting);
    setTimeout(() => {
      if (turnRef.current !== waiting) return;
      setTurn({ phase: 'idle', ended: null });
      resync(id).catch(showError);
    }, 1000);
  }

  // One event of the turn being read: items through applyEvent, and what
  // else it says (the harness's status line, a changed library or world).
  function receiveTurnEvent(event, controller) {
    if (event.type === 'turn_snapshot') {
      setItems(itemsFromMessages(event.messages));
      setMessageTree(event.messageTree);
      setTranscriptState(SHOWN);
      return;
    }
    if (event.type === 'harness_status' || event.type === 'done' || event.type === 'error') {
      setTurn({ ...turnRef.current, status: event.type === 'harness_status' ? event.text || null : null });
      if (event.type !== 'error') return;
    }
    if (event.type === 'tool_result' && event.ok && event.name === 'prompt_library') reloadPrompts();
    if (event.type === 'tool_result' && event.ok && event.name === 'build') liveWorldRef.current.wake();
    setItems((prev) => applyEvent(prev, event));
  }

  function createConversation() {
    return api('/api/conversations', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ provider, model }),
    });
  }

  // The new chat's row on the server: made once, when New chat or a send
  // first needs it, and the new chat's for as long as it is new (however
  // often the view is left and come back to), so no second row is made.
  const newChatRef = useRef(null); // { request, id }: `id` once the row exists
  function createdChat() {
    if (!newChatRef.current) {
      const made = { request: createConversation(), id: null };
      newChatRef.current = made;
      made.request.then((data) => {
        if (newChatRef.current !== made) return;
        made.id = data.id;
        // The view takes the row at once: the live world opens on it.
        if (placeRef.current.kind === 'new-chat') setPlace(withRow(placeRef.current, data.id));
      }, () => { if (newChatRef.current === made) newChatRef.current = null; });
    }
    return newChatRef.current.request;
  }

  // The new chat got its first message: it is the chat `id` from here on,
  // and the next new chat is another, with its own row.
  function settleNewChat(id) {
    if (newChatRef.current?.id !== id) return;
    newChatRef.current = null;
    if (placeRef.current.kind === 'new-chat') setPlace(asChat(placeRef.current));
  }

  // The new chat exists on the server from the start (the live world opens
  // on it), but the view switches before the request returns.
  async function newConversation() {
    if (!provider || !model) throw new Error('Choose a model before starting a chat');
    // Already on an untouched new chat.
    if (placeRef.current.kind === 'new-chat' && transcriptState.phase === 'shown' && !items.length && !busy) {
      composerRef.current?.focus();
      return;
    }
    showNewChat();
    await createdChat();
  }

  // The chat leaves the list when the server archives it, and the view on it
  // moves on with it (receiveConversations).
  async function archiveConversation(id, evt) {
    evt.stopPropagation();
    await api(`/api/conversations/${id}/archive`, { method: 'POST' });
    readArchiveAgain();
  }

  // ---- Archive ----

  // The archived counts from the list stream. An open archived section whose
  // mode has none left closes.
  function receiveArchivedCounts(counts) {
    setArchivedCounts(counts);
    const open = archiveRef.current;
    if (open.phase !== 'closed' && counts[open.mode === 'bench' ? 'benches' : 'conversations'] === 0) {
      archiveReadsRef.current += 1;
      setArchive(archiveClosed(open));
    }
  }

  function loadArchive(mode) {
    archiveReadsRef.current += 1;
    const read = archiveReadsRef.current;
    setArchive(archiveReading(archiveRef.current, mode));
    api('/api/archive').then((data) => {
      if (archiveReadsRef.current === read) setArchive(archiveRead(archiveRef.current, data));
    }, (error) => {
      if (archiveReadsRef.current === read) setArchive(archiveFailed(archiveRef.current, error));
    }).catch(showError);
  }

  // The archived list is read when it opens and after every change to it.
  const readArchiveAgain = () => { if (archiveRef.current.phase !== 'closed') loadArchive(archiveRef.current.mode); };

  function toggleArchive(mode) {
    if (archiveRef.current.phase === 'closed' || archiveRef.current.mode !== mode) return loadArchive(mode);
    archiveReadsRef.current += 1;
    setArchive(archiveClosed(archiveRef.current));
  }

  const archivePath = (mode, id) => `/api/${mode === 'bench' ? 'benches' : 'conversations'}/${id}`;

  async function restoreArchived(mode, id) {
    const navigation = navigationRef.current;
    await api(`${archivePath(mode, id)}/restore`, { method: 'POST' });
    readArchiveAgain();
    restoredRef.current = { mode, id, navigation };
    openRestored();
  }

  // The restored item opens once the list has it (it may come before or
  // after the restore's answer), unless the user went elsewhere meanwhile.
  function openRestored() {
    const restored = restoredRef.current;
    if (!restored) return;
    if (navigationRef.current !== restored.navigation) {
      restoredRef.current = null;
      return;
    }
    const rows = restored.mode === 'bench' ? benchesRef.current : conversationsRef.current;
    if (!rows.some((row) => row.id === restored.id)) return;
    restoredRef.current = null;
    if (restored.mode === 'bench') showBenchView({ kind: 'compare', benchId: restored.id });
    else openConversation(restored.id).catch(showError);
  }

  async function deleteArchived(mode, id) {
    await api(archivePath(mode, id), { method: 'DELETE' });
    readArchiveAgain();
  }

  // ---- Bench ----

  const roundFor = (benchId) => benchRoundsRef.current[benchId] ?? benchesRef.current.find((bench) => bench.id === benchId)?.rounds ?? 1;
  const postJson = (path, body) => api(path, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });

  // Every Bench view goes through here. A comparison keeps the round it
  // shows until the user picks another.
  function showBenchView(view) {
    if (view.kind === 'member') {
      openConversation(view.memberId, view).catch(showError);
      return;
    }
    leaveConversation();
    if (view.kind === 'compare') {
      showRound(view.benchId, roundFor(view.benchId));
      // Details that failed to load are read again by clicking the bench.
      if (benchDetailsRef.current[view.benchId]?.phase === 'failed') loadBenchDetails(view.benchId);
    }
    arrive(view);
    clearTranscript();
  }

  // The round a comparison shows, and so the world its viewer shows.
  function showRound(benchId, round) {
    benchRoundsRef.current = { ...benchRoundsRef.current, [benchId]: round };
    setBenchRounds(benchRoundsRef.current);
  }

  // Bench, starting from what the composer holds here: the prompt moves to
  // the new bench and leaves Build's draft empty. Selections belong to one
  // build. A prompt already typed for the new bench stays, and Build's then
  // stays in Build.
  function benchFromBuild() {
    const key = where().draftKey;
    const moved = { ...currentDraft(), selections: [], choice: null };
    const moves = !draftEmpty(moved) && draftEmpty(draftsRef.current.peek(NEW_BENCH_DRAFT));
    if (moves) draftsRef.current.put(NEW_BENCH_DRAFT, moved);
    goBench({ kind: 'new-bench' });
    if (moves) draftsRef.current.put(key, EMPTY_DRAFT);
    requestAnimationFrame(() => composerRef.current?.focus());
  }

  // The new bench's way to Build: the new chat, taking the bench's text and
  // files when its own draft is empty. The bench keeps its models and Blind.
  function buildFromBench() {
    const key = where().draftKey;
    const moved = { ...currentDraft(), selections: [], choice: null };
    const moves = !draftEmpty(moved) && draftEmpty(draftsRef.current.peek(NEW_CHAT_DRAFT));
    if (moves) draftsRef.current.put(NEW_CHAT_DRAFT, moved);
    showNewChat();
    if (moves) draftsRef.current.put(key, EMPTY_DRAFT);
    requestAnimationFrame(() => composerRef.current?.focus());
  }

  // The two modes. Bench keeps its own place; Build comes back to the chat
  // it had. The picker's model is Build's, and no Bench place changes it.
  function switchMode(mode) {
    if (mode === where().mode) return;
    if (mode === 'bench') goBench();
    else goBuild(leftRef.current.build.id);
  }

  function goBench(view = leftRef.current.bench) {
    showBenchView(view);
  }

  // Build on the chat `id` while the list has it; if not, on the new chat,
  // with the draft and model it was left with.
  function goBuild(id) {
    if (id && conversationsRef.current.some((row) => row.id === id && !row.benchId)) {
      openConversation(id).catch(showError);
      return;
    }
    showNewChat();
  }
  // Every bind runs through here; Settings changes what each action is bound to.
  const bindActionsRef = useRef(null);
  bindActionsRef.current = {
    build: () => switchMode('build'),
    bench: () => switchMode('bench'),
    search: () => setCommand((open) => (open ? null : { query: '', active: null })),
    new: () => (where().mode === 'bench' ? showBenchView({ kind: 'new-bench' }) : newConversation().catch(showError)),
    sidebar: () => setSidebarOpen((open) => !open),
    select: () => setSelectToggle((count) => count + 1),
  };
  const bindsRef = useRef(binds);
  bindsRef.current = binds;
  // Runs the action bound to a combination; the viewer passes on its own key
  // presses. One held back by an open layer (layers.mjs) is still the app's key.
  const runBind = (combo) => {
    const action = combo && Object.keys(bindsRef.current).find((id) => bindsRef.current[id] === combo);
    if (action && bindRuns(action, layersRef.current)) bindActionsRef.current[action]();
    return Boolean(action);
  };
  useEffect(() => {
    const onKey = (evt) => { if (runBind(comboOf(evt))) evt.preventDefault(); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  // Search everything (the search shortcut): one panel over the app. Builds
  // and benches come from the server's full-text index; prompts, settings
  // and actions are matched here.
  const [commandFound, setCommandFound] = useState(null); // { build, bench } results for the query
  const commandInputRef = useRef(null);
  const commandListRef = useRef(null);
  // Focused before the panel paints, so no key typed right after the shortcut is lost.
  useLayoutEffect(() => { if (command) commandInputRef.current.focus(); }, [Boolean(command)]);
  useEffect(() => {
    const query = command?.query.trim();
    setCommandFound(null);
    if (!query) return undefined;
    let current = true;
    const timer = setTimeout(() => {
      Promise.all(['build', 'bench'].map((scope) => api(`/api/search?scope=${scope}&q=${encodeURIComponent(query)}`)))
        .then(([build, bench]) => { if (current) setCommandFound({ build: build.results, bench: bench.results }); })
        .catch(showError);
    }, 90);
    return () => { current = false; clearTimeout(timer); };
  }, [command?.query]);
  // The key of the item the panel highlights, as last rendered. The
  // highlight stays on that item, wherever groups arriving later put it.
  const commandShownRef = useRef(null);
  useEffect(() => {
    if (command && command.active !== commandShownRef.current) setCommand((current) => current && { ...current, active: commandShownRef.current });
  });
  useEffect(() => {
    commandListRef.current?.querySelector('.cmdk-item.active')?.scrollIntoView({ block: 'nearest' });
  }, [command?.active]);

  // Opens Settings on a pane, and on a General row, scrolls to it and lights it up.
  async function openSetting(view, row) {
    await openSettings(view);
    if (!row) return;
    requestAnimationFrame(() => requestAnimationFrame(() => {
      const element = document.querySelector(`.settings-pane [data-setting="${CSS.escape(row)}"]`);
      element.scrollIntoView({ block: 'center' });
      element.classList.add('flash');
      setTimeout(() => element.classList.remove('flash'), 1400);
    }));
  }

  // The query's words, each matched on its own and in any order.
  const commandWords = (query) => query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const escapeRegExp = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

  // How well an item matches every word: the start of a word in its title
  // counts most, then anywhere in its title, then its other text. Null when
  // some word matches nowhere.
  function matchScore(words, title, ...others) {
    let total = 0;
    for (const word of words) {
      const best = Math.max(...[title, ...others].map((text, field) => {
        const lower = (text || '').toLowerCase();
        const at = lower.indexOf(word);
        if (at < 0) return 0;
        return (field === 0 ? 2 : 1) * (at === 0 || /[^a-z0-9]/.test(lower[at - 1]) ? 2 : 1);
      }));
      if (!best) return null;
      total += best;
    }
    return total;
  }

  // Every query word in `text`, marked.
  function markWords(text, words) {
    if (!words.length) return text;
    return text.split(new RegExp(`(${words.map(escapeRegExp).join('|')})`, 'ig'))
      .map((part, index) => (index % 2 ? html`<mark>${part}</mark>` : part));
  }

  // A prompt's text around the first query word in it, or its first line.
  function excerpt(text, words) {
    const lower = text.toLowerCase();
    const at = Math.min(...words.map((word) => lower.indexOf(word)).filter((index) => index >= 0));
    if (!Number.isFinite(at)) return text.split('\n')[0];
    const from = at > 40 ? text.lastIndexOf(' ', at - 30) + 1 : 0;
    return html`${from ? '…' : ''}${markWords(text.slice(from, at + 200), words)}`;
  }

  function commandGroups() {
    const words = commandWords(command.query);
    const searching = words.length > 0;
    const groups = [];
    const add = (title, items, limit) => { if (items.length) groups.push({ title, items: items.slice(0, limit) }); };
    // Items matched here, best first; `keywords` are searched but not shown.
    const rank = (items) => (searching
      ? items.map((item) => ({ item, score: matchScore(words, item.title, item.search, item.keywords) }))
        .filter((entry) => entry.score !== null)
        .sort((a, b) => b.score - a.score)
        .map((entry) => entry.item)
      : items);

    const builds = searching
      ? (commandFound?.build || []).map((result) => ({ row: conversations.find((row) => row.id === result.id && !row.benchId), snippet: result.snippet })).filter((entry) => entry.row)
      : conversations.filter((row) => !row.benchId).map((row) => ({ row, snippet: null }));
    add(searching ? 'Builds' : 'Recent builds', builds.map(({ row, snippet }) => ({
      key: `build:${row.id}`, icon: 'build', title: row.title, detail: snippet && marked(snippet), run: () => goBuild(row.id),
    })), searching ? 6 : 4);
    const benchList = searching
      ? (commandFound?.bench || []).map((result) => ({ bench: benches.find((bench) => bench.id === result.id), snippet: result.snippet })).filter((entry) => entry.bench)
      : benches.map((bench) => ({ bench, snippet: null }));
    add(searching ? 'Benches' : 'Recent benches', benchList.map(({ bench, snippet }) => ({
      key: `bench:${bench.id}`, icon: 'bench', title: bench.title, detail: snippet && marked(snippet),
      run: () => goBench({ kind: 'compare', benchId: bench.id }),
    })), searching ? 5 : 3);

    if (searching) {
      add('Prompts', rank(prompts.map((prompt) => ({
        key: `prompt:${prompt.id}`, icon: 'prompts', title: prompt.title || 'Untitled prompt',
        search: prompt.text, keywords: (prompt.tags || []).join(' '),
        detail: excerpt(prompt.text || '', words), run: () => openPrompt(prompt.id),
      }))), 4);
    }

    const other = appMode === 'build' ? 'bench' : 'build';
    const actions = [
      { key: 'new-chat', icon: 'compose', title: 'New chat', keywords: 'build create start', hint: appMode === 'build' ? binds.new : null,
        run: () => { if (where().mode === 'build') newConversation().catch(showError); else goBuild(null); } },
      { key: 'new-bench', icon: 'bench', title: 'New bench', keywords: 'compare models create start', hint: appMode === 'bench' ? binds.new : null, run: () => goBench({ kind: 'new-bench' }) },
      { key: 'switch', icon: 'swap', title: `Switch to ${other === 'build' ? 'Build' : 'Bench'}`, keywords: 'mode build bench', hint: binds[other], run: () => switchMode(other) },
      { key: 'leaderboard', icon: 'trophy', title: 'Leaderboard', keywords: 'votes ranking ratings blind', run: () => goBench({ kind: 'leaderboard' }) },
      { key: 'sidebar', icon: 'panel', title: sidebarOpen ? 'Hide the sidebar' : 'Show the sidebar', keywords: 'panel toggle', hint: binds.sidebar, run: () => setSidebarOpen((open) => !open) },
      { key: 'theme', icon: theme === 'dark' ? 'sun' : 'moon', title: theme === 'dark' ? 'Light theme' : 'Dark theme', keywords: 'appearance colors dark light', run: () => setTheme(theme === 'dark' ? 'light' : 'dark') },
      { key: 'settings', icon: 'settings', title: 'Settings', keywords: 'preferences options', run: () => openSettings().catch(showError) },
    ];
    add('Actions', rank(actions), searching ? 5 : actions.length);

    if (searching) {
      const settingRows = [
        ['Title model', 'General', 'chat names naming'],
        ['Theme', 'General', 'appearance colors dark light'],
        ['Clear leaderboard', 'Danger zone', 'votes reset delete'],
        ...BIND_ACTIONS.map(([, label]) => [label, 'Shortcuts', 'keybind bind key hotkey']),
      ].map(([title, section, keywords]) => ({
        key: `setting:${title}`, icon: 'settings', title, detail: `Settings · ${section}`, search: `Settings ${section}`, keywords,
        run: () => openSetting('general', title).catch(showError),
      }));
      const providerRows = Object.entries(providers).map(([id, provider]) => {
        const section = provider.harness ? 'Harnesses' : 'Providers';
        return {
          key: `provider:${id}`, logo: id, title: provider.label, detail: `Settings · ${section}`, search: `Settings ${section}`, keywords: 'api key login account',
          run: () => openSetting(id, null).catch(showError),
        };
      });
      add('Settings', rank([...settingRows, ...providerRows]), 5);
    }
    return groups;
  }

  function renderCommand() {
    const needle = command.query.trim();
    const groups = commandGroups();
    const items = groups.flatMap((group) => group.items);
    const keys = items.map((item) => item.key);
    const active = highlighted(keys, command.active);
    commandShownRef.current = active;
    const chosen = items.find((item) => item.key === active);
    const run = (item) => { setCommand(null); item.run(); };
    const move = (step) => setCommand((current) => ({ ...current, active: stepped(keys, active, step) }));
    const highlight = (title) => markWords(title, commandWords(command.query));
    return html`
      <div class="cmdk-overlay" onMouseDown=${(evt) => { if (evt.target === evt.currentTarget) setCommand(null); }}>
        <div class="cmdk" role="dialog" aria-label="Search everything">
          <label class="cmdk-input">
            <${Icon} name="search" size="17" />
            <input
              ref=${commandInputRef}
              value=${command.query}
              placeholder="Search builds, benches, prompts and settings"
              aria-label="Search everything"
              onInput=${(evt) => setCommand({ query: evt.target.value, active: null })}
              onKeyDown=${(evt) => {
                if (evt.key === 'ArrowDown' && items.length) { evt.preventDefault(); move(1); }
                else if (evt.key === 'ArrowUp' && items.length) { evt.preventDefault(); move(-1); }
                else if (evt.key === 'Enter' && chosen) { evt.preventDefault(); run(chosen); }
              }}
            />
            <kbd>esc</kbd>
          </label>
          <div class="cmdk-list" role="listbox" ref=${commandListRef}>
            ${!items.length && html`<div class="cmdk-empty">${commandFound ? `Nothing matches “${needle}”` : 'Searching…'}</div>`}
            ${groups.map((group) => html`
              <div class="cmdk-group" key=${group.title} role="group" aria-label=${group.title}>
                <div class="cmdk-heading">${group.title}</div>
                ${group.items.map((item) => html`
                    <div
                      key=${item.key}
                      role="option"
                      aria-selected=${item.key === active}
                      class="cmdk-item ${item.key === active ? 'active' : ''}"
                      onMouseMove=${() => { if (item.key !== active) setCommand((current) => ({ ...current, active: item.key })); }}
                      onClick=${() => run(item)}
                    >
                      <span class="cmdk-icon">${item.logo ? html`<${Logo} name=${item.logo} size="15" />` : html`<${Icon} name=${item.icon} size="15" />`}</span>
                      <span class="cmdk-text">
                        <span class="cmdk-title">${highlight(item.title)}</span>
                        ${item.detail && html`<span class="cmdk-detail">${typeof item.detail === 'string' ? highlight(item.detail) : item.detail}</span>`}
                      </span>
                      ${item.hint && html`<kbd class="cmdk-hint">${formatBind(item.hint)}</kbd>`}
                    </div>
                  `)}
              </div>
            `)}
          </div>
        </div>
      </div>
    `;
  }

  // Searches run on the server's full-text index, debounced, newest wins.
  useEffect(() => {
    const query = find.trim();
    if (!query) { setFound(null); return undefined; }
    let current = true;
    const timer = setTimeout(() => {
      api(`/api/search?scope=${appMode}&q=${encodeURIComponent(query)}`)
        .then((data) => { if (current) setFound(data.results); })
        .catch(showError);
    }, 90);
    return () => { current = false; clearTimeout(timer); };
  }, [find, appMode]);
  useEffect(() => { setFind(''); }, [appMode]);

  // What the composer would send for a bench, taken out of it, or null.
  // A request that fails puts it back (putBack).
  function takeMessage() {
    const message = composedMessage();
    const key = where().draftKey;
    if (!message || (!message.text && attachments.length === 0) || restoringRef.current === key || draftRef.current.reading.length) return null;
    const draft = { ...currentDraft(), segments: message.segments };
    applyDraft(EMPTY_DRAFT);
    return { text: message.text, draft, key, navigation: navigationRef.current };
  }

  async function runBench() {
    if (benchModels.length < 2) return;
    const sent = takeMessage();
    if (!sent) return;
    let id;
    try {
      ({ id } = await postJson('/api/benches', { text: sent.text, files: sent.draft.attachments, members: benchModels, blind: benchBlind, resolution: benchResolution }));
    } catch (error) {
      putBack(sent.key, sent.draft);
      throw error;
    }
    setOpenBenches((current) => new Set([...current, id]));
    if (navigationRef.current === sent.navigation) showBenchView({ kind: 'compare', benchId: id });
  }

  async function sendRound(benchId) {
    const sent = takeMessage();
    if (!sent) return;
    let rounds;
    try {
      ({ rounds } = await postJson(`/api/benches/${benchId}/rounds`, { text: sent.text, files: sent.draft.attachments }));
    } catch (error) {
      putBack(sent.key, sent.draft);
      throw error;
    }
    showRound(benchId, rounds);
  }

  // The bench leaves the list when the server archives it, and the view on
  // it moves on with it (receiveBenches).
  async function archiveBench(benchId, evt) {
    evt.stopPropagation();
    await api(`/api/benches/${benchId}/archive`, { method: 'POST' });
    readArchiveAgain();
  }

  // The model's chat continues in Build from its last finished round. One
  // fork at a time: its button waits for the answer.
  const [forking, setForking] = useState(false);
  async function forkToBuild(member) {
    const navigation = navigationRef.current;
    setForking(true);
    let row;
    try {
      row = await postJson(`/api/conversations/${member.id}/fork`, { round: member.runningSince ? member.round - 1 : member.round });
    } finally {
      setForking(false);
    }
    receiveConversations([row, ...conversationsRef.current.filter((entry) => entry.id !== row.id)]);
    if (navigationRef.current === navigation) goBuild(row.id);
  }

  function focusBuild(memberId) {
    const live = minecraftLiveRef.current;
    const entry = live.conversationId === where().viewKey && live.layout?.find((item) => item.id === memberId);
    const lift = (corner) => [corner[0], corner[1] + live.yOffset, corner[2]];
    if (entry) setViewerFocus({ min: lift(entry.min), max: lift(entry.max), at: Date.now() });
  }

  // A round's prompt shows that round with every model's build in view.
  function showWholeRound(benchId, round) {
    if (window.getSelection().toString()) return; // selecting the prompt's text
    if (round !== roundFor(benchId)) return showRound(benchId, round);
    const live = minecraftLiveRef.current;
    if (live.conversationId !== where().viewKey || !live.bounds) return;
    const lift = (corner) => [corner[0], corner[1] + live.yOffset, corner[2]];
    setViewerFocus({ min: lift(live.bounds.min), max: lift(live.bounds.max), at: Date.now() });
  }

  // An attachment stored on the server, with its bytes, to be sent again.
  async function withData(file) {
    const response = await fetch(file.url);
    if (!response.ok) throw new Error(`Could not load "${file.name}" (HTTP ${response.status})`);
    return { ...file, data: await readBlobAsDataUrl(await response.blob()) };
  }

  // An edit's prompt, as the server gave it back, lands in its draft
  // (endRestore). Once the server has reverted it always does, wherever the
  // user is by then: without its files, if they could not be read.
  async function restorePrompt(data, selections) {
    let files = [];
    try {
      files = await Promise.all(data.attachments.map(withData));
    } finally {
      endRestore({ ...EMPTY_DRAFT, segments: data.text ? [{ type: 'text', text: data.text }] : [], attachments: files, selections });
    }
  }

  // Editing a round takes every model back to before it; the prompt returns
  // to the bench's draft, files and all at once, and sending it starts a new
  // branch from there. Sending waits until it is back.
  async function editRound(benchId, roundId) {
    beginRestore(benchDraft(benchId));
    let data;
    try {
      data = await postJson(`/api/benches/${benchId}/revert`, { roundId });
    } catch (error) {
      endRestore(null);
      throw error;
    }
    showRound(benchId, data.rounds);
    await restorePrompt(data, []);
  }

  // A bench model's name, or its letter while the bench hides names.
  function nameOf(row) {
    const bench = benches.find((entry) => entry.id === row.benchId);
    return hidesNames(bench) ? `Build ${benchLetter(bench, row.id)}` : memberName(row, conversations);
  }

  // A bench's rounds, stats and votes. A read asked for later supersedes one
  // still on its way, so an older answer never undoes a newer vote.
  // What was read before stays on show while a read is on its way, and
  // after one that failed, which the comparison says and can try again.
  function loadBenchDetails(benchId) {
    const reads = detailsRef.current;
    reads.next += 1;
    const read = reads.next;
    reads.latest[benchId] = read;
    const latest = () => detailsRef.current.latest[benchId] === read;
    setBenchDetails(benchId, detailsReading(benchDetailsRef.current[benchId]));
    api(`/api/benches/${benchId}`).then((data) => {
      if (latest()) setBenchDetails(benchId, detailsRead(benchDetailsRef.current[benchId], data));
    }, (error) => {
      if (latest()) setBenchDetails(benchId, detailsFailed(benchDetailsRef.current[benchId], error));
    }).catch(showError);
  }

  async function voteRound(benchId, roundId, pick) {
    const { votes } = await postJson(`/api/benches/${benchId}/vote`, { roundId, pick });
    setBenchDetails(benchId, detailsVotes(benchDetailsRef.current[benchId], votes));
    loadBenchDetails(benchId);
  }

  // A branch switch on its way, of a chat or of a bench's rounds: the arrows
  // wait for its answer, so no second switch meets the first on the server.
  const [branching, setBranching] = useState(false);

  async function switchRoundBranch(benchId, roundId) {
    setBranching(true);
    try {
      const { round } = await postJson(`/api/benches/${benchId}/branch`, { roundId });
      showRound(benchId, round);
    } finally {
      setBranching(false);
    }
  }

  const viewedBench = appMode === 'bench' && place.benchId ? benches.find((bench) => bench.id === place.benchId) || null : null;
  const viewedMembers = viewedBench ? conversations.filter((row) => row.benchId === viewedBench.id).sort((a, b) => viewedBench.order.indexOf(a.id) - viewedBench.order.indexOf(b.id)) : [];
  const viewedDetails = viewedBench && benchDetails[viewedBench.id]?.data;
  const viewedDetailsError = viewedBench && benchDetails[viewedBench.id]?.phase === 'failed' ? benchDetails[viewedBench.id].error : null;
  // Rounds and per-round stats, read again whenever a model starts or ends a turn.
  const membersState = viewedMembers.map((row) => `${row.id}:${row.round}:${row.runningSince}:${row.halted}`).join();
  useEffect(() => {
    if (viewedBench) loadBenchDetails(viewedBench.id);
  }, [viewedBench?.id, viewedBench?.rounds, viewedBench?.head, viewedBench?.revealed, membersState]);

  useEffect(() => {
    if (place.kind !== 'leaderboard') return;
    api('/api/leaderboard').then((data) => setLeaderboard(data.entries)).catch(showError);
  }, [place]);

  // Looking at the comparison is looking at every model's result.
  useEffect(() => {
    if (place.kind !== 'compare' || !pageVisible) return;
    for (const row of viewedMembers) if (row.unread) markSeen(row.id);
  }, [place, viewedMembers.map((row) => `${row.id}:${row.unread}`).join(), pageVisible]);

  const viewerLabels = useMemo(() => {
    if (!viewKey?.startsWith('bench:') || minecraftLive.conversationId !== viewKey || !minecraftLive.layout) return [];
    return minecraftLive.layout.map((entry) => {
      const row = conversations.find((item) => item.id === entry.id);
      return {
        text: row ? nameOf(row) : '',
        x: (entry.min[0] + entry.max[0] + 1) / 2,
        y: entry.max[1] + 2 + minecraftLive.yOffset,
        z: (entry.min[2] + entry.max[2] + 1) / 2,
      };
    });
  }, [viewKey, minecraftLive.conversationId, minecraftLive.layout, minecraftLive.yOffset, conversations]);

  useEffect(() => { localStorage.setItem('mcb-open-benches', JSON.stringify([...openBenches])); }, [openBenches]);
  useEffect(() => { localStorage.setItem('mcb-bench-models', JSON.stringify(benchModels)); }, [benchModels]);

  // A box picked in the viewer joins the message as the next free letter,
  // named by what it mostly holds and where it sits in the build.
  async function addSelection(box) {
    const id = where().chatId;
    const navigation = navigationRef.current;
    const offset = minecraftLiveRef.current.yOffset;
    const min = [box.min[0], box.min[1] - offset, box.min[2]];
    const max = [box.max[0], box.max[1] - offset, box.max[2]];
    const { blocks, block, label } = await postJson(`/api/conversations/${id}/selection`, { min, max });
    if (navigationRef.current !== navigation) return;
    setSelections((current) => {
      const used = new Set(current.map((selection) => selection.letter));
      const letter = [...'ABCDEFGHIJKLMNOPQRSTUVWXYZ'].find((candidate) => !used.has(candidate));
      return letter ? [...current, { letter, label, min, max, blocks, block }] : current;
    });
  }

  function removeSelection(letter) {
    setSelections((current) => current.filter((entry) => entry.letter !== letter));
  }

  // The draft's boxes stay drawn while you write; hovering a chip, here or on
  // a sent message, lights its box up.
  const viewerSelections = useMemo(() => {
    if (appMode !== 'build' || !activeId || minecraftLive.yOffset == null) return [];
    const lift = (corner) => [corner[0], corner[1] + minecraftLive.yOffset, corner[2]];
    const drawn = selections.map((selection) => ({
      key: `draft-${selection.letter}`, label: true, icon: blockIconUrl(selection.block), min: lift(selection.min), max: lift(selection.max),
      lit: hoveredSelection?.draft === true && hoveredSelection.letter === selection.letter, removable: true,
    }));
    if (hoveredSelection && !hoveredSelection.draft) {
      // A sent selection shows only its box while its chip is hovered.
      drawn.push({ key: 'sent', label: false, min: lift(hoveredSelection.min), max: lift(hoveredSelection.max), lit: true, removable: false });
    }
    return drawn;
  }, [appMode, activeId, minecraftLive.yOffset, selections, hoveredSelection]);

  // A send before the app is ready is held, and goes once it is: a build
  // started earlier finds no engine to build with. A Build send with no
  // model goes on to say so.
  function submit() {
    const at = placeRef.current;
    if (preparing && !(where().mode === 'build' && (!provider || !model))) {
      if (hasText || attachments.length) setHeld(where().draftKey);
      return Promise.resolve();
    }
    if (where().mode === 'build') return send();
    if (at.kind === 'new-bench') return runBench();
    return sendRound(at.benchId);
  }

  // Files picked for the composer are part of its draft from the pick: the
  // draft is `reading` them until they are read, and is not sent meanwhile.
  // They count toward its limits at once.
  async function addFiles(files) {
    checkAttachmentLimits(draftFiles(draftRef.current), files);
    const batch = files.map((file) => ({ name: file.name, size: file.size }));
    // The ref has the read before the next render, so a send in between sees it.
    draftRef.current = beginRead(draftRef.current, batch);
    setReading(draftRef.current.reading);
    const read = [];
    try {
      for (const file of files) read.push(await readUpload(file));
    } catch (error) {
      finishRead(batch, []);
      throw error;
    }
    finishRead(batch, read);
  }

  // A batch's read is over: its files take its place in the draft that holds
  // it, on screen or parked, wherever the user is by now. A draft that is
  // gone by then (its chat archived, another prompt put in its place) took
  // the batch with it.
  function finishRead(batch, files) {
    if (holdsRead(draftRef.current, batch)) {
      draftRef.current = endRead(draftRef.current, batch, files);
      setAttachments(draftRef.current.attachments);
      setReading(draftRef.current.reading);
    }
    draftsRef.current.endRead(batch, files);
  }

  function onPaste(evt) {
    try {
      const message = readMessageClipboard(evt.clipboardData);
      if (message) {
        evt.preventDefault();
        checkAttachmentLimits(draftFiles(draftRef.current), message.attachments);
        document.execCommand('insertText', false, message.text);
        setAttachments((prev) => [...prev, ...message.attachments]);
        return;
      }
    } catch (error) {
      evt.preventDefault();
      showError(error);
      return;
    }
    const files = [...(evt.clipboardData?.items || [])]
      .filter((item) => item.kind === 'file')
      .map((item) => item.getAsFile())
      .filter(Boolean);
    evt.preventDefault();
    if (files.length) addFiles(files).catch(showError);
    // Only plain text enters the composer, never pasted formatting.
    else document.execCommand('insertText', false, evt.clipboardData.getData('text/plain'));
  }

  // A Stop while the message is still being sent is kept for send(): before
  // the POST it cancels the send, after it the server's turn is stopped.
  function stop() {
    const current = turnRef.current;
    if (current.phase === 'sending') {
      stopAskedRef.current = current.controller;
      return;
    }
    if (activeId) api(`/api/conversations/${activeId}/stop`, { method: 'POST' }).catch(showError);
  }

  // Sends the composer's message as a turn of the chat on screen (created
  // first for a new chat). The turn is `sending` from the first moment, so
  // a second Enter, or New chat's creation, never makes a second chat. A
  // message the server never took goes back to be sent again; once it took
  // it, the turn is the server's and the page only reads it.
  async function send() {
    const key = where().draftKey;
    const row = conversationsRef.current.find((entry) => entry.id === where().chatId);
    if (turnRunning(turnRef.current, row) || restoringRef.current === key || draftRef.current.reading.length) return;
    const message = composedMessage();
    if (!message || (!message.text && attachments.length === 0)) return;
    if (!provider || !model) {
      showError(new Error(noAI ? 'Connect an AI in Settings first' : 'Choose a model before sending'));
      return;
    }
    const { text } = message;
    const draft = { ...currentDraft(), segments: message.segments };
    const controller = new AbortController();
    setTurn({ phase: 'sending', controller, status: null });
    applyDraft(EMPTY_DRAFT);
    prevScrollHeightRef.current = 0; // sending always jumps to the live tail
    const shown = { kind: 'user', text, attachments: draft.attachments, selections: draft.selections };
    setItems((prev) => [...prev, shown]);

    let conversationId = where().chatId;
    let res;
    try {
      if (!conversationId) {
        // The view took the row when it landed (createdChat).
        conversationId = (await createdChat()).id;
        controller.signal.throwIfAborted();
      }
      if (stopAskedRef.current === controller) {
        stopAskedRef.current = null;
        putBack(key, draft);
        setItems((prev) => prev.filter((item) => item !== shown));
        finishTurn(controller, conversationId, null);
        return;
      }
      res = await fetch(`/api/conversations/${conversationId}/chat`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ text, provider, model, files: draft.attachments, selections: draft.selections, effort: modelCaps ? effortFor(modelCaps, effort) : effort, resolution }),
        signal: controller.signal,
      });
      if (!res.ok) throw new Error((await res.json()).error || `HTTP ${res.status}`);
    } catch (error) {
      putBack(key, draft);
      if (controller.signal.aborted) return;
      showError(error);
      setItems((prev) => prev.filter((item) => item !== shown));
      if (conversationId) finishTurn(controller, conversationId, null);
      else setTurn(IDLE);
      return;
    }
    settleNewChat(conversationId);
    setTurn({ phase: 'reading', id: conversationId, controller, status: null });
    if (stopAskedRef.current === controller) {
      stopAskedRef.current = null;
      api(`/api/conversations/${conversationId}/stop`, { method: 'POST' }).catch(showError);
    }
    await readTurn(res, conversationId, controller, null);
  }

  async function copyMessage(item) {
    await writeMessageClipboard(item);
    setCopied(item);
    setTimeout(() => setCopied((current) => (current === item ? null : current)), 1200);
  }

  // Edit from an earlier prompt without deleting its existing branch. The
  // prompt comes back to the chat's draft with its files and selections at
  // once; sending waits until it has. Leaving the chat meanwhile changes
  // nothing of that: the prompt is then there to come back to.
  async function editFrom(messageId) {
    const conversationId = activeId;
    const navigation = navigationRef.current;
    beginRestore(where().draftKey);
    let data;
    try {
      data = await api(`/api/conversations/${conversationId}/revert`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ messageId }),
      });
    } catch (error) {
      endRestore(null);
      throw error;
    }
    transcriptsRef.current.delete(conversationId);
    liveWorldRef.current.wake();
    if (navigationRef.current === navigation) {
      setItems(itemsFromMessages(data.messages));
      setMessageTree(data.messageTree);
    } else if (where().chatId === conversationId) {
      // Left and come back to: what is on screen may be from before the revert.
      resync(conversationId).catch(showError);
    }
    await restorePrompt(data, data.selections);
  }

  async function switchBranch(messageId) {
    const conversationId = activeId;
    if (!conversationId || busy) return;
    const navigation = navigationRef.current;
    setBranching(true);
    let data;
    try {
      data = await api(`/api/conversations/${conversationId}/branch`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ messageId }),
      });
    } finally {
      setBranching(false);
    }
    liveWorldRef.current.wake();
    transcriptsRef.current.delete(conversationId);
    if (navigationRef.current !== navigation) return;
    prevScrollHeightRef.current = 0;
    setItems(itemsFromMessages(data.messages));
    setMessageTree(data.messageTree);
  }

  // Prompt palette over the composer: null when closed. `slash` is the "/query"
  // that opened it (composer.slashAtCaret); null when the button opened it. A
  // rewrite of the composer closes it (setComposer), so its text node is live.
  const [palette, setPalette] = useState(null);
  const composerRef = useRef(null);
  const composerBoxRef = useRef(null);
  const caretRef = useRef(null);

  useEffect(() => {
    if (!palette) return undefined;
    const close = (evt) => { if (!composerBoxRef.current?.contains(evt.target)) setPalette(null); };
    document.addEventListener('mousedown', close);
    return () => document.removeEventListener('mousedown', close);
  }, [Boolean(palette)]);

  // Typing: keep the text and segments, follow a "/query" with the palette,
  // and drop the files of a prompt token that was deleted.
  function onComposerChange({ text, segments }) {
    const before = new Set(segmentsRef.current.filter((segment) => segment.type === 'prompt').map((segment) => segment.id));
    const after = new Set(segments.filter((segment) => segment.type === 'prompt').map((segment) => segment.id));
    const dropped = [...before].filter((id) => !after.has(id));
    if (dropped.length) setAttachments((current) => current.filter((file) => !dropped.includes(file.fromPrompt)));
    segmentsRef.current = segments;
    setHasText(Boolean(text.trim()));
    const slash = composer.slashAtCaret(composerRef.current);
    if (slash) setPalette({ query: slash.query, index: 0, slash, withSearch: false });
    else if (palette?.slash) setPalette(null);
  }

  // What a send carries, or null while a fill-in box is still empty: that box
  // takes the focus and flashes instead.
  function composedMessage() {
    const { text, segments, emptyField } = composer.read(composerRef.current);
    if (emptyField) {
      emptyField.parentNode.classList.remove('missing');
      void emptyField.offsetWidth;
      emptyField.parentNode.classList.add('missing');
      emptyField.focus();
      return null;
    }
    return { text: text.trim(), segments };
  }

  // A picked prompt replaces the "/query" that summoned it (or lands at the
  // caret): as a token, or as its text with boxes for its fill-ins, the
  // first of which takes the focus.
  function insertPrompt(prompt) {
    const root = composerRef.current;
    let range;
    if (palette?.slash) {
      range = document.createRange();
      range.setStart(palette.slash.node, palette.slash.start);
      range.setEnd(palette.slash.node, palette.slash.start + 1 + palette.slash.query.length);
    } else {
      range = caretRef.current && root.contains(caretRef.current.startContainer) ? caretRef.current : composer.rangeAtEnd(root);
    }
    root.focus();
    const field = composer.insert(root, composer.promptSegments(prompt), range);
    if (prompt.attachments?.length) setAttachments((current) => [...current, ...prompt.attachments.map((file) => ({ ...file, fromPrompt: prompt.id }))]);
    updatePrompt(prompt.id, { usedAt: Date.now() });
    setPalette(null);
    onComposerChange(composer.read(root));
    field?.focus();
  }

  function saveDraftAsPrompt() {
    const text = composer.read(composerRef.current).text.trim();
    const firstLine = text.split('\n')[0];
    const entry = {
      id: crypto.randomUUID(),
      title: firstLine.length > 48 ? `${firstLine.slice(0, 47)}…` : firstLine,
      text,
      attachments,
      tags: [],
    };
    setPrompts((list) => [...list, entry]);
    setPalette(null);
    openPrompt(entry.id);
  }

  // Arrow keys, Enter/Tab and Escape while the palette is open.
  function paletteKey(evt) {
    const flat = promptSections(prompts, palette.query).flatMap(([, list]) => list);
    if (evt.key === 'ArrowDown' || evt.key === 'ArrowUp') {
      evt.preventDefault();
      const step = evt.key === 'ArrowDown' ? 1 : -1;
      setPalette((current) => ({ ...current, index: flat.length ? (Math.min(current.index, flat.length - 1) + step + flat.length) % flat.length : 0 }));
      return true;
    }
    if ((evt.key === 'Enter' && !evt.shiftKey) || evt.key === 'Tab') {
      const prompt = flat[Math.min(palette.index, flat.length - 1)];
      if (!prompt) return false;
      evt.preventDefault();
      insertPrompt(prompt);
      return true;
    }
    if (evt.key === 'Escape') {
      evt.preventDefault();
      evt.stopPropagation();
      setPalette(null);
      composerRef.current?.focus();
      return true;
    }
    return false;
  }

  function onKeyDown(evt) {
    if (palette && paletteKey(evt)) return;
    // Tab walks through the fill-in boxes.
    if (evt.key === 'Tab') {
      const boxes = composer.fields(composerRef.current);
      if (boxes.length) {
        evt.preventDefault();
        const at = boxes.indexOf(evt.target);
        boxes[evt.shiftKey ? (at <= 0 ? boxes.length - 1 : at - 1) : (at + 1) % boxes.length].focus();
        return;
      }
    }
    if (evt.key === 'Enter' && !evt.shiftKey && !evt.isComposing) {
      evt.preventDefault();
      submit().catch(showError);
    }
  }

  const activeTitle = conversations.find((c) => c.id === activeId)?.title || 'New build';
  // One idea per conversation view, stable across re-renders, and none once
  // the chat has a prompt.
  const idea = useMemo(pickIdea, [activeId]);
  const prompted = items.some((item) => item.kind === 'user');

  const grassBlock = (px) => html`<${AppMark} size=${px} />`;
  const logo = grassBlock(28);

  function createPrompt() {
    const entry = { id: crypto.randomUUID(), title: '', text: '', attachments: [], tags: [] };
    setPrompts((list) => [...list, entry]);
    openPrompt(entry.id);
  }

  // The prompt library lives in Bench.
  function openPrompt(id) {
    setPromptsActiveId(id);
    goBench({ kind: 'prompt' });
  }

  function benchPrompt(prompt) {
    showBenchView({ kind: 'new-bench' });
    applyDraft({
      segments: composer.promptSegments(prompt),
      attachments: (prompt.attachments || []).map((file) => ({ ...file, fromPrompt: prompt.id })),
      selections: [],
      reading: [],
    });
    updatePrompt(prompt.id, { usedAt: Date.now() });
    requestAnimationFrame(() => composerRef.current?.focus());
  }

  // Library edits are functions of the list (prompt-sync.mjs); a prompt they
  // name may be gone from a list a builder changed, and they leave it be.
  // `patch` is the fields to change, or a function of the prompt giving them.
  function updatePrompt(id, patch) {
    setPrompts((list) => list.map((prompt) => (
      prompt.id === id ? { ...prompt, ...(typeof patch === 'function' ? patch(prompt) : patch) } : prompt
    )));
  }

  function togglePromptFavorite(id) {
    setPrompts((list) => list.map((prompt) => (
      prompt.id === id ? { ...prompt, favorite: !prompt.favorite } : prompt
    )));
  }

  function deletePrompt(id) {
    setPrompts((list) => list.filter((prompt) => prompt.id !== id));
    if (promptsActiveId === id) setPromptsActiveId(promptSyncRef.current.list[0]?.id ?? null);
  }

  // Rows `from` and `to` of the unfiltered list: the prompt moves to where the
  // one at `to` is, by their ids.
  function reorderPrompts(from, to) {
    setPromptsActiveId(shownPromptId); // the open prompt stays open wherever it moves
    const moved = prompts[from].id;
    const target = prompts[to].id;
    setPrompts((list) => {
      const item = list.find((prompt) => prompt.id === moved);
      const rest = list.filter((prompt) => prompt.id !== moved);
      const at = rest.findIndex((prompt) => prompt.id === target);
      if (!item || at < 0) return list;
      rest.splice(from < to ? at + 1 : at, 0, item);
      return rest;
    });
  }

  // The latest completion's usage: context use for the meter, limit state for the chip.
  // A stopped or failed request reports no usage; the meter keeps the last real one.
  const usageMeta = useMemo(() => [...items].reverse().find((item) => item.kind === 'usage' && !item.telemetry.failed) || null, [items]);

  // A bench chat is the record of one model's run: it reads, never edits.
  const readOnly = Boolean(activeRow?.benchId);
  // The chat's tree, or the bench's, is being changed (a branch switch, an
  // edit's revert): no second change starts until that one is over.
  const changing = branching || restoringKey !== null;
  // The composer's own draft is the one an edit's prompt is coming back to.
  const restoring = restoringKey !== null && restoringKey === draftKey;
  // Each entry's vnode is kept until something it shows changes. Preact
  // skips a subtree whose vnode it has seen, so a streamed token re-renders
  // only the entry it lands in, not the whole transcript.
  const entryVnodes = useRef({ items: new WeakMap(), groups: new WeakMap() });
  const transcript = useMemo(() => {
    const entries = groupItems(items.filter((item) => item.kind !== 'usage'), busy);

    function renderItem(item) {
      if (item.kind === 'user') {
        const branch = !readOnly && branches.get(item.messageId);
        return html`
          <div class="user-block">
            <div class="msg user">
              ${item.text}
              ${item.attachments && item.attachments.length > 0 && html`
                <div class="msg-attachments">
                  ${item.attachments.map((file) => file.type.startsWith('image/')
                    ? html`<button class="msg-file image" onClick=${() => setLightbox(file.data || file.url)}>
                        <img src=${file.data || file.url} alt="" />
                        <span>${file.name}</span>
                      </button>`
                    : html`<a class="msg-file" href=${file.url || '#'} target="_blank" rel="noreferrer">
                        <span class="file-tile"><${Icon} name="file" size="15" /></span>
                        <span>${file.name}</span>
                      </a>`)}
                </div>
              `}
              ${item.selections?.length > 0 && html`<div class="msg-selections">${item.selections.map((selection) => html`<${SelectionChip} selection=${selection} onHover=${setHoveredSelection} />`)}</div>`}
            </div>
            <div class="msg-tools">
              ${branch && html`
                <span class="branch-picker" aria-label="Conversation branch">
                  <button
                    class="mtool"
                    title="Previous branch"
                    aria-label="Previous branch"
                    disabled=${busy || changing || !branch.previousId}
                    onClick=${() => switchBranch(branch.previousId).catch(showError)}
                  ><${Icon} name="back" size="13" /></button>
                  <span>${branch.index + 1} / ${branch.count}</span>
                  <button
                    class="mtool"
                    title="Next branch"
                    aria-label="Next branch"
                    disabled=${busy || changing || !branch.nextId}
                    onClick=${() => switchBranch(branch.nextId).catch(showError)}
                  ><${Icon} name="forward" size="13" /></button>
                </span>
              `}
              <button class="mtool" title="Copy message" onClick=${() => copyMessage(item).catch(showError)}>
                <${Icon} name=${copied === item ? 'check' : 'copy'} size="13" />
              </button>
              ${item.messageId !== undefined && items.length > 1 && !busy && !readOnly && html`
                <button
                  class="mtool"
                  title="Edit and branch"
                  aria-label="Edit and branch"
                  disabled=${changing}
                  onClick=${() => editFrom(item.messageId).catch(showError)}
                >
                  <${Icon} name="compose" size="13" />
                </button>
              `}
            </div>
          </div>
        `;
      }
      if (item.kind === 'assistant') return html`<div class="msg assistant"><${Markdown} text=${item.text} /></div>`;
      if (item.kind === 'reasoning') return reasoningRow(item.text, item.live ? 'Reasoning…' : 'Reasoning');
      if (item.kind === 'plan' || item.kind === 'notice' || item.kind === 'shots') return renderLogItem(item);
      if (item.kind === 'tool-stack') {
        const hidden = item.calls.slice(0, -1);
        const latest = item.calls[item.calls.length - 1];
        const renderCall = (call) => {
          return html`
            <div class="live-tool-call" key=${call.tool.callId}>
              ${renderLogItem(call.tool)}
              ${call.outputs.map(renderLogItem)}
            </div>
          `;
        };
        return html`
          <div class="live-tool-stack">
            ${latest && renderCall(latest)}
            ${hidden.length > 0 && html`
              <${Fold}
                className="tool-history"
                title="Show previous tool calls"
                summary=${html`
                  <span class="history-icon-slot"><${Icon} name="chevron-down" size="14" /></span>
                  <span>+${hidden.length} previous tool call${hidden.length === 1 ? '' : 's'}</span>
                `}
                body=${() => html`<div class="tool-history-body">${hidden.map(renderCall)}</div>`}
              />
            `}
          </div>
        `;
      }
      if (item.kind === 'error') return html`
        <div class="msg error">
          <span>${item.text}</span>
          ${!readOnly && !busy && html`<button
            class="error-dismiss"
            title="Dismiss error"
            aria-label="Dismiss error"
            onClick=${() => {
              setItems((prev) => prev.filter((entry) => entry !== item));
              // Persisted errors would come back on the next resync
              // unless the server marks them dismissed too.
              if (item.messageId !== undefined && activeId) {
                forgetError(activeId, item.messageId);
                api(`/api/conversations/${activeId}/dismiss-error`, {
                  method: 'POST',
                  headers: { 'content-type': 'application/json' },
                  body: JSON.stringify({ messageId: item.messageId }),
                }).catch(showError);
              }
            }}
          ><${Icon} name="x" size="13" /></button>`}
        </div>
      `;
      return null;
    }

    function reasoningRow(text, label) {
      return html`
        <${Fold}
          className="reasoning-row"
          title="Show reasoning"
          summary=${html`
            <span class="reasoning-icon-slot"><${Icon} name="chevron-right" size="14" /></span>
            <span>${label}</span>
          `}
          body=${() => html`
            <div class="reasoning-body">
              <div class="reasoning-text"><${Markdown} text=${text} /></div>
            </div>
          `}
        />
      `;
    }

    // Inside the fold everything drops a register: commentary is a
    // log note, tools are one-line entries, screenshots are thumbs.
    function renderLogItem(item) {
      if (item.kind === 'assistant') return html`<div class="log-note"><${Markdown} text=${item.text} /></div>`;
      if (item.kind === 'plan') return html`<${PlanCard} items=${item.items} />`;
      if (item.kind === 'notice') return html`<${NoticeLine} level=${item.level} text=${item.text} />`;
      if (item.kind === 'reasoning') return reasoningRow(item.text, 'Reasoning');
      if (item.kind === 'shots') return html`
        <div class="log-shots">
          ${item.images.map((src) => html`<img src=${src} onClick=${() => setLightbox(src)} />`)}
        </div>
      `;
      const writing = item.status === 'writing';
      const program = item.program;
      const summary = item.summary || item.tool || '\u2026';
      const active = writing || item.status === 'running';
      const expandable = program !== undefined || writing;
      let action = item.tool || 'Tool';
      let detail = summary;
      if (item.tool === 'bash') {
        action = active ? 'Running command' : item.status === 'failed' ? 'Command failed' : 'Ran command';
        detail = summary.replace(/^\$\s*/, '');
      } else if (item.tool === 'build') {
        detail = summary.replace(/^(?:build|building|built)\s+/i, '');
        const inspection = detail.match(/^(?:inspect|check|verify|dump|read|examine)\s+(.*)$/i);
        if (inspection) {
          action = active ? 'Inspecting' : item.status === 'failed' ? 'Inspection failed' : 'Inspected';
          detail = inspection[1];
        } else {
          action = active ? 'Building' : item.status === 'failed' ? 'Build failed' : 'Built';
        }
      } else if (item.tool === 'see_build') {
        action = active ? 'Inspecting build' : item.status === 'failed' ? 'Inspection failed' : 'Inspected build';
        detail = summary.replace(/^Look at build\s*/i, '');
      } else if (item.tool === 'test_build') {
        action = active ? 'Testing in Minecraft' : item.status === 'cancelled' ? 'Minecraft test cancelled'
          : item.status === 'failed' ? 'Minecraft test failed' : 'Minecraft test completed';
        const tests = summary.match(/\((\d+) tests\)/)?.[1];
        detail = active && item.progress ? item.progress : tests ? `${tests} tests: input and block-state observations` : 'Input and block-state observations';
      } else if (item.tool === 'view_image') {
        action = active ? 'Viewing image' : item.status === 'failed' ? 'Image view failed' : 'Viewed image';
        detail = summary.replace(/^Look at\s*/i, '');
      }
      if (detail === action || !detail.trim()) detail = '';
      const toolIcon = item.tool === 'bash' ? 'terminal'
        : item.tool === 'build' ? 'hammer'
          : item.tool === 'see_build' || item.tool === 'view_image' ? 'eye'
            : 'wrench';
      const row = html`
        <span class="tool-kind-icon"><${Icon} name=${toolIcon} size="14" /></span>
        <span class="tool-copy">
          <span class="s ${active ? 'active' : ''}">${action}</span>
          ${detail && html`<span class="d ${item.tool === 'bash' ? 'mono' : ''}" title=${detail}>${detail}</span>`}
          ${writing && program && html`<span class="tool-meta">+${program.split('\n').length} lines</span>`}
          ${item.diff && html`<${DiffStat} diff=${item.diff} />`}
        </span>
        <span class="tool-actions">
          ${item.status === 'failed' && html`<span class="tool-failure"><${Icon} name="x" size="12" /></span>`}
        </span>
      `;
      const err = item.status === 'failed' && item.detail
        ? html`<div class="log-err">${item.detail}</div>`
        : null;
      const step = expandable
        ? html`
          <${Fold}
            className="log-code"
            summaryClass="log-step"
            title="Show code"
            summary=${row}
            body=${() => html`<pre
              class="code"
              ref=${(el) => {
                if (!el || !writing) return;
                const prev = Number(el.dataset.prevH || 0);
                const pinned = el.scrollTop + el.clientHeight >= prev - 40;
                el.dataset.prevH = el.scrollHeight;
                if (pinned) el.scrollTop = el.scrollHeight;
              }}
            >${program
              ? html`<${Code} text=${program} />`
              : writing
                ? html`<span class="code-awaiting">Streaming tool input…</span>`
                : null}</pre>
            ${item.output !== undefined && html`<pre class="code term">${item.output}</pre>`}`}
          />`
        : html`<div class="log-step">${row}</div>`;
      return html`<div class="log-entry">${step}${err}</div>`;
    }

    function cached(cache, anchor, deps, render) {
      const hit = cache.get(anchor);
      if (hit && hit.deps.length === deps.length && hit.deps.every((dep, i) => dep === deps[i])) return hit.vnode;
      const vnode = render();
      cache.set(anchor, { deps, vnode });
      return vnode;
    }

    const { items: itemVnodes, groups: groupVnodes } = entryVnodes.current;
    const keys = entryKeys(entries);
    return entries.map((entry, index) => {
      let vnode;
      if (entry.kind === 'activity') vnode = cached(groupVnodes, entry.children[0], ['activity', ...entry.children], () => renderActivity(entry));
      else if (entry.kind === 'tool-stack') {
        vnode = cached(groupVnodes, entry.calls[0].tool, ['stack', ...entry.calls.flatMap((call) => [call.tool, ...call.outputs])], () => renderItem(entry));
      } else vnode = cached(itemVnodes, entry, [branches.get(entry.messageId), copied === entry, busy, changing, items.length > 1, readOnly, activeId], () => renderItem(entry));
      return html`<${Keyed} key=${keys[index]}>${vnode}<//>`;
    });

    function renderActivity(entry) {
      const steps = entry.children.filter((c) => c.kind === 'tool').length;
      const label = steps > 0 ? `Worked · ${steps} step${steps === 1 ? '' : 's'}` : 'Worked';
      // A burst of reasoning deltas folds into one "Reasoning" row.
      const children = [];
      for (const child of entry.children) {
        const last = children[children.length - 1];
        if (child.kind === 'reasoning' && last?.kind === 'reasoning') {
          last.text = `${last.text}\n\n${child.text}`;
          continue;
        }
        children.push(child.kind === 'reasoning' ? { ...child } : child);
      }
      return html`
        <${Fold}
          className="activity"
          summary=${html`
            <span class="steps">${label}</span>
            <span class="activity-chevron"><${Icon} name="chevron-right" size="14" /></span>
          `}
          body=${() => html`<div class="activity-body">${children.map(renderLogItem)}</div>`}
        />
      `;
    }
  }, [items, busy, changing, branches, copied, activeId, readOnly]);

  const benchMode = appMode === 'bench';
  const memberRow = place.kind === 'member' ? conversations.find((row) => row.id === place.memberId) || null : null;
  const transcriptShown = !benchMode || place.kind === 'member';
  const composerShown = draftKey !== null;

  // A character typed anywhere outside a field: in a comparison, 1–9 fly to
  // that model's build; anything else goes into the message box, which takes
  // the focus, so typing never needs a click first. The viewer passes on keys
  // pressed in it, except in fullscreen, where they move the camera.
  const typeAhead = composerShown && !modalOpen(layers);
  const typeAheadRef = useRef(false);
  typeAheadRef.current = typeAhead;
  const typeKeyRef = useRef(null);
  typeKeyRef.current = (key) => {
    const member = place.kind === 'compare' && /^[1-9]$/.test(key) && viewedMembers[Number(key) - 1];
    if (member) return focusBuild(member.id);
    const root = composerRef.current;
    root.focus();
    const selection = window.getSelection();
    if (!root.contains(selection.anchorNode)) selection.collapse(root, root.childNodes.length);
    document.execCommand('insertText', false, key);
  };
  useEffect(() => {
    const onKey = (evt) => {
      if (!typeAheadRef.current || evt.isComposing || evt.metaKey || evt.ctrlKey || evt.altKey || evt.key.length !== 1 || evt.key === ' ') return;
      if (evt.target.closest?.('input, textarea, select, [contenteditable]:not([contenteditable="false"])')) return;
      evt.preventDefault();
      typeKeyRef.current(evt.key);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
  const promptView = place.kind === 'prompt';
  const shownRoundNumber = viewedBench ? benchRounds[viewedBench.id] ?? viewedBench.rounds : null;
  const DOT_LABEL = { running: 'Working', unread: 'Finished, not opened yet', halted: 'Stopped or failed', idle: 'Idle' };

  function renderFiles(files) {
    return html`
      <div class="msg-attachments">
        ${files.map((file) => file.type.startsWith('image/')
          ? html`<button class="msg-file image" onClick=${() => setLightbox(file.data || file.url)}>
              <img src=${file.data || file.url} alt="" />
              <span>${file.name}</span>
            </button>`
          : html`<a class="msg-file" href=${file.url || '#'} target="_blank" rel="noreferrer">
              <span class="file-tile"><${Icon} name="file" size="15" /></span>
              <span>${file.name}</span>
            </a>`)}
      </div>
    `;
  }

  // A dot that was working and is now waiting eases from grey into blue;
  // one already waiting when it first appears simply is blue.
  const dotHistory = useRef(new Map()); // what the dot is for -> { state, arrived }
  function dotClass(key, state) {
    const seen = dotHistory.current.get(key);
    const arrived = state === 'unread' && (seen?.state === 'running' || (seen?.state === 'unread' && seen.arrived));
    dotHistory.current.set(key, { state, arrived });
    return `conv-dot ${state}${arrived ? ' arrived' : ''}`;
  }

  // What the other mode has going on, for the dot on the switch: finished and
  // not yet looked at (a bench only once all its models are done), or working.
  const otherMode = appMode === 'build' ? 'bench' : 'build';
  const otherStates = otherMode === 'bench'
    ? benches.map((bench) => {
      const members = conversations.filter((row) => row.benchId === bench.id);
      return members.some((row) => row.runningSince) ? 'running' : members.some((row) => row.unread) ? 'unread' : null;
    })
    : conversations.filter((row) => !row.benchId).map((row) => (row.runningSince ? 'running' : row.unread ? 'unread' : null));
  const otherWaiting = otherStates.filter((state) => state === 'unread').length;
  const otherDot = otherWaiting ? 'unread' : otherStates.includes('running') ? 'running' : null;

  // Search results in rank order, each with the snippet that matched.
  const snippets = new Map((found || []).map((result) => [result.id, result.snippet]));
  const buildRows = found
    ? found.map((result) => conversations.find((row) => row.id === result.id && !row.benchId)).filter(Boolean)
    : conversations.filter((row) => !row.benchId);
  const benchRows = found ? found.map((result) => benches.find((bench) => bench.id === result.id)).filter(Boolean) : benches;
  // The sidebar is narrow: a snippet starts a few words before its first match.
  const marked = (snippet) => {
    const first = snippet.indexOf('\u0001');
    const from = first > 18 ? snippet.lastIndexOf(' ', first - 12) : -1;
    const shown = from > 0 ? `…${snippet.slice(from + 1)}` : snippet;
    return shown.split(/(\u0001[^\u0002]*\u0002)/).map((part) => (
      part.startsWith('\u0001') ? html`<mark>${part.slice(1, -1)}</mark>` : part
    ));
  };
  const rowText = (id, title) => html`
    <span class="conv-text">
      <span class="title">${title}</span>
      ${snippets.get(id) && html`<span class="conv-snippet">${marked(snippets.get(id))}</span>`}
    </span>
  `;

  // Below the list: the mode's archived items, each restorable until it
  // expires, or deleted now on a second click.
  function renderArchive(mode) {
    if (archivedCounts[mode === 'bench' ? 'benches' : 'conversations'] === 0) return null;
    const open = archive.phase !== 'closed' && archive.mode === mode;
    const rows = open ? archivedRows(archive) : null;
    const now = Date.now();
    return html`
      <div class="archived">
        <button type="button" class="side-title" aria-expanded=${open} onClick=${() => toggleArchive(mode)}>
          <${Icon} name="chevron-right" size="12" />
          <span>${open ? 'Hide archived' : 'Show archived'}</span>
        </button>
        ${open && html`
          <div class="archived-list">
            ${archive.phase === 'loading' && !rows && html`<div class="side-empty">Loading…</div>`}
            ${archive.phase === 'failed' && html`
              <div class="side-empty" role="alert">Archived items failed to load. <button type="button" class="text-link" onClick=${() => loadArchive(mode)}>Try again</button></div>
            `}
            ${rows && rows.map((row) => html`
              <${ArchivedRow}
                key=${row.id}
                title=${row.title}
                meta=${`${mode === 'bench' ? `${row.models} ${row.models === 1 ? 'model' : 'models'} · ` : ''}${timeLeft(row.expiresAt, now)}`}
                onRestore=${() => restoreArchived(mode, row.id).catch(showError)}
                onDelete=${() => deleteArchived(mode, row.id).catch(showError)}
              />
            `)}
          </div>
        `}
      </div>
    `;
  }

  function renderBenchGroup(bench) {
    const members = conversations.filter((row) => row.benchId === bench.id).sort((a, b) => bench.order.indexOf(a.id) - bench.order.indexOf(b.id));
    const open = openBenches.has(bench.id);
    const states = members.map((row) => memberDot(row, activeId));
    const dot = ['running', 'unread'].find((state) => states.includes(state));
    return html`
      <div class="bench-group" key=${bench.id}>
        <div
          class="conv-item bench-row ${place.kind === 'compare' && place.benchId === bench.id ? 'active' : ''}"
          onClick=${() => showBenchView({ kind: 'compare', benchId: bench.id })}
        >
          <button
            type="button"
            class="bench-caret ${open ? 'open' : ''}"
            aria-label=${open ? 'Hide models' : 'Show models'}
            aria-expanded=${open}
            onClick=${(evt) => {
              evt.stopPropagation();
              setOpenBenches((current) => {
                const next = new Set(current);
                if (next.has(bench.id)) next.delete(bench.id); else next.add(bench.id);
                return next;
              });
            }}
          ><${Icon} name="chevron-right" size="13" /></button>
          ${rowText(bench.id, bench.title)}
          <${ResolutionTag} value=${bench.resolution} />
          ${dot && !open && html`<span class=${dotClass(`bench:${bench.id}`, dot)} role="img" aria-label=${DOT_LABEL[dot]}></span>`}
          ${!states.includes('running') && html`<button class="archive" title="Archive bench (it can be restored for 7 days)" aria-label="Archive bench" onClick=${(evt) => archiveBench(bench.id, evt).catch(showError)}>
            <${Icon} name="archive" size="14" />
          </button>`}
        </div>
        ${open && html`
          <div class="bench-members">
            ${members.map((row) => {
              const state = memberDot(row, activeId);
              return html`
                <div
                  key=${row.id}
                  class="conv-item bench-member ${row.id === activeId ? 'active' : ''}"
                  onMouseEnter=${() => { if (!row.runningSince && row.id !== activeId) fetchTranscript(row.id).catch(() => {}); }}
                  onClick=${() => showBenchView({ kind: 'member', benchId: bench.id, memberId: row.id })}
                >
                  <span class=${dotClass(row.id, state)} role="img" aria-label=${DOT_LABEL[state]}></span>
                  <span class="title" title=${hidesNames(bench) ? '' : row.model}>${nameOf(row)}</span>
                </div>
              `;
            })}
          </div>
        `}
      </div>
    `;
  }

  function renderBenchCompare() {
    const running = viewedMembers.some((member) => member.runningSince);
    const hidden = hidesNames(viewedBench);
    return html`
      <div class="messages bench-compare" ref=${messagesRef}>
        ${(viewedDetails?.rounds || []).map((round, index) => {
          const number = index + 1;
          const content = typeof round.content === 'string' ? [{ type: 'text', text: round.content }] : round.content;
          const files = content.filter((part) => part.type === 'attachment').map((part) => part.attachment);
          const { branch } = round;
          // A round is voted on once no model is still to finish it, among
          // those that finished it; clicking the current pick takes it back.
          const states = viewedMembers.map((member) => roundState(member, number));
          const judged = viewedMembers.filter((_, i) => states[i] === 'done');
          // A round is voted on while the bench hides its models' names. Once
          // they show, the vote cast blind stays as a mark; a vote kept from
          // a bench that showed its names never counted and is not shown.
          const votable = hidden && !states.some((state) => state === 'running' || state === 'queued') && judged.length >= 2;
          const vote = viewedDetails.votes[round.id]?.blind ? viewedDetails.votes[round.id] : undefined;
          const pick = (choice) => voteRound(viewedBench.id, round.id, vote !== undefined && vote.pick === (choice === 'tie' ? null : choice) ? null : choice).catch(showError);
          return html`
            <section key=${round.id} class="bench-round ${number === shownRoundNumber ? 'shown' : ''}">
              <div class="user-block">
                <div class="msg user bench-prompt" onClick=${() => showWholeRound(viewedBench.id, number)}>
                  ${content.filter((part) => part.type === 'text').map((part) => part.text).join('\n')}
                  ${files.length > 0 && renderFiles(files)}
                </div>
                <div class="msg-tools">
                  ${branch.count > 1 && html`
                    <span class="branch-picker" aria-label="Round branch">
                      <button
                        class="mtool"
                        title="Previous branch"
                        aria-label="Previous branch"
                        disabled=${running || changing || !branch.previousId}
                        onClick=${() => switchRoundBranch(viewedBench.id, branch.previousId).catch(showError)}
                      ><${Icon} name="back" size="13" /></button>
                      <span>${branch.index + 1} / ${branch.count}</span>
                      <button
                        class="mtool"
                        title="Next branch"
                        aria-label="Next branch"
                        disabled=${running || changing || !branch.nextId}
                        onClick=${() => switchRoundBranch(viewedBench.id, branch.nextId).catch(showError)}
                      ><${Icon} name="forward" size="13" /></button>
                    </span>
                  `}
                  ${!running && html`
                    <button
                      class="mtool"
                      title="Edit and branch"
                      aria-label="Edit and branch"
                      disabled=${changing}
                      onClick=${() => editRound(viewedBench.id, round.id).catch(showError)}
                    ><${Icon} name="compose" size="13" /></button>
                  `}
                </div>
              </div>
              <div class="bench-results" role="list" aria-label=${`Round ${number}`}>
                ${viewedMembers.map((member) => html`
                  <${BenchResultRow}
                    key=${member.id}
                    member=${member}
                    letter=${hidden ? benchLetter(viewedBench, member.id) : null}
                    picked=${vote?.pick === member.id}
                    onVote=${votable && judged.includes(member) ? () => pick(member.id) : null}
                    state=${roundState(member, number)}
                    stats=${viewedDetails.stats[member.id]?.[index]}
                    onFocus=${() => (number === shownRoundNumber ? focusBuild(member.id) : showRound(viewedBench.id, number))}
                    onOpen=${() => showBenchView({ kind: 'member', benchId: viewedBench.id, memberId: member.id })}
                    onStop=${() => api(`/api/conversations/${member.id}/stop`, { method: 'POST' }).catch(showError)}
                    onRetry=${() => postJson(`/api/benches/${viewedBench.id}/retry`, { memberId: member.id }).catch(showError)}
                  />
                `)}
                ${votable && html`
                  <div role="listitem" class="bench-result bench-tie-row ${vote?.pick === null ? 'picked' : ''}">
                    <span></span>
                    <span></span>
                    <span class="bench-model">Tie<span class="bench-tie-note">no build is better</span></span>
                    <span></span>
                    <span></span>
                    <button
                      type="button"
                      class="bench-vote bench-tie ${vote?.pick === null ? 'picked' : ''}"
                      title=${vote?.pick === null ? 'Your call for this round. Click to take it back' : 'Call this round a tie'}
                      aria-label=${vote?.pick === null ? 'Take back the tie' : 'Call this round a tie'}
                      aria-pressed=${vote?.pick === null}
                      onClick=${() => pick('tie')}
                    ><${Icon} name="equal" size="13" />${vote?.pick === null && html`<span>Tie</span>`}</button>
                    <span></span>
                  </div>
                `}
                ${!votable && vote?.pick === null && html`
                  <div role="listitem" class="bench-result bench-tie-row picked">
                    <span></span>
                    <span></span>
                    <span class="bench-model">Tie<span class="bench-tie-note">no build was better</span></span>
                    <span></span>
                    <span></span>
                    <span class="bench-vote picked bench-mark" role="img" aria-label="Your blind call for this round: a tie"><${Icon} name="equal" size="13" /><span>Tie</span></span>
                    <span></span>
                  </div>
                `}
              </div>
            </section>
          `;
        })}
        ${viewedDetailsError && html`
          <div class="msg error" role="alert">
            <span>This bench could not be read: ${viewedDetailsError}</span>
            <button type="button" class="text-link" onClick=${() => loadBenchDetails(viewedBench.id)}>Read again</button>
          </div>
        `}
      </div>
    `;
  }

  const benchReady = benchModels.length >= 2;
  // Under an empty screen's question while no AI is connected.
  const connectHint = html`
    <div class="connect-hint">
      <div class="connect-hint-text">Connect an AI to start building</div>
      <button type="button" class="btn gold connect-hint-button" onClick=${connectAI}>Connect an AI</button>
    </div>
  `;
  // The bench models' popover opens down from their row: before it opens,
  // the row learns how much of the window is left below it, which caps the
  // popover's height.
  const fitBenchPicker = (evt) => {
    const row = evt.currentTarget.closest('.bench-lineup');
    row.style.setProperty('--below', `${Math.floor(window.innerHeight - row.getBoundingClientRect().bottom - 16)}px`);
  };
  // The resolution in the composer: a choice in Build's chats and for a new
  // bench; on a bench's comparison it only shows, above Standard, since its
  // models' chats keep it.
  const resolutionValue = place.kind === 'new-bench' ? benchResolution : place.kind === 'compare' ? viewedBench?.resolution : resolution;
  const resolutionShown = place.kind === 'chat' || place.kind === 'new-chat' || place.kind === 'new-bench' || (place.kind === 'compare' && resolutionValue > 1);
  const resolutionPicker = resolutionShown && html`
    <${ResolutionPicker}
      value=${resolutionValue}
      readOnly=${place.kind === 'compare'}
      onChange=${place.kind === 'new-bench' ? setBenchResolution : chooseResolution}
    />
  `;
  let composerControls;
  if (!benchMode) {
    composerControls = html`
      ${usageMeta?.telemetry?.provider === provider && usageMeta.telemetry.model === model && html`
        <${LimitChip} info=${usageMeta.rateLimit} />
        ${usageMeta.telemetry.contextTokens && modelCaps?.contextWindow && html`
          <${ContextMeter} used=${usageMeta.telemetry.contextTokens} window=${modelCaps.contextWindow} />
        `}
      `}
      ${resolutionPicker}
      ${listReady && html`<div class="composer-model">
        <${ModelPicker}
          providers=${providers}
          value=${provider && model ? { provider, model } : null}
          onChange=${(next) => {
            // The model chosen last is the one a fresh page starts with.
            localStorage.setItem('mcb-last-model', JSON.stringify({ provider: next.provider, model: next.model }));
            setModelChoice({ provider: next.provider, model: next.model, effort: '', resolution });
            // Chosen in the picker that opened by itself: the prompt is next.
            if (askedModelRef.current) requestAnimationFrame(() => composerRef.current?.focus());
            askedModelRef.current = false;
          }}
          favorites=${favorites}
          onToggleFavorite=${toggleFavorite}
          requireTools=${true}
          label="Chat model"
          openNow=${askModel}
          onOpened=${() => { setAskModel(false); askedModelRef.current = true; }}
          footer=${{ label: 'Compare several in Bench', icon: 'bench', onSelect: benchFromBuild }}
          onSetup=${(view) => openSettings(view).catch(showError)}
        />
      </div>`}
      ${listReady && modelCaps && modelCaps.reasoning && modelCaps.efforts.length > 0 && html`
        <${EffortPicker}
          caps=${modelCaps}
          value=${effort}
          onChange=${setEffort}
        />
      `}
      ${busy
        ? html`
          <button class="stop" title="Stop generating" aria-label="Stop generating" onClick=${stop}>
            <${Icon} name="stop" size="13" />
          </button>`
        : html`
          <button
            class="send"
            title="Send message"
            aria-label="Send message"
            disabled=${(!hasText && attachments.length === 0) || !provider || !model || restoring || reading.length > 0}
            onClick=${() => submit().catch(showError)}
          >
            <${Icon} name="send" size="15" />
          </button>`}
    `;
  } else {
    const label = place.kind === 'new-bench' ? `Run on ${benchModels.length} models` : 'Send to every model';
    composerControls = html`
      ${resolutionPicker}
      <button
        class="send"
        title=${label}
        aria-label=${label}
        disabled=${(!hasText && attachments.length === 0) || (place.kind === 'new-bench' && !benchReady) || restoring || reading.length > 0}
        onClick=${() => submit().catch(showError)}
      >
        <${Icon} name="send" size="15" />
      </button>
    `;
  }

  return html`
    <div
      class="layout ${sidebarOpen ? '' : 'sidebar-closed'} ${promptView ? 'prompts-mode' : ''}"
      style="--chat-col:${chatWidth ? `${chatWidth}px` : 'minmax(360px, 42%)'}; --sidebar-w:${sidebarWidth}px"
    >
      ${!sidebarOpen && html`<div class="sidebar-reveal" title="Show sidebar" onMouseDown=${startSidebarDrag}></div>`}
      <aside class="sidebar">
        <div class="brand">
          ${logo}
          <${ModeSwitcher}
            mode=${appMode}
            binds=${binds}
            waiting=${otherWaiting}
            dot=${otherDot && html`<span class=${`${dotClass(`switch:${otherMode}`, otherDot)} switch-dot`} aria-hidden="true"></span>`}
            onChange=${switchMode}
          />
          <div class="brand-actions">
            <button
              class="icon-btn"
              title=${benchMode ? 'New bench' : 'New chat'}
              aria-label=${benchMode ? 'New bench' : 'New chat'}
              onClick=${() => (benchMode ? showBenchView({ kind: 'new-bench' }) : newConversation().catch(showError))}
            >
              <${Icon} name="compose" size="16" />
            </button>
            <button class="icon-btn" title="Collapse sidebar" aria-label="Collapse sidebar" onClick=${() => setSidebarOpen(false)}>
              <${Icon} name="panel" size="16" />
            </button>
          </div>
        </div>
        <label class="side-find">
          <${Icon} name="search" size="14" />
          <input
            ref=${findRef}
            value=${find}
            placeholder="Search"
            aria-label=${benchMode ? 'Search benches' : 'Search chats'}
            onInput=${(evt) => setFind(evt.target.value)}
            onKeyDown=${(evt) => { if (evt.key === 'Escape') { setFind(''); evt.currentTarget.blur(); } }}
          />
        </label>
        ${!benchMode && html`<div class="conv-list">
          ${found && !buildRows.length && html`<div class="side-empty">No matches</div>`}
          ${buildRows.map((c) => {
            // Running pulses; finished and not yet looked at is solid; the
            // rest are hollow. The open chat is always looked at.
            const state = c.runningSince ? 'running' : c.unread && c.id !== activeId ? 'unread' : 'idle';
            return html`
            <div
              key=${c.id}
              class="conv-item ${c.id === activeId ? 'active' : ''}"
              onMouseEnter=${() => { if (!c.runningSince && c.id !== activeId) fetchTranscript(c.id).catch(() => {}); }}
              onClick=${() => openConversation(c.id).catch(showError)}
            >
              <span class=${dotClass(c.id, state)} role="img" aria-label=${DOT_LABEL[state]}></span>
              ${rowText(c.id, c.title)}
              <${ResolutionTag} value=${c.resolution} />
              ${!c.runningSince && html`<button class="archive" title="Archive build (it can be restored for 7 days)" aria-label="Archive build" onClick=${(evt) => archiveConversation(c.id, evt).catch(showError)}>
                <${Icon} name="archive" size="14" />
              </button>`}
            </div>
          `;
          })}
        </div>`}
        ${!benchMode && renderArchive('build')}
        ${benchMode && html`
          ${!found && html`
            <div
              class="conv-item leaderboard-row ${place.kind === 'leaderboard' ? 'active' : ''}"
              onClick=${() => showBenchView({ kind: 'leaderboard' })}
            ><${Icon} name="trophy" size="14" /><span class="title">Leaderboard</span></div>
          `}
          ${found && !benchRows.length && html`<div class="side-empty">No matches</div>`}
          ${benchRows.length > 0 && html`<div class="conv-list bench-list">${benchRows.map(renderBenchGroup)}</div>`}
          ${renderArchive('bench')}
          ${promptsReady && html`
            <${PromptsSection}
              prompts=${prompts}
              onCreate=${createPrompt}
              activeId=${promptView ? shownPromptId : null}
              onSwitch=${openPrompt}
              onReorder=${reorderPrompts}
              onToggleFavorite=${togglePromptFavorite}
              onDelete=${deletePrompt}
            />
          `}
          ${promptLibrary.phase === 'failed' && html`
            <div class="side-empty" role="alert">Prompts failed to load. <button type="button" class="text-link" onClick=${loadPrompts}>Try again</button></div>
          `}
        `}
        ${noAI && html`
          <button type="button" class="meta warn sidebar-connect" onClick=${connectAI}>No AI connected. Connect one</button>
        `}
        ${config.phase === 'failed' && html`
          <div class="side-empty" role="alert">Models failed to load. <button type="button" class="text-link" onClick=${loadConfig}>Try again</button></div>
        `}
        <div class="sidebar-foot">
          <button class="icon-btn" title="Settings" aria-label="Settings" onClick=${() => openSettings().catch(showError)}>
            <${Icon} name="settings" size="16" />
          </button>
        </div>
        <div
          class="sidebar-edge"
          title="Drag to resize · double-click to collapse"
          onMouseDown=${startSidebarDrag}
          onDblClick=${() => setSidebarOpen(false)}
        ></div>
      </aside>

      <section class="chat">
        <div class="chat-header">
          ${!sidebarOpen && html`
            <button class="icon-btn" title="Open sidebar" aria-label="Open sidebar" onClick=${() => setSidebarOpen(true)}>
              <${Icon} name="panel" size="16" />
            </button>
            <button
              class="icon-btn"
              title=${benchMode ? 'New bench' : 'New chat'}
              aria-label=${benchMode ? 'New bench' : 'New chat'}
              onClick=${() => (benchMode ? showBenchView({ kind: 'new-bench' }) : newConversation().catch(showError))}
            >
              <${Icon} name="compose" size="16" />
            </button>
          `}
          <div class="title">
            ${!benchMode ? activeTitle
              : place.kind === 'new-bench' ? 'New bench'
              : place.kind === 'compare' ? viewedBench?.title
              : place.kind === 'leaderboard' ? 'Leaderboard'
              : place.kind === 'member' ? html`
                <button class="crumb" onClick=${() => showBenchView({ kind: 'compare', benchId: place.benchId })}>${viewedBench?.title}</button>
                <span class="crumb-sep">/</span>${memberRow && nameOf(memberRow)}`
              : html`<span class="crumb">Prompts</span>${shownPrompt && html`<span class="crumb-sep">/</span>${shownPrompt.title || 'Untitled prompt'}`}`}
          </div>
          ${place.kind === 'compare' && viewedBench?.rounds > 1 && html`
            <div class="round-switch" role="group" aria-label="Round shown in the world">
              ${Array.from({ length: viewedBench.rounds }, (_, index) => html`
                <button
                  type="button"
                  class=${index + 1 === shownRoundNumber ? 'active' : ''}
                  aria-pressed=${index + 1 === shownRoundNumber}
                  title=${`Round ${index + 1}`}
                  onClick=${() => showRound(viewedBench.id, index + 1)}
                >${index + 1}</button>
              `)}
            </div>
          `}
          ${viewedBench && hidesNames(viewedBench) && (place.kind === 'compare' || place.kind === 'member') && html`
            <${ConfirmButton}
              subject=${`reveal:${viewedBench.id}`}
              icon="eye"
              label="Reveal models"
              confirmLabel="Reveal for good"
              title="Show which model made each build. Voting ends here: the votes cast stay as they are."
              onConfirm=${() => api(`/api/benches/${viewedBench.id}/reveal`, { method: 'POST' }).catch(showError)}
            />
          `}
          ${memberRow && (hidesNames(viewedBench) ? html`
            <${ConfirmButton}
              subject=${`fork:${memberRow.id}`}
              icon="fork"
              label="Fork to Build"
              confirmLabel="Fork and reveal models"
              title="Build shows the model, so forking reveals this bench"
              disabled=${forking || (memberRow.runningSince ? memberRow.round - 1 : memberRow.round) < 1}
              onConfirm=${() => forkToBuild(memberRow).catch(showError)}
            />
          ` : html`
            <button
              type="button"
              class="header-action quiet"
              disabled=${forking || (memberRow.runningSince ? memberRow.round - 1 : memberRow.round) < 1}
              title="Continue this model's build as a new chat in Build"
              onClick=${() => forkToBuild(memberRow).catch(showError)}
            ><${Icon} name="fork" size="14" /> Fork to Build</button>
          `)}
          ${promptView && shownPrompt && html`
            <button
              type="button"
              class="header-action"
              disabled=${!shownPrompt.text?.trim()}
              onClick=${() => benchPrompt(shownPrompt)}
            >Run bench</button>
          `}
          ${!benchMode && modelCaps && modelCaps.known === false && html`
            <span class="cap-warn" title="This model is not in the provider's catalog">unknown model</span>
          `}
          ${!benchMode && modelCaps && modelCaps.known && !modelCaps.tools && html`
            <span class="cap-warn" title="This model cannot call tools — the builder needs tool calling">no tools</span>
          `}
          ${shareSubject && html`
            <div class="share-anchor">
              <button
                type="button"
                class="header-action quiet share-button ${shareShown ? 'active' : ''}"
                aria-expanded=${shareShown}
                disabled=${shareBlocked}
                title=${shareBlocked
                  ? "Wait for the bench's running turns before sharing"
                  : shareSubject.kind === 'bench' ? "Publish this bench's builds to a public page" : "Publish this build's 3D view to a public page"}
                onClick=${() => (shareShown ? setShareFor(null) : openShare(shareSubject).catch(showError))}
              ><${Icon} name="share" size="14" /> Share</button>
              ${shareShown && html`
                <${SharePanel}
                  subject=${shareSubject}
                  hiddenNames=${shareSubject.kind === 'bench' && viewedBench !== null && hidesNames(viewedBench)}
                  share=${shares[shareKey(shareSubject)] ?? SHARE_UNKNOWN}
                  repo=${shareRepo}
                  onPublish=${() => publishShare(shareSubject).catch(showError)}
                  onRemove=${() => removeShare(shareSubject).catch(showError)}
                  onSettings=${() => { setShareFor(null); openSettings('general').catch(showError); }}
                  onClose=${closeShare}
                  onError=${showError}
                />
              `}
            </div>
          `}
        </div>
        <div class="app-notices-anchor">
          ${notices.length > 0 && html`
          <div class="app-notices" role="alert">
            ${notices.map((notice) => html`
              <div class="msg error app-notice" key=${notice.id}>
                <span>${notice.text}</span>
                <button
                  class="error-dismiss"
                  title="Dismiss error"
                  aria-label="Dismiss error"
                  onClick=${() => setNotices((current) => current.filter((entry) => entry.id !== notice.id))}
                ><${Icon} name="x" size="13" /></button>
              </div>
            `)}
          </div>
          `}
        </div>
        ${promptView && promptsReady && html`
          <${PromptEditor}
            prompts=${prompts}
            activeId=${shownPromptId}
            providers=${providers}
            favorites=${favorites}
            onSetup=${(view) => openSettings(view).catch(showError)}
            onToggleFavorite=${toggleFavorite}
            onCreate=${createPrompt}
            onUpdate=${updatePrompt}
            onError=${showError}
          />
        `}
        ${transcriptShown && html`<div class="messages" ref=${messagesRef}>
          ${!benchMode && transcriptState.phase === 'shown' && items.length === 0 && !busy && (preparing ? html`
            <${SetupStatus} status=${preparing} noAI=${noAI} onConnect=${connectAI}
              onRetry=${() => api('/api/setup/retry', { method: 'POST' }).then(setSetup).catch(showError)} />
          ` : html`
            <div class="empty-hint">
              ${grassBlock(56)}
              <div class="big">What should we build?</div>
              ${noAI ? connectHint : html`<div class="empty-sub">Or have several models build it in <button type="button" class="text-link" onClick=${benchFromBuild}>Bench</button></div>`}
            </div>
          `)}
          ${transcript}
          ${transcriptState.phase === 'failed' && html`
            <div class="msg error" role="alert">
              <span>This chat could not be read: ${transcriptState.error}</span>
              <button type="button" class="text-link" onClick=${readAgain}>Read again</button>
            </div>
          `}
          ${busy && activeRow?.runningSince && html`<div class="pending"><${Ticker} startedAt=${activeRow.runningSince} status=${turn.phase === 'detached' ? 'Still working, shown when it ends' : turn.status} /></div>`}
        </div>`}
        ${place.kind === 'new-bench' && html`
          <div class="messages bench-new">
            <div class="empty-hint bench-hint">
              ${grassBlock(56)}
              <div class="big">What should they build?</div>
              <div class="bench-models bench-lineup">
                ${benchModels.map((entry, index) => html`
                  <${BenchModelChip}
                    key=${`${entry.provider}::${entry.model}::${entry.effort}`}
                    entry=${entry}
                    taken=${benchModels.filter((item) => item.provider === entry.provider && item.model === entry.model).map((item) => item.effort)}
                    onEffort=${(value) => setBenchModels((current) => current.map((item, i) => (i === index ? { ...item, effort: value } : item)))}
                    onCopy=${(value, shown) => setBenchModels((current) => (current.length < 8
                      ? [...current.slice(0, index), { ...entry, effort: shown }, { ...entry, effort: value }, ...current.slice(index + 1)]
                      : current))}
                    onRemove=${() => setBenchModels((current) => current.filter((_, i) => i !== index))}
                  />
                `)}
                ${listReady && html`
                  <div class="bench-picker" onClickCapture=${fitBenchPicker}>
                    <${ModelPicker}
                      providers=${providers}
                      value=${null}
                      selected=${benchModels}
                      onToggle=${(next) => setBenchModels((current) => (
                        current.some((item) => item.provider === next.provider && item.model === next.model)
                          ? current.filter((item) => item.provider !== next.provider || item.model !== next.model)
                          : current.length < 8 ? [...current, { ...next, effort: '' }] : current
                      ))}
                      favorites=${favorites}
                      onToggleFavorite=${toggleFavorite}
                      requireTools=${true}
                      onSetup=${(view) => openSettings(view).catch(showError)}
                      label="Bench models"
                      emptyLabel=${benchModels.length ? 'Add' : 'Models'}
                    />
                  </div>
                `}
              </div>
              ${noAI ? connectHint : html`
                ${!benchReady && html`<div class="bench-need">Choose at least two models</div>`}
                <div class="empty-sub">Or have one model build it in <button type="button" class="text-link" onClick=${buildFromBench}>Build</button></div>
              `}
            </div>
          </div>
        `}
        ${place.kind === 'compare' && viewedBench && renderBenchCompare()}
        ${place.kind === 'leaderboard' && html`
          <div class="messages leaderboard">
            ${leaderboard && !leaderboard.length && html`<div class="empty-hint"><div class="big">No blind votes yet</div><div>Vote on a blind bench's rounds and the models are ranked here.</div></div>`}
            ${leaderboard?.length > 0 && html`
              <div class="leaderboard-table" role="table" aria-label="Models ranked by blind votes">
                <div class="lb-row lb-head" role="row">
                  <span role="columnheader">#</span><span role="columnheader">Model</span><span role="columnheader">Rating</span><span role="columnheader">Picked</span><span role="columnheader">Benches</span>
                </div>
                ${leaderboard.map((entry, index) => html`
                  <div class="lb-row" role="row" key=${`${entry.model}:${entry.effort}`}>
                    <span>${index + 1}</span>
                    <span class="lb-model"><${Logo} name=${vendorIcon(entry.provider, entry.model)} size="14" />${modelLabel(entry.model)}${entry.effort && html`<span class="bench-effort">${entry.effort}</span>`}</span>
                    <span>${entry.rating}</span>
                    <span title=${`Picked as best in ${entry.wins} of the ${entry.votes} votes it was in`}>${entry.wins} / ${entry.votes}</span>
                    <span>${entry.benches}</span>
                  </div>
                `)}
              </div>
            `}
          </div>
        `}
        ${composerShown && html`<div class="composer">
          ${held === draftKey && preparing && html`
            <div class="composer-held" role="status">
              ${!preparing.failed && html`<span class="working-dots" aria-hidden="true"><i></i><i></i><i></i></span>`}
              <span>${heldLine(preparing)}</span>
              <button type="button" class="text-link" onClick=${() => setHeld(null)}>Don't send</button>
            </div>
          `}
          <div class="composer-box ${busy ? 'busy' : ''}" ref=${composerBoxRef}>
            ${palette && html`
              <${PromptPalette}
                prompts=${prompts}
                query=${palette.query}
                index=${palette.index}
                withSearch=${palette.withSearch}
                onQuery=${(query) => setPalette((current) => ({ ...current, query, index: 0 }))}
                onIndex=${(index) => setPalette((current) => ({ ...current, index }))}
                onPick=${(prompt) => insertPrompt(prompt)}
                onEdit=${(prompt) => { setPalette(null); openPrompt(prompt.id); }}
                onToggleFavorite=${togglePromptFavorite}
                canSaveDraft=${hasText}
                onSaveDraft=${() => { try { saveDraftAsPrompt(); } catch (error) { showError(error); } }}
                onRetry=${promptLibrary.phase === 'failed' ? loadPrompts : null}
                onKey=${paletteKey}
              />
            `}
            ${!benchMode && selections.length > 0 && html`
              <div class="composer-selections">${selections.map((selection) => html`<${SelectionChip} key=${selection.letter} selection=${selection} draft=${true} onHover=${setHoveredSelection} onRemove=${removeSelection} />`)}</div>
            `}
            ${(attachments.length > 0 || reading.length > 0) && html`
              <div class="composer-attachments">
                ${attachments.map((file, i) => html`
                  <div class="composer-attachment">
                    ${file.type.startsWith('image/')
                      ? html`<img src=${file.data || file.url} alt="" />`
                      : html`<span class="file-tile"><${Icon} name="file" size="16" /></span>`}
                    <span class="attachment-copy">
                      <b title=${file.name}>${file.name}</b>
                      <small>${file.type.split('/').pop() || 'file'}</small>
                    </span>
                    <button title="Remove attachment" aria-label="Remove attachment" onClick=${() => setAttachments((prev) => prev.filter((_, j) => j !== i))}>
                      <${Icon} name="x" size="12" />
                    </button>
                  </div>
                `)}
                ${reading.map((file) => html`
                  <div class="composer-attachment reading" role="status">
                    <span class="file-tile"><span class="working-dots"><i></i><i></i><i></i></span></span>
                    <span class="attachment-copy">
                      <b title=${file.name}>${file.name}</b>
                      <small>Reading…</small>
                    </span>
                  </div>
                `)}
              </div>
            `}
            <${ComposerInput}
              inputRef=${composerRef}
              value=${composerValue}
              placeholder=${place.kind === 'compare' ? 'Follow up with every model' : prompted ? '' : idea}
              onChange=${onComposerChange}
              onKeyDown=${onKeyDown}
              onPaste=${onPaste}
              onCaret=${(range) => { caretRef.current = range; }}
            />
            <div class="composer-footer">
              <button class="attach" title="Attach file" aria-label="Attach file" onClick=${() => fileInputRef.current?.click()}>
                <${Icon} name="plus" size="17" />
              </button>
              <input
                ref=${fileInputRef}
                type="file"
                multiple
                style="display:none"
                onChange=${(evt) => {
                  addFiles([...evt.target.files]).catch(showError);
                  evt.target.value = '';
                }}
              />
              <button
                class="attach ${palette?.withSearch ? 'active' : ''}"
                title="Prompts (type / in the message)"
                aria-label="Insert a saved prompt"
                onClick=${() => setPalette((current) => (current ? null : { query: '', index: 0, slash: null, withSearch: true }))}
              ><${Icon} name="prompts" size="15" /></button>
              ${place.kind === 'new-bench' && html`
                <button
                  type="button"
                  class="composer-toggle ${benchBlind ? 'on' : ''}"
                  aria-pressed=${benchBlind}
                  title=${benchBlind ? 'Blind: models go by letters until you reveal them' : 'Names shown: turn on to judge without knowing which model is which'}
                  onClick=${() => setBenchBlind((on) => !on)}
                ><${Icon} name=${benchBlind ? 'eye-off' : 'eye'} size="14" /> Blind</button>
              `}
              <div class="composer-controls">${composerControls}</div>
            </div>
          </div>
        </div>`}
      </section>

      <div
        class="strata"
        hidden=${promptView}
        title="Drag to resize"
        onMouseDown=${startDividerDrag}
      ></div>

      <section
        class="viewer ${viewerFull ? 'full' : ''}"
        hidden=${promptView}
        title=${viewerFull ? 'Double-click or press Esc to exit fullscreen' : 'Double-click to expand'}
        onDblClick=${() => setViewerFull((v) => !v)}
      >
        <${BlueMapViewer}
          live=${minecraftLive}
          world=${liveWorldRef.current}
          exportName=${exportName}
          onError=${showError}
          activeId=${viewKey}
          labels=${viewerLabels}
          focus=${viewerFocus}
          emptyText=${benchMode ? null : preparing ? 'The world shows here once the app is ready.' : 'Open a chat to view its world.'}
          selections=${viewerSelections}
          onSelect=${benchMode ? null : (box) => addSelection(box).catch(showError)}
          onDeselect=${(key) => removeSelection(key.slice('draft-'.length))}
          binds=${binds}
          onBind=${runBind}
          typeAhead=${typeAhead && !viewerFull}
          onType=${(key) => typeKeyRef.current(key)}
          full=${viewerFull}
          onFull=${setViewerFull}
          onEscape=${closeTopLayer}
          selectToggle=${selectToggle}
        />
      </section>

      ${command && renderCommand()}

      ${lightbox && html`
        <div class="lightbox" onClick=${() => setLightbox(null)}>
          <img src=${lightbox} />
        </div>
      `}

      ${settings && html`
        <div class="overlay" onClick=${(evt) => { if (evt.target === evt.currentTarget) closeSettings(); }}>
          <div class="settings-panel sidebar">
            <div class="settings-body" inert=${panel.phase === 'saving'}>
              <nav class="settings-nav" aria-label="Settings sections">
                <div class="settings-nav-title">Settings</div>
                <button type="button" class="settings-nav-item connect ${panel.view === CONNECT ? 'active' : ''}" onClick=${() => showSettingsPane(CONNECT)}>
                  <${Icon} name="plug" size="16" />
                  <span class="name">Connect an AI</span>
                </button>
                <button type="button" class="settings-nav-item single ${panel.view === 'general' ? 'active' : ''}" onClick=${() => showSettingsPane('general')}>
                  <${Icon} name="wrench" size="16" />
                  <span class="name">General</span>
                </button>
                <div class="settings-nav-label">Providers</div>
                ${Object.entries(settings.providers).filter(([, connection]) => !connection.harness).map(([id, connection]) => html`
                  <${SettingsNavItem} id=${id} connection=${connection} active=${panel.view === id}
                    onSelect=${showSettingsPane}
                    onToggle=${(target, on) => updateConnection(target, 'enabled', on)} />
                `)}
                <div class="settings-nav-label">Harnesses</div>
                ${Object.entries(settings.providers).filter(([, connection]) => connection.harness).map(([id, connection]) => html`
                  <${SettingsNavItem} id=${id} connection=${connection} active=${panel.view === id}
                    onSelect=${showSettingsPane}
                    onToggle=${(target, on) => updateConnection(target, 'enabled', on)} />
                `)}
              </nav>

              <div class="settings-main">
              <button class="panel-close settings-close" title="Close settings" aria-label="Close settings" onClick=${closeSettings}>
                <${Icon} name="x" size="17" />
              </button>
              <div class="settings-pane">
                ${panel.view === 'general' ? html`
                  <div class="pane-head"><h3>General</h3></div>
                  ${hosted && html`
                    <${SettingsSection} title="Account">
                      <${SettingsRow} title="Your account" help="Sign out of the website, or delete this workspace and everything in it.">
                        <a class="btn" href="/account">Manage account</a>
                      <//>
                    <//>
                  `}
                  <${SettingsSection} title="Conversations">
                    <${SettingsRow} title="Title model" tip="Names each new chat from its first message">
                      <div class="title-model-controls">
                        <${ModelPicker}
                          providers=${settings.providers}
                          value=${settings.titleModel}
                          onChange=${(next) => editSettings((draft) => withTitleModel(draft, next))}
                          favorites=${favorites}
                          onToggleFavorite=${toggleFavorite}
                          label="Title model"
                          emptyLabel="The chat's own model"
                          onSetup=${showSettingsPane}
                        />
                        ${titleModelCaps?.reasoning && titleModelCaps.efforts.length > 0 && html`
                          <${EffortPicker}
                            caps=${titleModelCaps}
                            value=${settings.titleModel.effort || ''}
                            label="Title model reasoning effort"
                            onChange=${(effort) => editSettings((draft) => ({
                              ...draft,
                              titleModel: { ...draft.titleModel, effort },
                            }))}
                          />
                        `}
                        ${settings.titleModel && html`
                          <button
                            type="button"
                            class="text-link"
                            onClick=${() => editSettings((draft) => withTitleModel(draft, null))}
                          >Use the chat's own model</button>
                        `}
                      </div>
                    <//>
                  <//>
                  <${SettingsSection} title="Appearance">
                    <${SettingsRow} title="Theme" tip="Applies right away on this browser">
                      <div class="theme-picker" role="group" aria-label="Color theme">
                        <button
                          type="button"
                          class=${theme === 'light' ? 'active' : ''}
                          aria-pressed=${theme === 'light'}
                          onClick=${() => setTheme('light')}
                        ><${Icon} name="sun" size="14" /> Light</button>
                        <button
                          type="button"
                          class=${theme === 'dark' ? 'active' : ''}
                          aria-pressed=${theme === 'dark'}
                          onClick=${() => setTheme('dark')}
                        ><${Icon} name="moon" size="14" /> Dark</button>
                      </div>
                    <//>
                  <//>
                  <${SettingsSection} title="Sharing">
                    <${SettingsRow} title="GitHub repository for shared builds" help="A public repository with GitHub Pages on. Needs git and gh signed in on this machine.">
                      <input
                        id="share-repo"
                        class="s-input mono"
                        autocomplete="off"
                        spellcheck="false"
                        placeholder="owner/name"
                        value=${settings.shareRepo ?? ''}
                        onInput=${(evt) => editSettings((draft) => withShareRepo(draft, evt.target.value))}
                      />
                    <//>
                    <${PublishedBuilds}
                      published=${published}
                      onRetry=${loadPublished}
                      onRemove=${(build) => removePublished(build).catch(showError)}
                      onGoTo=${(subject) => {
                        closeSettings();
                        if (subject.kind === 'bench') showBenchView({ kind: 'compare', benchId: subject.id });
                        else openConversation(subject.id).catch(showError);
                      }}
                      onError=${showError}
                    />
                  <//>
                  <${BindsSection}
                    binds=${binds}
                    onChange=${(action, combo) => setBinds((current) => Object.fromEntries(Object.entries({ ...current, [action]: combo })
                      .map(([id, value]) => [id, id !== action && value === combo ? null : value])))}
                    onReset=${() => setBinds(DEFAULT_BINDS)}
                  />
                  <${DangerZone}
                    onClearVotes=${async () => {
                      await api('/api/votes', { method: 'DELETE' });
                      detailsRef.current.latest = {}; // reads still on their way hold the old votes
                      for (const [id, details] of Object.entries(benchDetailsRef.current)) {
                        if (details.data) setBenchDetails(id, detailsVotes(details, {}));
                      }
                      setLeaderboard([]);
                    }}
                    onError=${showError}
                  />
                ` : panel.view === CONNECT ? html`
                  <${ConnectPane} providers=${settings.providers} onSelect=${showSettingsPane}
                    onConnect=${(id) => { setSignInStart({ id, method: settings.providers[id].signIn.methods?.[0] }); showSettingsPane(id); }} />
                ` : html`
                  <${ProviderPane}
                    id=${panel.view}
                    connection=${settings.providers[panel.view]}
                    favorites=${favorites}
                    onToggleFavorite=${toggleFavorite}
                    onChange=${(field, value) => updateConnection(panel.view, field, value)}
                    checkedAt=${settings.checkedAt}
                    onRecheck=${() => recheckSettings().catch(showError)}
                    startWith=${signInStart?.id === panel.view ? signInStart : null}
                    onStarted=${() => setSignInStart(null)}
                    onError=${showError}
                  />
                `}
              </div>
              ${panel.error && html`
                <div class="settings-error">
                  <span>${panel.error}</span>
                  <button
                    class="error-dismiss"
                    title="Dismiss error"
                    aria-label="Dismiss error"
                    onClick=${() => editSettings((draft) => draft)}
                  ><${Icon} name="x" size="13" /></button>
                </div>
              `}
              <div class="panel-actions">
                <button class="btn" disabled=${panel.phase === 'saving'} onClick=${closeSettings}>Cancel</button>
                <button class="btn gold" disabled=${panel.phase === 'saving'} onClick=${() => saveSettings().catch(showError)}>${panel.phase === 'saving' ? 'Saving…' : 'Save changes'}</button>
              </div>
              </div>
            </div>
          </div>
        </div>
      `}
    </div>
  `;
}

// A render (or an effect) that throws replaces the page with what went
// wrong and a way out, not a frozen or blank page. Notices live in the same
// tree, so they could not say it.
function ErrorBoundary({ children }) {
  const [error] = useErrorBoundary((caught) => console.error(caught));
  if (!error) return children;
  return html`
    <div class="crash" role="alert">
      <h1>The page stopped on an error</h1>
      <pre>${String(error.message || error)}</pre>
      <button type="button" class="btn gold" onClick=${() => location.reload()}>Reload</button>
    </div>
  `;
}

// The app replaces the loading note in index.html.
const root = document.getElementById('app');
root.textContent = '';
render(h(ErrorBoundary, null, h(App)), root);
