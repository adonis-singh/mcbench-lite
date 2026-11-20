# The macOS staging server: answers on 127.0.0.1 as GitHub's releases and
# Pages do, for host-proof runs on the Mac and the guest test.
#
#   python3 -I server.py <staging> <release port> <site port>
#
# <staging> holds releases/LATEST (the tag the latest links go to, for
# example v2.0.0), releases/download/<tag>/<asset> (each release.sh run's
# release/ folder under its tag) and site/ (a release.sh run's site/). On
# the release port, /releases/latest/download/<name> is a 302 to
# /releases/download/<tag>/<name>, which serves the file; LATEST is read on
# every request, so writing another tag into it publishes that release to
# a running app. The site port serves site/. Every request is logged on
# stderr. The ports are given, and both listen on 127.0.0.1 only.
import functools
import http.server
import os
import sys
import threading

ROOT, RELEASE_PORT, SITE_PORT = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])


class Releases(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        path = self.path.split('?')[0]
        if path.startswith('/releases/latest/download/'):
            with open(os.path.join(ROOT, 'releases', 'LATEST')) as f:
                tag = f.read().strip()
            self.send_response(302)
            self.send_header('Location', f'/releases/download/{tag}/{path.rsplit("/", 1)[1]}')
            self.send_header('Content-Length', '0')
            self.end_headers()
            return
        if path.startswith('/releases/download/'):
            return super().do_GET()
        self.send_error(404)

    def log_message(self, fmt, *args):
        sys.stderr.write('[releases] ' + fmt % args + '\n')


class Site(http.server.SimpleHTTPRequestHandler):
    def log_message(self, fmt, *args):
        sys.stderr.write('[site] ' + fmt % args + '\n')


def serve(port, handler):
    http.server.ThreadingHTTPServer(('127.0.0.1', port), handler).serve_forever()


threading.Thread(target=serve, args=(SITE_PORT, functools.partial(Site, directory=os.path.join(ROOT, 'site'))), daemon=True).start()
serve(RELEASE_PORT, functools.partial(Releases, directory=ROOT))
