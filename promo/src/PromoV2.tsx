// Promo v2: shorter cut in the landing page's style (Geist, landing animations,
// theme switcher, "No catch", "Designed by Humans."), Dark Miami.
// Timing is a beat grid (BPM below); retime to the final track by changing BPM / OFFSET.
import React from 'react';
import {AbsoluteFill, Audio, Img, staticFile, useCurrentFrame, useVideoConfig} from 'remotion';
import {Scene} from './Scene';
import {main, opener} from './scenes';
import icons from './scenes/landing-icons.json';
import './fonts-landing.css';

export const V2_BPM = 111.98; // track: drop at song 16.155 s = Bt(6)
export const V2_OFFSET = 0; // seconds before beat 0 (track intro)
const BEAT = 60 / V2_BPM;
const Bt = (n: number) => V2_OFFSET + n * BEAT;
export const V2_END = Bt(38); // final hit of the track
export const V2_DURATION = V2_END + 3.6;

// ---- helpers ----
const clamp = (x: number, a = 0, b = 1) => Math.min(b, Math.max(a, x));
const P = (t: number, a: number, b: number) => clamp((t - a) / (b - a));
const lerp = (a: number, b: number, p: number) => a + (b - a) * p;
const eo = (p: number) => 1 - Math.pow(1 - p, 3);
const eo5 = (p: number) => 1 - Math.pow(1 - p, 5);
const eio = (p: number) => (p < 0.5 ? 4 * p * p * p : 1 - Math.pow(-2 * p + 2, 3) / 2);
const spring = (dt: number, k = 7, w = 13) => (dt <= 0 ? 0 : 1 - Math.exp(-k * dt) * Math.cos(w * dt));
// landing "rise": .8s cubic-bezier(.22,1,.36,1) from translateY(16px), here scaled for video
const rise = (t: number, at: number, dy = 40, dur = 0.7) => {
  const p = eo5(P(t, at, at + dur));
  return {opacity: p, transform: `translateY(${lerp(dy, 0, p)}px)`, filter: `blur(${lerp(8, 0, p)}px)`};
};

const PINK = '#FF5FA2', ACC = '#FF9DC6', INK2 = 'rgba(255,255,255,.66)';
const SANS = 'Geist, Inter, sans-serif';
const SKY = 'linear-gradient(to bottom,#0B1240 0%,#2A1452 45%,#5E1D57 85%,#70283C 100%)';
const img = (f: string) => staticFile('v2/' + f);

const Cursor: React.FC<{x: number; y: number; press?: boolean; o?: number}> = ({x, y, press, o = 1}) => (
  <svg viewBox="0 0 34 44" style={{position: 'absolute', left: 0, top: 0, width: 38, height: 48, opacity: o, transform: `translate(${x}px,${y}px) scale(${press ? 0.84 : 1})`, filter: 'drop-shadow(0 3px 6px rgba(0,0,0,.5))', zIndex: 40}}>
    <path d="M3 2v34l9-8 6 13 6-3-6-12h12z" fill="#000" stroke="#fff" strokeWidth={2.5} strokeLinejoin="round" />
  </svg>
);

