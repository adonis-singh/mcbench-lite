// Failures nothing else catches: an exception or a rejection no code
// handled, and a script that failed to load (app.js, or a module it
// imports), which would otherwise leave an empty page. A classic script in
// the page's head, so it listens before app.js loads; it writes into
// #page-error, outside the app, so it shows even if the app never rendered.
// The first failure stays on show: later ones usually follow from it.
(() => {
  function show(text) {
    const sink = document.getElementById('page-error');
    if (!sink.hidden) return;
    sink.querySelector('.page-error-text').textContent = text;
    sink.querySelector('button').onclick = () => location.reload();
    sink.hidden = false;
  }
  // A script's failed load does not bubble: it reaches the window only in
  // the capture phase. Other resources that fail (an image) are not failures
  // of the page.
  window.addEventListener('error', (event) => {
    if (event instanceof ErrorEvent) show(`The page hit an error: ${event.error?.message || event.message}`);
    else if (event.target instanceof HTMLScriptElement) show(`The page could not load ${new URL(event.target.src).pathname} or a module it imports.`);
  }, true);
  window.addEventListener('unhandledrejection', (event) => {
    show(`The page hit an error: ${event.reason?.message || event.reason}`);
  });
})();
