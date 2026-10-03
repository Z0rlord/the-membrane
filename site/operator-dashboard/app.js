'use strict';
// SIMULATED LIVE TRAFFIC. Every decision below is invented in this browser tab.
// No gate is read, no request is sent, nothing leaves the page.
const $ = id => document.getElementById(id);
// Older WebKit has no replaceChildren; keep text rendering independent of canvas.
function replace(el, ...items){while(el.firstChild)el.removeChild(el.firstChild);items.forEach(item=>el.appendChild(typeof item==='string'?document.createTextNode(item):item));}
function cell(parent, tag, text, cls) { const el=document.createElement(tag); el.textContent=text; if(cls)el.className=cls; parent.append(el); return el; }

// ---------- Invented world ----------
const POLICY={permitted_channels:['local-llm'],forbidden_exports:['cloud-telemetry','training-retention'],model_allowlist:['local-model'],github_repo_allowlist:['acme/pilot'],delta_t_secs:300};
const IDENTITIES=[
  {name:'billing-agent',id:'agt_7f3a9c21e0b4',scope:{chat:'chat:billing',tool:'tools:ledger-read'}},
  {name:'support-triage',id:'agt_2c81d4f6a9e7',scope:{chat:'chat:support',tool:'tools:ticket-update'}},
  {name:'code-review-bot',id:'agt_91be05a7c3d2',scope:{chat:'chat:review',tool:'tools:github-read'}},
  {name:'data-sync-job',id:'agt_5d07e8b14f6a',scope:{chat:'chat:ops',tool:'tools:warehouse-sync'}},
  {name:'research-agent',id:'agt_c4a2f93b7081',scope:{chat:'chat:research',tool:'tools:docs-search'}},
  {name:'release-bot',id:'agt_08e6b2d5a1c9',scope:{chat:'chat:release',tool:'tools:github-write'},noisy:true}
];
const LANES=['chat','tool'];
const DENY_RULES={chat:[['model_allowlist',.4],['iac_signature',.3],['permitted_channels',.2],['delta_t',.1]],tool:[['tool_allowlist',.5],['iac_signature',.2],['github_repo_allowlist',.15],['forbidden_exports',.15]]};
const ALLOW_RULE='all_authorization_checks_passed';

let seed=(Number((location.hash.match(/seed=(\d+)/)||[])[1])||Date.now())>>>0;
function rnd(){seed=(seed+0x6D2B79F5)>>>0;let t=seed;t=Math.imul(t^t>>>15,t|1);t^=t+Math.imul(t^t>>>7,t|61);return((t^t>>>14)>>>0)/4294967296;}
function weighted(list){let r=rnd(),acc=0;for(const [v,w] of list){acc+=w;if(r<acc)return v;}return list[0][0];}

function makeDecision(ts,identIndex,opts){
  const ident=IDENTITIES[identIndex];opts=opts||{};
  const action=opts.action||(rnd()<.62?'chat':'tool');
  const p=ident.noisy&&action==='tool'?.18:.045;
  const deny=opts.deny===true||(opts.deny!==false&&rnd()<p);
  const rule=deny?(opts.rule||weighted(DENY_RULES[action])):ALLOW_RULE;
  return{timestamp:ts,outcome:deny?'deny':'allow',rule,action,agent:ident.id,name:ident.name,ident:identIndex,lane:LANES.indexOf(action),scope:deny&&rule==='iac_signature'?null:ident.scope[action]};
}

// ---------- Rolling state ----------
const BUFFER_MAX=120, ROWS_SHOWN=30, WINDOW_MS=60000;
let buffer=[], total=0, allowed=0, denied=0;
const laneCount=[{allow:0,deny:0},{allow:0,deny:0}];
let burst={n:0,ident:0,probe:false};

