/* eslint-disable */
// @ts-nocheck
// Main scene, Dark Miami (docs/ui-rules.md §13). Draws the click-through for time t.
// Every element lookup is scoped to `root`; rects are measured in stage pixels (Studio zoom safe).
export function create(root: HTMLElement): {render:(t:number)=>void; init:()=>void} {
const __r=(el:any)=>{const r=el.getBoundingClientRect(),R=root.getBoundingClientRect(),s=R.width/root.offsetWidth||1;const left=(r.left-R.left)/s,top=(r.top-R.top)/s,width=r.width/s,height=r.height/s;return {left,top,width,height,right:left+width,bottom:top+height,x:left,y:top};};

const $=id=>root.querySelector("#"+id);
const clamp=(x,a=0,b=1)=>Math.min(b,Math.max(a,x)), lerp=(a,b,p)=>a+(b-a)*p, P=(t,a,b)=>clamp((t-a)/(b-a));
const eo=p=>1-Math.pow(1-p,3), eo5=p=>1-Math.pow(1-p,5), eio=p=>p<.5?4*p*p*p:1-Math.pow(-2*p+2,3)/2;
const spring=(dt,k=7,w=13)=>dt<=0?0:1-Math.exp(-k*dt)*Math.cos(w*dt);
// grid: track 2, 115 BPM; bar k starts at 3 + k*B
const B=2.0870056, b=B/4, K=k=>3+k*B;
const CX=1250, CY=610;          // small windows (notes, editor, messenger)
const AX=1322, AY=596, AS=.95;  // app window centre and scale
// Dark Miami tokens (TH5)
const ACT='#FF5FA2', ACC='#FF9DC6', AQ='#4FE3E0', CR='#FF9A4D', LAV='#C3B0FF', GR='#7EEBA6', AM='#FFC06B', RD='#FF6B5E';
const COL={create:CR,clean:ACT,monitor:AQ,util:LAV};
const TRK='rgba(255,255,255,.1)';
// ---- keycap cat ----
function cmdPath(cx,cy,q){const a=cx-q,bb=cx+q,c=cy-q,d=cy+q,r=q*.93;return `M${a} ${c-r}V${d+r}M${bb} ${c-r}V${d+r}M${a-r} ${c}H${bb+r}M${a-r} ${d}H${bb+r}M${a} ${c-r}A${r} ${r} 0 1 0 ${a-r} ${c}M${bb} ${c-r}A${r} ${r} 0 1 1 ${bb+r} ${c}M${a} ${d+r}A${r} ${r} 0 1 1 ${a-r} ${d}M${bb} ${d+r}A${r} ${r} 0 1 0 ${bb+r} ${d}`;}
const EARS='M17 36 L23.500 14 Q25.500 9.500 29.500 13 L42 25 Z M83 36 L76.500 14 Q74.500 9.500 70.500 13 L58 25 Z';
const kc=(id,cmd)=>`<rect x="15" y="30" width="70" height="60" rx="13" fill="#CBBFA8"/><g id="${id}ears"><path d="${EARS}" fill="#F2ECE3" stroke="#F2ECE3" stroke-width="4" stroke-linejoin="round"/></g><rect x="15" y="24" width="70" height="60" rx="13" fill="#F2ECE3"/>`+(cmd?`<path id="${id}cmd" d="${cmdPath(50,54,7)}" fill="none" stroke="${ACT}" stroke-width="4.2"/>`:'')+`<rect id="${id}el" x="36" y="45" width="8" height="18" rx="4" fill="#14181A"/><rect id="${id}er" x="56" y="45" width="8" height="18" rx="4" fill="#14181A"/>`;
$('cat').innerHTML=kc('k',true);$('sloticon').innerHTML=kc('m',false); $('camcat').innerHTML=kc('c',false);
function cat(x,y,size,o=1,eyesOn=1,blink=0,cmd=0){const c=$('cat');c.style.display=o>0?'block':'none';c.style.opacity=o;c.style.width=c.style.height=size+'px';c.style.transform=`translate(${x-size/2}px,${y-size/2}px)`;
  const h=18*(1-blink*.85);['kel','ker'].forEach(id=>{$(id).setAttribute('height',h);$(id).setAttribute('y',54-h/2);$(id).style.opacity=eyesOn});$('kcmd').style.opacity=cmd;}
// ring: radius r, share p, colour, stroke width; optional icon in the middle
function ring(r,p,col,sw=5,inner=''){const C=2*Math.PI*r,sz=2*r+sw+2;return `<span class="rg" style="width:${sz}px;height:${sz}px"><svg width="${sz}" height="${sz}" viewBox="0 0 ${sz} ${sz}" style="transform:rotate(-90deg)"><circle cx="${sz/2}" cy="${sz/2}" r="${r}" fill="none" stroke="${TRK}" stroke-width="${sw}"/><circle cx="${sz/2}" cy="${sz/2}" r="${r}" fill="none" stroke="${col}" stroke-width="${sw}" stroke-linecap="round" stroke-dasharray="${C*clamp(p)} ${C}"/></svg>${inner}</span>`;}
// line icons (24 grid)
const ic=(d,c='#fff',s=18,w=1.8)=>`<svg width="${s}" height="${s}" viewBox="0 0 24 24" fill="none" stroke="${c}" stroke-width="${w}" stroke-linecap="round" stroke-linejoin="round">${d}</svg>`;
const D={
 broom:'<path d="M14 4l6 6M10.5 7.5l6 6-6 6.5-8-8z"/>', map:'<rect x="3.5" y="3.5" width="17" height="17" rx="2"/><path d="M3.5 12h17M12 3.5v17"/>',
 trash:'<path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/>', power:'<path d="M12 3v8M6.3 7a8 8 0 1 0 11.4 0"/>',
 pulse:'<path d="M3 12h4l3-7 4 14 3-7h4"/>', gauge:'<path d="M4.5 18a8.5 8.5 0 1 1 15 0M12 14l4-5"/>',
 fan:'<circle cx="12" cy="12" r="1.6"/><path d="M12 10.4C11 6 13 3 16 4c1 2.5-1 5.5-4 6.4M13.5 12.8c4 1.5 5.5 4.5 3.5 6.5-2.5.4-5-2-5-5.2M10.5 12.8C7 15.5 3.5 15 3.3 12.3c1.5-2 4.5-2.3 7.2-1.5"/>',
 crop:'<path d="M7 3v13a1 1 0 0 0 1 1h13M17 21V8a1 1 0 0 0-1-1H3"/>', video:'<path d="M3.5 6.5h12v11h-12zM15.5 10.5l5-3v9l-5-3"/>',
 link:'<path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/>',
 mic:'<path d="M9 4.5a3 3 0 0 1 6 0v6.5a3 3 0 0 1-6 0zM5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21"/>', clip:'<path d="M9 3.5h6v3H9zM9 5H5.5v15.5h13V5H15"/>',
 cup:'<path d="M5 9h11v5a5 5 0 0 1-5 5h-1a5 5 0 0 1-5-5zM16 10h1.5a2.5 2.5 0 0 1 0 5H16"/>', display:'<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M9 20h6M12 16v4"/>',
 gear:'<circle cx="12" cy="12" r="3"/><path d="M12 2.5v3M12 18.5v3M2.5 12h3M18.5 12h3M5.3 5.3l2.1 2.1M16.6 16.6l2.1 2.1M5.3 18.7l2.1-2.1M16.6 7.4l2.1-2.1"/>',
 panel:'<rect x="3.5" y="5" width="17" height="14" rx="2"/><path d="M3.5 9h17"/>', disk:'<rect x="3" y="7" width="18" height="10" rx="2"/><path d="M7 13h2"/>',
 mem:'<rect x="4" y="7" width="16" height="10" rx="1.5"/><path d="M8 7v10M12 7v10M16 7v10"/>', chip:'<rect x="7" y="7" width="10" height="10" rx="1.5"/><path d="M10 3v4M14 3v4M10 17v4M14 17v4M3 10h4M3 14h4M17 10h4M17 14h4"/>',
 bat:'<rect x="3" y="8" width="15" height="8" rx="2"/><path d="M20.5 10.5v3M6 12h6"/>', wifi:'<path d="M5 12.5a10 10 0 0 1 14 0M8 15.5a5.5 5.5 0 0 1 8 0M12 19h.01"/>',
 arrow:'<path d="M5 19L19 5M10 5h9v9"/>', blur:'<circle cx="12" cy="12" r="8" stroke-dasharray="2.5 2.5"/>', text:'<path d="M5 6h14M12 6v13"/>', frame:'<rect x="3.5" y="3.5" width="17" height="17" rx="3"/><rect x="7.5" y="7.5" width="9" height="9" rx="1"/>',
 rect:'<rect x="4" y="6" width="16" height="12" rx="1.5"/>', pen:'<path d="M4 20l4-1 11-11-3-3L5 16z"/>', hl:'<path d="M5 19h6M8 16l9-9-3-3-9 9v3z"/>', undo:'<path d="M9 7L4 12l5 5M4 12h11a5 5 0 0 1 0 10"/>',
 copy:'<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h3"/>',
 text2:'<path d="M5 6.5h14M12 6.5v12"/>', box:'<path d="M3.5 7.5 12 3l8.5 4.5v9L12 21l-8.5-4.5zM3.5 7.5 12 12l8.5-4.5M12 12v9"/>', pin:'<path d="M12 21s-7-6.2-7-11.5A7 7 0 0 1 19 9.5C19 14.8 12 21 12 21z"/><circle cx="12" cy="9.5" r="2.5"/>',
 mouse:'<rect x="6.5" y="3" width="11" height="18" rx="5.5"/><path d="M12 7v3"/>', buds:'<path d="M7 4a3 3 0 0 0-3 3v3a2 2 0 0 0 4 0V4zM17 4a3 3 0 0 1 3 3v3a2 2 0 0 1-4 0V4zM6 12v8M18 12v8"/>', kb:'<rect x="2.5" y="6.5" width="19" height="11" rx="2"/><path d="M6 10h1M9 10h1M12 10h1M15 10h1M18 10h1M7 14h10"/>',
 folder:'<path d="M3.5 6.5h6l2 2h9v10h-17z"/>', down:'<path d="M12 4v12M7 11l5 5 5-5M5 20h14"/>', doc:'<path d="M6 3h8l4 4v14H6zM14 3v4h4"/>', photo:'<rect x="3.5" y="5" width="17" height="14" rx="2"/><circle cx="9" cy="10" r="1.6"/><path d="M4 17l5-5 4 4 3-3 4 4"/>',
};
// ---- sidebar (real app order, TH5 group colours) ----
const SIDE=[['Clean',ACT,[['Cleanup','broom'],['Disk Map','map'],['Uninstaller','trash'],['Startup','power']]],['Monitor',AQ,[['Mac Health','pulse'],['AI Usage','gauge'],['Fan Control','fan']]],['Create',CR,[['Screenshot','crop','⇧⌘2'],['Video','video','⇧⌘0'],['Sharing','link'],['Dictation','mic','⌥D'],['Clipboard','clip']]],['Utilities',LAV,[['Keep Awake','cup'],['Displays','display']]]];
function sidebar(on){return `<div class="tls"><i></i><i></i><i></i></div><div class="brand"><svg width="18" height="18" viewBox="0 0 100 100">${kc('sb'+on.replace(/\W/g,''),false)}</svg>Cmd+Meow</div>`+SIDE.map(([g,c,items])=>`<div class="sg"><i style="background:${c}"></i>${g}</div>`+items.map(([n,i,k])=>`<div class="si${n===on?' on':''}">${ic(D[i],c,15,1.7)}<span>${n}</span>${k?`<span class="k">${k}</span>`:''}</div>`).join('')).join('')+
 `<div class="sbb"><div class="si" style="padding-left:10px">${ic(D.gear,'rgba(255,255,255,.75)',15,1.7)}Settings</div><div class="si" style="padding-left:10px">${ic(D.panel,'rgba(255,255,255,.75)',15,1.7)}Menu bar panel</div></div>`;}
$('sb-clip').innerHTML=sidebar('Clipboard');$('sb-clean').innerHTML=sidebar('Cleanup');$('sb-health').innerHTML=sidebar('Mac Health');$('sb-ai').innerHTML=sidebar('AI Usage');

// ---- panel: the real menu bar panel, the widgets the video uses ----
const w3=(id,vis,lb,vl,cp,span='c2')=>`<div class="cd ${span} pw" id="${id}"><div class="w3">${vis}<div><div class="lb">${lb}</div><div class="vl num">${vl}</div><div class="cp">${cp}</div></div></div></div>`;
const w1=(id,icon,c,nm,cp)=>`<div class="cd c1 pw" id="${id}"><div class="w1">${ic(D[icon],c,20,1.8)}<span class="nm">${nm}</span><span class="cp num">${cp}</span></div></div>`;
const mid=(d,c)=>`<span style="position:absolute;display:flex">${ic(d,c,16,1.8)}</span>`;
$('panel').innerHTML=`<div class="hd">MacBook Pro<span class="more"><i></i><i></i><i></i></span></div>
<div class="grid">
 ${w3('t-clean',`<span style="width:44px;display:flex;justify-content:center">${ic(D.broom,ACT,26,1.8)}</span>`,'Clean','12.4 GB','freed so far')}
 ${w1('t-cap','crop',CR,'Screenshot','⇧⌘2')}${w1('t-rec','video',CR,'Video','⇧⌘0')}
 ${w3('t-awake',ring(19,1,LAV,3,mid(D.cup,'#fff')),'Keep Awake','Awake','Since 9:40')}
 ${w1('t-clip','clip',CR,'Clipboard','10 items')}${w1('t-dict','mic',CR,'Dictation','⌥D')}
 ${w3('t-hd',ring(19,.72,AQ,5,mid(D.disk,'rgba(255,255,255,.8)')),'Macintosh HD','1.44 TB','Free of 2.00 TB')}
 ${w3('t-mem',ring(19,.17,AQ,5,mid(D.mem,'rgba(255,255,255,.8)')),'Memory','17%','Normal')}
 ${w3('t-cpu',ring(19,.16,AQ,5,mid(D.chip,'rgba(255,255,255,.8)')),'CPU','16%','Cool')}
 ${w3('t-bat',ring(19,.8,GR,5,mid(D.bat,'rgba(255,255,255,.8)')),'Battery','80%','Not charging')}
 <div class="cd c4 pw" id="t-ai" style="display:flex;align-items:center"><div class="w3" style="width:50%;border-right:1px solid rgba(255,255,255,.08)">${ring(19,.19,AQ,5,mid(D.gauge,'rgba(255,255,255,.8)'))}<div><div class="lb">AI Usage</div><div class="vl num">19%</div><div class="cp">Resets 21:10</div></div></div>
  <div style="flex:1;padding-left:12px;display:grid;grid-template-columns:auto 1fr auto;gap:10px 8px;align-items:center;font-size:12px"><span style="color:var(--t2)">Week</span><span class="bar"><i style="width:62%;background:${AQ}"></i></span><span class="num">62%</span><span style="color:var(--t2)">Today</span><span></span><span class="num">3.4M tokens</span></div></div>
</div><div style="text-align:center;color:var(--t2);font-size:12px;padding:10px 0 2px">Edit widgets…</div>`;
const pws=[...root.querySelectorAll('#panel .pw')];

// ---- editor: icon tool row + the current tool's option strip ----
const TOOLS=[['arrow','Arrow'],['rect','Box'],['pen','Pen'],['text','Label'],['hl','Highlighter'],['blur','Blur'],['frame','Frame']];
$('tools').innerHTML=TOOLS.map(([i],k)=>`<span class="ti" id="tl${k}">${ic(D[i],'rgba(255,255,255,.85)',19,1.7)}</span>`).join('')+`<span style="width:1px;height:22px;background:rgba(255,255,255,.12);margin:0 6px"></span><span class="ti">${ic(D.undo,'rgba(255,255,255,.5)',18,1.7)}</span><span class="qbtn" id="copyb" style="margin-left:auto;gap:8px">${ic(D.copy,'#fff',15,1.7)}Copy ⌘C</span>`;
const toolEls=[0,5,3,6].map(k=>$('tl'+k)); const toolNames=['Arrow','Blur','Label','Frame'];
$('bars').innerHTML=[42,58,51,77,64,92,86].map((h,i)=>`<div style="flex:1;border-radius:10px 10px 4px 4px;background:${i===5?CR:'#cfcabf'};height:${h}%"></div>`).join('');
// ---- clipboard (everyday clips, as on the site's panel) ----
const th=(d,c='rgba(255,255,255,.7)')=>`<span class="th">${ic(d,c,16,1.7)}</span>`;
const CLIP=[
 ['shot',`<span class="th" style="background:#F6F3EC"><b style="width:22px;height:4px;background:#1d1d1b;border-radius:2px"></b></span><span>Screenshot 19:42</span><span class="k">now</span>`],
 ['a',`${th(D.text2)}<span>Hi! Could you send me the photos from Sunday?</span><span class="k">2 min</span>`],
 ['b',`<span class="th" style="background:linear-gradient(160deg,#FF9A4D,#5E1D57)"></span><span>IMG_2041.HEIC</span><span class="k">9 min</span>`],
 ['c',`${th(D.pin)}<span>221B Baker Street, London</span><span class="k">1 h</span>`],
 ['d',`${th(D.box)}<span class="num">1Z 999 AA1 01 2345 6784</span><span class="k">3 h</span>`],
 ['e',`${th(D.text2)}<span>Table for 4 at 20:30, Saturday</span><span class="k">5 h</span>`],
 ['f',`${th(D.link,ACC)}<span>cmdmeow.com</span><span class="k">Yesterday</span>`],
 ['g',`${th(D.text2)}<span>Gate B32 · Seat 14A</span><span class="k">Yesterday</span>`],
 ['h',`<span class="th" style="background:linear-gradient(160deg,#4FE3E0,#2A1452)"></span><span>IMG_2038.HEIC</span><span class="k">Yesterday</span>`],
];
$('cliplist').innerHTML=CLIP.map(c=>`<div class="clip-row" id="cr-${c[0]}">${c[1]}</div>`).join('');
$('wave').innerHTML=Array.from({length:9},()=>'<i></i>').join('');const waveEls=[...root.querySelectorAll('#wave i')];
// ---- cleanup start screen (real layout; everyday categories) ----
const GROUPS=[['App Caches','folder',9.8],['Captures','video',6.4],['Trash','trash',3.4],['Installers','down',2.2],['Logs','doc',1.3]];const TOT=23.1, GMAX=9.8;
$('legend').innerHTML=GROUPS.map((g,i)=>`<div id="lgi${i}" style="height:44px;display:flex;align-items:center;gap:14px;border-top:1px solid rgba(255,255,255,.06);font:400 13px Inter"><span style="width:30px;height:30px;border-radius:15px;background:rgba(255,255,255,.07);display:flex;align-items:center;justify-content:center">${ic(D[g[1]],i<2?ACT:'#B77DE0',15,1.7)}</span><span style="width:96px">${g[0]}</span><span class="bar" style="flex:1;height:4px"><i id="lgb${i}" style="width:0;background:${['#FF5FA2','#F062B0','#D96BC4','#B77DE0','#9A7AE6'][i]}"></i></span><span id="lgs${i}" class="num" style="width:76px;text-align:right">—</span></div>`).join('');
const RR=96;$('ring').innerHTML=`<circle cx="110" cy="110" r="${RR}" fill="none" stroke="${TRK}" stroke-width="13"/><circle id="ringv" cx="110" cy="110" r="${RR}" fill="none" stroke="${ACT}" stroke-width="13" stroke-linecap="round" transform="rotate(-90 110 110)"/>`;
// ---- mac health (real 3×2 grid) ----
const CPUB=Array.from({length:44},(_,i)=>.3+.5*Math.abs(Math.sin(i*.9)*Math.sin(i*.31+1)));
const NETB=Array.from({length:44},(_,i)=>i===16||i===37?.95:.06+.08*Math.abs(Math.sin(i*1.3)));
const hc=(x,y,w,h,inner)=>`<div class="ac" style="left:${x}px;top:${y}px;width:${w}px;height:${h}px">${inner}</div>`;
const DEV=[['MX Master 3S','Mouse · 30 h left','mouse',.14,AM],['AirPods Pro','Buds 38 · Case 71','buds',.38,GR],['Magic Keyboard','Keyboard · 19 d left','kb',.82,GR]];
$('hgrid').innerHTML=
 hc(25,17,365,320,`<div class="h">CPU load ›<span id="hcpu" class="num" style="margin-left:auto;font:400 28px Inter">16%</span></div><div id="cpubars" class="abs" style="left:15px;right:15px;top:70px;height:180px;display:flex;align-items:flex-end;gap:3px">${CPUB.map((v,i)=>`<i id="cb${i}" style="flex:1;border-radius:1.5px;background:${AQ};height:${v*100}%"></i>`).join('')}</div><div class="abs" style="left:15px;right:15px;top:256px;display:flex;justify-content:space-between;font:400 11px Inter;color:var(--t3)"><span>2 min ago</span><span>now</span></div><div class="abs" style="left:15px;top:290px;font:400 12px Inter;color:var(--t2)">User <span id="hcu" class="num">11%</span>&nbsp;&nbsp;&nbsp;System 5%</div>`)+
 hc(407,17,260,320,`<div class="h">Memory ›</div><div style="font:400 34px Inter;margin-top:18px" class="num" id="hmem">17%</div><div style="font:400 12px Inter;color:${GR};margin-top:2px">Normal</div><div class="bar" style="margin-top:20px;height:8px;border-radius:4px"><i id="hmemb" style="width:17%;background:${AQ}"></i></div><div class="r" style="margin-top:22px">Apps<b class="num">5.2 GB</b></div><div class="r">Cached<b class="num">3.1 GB</b></div><div class="r">Swap used<b class="num">0 MB</b></div><div class="r">Installed<b class="num">32 GB</b></div>`)+
 hc(684,17,261,320,`<div class="h">Macintosh HD ›</div><div class="abs" style="left:40px;top:52px" id="hhd"></div><div class="abs" style="left:40px;top:52px;width:184px;height:184px;display:flex;align-items:center;justify-content:center;text-align:center"><div><div style="font:400 28px Inter" class="num">1.44 TB</div><div style="font:400 11.5px Inter;color:var(--t2)">free of 2.00 TB</div></div></div><div class="r abs" style="left:15px;right:15px;top:250px;margin:0">Trash<b class="num">46.1 MB</b></div><span class="qbtn abs" style="left:15px;top:280px">Free up space</span>`)+
 hc(25,354,365,320,`<div class="h">Home Wi-Fi ›<span class="num" style="margin-left:auto;font:400 28px Inter" id="hnet">↓ 20.0 KB/s</span></div><div class="abs" style="left:15px;right:15px;top:70px;height:110px;display:flex;align-items:flex-end;gap:3px">${NETB.map((v,i)=>`<i id="nb${i}" style="flex:1;border-radius:1.5px;background:${AQ};height:${v*100}%"></i>`).join('')}</div><div class="abs" style="left:15px;right:15px;top:186px;display:flex;justify-content:space-between;font:400 11px Inter;color:var(--t3)"><span>2 min ago</span><span>now</span></div><div class="abs" style="left:15px;right:15px;top:206px"><div class="r">Upload<b class="num">↑ 3.20 MB/s</b></div><div class="r">Today<b class="num">5.40 GB in · 1.20 GB out</b></div></div><div class="abs" style="left:15px;top:276px;display:flex;gap:8px"><span class="qbtn">Quick test</span><span class="qbtn">Full test</span></div>`)+
 hc(407,354,260,320,`<div class="h">Battery ›</div><div class="abs" style="left:38px;top:50px" id="hbat"></div><div class="abs" style="left:38px;top:50px;width:184px;height:184px;display:flex;align-items:center;justify-content:center;text-align:center"><div><div id="hbatv" style="font:400 28px Inter" class="num">80%</div><div style="font:400 11.5px Inter;color:var(--t2)">charge</div></div></div><div class="abs" style="left:15px;right:15px;top:246px"><div class="r">Health<b class="num">100%</b></div><div class="r">Cycles<b class="num">13 of 1000</b></div></div>`)+
 hc(684,354,261,320,`<div class="h">Devices ›</div>${DEV.map(([n,s,i,p,c],k)=>`<div style="display:flex;align-items:center;gap:10px;margin-top:${k?14:18}px"><span style="width:20px;display:flex">${ic(D[i],'rgba(255,255,255,.75)',17,1.6)}</span><div style="flex:1"><div style="font:400 12.5px Inter">${n}</div><div style="font:400 10.5px Inter;color:var(--t2)">${s}</div></div><span class="bar" style="width:40px;height:4px"><i style="width:${p*100}%;background:${c}"></i></span><span class="num" style="width:34px;text-align:right;font:400 12px Inter;color:${c===AM?AM:'#fff'}">${Math.round(p*100)}%</span></div>`).join('')}`);
// ---- AI usage (real page; developer examples belong here) ----
$('aitabs').innerHTML=['All','Claude','Codex','Gemini','OpenCode'].map(n=>`<span class="${n==='Claude'?'on':''}">${n}</span>`).join('');
const DAYS=Array.from({length:30},(_,i)=>{const x=Math.sin(i*1.7)*.5+.5,y=Math.sin(i*.37+1)*.5+.5;return .18+.75*x*y+(i%7===5||i%7===6?-.12:0)});DAYS[29]=.82;
const lim=(x,id,name,res,col)=>hc(x,17,217,170,`<div class="h">${name} ›</div><div class="abs" style="left:15px;top:44px" id="${id}r"></div><div class="abs num" id="${id}p" style="left:15px;top:44px;width:116px;height:116px;display:flex;align-items:center;justify-content:center;font:400 26px Inter">0%</div><div class="abs" style="left:140px;top:84px;font:400 11.5px/1.45 Inter;color:var(--t2)">${res}</div>`);
$('aigrid').innerHTML=lim(25,'l0','Session','Resets<br>21:10',AQ)+lim(259,'l1','Week','<span style="color:#FF6B5E">Runs out<br>Thu 16:50</span><br>Resets<br>Sat 06:33')+lim(493,'l2','Week · Opus','Resets<br>Sat 06:33')+
 hc(727,17,218,170,`<div class="h">Saved · 30 days</div><div style="display:flex;align-items:baseline;gap:8px;margin-top:12px"><span id="aisaved" class="num" style="font:400 30px Inter">$0</span><span style="font:400 11.5px Inter;color:var(--t2)">7.8× plan</span></div><div class="r" style="margin-top:26px">At API prices<b class="num">$780</b></div><div class="r">You paid<b class="num">$100</b></div>`)+
 hc(25,204,451,452,`<div class="h">Usage · 30 days<span class="seg" style="margin-left:auto;font-size:11.5px"><span class="on" style="padding:4px 12px">Tokens</span><span style="padding:4px 12px">$</span></span></div><div style="margin-top:14px;font:400 13px Inter">Today&nbsp;&nbsp;<span id="aiusd" class="num">$0</span>&nbsp;&nbsp;<span style="color:var(--t2)" class="num"><span id="aitok">0</span> tok&nbsp;&nbsp;39 req</span></div><div id="aibars" class="abs" style="left:15px;right:15px;top:92px;height:322px;display:flex;align-items:flex-end;gap:4px">${DAYS.map((v,i)=>`<i id="db${i}" style="flex:1;border-radius:2px;background:${i===29?AQ:'#3A4A6E'};height:0"></i>`).join('')}</div><div class="abs" style="left:15px;right:15px;top:424px;display:flex;justify-content:space-between;font:400 11px Inter;color:var(--t2)"><span>Oct 26</span><span>Today</span></div>`)+
 hc(493,204,452,218,`<div class="h">Models<span style="margin-left:auto;font:400 11px Inter;color:var(--t2)">30 days</span></div>${[['Opus 4.6',1,'47.9M tok · $584'],['Sonnet 4.6',.5,'24.1M tok · $177'],['Haiku 4.5',.16,'7.7M tok · $18.86']].map(([n,p,v],k)=>`<div style="display:flex;align-items:center;gap:12px;margin-top:${k?10:16}px;font:400 12px Inter"><span style="width:66px;color:var(--t2)">${n}</span><span class="bar" style="flex:1;height:4px"><i class="aim" data-p="${p}" style="width:0;background:${AQ}"></i></span><span class="num" style="width:120px;text-align:right">${v}</span></div>`).join('')}`)+
 hc(493,438,452,218,`<div class="h">Projects<span style="margin-left:auto;font:400 11px Inter;color:var(--t2)">30 days</span></div>${[['cmdmeow',1,'40.8M tok · $399'],['site',.52,'21.0M tok · $199'],['api-gateway',.25,'10.1M tok · $103'],['notes',.15,'6.3M tok · $63.33']].map(([n,p,v],k)=>`<div style="display:flex;align-items:center;gap:12px;margin-top:${k?9:14}px;font:400 12px Inter"><span style="width:100px;color:var(--t2)">${n}</span><span class="bar" style="flex:1;height:4px"><i class="aim" data-p="${p}" style="width:0;background:${AQ}"></i></span><span class="num" style="width:120px;text-align:right">${v}</span></div>`).join('')}`);
const aims=[...root.querySelectorAll('#aigrid .aim')];
// ---- and more ----
const MC=[
 `<div class="h">${ic(D.cup,LAV,22)}Keep Awake</div><div style="display:flex;align-items:center;gap:22px;margin-top:24px"><span id="awr">${ring(40,1,LAV,6,mid(D.cup,'#fff'))}</span><div><div style="font:500 30px Inter">Awake</div><div style="font:400 16px Inter;color:var(--t2);margin-top:4px">On · Since 9:40</div></div></div>`,
 `<div class="h">${ic(D.fan,AQ,22)}Fan Control</div><div style="display:flex;align-items:center;gap:22px;margin-top:24px"><span id="fanic" style="display:flex">${ic(D.fan,AQ,84,1.4)}</span><div><div id="rpm" class="num" style="font:500 30px Inter">2 225 rpm</div><div style="font:400 16px Inter;color:${AM};margin-top:4px">P-cores 71 °C</div></div></div>`,
 `<div class="h">${ic(D.trash,ACT,22)}Uninstaller</div><div style="display:flex;align-items:center;gap:18px;margin-top:22px"><div style="width:72px;height:72px;border-radius:17px;background:linear-gradient(145deg,#4FE3E0,#7F63D0);display:flex;align-items:center;justify-content:center;font:700 30px Inter">Ph</div><div><div style="font:500 22px Inter">PhotoTrim</div><div style="font:400 16px Inter;color:var(--t2);margin-top:4px">App + 3 leftovers · 1.3 GB</div></div></div><span id="unb" class="btn" style="position:absolute;right:24px;bottom:22px;height:40px;font-size:16px">Remove</span>`,
 `<div class="h">${ic(D.link,CR,22)}Sharing</div><div style="margin-top:30px;height:60px;border-radius:12px;background:rgba(255,255,255,.07);border:1px solid rgba(255,255,255,.12);display:flex;align-items:center;gap:14px;padding:0 16px;font:500 20px Inter"><span style="width:30px;height:30px;border-radius:50%;background:${GR};display:flex;align-items:center;justify-content:center">${ic('<path d="M5 12.5l4.5 4.5L19 7.5"/>','#0E1405',18,3)}</span>cmdmeow.com/v/8f2k</div><div style="margin-top:12px;font:400 15px Inter;color:var(--t2)">Link copied</div>`];
MC.forEach((h,i)=>$('mc'+i).innerHTML=`<div class="in"></div><div style="position:relative">${h}</div>`);

// ---- panel geometry ----
const PS=1.4;let SX=0,SY=29,PX=0;const TILE={};
function panelXf(open){$('panel').style.transformOrigin='top left';$('panel').style.transform=`translate(${PX}px,70px) scale(${PS},${PS*lerp(.6,1,open)})`;}
function measure(){const sl=__r($('slot'));SX=sl.left+sl.width/2;PX=Math.min(SX-190*PS,1920-380*PS-24);
  $('panel').style.display='block';panelXf(1);pws.forEach(e=>{e.style.transform='';e.style.opacity=1});
  pws.forEach(e=>{const r=__r(e);TILE[e.id]=[r.left+r.width/2,r.top+r.height/2]});}
// ---- helpers ----
function setCur(x,y,press=0,o=1){const c=$('cur');c.style.display=o>0?'block':'none';c.style.opacity=o;c.style.transform=`translate(${x}px,${y}px) scale(${press?.84:1})`;}
const near=(t,a,w=.07)=>Math.abs(t-a)<w;
function openW(el,t,o,c,from,cx=CX,cy=CY,sc=1){ // macOS-like zoom from the clicked tile
  if(t<o||t>c){el.style.display='none';return false;}el.style.display='block';
  const w=el.offsetWidth,h=el.offsetHeight,po=eo5(P(t,o,o+.42)),pc=eio(P(t,c-.24,c));
  el.style.left=(cx-w/2)+'px';el.style.top=(cy-h/2)+'px';
  const fx=from?from[0]-cx:0,fy=from?from[1]-cy:0;
  el.style.transform=`translate(${fx*(1-po)}px,${fy*(1-po)}px) scale(${sc*lerp(.12,1,po)*(1-pc*.06)})`;el.style.opacity=clamp(po*2.2)*(1-pc);return true;}
const openA=(el,t,o,c,from)=>openW(el,t,o,c,from,AX,AY,AS);
function uiPt(el,x,y){const r=__r(el),s=r.width/el.offsetWidth;return [r.left+x*s,r.top+y*s];}
function ctr(el){const r=__r(el);return [r.left+r.width/2,r.top+r.height/2];}
// panel visit: click menu bar icon at ic, click tile at tc
function visit(t,from,fromT,icT,tc,tile,hold=0){
  const pn=$('panel');const op=eo5(P(t,icT,icT+.3)),cl=eio(P(t,tc+.04,tc+.22));
  if(t>=icT&&t<tc+.22){pn.style.display='block';panelXf(op);pn.style.opacity=clamp(op*3)*(1-cl);}
  const [tx,ty]=TILE[tile];const ix=SX+4,iy=SY+6;
  if(t<icT){const p=eio(P(t,fromT,icT-.04));setCur(lerp(from[0],ix,p),lerp(from[1],iy,p),near(t,icT,.05));}
  else if(t<tc+.3){const p=eio(P(t,icT+.12+hold,tc-.05));setCur(lerp(ix,tx,p),lerp(iy,ty,p),near(t,tc,.06)||near(t,icT,.05));}
  $('slot').style.background=t>=icT&&t<tc+.1?'rgba(255,255,255,.22)':'';
  const te=$(tile);te.style.background=near(t,tc,.12)?'rgba(255,255,255,.16)':'';
}
// left title
const LBL={intro:['One click.',null,'#fff'],shot:['Screenshot',['⇧','⌘','2'],COL.create],mark:['Mark it up',null,COL.create],rec:['Video',['⇧','⌘','0'],COL.create],clip:['Clipboard',null,COL.create],say:['Dictation',['⌥','D'],COL.create],clean:['Clean up',null,COL.clean],health:['Mac Health',null,COL.monitor],ai:['AI Usage',null,COL.monitor],more:['And more.',null,'#fff']};
let curL='';
function lab2(t,s,e,id,keyOn){const L=$('lab');L.style.display='flex';const [txt,keys,col]=LBL[id];
  if(curL!==id){$('ttl').textContent=txt;$('keys').innerHTML=(keys||[]).map(x=>`<span class="key">${x}</span>`).join('');$('keys').style.setProperty('--kc',col);curL=id;}
  const p=eo5(P(t,s,s+.35)),q=eio(P(t,e-.25,e));$('ttl').style.opacity=p*(1-q);$('ttl').style.transform=`translateY(${lerp(40,0,p)-q*30}px)`;$('ttl').style.filter=`blur(${lerp(10,0,p)+q*8}px)`;
  $('keys').style.opacity=eo5(P(t,s+.1,s+.45))*(1-q);root.querySelectorAll('#keys .key').forEach(k=>k.classList.toggle('on',!!keyOn));}

// ================== TIMELINE ==================
const T={intro:K(2),shot:K(3),recV:K(5),rec:K(5.5),clipV:K(7),clip:K(7.5),sayV:K(8.5),say:K(9),cleanV:K(10.5),clean:K(11),hV:K(12.25),h:K(12.75),aiV:K(13.75),ai:K(14.25),more:K(15.25),end:K(16)};
Object.assign($('notesw').style,{left:(1200-410)+'px',top:(640-280)+'px'});
const vic=s0=>s0+.75*b, vtc=s0=>s0+2*b;
function render(t){
  ['lab','cur'].forEach(i=>$(i).style.display='none');$('cat').style.display='none';$('panel').style.display='none';
  root.querySelectorAll('.w,.mc').forEach(e=>e.style.display='none');['sel','endwm','endurl','enddl'].forEach(i=>$(i).style.display='none');$('flash').style.opacity=0;
  $('sloticon').style.opacity=t>T.intro+b*1.2?1:0;
  $('slot').style.background='';pws.forEach(e=>{e.style.background='';});
  $('mb').style.display='flex';$('mb').style.opacity=1;$('glow').style.background='';
  $('shade').style.opacity=t<T.end?lerp(.55,.22,eo(P(t,T.intro,T.intro+.6))):lerp(.22,.6,eo(P(t,T.end,T.end+.6)));
  // ---- intro: cat flies into menu bar, click, panel cascade ----
  if(t<T.shot+.3){const s=T.intro,icT=s+2*b,tc=T.shot;
    if(t<icT+.1){const g=eio(P(t,s,s+b*1.2));cat(lerp(960,SX,g),lerp(430,SY,g),lerp(280,40,g),1-P(t,s+b*1.1,s+b*1.25),1,0,0);}
    visit(t,[1215,720],s+.3,icT,tc,'t-cap',.5*b);
    pws.forEach((el,i)=>{const d=t-icT-i*(b/5);const p=eo5(clamp(d/.3));el.style.opacity=p;el.style.transform=`translateY(${lerp(16,0,p)}px) scale(${lerp(.92,1,p)})`;});
    if(t>=icT&&t<tc)lab2(t,icT,tc,'intro');
    $('notesw').style.display='block';$('notesw').style.opacity=clamp((t-s-.2)/.4);
    return;}
  pws.forEach(el=>{el.style.opacity=1;el.style.transform='';});
  // ---- screenshot + mark up ----
  if(t<T.recV){const f=T.shot,me=f+2.5*b,e=T.recV;
    const nv=t<me+.4;$('notesw').style.display=nv?'block':'none';
    if(t<me)lab2(t,f,me,'shot',t<f+.3);else lab2(t,me,e,'mark');
    const x0=840,y0=380,x1=1430,y1=720,sp=eio(P(t,f+.5*b,f+2*b));
    if(t<f+2*b+.05){const sel=$('sel');sel.style.display=t>f+.45*b?'block':'none';Object.assign(sel.style,{left:x0+'px',top:y0+'px',width:lerp(0,x1-x0,sp)+'px',height:lerp(0,y1-y0,sp)+'px'});$('seldim').textContent=`${Math.round(lerp(0,x1-x0,sp)*2)} × ${Math.round(lerp(0,y1-y0,sp)*2)}`;
      const [tx,ty]=TILE['t-cap'];const cp=eio(P(t,f+.1,f+.45*b));setCur(lerp(tx,x0,cp)+(t>f+.45*b?lerp(0,x1-x0,sp):0),lerp(ty,y0,cp)+(t>f+.45*b?lerp(0,y1-y0,sp):0));}
    $('flash').style.opacity=t>f+2*b?Math.exp(-(t-f-2*b)*9)*.7:0;
    if(openW($('markw'),t,me,e,[(x0+x1)/2,(y0+y1)/2])){const s=me+.3*b;
      const step=Math.min(3,Math.max(0,Math.floor((t-s)/b)));toolEls.forEach((el,i)=>el.classList.toggle('on',t>s&&i===step));$('opname').textContent=toolNames[t>s?step:0];$('opsw').style.opacity=step===0||step===2?1:0;
      const fr=eio(P(t,s+3*b,s+3*b+.35)),pad=lerp(0,52,fr);
      Object.assign($('shot').style,{left:pad+'px',top:pad*.8+'px',width:(880-2*pad)+'px',height:(526-1.6*pad)+'px',borderRadius:lerp(0,14,fr)+'px',boxShadow:`0 ${30*fr}px ${60*fr}px rgba(0,0,0,${.55*fr})`});
      $('frame').style.background=`linear-gradient(160deg,rgba(255,154,77,${fr}),rgba(255,95,162,${fr}) 55%,rgba(94,29,87,${fr}))`;
      const ap=eo(P(t,s+.1,s+.4)),ax0=600,ay0=230,ax1=420,ay1=112,cxq=580,cyq=120;const ex=lerp(ax0,ax1,ap),ey=lerp(ay0,ay1,ap);
      $('arrp').setAttribute('d',ap>0?`M${ax0} ${ay0} Q${lerp(ax0,cxq,ap)} ${lerp(ay0,cyq,ap)} ${ex} ${ey}`:'');const ang=Math.atan2(ey-lerp(ay0,cyq,ap),ex-lerp(ax0,cxq,ap)),hl=22*clamp((ap-.8)/.2);
      $('arrh').setAttribute('d',hl>0?`M${ex-hl*Math.cos(ang-.5)} ${ey-hl*Math.sin(ang-.5)} L${ex} ${ey} L${ex-hl*Math.cos(ang+.5)} ${ey-hl*Math.sin(ang+.5)}`:'');
      $('blur').style.width=eio(P(t,s+b,s+b+.25))*100+'%';
      const tp=spring(t-s-2*b,9,14);$('txt').style.opacity=t>s+2*b?1:0;$('txt').style.transform=`scale(${tp}) rotate(-4deg)`;
      const ck=e-.5*b;$('copyb').style.background=near(t,ck,.15)?'rgba(255,255,255,.2)':'';
      const [bx,by]=ctr($('copyb'));const cp=eio(P(t,s+3.4*b,ck-.05));if(t>s+3.2*b)setCur(lerp(bx-160,bx,cp),lerp(by+260,by,cp),near(t,ck,.06));}
    return;}
  // ---- video ----
  if(t<T.clipV){const s0=T.recV,f=T.rec,e=T.clipV;
    if(t<f+.3)visit(t,[1560,470],s0,vic(s0),vtc(s0),'t-rec');
    if(openW($('recw'),t,f,e,TILE['t-rec'])){const ui=$('recw');lab2(t,f,e,'rec',t<f+.3);const st=1.2*b;
      const rp=eo5(P(t,f+.2,f+.5));$('recpill').style.opacity=rp;$('recpill').style.transform=`translateY(${lerp(-30,0,rp)}px)`;
      $('recdot').style.opacity=(Math.floor((t-f)*3)%2===0)?1:.25;$('rectime').textContent=`0:${String(Math.floor(clamp(t-f-.3,0,9)*2.5)).padStart(2,'0')}`;
      $('cam').style.transform=`scale(${spring(t-f-.35,8,12)})`;
      const ce=Math.abs(((t-f)*2.2)%3-1.5)<.08?1:0;['cel','cer'].forEach(i=>{const h=18*(1-ce*.85);$(i).setAttribute('height',h);$(i).setAttribute('y',54-h/2)});
      const pts=[[200,420],[560,300],[640,0]];
      let seg=clamp(Math.floor((t-f-.3)/st),0,3);const from=seg===0?[480,560]:pts[seg-1],to=pts[Math.min(seg,2)];const q=eio(P(t-f-.3-seg*st,0,st*.6));
      const [cx,cy]=uiPt(ui,lerp(from[0],to[0],q),lerp(from[1],to[1],q));if(t>f+.25)setCur(cx,cy,near(t,f+.3+3*st,.06),1-P(t,e-.3,e-.05));
      $('ripples').innerHTML=[1,2].map(k=>{const d=t-(f+.3+k*st);if(d<0||d>.5)return'';const p=pts[k-1];const R=lerp(10,70,eo(d/.5));return `<i class="abs" style="left:${p[0]-R}px;top:${p[1]-R}px;width:${2*R}px;height:${2*R}px;border-radius:50%;border:4px solid rgba(255,154,77,${1-d/.5})"></i>`}).join('');
      const stopT=f+.3+3*st;$('stopb').style.transform=`scale(${near(t,stopT)?.9:1})`;$('recdot').style.background=t>stopT?'#888':'#FF453A';}
    return;}
  // ---- clipboard ----
  if(t<T.sayV){const s0=T.clipV,f=T.clip,e=T.sayV;
    if(t<f+.3)visit(t,[1250,350],s0,vic(s0),vtc(s0),'t-clip');
    if(openA($('clipw'),t,f,e,TILE['t-clip'])){lab2(t,f,e,'clip');
      const ins=eo5(P(t,f+.15,f+.5)),rowH=64;let sel=t>f+b?0:-1;if(t>f+1.6*b&&t<f+2.4*b)sel=1;if(t>f+2.4*b)sel=0;
      CLIP.forEach((c,i)=>{const el=$('cr-'+c[0]);let y=8+(c[0]==='shot'?lerp(-40,0,ins):(i-1+ins)*rowH);el.style.opacity=c[0]==='shot'?ins:1;el.style.top=y+'px';el.classList.toggle('on',i===sel);});
      const pk=f+3*b;const [rx,ry]=ctr($('cr-shot'));const cp=eio(P(t,f+.5*b,pk-.05));setCur(lerp(TILE['t-clip'][0],rx+120,cp),lerp(TILE['t-clip'][1],ry,cp),near(t,pk,.06),1-P(t,e-.3,e-.05));}
    return;}
  // ---- dictation: messenger is open first, then Cmd+Meow > Dictation ----
  if(t<T.cleanV){const s0=T.sayV,f=T.say,e=T.cleanV;const ui=$('sayw');
    if(t<f+.3)visit(t,[1250,560],s0+.15,vic(s0)+.15,vtc(s0),'t-dict');
    if(openW(ui,t,s0-.15,e,null)){ui.style.transform=`scale(${lerp(.96,1,eo5(P(t,s0-.15,s0+.25)))*(1-.06*eio(P(t,e-.24,e)))})`;
      if(t>=f)lab2(t,f,e,'say',t>f+.05&&t<f+.3);
      const TXT=[..."Yes! Sending them tonight 🙌"],t0=f+.4,t1=f+1.6,send=f+1.8;
      const typed=t<send?TXT.slice(0,Math.floor(clamp((t-t0)/(t1-t0))*TXT.length)).join(''):'';
      $('saytext').textContent=typed;$('ph').style.display=typed?'none':'block';$('caret').style.opacity=t>f&&t<send?1:0;
      const ob=$('outb');const op=spring(t-send,8,14);ob.style.display=t>send?'block':'none';$('outt').textContent=TXT.join('');ob.style.opacity=clamp((t-send)/.1);ob.style.transform=`scale(${op})`;
      const pill=$('pill'),cr=__r($('caret')),u=__r(ui);const sc2=u.width/ui.offsetWidth;const px=(cr.left-u.left)/sc2,py=(cr.top-u.top)/sc2;
      const pp=eo5(P(t,f+.15,f+.4))*(1-eo(P(t,t1+.05,t1+.2)));pill.style.left=Math.min(px-20,640)+'px';pill.style.top=(py-74)+'px';pill.style.opacity=pp;pill.style.transform=`scale(${lerp(.7,1,pp)})`;
      waveEls.forEach((el,i)=>{const a=t>t0&&t<t1?.35+.65*Math.abs(Math.sin(t*9+i*1.7)*Math.sin(t*4.3+i)):.15;el.style.height=(6+24*a)+'px';});}
    return;}
  // ---- clean up: Scan -> categories fill -> Clean -> freed ----
  if(t<T.hV){const s0=T.cleanV,f=T.clean,e=T.hV;
    if(t<f+.3)visit(t,[1400,800],s0,vic(s0),vtc(s0),'t-clean');
    const sc=f+.75*b, found=f+2.4*b, ck=f+3.25*b, em=eio(P(t,ck+.1,ck+.7)), done=t>ck+.75;
    if(openA($('cleanw'),t,f,e,TILE['t-clean'])){lab2(t,f,e,'clean');
      const C=2*Math.PI*RR, fp=eo(P(t,sc+.1,found));
      // ring: free space before (61 of 271 GB), sweeps while scanning, grows by what was freed
      const free=done?(61+TOT)/271:t>sc?lerp(61/271,1,(Math.sin((t-sc)*5)*.5+.5)*(1-P(t,found-.2,found))):61/271;
      const shown=done?lerp(61/271,(61+TOT)/271,eo(P(t,ck+.75,ck+1.2))):t>found?61/271:free;
      $('ringv').setAttribute('stroke-dasharray',`${C*shown} ${C}`);
      let acc=0;GROUPS.forEach((g,i)=>{const sh=g[2]/TOT,gp=clamp((fp-acc)/sh);$('lgb'+i).style.width=(g[2]/GMAX*100*eo(gp)*(1-em))+'%';$('lgs'+i).textContent=gp>0?(done?'0 KB':(g[2]*eo(gp)*(1-em)).toFixed(g[2]<10?2:1)+' GB'):'—';acc+=sh;});
      const scanned=TOT*fp;
      $('cleannum').textContent=done?(61+TOT).toFixed(1)+' GB':t>sc&&t<found?scanned.toFixed(1)+' GB':'61.0 GB';
      $('cleanlbl').textContent=done?'free · 23.1 GB freed':t>sc&&t<found?'found so far':'free of 271 GB';$('cleanlbl').style.color=done?ACC:'';
      $('cleantot').textContent=t>sc?(TOT*fp*(1-em)).toFixed(1)+' GB':'23.1 GB';$('lastscan').textContent=t>sc&&t<found?'Scanning…':t>=found&&!done?'Found just now':done?'Cleaned just now':'Last scan 2 days ago';
      const btn=$('cleanbtn');btn.textContent=t<found?(t>sc?'Scanning…':'Scan'):done?'Done':`Clean ${TOT.toFixed(1)} GB`;btn.style.opacity=t>sc&&t<found?.55:done?.45:1;
      btn.style.transform=`scale(${near(t,sc)||near(t,ck)?.94:1})`;
      $('cleannum').style.transform=done?`scale(${1+.1*Math.exp(-(t-ck-.75)*6)*Math.cos((t-ck-.75)*18)})`:'';
      const [bx,by]=ctr(btn);const cp=eio(P(t,f+.15,sc-.05));
      if(t>f+.25){if(t<found)setCur(lerp(TILE['t-clean'][0],bx,cp),lerp(TILE['t-clean'][1],by,cp),near(t,sc,.06));else setCur(bx+lerp(40,0,eio(P(t,found,ck-.05))),by+lerp(30,0,eio(P(t,found,ck-.05))),near(t,ck,.06),1-P(t,e-.3,e-.05));}}
    return;}
  // ---- mac health ----
  if(t<T.aiV){const s0=T.hV,f=T.h,e=T.aiV;
    if(t<f+.3)visit(t,[1560,780],s0,vic(s0),vtc(s0),'t-cpu');
    if(openA($('healthw'),t,f,e,TILE['t-cpu'])){lab2(t,f,e,'health');
      const u=P(t,f+.25,e-.2),ld=Math.sin(Math.PI*Math.pow(u,.8)),sh=Math.floor((t-f)*8);
      CPUB.forEach((_,i)=>{const k=i+sh;const v=.3+.5*Math.abs(Math.sin(k*.9)*Math.sin(k*.31+1));const boost=i>30?ld*.45*((i-30)/13):0;$('cb'+i).style.height=clamp(v*(.5+.5*(i>30?1:.6))+boost)*100+'%';});
      const cpu=16+56*ld;$('hcpu').textContent=Math.round(cpu)+'%';$('hcu').textContent=Math.round(cpu-5)+'%';
      const mem=17+14*ld;$('hmem').textContent=Math.round(mem)+'%';$('hmemb').style.width=mem+'%';
      NETB.forEach((_,i)=>{const k=i+sh;const v=(k%21===16)?.95:.06+.08*Math.abs(Math.sin(k*1.3));$('nb'+i).style.height=v*100+'%';});
      $('hhd').innerHTML=ring(86,.72*eo(P(t,f+.2,f+1)),AQ,11);$('hbat').innerHTML=ring(86,.8*eo(P(t,f+.3,f+1.1)),GR,11);
      setCur(...TILE['t-cpu'],0,1-P(t,f,f+.3));}
    return;}
  // ---- AI usage ----
  if(t<T.more){const s0=T.aiV,f=T.ai,e=T.more;
    if(t<f+.3)visit(t,[1500,820],s0,vic(s0),vtc(s0),'t-ai');
    if(openA($('aiw'),t,f,e,TILE['t-ai'])){lab2(t,f,e,'ai');
      [['l0',.19,AQ],['l1',.62,RD],['l2',.04,AQ]].forEach(([id,v,c],k)=>{const p=eo(P(t,f+.25+k*.12,f+1.1+k*.12));$(id+'r').innerHTML=ring(52,v*p,c,11);$(id+'p').textContent=Math.round(v*p*100)+'%';});
      DAYS.forEach((v,i)=>{const p=eo5(P(t,f+.3+i*.018,f+.75+i*.018));$('db'+i).style.height=(v*p*100)+'%';});
      const q=eo(P(t,f+.3,f+1.4));$('aitok').textContent=(3.4*q).toFixed(1)+'M';$('aiusd').textContent='$'+(30.73*q).toFixed(2);$('aisaved').textContent='$'+Math.round(680*q);
      aims.forEach((el,i)=>el.style.width=(+el.dataset.p*100*eo(P(t,f+.4+i*.05,f+1.1+i*.05)))+'%');
      setCur(...TILE['t-ai'],0,1-P(t,f,f+.3));}
    return;}
  // ---- and more ----
  if(t<T.end){const f=T.more,e=T.end;lab2(t,f,e,'more');
    MC.forEach((_,i)=>{const el=$('mc'+i);el.style.display='block';const a=f+i*b*.6;const p=spring(t-a,8,13);
      el.style.left=(AX-434+(i%2)*448)+'px';el.style.top=(CY-244+Math.floor(i/2)*258)+'px';el.style.opacity=clamp((t-a)/.12)*(1-P(t,e-.22,e));el.style.transform=`translateY(${(1-eo5(P(t,a,a+.35)))*40}px) scale(${t<a?0:lerp(.85,1,p)})`;});
    const fi=$('fanic');if(fi){fi.style.transform=`rotate(${(t-f)*900}deg)`;}
    $('rpm').textContent=Math.round(2225+400*Math.sin((t-f)*2)).toLocaleString('en-US').replace(',',' ')+' rpm';
    return;}
  // ---- end card ----
  {const s=T.end,hit=s+b;$('mb').style.opacity=1-P(t,s,s+.4);
    const size=300,cy=400;const pop=spring(t-s,6,10)*(t>hit?1+.12*Math.exp(-(t-hit)*7)*Math.cos((t-hit)*22):1);
    cat(960,cy,size*pop,1,0,0,1);$('kears').setAttribute('transform',`translate(0 ${t>hit+.3?(1-spring(t-hit-.3,9,22))*6:6})`);
    const a=eo5(P(t,hit+.35,hit+.7)),c=eo5(P(t,hit+.6,hit+.95)),d=eo5(P(t,hit+.85,hit+1.2));
    ['endwm','endurl'].forEach(i=>$(i).style.display='block');$('enddl').style.display='inline-flex';
    const wm=$('endwm');wm.style.opacity=a;wm.style.transform=`translate(${960-wm.offsetWidth/2+16}px,${600+lerp(20,0,a)}px)`;
    const u=$('endurl');u.style.opacity=c;u.style.transform=`translate(${960-u.offsetWidth/2}px,${700+lerp(20,0,c)}px)`;
    const dl=$('enddl');dl.style.opacity=d;dl.style.transform=`translate(${960-dl.offsetWidth/2}px,${780+lerp(20,0,d)}px)`;
    $('glow').style.background=`radial-gradient(900px 700px at 960px 480px, rgba(255,95,162,${.28*a}) 0%, rgba(94,29,87,${.25*a}) 40%, transparent 70%)`;}
}
return {render, init: measure};
}
