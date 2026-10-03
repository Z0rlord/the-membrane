'use strict';
const $ = id => document.getElementById(id);
let snapshot = null;
function cell(parent, tag, text, cls) { const el=document.createElement(tag); el.textContent=text; if(cls)el.className=cls; parent.append(el); return el; }
function renderRows() {
  $('rows').replaceChildren();
  const q=$('search').value.toLowerCase(), filter=$('filter').value;
  const rows=(snapshot?.decisions||[]).filter(d=>(filter==='all'||d.outcome===filter)&&[d.rule,d.scope,d.agent,d.action].some(v=>(v||'').toLowerCase().includes(q)));
  if(!rows.length){const tr=cell($('rows'),'tr','');const td=cell(tr,'td',snapshot?'No decisions match this view.':'No verified decisions yet.','empty');td.colSpan=5;return;}
  rows.forEach(d=>{const tr=cell($('rows'),'tr','');cell(tr,'td',new Date(d.timestamp*1000).toLocaleString());const out=cell(tr,'td','');cell(out,'span',d.outcome.toUpperCase(),'badge '+(d.outcome==='allow'?'allow':'deny'));cell(tr,'td',d.rule);cell(tr,'td',d.action);const who=cell(tr,'td','');const identity=cell(who,'code',d.agent?.slice(0,12)+'…','identity');identity.title=d.agent||'';cell(who,'code',d.scope||'Scope not verified');});
}
function clearUnknown(message) {
  snapshot=null; $('status').textContent='UNKNOWN';$('status').className='badge unknown';$('updated').textContent='No current verified snapshot';$('liveness').textContent='Unknown';$('cp').textContent='No checkpoint data';['count','rate','total'].forEach(id=>$(id).textContent='-');$('counts').textContent='Metrics unavailable';$('policy').replaceChildren();cell($('policy'),'dt','Source');cell($('policy'),'dd','Unavailable');$('alert').hidden=false;$('alert').textContent=message;renderRows();
}
function render(data) {
  snapshot=data;$('alert').hidden=true;$('status').textContent=data.status.toUpperCase();$('status').className='badge '+(['live','idle','degraded'].includes(data.status)?data.status:'unknown');$('updated').textContent='Observed '+new Date(data.observed_at*1000).toLocaleTimeString();$('liveness').textContent=data.status[0].toUpperCase()+data.status.slice(1);$('cp').textContent=data.last_cp_age_secs===null?'No active checkpoint':'Last checkpoint '+data.last_cp_age_secs+'s ago';$('count').textContent=data.retained;$('total').textContent=data.total_observed;$('rate').textContent=data.deny_rate===null?'N/A':(data.deny_rate*100).toFixed(1)+'%';$('counts').textContent=data.denied+' deny / '+data.allowed+' allow';
  $('policy').replaceChildren();const names={permitted_channels:'Permitted channels',forbidden_exports:'Forbidden exports',model_allowlist:'Model allowlist',github_repo_allowlist:'GitHub repositories',delta_t_secs:'Checkpoint freshness'};
  Object.entries(names).forEach(([key,label])=>{cell($('policy'),'dt',label);const v=data.policy[key];cell($('policy'),'dd',Array.isArray(v)?(v.join(', ')||'(empty: no grants)'):v+' seconds');});renderRows();
}

$('filter').addEventListener('change',renderRows);$('search').addEventListener('input',renderRows);
const fixture={schema_version:1,observed_at:1790998200,status:'idle',last_cp_age_secs:null,router_stale:false,degraded:false,audit_available:true,retained:3,total_observed:3,allowed:1,denied:2,deny_rate:2/3,policy:{permitted_channels:['local-llm'],forbidden_exports:['cloud-telemetry','training-retention'],model_allowlist:['local-model'],github_repo_allowlist:['acme/pilot'],delta_t_secs:300},decisions:[{timestamp:1790998200,outcome:'deny',rule:'iac_signature',action:'chat',agent:'synthetic-gate-identity',scope:null},{timestamp:1790998140,outcome:'deny',rule:'tool_allowlist',action:'tool',agent:'synthetic-gate-identity',scope:null},{timestamp:1790998080,outcome:'allow',rule:'all_authorization_checks_passed',action:'chat',agent:'synthetic-gate-identity',scope:'preview-scope'}]};
render(fixture);$('status').textContent='DEMO / IDLE';$('updated').textContent='Synthetic sample, not a live observation';$('alert').hidden=false;$('alert').textContent='Static preview with invented decisions. Not connected to a gate. No live status, credentials or external effects.';

