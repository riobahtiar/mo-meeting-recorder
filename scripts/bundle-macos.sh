#!/bin/sh
# Assembles a self-contained MOM Recorder.app in target/bundle from the
# release build: Mach-O binaries, rewritten dylibs, schemas and icons.
# Signed with APPLE_IDENTITY when it names a Developer ID certificate in the
# keychain, ad hoc otherwise; notarization is plan 08 step 7.
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
# -L: Homebrew's icon files are relative symlinks into the Cellar, which
# would dangle inside the bundle (missing icons, and a seal that fails).
rsync -aL --include '*/' --include '*symbolic*' --include 'index.theme' --exclude '*' \
  "$(brew --prefix)/share/icons/Adwaita/" "$C/Resources/share/icons/Adwaita/"
mkdir -p "$C/Resources/share/icons/hicolor"
cp "$(brew --prefix)/share/icons/hicolor/index.theme" "$C/Resources/share/icons/hicolor/"
# Every non-system dylib each Mach-O needs, with install names rewritten.
# -of overwrites shared libraries between binaries; -od would wipe the whole
# destination on every pass, leaving only the last binary's set.
for bin in "$C/MacOS/"*; do
  dylibbundler -of -b -x "$bin" -d "$C/Frameworks/" -p '@executable_path/../Frameworks/' >/dev/null
done

# Always sign, the same way either way: install_name_tool breaks the
# signatures dylibbundler leaves, and on Apple silicon a Mach-O with a broken
# signature is killed at launch. With APPLE_IDENTITY (a Developer ID) the
# bundle is ready for notarization; without it, it is signed ad hoc ("-"),
# which runs on this Mac with the same hardened runtime and entitlements, so
# a local build behaves like a shipped one. Ad-hoc signatures change with
# every build, so macOS asks for Microphone and System Audio Recording again
# after a rebuild.
IDENTITY="${APPLE_IDENTITY:--}"
if [ "$IDENTITY" = "-" ]; then
  TIMESTAMP="--timestamp=none"
else
  TIMESTAMP="--timestamp"
fi
sign() {
  codesign --force "$TIMESTAMP" --options runtime --sign "$IDENTITY" "$@"
}
find "$C/Frameworks" -name '*.dylib' | while IFS= read -r lib; do sign "$lib"; done
# Every executable in Contents/MacOS is signed on its own (--deep on the
# verify below rejects an unsigned one). The two that open the microphone,
# the helper's `mic` and ffmpeg's avfoundation fallback, run as their own
# processes under the hardened runtime, so they carry the audio-input
# entitlement themselves; the app's entitlement does not reach them.
for b in momr-audio ffmpeg; do
  sign --entitlements packaging/macos/entitlements.plist "$C/MacOS/$b"
done
# ffprobe loads the bundled ffmpeg dylibs too, so it needs library
# validation off like ffmpeg, but not the microphone. momr-menubar links
# only system frameworks.
sign --entitlements packaging/macos/entitlements-tools.plist "$C/MacOS/ffprobe"
sign "$C/MacOS/momr-menubar"
sign --entitlements packaging/macos/entitlements.plist "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"
if [ "$IDENTITY" = "-" ]; then
  echo "bundle-macos.sh: APPLE_IDENTITY is unset; the bundle is signed ad hoc for this Mac only (not notarizable)" >&2
fi
echo "bundle-macos.sh: $APP (version $VERSION)"
