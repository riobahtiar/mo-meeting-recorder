# Demo kit

Invented meetings only, never a real one. `script.txt` (Maya on the
microphone, Tom on the computer audio) and `import-script.txt` (Anna, Rob and
Lena in one room recording) are the two fixtures; everything else derives
from them.

## Rendering the audio

`render.sh` voices the scripts with macOS `say` and lays the turns out with
`ffmpeg` and Python 3 (standard library only). Nothing is played.

```bash
demo/render.sh /tmp/demo-out   # maya.wav, tom.wav, import.wav
```

Voices: Samantha (Maya), Daniel (Tom), Karen (Anna), Alex (Rob), Anna
(Lena). Swap them at the top of `render.sh` if macOS renames a voice; keep
three distinct voices for the import file so diarization has something to
separate.

## Shooting screenshots

On a Mac with a display, with the app built from this checkout:

1. Play `maya.wav` and `tom.wav` from two players at once for the recording
   shots, or record for real with a colleague who agrees to appear.
2. Ready, recording (strip too), transcribing, done, import dialog and
   Settings, in light and dark: `screenshots/` holds only what the README
   shows, as WebP.
3. Look at every file before it goes anywhere: sharp, nothing private, no
   stray pointer in a still, nothing cut off.