function nextGap(){
  if(burst.n>0){burst.n--;return .22+rnd()*.45;}
  const r=rnd();
  if(r<.015){burst={n:2+Math.floor(rnd()*3),ident:Math.floor(rnd()*IDENTITIES.length),probe:true};return .9+rnd();}
  if(r<.12){burst={n:2+Math.floor(rnd()*3),ident:Math.floor(rnd()*IDENTITIES.length),probe:false};return .8+rnd();}
  return Math.min(8,.9-Math.log(1-rnd())*2);
}
function generate(ts){
  if(burst.n>0&&burst.probe)return makeDecision(ts,burst.ident,{action:'tool',deny:true,rule:rnd()<.8?'tool_allowlist':'forbidden_exports'});
  if(burst.n>0)return makeDecision(ts,burst.ident);
  return makeDecision(ts,Math.floor(rnd()*IDENTITIES.length));
}
function record(d,pre){
  buffer.unshift(d);if(buffer.length>BUFFER_MAX)buffer.pop();
  if(pre)return;
  total++;if(d.outcome==='allow'){allowed++;laneCount[d.lane].allow++;}else{denied++;laneCount[d.lane].deny++;}
}

// ---------- Rendering ----------
function renderRows(fresh){
  const body=$('rows');replace(body);
  const q=$('search').value.toLowerCase(),filter=$('filter').value;
  const rows=buffer.filter(d=>(filter==='all'||d.outcome===filter)&&[d.rule,d.scope,d.agent,d.action,d.name].some(v=>(v||'').toLowerCase().includes(q))).slice(0,ROWS_SHOWN);
  if(!rows.length){const tr=cell(body,'tr','');const td=cell(tr,'td','No simulated decisions match this view.','empty');td.colSpan=5;return;}
  rows.forEach((d,i)=>{const tr=cell(body,'tr','');if(fresh&&i===0&&d===buffer[0])tr.className='fresh is-'+d.outcome;
    cell(tr,'td',new Date(d.timestamp).toLocaleTimeString([],{hour12:false}));
    const out=cell(tr,'td','');cell(out,'span',d.outcome.toUpperCase(),'badge '+d.outcome);
    cell(tr,'td',d.rule);cell(tr,'td',d.action);
    const who=cell(tr,'td','');const identity=cell(who,'code',d.agent.slice(0,12)+'…','identity');identity.title=d.name+' (simulated)';cell(who,'code',d.scope||'Scope not verified');});
}
function renderStats(now){
  const win=buffer.filter(d=>now-d.timestamp<=WINDOW_MS);
  const wd=win.filter(d=>d.outcome==='deny').length;
  $('count').textContent=buffer.length;$('total').textContent=total;
  $('rate').textContent=win.length?(wd/win.length*100).toFixed(1)+'%':'N/A';
  $('counts').textContent=wd+' deny / '+(win.length-wd)+' allow in last 60s';
  $('cp').textContent='Synthetic stream, about '+win.length+' decisions/min';
  $('chip-allow').textContent=allowed;$('chip-deny').textContent=denied;
  LANES.forEach((n,i)=>{const c=laneCount[i],N=n.toUpperCase();
    $('lane-allow-'+i).textContent=N+' · '+c.allow+' crossed';$('lane-deny-'+i).textContent=c.deny+' stopped';$('gc-'+i).textContent=c.allow+c.deny;
    $('mlane-allow-'+i).textContent=c.allow+' crossed';$('mlane-deny-'+i).textContent=c.deny+' stopped';$('mgc-'+i).textContent=c.allow+c.deny;});
}
function renderTicker(d){
  const t=$('ticker');t.dataset.outcome=d.outcome;replace(t);
  cell(t,'b',d.outcome.toUpperCase());t.append(' '+d.name+' · '+d.action+' · ');cell(t,'b',d.rule.replace('all_authorization_checks_passed','all checks passed'));
  t.append(d.outcome==='allow'?' · crossed (simulated)':' · stopped at the gate (simulated)');
}
function renderPolicy(){
  replace($('policy'));const names={permitted_channels:'Permitted channels',forbidden_exports:'Forbidden exports',model_allowlist:'Model allowlist',github_repo_allowlist:'GitHub repositories',delta_t_secs:'Checkpoint freshness'};
  Object.entries(names).forEach(([k,l])=>{cell($('policy'),'dt',l);const v=POLICY[k];cell($('policy'),'dd',Array.isArray(v)?v.join(', '):v+' seconds');});
}
$('filter').addEventListener('change',()=>renderRows(false));$('search').addEventListener('input',()=>renderRows(false));
renderPolicy();

