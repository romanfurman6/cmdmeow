// Cmd+Meow home page: the menu bar icons that jump into one app, and the demo of
// the onboarding goals (each goal's tools play through once, then stop).
(() => {
  const $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const sleep = ms => new Promise(r => setTimeout(r, ms));
  const still = matchMedia('(prefers-reduced-motion: reduce)').matches;

  // ---------- Demo: fill the repeated bits ----------
  const root = document.getElementById('demo');
  if (root) {
    const G = [['App Caches', 7.4], ['Installers', 4.6], ['Trash', 3.5], ['Logs', 1.9], ['AI Tools', 0.9]];
    const TICK = '<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12l4 4 9-9"/></svg>';
    $$('[data-rows]', root).forEach(g => g.innerHTML = G.map(([n, s]) => `<div class="gr"><span class="ck">${TICK}</span><span>${n}</span><span class="bar"><i style="--w:${Math.round(s / 7.4 * 100)}%"></i></span><span class="num">${s.toFixed(1)} GB</span></div>`).join(''));
    $$('.fl2 .ck', root).forEach(c => c.innerHTML = TICK);
    $$('[data-cpu]', root).forEach(c => { const h = [22, 30, 26, 40, 35, 28, 24, 52, 61, 44, 30, 26, 22, 34, 48, 72, 58, 40, 32, 28, 24, 30, 26, 23]; c.innerHTML = h.map((v, k) => `<i style="--h:${v}%;--k:${k}"${v > 60 ? ' class="hot"' : ''}></i>`).join(''); });

    // Dictation: words are added one by one, so the caret always sits right after the last word
    $$('.words', root).forEach(w => {
      const said = 'Move the launch to Tuesday, December 8. Ask Dana for the final video and send the press kit on Friday.'.split(' ');
      const scene = w.closest('.scene'); let t = 0;
      const run = () => {
        clearTimeout(t); w.textContent = '';
        if (!scene.classList.contains('on')) return;
        if (still) { w.textContent = said.join(' '); return; }
        let k = 0;
        const next = () => { w.textContent += (k ? ' ' : '') + said[k++]; if (k < said.length) t = setTimeout(next, 170); };
        t = setTimeout(next, 600);
      };
      new MutationObserver(run).observe(scene, { attributes: true, attributeFilter: ['class'] });
    });

    // Tabs are the goals. A goal's tools play through once, then stop.
    // Pointing at the card or clicking a tool stops it.
    const tabs = $$('.gtabs .ttab', root), texts = $$('.gws .scene', root), cards = $$('.cards .scene', root);
    let goal = 0, step = 0, timer = 0, held = false, touched = false;
    const swap = (list, on) => list.forEach(s => { const was = s.classList.contains('on'); s.classList.toggle('on', s === on); s.classList.toggle('out', was && s !== on); });
    const steps = () => $$('.st2', texts[goal]);
    const mine = () => cards.filter(c => +c.dataset.g === goal);
    const show = i => {
      step = i; swap(cards, mine()[i]);
      steps().forEach((b, j) => { b.classList.toggle('on', j === i); b.setAttribute('aria-pressed', j === i); b.classList.remove('tick'); });
    };
    const play = () => {
      clearTimeout(timer);
      const st = steps();
      if (held || still || step >= st.length - 1) return;
      const b = st[step]; void b.offsetWidth; b.classList.add('tick');
      timer = setTimeout(() => { if (document.hidden) { held = true; return; } show(step + 1); play(); }, 3400);
    };
    const pick = g => {
      goal = g;
      tabs.forEach((t, j) => { t.classList.toggle('on', j === g); t.setAttribute('aria-pressed', j === g); });
      swap(texts, texts[g]);
    };
    const open = g => { held = false; pick(g); show(0); play(); };
    const hold = () => { held = true; clearTimeout(timer); steps().forEach(b => b.classList.remove('tick')); };
    tabs.forEach((t, g) => { t.type = 'button'; t.addEventListener('click', () => { touched = true; open(g); }); });
    texts.forEach(tx => $$('.st2', tx).forEach((b, i) => { b.type = 'button'; b.addEventListener('click', () => { touched = true; hold(); show(i); }); }));
    const stage = root.querySelector('.cards');
    stage.addEventListener('pointerenter', e => { if (e.pointerType === 'mouse') hold(); });
    pick(0); show(0);
    // The first pass starts when the section scrolls in, not before
    new IntersectionObserver((e, o) => { if (e[0].isIntersecting) { o.disconnect(); if (!held && !touched) open(0); } }, { threshold: .35 }).observe(stage);

    // Other parts of the page can open a tool here
    const AT = { cleanup: [0, 0], diskmap: [0, 1], uninstall: [0, 2], capture: [1, 0], video: [1, 1], sharing: [1, 2], clipboard: [2, 0], dictation: [3, 0], health: [4, 0], fans: [4, 0], startup: [4, 1], displays: [4, 2], claude: [5, 0], awake: [5, 1] };
    window.cmShow = key => { const at = AT[key]; if (!at) return; touched = true; hold(); pick(at[0]); show(at[1]); root.scrollIntoView({ behavior: still ? 'auto' : 'smooth', block: 'center' }); };
  }

  // Hero panel: a widget opens its tool in the demo
  $$('.hero-pn .sw[data-to]').forEach(w => {
    const to = w.dataset.to; if (!window.cmShow) return;
    w.tabIndex = 0; w.setAttribute('role', 'button');
    w.addEventListener('click', e => { if (!e.target.closest('.seg span, .lk, .pz')) window.cmShow(to); });
    w.addEventListener('keydown', e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); window.cmShow(to); } });
  });

  // One bar: the menu bar icons jump into Cmd+Meow, each becomes a live widget.
  // Plays once when it comes into view and stays on the open panel; the round button plays it again.
  (() => {
    const bars = document.getElementById('bars'); if (!bars) return;
    const again = document.getElementById('bars-again'), ap = $$('.mi2.ap', bars), wg = $$('.wg', bars);
    ap.forEach((ic, k) => ic.style.setProperty('--k', k));
    const wOf = i => wg.find(w => +w.dataset.i === i);
    const end = () => { bars.className = 'bars b-one b-open b-done'; ap.forEach(ic => ic.classList.add('out')); wg.forEach(w => w.classList.add('on')); };
    const reset = () => { bars.className = 'bars'; ap.forEach(ic => ic.classList.remove('out')); wg.forEach(w => w.classList.remove('on')); };
    const clearFlying = () => $$('.mi2.fly').forEach(f => f.parentNode && f.parentNode.removeChild(f));
    if (still) { end(); return; }
    const ORDER = [7, 0, 12, 5, 11, 2, 13, 8, 1, 10, 4, 9, 3, 6];
    let gen = 0, state = 'idle';
    const fly = (ic, w, i) => {
      const a = ic.getBoundingClientRect(), b = w.querySelector('.vis, .rg, .scw, svg.g').getBoundingClientRect();
      const dx = b.left + b.width / 2 - (a.left + a.width / 2), dy = b.top + b.height / 2 - (a.top + a.height / 2);
      const f = ic.cloneNode(true); f.classList.remove('ap'); f.classList.add('fly'); f.style.left = a.left + 'px'; f.style.top = a.top + 'px'; f.style.width = a.width + 'px';
      document.body.append(f); ic.classList.add('out');
      const side = (i % 2 ? 1 : -1) * (40 + (i * 37) % 70), lift = -18 - (i * 13) % 22, tilt = (i % 2 ? 1 : -1) * (14 + (i * 7) % 16);
      const an = f.animate([
        { transform: 'translate(0,0) scale(1) rotate(0deg)', easing: 'cubic-bezier(.2,.7,.4,1)' },
        { transform: `translate(${dx * 0.35 + side}px, ${lift}px) scale(1.7) rotate(${tilt}deg)`, offset: 0.32, easing: 'cubic-bezier(.5,0,.3,1)' },
        { transform: `translate(${dx}px, ${dy}px) scale(1.05) rotate(0deg)` }
      ], { duration: 820 + (i * 53) % 220, fill: 'forwards' });
      return new Promise(r => { an.onfinish = () => { if (f.parentNode) f.parentNode.removeChild(f); w.classList.add('on'); r(); }; });
    };
    // Starts on the menu bar full of icons (they wiggle at once), ends on the panel and stays there
    const play = async id => {
      again.hidden = true; reset(); bars.classList.add('b-wig'); await sleep(1100); if (id !== gen) return;
      bars.classList.add('b-one'); await sleep(420); bars.classList.add('b-open'); await sleep(380); if (id !== gen) return;
      bars.classList.remove('b-wig');
      const all = []; bars.classList.add('b-eat');
      for (const [k, i] of ORDER.entries()) {
        if (id !== gen) return;
        all.push(fly(ap[i], wOf(i), k));
        await sleep(60 + (k * 29) % 60);
      }
      await Promise.all(all); bars.classList.remove('b-eat'); if (id !== gen) return;
      bars.classList.add('b-done'); again.hidden = false; state = 'done';
    };
    const run = () => { state = 'run'; play(++gen); };
    // Scrolled away mid-way: jump to the end, so it never waits half-done
    const finish = () => { gen++; clearFlying(); end(); again.hidden = false; state = 'done'; };
    new IntersectionObserver(es => {
      if (es[0].isIntersecting && state === 'idle') run();
      else if (!es[0].isIntersecting && state === 'run') finish();
    }, { threshold: 0.45 }).observe(bars);
    again.addEventListener('click', run);
  })();
})();
