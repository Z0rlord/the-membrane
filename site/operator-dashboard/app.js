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
let replayTimer;
$('replay').addEventListener('click', () => {
  const panel = document.querySelector('.boundary-panel');
  if (panel.classList.contains('replaying')) {
    clearTimeout(replayTimer);
    panel.classList.remove('replaying');
    $('replay').textContent = 'Replay sample ↗';
    $('replay-note').textContent = 'Replay stopped. Static fixture remains visible; no requests were sent.';
    return;
  }
  if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
    $('replay-note').textContent = 'Reduced motion: showing the static sample paths. One allow, two denies. No requests were sent.';
    return;
  }
  panel.classList.add('replaying');
  $('replay').textContent = 'Stop replay';
  $('replay-note').textContent = 'Replaying three invented samples once. This is animation, not live activity.';
  replayTimer = setTimeout(() => {
    panel.classList.remove('replaying');
    $('replay').textContent = 'Replay sample ↗';
    $('replay-note').textContent = 'Replay complete. One allow, two denies. Static fixture only; no requests were sent.';
  }, 5400);
});
