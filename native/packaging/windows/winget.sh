#!/usr/bin/env bash
# winget.sh <release folder> <version> <out folder>
#
# Writes the winget manifest of adonis-singh.mcbench-lite for <version>
# (MAJOR.MINOR.PATCH) into <out folder>: the version, installer and
# defaultLocale files of manifest schema 1.28.0, from the templates in
# winget/ beside this script, with the version and the sha256 of the release
# folder's mcbench-lite-setup-x64.exe filled in (Windows on ARM is not
# built). The installer addresses are the versioned release assets
# (releases/download/v<version>/), so the hashes stay true after later
# releases. Nothing is submitted: the owner opens the pull request to
# microsoft/winget-pkgs with these three files under
# manifests/a/adonis-singh/mcbench-lite/<version>/.
set -euo pipefail

fail() { echo "winget.sh: $*" >&2; exit 1; }

[ "$#" = 3 ] || fail "usage: winget.sh <release folder> <version> <out folder>"
release="$1" version="$2" out="$3"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "the version $version is not MAJOR.MINOR.PATCH."
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[ -f "$release/mcbench-lite-setup-x64.exe" ] || fail "the release folder $release has no mcbench-lite-setup-x64.exe."
sha_x64="$(sha256sum "$release/mcbench-lite-setup-x64.exe" | cut -d' ' -f1 | tr a-f A-F)"
mkdir -p "$out"
for template in "$here"/winget/adonis-singh.mcbench-lite*.yaml; do
  sed -e "s/@VERSION@/$version/g" -e "s/@SHA256_X64@/$sha_x64/g" \
    -e '/^# Template: /d' "$template" >"$out/${template##*/}"
  ! grep -n '@[A-Z0-9_]*@' "$out/${template##*/}" || fail "${template##*/} still holds a placeholder."
done
echo "winget.sh: $out: $(cd "$out" && ls adonis-singh.mcbench-lite*.yaml | tr '\n' ' ')"
