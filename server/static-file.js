'use strict';

// Sends a file a browser may keep: its ETag names its size and modification
// time, so a revalidation of an unchanged file is answered 304 without
// reading it.

const fs = require('fs');
const { pipeline } = require('stream');

function sendFile(req, res, file, headers) {
  const { size, mtimeMs } = fs.statSync(file);
  const etag = `"${size.toString(36)}-${Math.floor(mtimeMs).toString(36)}"`;
  if (req.headers['if-none-match'] === etag) {
    res.writeHead(304, { etag, 'cache-control': headers['cache-control'] });
    return res.end();
  }
  res.writeHead(200, { ...headers, etag, 'content-length': size });
  if (req.method === 'HEAD') return res.end();
  // A read that fails (the file went away after the stat) ends this response;
  // an unhandled stream error would end the server.
  return pipeline(fs.createReadStream(file), res, () => {});
}

module.exports = { sendFile };
