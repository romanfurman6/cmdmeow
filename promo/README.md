# Cmd+Meow promo (Remotion)

```bash
npm install
npm run studio        # timeline + live scrubbing, hot reload
npm run render:16x9   # out/cmdmeow-promo-16x9.mp4  (~3 min, 4 threads); site/promo.mp4 = this at 1280 wide
npm run render:9x16   # out/cmdmeow-promo-9x16.mp4
```

- `src/timing.ts` – music grid (115 BPM, bar = 2.087 s) and scene boundaries.
- `src/scenes/*.draw.ts` – scene drawing for time `t` (opener ⌘ slam, main click-through).
- `src/scenes/*.css.ts` – scene markup and styles.
- `src/Promo16x9.tsx`, `src/Promo9x16.tsx` – the two formats (9:16 = title on top, zoomed Mac screen card).
- `public/music.mp3` (-14 LUFS), `public/desk.jpg`, `src/fonts.css` (Inter, embedded).
- Set `REMOTION_CHROME=/path/to/chrome-headless-shell` to use a local browser; otherwise Remotion downloads its own.

## Credits

- Music: `public/music.mp3`, an AI-generated track made by the Cmd+Meow author for this video, cut and mixed to -14 LUFS.
- Font: Inter by The Inter Project Authors, SIL Open Font License 1.1, see `FONTS-LICENSE.txt`.
- Background: `public/desk-miami.jpg` (Dark Miami, the video's theme), the same image as `site/desk-miami.jpg`. `public/desk.jpg` is the Dark Classic one.
