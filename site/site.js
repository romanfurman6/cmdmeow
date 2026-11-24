// Cmd+Meow home page: the live desktop in the first screen and the download
// button on phones.
(() => {
  const $ = id => document.getElementById(id);
  const still = matchMedia('(prefers-reduced-motion: reduce)').matches;
  // Live numbers tick only while their block is on screen
  const seen = el => {
    const st = { on: false };
    if (el) new IntersectionObserver(es => { st.on = es[0].isIntersecting; }).observe(el);
    return () => st.on && !document.hidden;
  };

  // Clock in the menu bar
  const clock = $('clock');
  const tick = () => { if (clock) clock.textContent = new Date().toLocaleString('en-US', { weekday: 'short', hour: '2-digit', minute: '2-digit', hour12: false }).replace(',', ''); };
  tick(); setInterval(tick, 15000);


  // App pictures follow the visitor's theme, like the app does
  const lightQ = matchMedia('(prefers-color-scheme: light)');
  const theme = () => { if (!document.documentElement.dataset.pin) document.documentElement.dataset.app = lightQ.matches ? 'light' : 'dark'; };
  theme(); lightQ.addEventListener('change', theme);

  // Panel header: the round "…" slides out Open, Hide and Quit, and tucks them back
  document.querySelectorAll('.swh .more').forEach(b => b.addEventListener('click', e => {
    e.stopPropagation(); const hb = b.parentElement; hb.classList.toggle('open'); b.title = hb.classList.contains('open') ? 'Less' : 'More';
  }));

  // Phones can't install a Mac app: send the link to the Mac instead
  if (matchMedia('(hover: none) and (pointer: coarse)').matches) document.querySelectorAll('.get').forEach(get => {
    const url = 'https://cmdmeow.com/';
    get.innerHTML = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 14.5V3.5M7.5 8L12 3.5 16.5 8M5.5 12v8.5h13V12"/></svg><span class="gl-tx">Send link to my Mac</span><span class="gl-ok">Link copied</span>';
    get.href = url;
    get.addEventListener('click', async e => {
      e.preventDefault();
      if (navigator.share) { try { await navigator.share({ title: 'Cmd+Meow', text: 'Cmd+Meow: free Mac app', url }); return; } catch (err) { if (err && err.name === 'AbortError') return; } }
      try { await navigator.clipboard.writeText(url); } catch { return; }
      get.classList.add('done'); setTimeout(() => get.classList.remove('done'), 1800);
    });
  });

  // Windows rise in one after another
  if (!still) ['.hero', '.drop'].forEach((s, i) => {
    const el = document.querySelector(s); if (el) { el.classList.add('reveal'); el.style.animationDelay = (i * 0.12) + 's'; }
  });

  if (still) return;

  // First screen: CPU in the menu bar and the Keep Awake timer, while it's on screen
  const top = seen(document.querySelector('.desk'));
  const mbCpu = $('mb-cpu');
  let load = 16;
  setInterval(() => {
    if (!top() || !mbCpu) return;
    load = Math.max(6, Math.min(78, load + (Math.random() - 0.48) * 14));
    mbCpu.textContent = Math.round(load) + '%';
  }, 800);
  const awT = $('aw-t'), ring = $('aw-ring');
  let secs = 1 * 3600 + 24 * 60 + 10;
  setInterval(() => {
    secs++; // the timer keeps real time, only the drawing waits
    if (!awT || !top()) return;
    const h = Math.floor(secs / 3600), m = Math.floor(secs / 60) % 60, s = secs % 60;
    awT.textContent = `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
    if (ring) ring.setAttribute('stroke-dashoffset', (119.4 * (1 - (secs % 7200) / 7200)).toFixed(1));
  }, 1000);
})();
