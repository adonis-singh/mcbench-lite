// The saved theme, applied before the first paint (a classic script in the
// page's head), not after the app loads.
document.documentElement.dataset.theme = localStorage.getItem('mcb-theme') || 'dark';
