# 08 App bundle and distribution

## Goal

Two ways to install: `brew install` from a tap for people who have Homebrew, and a signed, notarized `MOM Recorder.app` in a DMG for everyone else. The `.app` owns its permissions (the microphone prompt names MOM Recorder), opens `.meeting-recorder` files on double-click, and runs on a Mac with nothing else installed.

## Done when

- [ ] `brew install <tap>/momr` builds and installs the binary and the helper; `momr --help` works.
- [ ] `scripts/bundle-macos.sh` produces `MOM Recorder.app` that launches on a clean user account without Homebrew.
- [ ] Double-clicking a `.meeting-recorder` file opens the meeting on the done page; the file shows the app's icon in Finder.
- [ ] The microphone and System Audio Recording prompts show the app's name and the `Info.plist` texts.
- [ ] `spctl --assess --type execute` accepts the app; the DMG opens with no Gatekeeper warning after notarization.
- [ ] GitHub Actions builds and publishes arm64 and x86_64 DMGs on a tag.

## Prerequisites

Plans 03, 04, 05, 06. Plan 07 for the icon and notifications, though the bundle can be built before. Plan 11 must confirm the bundle identifier before the first public DMG.

## Background

**Homebrew** handles every dependency (GTK, libadwaita, ffmpeg, icons, schemas) and puts the binary on `PATH`, so a formula is the shortest path to a clean install. Its limits: users need Homebrew, and there is no `.app`, so Finder integration and TCC identity are those of the terminal.

**An `.app` bundle** is a directory. For a GTK app the hard part is that GTK finds its resources through paths compiled into Homebrew's dylibs (`/opt/homebrew/share/…`). Bundling means copying the dylibs into `Contents/Frameworks`, rewriting their install names to `@executable_path/../Frameworks/…`, copying schemas and icons into `Contents/Resources`, and pointing GTK at them with environment variables before it initialises. ffmpeg and ffprobe are child processes and ship as executables with their own dylibs, found through `PATH` (plan 06 step 4 already prepends the executable's directory).

**Opening documents.** Finder does not pass a file as argv. It sends an Apple Event that GTK's macOS backend turns into the `GApplication::open` signal, so the app must set `gio::ApplicationFlags::HANDLES_OPEN` and connect `open`. Verify: GTK 4's `GdkMacosDisplay` application delegate forwards `application:openFiles:` to `g_application_open`; if not, the fallback is a tiny Objective-C shim in the helper's package, or the `NSAppleEventManager` route.

**TCC** attributes permissions to the bundle identifier of the responsible process. From the `.app`, both prompts (microphone, System Audio Recording) name MOM Recorder and show the `NSMicrophoneUsageDescription` and `NSAudioCaptureUsageDescription` strings. The helper inherits the app's identity because the app spawns it.

**Signing and notarization** need an Apple Developer account (Developer ID Application certificate). Without one, the DMG still works but users must right-click › Open once. The formula needs no signing.

## Steps

### 1. Homebrew formula

Create a tap repository (`<github-user>/homebrew-tap`) with `Formula/momr.rb`:

```ruby
# sketch
class Momr < Formula
  desc "MOM Recorder: record a meeting in two tracks and transcribe it on this computer"
  homepage "https://github.com/riobahtiar/mo-meeting-recorder"
  url "https://github.com/riobahtiar/mo-meeting-recorder/archive/refs/tags/v1.2.0.tar.gz"
  sha256 "…"
  license "MIT"

  depends_on "cmake" => :build
  depends_on "pkgconf" => :build
  depends_on "rust" => :build
  depends_on xcode: :build           # swift build for the helper
  depends_on "adwaita-icon-theme"
  depends_on "ffmpeg"
  depends_on "gtk4"
  depends_on "libadwaita"
  depends_on macos: :sonoma

  def install
    system "cargo", "install", "--features", "metal", *std_cargo_args
    system "swift", "build", "-c", "release", "--package-path", "helpers/momr-audio"
    bin.install "helpers/momr-audio/.build/release/momr-audio"
  end

  test do
    assert_match "Usage", shell_output("#{bin}/momr --help")
    system bin/"momr-audio", "list"
  end
end
```

