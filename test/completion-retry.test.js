'use strict';
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {completionWithRetry, transientCompletionError, MAX_ATTEMPTS} = require('../server/completion-retry');

test('retry preserves request and running work, returns only the complete tool message', async () => {
  const events = [], requests = [], delays = [];
  const options = {messages: [{role:'user',content:'build'}], onDelta:d=>events.push(d)};
  const final = {tool_calls:[{id:'complete',function:{name:'build',arguments:'{}'}}]};
  const result = await completionWithRetry(async request => {
    requests.push(request);
    if(requests.length===1) {request.onDelta({toolCall:{id:'partial',args:'{"program":'}});throw new TypeError('terminated');}
    return final;
  }, options, {onRetry:e=>events.push({retry:e}),wait:async ms=>delays.push(ms)});
  assert.equal(result,final);
  assert.equal(requests.length,2);assert.ok(requests.every(r=>r===options));
  assert.deepEqual(options.messages,[{role:'user',content:'build'}]);
  assert.equal(events[1].retry.attempt,1);assert.deepEqual(delays,[1000]);
});

test('transient recovery has bounded backoff and a capped attempt count; permanent errors escape', async () => {
  const waits=[];let calls=0;
  assert.equal(await completionWithRetry(async()=>{if(++calls<MAX_ATTEMPTS)throw new Error('HTTP 503: unavailable');return 'done';}, {},{wait:async ms=>waits.push(ms)}),'done');
  assert.equal(calls,MAX_ATTEMPTS);assert.equal(Math.max(...waits),30000);
  // A provider stuck on 429 fails the turn with its last error instead of retrying forever.
  calls=0;waits.length=0;
  await assert.rejects(completionWithRetry(async()=>{throw new Error(`HTTP 429: busy ${++calls}`);},{},{wait:async ms=>waits.push(ms)}),{message:`HTTP 429: busy ${MAX_ATTEMPTS}`});
  assert.equal(calls,MAX_ATTEMPTS);assert.equal(waits.length,MAX_ATTEMPTS-1);
  for(const message of ['HTTP 401: invalid credentials','HTTP 400: bad request','invalid tool schema']) {
    let count=0;await assert.rejects(completionWithRetry(async()=>{count++;throw new Error(message);},{}),{message});assert.equal(count,1);
  }
  assert.equal(transientCompletionError(new TypeError('fetch failed',{cause:{code:'ECONNRESET'}})),true);
});

test('cancellation interrupts backoff without another model request', async () => {
  const controller=new AbortController();let calls=0;
  const result=completionWithRetry(async()=>{calls++;throw new TypeError('terminated');},{signal:controller.signal},{onRetry:()=>controller.abort(new Error('user stopped'))});
  await assert.rejects(result);assert.equal(calls,1);
});
