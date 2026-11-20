#!/usr/bin/env bash
# A staging OpenPGP key for the tests (ed25519, no passphrase), written
# armoured to <file>, which must not exist. It signs a staging release's
# repositories and RPMs only; the real key is the owner's. Never commit it.
#
#   native/packaging/test/linux/staging-key.sh <file>
set -euo pipefail
. "$(dirname "$0")/env.sh"
[ $# -eq 1 ] || { echo "usage: native/packaging/test/linux/staging-key.sh <file>" >&2; exit 2; }
[ ! -e "$1" ] || { echo "$1 exists" >&2; exit 1; }
dir=$(cd "$(dirname "$1")" && pwd)
docker run --rm --name "mcbn-s5-linux-key-$$" --cpus 1 -v "$dir:/out" "$DEBIAN_IMAGE" bash -euo pipefail -c "
apt-get -qq update && DEBIAN_FRONTEND=noninteractive apt-get -qq install -y --no-install-recommends gnupg >/dev/null
export GNUPGHOME=\$(mktemp -d)
gpg --batch --quiet --passphrase '' --quick-gen-key 'mcbench-lite staging <staging@mcbench-lite.invalid>' ed25519 sign never
gpg --batch --armor --export-secret-keys > '/out/$(basename "$1")'
chown $(id -u):$(id -g) '/out/$(basename "$1")'; chmod 600 '/out/$(basename "$1")'"
echo "$1: staging key $(grep -c . "$1") lines"
