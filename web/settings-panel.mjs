// The Settings panel. One value:
//   closed   { phase }
//   opening  { phase, view }: the saved settings being read, to open on the
//            pane `view`
//   open     { phase, view, draft, error }: `draft` is the settings as edited
//            and not yet saved; `error` is why the last save failed, or ''
//   saving   { phase, view, draft }: the draft on its way to the server. No
//            edit, no second save and no closing until the answer: a save
//            that lands closes the panel, one that fails opens it again with
//            its error
export const CLOSED = Object.freeze({ phase: 'closed' });

// The phases each phase can go to. Opening goes on opening when another pane
// is asked for meanwhile, and closes if the read fails.
const NEXT = {
  closed: ['opening'],
  opening: ['opening', 'open', 'closed'],
  open: ['open', 'saving', 'closed'],
  saving: ['open', 'closed'],
};

// The panel after `panel`: `next`, if it can follow it.
export function panelTo(panel, next) {
  if (!NEXT[panel.phase].includes(next.phase)) throw new Error(`Settings cannot go from ${panel.phase} to ${next.phase}`);
  if ((next.phase === 'open' || next.phase === 'saving') !== Boolean(next.draft)) {
    throw new Error('Open settings, and only they, hold a draft');
  }
  if ((next.phase === 'closed') !== (next.view === undefined)) throw new Error('Settings not closed, and only they, are on a pane');
  return next;
}

// The draft changed (`change` is a function of it), which also lets go of
// the last save's error. Only open settings can be edited: not ones being saved.
export function panelEdit(panel, change) {
  if (panel.phase !== 'open') throw new Error(`Settings cannot be edited while ${panel.phase}`);
  return { ...panel, draft: change(panel.draft), error: '' };
}

// The draft shown, or null: closed settings and ones still opening have none.
export const panelDraft = (panel) => (panel.phase === 'open' || panel.phase === 'saving' ? panel.draft : null);

// The title model the draft names: a model picked (on its default effort),
// or null, which has each chat's own model write its title.
export const withTitleModel = (draft, choice) => ({
  ...draft,
  titleModel: choice && { provider: choice.provider, model: choice.model, effort: '' },
});

// The repository shared builds publish to, "owner/name" as typed; an empty
// field saves null, which turns sharing off.
export const withShareRepo = (draft, text) => ({ ...draft, shareRepo: text.trim() || null });

// A provider the page can use: connected (a key or a login) and on in Settings.
export const usable = (provider) => Boolean(provider?.connected && provider.enabled);

// The Connect an AI pane: the ways to give mcbench a model, in plain words.
export const CONNECT = 'connect';

// The pane Settings open on when none is named: Connect an AI while no
// provider can be used, General once one can.
export const firstPane = (providers) => (Object.values(providers).some(usable) ? 'general' : CONNECT);

// Each way in: what it is, what the person needs and who bills it (commands
// between backticks), and the provider panes (server/settings.js ids)
// that set it up, first the one most people want.
export const CONNECT_WAYS = Object.freeze([
  {
    key: 'api-key',
    title: 'An API key',
    need: 'Get a key on the provider\'s website and paste it into its pane. OpenRouter is the one key that reaches most models; the others reach their own. Usage is billed by that provider.',
    panes: ['openrouter', 'openai', 'anthropic', 'google', 'groq', 'qwencloud'],
  },
  {
    key: 'claude',
    title: 'Your Claude subscription, through Claude Code',
    need: 'Sign in with your Claude subscription, or an Anthropic Console account from the Claude Code pane. The app installs Claude Code first if this machine does not have it. Usage counts against that plan or account.',
    panes: ['claude-code'],
  },
  {
    key: 'chatgpt',
    title: 'Your ChatGPT subscription, through Codex CLI or ChatGPT sign-in',
    need: 'Sign in with your ChatGPT account from either; one sign-in serves both. The app installs Codex CLI first if this machine does not have it. Usage counts against your ChatGPT plan.',
    panes: ['codex-cli', 'codex'],
  },
  {
    key: 'cursor',
    title: 'Cursor',
    need: 'Needs a Cursor API key, or a sign-in from the Cursor pane. Usage counts against your Cursor account.',
    panes: ['cursor'],
  },
]);

// The ways with each pane's provider entry from `providers` (as the server
// reports them). A pane the server does not report is a mistake here.
export function connectWays(providers) {
  return CONNECT_WAYS.map((way) => ({
    ...way,
    panes: way.panes.map((id) => {
      if (!providers[id]) throw new Error(`No provider ${id} to connect`);
      return { id, connection: providers[id] };
    }),
  }));
}