// ---- A: crowded menu bar; the cat eats every icon (landing 14 -> 1) ----
const MB_H = 92;
const MenuBar: React.FC<{t: number; s0: number}> = ({t, s0}) => {
  const n = icons.icons.length;
  const eatAt = (i: number) => s0 + 2.2 * BEAT + (n - 1 - i) * 0.075; // nearest to the cat first
  const scale = lerp(1, 58 / MB_H, eio(P(t, Bt(6), Bt(7))));
  const catX = 1580, gap = 62, x0 = catX - (n + 0.2) * gap;
  return (
    <div style={{position: 'absolute', left: 0, top: 0, width: 1920 / scale, height: MB_H, transform: `scale(${scale})`, transformOrigin: 'top left', background: 'rgba(8,6,28,.5)', backdropFilter: 'blur(24px)', display: 'flex', alignItems: 'center', font: `600 32px ${SANS}`, color: '#fff', zIndex: 20}}>
      <span style={{position: 'absolute', left: 40, display: 'flex', gap: 44, alignItems: 'center'}}>
        <svg width="30" height="36" viewBox="0 0 17 20"><path fill="#fff" d="M14.1 10.6c0-2.3 1.9-3.4 2-3.5-1.1-1.6-2.8-1.8-3.4-1.8-1.4-.1-2.8.9-3.5.9-.7 0-1.8-.9-3-.8C4.6 5.4 3.2 6.3 2.4 7.7.7 10.6 2 15 3.6 17.4c.8 1.2 1.8 2.5 3 2.4 1.2 0 1.7-.8 3.1-.8 1.5 0 1.9.8 3.1.8 1.3 0 2.1-1.2 2.9-2.4.9-1.3 1.3-2.6 1.3-2.7-.1 0-2.6-1-2.6-4.1zM11.8 3.8c.6-.8 1.1-1.9 1-3-.9 0-2.1.6-2.7 1.4-.6.7-1.1 1.8-1 2.8 1 .1 2.1-.5 2.7-1.2z" /></svg>
        <b>Finder</b><span style={{fontWeight: 500}}>File</span><span style={{fontWeight: 500}}>Edit</span><span style={{fontWeight: 500}}>View</span>
      </span>
      {icons.icons.map((svg, i) => {
        const k = t - (s0 + i * 0.05);
        const pop = spring(k, 9, 16);
        const wig = t > s0 + 1.2 * BEAT && t < eatAt(i) ? Math.sin((t - i * 0.053) * 2 * Math.PI / 0.64) * 6 : 0;
        const e = eio(P(t, eatAt(i), eatAt(i) + 0.22));
        const x = lerp(x0 + i * gap, catX, e);
        return (
          <span key={i} style={{position: 'absolute', left: x - 24, top: MB_H / 2 - 24, width: 48, height: 48, borderRadius: 11, display: 'grid', placeItems: 'center', background: 'rgba(255,95,162,.16)', color: ACC, opacity: 1 - P(t, eatAt(i) + 0.14, eatAt(i) + 0.22), transform: `scale(${pop * lerp(1, 0.3, e)}) rotate(${wig}deg)`}}
            dangerouslySetInnerHTML={{__html: svg.replace('width="16" height="16"', 'width="30" height="30" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"')}} />
        );
      })}
      {(() => {
        const gulps = icons.icons.map((_, i) => eatAt(i) + 0.2).filter(g => t > g).map(g => Math.exp(-(t - g) * 14) * Math.sin((t - g) * 40));
        const sq = gulps.reduce((a, b) => a + b, 0) * 0.08;
        const glow = t > eatAt(n - 1) && t < eatAt(0) + 0.4 ? 0.5 + 0.5 * Math.sin(t * 12) : 0;
        return (
          <span style={{position: 'absolute', left: catX - 26, top: MB_H / 2 - 26, width: 52, height: 52, borderRadius: 12, display: 'grid', placeItems: 'center', background: t > eatAt(0) + 0.3 ? 'rgba(255,255,255,.18)' : 'transparent', boxShadow: `0 0 0 ${5 * glow}px rgba(255,95,162,.18), 0 0 ${18 * glow}px rgba(255,95,162,.45)`, transform: `scale(${1 + sq},${1 - sq})`}}>
            <svg width="36" height="36" viewBox="0 0 100 100" style={{color: '#fff'}} dangerouslySetInnerHTML={{__html: icons.markLine}} />
          </span>
        );
      })()}
      <span style={{position: 'absolute', right: 40, display: 'flex', gap: 34, alignItems: 'center', fontWeight: 500}}>
        <svg width="34" height="30" viewBox="0 -2.5 24 20.5"><path d="M12 15.5l2.6-3.1a4 4 0 00-5.2 0zM6.3 9.4a8.4 8.4 0 0111.4 0l1.7-2a11 11 0 00-14.8 0zM2.4 4.9a14 14 0 0119.2 0l1.7-2a16.5 16.5 0 00-22.6 0z" fill="#fff" /></svg>
        <span>Sat 19:42</span>
      </span>
    </div>
  );
};

// ---- B: landing hero, video-sized ----
// ---- hook: sound-off viewers decide in ~2 s, so the pain is on screen at once ----
const Hook: React.FC<{t: number}> = ({t}) => {
  const out = eo(P(t, Bt(5.3), Bt(6)));
  return (
    <div style={{position: 'absolute', left: 150, top: 380, font: `500 112px/1.08 ${SANS}`, letterSpacing: '-.04em', color: '#fff', textShadow: '0 4px 40px rgba(10,8,38,.55)', opacity: 1 - out, transform: `translateY(${-30 * out}px)`, filter: `blur(${8 * out}px)`}}>
      <div style={rise(t, 0.15)}>Too many apps</div>
      <div style={rise(t, 0.15 + BEAT)}>in your <span style={{color: PINK}}>menu bar?</span></div>
    </div>
  );
};

