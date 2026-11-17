# Sourced on adipc by every script here. R is this work's scratch folder;
# the Rust toolchain is the machine's existing stable one, called by its own
# bin folder so the rustup proxy never runs (nothing is installed); cargo's
# home and target live in R.
export R=${R:-$HOME/mcbench-native-scratch/s2-floor}
export PATH=$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:/usr/bin
export CARGO_HOME=$R/cargo-home CARGO_TARGET_DIR=$R/target CARGO_BUILD_JOBS=24
# The renderer spike's test data (read only): the baked pack, block table and
# atlas, and the builds' blueprints and job.json.
export MINI=$HOME/mcbench-native-scratch/renderer/mini
export DATA=$MINI/data
