#!/bin/sh
# Produces data/icon/MOMRecorder.icns from the Swift-drawn 1024 master.
set -eu
cd "$(dirname "$0")/.."
mkdir -p data/icon
swift data/icon/render.swift
ICONSET=data/icon/MOMRecorder.iconset
rm -rf "$ICONSET"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s data/icon/icon-1024.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  if [ "$d" -le 1024 ]; then
    sips -z $d $d data/icon/icon-1024.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
  fi
done
iconutil -c icns "$ICONSET" -o data/icon/MOMRecorder.icns
rm -rf "$ICONSET"
ls -la data/icon/MOMRecorder.icns
