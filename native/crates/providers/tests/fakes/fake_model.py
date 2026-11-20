"""A fake OpenAI responses server for Codex: `fake_model.py <port> <log> <reply.json>`.

Each POST is logged (path, then the body, one line). The reply file holds a
list: the n-th request is answered with its n-th entry (the last one
repeats), an output item streamed as server-sent events between
response.created and response.completed.
"""
import http.server
import json
import sys
import zlib

PORT = int(sys.argv[1])
LOG = sys.argv[2]
REPLIES = json.load(open(sys.argv[3]))
count = [0]


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *args):
        pass

    def do_GET(self):
        with open(LOG, 'a') as f:
            f.write('GET %s\n' % self.path)
        body = b'{"models":[]}'
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        body = self.rfile.read(int(self.headers.get('Content-Length', '0')))
        encoding = self.headers.get('Content-Encoding', '')
        if encoding == 'zstd':
            text = '<zstd %d bytes>' % len(body)
        elif encoding == 'gzip':
            text = zlib.decompress(body, 47).decode('utf-8', 'replace')
        else:
            text = body.decode('utf-8', 'replace')
        with open(LOG, 'a') as f:
            f.write('POST %s %s\n' % (self.path, text))
        index = min(count[0], len(REPLIES) - 1)
        count[0] += 1
        rid = 'resp_%d' % count[0]
        events = [('response.created', {'response': {'id': rid}})]
        for i, item in enumerate(REPLIES[index]):
            events.append(('response.output_item.done', {'output_index': i, 'item': item}))
        events.append(('response.completed', {'response': {'id': rid, 'usage': {
            'input_tokens': 10, 'input_tokens_details': {'cached_tokens': 0},
            'output_tokens': 5, 'output_tokens_details': {'reasoning_tokens': 0},
            'total_tokens': 15}}}))
        payload = b''.join(
            ('event: %s\ndata: %s\n\n' % (name, json.dumps(dict(data, type=name)))).encode()
            for name, data in events)
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


http.server.ThreadingHTTPServer(('127.0.0.1', PORT), Handler).serve_forever()
