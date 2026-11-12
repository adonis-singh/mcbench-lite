#!/bin/sh
# Makes jmap_order.txt with the GraalVM JDK 25 the engine's reference uses.
# Usage: MCBENCH_GRAALVM_HOME=<GraalVM JDK 25 home> make.sh seed count out.txt [dump]
set -e
JAVA_HOME=${MCBENCH_GRAALVM_HOME:?set MCBENCH_GRAALVM_HOME to the GraalVM JDK 25 home the engine is built with (the folder that holds bin/java and lib/src.zip)}
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=${WORK:-$(mktemp -d)}
mkdir -p "$WORK/jdksrc" "$WORK/patch/java/util" "$WORK/patched" "$WORK/probe"
unzip -o -q "$JAVA_HOME/lib/src.zip" java.base/java/util/HashMap.java -d "$WORK/jdksrc"
python3 "$HERE/patch_hashmap.py" "$WORK/jdksrc/java.base/java/util/HashMap.java" "$WORK/patch/java/util/HashMap.java"
"$JAVA_HOME/bin/javac" --patch-module java.base="$WORK/patch" -d "$WORK/patched" "$WORK/patch/java/util/HashMap.java" 2>/dev/null
"$JAVA_HOME/bin/javac" -d "$WORK/probe" "$HERE/JMapOrder.java"
"$JAVA_HOME/bin/java" --patch-module java.base="$WORK/patched" -cp "$WORK/probe" JMapOrder "$@"
