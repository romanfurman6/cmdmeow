// Promo v2, vertical (1080x1920) for TikTok / Reels: same beats, music and scenes as PromoV2,
// laid out for a phone. The 16:9 stages are shown large (hook panned, features as a screen card).
import React, {useCallback, useRef} from 'react';
import {AbsoluteFill, Audio, Img, staticFile, useCurrentFrame, useVideoConfig} from 'remotion';
import {Scene} from './Scene';
import {opener} from './scenes';
import {
  BEAT, Bt, V2_END, V2_INTRO, clamp, P, lerp, eo, eo5, eio, spring, rise, PINK, SANS, SKY, img, NC, CUTS, mainV2,
  flick, bentEar, BODY_PTS, CMD,
} from './PromoV2';

const W = 1080, H = 1920;
const INK2 = 'rgba(255,255,255,.7)';
const desk = staticFile('desk-miami.jpg');
const Bg: React.FC<{blur?: number; dim?: number}> = ({blur = 0, dim = 1}) => (
  <Img src={desk} style={{position: 'absolute', inset: 0, width: W, height: H, objectFit: 'cover', objectPosition: '62% 50%', filter: `blur(${blur}px) brightness(${dim})`, transform: blur ? 'scale(1.06)' : undefined}} />
);

// ---- hook: the 16:9 opener filling the height, panned from the left windows to the cat ----
const Hook: React.FC<{tv: number}> = ({tv}) => {
  const SPLIT = Bt(9.6);
  const ot = tv < SPLIT ? (tv / SPLIT) * 3.0 : 3.0 + (tv - SPLIT);
  const s = H / 1080;
  const pan = lerp(80, 1150 * s - W / 2, eio(P(tv, 0.4, SPLIT)));
  const q = eo5(P(tv, Bt(4.5), Bt(5.3))), qo = eo(P(tv, SPLIT - 0.25, SPLIT + 0.1));
  return (
    <AbsoluteFill data-var="a" style={{background: '#0A0826', overflow: 'hidden'}}>
      <div style={{position: 'absolute', left: 0, top: 0, width: 1920, height: 1080, transformOrigin: '0 0', transform: `translateX(${-pan}px) scale(${s})`}}>
        <Scene key="opener" mod={opener} t={Math.min(ot, 4.25)} />
      </div>
      <div style={{position: 'absolute', left: 60, right: 60, bottom: 260, textAlign: 'center', zIndex: 20, font: `600 128px/1.02 ${SANS}`, letterSpacing: '-.045em', color: '#fff', textShadow: '0 6px 40px rgba(10,8,38,.85), 0 0 2px rgba(10,8,38,.6)', opacity: q * (1 - qo), transform: `translateY(${lerp(40, 0, q) - 30 * qo}px)`, filter: `blur(${lerp(8, 0, q) + 8 * qo}px)`}}>
        Paying for<br /><span style={{color: PINK}}>all of these?</span>
      </div>
    </AbsoluteFill>
  );
};

// ---- hero: headline on top, the menu bar panel below ----
const Hero: React.FC<{t: number}> = ({t}) => {
  const s = Bt(6);
  const pn = eo5(P(t, s + 0.5 * BEAT, s + 0.5 * BEAT + 0.8));
  const free = t - Bt(8);
  const fp = spring(free, 8, 18);
  return (
    <AbsoluteFill style={{overflow: 'hidden'}}>
      <Bg blur={lerp(0, 18, eo(P(t, s, s + BEAT)))} dim={lerp(0.85, 0.62, eo(P(t, s, s + BEAT)))} />
      <div style={{position: 'absolute', left: 80, right: 60, top: 220, font: `500 150px/150px ${SANS}`, letterSpacing: '-.045em', color: '#fff', textShadow: '0 4px 40px rgba(10,8,38,.45)'}}>
        <div style={rise(t, s)}>Everything</div>
        <div style={rise(t, s + 0.5 * BEAT)}>your Mac</div>
        <div style={{display: 'flex', gap: 34}}>
          <span style={rise(t, Bt(7))}>needs.</span>
          <span style={{color: PINK, display: 'inline-block', opacity: free > 0 ? 1 : 0, transform: `scale(${lerp(1.6, 1, clamp(fp, 0, 2))})`, transformOrigin: 'left 70%', textShadow: '0 0 40px rgba(255,95,162,.45)'}}>Free.</span>
        </div>
      </div>
      <Img src={img('panel-miami.png')} style={{position: 'absolute', left: (W - 620) / 2, top: 790, width: 620, borderRadius: 26, opacity: pn, transform: `translateY(${lerp(60, 0, pn)}px) scale(${lerp(0.96, 1, pn)})`, boxShadow: '0 40px 90px rgba(5,3,20,.6)'}} />
    </AbsoluteFill>
  );
};

