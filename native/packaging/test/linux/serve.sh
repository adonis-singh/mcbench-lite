#!/usr/bin/env bash
# Starts or stops the staging server: SERVER (env.sh) on the Docker network
# NET, serving <staging folder> read-only with server.py, with no port on
# the host. The network is made by start and removed by stop.
#
#   native/packaging/test/linux/serve.sh start <staging folder>
#   native/packaging/test/linux/serve.sh stop
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/env.sh"
case ${1:-} in
  start)
    [ $# -eq 2 ] || { echo "usage: serve.sh start <staging folder>" >&2; exit 2; }
    staging=$(cd "$2" && pwd)
    docker network inspect "$NET" >/dev/null 2>&1 || docker network create "$NET" >/dev/null
    docker run -d --rm --cpus 1 --name "$SERVER" --network "$NET" -v "$staging:/srv:ro" -v "$here/server.py:/server.py:ro" \
      "$SERVER_IMAGE" python3 -u /server.py /srv >/dev/null
    echo "$SERVER on $NET: $STAGING_RELEASES and $STAGING_SITE"
    ;;
  stop)
    docker stop "$SERVER" >/dev/null
    docker network rm "$NET" >/dev/null
    echo "$SERVER and $NET removed"
    ;;
  *) echo "usage: serve.sh start <staging folder> | stop" >&2; exit 2 ;;
esac
