// Renders the link preview card site/og.png (1200×630) from the home page itself:
// the first screen's wallpaper and menu bar panel, the app icon and one line.
//   node scripts/og-image.mjs        (needs Playwright with Chromium)
// Run it again after the hero panel or the headline changes.
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createRequire } from 'node:module';

const { chromium } = createRequire(import.meta.url)('playwright');
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const site = path.join(root, 'site');
const icon = pathToFileURL(path.join(root, 'docs/images/icon.png')).href;

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1200, height: 630 }, deviceScaleFactor: 1, reducedMotion: 'reduce', colorScheme: 'dark' });
await page.goto(pathToFileURL(path.join(site, 'index.html')).href, { waitUntil: 'networkidle' });
await page.evaluate((icon) => {
  document.querySelector('main').remove();
  document.querySelector('footer').remove();
  document.querySelector('.tseg')?.remove();
  document.documentElement.dataset.app = 'dark';
  const hero = document.querySelector('.hero');
  hero.querySelectorAll('.cta, .alt, .facts').forEach((e) => e.remove());
  hero.insertAdjacentHTML('afterbegin', `<div class="og-id"><img src="${icon}" alt=""><span>Cmd+Meow</span></div>`);
  const css = document.createElement('style');
  css.textContent = `
    html, body { width: 1200px; height: 630px; overflow: hidden; }
    .desk { height: 630px; } .mbar { display: none; }
    /* Soft blur under the text: darkest at the left edge, fades out before the panel, no hard edges */
    .desk::after { inset: 0 auto 0 0; width: 860px; z-index: 1; background: linear-gradient(90deg, rgba(4,9,4,.72) 0%, rgba(4,9,4,.55) 50%, rgba(4,9,4,0) 100%); backdrop-filter: blur(18px); -webkit-backdrop-filter: blur(18px); -webkit-mask-image: linear-gradient(90deg, #000 50%, transparent 100%); mask-image: linear-gradient(90deg, #000 50%, transparent 100%); }
    .stage { height: 630px; max-width: none; }
    .hero { left: 64px; top: 50%; max-width: 540px; text-shadow: 0 2px 24px rgba(0,0,0,.45); }
    .hero h1 { font-size: 62px; line-height: 66px; }
    .hero .sub { margin-top: 22px; font-size: 26px; line-height: 34px; }
    .og-id { display: flex; align-items: center; gap: 14px; margin-bottom: 30px; font: 500 30px/1 var(--sans); letter-spacing: -.01em; text-shadow: 0 2px 18px rgba(0,0,0,.5); }
    .og-id img { width: 64px; height: 64px; filter: drop-shadow(0 6px 16px rgba(0,0,0,.45)); }
    .drop { top: 50%; right: 56px; transform: translateY(-50%) scale(.92); transform-origin: right center; }
    .reveal { animation: none !important; }`;
  document.head.append(css);
}, icon);
await page.evaluate(() => document.fonts.ready);
await page.waitForTimeout(300);
await page.screenshot({ path: path.join(site, 'og.png') });
await browser.close();
console.log('wrote site/og.png');