// Visualize only the retained fixture. No destinations or live health are inferred.
let selectedDecision = 0;
function inspectDecision(index) {
  selectedDecision = index;
  const d = fixture.decisions[index];
  document.querySelectorAll('.decision-card').forEach((el, i) => el.setAttribute('aria-pressed', String(i === index)));
  const detail = $('map-detail');
  detail.replaceChildren();
  detail.dataset.outcome = d.outcome;
  cell(detail, 'code', d.rule);
  cell(detail, 'span', d.outcome === 'allow' ? ' - Allowed in this invented sample. The illustrative path crosses the gate; no external effect occurred.' : ' - Denied in this invented sample. The path stops at the gate; the action boundary is not reached.');
  cell(detail, 'span', ' Scope: ');
  cell(detail, 'code', d.scope || 'not verified');
}
fixture.decisions.forEach((d, index) => {
  const button = cell($('map-decisions'), 'button', '', 'decision-card');
  button.type = 'button';
  button.setAttribute('aria-controls', 'map-detail');
  cell(button, 'span', d.outcome.toUpperCase(), 'badge ' + d.outcome);
  cell(button, 'span', d.rule === 'all_authorization_checks_passed' ? 'All checks passed' : d.rule);
  button.addEventListener('click', () => inspectDecision(index));
});
inspectDecision(selectedDecision);

// Missile Command style motion. Canvas overlay, fixture decisions only, no requests.
const ACC='#7edfc0', DENY='#ffb3a4';
const GEO={
  desktop:{w:1060,h:340,launch:[498,300],lanes:[
    {out:'allow',a:[241,180],g:[410,113],e:[800,113],c:[320,98]},
    {out:'deny',a:[241,180],g:[410,188],c:[325,95]},
    {out:'deny',a:[241,180],g:[410,263],c:[345,150]}]},
  mobile:{w:360,h:400,launch:[180,243],lanes:[
    {out:'allow',a:[180,84],g:[62,145],e:[62,301],c:[40,62]},
    {out:'deny',a:[180,84],g:[180,145],c:[222,98]},
    {out:'deny',a:[180,84],g:[298,145],c:[322,62]}]}};
