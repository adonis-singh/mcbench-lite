'use strict';

const { Cursor } = require('@cursor/sdk');

Cursor.auth.login({ onLoginUrl: (url) => console.log(`Open this URL to sign in to Cursor:\n${url}`) })
  .then(({ email, apiKeyExpiresAtMs }) => {
    console.log(`Signed in${email ? ` as ${email}` : ''}; the key expires ${new Date(apiKeyExpiresAtMs).toISOString()}.`);
  })
  .catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
