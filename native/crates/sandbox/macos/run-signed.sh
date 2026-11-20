#!/bin/sh
# cargo's runner on macOS for programs that start the bash sandbox's VM:
# signs the program ad hoc with the one entitlement the Virtualization
# framework needs (vm.entitlements), then runs it. Ad-hoc codesign shows
# nothing and asks nothing. Use it as
#
#   CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER=crates/sandbox/macos/run-signed.sh cargo test ...
#
# (from native/; an absolute path works from anywhere).
set -e
codesign --sign - --force --entitlements "$(dirname "$0")/vm.entitlements" "$1" 2> /dev/null
exec "$@"
