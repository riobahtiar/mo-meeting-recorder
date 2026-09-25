#!/bin/sh
# Renders the invented meetings into audio with `say` (macOS-native).
# Nothing is played; every fixture stays invented people saying invented things.
#
#   demo/render.sh <out-dir>
#
# Writes <out-dir>/maya.wav and <out-dir>/tom.wav (the two aligned tracks for
# demo/script.txt, 48 kHz stereo, same length: play them at the same moment
# for a natural conversation) and <out-dir>/import.wav (demo/import-script.txt
# as one mono room recording for the import demo).
set -eu
cd "$(dirname "$0")"
OUT=${1:?usage: demo/render.sh <out-dir>}
mkdir -p "$OUT" "$OUT/.turns"

turns() {
  # $1 script, $2 who: the turn texts, one per line, no blanks or comments.
  grep -v '^#' "$1" | grep "^$2|" | grep -v '^..$' | sed 's/^..//'
}

voice_turns() {
  # $1 script, $2 who, $3 say-voice: one s16le 48 kHz mono file per turn.
  i=0
  turns "$1" "$2" | while IFS= read -r text; do
    i=$((i + 1))
    say -v "$3" -o "$OUT/.turns/$2-$i.aiff" "$text"
    ffmpeg -v error -y -nostdin -i "$OUT/.turns/$2-$i.aiff" \
      -f s16le -ar 48000 -ac 1 "$OUT/.turns/$2-$i.s16"
  done
}

voice_turns script.txt M Samantha
voice_turns script.txt T Daniel
python3 - "$OUT" <<'EOF'
import array, random, sys
from pathlib import Path
out = Path(sys.argv[1])
RATE = 48000
random.seed(7)
tracks = {"M": array.array("h"), "T": array.array("h")}
cursor = int(RATE * 0.8)
previous = None
by_tag = {"M": [], "T": []}
script = [l for l in Path("script.txt").read_text().splitlines()
          if l and not l.startswith("#")]
for tag, path in [("M", p) for p in sorted((out / ".turns").glob("M-*.s16"),
                  key=lambda p: int(p.stem.split("-")[1]))] + \
                 [("T", p) for p in sorted((out / ".turns").glob("T-*.s16"),
                  key=lambda p: int(p.stem.split("-")[1]))]:
    by_tag[tag].append(array.array("h", path.read_bytes()))
counts = {"M": 0, "T": 0}
for line in script:
    who = line.split("|", 1)[0]
    speech = by_tag[who][counts[who]]
    counts[who] += 1
    gap = random.uniform(0.35, 0.6) if who == previous else random.uniform(0.45, 0.9)
    cursor += int(RATE * gap) if previous else 0
    for track in tracks.values():
        if len(track) < cursor + len(speech):
            track.extend([0] * (cursor + len(speech) - len(track)))
    tracks[who][cursor:cursor + len(speech)] = speech
    cursor += len(speech)
    previous = who
end = cursor + int(RATE * 1.5)
for who, name in (("M", "maya"), ("T", "tom")):
    track = tracks[who]
    track.extend([0] * (end - len(track)))
    stereo = array.array("h")
    for s in track:
        stereo.extend((s, s))
    Path(out / f".turns/{name}.s16").write_bytes(stereo.tobytes())
print(f"{sum(counts.values())} turns, {end / RATE:.1f} s")
EOF
for name in maya tom; do
  ffmpeg -v error -y -nostdin -f s16le -ar 48000 -ac 2 \
    -i "$OUT/.turns/$name.s16" "$OUT/$name.wav"
done

# Import demo: Anna, Rob and Lena in one mono room recording.
voice_turns import-script.txt A Karen
voice_turns import-script.txt R Alex
voice_turns import-script.txt L Anna
python3 - "$OUT" <<'EOF'
import array, sys
from pathlib import Path
out = Path(sys.argv[1])
mono = array.array("h")
script = [l for l in Path("import-script.txt").read_text().splitlines()
          if l and not l.startswith("#")]
index = {"A": 0, "R": 0, "L": 0}
prefix = {"A": "A", "R": "R", "L": "L"}
for line in script:
    who = line.split("|", 1)[0]
    index[who] += 1
    speech = array.array("h", (out / f".turns/{prefix[who]}-{index[who]}.s16").read_bytes())
    if mono:
        mono.extend([0] * 24000)
    mono.extend(speech)
(out / ".turns/import.s16").write_bytes(mono.tobytes())
print(f"import: {len(mono) / 48000:.1f} s")
EOF
ffmpeg -v error -y -nostdin -f s16le -ar 48000 -ac 1 \
  -i "$OUT/.turns/import.s16" "$OUT/import.wav"
rm -rf "$OUT/.turns"
ls -la "$OUT"
