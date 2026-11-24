// Main scene markup and styles: Dark Miami theme (docs/ui-rules.md §13 TH5/TH6).
// App surfaces mirror the real app (`CMDMEOW_THEME=miami cmdmeow --screenshot … --view …`).
export const css = `
:root{--act:#FF5FA2;--acc:#FF9DC6;--aq:#4FE3E0;--cr:#FF9A4D;--lav:#C3B0FF;--gr:#7EEBA6;--am:#FFC06B;--rd:#FF6B5E;
 --sky:linear-gradient(to bottom,#0B1240 0%,#2A1452 40%,#5E1D57 78%,#70283C 100%);
 --card:rgba(38,32,50,.54);--edge:rgba(255,255,255,.09);--t2:rgba(255,255,255,.62);--t3:rgba(255,255,255,.42)}
*{box-sizing:border-box;margin:0;padding:0}
html,body{background:#000;overflow:hidden}
body{font-family:"Inter",sans-serif;-webkit-font-smoothing:antialiased;color:#fff}
#stage{position:relative;width:1920px;height:1080px;overflow:hidden;background:#0A0826}
.abs{position:absolute;left:0;top:0}
#desk{position:absolute;inset:0;background:url(desk-miami.jpg) center/cover}
#shade{position:absolute;inset:0;background:#0A0826}
#glow{position:absolute;inset:0;pointer-events:none}
.num{font-variant-numeric:tabular-nums}
/* left label */
#lab{position:absolute;left:110px;top:0;width:620px;height:1080px;display:flex;flex-direction:column;justify-content:center;gap:18px}
#ttl{font:700 104px/1 "Inter Display";letter-spacing:-.035em;white-space:nowrap;text-shadow:0 6px 40px rgba(10,8,38,.6)}
#keys{display:flex;gap:10px;margin-top:22px}
.key{min-width:62px;height:62px;padding:0 18px;border-radius:14px;background:#1C1640;border:1.5px solid rgba(255,255,255,.14);box-shadow:inset 0 -4px 0 rgba(0,0,0,.45);display:flex;align-items:center;justify-content:center;font:600 28px Inter}
.key.on{background:var(--kc,#FF5FA2);color:#160A20;border-color:transparent}
/* menubar */
.menubar{position:absolute;left:0;top:0;width:1920px;height:58px;display:flex;align-items:center;gap:30px;padding:0 28px;background:rgba(8,6,28,.45);backdrop-filter:blur(24px);font:500 23px Inter;color:rgba(255,255,255,.92);z-index:5}
.menubar b{font-weight:700}.menubar .sp{flex:1}
.cursor{position:absolute;width:38px;height:48px;left:0;top:0;pointer-events:none;filter:drop-shadow(0 3px 6px rgba(0,0,0,.5));z-index:20}
/* menu bar panel (real panel: header + widget cards, sky backdrop) */
.pnl{width:380px;padding:12px 10px 10px;border-radius:16px;background:var(--sky);box-shadow:0 0 0 1px rgba(255,255,255,.14),0 40px 90px rgba(5,3,20,.7);font:400 13px Inter}
.pnl .hd{display:flex;align-items:center;justify-content:space-between;padding:4px 6px 12px;font-weight:600;font-size:15px}
.pnl .more{width:26px;height:26px;border-radius:13px;background:rgba(255,255,255,.08);border:1px solid rgba(255,255,255,.1);display:flex;align-items:center;justify-content:center;gap:2px}
.pnl .more i{width:3px;height:3px;border-radius:50%;background:#fff}
.grid{display:grid;grid-template-columns:repeat(4,1fr);gap:8px}
.cd{background:var(--card);border:1px solid var(--edge);border-radius:10px;padding:10px;min-height:83px;position:relative;box-shadow:0 1px 0 rgba(0,0,0,.25)}
.c2{grid-column:span 2}.c1{grid-column:span 1}.c4{grid-column:span 4}
.cd .lb{color:var(--t2);font-size:11.5px}.cd .vl{font-weight:500;font-size:15px;margin:2px 0}.cd .cp{color:var(--t2);font-size:11px}
.w3{display:flex;align-items:center;gap:12px;height:100%}
.w1{display:flex;flex-direction:column;justify-content:flex-end;height:100%;gap:2px;padding-top:24px}
.w1 svg{position:absolute;left:10px;top:12px}
.w1 .nm{font-weight:600;font-size:12px}
.rg{position:relative;flex:none;display:flex;align-items:center;justify-content:center}
.rg>svg:first-child{position:absolute;inset:0}
/* app window (sidebar + page on the sky backdrop) */
.w{position:absolute;display:none;transform-origin:center center}
.aw{position:absolute;left:0;top:0;width:1180px;height:740px;border-radius:12px;overflow:hidden;background:var(--sky);box-shadow:0 0 0 1px rgba(255,255,255,.14),0 40px 100px rgba(5,3,20,.7);font:400 13px Inter}
.aw .sb{position:absolute;left:0;top:0;bottom:0;width:210px;background:rgba(4,2,14,.42);border-right:1px solid rgba(255,255,255,.07);padding:12px 8px}
.tls{display:flex;gap:8px;padding:2px 8px 18px}.tls i{width:12px;height:12px;border-radius:50%;background:#FF5F57}.tls i+i{background:#FEBC2E}.tls i+i+i{background:#28C840}
.brand{display:flex;align-items:center;gap:10px;padding:0 10px 14px;font:500 13.5px Inter}
.sg{display:flex;align-items:center;gap:7px;padding:6px 10px 3px;font:500 11px Inter;color:var(--t2)}.sg i{width:6px;height:6px;border-radius:50%}
.si{height:30px;border-radius:8px;display:flex;align-items:center;gap:10px;padding:0 10px 0 16px;font:400 13px Inter;color:rgba(255,255,255,.9)}
.si.on{background:rgba(255,255,255,.09);color:var(--acc)}
.si .k{margin-left:auto;font:400 10.5px Inter;color:var(--t3)}
.sbb{position:absolute;left:8px;right:8px;bottom:10px}
.pg{position:absolute;left:210px;top:0;right:0;bottom:0}
.ph{height:50px;display:flex;align-items:center;gap:12px;padding:0 24px;border-bottom:1px solid rgba(255,255,255,.07);font:500 15px Inter}
.ph .sub{font:400 12.5px Inter;color:var(--t2)}
.ac{position:absolute;background:var(--card);border:1px solid var(--edge);border-radius:12px;padding:14px 15px}
.ac .h{font:500 13.5px Inter;display:flex;align-items:center;gap:6px}
.ac .r{display:flex;justify-content:space-between;font:400 12px Inter;color:var(--t2);margin-top:8px}.ac .r b{font-weight:500;color:#fff}
.seg{display:flex;padding:3px;border-radius:9px;background:rgba(255,255,255,.06);border:1px solid rgba(255,255,255,.07);font:400 12.5px Inter}
.seg span{padding:6px 16px;border-radius:7px;color:rgba(255,255,255,.75)}.seg span.on{background:rgba(255,255,255,.12);color:#fff;box-shadow:0 1px 0 rgba(0,0,0,.3)}
.btn{height:42px;padding:0 24px;border-radius:9px;background:var(--act);color:#160A20;font:500 15px Inter;display:inline-flex;align-items:center;justify-content:center;box-shadow:0 1px 0 rgba(0,0,0,.35),0 10px 18px -10px rgba(255,95,162,.6)}
.qbtn{height:30px;padding:0 13px;border-radius:8px;background:rgba(255,255,255,.08);border:1px solid rgba(255,255,255,.1);font:400 12.5px Inter;display:inline-flex;align-items:center}
.bar{height:5px;border-radius:3px;background:rgba(255,255,255,.1);overflow:hidden;position:relative}
.bar i{position:absolute;left:0;top:0;bottom:0;border-radius:3px}
/* editor + user windows */
.win{border-radius:12px;overflow:hidden;box-shadow:0 0 0 1px rgba(255,255,255,.14),0 40px 100px rgba(5,3,20,.7)}
.win .tb{height:44px;display:flex;align-items:center;gap:8px;padding:0 16px;font:600 15px Inter}
.tl{width:12px;height:12px;border-radius:50%;background:#FF5F57}.tl.y{background:#FEBC2E}.tl.g{background:#28C840}
.notes{background:#F6F3EC;color:#1d1d1b}
.notes .tb{background:#ECE7DD;color:#77736a}
.sel{position:absolute;border:2px solid #fff;outline:1px solid rgba(0,0,0,.7);box-shadow:0 0 0 9999px rgba(8,6,28,.45)}
.sel .dim{white-space:nowrap;position:absolute;right:-2px;bottom:-38px;background:rgba(10,8,30,.85);color:#fff;font:500 15px Inter;padding:5px 10px;border-radius:7px}
.flash{position:absolute;inset:0;background:#fff;pointer-events:none}
.etb{height:52px;display:flex;align-items:center;gap:4px;padding:0 12px;background:rgba(4,2,14,.35);border-bottom:1px solid rgba(255,255,255,.07)}
.etb .ti{width:36px;height:34px;border-radius:8px;display:flex;align-items:center;justify-content:center}
.etb .ti.on{background:rgba(255,154,77,.18)}
.eop{height:38px;display:flex;align-items:center;gap:14px;padding:0 16px;background:rgba(4,2,14,.25);border-bottom:1px solid rgba(255,255,255,.06);font:500 13.5px Inter}
.eop i{width:16px;height:16px;border-radius:50%}
.clip-row{height:58px;display:flex;align-items:center;gap:14px;padding:0 14px;border-radius:8px;font:400 14px Inter;position:absolute;left:16px;right:16px}
.clip-row .th{width:36px;height:36px;border-radius:7px;flex:none;display:flex;align-items:center;justify-content:center;overflow:hidden;background:rgba(255,255,255,.07)}
.clip-row .k{margin-left:auto;font:400 12px Inter;color:var(--t3)}
.clip-row.on{background:rgba(255,255,255,.09)}
.pill{position:absolute;height:54px;padding:0 18px 0 14px;border-radius:27px;background:#151032;border:1.5px solid rgba(255,154,77,.6);display:flex;align-items:center;gap:12px;box-shadow:0 14px 40px rgba(0,0,0,.6)}
.wave{display:flex;gap:4px;align-items:center;height:30px}
.wave i{width:5px;border-radius:3px;background:var(--cr)}
.caret{display:inline-block;width:3px;height:1.1em;background:#2b6cff;vertical-align:-.15em;margin-left:2px}
.mc{position:absolute;width:420px;height:230px;border-radius:16px;background:var(--sky);border:1px solid rgba(255,255,255,.14);box-shadow:0 30px 70px rgba(5,3,20,.6);padding:22px 24px;overflow:hidden}
.mc .h{font:600 20px Inter;display:flex;align-items:center;gap:10px}
.mc .in{position:absolute;inset:0;background:rgba(38,32,50,.35)}
`;

