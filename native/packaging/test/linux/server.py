# The staging release's server, run by serve.sh in its own container.
#
# Port 8000 answers as GitHub's releases do: /releases/latest/download/<name>
# is a 302 to /releases/download/<tag>/<name> (the tag in releases/LATEST),
# which is a 302 to /assets/<tag>/<name> (GitHub's second redirect, to its
# storage host), which serves releases/download/<tag>/<name>. Port 8001 serves
# site/ as GitHub Pages does, and also aur/ for git's dumb HTTP clone.
# Every request is logged with its byte count on stderr.
import functools
import http.server
import os
import sys
import threading

ROOT = sys.argv[1]


class Releases(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        path = self.path.split('?')[0]
        if path.startswith('/releases/latest/download/'):
            with open(os.path.join(ROOT, 'releases', 'LATEST')) as f:
                tag = f.read().strip()
            return self.redirect(f'/releases/download/{tag}/{path.rsplit("/", 1)[1]}')
        if path.startswith('/releases/download/'):
            return self.redirect('/assets/' + path[len('/releases/download/'):])
        if path.startswith('/assets/'):
            self.path = '/releases/download/' + path[len('/assets/'):]
            return super().do_GET()
        self.send_error(404)

    def redirect(self, to):
        self.send_response(302)
        self.send_header('Location', to)
        self.send_header('Content-Length', '0')
        self.end_headers()

    def log_message(self, fmt, *args):
        sys.stderr.write('[releases] ' + fmt % args + '\n')


class Site(http.server.SimpleHTTPRequestHandler):
    def log_message(self, fmt, *args):
        sys.stderr.write('[site] ' + fmt % args + '\n')


def serve(port, handler):
    http.server.ThreadingHTTPServer(('0.0.0.0', port), handler).serve_forever()


threading.Thread(target=serve, args=(8001, functools.partial(Site, directory=os.path.join(ROOT, 'site'))), daemon=True).start()
serve(8000, functools.partial(Releases, directory=ROOT))