const Hero: React.FC<{t: number}> = ({t}) => {
  const s = Bt(6);
  const pn = eo5(P(t, s + 0.5 * BEAT, s + 0.5 * BEAT + 0.8));
  const free = t - Bt(8);
  const fp = spring(free, 8, 18);
  return (
    <>
      <div style={{position: 'absolute', left: 150, top: 300, font: `500 132px/138px ${SANS}`, letterSpacing: '-.04em', color: '#fff', textShadow: '0 4px 40px rgba(10,8,38,.45)'}}>
        <div style={rise(t, s)}>Everything your</div>
        <div style={{display: 'flex', gap: 34}}>
          <span style={rise(t, Bt(7))}>Mac needs.</span>
          <span style={{color: PINK, display: 'inline-block', opacity: free > 0 ? 1 : 0, transform: `scale(${lerp(1.6, 1, clamp(fp, 0, 2))})`, transformOrigin: 'left 70%', textShadow: '0 0 40px rgba(255,95,162,.45)'}}>Free.</span>
        </div>
        <div style={{...rise(t, Bt(8.5), 24), font: `400 38px/52px ${SANS}`, letterSpacing: '-.01em', marginTop: 30, color: 'rgba(255,255,255,.9)'}}>Clean up, screenshots, clipboard, dictation<br />and more, in one menu bar app.</div>
      </div>
      <Img src={img('panel-miami.png')} style={{position: 'absolute', left: 1330, top: 84, width: 513, borderRadius: 22, opacity: pn, transform: `translateY(${lerp(-40, 0, pn)}px) scale(${lerp(0.96, 1, pn)})`, transformOrigin: 'top right', boxShadow: '0 40px 90px rgba(5,3,20,.6)'}} />
    </>
  );
};

// ---- D: the real landing hero switching themes from the dots pill (TH12 ripple through blur) ----
// ---- E: No catch (landing .trust) ----
const NC = [
  ['$0', 'Free forever', 'No account, no subscription', ''],
  ['', 'Undo anytime', 'Review first, then Trash', '<path d="M8 13h32M18 13V8h12v5M12 13l2 27h20l2-27"/><path d="M28.75 32h-6a3.5 3.5 0 0 1 0-7h5M25.75 22l3 3-3 3"/>'],
  ['', 'Only what you use', 'Asks per tool you turn on', '<rect x="9" y="21" width="30" height="21" rx="5"/><path d="M15 21v-6a9 9 0 0 1 18 0v6M24 29v5"/>'],
  ['', 'Open source', 'Code on GitHub', '<circle cx="14" cy="10" r="4.5"/><circle cx="14" cy="38" r="4.5"/><circle cx="34" cy="10" r="4.5"/><path d="M14 14.5v19M34 14.5v4a6 6 0 0 1-6 6h-8a6 6 0 0 0-6 6"/>'],
];
const NoCatch: React.FC<{t: number}> = ({t}) => (
  <AbsoluteFill style={{background: SKY, fontFamily: SANS, color: '#fff'}}>
    <div style={{position: 'absolute', left: 150, top: 250, font: `500 96px/1 ${SANS}`, letterSpacing: '-.035em', ...rise(t, Bt(26))}}>No catch</div>
    <div style={{position: 'absolute', left: 150, right: 150, top: 470, display: 'grid', gridTemplateColumns: 'repeat(4,1fr)'}}>
      {NC.map(([k, b, s, path], i) => {
        const at = Bt(26.5 + i * 0.75);
        const p = spring(t - at, 9, 15);
        return (
          <div key={i} style={{padding: '0 36px', borderLeft: i ? '1px solid rgba(255,255,255,.12)' : 'none', display: 'flex', flexDirection: 'column', gap: 14, opacity: clamp((t - at) / 0.12)}}>
            <div style={{height: 130, display: 'flex', alignItems: 'center', transform: `scale(${clamp(p, 0, 1.2)})`, transformOrigin: 'left center'}}>
              {k ? <span style={{font: `500 120px/1 ${SANS}`, letterSpacing: '-.04em', color: PINK}}>{k}</span>
                : <svg width="120" height="120" viewBox="0 0 48 48" fill="none" stroke={PINK} strokeWidth={2.4} strokeLinecap="round" strokeLinejoin="round" dangerouslySetInnerHTML={{__html: path}} />}
            </div>
            <b style={{font: `500 40px/1.2 ${SANS}`, letterSpacing: '-.02em', ...rise(t, at + 0.08, 20, 0.5)}}>{b}</b>
            <span style={{font: `400 26px/1.3 ${SANS}`, color: INK2, ...rise(t, at + 0.16, 20, 0.5)}}>{s}</span>
          </div>
        );
      })}
    </div>
  </AbsoluteFill>
);