const STEP=1.5, FLY=1.1, CYCLE=6.6;
const fx=[...document.querySelectorAll('.fx')].map((canvas,i)=>({canvas,ctx:canvas.getContext('2d'),geo:i?GEO.mobile:GEO.desktop,scale:1}));
const reduce=window.matchMedia('(prefers-reduced-motion: reduce)');
const q=(p0,c,p1,u)=>{const m=1-u;return[m*m*p0[0]+2*m*u*c[0]+u*u*p1[0],m*m*p0[1]+2*m*u*c[1]+u*u*p1[1]];};
function size(f){
  const r=f.canvas.getBoundingClientRect();if(!r.width)return false;
  const dpr=Math.min(window.devicePixelRatio||1,f.geo===GEO.mobile?1.5:2);
  const w=Math.round(r.width*dpr),h=Math.round(r.height*dpr);
  if(f.canvas.width!==w||f.canvas.height!==h){f.canvas.width=w;f.canvas.height=h;}
  f.scale=w/f.geo.w;return true;
}
function trail(ctx,p0,c,p1,u0,u1,col,alpha,width){
  ctx.strokeStyle=col;ctx.lineWidth=width;ctx.globalAlpha=alpha;ctx.lineCap='round';
  ctx.beginPath();const n=14;for(let k=0;k<=n;k++){const p=q(p0,c,p1,u0+(u1-u0)*k/n);k?ctx.lineTo(p[0],p[1]):ctx.moveTo(p[0],p[1]);}ctx.stroke();ctx.globalAlpha=1;
}
function ring(ctx,x,y,r,col,alpha,width){ctx.strokeStyle=col;ctx.globalAlpha=Math.max(0,alpha);ctx.lineWidth=width;ctx.beginPath();ctx.arc(x,y,r,0,6.2832);ctx.stroke();ctx.globalAlpha=1;}
function draw(f,t){
  if(!size(f))return;const ctx=f.ctx,g=f.geo;
  ctx.setTransform(1,0,0,1,0,0);ctx.clearRect(0,0,f.canvas.width,f.canvas.height);ctx.setTransform(f.scale,0,0,f.scale,0,0);
  g.lanes.forEach((L,i)=>{
    const tt=t-i*STEP;if(tt<0)return;const col=L.out==='allow'?ACC:DENY;
    if(tt<FLY){
      const u=tt/FLY;trail(ctx,L.a,L.c,L.g,Math.max(0,u-.4),u,col,.8,2);
      const p=q(L.a,L.c,L.g,u);ctx.fillStyle=col;ctx.beginPath();ctx.arc(p[0],p[1],4,0,6.2832);ctx.fill();ring(ctx,p[0],p[1],8,col,.35,1);
      if(L.out==='deny'&&tt>.35){ // interceptor rises from the gate to meet the attempt
        const v=(tt-.35)/(FLY-.35),mid=[(g.launch[0]+L.g[0])/2,Math.min(g.launch[1],L.g[1])-30];
        trail(ctx,g.launch,mid,L.g,Math.max(0,v-.35),v,'#eff4fb',.85,1.5);
        const s=q(g.launch,mid,L.g,v);ctx.fillStyle='#eff4fb';ctx.fillRect(s[0]-2,s[1]-2,4,4);
        ctx.strokeStyle='#eff4fb';ctx.globalAlpha=.7;ctx.lineWidth=1.2;ctx.beginPath();ctx.moveTo(L.g[0]-5,L.g[1]);ctx.lineTo(L.g[0]+5,L.g[1]);ctx.moveTo(L.g[0],L.g[1]-5);ctx.lineTo(L.g[0],L.g[1]+5);ctx.stroke();ctx.globalAlpha=1;
      }
      return;
    }
    const a=tt-FLY;
    if(L.out==='deny'){
      if(a<.9)trail(ctx,L.a,L.c,L.g,0,1,DENY,.5*(1-a/.9),1.5);
      if(a<.7){const k=a/.7;ctx.fillStyle=DENY;ctx.globalAlpha=(1-k)*.55;ctx.beginPath();ctx.arc(L.g[0],L.g[1],26*Math.sqrt(k)+4,0,6.2832);ctx.fill();ctx.globalAlpha=1;
        ring(ctx,L.g[0],L.g[1],4+30*k,DENY,1-k,2.5);ring(ctx,L.g[0],L.g[1],2+16*k,'#eff4fb',1-k,1.5);
        for(let s=0;s<8;s++){const ang=s*.785+.3,r1=8+26*k,r2=r1+7*(1-k);ctx.strokeStyle=DENY;ctx.globalAlpha=1-k;ctx.lineWidth=1.5;ctx.beginPath();ctx.moveTo(L.g[0]+Math.cos(ang)*r1,L.g[1]+Math.sin(ang)*r1);ctx.lineTo(L.g[0]+Math.cos(ang)*r2,L.g[1]+Math.sin(ang)*r2);ctx.stroke();}ctx.globalAlpha=1;}
    }else{
      if(a<.45)ring(ctx,L.g[0],L.g[1],6+14*(a/.45),ACC,1-a/.45,2);
      const run=.75,u=Math.min(1,a/run);const p=[L.g[0]+(L.e[0]-L.g[0])*u,L.g[1]+(L.e[1]-L.g[1])*u];
      if(a<run+.9){trail(ctx,L.a,L.c,L.g,0,1,ACC,Math.max(0,.5*(1-a/(run+.9))),1.5);
        ctx.strokeStyle=ACC;ctx.lineWidth=2;ctx.globalAlpha=a<run?.8:.8*(1-(a-run)/.9);ctx.beginPath();ctx.moveTo(L.g[0]+(p[0]-L.g[0])*.0,L.g[1]+(p[1]-L.g[1])*.0);ctx.lineTo(p[0],p[1]);ctx.stroke();ctx.globalAlpha=1;}
      if(a<run){ctx.fillStyle=ACC;ctx.beginPath();ctx.arc(p[0],p[1],4,0,6.2832);ctx.fill();}
      else{const k=Math.min(1,(a-run)/.6);ring(ctx,L.e[0],L.e[1],5+16*k,ACC,1-k,2);}
    }
  });
}
let playing=false,raf=0,t0=0,visible=true,paused=false;
function frame(now){
  raf=0;if(!playing)return;draw(fx[0],((now-t0)/1000)%CYCLE);draw(fx[1],((now-t0)/1000)%CYCLE);
  raf=requestAnimationFrame(frame);
}
function setPlaying(on){
  const want=on&&visible&&!document.hidden;
  if(want&&!playing){playing=true;t0=performance.now();raf=requestAnimationFrame(frame);}
  if(!want&&playing){playing=false;if(raf)cancelAnimationFrame(raf);raf=0;fx.forEach(f=>f.ctx.clearRect(0,0,f.canvas.width,f.canvas.height));}
}
function label(){
  const b=$('replay'),n=$('replay-note');
  if(reduce.matches){b.textContent='Motion off';b.disabled=true;n.textContent='Reduced motion is on, so the map stays static: one allow, two denies. No requests were sent.';return;}
  b.disabled=false;
  if(paused){b.replaceChildren('Play ','▶');n.textContent='Paused. The static map remains; nothing was sent.';}
  else{b.replaceChildren('Pause ','❚❚');n.textContent='Animated from three invented decisions, looping. Allowed attempts cross the gate; denied attempts are intercepted at it. No agent is running and no requests are sent.';}
}
$('replay').addEventListener('click',()=>{paused=!paused;setPlaying(!paused);label();});
if('IntersectionObserver' in window){new IntersectionObserver(es=>{visible=es[es.length-1].isIntersecting;setPlaying(!paused&&!reduce.matches);}).observe(document.querySelector('.boundary-map'));}
document.addEventListener('visibilitychange',()=>setPlaying(!paused&&!reduce.matches));
(reduce.addEventListener?reduce.addEventListener.bind(reduce,'change'):reduce.addListener.bind(reduce))(()=>{setPlaying(!paused&&!reduce.matches);label();});
label();setPlaying(!reduce.matches);
