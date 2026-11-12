function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, (char) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
  })[char]);
}

export function readBlobAsDataUrl(blob) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result);
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(blob);
  });
}

export async function messageClipboardHtml({ text = '', attachments = [] }) {
  const files = await Promise.all(attachments.map(async (file) => {
    let data = file.data;
    if (!data) {
      const response = await fetch(file.url);
      if (!response.ok) throw new Error(`Could not copy "${file.name}" (HTTP ${response.status})`);
      data = await readBlobAsDataUrl(new Blob([await response.blob()], { type: file.type }));
    }
    return { name: file.name, type: file.type, size: file.size, data };
  }));
  // Keep the bytes on the clipboard: copies survive reloads and source removal.
  const payload = escapeHtml(JSON.stringify({ text, attachments: files }));
  const body = escapeHtml(text).replace(/\n/g, '<br>');
  const names = files.map((file) => `<p>${escapeHtml(file.name)}</p>`).join('');
  return `<div data-mcbench-message="${payload}"><p>${body}</p>${names}</div>`;
}

export function writeMessageClipboard(message) {
  // Start the clipboard write in the click gesture, while file loading resolves.
  return navigator.clipboard.write([new ClipboardItem({
    'text/html': messageClipboardHtml(message).then((html) => new Blob([html], { type: 'text/html' })),
    'text/plain': new Blob([message.text || ''], { type: 'text/plain' }),
  })]);
}

// A paste can come from any page, so a message is rebuilt from what a copy
// writes: text, and attachments that carry their bytes as base64 data URLs.
export function readMessageClipboard(clipboardData) {
  const html = clipboardData.getData('text/html');
  if (!html) return null;
  const document = new DOMParser().parseFromString(html, 'text/html');
  const element = document.querySelector('[data-mcbench-message]');
  if (!element) return null;
  const message = JSON.parse(element.getAttribute('data-mcbench-message'));
  const attachments = Array.isArray(message?.attachments) ? message.attachments : [];
  if (attachments.some((file) => typeof file?.name !== 'string' || typeof file.data !== 'string' || !/^data:[^;,]+;base64,/.test(file.data))) {
    throw new Error('The pasted message holds an attachment that is not a file');
  }
  return {
    text: String(message.text ?? ''),
    attachments: attachments.map((file) => {
      const base64 = file.data.slice(file.data.indexOf(',') + 1);
      const size = Math.floor(base64.length * 3 / 4) - (base64.endsWith('==') ? 2 : base64.endsWith('=') ? 1 : 0);
      return { name: file.name, type: file.data.slice(5, file.data.indexOf(';')), size, data: file.data };
    }),
  };
}

export function insertClipboardText(textarea, text) {
  const start = textarea.selectionStart;
  const end = textarea.selectionEnd;
  const value = textarea.value.slice(0, start) + text + textarea.value.slice(end);
  requestAnimationFrame(() => textarea.setSelectionRange(start + text.length, start + text.length));
  return value;
}
