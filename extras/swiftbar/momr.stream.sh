#!/bin/bash
# <swiftbar.type>streaming</swiftbar.type>
# <swiftbar.hideAbout>true</swiftbar.hideAbout>
# <swiftbar.name>MOM Recorder</swiftbar.name>
# MOM Recorder status for SwiftBar: a pulsing dot, the clock and a tiny
# two-lane meter while recording, "paused" while paused, the percentage while
# transcribing, hidden otherwise. Link into SwiftBar's plugin folder; SwiftBar
# runs it and every ~~~ block replaces the item.
bars() {
  # $1 mic, $2 computer as 0..1: one block character per lane. A case table,
  # not awk substr: BSD awk counts bytes and would split the multibyte glyphs.
  glyph() {
    i=$(awk -v v="$1" 'BEGIN{i=int(v*8)+1; if (i>9) i=9; if (i<1) i=1; print i}');
    case $i in
      1) printf ' ' ;; 2) printf '▁' ;; 3) printf '▂' ;; 4) printf '▃' ;;
      5) printf '▄' ;; 6) printf '▅' ;; 7) printf '▆' ;; 8) printf '▇' ;;
      *) printf '█' ;;
    esac
  }
  printf '%s%s' "$(glyph "$1")" "$(glyph "$2")"
}
clock() { printf '%02d:%02d' "$(($1 / 60))" "$(($1 % 60))"; }
momr watch | while read -r line; do
  tick=$((tick + 1))
  # 20 lines a second is more than a menu bar needs; show every fourth.
  [ $((tick % 4)) -ne 0 ] && continue
  state=$(jq -r .state <<<"$line")
  elapsed=$(jq -r .elapsed <<<"$line")
  mic=$(jq -r .mic <<<"$line")
  computer=$(jq -r .computer <<<"$line")
  progress=$(jq -r .progress <<<"$line")
  case "$state" in
    off | idle | done)
      printf '~~~\n\n'
      ;;
    recording)
      printf '~~~\n● %s %s\n---\nPause | bash=momr param1=pause terminal=false refresh=true\nStop | bash=momr param1=stop terminal=false refresh=true\n' \
        "$(clock "$elapsed")" "$(bars "$mic" "$computer")"
      ;;
    paused)
      printf '~~~\n❚❚ paused %s\n---\nResume | bash=momr param1=pause terminal=false refresh=true\n' \
        "$(clock "$elapsed")"
      ;;
    transcribing)
      printf '~~~\n⟳ %d%%\n' "$(awk "BEGIN{print int($progress*100)}")"
      ;;
  esac
done
