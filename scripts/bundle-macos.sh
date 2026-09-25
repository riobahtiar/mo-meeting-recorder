#!/bin/sh
# Assembles a self-contained MOM Recorder.app in target/bundle from the
# release build: Mach-O binaries, rewritten dylibs, schemas and icons.
# Unsigned unless APPLE_IDENTITY names a Developer ID certificate in the
# keychain; signing and notarization are plan 08 steps 6-7.
set -eu
cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
APP="target/bundle/MOM Recorder.app"
C="$APP/Contents"

scripts/build-macos.sh --features metal
rm -rf "$APP"
mkdir -p "$C/MacOS" "$C/Frameworks" "$C/Resources/share/glib-2.0/schemas"

sed "s/@VERSION@/$VERSION/g" packaging/macos/Info.plist > "$C/Info.plist"
printf 'APPL????' > "$C/PkgInfo"
cp target/release/momr target/release/momr-audio target/release/momr-menubar "$C/MacOS/"
cp "$(brew --prefix ffmpeg)/bin/ffmpeg" "$(brew --prefix ffmpeg)/bin/ffprobe" "$C/MacOS/"
if [ -f data/icon/MOMRecorder.icns ]; then
  cp data/icon/MOMRecorder.icns "$C/Resources/"
else
  echo "bundle-macos.sh: data/icon/MOMRecorder.icns is missing (plan 07 step 10); the bundle has no icon yet" >&2
fi
"$(brew --prefix glib)/bin/glib-compile-schemas" \
  --targetdir "$C/Resources/share/glib-2.0/schemas" \
  "$(brew --prefix)/share/glib-2.0/schemas"
rsync -a --include '*/' --include '*symbolic*' --include 'index.theme' --exclude '*' \
  "$(brew --prefix)/share/icons/Adwaita/" "$C/Resources/share/icons/Adwaita/"
mkdir -p "$C/Resources/share/icons/hicolor"
cp "$(brew --prefix)/share/icons/hicolor/index.theme" "$C/Resources/share/icons/hicolor/"
# Every non-system dylib each Mach-O needs, with install names rewritten.
# -of overwrites shared libraries between binaries; -od would wipe the whole
# destination on every pass, leaving only the last binary's set.
for bin in "$C/MacOS/"*; do
  dylibbundler -of -b -x "$bin" -d "$C/Frameworks/" -p '@executable_path/../Frameworks/' >/dev/null
done

if [ -n "${APPLE_IDENTITY:-}" ]; then
  find "$C/Frameworks" -name '*.dylib' -exec codesign --force --timestamp --options runtime --sign "$APPLE_IDENTITY" {} \;
  for b in momr-audio ffmpeg ffprobe; do
    codesign --force --timestamp --options runtime --sign "$APPLE_IDENTITY" "$C/MacOS/$b"
  done
  codesign --force --timestamp --options runtime --entitlements packaging/macos/entitlements.plist --sign "$APPLE_IDENTITY" "$APP"
  codesign --verify --deep --strict --verbose=2 "$APP"
else
  echo "bundle-macos.sh: APPLE_IDENTITY is unset; the bundle is unsigned (right-click Open once to launch)" >&2
fi
echo "bundle-macos.sh: $APP (version $VERSION)"