// ---------- Motion: missile-command style, capped particles ----------
const ACC='#7edfc0', DENY='#ffb3a4', INK='#eff4fb';
const GEO={
  desktop:{w:1060,h:340,laneY:[130,228],launch:()=>[498,300],origin:i=>[241,104+i*34],gate:l=>[410,l],end:l=>[800,l]},
  mobile:{w:360,h:400,laneX:[110,250],origin:i=>[40+i*56,88]}
};
const FLY=1.1, MAXP=12;
const fx=[...document.querySelectorAll('.fx')].map((canvas,i)=>({canvas,ctx:(()=>{try{return canvas.getContext('2d');}catch(e){return null;}})(),mobile:!!i,scale:1}));
const reduce=window.matchMedia('(prefers-reduced-motion: reduce)');
const q=(p0,c,p1,u)=>{const m=1-u;return[m*m*p0[0]+2*m*u*c[0]+u*u*p1[0],m*m*p0[1]+2*m*u*c[1]+u*u*p1[1]];};
function path(f,d){
  if(!f.mobile){const G=GEO.desktop,y=G.laneY[d.lane],a=G.origin(d.ident),g=G.gate(y);
    return{a,g,e:G.end(y),c:[(a[0]+g[0])/2-20+d.j*30,Math.min(a[1],g[1])-25-d.k*45],launch:G.launch()};}
  const G=GEO.mobile,x=G.laneX[d.lane],a=G.origin(d.ident),g=[x,145];
  return{a,g,e:[x,301],c:[(a[0]+g[0])/2+d.j*30-15,a[1]+16+d.k*14],launch:[x,243]};
}
function size(f){
  const r=f.canvas.getBoundingClientRect();if(!r.width)return false;
  const dpr=Math.min(window.devicePixelRatio||1,f.mobile?1.5:2);
  const w=Math.round(r.width*dpr),h=Math.round(r.height*dpr);
  if(f.canvas.width!==w||f.canvas.height!==h){f.canvas.width=w;f.canvas.height=h;}
  f.scale=w/(f.mobile?GEO.mobile.w:GEO.desktop.w);return true;
}
function trail(ctx,p0,c,p1,u0,u1,col,alpha,width){
  ctx.strokeStyle=col;ctx.lineWidth=width;ctx.globalAlpha=alpha;ctx.lineCap='round';
  ctx.beginPath();const n=12;for(let k=0;k<=n;k++){const p=q(p0,c,p1,u0+(u1-u0)*k/n);k?ctx.lineTo(p[0],p[1]):ctx.moveTo(p[0],p[1]);}ctx.stroke();ctx.globalAlpha=1;
}
function ring(ctx,x,y,r,col,alpha,width){ctx.strokeStyle=col;ctx.globalAlpha=Math.max(0,alpha);ctx.lineWidth=width;ctx.beginPath();ctx.arc(x,y,r,0,6.2832);ctx.stroke();ctx.globalAlpha=1;}
function drawParticle(ctx,L,deny,tt){
  const col=deny?DENY:ACC;
  if(tt<FLY){
    const u=tt/FLY;trail(ctx,L.a,L.c,L.g,Math.max(0,u-.4),u,col,.8,2);
    const p=q(L.a,L.c,L.g,u);ctx.fillStyle=col;ctx.beginPath();ctx.arc(p[0],p[1],4,0,6.2832);ctx.fill();ring(ctx,p[0],p[1],8,col,.35,1);
    if(deny&&tt>.35){
      const v=(tt-.35)/(FLY-.35),mid=[(L.launch[0]+L.g[0])/2+(L.mob?20:0),Math.min(L.launch[1],L.g[1])-30];
      trail(ctx,L.launch,mid,L.g,Math.max(0,v-.35),v,INK,.85,1.5);
      const s=q(L.launch,mid,L.g,v);ctx.fillStyle=INK;ctx.fillRect(s[0]-2,s[1]-2,4,4);
      ctx.strokeStyle=INK;ctx.globalAlpha=.7;ctx.lineWidth=1.2;ctx.beginPath();ctx.moveTo(L.g[0]-5,L.g[1]);ctx.lineTo(L.g[0]+5,L.g[1]);ctx.moveTo(L.g[0],L.g[1]-5);ctx.lineTo(L.g[0],L.g[1]+5);ctx.stroke();ctx.globalAlpha=1;
    }
    return;
  }
  const a=tt-FLY;
  if(deny){
    if(a<.8)trail(ctx,L.a,L.c,L.g,0,1,DENY,.45*(1-a/.8),1.5);
    if(a<.7){const k=a/.7;ctx.fillStyle=DENY;ctx.globalAlpha=(1-k)*.55;ctx.beginPath();ctx.arc(L.g[0],L.g[1],24*Math.sqrt(k)+4,0,6.2832);ctx.fill();ctx.globalAlpha=1;
      ring(ctx,L.g[0],L.g[1],4+28*k,DENY,1-k,2.5);ring(ctx,L.g[0],L.g[1],2+15*k,INK,1-k,1.5);
      for(let s=0;s<8;s++){const ang=s*.785+.3,r1=8+24*k,r2=r1+7*(1-k);ctx.strokeStyle=DENY;ctx.globalAlpha=1-k;ctx.lineWidth=1.5;ctx.beginPath();ctx.moveTo(L.g[0]+Math.cos(ang)*r1,L.g[1]+Math.sin(ang)*r1);ctx.lineTo(L.g[0]+Math.cos(ang)*r2,L.g[1]+Math.sin(ang)*r2);ctx.stroke();}ctx.globalAlpha=1;}
  }else{
    if(a<.45)ring(ctx,L.g[0],L.g[1],6+12*(a/.45),ACC,1-a/.45,2);
    const run=.75,u=Math.min(1,a/run),p=[L.g[0]+(L.e[0]-L.g[0])*u,L.g[1]+(L.e[1]-L.g[1])*u];
    if(a<run+.9){trail(ctx,L.a,L.c,L.g,0,1,ACC,Math.max(0,.4*(1-a/(run+.9))),1.5);
      ctx.strokeStyle=ACC;ctx.lineWidth=2;ctx.globalAlpha=a<run?.8:.8*(1-(a-run)/.9);ctx.beginPath();ctx.moveTo(L.g[0],L.g[1]);ctx.lineTo(p[0],p[1]);ctx.stroke();ctx.globalAlpha=1;}
    if(a<run){ctx.fillStyle=ACC;ctx.beginPath();ctx.arc(p[0],p[1],4,0,6.2832);ctx.fill();}
    else{const k=Math.min(1,(a-run)/.6);ring(ctx,L.e[0],L.e[1],5+14*k,ACC,1-k,2);}
  }
}
let particles=[], playing=false, raf=0, timer=0, visible=true, paused=reduce.matches, canvasOK=fx.every(f=>!!f.ctx);
function life(p){return p.deny?FLY+.8:FLY+.75+.9;}
function frame(now){
  raf=0;
  particles=particles.filter(p=>(now-p.t0)/1000<life(p));
  fx.forEach(f=>{
    try{
    if(!f.ctx||!size(f))return;
    const ctx=f.ctx;ctx.setTransform(1,0,0,1,0,0);ctx.clearRect(0,0,f.canvas.width,f.canvas.height);ctx.setTransform(f.scale,0,0,f.scale,0,0);
    particles.forEach(p=>{const L=path(f,p);L.mob=f.mobile;drawParticle(ctx,L,p.deny,Math.max(0,(now-p.t0)/1000));});
    }catch(e){canvasOK=false;particles=[];label();}
  });
  if(particles.length&&playing&&visible)raf=requestAnimationFrame(frame);
}
function spawn(d){
  if(reduce.matches||!canvasOK||!playing||!visible||document.hidden||particles.length>=MAXP)return;
  particles.push({lane:d.lane,ident:d.ident,deny:d.outcome==='deny',j:rnd(),k:rnd(),t0:performance.now()});
  if(!raf)raf=requestAnimationFrame(frame);
}
function clearFx(){particles=[];if(raf){cancelAnimationFrame(raf);raf=0;}fx.forEach(f=>{if(f.ctx)f.ctx.clearRect(0,0,f.canvas.width,f.canvas.height);});}

