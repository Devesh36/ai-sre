'use strict';
const $ = id => document.getElementById(id);
let token = '', incidents = [], selected = null, detailIncident = null, tab = 'overview', loading = false;
const escapeHtml = text => String(text ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const when = date => new Date(date).toLocaleString([], {month:'short',day:'numeric',hour:'2-digit',minute:'2-digit'});
const badge = status => `<span class="badge ${escapeHtml(status)}">${escapeHtml(status)}</span>`;
async function api(path, options = {}) {
  const response = await fetch(path, {...options, headers: {'Content-Type':'application/json', ...(token ? {'Authorization':`Bearer ${token}`} : {}), ...options.headers}});
  if (response.status === 401) { if (!$('auth-dialog').open) $('auth-dialog').showModal(); throw new Error('Connect with your workspace API token.'); }
  const result = await response.json();
  if (!response.ok) throw new Error(result.error || `Request failed (${response.status})`);
  return result;
}
let noticeTimer;
function notice(text, error = false) { $('notice').textContent = text; $('notice').className = error ? 'error' : ''; $('notice').hidden = false; clearTimeout(noticeTimer); noticeTimer = setTimeout(() => $('notice').hidden = true, 7000); }
async function refresh(quiet = false) {
  if (loading) return;
  loading = true;
  try {
    incidents = await api('/api/overview?limit=100');
    $('connection-state').textContent = 'Workspace connected';
    $('total-count').textContent = incidents.length;
    $('nav-count').textContent = incidents.length;
    $('running-count').textContent = incidents.filter(i => i.status === 'investigating').length;
    $('triaged-count').textContent = incidents.filter(i => i.status === 'triaged').length;
    $('evidence-count').textContent = incidents.reduce((n,i) => n + i.evidence_count, 0);
    if (!selected && incidents.length) selected = incidents[0].id;
    renderList();
    // Preserve focused follow-up text and expanded evidence during background polling.
    const current = incidents.find(i => i.id === selected);
    const revision = current ? `${current.id}:${current.updated_at}:${tab}` : '';
    if ($('detail').dataset.revision !== revision) await loadSelected();
  } catch (e) { $('connection-state').textContent = 'Connection needs attention'; if (!quiet) notice(e.message, true); }
  finally { loading = false; }
}
function renderList() {
  const query = $('search').value.toLowerCase(), status = $('status-filter').value;
  const items = incidents.filter(i => `${i.title} ${i.service}`.toLowerCase().includes(query) && (status === 'all' || i.status === status));
  $('incident-items').innerHTML = items.length ? items.map(i => `<button class="incident-card ${i.id === selected ? 'selected' : ''}" data-id="${i.id}" aria-pressed="${i.id === selected}"><div class="card-top"><span class="service">${escapeHtml(i.service)}</span>${badge(i.status)}</div><strong>${escapeHtml(i.title)}</strong><small>${when(i.updated_at)} · ${i.evidence_count} evidence items</small></button>`).join('') : '<p class="no-results">No investigations found.</p>';
  $('incident-items').querySelectorAll('[data-id]').forEach(b => b.onclick = () => { selected = b.dataset.id; tab = 'overview'; renderList(); loadSelected(); });
}
async function loadSelected() {
  if (!selected) return; const id = selected;
  try { const value = await api(`/api/incidents/${id}`); if (selected === id) { detailIncident = value; renderDetail(); } } catch(e) { notice(e.message,true); }
}
function renderDetail() {
  const i = detailIncident?.id === selected ? detailIncident : null;
  if (!i) return;
  const draft = $('detail').dataset.incident === i.id ? ($('followup-input')?.value || '') : '';
  $('detail').dataset.incident = i.id;
  $('detail').dataset.revision = `${i.id}:${i.updated_at}:${tab}`;
  let content = '';
  if (tab === 'overview') {
    const r = i.report;
    if (i.status === 'failed') content += `<p class="gap">Investigation failed: ${escapeHtml(i.events.filter(e => e.kind === 'investigation_failed').at(-1)?.message || 'See activity for details.')}${r ? ' The previous report is shown below.' : ''}</p>`;
    if (i.status === 'investigating') content += '<p class="gap">Investigation is running. This view updates automatically; completed tools are checkpointed as the run progresses.</p>';
    content += r ? `<p class="summary">${escapeHtml(r.summary)}</p><p class="section-label">SUPPORTED HYPOTHESES</p>${r.findings.length ? r.findings.map(f => `<article class="finding"><h3><span class="confidence">${escapeHtml(f.confidence)} support</span>${escapeHtml(f.title)}</h3><p>${escapeHtml(f.explanation)}</p>${f.evidence_ids.map(id => `<button class="citation" data-evidence="${escapeHtml(id)}">${escapeHtml(id)} ↗</button>`).join('')}</article>`).join('') : '<p class="muted">No supported findings yet.</p>'}<p class="section-label">PROPOSED NEXT STEPS · OPERATOR REVIEW</p><ol class="steps">${r.next_steps.map(s => `<li>${escapeHtml(s)}</li>`).join('')}</ol><p class="section-label">WHAT WE STILL DON’T KNOW</p>${r.gaps.map(g => `<p class="gap">${escapeHtml(g)}</p>`).join('')}` : '<div class="empty"><div class="empty-icon">⌁</div><h2>Ready to gather context.</h2><p>Run this investigation to query configured sources and review the evidence.</p></div>';
    if (!['resolved','investigating'].includes(i.status)) content += '<form class="followup" id="followup-form"><input id="followup-input" required maxlength="4000" placeholder="Ask a follow-up question…" aria-label="Follow-up question"><button class="secondary" type="submit">Investigate →</button></form>';
  } else if (tab === 'evidence') {
    content = i.evidence.length ? i.evidence.map(e => `<details class="evidence-item" id="evidence-${escapeHtml(e.id)}"><summary><span class="citation">${escapeHtml(e.id)}</span> ${escapeHtml(e.summary)}</summary><small>${escapeHtml(e.source)} · collected ${when(e.observed_at)}</small><pre>${escapeHtml(JSON.stringify(e.data, null, 2))}</pre></details>`).join('') : '<p class="muted">No evidence collected yet. Configure a source or run the bundled demo.</p>';
  } else if (tab === 'trace') {
    content = (i.trace || []).length ? i.trace.map(t => `<article class="event"><strong>${escapeHtml(t.phase.replaceAll('_',' '))}</strong><p>${escapeHtml(t.operation)} · ${escapeHtml(t.elapsed_ms)} ms from run start${t.request_bytes ? ` · ${escapeHtml(t.request_bytes)} request bytes` : ''}</p><small>Run ${escapeHtml((t.run_id || '').slice(0,8))}${t.evidence_ids.length ? ` · ${t.evidence_ids.map(escapeHtml).join(', ')}` : ''}</small></article>`).join('') : '<p class="muted">No runtime trace recorded for this investigation.</p>';
  } else content = i.events.map(e => `<article class="event"><strong>${escapeHtml(e.kind.replaceAll('_',' '))}</strong><p>${escapeHtml(e.message)}</p><small>${when(e.at)}</small></article>`).join('');
  $('detail').innerHTML = `<div class="detail-header"><div class="detail-meta">${badge(i.status)}<span>${escapeHtml(i.service)}</span><span>${i.id.slice(0,8)}</span></div><h2>${escapeHtml(i.title)}</h2><div class="detail-actions"><button class="secondary" id="run-button" ${['investigating','resolved'].includes(i.status) ? 'disabled' : ''}>↻ ${i.report ? 'Run again' : 'Investigate'}</button><button id="resolve-button" ${['investigating','resolved'].includes(i.status) ? 'disabled' : ''}>✓ Mark resolved</button><button id="export-button">↓ Export JSON</button>${i.status === 'investigating' ? '<button id="cancel-button">Cancel run</button>' : '<button id="compact-button">Compact context</button>'}</div></div><nav class="tabs" aria-label="Investigation details">${['overview','evidence','activity','trace'].map(t => `<button data-tab="${t}" class="${tab === t ? 'active' : ''}">${t[0].toUpperCase()+t.slice(1)}${t === 'evidence' ? ` (${i.evidence.length})` : ''}</button>`).join('')}</nav><div class="detail-body">${content}<p class="usage">${i.usage.tool_calls} tool calls · ${i.usage.model_calls} model calls · ${i.usage.input_tokens.toLocaleString()} input / ${i.usage.output_tokens.toLocaleString()} output tokens</p></div>`;
  document.querySelectorAll('[data-tab]').forEach(b => b.onclick = () => { tab = b.dataset.tab; renderDetail(); });
  document.querySelectorAll('[data-evidence]').forEach(b => b.onclick = () => { const id = b.dataset.evidence; tab = 'evidence'; renderDetail(); const el = $(`evidence-${id}`); if (el) { el.open = true; el.scrollIntoView({behavior:'smooth',block:'nearest'}); } });
  $('run-button').onclick = () => run(i.id);
  for (const action of ['cancel','compact']) { const button = $(`${action}-button`); if(button) button.onclick=async()=>{try{await api(`/api/incidents/${i.id}/${action}`,{method:'POST'});await refresh();notice(action==='cancel'?'Cancellation requested.':'Context compacted; evidence retained.');}catch(e){notice(e.message,true);}}; }
  $('resolve-button').onclick = async () => { try { await api(`/api/incidents/${i.id}/resolve`, {method:'POST'}); await refresh(); notice('Incident marked resolved.'); } catch(e) { notice(e.message,true); } };
  $('export-button').onclick = () => { const url = URL.createObjectURL(new Blob([JSON.stringify(i,null,2)],{type:'application/json'})); const link = document.createElement('a'); link.href=url; link.download=`incident-${i.id}.json`; link.click(); setTimeout(() => URL.revokeObjectURL(url), 1000); };
  if ($('followup-form')) { $('followup-input').value = draft; $('followup-form').onsubmit = e => { e.preventDefault(); run(i.id, $('followup-input').value); }; }
}
async function run(id, question) {
  try { await api(`/api/incidents/${id}/investigate`,{method:'POST',body:JSON.stringify(question ? {question} : {})}); if ($('followup-input')) $('followup-input').value=''; await refresh(); notice('Investigation started.'); } catch(e) { notice(e.message,true); }
}
function openCreate() { $('create-error').textContent = ''; $('create-dialog').showModal(); }
$('new-button').onclick = openCreate;
$('empty-new').onclick = openCreate;
$('close-create').onclick = () => $('create-dialog').close();
$('create-form').onsubmit = async e => {
  e.preventDefault(); const button = e.target.querySelector('[type=submit]'); button.disabled=true;
  try { const data = Object.fromEntries(new FormData(e.target)); const i = await api('/api/incidents',{method:'POST',body:JSON.stringify(data)}); selected=i.id; tab='overview'; $('create-dialog').close(); e.target.reset(); await refresh(); await run(i.id); } catch(error) { $('create-error').textContent=error.message; } finally { button.disabled=false; }
};
$('search').oninput=renderList;
$('status-filter').onchange=renderList;
$('refresh-button').onclick=() => refresh();
$('auth-button').onclick=() => $('auth-dialog').showModal();
$('close-auth').onclick=() => $('auth-dialog').close();
$('auth-form').onsubmit=async e => { e.preventDefault(); token=new FormData(e.target).get('token'); e.target.reset(); $('auth-dialog').close(); await refresh(); await connections(); };
async function connections() {
  try { const data=await api('/api/integrations'); $('mode').textContent=data.mode === 'model' ? 'MODEL ASSISTED' : 'OFFLINE TRIAGE'; $('connections').innerHTML=[...data.connectors,...data.connections.map(c=>({name:`${c.kind} / ${c.name} · ${c.operations} operations`,configured:true})),...data.mcp_servers.map(c=>({name:`MCP / ${c.name}`,configured:true}))].map(c=>`<article class="connection-card"><h2>${escapeHtml(c.name)}</h2>${badge(c.configured ? 'configured' : 'not configured')}</article>`).join(''); } catch(e) { notice(e.message,true); }
}
function page(showConnections) { $('operations-page').hidden=true; $('operations-nav').classList.remove('active'); $('incident-page').hidden=showConnections; $('connections-page').hidden=!showConnections; $('incidents-nav').classList.toggle('active',!showConnections); $('integrations-nav').classList.toggle('active',showConnections); $('page-label').textContent=showConnections?'Connections':'Investigations'; if(showConnections) connections(); }
$('incidents-nav').onclick=()=>page(false);
$('integrations-nav').onclick=()=>page(true);
refresh().then(connections);
setInterval(()=>{ if(!document.hidden && !$('auth-dialog').open) refresh(true); },3000);