`brew install --build-from-source ./Formula/momr.rb` locally first; `brew audit --strict` and `brew test`. Note in the README that first use asks for microphone permission for the terminal (Homebrew's binary has no bundle).

### 2. Bundle layout and `Info.plist`

`packaging/macos/Info.plist` (template; the script fills the version):

```xml
<key>CFBundleIdentifier</key>            <string>io.github.riobahtiar.MOMRecorder</string>  <!-- confirm in plan 11 -->
<key>CFBundleName</key>                  <string>MOM Recorder</string>
<key>CFBundleDisplayName</key>           <string>MOM Recorder</string>
<key>CFBundleExecutable</key>            <string>momr</string>
<key>CFBundleIconFile</key>              <string>MOMRecorder</string>
<key>CFBundleShortVersionString</key>    <string>@VERSION@</string>
<key>CFBundleVersion</key>               <string>@VERSION@</string>
<key>CFBundlePackageType</key>           <string>APPL</string>
<key>LSMinimumSystemVersion</key>        <string>14.0</string>
<key>LSApplicationCategoryType</key>     <string>public.app-category.productivity</string>
<key>NSHighResolutionCapable</key>       <true/>
<key>NSMicrophoneUsageDescription</key>  <string>MOM Recorder records your side of the meeting from the microphone.</string>
<key>NSAudioCaptureUsageDescription</key><string>MOM Recorder records the other side of the meeting from what your Mac plays.</string>
<key>CFBundleDocumentTypes</key>
<array><dict>
  <key>CFBundleTypeName</key>        <string>Meeting recording</string>
  <key>CFBundleTypeRole</key>        <string>Editor</string>
  <key>LSHandlerRank</key>           <string>Owner</string>
  <key>LSItemContentTypes</key>      <array><string>io.github.riobahtiar.momr.meeting</string></array>
  <key>CFBundleTypeIconFile</key>    <string>MOMRecorder</string>
</dict></array>
<key>UTExportedTypeDeclarations</key>
<array><dict>
  <key>UTTypeIdentifier</key>        <string>io.github.riobahtiar.momr.meeting</string>
  <key>UTTypeDescription</key>       <string>Meeting recording</string>
  <key>UTTypeConformsTo</key>        <array><string>public.json</string><string>public.data</string></array>
  <key>UTTypeTagSpecification</key>  <dict><key>public.filename-extension</key><array><string>meeting-recorder</string></array></dict>
</dict></array>
```

Layout produced by the script:

```
MOM Recorder.app/Contents/
├── Info.plist
├── PkgInfo                          "APPL????"
├── MacOS/
│   ├── momr                         the Rust binary
│   ├── momr-audio                   the helper
│   ├── momr-menubar                 plan 09
│   ├── ffmpeg, ffprobe              from Homebrew, install names rewritten
├── Frameworks/*.dylib               GTK 4, libadwaita, GLib, Pango, Cairo, HarfBuzz, GdkPixbuf, graphene, epoxy, …
│                                    plus ffmpeg's libav*, libopus, …
└── Resources/
    ├── MOMRecorder.icns
    ├── share/glib-2.0/schemas/gschemas.compiled
    ├── share/icons/Adwaita/          symbolic icons only (index.theme + scalable, symbolic)
    ├── share/icons/hicolor/index.theme
    └── licenses/                     ffmpeg and library licences
```

### 3. Environment from inside the binary, not a launcher script

A shell launcher as `CFBundleExecutable` complicates signing and TCC. Instead, at the top of `main()` (next to plan 06's `extend_path`), detect a bundle and set GTK's environment:

```rust
// sketch
fn bundle_environment() {
    let Ok(exe) = std::env::current_exe() else { return };
    let Some(contents) = exe.parent().and_then(|p| p.parent()) else { return };
    if contents.file_name() != Some("Contents".as_ref()) { return }
    let res = contents.join("Resources");
    // SAFETY: first thing in main, single-threaded.
    unsafe {
        std::env::set_var("XDG_DATA_DIRS", res.join("share"));
        std::env::set_var("GSETTINGS_SCHEMA_DIR", res.join("share/glib-2.0/schemas"));
        std::env::set_var("GDK_PIXBUF_MODULE_FILE", res.join("lib/gdk-pixbuf-2.0/2.10.0/loaders.cache"));
    }
}
```

The app draws with Cairo and uses named icons only, so GdkPixbuf loaders may be unnecessary; verify with a bundle that has none. If GTK complains about a missing PNG loader, bundle `libpixbufloader-png.dylib` and a `loaders.cache` with relative paths.

### 4. `scripts/bundle-macos.sh`

```bash
#!/bin/sh
# sketch: builds, assembles and rewrites a self-contained MOM Recorder.app in target/bundle
set -eu
cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
APP="target/bundle/MOM Recorder.app"; C="$APP/Contents"
scripts/build-macos.sh --features metal
rm -rf "$APP"; mkdir -p "$C/MacOS" "$C/Frameworks" "$C/Resources/share"
sed "s/@VERSION@/$VERSION/g" packaging/macos/Info.plist > "$C/Info.plist"; printf 'APPL????' > "$C/PkgInfo"
cp target/release/momr target/release/momr-audio "$C/MacOS/"
cp "$(brew --prefix ffmpeg)/bin/ffmpeg" "$(brew --prefix ffmpeg)/bin/ffprobe" "$C/MacOS/"
cp data/icon/MOMRecorder.icns "$C/Resources/"
glib-compile-schemas --targetdir "$C/Resources/share/glib-2.0/schemas" "$(brew --prefix)/share/glib-2.0/schemas"
rsync -a --include '*/' --include '*symbolic*' --include 'index.theme' --exclude '*' "$(brew --prefix)/share/icons/Adwaita/" "$C/Resources/share/icons/Adwaita/"
cp "$(brew --prefix)/share/icons/hicolor/index.theme" "$C/Resources/share/icons/hicolor/"
# Copy every non-system dylib each Mach-O depends on into Frameworks and rewrite install names.
for bin in "$C/MacOS/"*; do dylibbundler -od -b -x "$bin" -d "$C/Frameworks/" -p @executable_path/../Frameworks/; done
```

`dylibbundler` is in Homebrew. Run the app from the bundle with `open "$APP"` on the build machine, then on a clean account (`sudo sysadminctl -addUser tester …` or a VM) where `brew` is absent. Everything that fails there is a missing resource; add it to the script.

### 5. Document open through `GApplication`

In `ui.rs` `run()`: create the `adw::Application` with `gio::ApplicationFlags::HANDLES_OPEN`, connect `open` to the same code path as the argv meeting, and keep `activate` for the plain launch. Test from Finder by double-clicking a `.meeting-recorder` file and by dragging a meeting folder onto the Dock icon. Verify the delegate forwarding noted in Background; if `open` never fires, implement the Apple Event handler in the helper package as a dylib the app loads, or in Rust through `objc2` on this one call.

Register the type after copying the bundle: `lsregister -f "$APP"` (`/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister`), or just launch the app once.

### 6. Signing and notarization

`packaging/macos/entitlements.plist`:

```xml
<key>com.apple.security.device.audio-input</key> <true/>     <!-- hardened runtime: microphone -->
<key>com.apple.security.cs.disable-library-validation</key> <true/>   <!-- Homebrew dylibs are not signed by us; alternatively sign each -->
```

Sign inside-out, then the app, then verify:

```bash
find "$C/Frameworks" -name '*.dylib' -exec codesign --force --timestamp --options runtime --sign "$ID" {} \;
for b in momr-audio momr-menubar ffmpeg ffprobe; do codesign --force --timestamp --options runtime --sign "$ID" "$C/MacOS/$b"; done
codesign --force --timestamp --options runtime --entitlements packaging/macos/entitlements.plist --sign "$ID" "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"; spctl --assess --type execute --verbose "$APP"
ditto -c -k --keepParent "$APP" MOMRecorder.zip
xcrun notarytool submit MOMRecorder.zip --keychain-profile notary --wait
xcrun stapler staple "$APP"
```

If Metal shader compilation or ONNX Runtime trips the hardened runtime, the crash log names the entitlement (`com.apple.security.cs.allow-jit` or `allow-unsigned-executable-memory`); add only what the log asks for.

### 7. DMG

`brew install create-dmg`; `create-dmg --volname "MOM Recorder" --app-drop-link 450 200 "MOMRecorder-$VERSION-arm64.dmg" target/bundle/`. Sign and notarize the DMG the same way. Optionally a Homebrew cask `momr` in the tap pointing at the DMG.

### 8. Release workflow

`.github/workflows/release.yml`, on tag `v*`: matrix `macos-14` (arm64) and `macos-13` (x86_64); `brew install gtk4 libadwaita adwaita-icon-theme ffmpeg cmake pkgconf dylibbundler create-dmg`; run `scripts/bundle-macos.sh`; import the Developer ID certificate from secrets into a temporary keychain; sign, notarize, staple, DMG; upload both DMGs to the GitHub release. Store `APPLE_ID`, `APPLE_TEAM_ID`, an app-specific password and the `.p12` as secrets. Keep signing steps `if: secrets present` so forks without a certificate still get unsigned DMGs.

### 9. README

Install section: DMG first (download, drag to Applications, first-launch prompts), Homebrew second, build from source third. Troubleshooting: "no computer audio" (System Audio Recording permission, muted output, BlackHole), "app is damaged" (unsigned build, right-click Open).

## Verify

1. Clean account or VM without Homebrew: mount DMG, drag to Applications, launch, record 20 seconds with a video playing, stop, play back, quit. All prompts name MOM Recorder.
2. Double-click the `.meeting-recorder` file in the new meeting folder: the app opens on the done page. The file's icon in Finder is the app's.
3. `spctl --assess` passes; Gatekeeper shows no warning on first launch of the notarized DMG.
4. `brew install` from the tap on a machine with Homebrew: `momr --help` and `momr-audio list` work.
5. Both CI DMGs download and launch on the matching architecture.

## Risks and notes

- Homebrew dylibs are built for the Homebrew prefix and the current macOS; a bundle built on macOS 27 may not launch on 14. Set `MACOSX_DEPLOYMENT_TARGET=14.0` for the Rust and Swift builds and test the DMG on the oldest supported version in a VM. If Homebrew's libraries require a newer macOS than 14, the floor moves and the README says so.
- ffmpeg from Homebrew is a GPL build. Shipping it as a separate executable next to an MIT app is permitted; include ffmpeg's licence text in `Resources/licenses/`.
- Notarization can take minutes and occasionally fails on a stale timestamp server; retry once before investigating.

## Status

- [ ] Step 1 formula
- [ ] Step 2 `Info.plist` and layout
- [ ] Step 3 bundle environment in `main()`
- [ ] Step 4 bundle script runs on a clean account
- [ ] Step 5 `HANDLES_OPEN`
- [ ] Step 6 signing and notarization
- [ ] Step 7 DMG
- [ ] Step 8 release workflow
- [ ] Step 9 README install section