// ---- F: > coded by AI / Designed by Humans. ----
const Human: React.FC<{t: number}> = ({t}) => {
  const line = '> coded by AI';
  const typed = line.slice(0, Math.floor(clamp((t - Bt(32.2)) / (1.4 * BEAT)) * line.length));
  // handwriting: the marker line is revealed left to right, like a pen stroke
  const w0 = Bt(34.2), wp = eio(P(t, w0, w0 + 1.15));
  const ul = eo(P(t, w0 + 1.05, w0 + 1.45)); // underline swoosh after the text
  const glow = 0.25 + 0.25 * eo(P(t, w0 + 1.1, w0 + 1.6));
  const fade = 1 - eo(P(t, w0 - 0.25, w0)) * 0.55; // "coded by AI" steps back as the pen starts
  return (
    <AbsoluteFill style={{background: SKY, alignItems: 'center', justifyContent: 'center', flexDirection: 'column', gap: 34}}>
      <code style={{font: `400 52px/1.3 'Geist Mono', monospace`, color: INK2, opacity: fade, transform: `translateY(${lerp(0, -10, 1 - fade)}px)`}}>{typed}<span style={{display: 'inline-block', width: 24, height: 50, marginLeft: 10, verticalAlign: -8, background: INK2, opacity: t > w0 ? 0 : Math.floor(t * 2) % 2 ? 0.15 : 1}} /></code>
      <div style={{position: 'relative', transform: 'rotate(-2deg)'}}>
        <b style={{display: 'block', font: `400 168px/1.15 'Permanent Marker', cursive`, color: PINK, clipPath: `inset(-20% ${lerp(102, -2, wp)}% -20% -2%)`, textShadow: `0 0 60px rgba(255,95,162,${glow})`}}>Designed by Humans.</b>
        <svg viewBox="0 0 1000 40" preserveAspectRatio="none" style={{position: 'absolute', left: '4%', right: '6%', bottom: -18, width: '90%', height: 40, overflow: 'visible'}}>
          <path d="M5 28 C 250 10, 600 6, 995 18" fill="none" stroke={PINK} strokeWidth={9} strokeLinecap="round" pathLength={1} strokeDasharray="1" strokeDashoffset={1 - ul} opacity={ul > 0 ? 1 : 0} />
        </svg>
      </div>
    </AbsoluteFill>
  );
};


