# Website

Static site for https://cmdmeow.com, deployed by `.github/workflows/pages.yml`
on every push to `main` that touches `site/`.

- `index.html`, `style.css`: the landing page. `app.css` styles the app mock-ups, `widgets.css` draws the menu bar panel as the app's widgets (`docs/ui-rules.md` SP13; redo it when the widgets change), `desk.jpg` / `desk-1376.jpg` are the desktop wallpaper behind them.
- `view/`: share page for shots and recordings. `view/?u=<file url>` (optional `&t=<title>`, `&mode=edit`).
  View shows the picture or plays the video. Edit changes a copy in the browser: marks, text and crop on
  pictures (saved as PNG), split / remove / trim on videos (saved as MP4 through WebCodecs and
  [Mediabunny](https://mediabunny.dev), loaded from jsDelivr only when saving). Nothing is uploaded.
  Edit needs the storage to allow this site (CORS); the app adds that rule to the bucket on its first upload.
  Pure logic lives in `view/core.mjs` (tests: `node --test tests/site/core.test.mjs`);
  `node tests/site/smoke.mjs` runs the page in Chromium (needs Playwright and ffmpeg).
- `play/`: old player links (`play/?v=…`) forward to `view/`.
- `icon.png` is copied from `docs/images/icon.png` at deploy time (PNGs are gitignored).
- `og.png` is the 1200×630 link preview card, rendered from the home page by `node scripts/og-image.mjs` (needs Playwright). Run it again after the hero panel or the headline changes.
- Motion: the app window walks through its tools once and stops on Cleanup; the 14→1 scene plays once and has a replay button; live numbers pause off screen. The tool count (14) follows `src/tools/mod.rs` `all()`.

Preview locally: `cp docs/images/icon.png site/ && python3 -m http.server -d site 8080`.

Tool icons on the site are from [Phosphor Icons](https://phosphoricons.com) (regular weight, MIT licence, © Phosphor Icons). The app itself uses SF Symbols, which Apple doesn't license for the web.
