# 11 Identity

## Goal

MOM Recorder's identity is final before the first public DMG: the bundle identifier and UTI are confirmed under a namespace the maintainer controls, folder and socket names are settled, and the last references to the app's Omarchy origin are gone from code, comments, screenshots and the demo kit, leaving only the credit.

## Done when

- [ ] `CFBundleIdentifier`, `APP_ID` and the UTI are confirmed (D13) and, if changed, changed everywhere in one commit.
- [ ] `grep -rn -i "omarchy\|omacon\|hyprland\|quattro\|pacman\|parec\|pacat" --exclude-dir=.git --exclude-dir=target .` returns only the credit lines in `README.md`, `plans/00-overview.md` and `LICENSE`, plus this plan.
- [ ] `screenshots/` holds macOS screenshots of MOM Recorder; the upstream Linux images are gone.
- [ ] `demo/` either documents how to shoot MOM Recorder on macOS or is reduced to the invented meeting scripts it exists to provide.
- [ ] The `Cargo.toml` package version is bumped (the first MOMR release) and the README status table says "shipped" for phases 1 to 3.

## Prerequisites

Plan 08 (there is a bundle to confirm the identity of). Do this before the first public DMG, not after: the bundle identifier is also the TCC identity, and changing it later resets every user's permissions.

## Background

Done already: the crate is `momr`; `APP_NAME` is `momr`; `APP_ID` is `io.github.riobahtiar.MOMRecorder`; the pacman packaging, installer, bar plugin, desktop entry and MIME type are deleted; every plan and the root docs use the new names. Plans 02 to 07 remove the Omarchy code paths as they land.

What still carries the origin after those plans:

| Where | What | Why it is still there |
|---|---|---|
| `screenshots/*.webp` | Upstream's Linux screenshots in Omarchy themes | Nothing to replace them with until plan 07 gives the app its macOS look |
| `demo/` | Upstream's guide for shooting screenshots in an Omarchy VM, plus the invented meeting scripts and piper voices | The scripts are the fixture source; the VM guide is Linux-only |
| `agent.rs` module doc | Credits the runner it was ported from | Attribution to the original author's work |
| `README.md`, `LICENSE` | Credit and MIT notice for the upstream author | Required by the licence and right to keep |

## Open decisions

Settle these in `01-decisions.md` before step 1.

- **Bundle identifier.** `io.github.riobahtiar.MOMRecorder` is under the maintainer's GitHub namespace and works everywhere except the Mac App Store, which wants a Team-registered id. If a domain is available, `<tld>.<domain>.MOMRecorder` is conventional. Whatever it is, it is fixed from the first public DMG on.
- **UTI.** Follows the bundle id prefix: `<prefix>.momr.meeting`.
- **Application Support folder.** `momr` (matches the binary and the socket) or `MOM Recorder` (matches how Apple's own apps name theirs). The plans assume `momr`; changing it is one constant.

## Steps

### 1. Confirm and, if needed, change the identifier

If the decision changes the id: `main.rs` `APP_ID`, `examples/transcribe_animation.rs`, `packaging/macos/Info.plist` (bundle id and UTI, both keys), plan 08's formula `homepage` if the repository moves, and `01-decisions.md` D13. One commit, message "Identity: bundle id …".

### 2. Screenshots

After plan 07: shoot ready, recording, done, import and Preferences in light and dark on macOS with the invented meetings from `demo/script.txt` and `demo/import-script.txt` (voiced with `say`, not piper, to keep the toolchain macOS-native). Replace `screenshots/*.webp`; add one hero image to the README. Delete the upstream images.

### 3. Demo kit

Reduce `demo/` to what MOM Recorder needs: the two scripts, a `say`-based `render.sh` that produces the demo tracks and the import file, and a short `README.md` with the fixture rule (invented meetings only) and the shoot steps on macOS. Delete the Omarchy VM guide, the Python renderers that depended on it, and the theme grid.

### 4. Comment sweep

`grep -rn -i omarchy src/` after plan 07 should be empty. Any survivor is a comment; rewrite it to say what the code does now. Keep the attribution in `agent.rs`'s module doc as "ported from the original author's text-transform runner" with the link.

### 5. Version and README

Bump `Cargo.toml` to the first MOMR release (proposal: `2.0.0`, since the identity and platform changed). Update the README's status table and add the install section from plan 08 step 9.

## Verify

1. The grep in Done-when returns only the allowed lines.
2. Build the DMG (plan 08) with the final id; install on a clean account; the permission prompts show "MOM Recorder".
3. `momr --help` prints the name and the version.

## Risks and notes

- Changing the bundle id after users have granted permissions forces them to grant again. Hence "before the first public DMG".
- Deleting `demo/` renderers loses the recipe that produced the exact upstream screenshots. They remain in git history (`git show 1a352b3:demo/README.md`), which is enough.

## Status

- [x] Open decisions settled
- [x] Step 1 identifier confirmed
- [ ] Step 2 macOS screenshots
- [x] Step 3 demo kit reduced
- [x] Step 4 comment sweep
- [ ] Step 5 version and README

Decided 2026-09-25 with the maintainer: bundle id and UTI stay as built,
Application Support stays `momr` — no renames. `demo/` is `README.md`,
`script.txt`, `import-script.txt` and say-based `render.sh` (verified:
274 s aligned two-track plus 92 s import). Screenshots and the version bump
wait for a display session and the release tag.
