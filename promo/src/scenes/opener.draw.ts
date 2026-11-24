/* eslint-disable */
// @ts-nocheck
// Generated from the original render(t) page, then kept as source: the scene's drawing code.
// Every element lookup is scoped to `root`; rects are measured in stage pixels (Studio zoom safe).
export function create(root: HTMLElement): {render:(t:number)=>void; init:()=>void} {
const __r=(el:any)=>{const r=el.getBoundingClientRect(),R=root.getBoundingClientRect(),s=R.width/root.offsetWidth||1;const left=(r.left-R.left)/s,top=(r.top-R.top)/s,width=r.width/s,height=r.height/s;return {left,top,width,height,right:left+width,bottom:top+height,x:left,y:top};};

const $=id=>root.querySelector("#"+id);
const clamp=(x,a=0,b=1)=>Math.min(b,Math.max(a,x)), lerp=(a,b,p)=>a+(b-a)*p, P=(t,a,b)=>clamp((t-a)/(b-a));
const eo=p=>1-Math.pow(1-p,3), eo5=p=>1-Math.pow(1-p,5), eio=p=>p<.5?4*p*p*p:1-Math.pow(-2*p+2,3)/2;
const spring=(dt,k=7,w=13)=>dt<=0?0:1-Math.exp(-k*dt)*Math.cos(w*dt);
const S='fill="none" stroke="#fff" stroke-width="1.900" stroke-linecap="round" stroke-linejoin="round"';
const ICONS=[
 `<path ${S} d="M3 8h10a3 3 0 1 0-3-3M3 12h15a3 3 0 1 1-3 3M3 16h7"/>`,
 `<path ${S} d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/>`,
 `<path ${S} d="M3 12h4l3-7 4 14 3-7h4"/>`,
 `<path ${S} d="M5 18a8 8 0 1 1 14 0M12 13l4-4"/>`,
 `<g ${S}><circle cx="12" cy="12" r="1.600"/><path d="M12 10.400C11 6 13 3 16 4c1 2.500-1 5.500-4 6.400M13.500 12.800c4 1.500 5.500 4.500 3.500 6.500-2.500.4-5-2-5-5.200M10.500 12.800C7 15.500 3.500 15 3.300 12.300c1.500-2 4.500-2.300 7.200-1.500"/></g>`,
 `<path ${S} d="M6 2v16h16M2 6h16v16"/>`,
 `<g ${S}><rect x="3" y="7" width="12" height="10" rx="2"/><path d="M15 10.500l6-3v9l-6-3"/></g>`,
 `<g ${S}><rect x="5" y="4" width="14" height="17" rx="2.500"/><rect x="9" y="2" width="6" height="4" rx="1.500"/></g>`,
 `<g ${S}><rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3"/></g>`,
 `<path ${S} d="M4 9h12v5a5 5 0 0 1-5 5H9a5 5 0 0 1-5-5zM16 10h2a2.500 2.500 0 0 1 0 5h-2"/>`,
 `<g ${S}><rect x="3" y="4" width="18" height="12" rx="2"/><path d="M9 20h6M12 16v4"/></g>`,
];
$('icons').innerHTML=ICONS.map((p,i)=>`<svg id="ic${i}" viewBox="0 0 24 24">${p}</svg>`).join('');
// eight mismatched "app" windows
const WINS=[
 {x:150,y:150,w:520,h:360,bg:'linear-gradient(160deg,#5b3df5,#c03cf0)',tb:'rgba(255,255,255,.15)',c:'#fff',t:'Disk Sweeper Pro',body:`<div style="text-align:center;padding-top:56px"><div style="font:800 64px Inter">47 GB</div><div style="opacity:.8">of junk found</div><div style="margin:26px auto 0;width:220px;height:56px;border-radius:28px;background:#fff;color:#5b3df5;font:700 22px/56px Inter">Upgrade to clean</div></div>`},
 {x:760,y:120,w:470,h:300,bg:'#f3f3f3',tb:'#e2e2e2',c:'#222',t:'SnapIt',body:`<div style="display:flex;gap:10px;padding:16px">${['Area','Window','Full','Timer','Scroll'].map(n=>`<span style="padding:10px 14px;border-radius:8px;background:#fff;border:1px solid #ddd">${n}</span>`).join('')}</div><div style="margin:8px 16px;height:150px;border-radius:10px;background:repeating-linear-gradient(45deg,#e8e8e8 0 12px,#f6f6f6 12px 24px)"></div>`},
 {x:1330,y:110,w:430,h:430,bg:'#1d2733',tb:'#151d26',c:'#dfe9f5',t:'ClipStack',body:`<div style="padding:10px 16px">${['meeting notes.txt','https://…','#ff6600','Password ••••','Invoice 0423'].map(n=>`<div style="padding:13px 10px;border-bottom:1px solid rgba(255,255,255,.08)">${n}</div>`).join('')}</div>`},
 {x:420,y:560,w:420,h:330,bg:'#0b0b0b',tb:'#1a1a1a',c:'#38ff7a',t:'FanBoost',body:`<div style="text-align:center;padding-top:40px;font-family:monospace"><div style="font:700 72px monospace">4 820</div><div>RPM · CPU 92°C</div><div style="margin:20px auto;width:300px;height:12px;background:#143;border:1px solid #38ff7a"><div style="width:84%;height:100%;background:#38ff7a"></div></div></div>`},
 {x:920,y:470,w:470,h:340,bg:'#fff8e7',tb:'#f2e6c6',c:'#5a3d00',t:'BatteryGuru',body:`<div style="padding:24px"><div style="font:800 54px Inter">81%</div><div>Health · 412 cycles</div><svg width="420" height="110" style="margin-top:12px"><polyline fill="none" stroke="#e8a000" stroke-width="4" points="0,90 50,70 100,80 150,40 200,55 250,30 300,60 350,35 420,50"/></svg></div>`},
 {x:1480,y:600,w:380,h:300,bg:'#e9f3ff',tb:'#d3e6fb',c:'#0b3d7a',t:'AppZapper X',body:`<div style="margin:24px;height:190px;border:3px dashed #7aa7dd;border-radius:16px;display:flex;align-items:center;justify-content:center;text-align:center">Drop apps here<br>to uninstall</div>`},
 {x:90,y:640,w:330,h:250,bg:'#2b1055',tb:'#220c45',c:'#f0e6ff',t:'Dictator',body:`<div style="text-align:center;padding-top:30px"><div style="width:90px;height:90px;border-radius:50%;background:#ff3b6b;margin:0 auto;display:flex;align-items:center;justify-content:center;font-size:40px">●</div><div style="margin-top:14px">Listening… (2 min left)</div></div>`},
 {x:1150,y:300,w:300,h:220,bg:'#fff',tb:'#eee',c:'#333',t:'StayUp',body:`<div style="padding:22px;display:flex;align-items:center;justify-content:space-between"><span style="font:700 26px Inter">Awake</span><span style="width:70px;height:40px;border-radius:20px;background:#34c759;position:relative"><i style="position:absolute;right:4px;top:4px;width:32px;height:32px;border-radius:50%;background:#fff"></i></span></div><div style="padding:0 22px;opacity:.6">Trial: 3 days left</div>`},
];
$('wins').innerHTML=WINS.map((w,i)=>`<div class="win" id="w${i}" style="left:${w.x}px;top:${w.y}px;width:${w.w}px;height:${w.h}px;background:${w.bg};color:${w.c}"><div class="tb" style="background:${w.tb}"><i class="tl"></i><i class="tl y"></i><i class="tl g"></i><span style="margin-left:10px;opacity:.8">${w.t}</span></div>${w.body}</div>`).join('');
const NOTES=[['#ff9500','Your trial ends in 3 days','Disk Sweeper Pro'],['#5856d6','Update available (412 MB)','ClipStack'],['#ff2d55','Upgrade to Pro for $29.99/yr','FanBoost']];
$('notes').innerHTML=NOTES.map((n,i)=>`<div class="note" id="n${i}"><i style="background:${n[0]}"></i><span><b>${n[2]}</b>${n[1]}</span></div>`).join('');
// keycap
const EARS='M17 36 L23.500 14 Q25.500 9.500 29.500 13 L42 25 Z M83 36 L76.500 14 Q74.500 9.500 70.500 13 L58 25 Z';
$('cat').innerHTML=`<ellipse id="cglow" cx="50" cy="56" rx="44" ry="40" fill="#FF5FA2" opacity="0" style="filter:blur(8px)"/><rect x="15" y="30" width="70" height="60" rx="13" fill="#CBBFA8"/><g id="cears"><path d="${EARS}" fill="#F2ECE3" stroke="#F2ECE3" stroke-width="4" stroke-linejoin="round"/></g><rect x="15" y="24" width="70" height="60" rx="13" fill="#F2ECE3"/><rect id="el" x="36" y="45" width="8" height="18" rx="4" fill="#14181A"/><rect id="er" x="56" y="45" width="8" height="18" rx="4" fill="#14181A"/>`;

let icRects=null;

const VAR='b';
function cmdPath(cx,cy,q){const a=cx-q,b=cx+q,c=cy-q,d=cy+q,r=q*.93;return `M${a} ${c-r}V${d+r}M${b} ${c-r}V${d+r}M${a-r} ${c}H${b+r}M${a-r} ${d}H${b+r}M${a} ${c-r}A${r} ${r} 0 1 0 ${a-r} ${c}M${b} ${c-r}A${r} ${r} 0 1 1 ${b+r} ${c}M${a} ${d+r}A${r} ${r} 0 1 1 ${a-r} ${d}M${b} ${d+r}A${r} ${r} 0 1 0 ${b+r} ${d}`;}
$('cat').insertAdjacentHTML('beforeend',`<path id="ccmd" d="${cmdPath(50,54,7)}" fill="none" stroke="#FF5FA2" stroke-width="4.2"/>`);
const ring=document.createElement('div');ring.style.cssText='position:absolute;border-radius:50%;border:4px solid #FF5FA2;z-index:8;pointer-events:none';$('stage').appendChild(ring);
const shadeEl=document.createElement('div');shadeEl.id='shade';shadeEl.style.cssText='position:absolute;inset:0;background:#0A0826;z-index:4;pointer-events:none';$('stage').insertBefore(shadeEl,$('mb'));
// pseudo random
const rnd=i=>{const x=Math.sin(i*127.1+3.7)*43758.5453;return x-Math.floor(x)};
function eyes(m,bl){['el','er'].forEach(id=>{const e=$(id);e.style.opacity=m;const h=18*(1-bl*.85);e.setAttribute('height',h);e.setAttribute('y',54-h/2)});$('ccmd').style.opacity=1-m;}
function placeCat(x,y,size,sq=0){const c=$('cat');c.style.width=c.style.height=size+'px';c.style.left=(x-size/2)+'px';c.style.top=(y-size/2)+'px';c.style.transform=`scale(${1+sq*.15},${1-sq*.15})`;c.style.transformOrigin='50% 85%';}
function render(t){
  $('menu').style.display='none';$('boot').style.opacity=0;
  const stage=$('stage');stage.style.transform='';
  // ---------- chaos 0–3 ----------
  ICONS.forEach((_,i)=>{const e=$('ic'+i);const s=spring(t-.05-i*.11,9,16);e.style.transform=`scale(${s})`;e.style.opacity=1;});
  WINS.forEach((w,i)=>{const e=$('w'+i);const d=t-.375*(i+1)*.95;const s=spring(d,8,14);e.style.opacity=clamp(d/.12);e.style.transform=`scale(${lerp(.85,1,s)})`;});
  NOTES.forEach((n,i)=>{const e=$('n'+i);const d=t-(1.6+i*.375);const p=eo5(clamp(d/.3));e.style.top=(74+i*96)+'px';e.style.transform=`translateX(${lerp(460,0,p)}px)`;e.style.opacity=p;});
  const cnt=$('count');cnt.style.left=(BASE[0]-85)+'px';cnt.style.top='14px';cnt.style.opacity=t<1.4?0:t<2.6?1:0;cnt.textContent=Math.max(1,Math.min(11,Math.floor((t-.05)/.11)+1))+'';
  shadeEl.style.opacity=0; ring.style.opacity=0; $('cat').style.opacity=0; $('word').innerHTML='';
  // items for both transitions
  const items=[];
  WINS.forEach((w,i)=>items.push({el:$('w'+i),x:w.x+w.w/2,y:w.y+w.h/2,base:`scale(1)`}));
  NOTES.forEach((n,i)=>items.push({el:$('n'+i),x:1686,y:74+i*96+40,base:''}));
  ICONS.forEach((_,i)=>items.push({el:$('ic'+i),x:BASE[i],y:29,base:'',icon:true}));
  if(VAR==='a'){
    // keycap lands in the menu bar 2.625, swallows everything 3.0–4.1, then hero
    const kx=BASE[0]-70, ky=29;
    let sq=0; const order=items.map((it,i)=>i).sort((p,q)=>Math.hypot(items[p].x-kx,items[p].y-ky)-Math.hypot(items[q].x-kx,items[q].y-ky));
    order.forEach((idx,k)=>{const it=items[idx];const st=3.0+k*.03, p=eio(P(t,st,st+.42));
      if(t>st){const dx=(kx-it.x)*p,dy=(ky-it.y)*p;it.el.style.transform=`translate(${dx}px,${dy}px) rotate(${(rnd(idx)-.5)*40*p}deg) scale(${lerp(1,.04,p)})`;it.el.style.opacity=1-P(t,st+.32,st+.42);}
      const arr=st+.42; if(t>arr) sq+=Math.exp(-(t-arr)*14)*Math.sin((t-arr)*30)*.5;});
    const appear=spring(t-2.625,8,14);
    let size=lerp(0,40,clamp(appear))+ 14*P(t,3,4.1), x=kx, y=ky;
    // burp at 4.125
    if(t>4.125){const r=eo(P(t,4.125,4.6));ring.style.opacity=1-r;const R=lerp(20,700,r);ring.style.width=ring.style.height=2*R+'px';ring.style.left=(kx-R)+'px';ring.style.top=(ky-R)+'px';}
    const g=eio(P(t,4.5,5.15));
    if(t>4.5){size=lerp(54,240,g);x=lerp(kx,960,g);y=lerp(ky,420,g);}
    $('cat').style.opacity=t>2.62?1:0; placeCat(x,y,size,clamp(sq,-1,1)+(t>4.125&&t<4.4?Math.sin((t-4.125)*25)*.6*Math.exp(-(t-4.125)*6):0));
    eyes(1,Math.abs(t-6.3)<.08?1-Math.abs(t-6.3)/.08:(t>3&&t<4.1?.35:0));
    $('cears').setAttribute('transform',`translate(0 ${t>5.1?(1-spring(t-5.1,9,22))*6:0})`);
    $('cglow').setAttribute('opacity',t>5.1?.55*Math.exp(-(t-5.1)*1.4):0);
    shadeEl.style.opacity=t>4.5?eo(P(t,4.5,5.1))*.78:0;
    if(t>5.2) word(t,5.25,5.6);
  } else {
    // giant ⌘ keycap slams on 3.0, blast clears the desk
    const imp=3.0;
    const drop=eio(P(t,2.62,imp));
    let size=lerp(900,280,drop), y=lerp(-500,430,drop);
    if(t>imp){size=280*(1+.08*Math.exp(-(t-imp)*9)*Math.cos((t-imp)*30));y=430;}
    $('cat').style.opacity=t>2.62?1:0;
    // shake
    if(t>imp&&t<imp+.4){const k=Math.exp(-(t-imp)*10)*16;stage.style.transform=`translate(${(rnd(Math.floor(t*60))-.5)*k}px,${(rnd(Math.floor(t*60)+9)-.5)*k}px)`;}
    items.forEach((it,i)=>{if(t>imp){const p=eo(P(t,imp,imp+.6));let vx=it.x-960,vy=it.y-430;const L=Math.hypot(vx,vy)||1;vx/=L;vy/=L;const d=lerp(0,900+rnd(i)*500,p);it.el.style.transform=`translate(${vx*d}px,${vy*d}px) rotate(${(rnd(i+3)-.5)*90*p}deg) scale(${1-p*.3})`;it.el.style.opacity=1-P(t,imp+.2,imp+.6);}});
    if(t>imp){const r=eo(P(t,imp,imp+.55));ring.style.opacity=1-r;const R=lerp(140,1200,r);ring.style.width=ring.style.height=2*R+'px';ring.style.left=(960-R)+'px';ring.style.top=(430-R)+'px';}
    shadeEl.style.opacity=t>imp?eo(P(t,imp,imp+.5))*.78:clamp((t-2.62)/.38)*.3;
    const m=eio(P(t,3.75,4.1));
    eyes(m,Math.abs(t-6.0)<.08?1-Math.abs(t-6.0)/.08:0);
    $('cears').setAttribute('transform',`translate(0 ${t>4.5?(1-spring(t-4.5,9,22))*6:0})`);
    $('cglow').setAttribute('opacity',t>imp?.6*Math.exp(-(t-imp)*1.2):0);
    placeCat(960,y,size,0);
    if(t>4.5) word(t,4.55,4.9);
  }
}
function word(t,a,b){const w=$('word');const p1=eo5(P(t,a,a+.35)),p2=eo5(P(t,b,b+.35));w.innerHTML=`<span style="display:inline-block;opacity:${p1};transform:translateY(${lerp(40,0,p1)}px)">One app.</span> <span class="it" style="display:inline-block;opacity:${p2};transform:translateY(${lerp(40,0,p2)}px)">Cmd+Meow.</span>`;w.style.top='640px';}
let BASE=[];
return {render, init(){BASE=ICONS.map((_,i)=>{const r=__r($('ic'+i));return r.left+r.width/2});}};
}