// ---- features: big title on top, the Mac screen as a card; Spaces swipe between them ----
const CROP = {x: 770, w: 1080}; // the part of the 1920 stage with the app window
const Montage: React.FC<{t: number}> = ({t}) => {
  const STEP = 2 * BEAT, SW = 0.42, GAP = 60;
  const i = Math.min(Math.floor((t - Bt(10)) / STEP), CUTS.length - 1);
  const t0 = Bt(10) + i * STEP, t1 = t0 + STEP;
  const at = (k: number) => CUTS[k][0] + (t - (Bt(10) + k * STEP)) * CUTS[k][1];
  const sw = i + 1 < CUTS.length ? eio(P(t, t1 - SW, t1)) : 0;
  const x = -sw * (W + GAP);
  const mb = Math.sin(Math.PI * sw);
  const s = (W - 40) / CROP.w; // card 1040 wide
  const ch = 1080 * s;
  const ttl = useRef<(HTMLDivElement | null)[]>([]);
  const copy = useCallback((k: number) => (root: HTMLElement) => {
    const L = root.querySelector('#lab') as HTMLElement | null, T = ttl.current[k];
    if (!L || !T) return;
    L.style.visibility = 'hidden';
    const tt = root.querySelector('#ttl') as HTMLElement;
    T.textContent = tt.textContent;
    T.style.opacity = tt.style.opacity;
    T.style.transform = tt.style.transform;
    T.style.filter = tt.style.filter;
  }, []);
  const page = (k: number, dx: number) => (
    <AbsoluteFill key={'cut' + k} style={{transform: `translateX(${dx}px) scale(${1 - 0.04 * mb})`, filter: mb > 0.01 ? `blur(${mb * 3}px)` : undefined, overflow: 'hidden'}}>
      <Bg blur={16} dim={0.6} />
      <div style={{position: 'absolute', left: 0, right: 0, top: 300, height: 260, display: 'flex', alignItems: 'flex-end', justifyContent: 'center'}}>
        <div ref={el => { ttl.current[k] = el; }} style={{font: `600 150px/1 ${SANS}`, letterSpacing: '-.045em', color: '#fff', whiteSpace: 'nowrap', textShadow: '0 6px 40px rgba(0,0,0,.5)'}} />
      </div>
      <div style={{position: 'absolute', left: 20, top: 700, width: W - 40, height: ch, borderRadius: 28, overflow: 'hidden', boxShadow: '0 40px 120px rgba(0,0,0,.6)'}}>
        <div style={{position: 'absolute', left: 0, top: 0, width: 1920, height: 1080, transformOrigin: '0 0', transform: `translateX(${-CROP.x * s}px) scale(${s})`}}>
          <Scene mod={mainV2} t={at(k)} onDrawn={copy(k)} />
        </div>
      </div>
    </AbsoluteFill>
  );
  return (
    <AbsoluteFill style={{background: '#07061A', overflow: 'hidden'}}>
      {page(i, x)}
      {sw > 0 && page(i + 1, x + W + GAP)}
    </AbsoluteFill>
  );
};

// ---- No catch: one row per point ----
const NoCatch: React.FC<{t: number}> = ({t}) => (
  <AbsoluteFill style={{background: SKY, fontFamily: SANS, color: '#fff'}}>
    <div style={{position: 'absolute', left: 90, top: 300, font: `500 130px/1 ${SANS}`, letterSpacing: '-.04em', ...rise(t, Bt(26))}}>No catch</div>
    <div style={{position: 'absolute', left: 90, right: 70, top: 560, display: 'flex', flexDirection: 'column', gap: 64}}>
      {NC.map(([k, b, sub, path], i) => {
        const at = Bt(26.5 + i * 0.75);
        const p = spring(t - at, 9, 15);
        return (
          <div key={i} style={{display: 'flex', alignItems: 'center', gap: 44, opacity: clamp((t - at) / 0.12)}}>
            <div style={{width: 150, height: 150, flex: 'none', display: 'flex', alignItems: 'center', justifyContent: 'center', transform: `scale(${clamp(p, 0, 1.2)})`}}>
              {k ? <span style={{font: `500 120px/1 ${SANS}`, letterSpacing: '-.04em', color: PINK}}>{k}</span>
                : <svg width="130" height="130" viewBox="0 0 48 48" fill="none" stroke={PINK} strokeWidth={2.4} strokeLinecap="round" strokeLinejoin="round" dangerouslySetInnerHTML={{__html: path}} />}
            </div>
            <div style={{display: 'flex', flexDirection: 'column', gap: 10}}>
              <b style={{font: `500 64px/1.1 ${SANS}`, letterSpacing: '-.03em', ...rise(t, at + 0.08, 20, 0.5)}}>{b}</b>
              <span style={{font: `400 40px/1.25 ${SANS}`, color: INK2, ...rise(t, at + 0.16, 20, 0.5)}}>{sub}</span>
            </div>
          </div>
        );
      })}
    </div>
  </AbsoluteFill>
);

