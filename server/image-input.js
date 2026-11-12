'use strict';

// Formats accepted by the configured vision providers. Other image formats
// remain workspace attachments, where the agent can convert and inspect them.
const NATIVE_IMAGE_TYPES = new Set(['image/jpeg', 'image/png', 'image/gif', 'image/webp']);
function supportsImageType(type) {
  return NATIVE_IMAGE_TYPES.has(String(type).toLowerCase());
}
function supportsImageData(data) {
  const type = typeof data === 'string' && data.match(/^data:([^;,]+);base64,/i)?.[1];
  return Boolean(type && supportsImageType(type));
}
module.exports = { supportsImageType, supportsImageData };
