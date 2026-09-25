#!/bin/sh
# Builds MOM Recorder for macOS: the Rust binary plus the momr-audio Swift
# helper, placed next to the binary so helper::path() finds it. Extra arguments
# go to cargo (e.g. --features metal).
set -eu
cd "$(dirname "$0")/.."
cargo build --release "$@"
swift build -c release --package-path helpers/momr-audio
cp helpers/momr-audio/.build/release/momr-audio target/release/momr-audio
cp helpers/momr-audio/.build/release/momr-menubar target/release/momr-menubar
mkdir -p target/debug
cp helpers/momr-audio/.build/release/momr-audio target/debug/momr-audio 2>/dev/null || true
cp helpers/momr-audio/.build/release/momr-menubar target/debug/momr-menubar 2>/dev/null || true