// ---- > coded by AI / Designed by Humans. (two marker lines, written one after the other) ----
const Human: React.FC<{t: number}> = ({t}) => {
  const line = '> coded by AI';
  const typed = line.slice(0, Math.floor(clamp((t - Bt(32.2)) / (1.4 * BEAT)) * line.length));
  const w0 = Bt(34.2);
  const p1 = eio(P(t, w0, w0 + 0.6)), p2 = eio(P(t, w0 + 0.6, w0 + 1.2));
  const ul = eo(P(t, w0 + 1.15, w0 + 1.55));
  const fade = 1 - eo(P(t, w0 - 0.25, w0)) * 0.55;
  const mk = (txt: string, p: number) => (
    <b style={{display: 'block', font: `400 160px/1.08 'Permanent Marker', cursive`, color: PINK, clipPath: `inset(-20% ${lerp(102, -2, p)}% -20% -2%)`, textShadow: '0 0 60px rgba(255,95,162,.4)'}}>{txt}</b>
  );
  return (
    <AbsoluteFill style={{background: SKY, alignItems: 'center', justifyContent: 'center', flexDirection: 'column', gap: 50}}>
      <code style={{font: `400 64px/1.3 'Geist Mono', monospace`, color: INK2, opacity: fade}}>{typed}<span style={{display: 'inline-block', width: 30, height: 62, marginLeft: 12, verticalAlign: -10, background: INK2, opacity: t > w0 ? 0 : Math.floor(t * 2) % 2 ? 0.15 : 1}} /></code>
      <div style={{position: 'relative', transform: 'rotate(-3deg)', textAlign: 'center'}}>
        {mk('Designed by', p1)}
        {mk('Humans.', p2)}
        <svg viewBox="0 0 1000 40" preserveAspectRatio="none" style={{position: 'absolute', left: '12%', bottom: -24, width: '76%', height: 44, overflow: 'visible'}}>
          <path d="M5 28 C 250 10, 600 6, 995 18" fill="none" stroke={PINK} strokeWidth={11} strokeLinecap="round" pathLength={1} strokeDasharray="1" strokeDashoffset={1 - ul} opacity={ul > 0 ? 1 : 0} />
        </svg>
      </div>
    </AbsoluteFill>
  );
};

// ---- end card ----
const EndCard: React.FC<{t: number}> = ({t}) => {
  const s = V2_END, dt = t - s;
  const pop = spring(dt, 7, 11);
  const ears = flick(dt - 0.45);
  const a = eo5(P(t, s + 0.35, s + 0.75)), c = eo5(P(t, s + 0.55, s + 0.95)), d = eo5(P(t, s + 0.75, s + 1.15));
  return (
    <AbsoluteFill style={{overflow: 'hidden', fontFamily: SANS, color: '#fff'}}>
      <Bg blur={16} dim={0.62} />
      <svg viewBox="0 0 1024 1024" style={{position: 'absolute', left: W / 2 - 200, top: 520, width: 400, height: 400, overflow: 'visible', transform: `scale(${clamp(pop, 0, 1.3)})`, transformOrigin: '50% 60%', filter: 'drop-shadow(0 30px 60px rgba(5,3,20,.55)) drop-shadow(0 0 50px rgba(255,95,162,.35))'}}>
        <g fill="#F2ECE3" stroke="#F2ECE3" strokeWidth={2} strokeLinejoin="round">
          <polygon points={bentEar(0, ears)} /><polygon points={bentEar(1, ears)} /><polygon points={BODY_PTS} />
        </g>
        <path d={CMD} fill="none" stroke={PINK} strokeWidth={59} strokeLinecap="round" />
      </svg>
      <div style={{position: 'absolute', left: 0, right: 0, top: 1000, textAlign: 'center', font: `600 110px/1 ${SANS}`, letterSpacing: '-.035em', opacity: a, transform: `translateY(${lerp(24, 0, a)}px)`, filter: `blur(${lerp(6, 0, a)}px)`}}>Cmd+Meow</div>
      <div style={{position: 'absolute', left: 0, right: 0, top: 1140, textAlign: 'center', font: `400 50px/1 ${SANS}`, color: 'rgba(255,255,255,.75)', opacity: c, transform: `translateY(${lerp(20, 0, c)}px)`}}>cmdmeow.com</div>
      <div style={{position: 'absolute', left: 0, right: 0, top: 1250, display: 'flex', justifyContent: 'center', opacity: d, transform: `translateY(${lerp(20, 0, d)}px)`}}>
        <span style={{background: PINK, color: '#1A0B1E', font: `600 46px/1 ${SANS}`, padding: '30px 54px', borderRadius: 24}}>Free download for Mac</span>
      </div>
    </AbsoluteFill>
  );
};

const Body: React.FC = () => {
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();
  const tv = frame / fps;
  if (tv < Bt(12)) return <Hook tv={tv} />;
  const t = tv - V2_INTRO;
  if (t < Bt(10)) return <Hero t={t} />;
  if (t < Bt(26)) return <Montage t={t} />;
  if (t < Bt(32)) return <NoCatch t={t} />;
  if (t < V2_END) return <Human t={t} />;
  return <EndCard t={t} />;
};

export const PromoV2V: React.FC = () => (
  <AbsoluteFill>
    <Body />
    <Audio src={img('music.mp3')} />
  </AbsoluteFill>
);
