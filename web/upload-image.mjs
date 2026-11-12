// An attached image as the model gets it: no edge longer than `maxEdge`, on
// white. The canvas starts transparent and JPEG has no alpha, so a
// transparent PNG, WebP or GIF drawn as is would arrive on black.
export function paintUpload(canvas, bitmap, maxEdge) {
  const scale = Math.min(1, maxEdge / Math.max(bitmap.width, bitmap.height));
  canvas.width = Math.round(bitmap.width * scale);
  canvas.height = Math.round(bitmap.height * scale);
  const context = canvas.getContext('2d');
  context.fillStyle = '#ffffff';
  context.fillRect(0, 0, canvas.width, canvas.height);
  context.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
}