// ---- end card: the current mark (src/kit/brand.rs BODY + EARS, 1024 box) ----
// Ears follow rule M6: an ear never turns as one piece; its base stays on the head
// and the turn grows smoothly to the tip; a flick = tilt out and back, max 0.28 rad.
type Seg = ['M' | 'L', number, number] | ['C', number, number, number, number, number, number];
const BODY: Seg[] = [['M', 330, 200], ['L', 692, 200], ['L', 940, 300], ['C', 955, 350, 958, 400, 958, 450], ['L', 958, 840], ['C', 958, 930, 895, 992, 805, 992], ['L', 218, 992], ['C', 128, 992, 65, 930, 65, 840], ['L', 65, 450], ['C', 65, 400, 68, 350, 84, 300]];
const EARS: Seg[][] = [
  [['M', 84, 300], ['C', 84, 200, 118, 44, 161, 44], ['C', 193, 44, 263, 110, 330, 200], ['L', 330, 250], ['L', 110, 330]],
  [['M', 692, 200], ['C', 760, 110, 830, 44, 862, 44], ['C', 905, 44, 940, 200, 940, 300], ['L', 915, 330], ['L', 692, 250]],
];
const flatten = (segs: Seg[], n = 32) => {
  const out: [number, number][] = [];
  for (const sg of segs) {
    if (sg[0] !== 'C') { out.push([sg[1], sg[2]]); continue; }
    const [, x1, y1, x2, y2, x, y] = sg; const [x0, y0] = out[out.length - 1];
    for (let k = 1; k <= n; k++) { const t = k / n, u = 1 - t; const a = u * u * u, b = 3 * u * u * t, c = 3 * u * t * t, d = t * t * t; out.push([a * x0 + b * x1 + c * x2 + d * x, a * y0 + b * y1 + c * y2 + d * y]); }
  }
  return out;
};
const BEND: [number, number][] = [[210, 200], [814, 200]];
const bentEar = (right: number, turn: number) => flatten(EARS[right]).map(([x, y]) => {
  let w = clamp((200 - y) / (200 - 44)); w = w * w * (3 - 2 * w);
  const a = (right ? turn : -turn) * w; const [px, py] = BEND[right]; const c = Math.cos(a), sn = Math.sin(a);
  return `${(px + (x - px) * c - (y - py) * sn).toFixed(1)},${(py + (x - px) * sn + (y - py) * c).toFixed(1)}`;
}).join(' ');
const BODY_PTS = flatten(BODY).map(p => p.join(',')).join(' ');
const CMD = 'M347.5 433.5m0 0' + [[347.5, 433.5, 0, -1.5], [670.5, 433.5, 1, 2.5], [347.5, 757.5, -0.5, -2], [670.5, 757.5, 1.5, 3]].map(([cx, cy, a0, a1]) => {
  const r = 87.5, A0 = a0 * Math.PI, A1 = a1 * Math.PI; const sx = cx + r * Math.cos(A0), sy = cy + r * Math.sin(A0), ex = cx + r * Math.cos(A1), ey = cy + r * Math.sin(A1);
  return `M${sx} ${sy}A${r} ${r} 0 1 ${A1 > A0 ? 1 : 0} ${ex} ${ey}`;
}).join('') + 'M435 433.5V757.5M583 433.5V757.5M347.5 521H670.5M347.5 670H670.5';
// two-tilt flick: out and back, twice, eased
const flick = (dt: number, dur = 0.9) => (dt <= 0 || dt >= dur ? 0 : 0.28 * Math.sin((dt / dur) * 2 * Math.PI) * Math.sin(Math.PI * dt / dur));

const EndCard: React.FC<{t: number}> = ({t}) => {
  const s = V2_END, dt = t - s;
  const pop = spring(dt, 7, 11);
  const ears = flick(dt - 0.45);
  const a = eo5(P(t, s + 0.35, s + 0.75)), c = eo5(P(t, s + 0.55, s + 0.95)), d = eo5(P(t, s + 0.75, s + 1.15));
  return (
    <AbsoluteFill style={{overflow: 'hidden', fontFamily: SANS, color: '#fff'}}>
      <Img src={staticFile('desk-miami.jpg')} style={{position: 'absolute', inset: 0, width: 1920, height: 1080, objectFit: 'cover', filter: 'blur(16px) brightness(.62)', transform: 'scale(1.06)'}} />
      <svg viewBox="0 0 1024 1024" style={{position: 'absolute', left: 960 - 150, top: 250, width: 300, height: 300, overflow: 'visible', transform: `scale(${clamp(pop, 0, 1.3)})`, transformOrigin: '50% 60%', filter: 'drop-shadow(0 30px 60px rgba(5,3,20,.55)) drop-shadow(0 0 50px rgba(255,95,162,.35))'}}>
        <g fill="#F2ECE3" stroke="#F2ECE3" strokeWidth={2} strokeLinejoin="round">
          <polygon points={bentEar(0, ears)} /><polygon points={bentEar(1, ears)} /><polygon points={BODY_PTS} />
        </g>
        <path d={CMD} fill="none" stroke={PINK} strokeWidth={59} strokeLinecap="round" />
      </svg>
      <div style={{position: 'absolute', left: 0, right: 0, top: 600, textAlign: 'center', font: `600 76px/1 ${SANS}`, letterSpacing: '-.03em', opacity: a, transform: `translateY(${lerp(24, 0, a)}px)`, filter: `blur(${lerp(6, 0, a)}px)`}}>Cmd+Meow</div>
      <div style={{position: 'absolute', left: 0, right: 0, top: 700, textAlign: 'center', font: `400 34px/1 ${SANS}`, color: 'rgba(255,255,255,.75)', opacity: c, transform: `translateY(${lerp(20, 0, c)}px)`}}>cmdmeow.com</div>
      <div style={{position: 'absolute', left: 0, right: 0, top: 778, display: 'flex', justifyContent: 'center', opacity: d, transform: `translateY(${lerp(20, 0, d)}px)`}}>
        <span style={{background: PINK, color: '#1A0B1E', font: `600 30px/1 ${SANS}`, padding: '20px 36px', borderRadius: 16}}>Free download for Mac</span>
      </div>
    </AbsoluteFill>
  );
};

