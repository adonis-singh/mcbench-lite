#!/bin/sh
# Builds the gate's macOS tools into $1: the host probes (hoststate,
# displaylog: read-only, no window, no permission) and the guest tools
# (guestctl, libguestactivate.dylib: refuse to run outside a VM). Plain
# command-line programs, ad-hoc signed by the linker.
set -eu
out=$1
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$out"
cc() { clang -O2 -fobjc-arc -Wall -Werror -Wno-unused-parameter -mmacosx-version-min=14.0 "$@"; }
cc -framework AppKit -framework CoreGraphics -o "$out/hoststate" "$here/probe/hoststate.m"
cc -framework AppKit -framework CoreGraphics -o "$out/displaylog" "$here/probe/displaylog.m"
cc -framework AppKit -framework ApplicationServices -framework Carbon -o "$out/guestctl" "$here/guest/guestctl.m"
cc -dynamiclib -framework AppKit -o "$out/libguestactivate.dylib" "$here/guest/activate.m"