// ---------- Scheduler ----------
// Lifecycle events restart the timer, not just a boolean. Safari can suspend a tab
// (including back-forward cache) without delivering a pending timeout on return.
function tick(){
  timer=0;if(!playing||document.hidden)return;
  const now=Date.now(),d=generate(now);record(d);renderStats(now);renderRows(true);renderTicker(d);
  // A canvas problem must never freeze the decision log or its timer.
  try{spawn(d);}catch(e){canvasOK=false;clearFx();label();}
  timer=setTimeout(tick,nextGap()*1000);
}
function setPlaying(on){
  if(timer)clearTimeout(timer);timer=0;
  playing=on&&!document.hidden;
  if(playing)timer=setTimeout(tick,400);else clearFx();
}
function setStatus(){
  const staticMode=paused&&reduce.matches;
  $('status').textContent=staticMode?'SIMULATED / STATIC':paused?'SIMULATED / PAUSED':document.hidden?'SIMULATED / BACKGROUND':reduce.matches||!canvasOK?'SIMULATED / TEXT ONLY':'SIMULATED / RUNNING';
  $('status').className='badge idle';
  $('updated').textContent=staticMode?'System Reduce Motion is on. Static preview, not a loading failure. Press Start text stream to update without animation.':paused?'Simulation paused. Press Play to resume.':reduce.matches?'Reduce Motion is on. Decisions update without animation.':!canvasOK?'Map animation unavailable. The simulated log still updates.':'Synthetic stream, not a live observation';
  $('liveness').textContent='Simulated';
}
function label(){
  const b=$('replay'),n=$('replay-note');setStatus();b.disabled=false;
  if(paused){b.textContent=reduce.matches?'Start text stream':'Play ▶';n.textContent=reduce.matches?'Your system Reduce Motion setting keeps this preview still. Start text stream updates the simulated decisions and counters, with no map animation. No requests are sent.':'Paused. The simulated log is frozen; nothing was sent.';}
  else{b.textContent='Pause ❚❚';n.textContent=reduce.matches?'Text-only simulation running. Reduce Motion is respected: the map stays still while decisions and counters update. No requests are sent.':'Simulated live traffic: a few decisions a minute to a few a second in bursts, invented in this page. Allowed calls cross the gate; denied calls are intercepted at it. No agent is running and no requests are sent.';}
}
function backfill(){
  const now=Date.now();let t=now-150000;
  while(t<now-2500){record(generate(t),true);t+=nextGap()*1000;}
  burst={n:0,ident:0,probe:false};
  renderStats(now);renderRows(false);if(buffer[0])renderTicker(buffer[0]);
}
$('replay').addEventListener('click',()=>{paused=!paused;setPlaying(!paused);label();});
if('IntersectionObserver' in window){new IntersectionObserver(es=>{visible=es[es.length-1].isIntersecting;if(!visible)clearFx();}).observe(document.querySelector('.boundary-map'));}
function resume(){setPlaying(!paused);renderStats(Date.now());label();}
document.addEventListener('visibilitychange',resume);
window.addEventListener('pageshow',resume);
window.addEventListener('focus',resume);
window.addEventListener('pagehide',()=>setPlaying(false));
(reduce.addEventListener?reduce.addEventListener.bind(reduce,'change'):reduce.addListener.bind(reduce))(()=>{if(reduce.matches){paused=true;clearFx();}resume();});
$('alert').hidden=false;
backfill();label();setPlaying(!paused);