// ---- C: fast cuts through the app (the main scene, time-remapped) ----
const mainV2 = {...main, css: main.css + `#ttl{font:600 112px/1 ${SANS};letter-spacing:-.045em}#endwm{font-family:${SANS}!important;font-weight:600!important}`};
const CUTS: [number, number][] = [ // [main scene start, rate]
  [26.6, 1.8], // Clean up
  [9.45, 1.0], // Screenshot
  [11.0, 1.6], // Mark it up
  [14.9, 1.2], // Video
  [18.9, 1.2], // Clipboard
  [22.1, 1.5], // Dictation
  [30.0, 1.0], // Mac Health
  [33.0, 1.2], // AI Usage
];
const OPENER_RATE = 4.25 / (6 * 60 / 111.98); // opener's 0–4.25 s fits beats 0–6
const MAIN_END = 36.39; // main scene end card start (K(16))

const Body: React.FC = () => {
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();
  const t = frame / fps;
  if (t < Bt(6)) {
    // hook: the old video's opener (paid apps everywhere, price tags), sped up; the cat eats it all
    return (
      <AbsoluteFill data-var="a" style={{background: '#0A0826'}}>
        <Scene key="opener" mod={opener} t={t * OPENER_RATE} />
      </AbsoluteFill>
    );
  }
  if (t < Bt(10)) {
    return (
      <AbsoluteFill style={{background: '#0A0826', overflow: 'hidden'}}>
        <Img src={img('hero-miami-bare.jpg')} style={{position: 'absolute', inset: 0, width: 1920, height: 1080, filter: `blur(${lerp(0, 18, eo(P(t, Bt(6), Bt(7))))}px) brightness(${lerp(0.55, 1, eo(P(t, Bt(5.5), Bt(6.5)))) * lerp(1, 0.72, eo(P(t, Bt(6), Bt(7))))})`, transform: `scale(${lerp(1, 1.04, eo(P(t, Bt(6), Bt(7))))})`}} />
        {t >= Bt(6) && <AbsoluteFill style={{background: 'radial-gradient(70% 80% at 25% 50%, rgba(10,8,38,.55), rgba(10,8,38,0) 70%)', opacity: eo(P(t, Bt(6), Bt(7)))}} />}
        {t >= Bt(6) && <Hero t={t} />}
        <MenuBar t={t} s0={0.15} />
      </AbsoluteFill>
    );
  }
  if (t < Bt(26)) {
    // macOS Spaces swipe: the whole desktop slides left and the next feature's desktop
    // follows it in, like a three-finger swipe between Spaces. Both are on screen, side by side.
    const STEP = 2 * BEAT, SW = 0.42, GAP = 40;
    const i = Math.min(Math.floor((t - Bt(10)) / STEP), CUTS.length - 1);
    const t0 = Bt(10) + i * STEP, t1 = t0 + STEP;
    const at = (k: number) => CUTS[k][0] + (t - (Bt(10) + k * STEP)) * CUTS[k][1];
    const sw = i + 1 < CUTS.length ? eio(P(t, t1 - SW, t1)) : 0;
    const x = -sw * (1920 + GAP);
    const mb = Math.sin(Math.PI * sw); // motion: a touch of blur + depth mid-swipe
    const page = (k: number, dx: number) => (
      <AbsoluteFill key={'cut' + k} style={{transform: `translateX(${dx}px) scale(${1 - 0.04 * mb})`, filter: mb > 0.01 ? `blur(${mb * 3}px)` : undefined, borderRadius: 24 * mb, overflow: 'hidden'}}>
        <Scene mod={mainV2} t={at(k)} />
      </AbsoluteFill>
    );
    return (
      <AbsoluteFill style={{background: '#07061A', overflow: 'hidden'}}>
        {page(i, x)}
        {sw > 0 && page(i + 1, x + 1920 + GAP)}
      </AbsoluteFill>
    );
  }
  if (t < Bt(32)) return <NoCatch t={t} />;
  if (t < V2_END) return <Human t={t} />;
  return <EndCard t={t} />;
};

export const PromoV2: React.FC = () => (
  <AbsoluteFill>
    <Body />
    <Audio src={img('music.mp3')} />
  </AbsoluteFill>
);
