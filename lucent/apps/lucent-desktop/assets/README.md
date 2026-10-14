# Typography

`GoogleSansFlex.ttf` and its OFL license come from the pinned Lucid reference.
`LucentSans.ttf` and `LucentDisplay.ttf` are renamed static instances produced
with fontTools 4.66.1 (`fontTools.varLib.instancer.instantiateVariableFont`):

| Face | wght | ROND | opsz |
| --- | --- | --- | --- |
| LucentSans | 500 | 0 | 18 |
| LucentDisplay | 620 | 100 | 110 |

The clock's rounded 620-weight face follows Lucid's stacked-clock typography.
The original OFL license covers the derived fonts. `LICENSE-Lucid.txt` preserves
attribution to the visual and interaction reference. No QML runs in Lucent.

The rich-text faces use the official [Google Fonts source](https://github.com/google/fonts/tree/bd8f81ddb5c74d5c8897b36ad88b440266245103/ofl/googlesansflex),
commit `bd8f81ddb5c74d5c8897b36ad88b440266245103`. The full variable font
`GoogleSansFlex[ROND,opsz,slnt,wdth,wght].ttf` has SHA-256
`a9ac770798857d5e8ac593ced42b22e40a7c08cec98319f92b0eccc2783a77ba`.
Its OFL text is identical to the bundled `OFL-GoogleSansFlex.txt`.

| Face | wght | slnt | wdth | ROND | opsz |
| --- | --- | --- | --- | --- | --- |
| LucentBold | 700 | 0 | 100 | 0 | 18 |
| LucentItalic | 500 | -10 | 100 | 0 | 18 |
| LucentBoldItalic | 700 | -10 | 100 | 0 | 18 |

Run `python scripts/generate-rich-fonts.py /path/to/pinned-source.ttf` with
fontTools 4.66.1 to reproduce these renamed static derivatives. Existing sans
and display faces retain their previous source and metrics.
