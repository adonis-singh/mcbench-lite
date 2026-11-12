'use strict';

const { chatCompletion } = require('./llm');

// Turns a rough build request into a prompt for an AI builder: clearer, not
// longer than it needs to be, and never inventing what the user didn't ask
// for. The source is the prompt's own text or a short note the user typed.
const INSTRUCTIONS = `Rewrite the user's Minecraft build request as a prompt for an AI builder.
Keep everything they asked for. Add only the detail needed so it is not ambiguous, such as rough scale, the main parts and how they fit together. Do not add specifics they did not ask for or imply.
Where a choice is likely to change each time the prompt is used, write it as a fill-in in double braces, like {{style}} or {{size}}.
Return only the prompt text.`;

async function generatePrompt(input, complete = chatCompletion) {
  const provider = String(input.provider || '').trim();
  const model = String(input.model || '').trim();
  const text = String(input.text || '').trim();
  if (!provider) throw new Error('Choose a provider');
  if (!model) throw new Error('Choose a model');
  if (!text) throw new Error('Write something to improve first');

  const message = await complete({
    provider,
    model,
    effort: String(input.effort || '').trim(),
    messages: [{ role: 'system', content: INSTRUCTIONS }, { role: 'user', content: text }],
    tools: [],
  });
  const generated = String(message.content || '').trim();
  if (!generated) throw new Error('The model returned an empty prompt');
  return generated;
}

module.exports = { generatePrompt, INSTRUCTIONS };
