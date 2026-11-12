// A chat's build resolution: how many blocks a build gives its subject. A
// chat takes it with each message, a bench gives it to all its models. One
// stop per row: `feature` is about how many blocks a main feature (an eye, a
// window, a wheel) gets, against about 10 by default, and `across` the whole
// build where the subject allows; Standard leaves both to the model.
// `short` is the tag a list row shows. The server (server/prompt.js, the
// message and bench routes) and the page read this one table.
export const RESOLUTIONS = [
  { value: 1, label: 'Standard', short: '1×', feature: null, across: null },
  { value: 2, label: 'High (2×)', short: '2×', feature: 20, across: 100 },
  { value: 5, label: 'Very high (5×)', short: '5×', feature: 50, across: 250 },
  { value: 10, label: 'Maximum (10×)', short: '10×', feature: 100, across: 500 },
];

export function resolutionStop(value) {
  const stop = RESOLUTIONS.find((entry) => entry.value === value);
  if (!stop) throw new Error(`${JSON.stringify(value)} is not a resolution (${RESOLUTIONS.map((entry) => entry.value).join(', ')})`);
  return stop;
}

// What one stop does, in the words of server/prompt.js; the picker shows it
// under the slider.
export function resolutionHelp(stop) {
  return stop.feature === null
    ? 'The model chooses the size.'
    : `About ${stop.feature} blocks for a main feature such as an eye or a window. About ${stop.across} blocks across the whole build.`;
}