export const body = `
<div id="stage">
  <div id="desk"></div><div id="shade"></div><div id="glow"></div>
  <div id="mb" class="menubar">
    <svg width="22" height="26" viewBox="0 0 17 20"><path fill="#fff" d="M14.100 10.600c0-2.300 1.900-3.400 2-3.500-1.100-1.600-2.800-1.800-3.400-1.800-1.400-.1-2.800.9-3.500.9-.7 0-1.800-.9-3-.8C4.600 5.400 3.200 6.300 2.400 7.700.7 10.600 2 15 3.600 17.400c.8 1.200 1.800 2.500 3 2.400 1.200 0 1.700-.8 3.100-.8 1.500 0 1.900.8 3.100.8 1.300 0 2.100-1.200 2.900-2.400.9-1.300 1.300-2.600 1.300-2.700-.1 0-2.600-1-2.600-4.100zM11.800 3.800c.6-.8 1.100-1.900 1-3-.9 0-2.100.6-2.700 1.400-.6.700-1.100 1.800-1 2.800 1 .1 2.100-.5 2.700-1.200z"/></svg>
    <b>Finder</b><span>File</span><span>Edit</span><span>View</span><span>Go</span><span>Window</span>
    <span class="sp"></span><span id="slot" style="width:44px;height:40px;border-radius:8px;display:flex;align-items:center;justify-content:center"><svg id="sloticon" viewBox="0 0 100 100" width="40" height="40" style="overflow:visible"></svg></span>
    <svg width="28" height="24" viewBox="0 -2.500 24 20.500" style="overflow:visible"><path d="M12 15.500l2.600-3.100a4 4 0 00-5.200 0zM6.300 9.400a8.400 8.400 0 0111.400 0l1.700-2a11 11 0 00-14.800 0zM2.400 4.900a14 14 0 0119.200 0l1.700-2a16.500 16.500 0 00-22.600 0z" fill="#fff"/></svg>
    <svg width="40" height="20" viewBox="0 0 34 16"><rect x="1" y="1" width="28" height="14" rx="4" fill="none" stroke="#fff" stroke-opacity=".5" stroke-width="1.500"/><rect x="3.500" y="3.500" width="18" height="9" rx="2" fill="#fff"/></svg>
    <span>Sat 19:42</span>
  </div>

  <div class="w win notes" id="notesw" style="width:820px;height:560px">
    <div class="tb"><i class="tl"></i><i class="tl y"></i><i class="tl g"></i><span style="margin-left:14px">Notes</span></div>
    <div style="padding:34px 40px;font:400 22px/1.55 Inter">
      <div style="font:700 32px 'Inter Display';margin-bottom:14px">Weekend plan</div>
      <div>☐ Book a table for Saturday</div><div>☐ Send the beach photos to Mom</div><div>☑ Clean up the Mac</div>
      <div style="margin-top:18px;color:#6b675e">Email: roman@cmdmeow.com</div><div style="color:#6b675e">Flight: Sun 10:40</div>
    </div>
  </div>
  <div class="sel" id="sel" style="display:none"><span class="dim num" id="seldim"></span></div>
  <div class="flash" id="flash" style="opacity:0"></div>

  <!-- screenshot editor -->
  <div class="w" id="markw" style="width:880px;height:660px">
    <div class="win abs" style="width:880px;height:660px;background:var(--sky)">
      <div class="tb" style="color:rgba(255,255,255,.8);font-weight:500"><i class="tl"></i><i class="tl y"></i><i class="tl g"></i><span style="margin-left:14px">Screenshot 19:42</span></div>
      <div class="etb" id="tools"></div>
      <div class="eop"><span id="opname">Arrow</span><span id="opsw" style="display:flex;gap:8px"><i style="background:#FF6A3D;box-shadow:0 0 0 2px #fff"></i><i style="background:#FFD23F"></i><i style="background:#3A7BFF"></i><i style="background:#fff"></i></span></div>
      <div class="abs" style="top:134px;left:0;width:880px;height:526px;background:rgba(4,2,14,.35)">
        <div id="frame" class="abs" style="width:880px;height:526px"></div>
        <div id="shot" class="abs notes" style="overflow:hidden;font:400 21px/1.55 Inter;color:#1d1d1b">
          <div style="padding:26px 32px">
            <div style="font:700 28px 'Inter Display';margin-bottom:10px">Weekend plan</div>
            <div>☐ Book a table for Saturday</div><div>☐ Send the beach photos to Mom</div>
            <div style="margin-top:14px;color:#6b675e">Email: <span id="mail" style="position:relative">roman@cmdmeow.com<i id="blur" class="abs" style="height:100%;backdrop-filter:blur(7px);background:rgba(200,200,190,.35);border-radius:4px"></i></span></div>
          </div>
          <svg class="abs" style="overflow:visible" width="10" height="10"><path id="arrp" fill="none" stroke="#FF6A3D" stroke-width="7" stroke-linecap="round"/><path id="arrh" fill="none" stroke="#FF6A3D" stroke-width="7" stroke-linecap="round" stroke-linejoin="round"/></svg>
          <div id="txt" class="abs" style="left:470px;top:58px;font:800 34px Inter;color:#FF6A3D;transform-origin:left center">today!</div>
        </div>
      </div>
    </div>
  </div>

  <!-- record (an everyday page being recorded) -->
  <div class="w" id="recw" style="width:900px;height:600px">
    <div class="win abs" style="width:900px;height:600px;background:#f4f4f2;color:#1d1d1b">
      <div class="tb" style="background:#e9e9e6;color:#77736a"><i class="tl"></i><i class="tl y"></i><i class="tl g"></i><span style="margin-left:14px">Health · Activity</span></div>
      <div style="padding:30px 44px">
        <div style="font:700 34px 'Inter Display'">Steps this week</div>
        <div id="bars" style="display:flex;gap:26px;align-items:flex-end;height:330px;margin-top:30px"></div>
        <div style="display:flex;gap:26px;margin-top:10px;font:500 16px Inter;color:#8a877f">${['Mon','Tue','Wed','Thu','Fri','Sat','Sun'].map(d=>`<span style="flex:1;text-align:center">${d}</span>`).join('')}</div>
      </div>
    </div>
    <div id="ripples"></div>
    <div id="recpill" class="abs" style="left:300px;top:-34px;height:64px;padding:0 10px 0 22px;border-radius:32px;background:#151032;border:1.5px solid rgba(255,255,255,.14);display:flex;align-items:center;gap:16px;font:600 24px Inter;box-shadow:0 16px 40px rgba(0,0,0,.6);color:#fff">
      <i id="recdot" style="width:16px;height:16px;border-radius:50%;background:#FF453A"></i><span id="rectime" class="num">0:00</span>
      <span id="stopb" style="height:46px;padding:0 18px;border-radius:23px;background:rgba(255,255,255,.1);display:flex;align-items:center;gap:10px;font:600 20px Inter"><i style="width:14px;height:14px;border-radius:3px;background:#FF453A"></i>Stop</span>
    </div>
    <div id="cam" class="abs" style="left:700px;top:400px;width:210px;height:210px;border-radius:50%;background:radial-gradient(circle at 50% 35%,#5E1D57,#1A1248);border:4px solid #fff;box-shadow:0 20px 50px rgba(0,0,0,.6);overflow:hidden"><svg id="camcat" class="abs" viewBox="0 0 100 100" style="left:25px;top:30px;width:160px;height:160px"></svg></div>
  </div>

  <!-- app window pages -->
  <div class="w" id="clipw" style="width:1180px;height:740px"><div class="aw"><div class="sb" id="sb-clip"></div><div class="pg">
    <div class="ph">Clipboard<span class="sub">10 items</span><span class="qbtn" style="margin-left:auto">Pause 15 min</span></div>
    <div id="cliplist" class="abs" style="top:60px;left:0;right:0;bottom:0;overflow:hidden"></div>
  </div></div></div>

  <div class="w" id="cleanw" style="width:1180px;height:740px"><div class="aw"><div class="sb" id="sb-clean"></div><div class="pg">
    <div class="ph">Cleanup<span class="sub" id="cleansub"></span></div>
    <div class="abs" style="left:0;top:50px;width:355px;height:690px">
      <svg id="ring" class="abs" width="220" height="220" viewBox="0 0 220 220" style="left:75px;top:185px"></svg>
      <div class="abs" style="left:75px;top:185px;width:220px;height:220px;display:flex;align-items:center;justify-content:center;text-align:center"><div><div id="cleannum" class="num" style="font:400 26px Inter">61.0 GB</div><div id="cleanlbl" style="font:400 12px Inter;color:var(--t2);margin-top:2px">free of 271 GB</div></div></div>
      <div class="abs" style="left:0;width:370px;top:418px;text-align:center;font:400 13px Inter;color:rgba(255,255,255,.85)">▭ Macintosh HD</div>
      <div class="abs" style="left:85px;top:459px"><span class="btn" id="cleanbtn" style="width:200px">Scan</span></div>
    </div>
    <div class="abs" style="left:353px;top:198px;width:1px;height:392px;background:rgba(255,255,255,.08)"></div>
    <div class="abs" style="left:402px;top:162px;width:502px">
      <div style="display:flex;justify-content:space-between;font:400 11.5px Inter;color:var(--t2);padding-bottom:12px"><span id="lastscan">Last scan 2 days ago</span><span id="cleantot" class="num">23.1 GB</span></div>
      <div id="legend"></div>
    </div>
  </div></div></div>

  <div class="w" id="healthw" style="width:1180px;height:740px"><div class="aw"><div class="sb" id="sb-health"></div><div class="pg">
    <div class="ph">Mac Health<span class="seg" style="margin-left:auto"><span class="on">Overview</span><span>Settings</span></span></div>
    <div id="hgrid" class="abs" style="left:0;top:50px;right:0;bottom:0"></div>
  </div></div></div>

  <div class="w" id="aiw" style="width:1180px;height:740px"><div class="aw"><div class="sb" id="sb-ai"></div><div class="pg">
    <div class="ph">AI Usage<span class="sub">Claude Max · Updated 19:42</span><span class="seg" style="margin-left:auto" id="aitabs"></span></div>
    <div id="aigrid" class="abs" style="left:0;top:50px;right:0;bottom:0"></div>
  </div></div></div>

  <div class="mc" id="mc0" style="display:none"></div>
  <div class="mc" id="mc1" style="display:none"></div>
  <div class="mc" id="mc2" style="display:none"></div>
  <div class="mc" id="mc3" style="display:none"></div>

  <!-- messenger for dictation -->
  <div class="w" id="sayw" style="width:820px;height:600px">
    <div class="win abs" style="width:820px;height:600px;background:#F4F5F7;color:#1d1d1b">
      <div class="tb" style="background:#E9EAEE;color:#6d6f76"><i class="tl"></i><i class="tl y"></i><i class="tl g"></i><span style="margin-left:14px;display:flex;align-items:center;gap:10px"><span style="width:28px;height:28px;border-radius:50%;background:#FF9A4D;color:#fff;display:inline-flex;align-items:center;justify-content:center;font:700 14px Inter">R</span>Roman Dmitrievich</span></div>
      <div class="abs" style="left:32px;top:84px;max-width:560px;padding:14px 20px;border-radius:22px 22px 22px 6px;background:#fff;font:400 24px/1.4 Inter;box-shadow:0 1px 2px rgba(0,0,0,.06)">Did you send the beach photos? 👀<div style="font:500 14px Inter;color:#9a9ca3;margin-top:4px">19:41</div></div>
      <div id="outb" class="abs" style="left:auto;right:32px;top:210px;max-width:560px;padding:14px 20px;border-radius:22px 22px 6px 22px;background:#3A7BFF;color:#fff;font:400 24px/1.4 Inter;transform-origin:right bottom"><span id="outt"></span><div style="font:500 14px Inter;color:rgba(255,255,255,.7);margin-top:4px;text-align:right">19:42 ✓✓</div></div>
      <div class="abs" style="top:auto;left:24px;right:24px;bottom:22px;min-height:64px;border-radius:32px;background:#fff;border:1.5px solid #dcdee3;display:flex;align-items:center;padding:0 24px;font:400 24px/1.4 Inter"><span id="ph" style="color:#a7a9af;position:absolute;left:24px">Message</span><span id="saytext"></span><span class="caret" id="caret"></span></div>
    </div>
    <div class="pill" id="pill"><div class="wave" id="wave"></div><span style="font:600 18px Inter;color:rgba(255,255,255,.75)">⌥D</span></div>
  </div>

  <div id="endwm" class="abs" style="display:none;font:700 60px 'Inter Display';letter-spacing:.3em">CMD+MEOW</div>
  <div id="endurl" class="abs" style="display:none;font:500 32px Inter;color:rgba(255,255,255,.75)">cmdmeow.com</div>
  <div id="enddl" class="abs btn" style="display:none;height:72px;font-size:26px;padding:0 34px;border-radius:16px">Download for macOS</div>

  <div id="panel" class="pnl abs" style="z-index:8"></div>
  <div id="lab"><div id="ttl"></div><div id="keys"></div></div>
  <svg id="cur" class="cursor" viewBox="0 0 34 44"><path d="M3 2v34l9-8 6 13 6-3-6-12h12z" fill="#000" stroke="#fff" stroke-width="2.500" stroke-linejoin="round"/></svg>
  <svg id="cat" class="abs" viewBox="0 0 100 100" style="overflow:visible;z-index:15"></svg>
</div>
`;
