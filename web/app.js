'use strict';
const $ = id => document.getElementById(id);
let token = '', incidents = [], selected = null, detailIncident = null, detailReview = null, detailRecall = null, tab = 'overview', loading = false, refreshQueued = false, detailRequest = 0;
const drafts = new Map();
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
  if (loading) { if (!quiet) refreshQueued = true; return; }
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
    if (!quiet || tab === 'recall' || $('detail').dataset.revision !== revision) await loadSelected();
  } catch (e) { $('connection-state').textContent = 'Connection needs attention'; if (!quiet) notice(e.message, true); }
  finally { loading = false; if (refreshQueued) { refreshQueued = false; await refresh(); } }
}
function renderList() {
  const query = $('search').value.toLowerCase(), status = $('status-filter').value;
  const items = incidents.filter(i => `${i.title} ${i.service}`.toLowerCase().includes(query) && (status === 'all' || i.status === status));
  $('incident-items').innerHTML = items.length ? items.map(i => `<button class="incident-card ${i.id === selected ? 'selected' : ''}" data-id="${i.id}" aria-pressed="${i.id === selected}"><div class="card-top"><span class="service">${escapeHtml(i.service)}</span>${badge(i.status)}</div><strong>${escapeHtml(i.title)}</strong><small>${when(i.updated_at)} · ${i.evidence_count} evidence items</small></button>`).join('') : '<p class="no-results">No investigations found.</p>';
  $('incident-items').querySelectorAll('[data-id]').forEach(b => b.onclick = () => { selectIncident(b.dataset.id); renderList(); loadSelected(); });
}
function selectIncident(id) {
  selected = id; tab = 'overview';
  detailRequest++; // Invalidate requests even if the user returns to the same incident.
  detailIncident = null; detailReview = null; detailRecall = null;
  $('detail').dataset.incident = id;
  $('detail').dataset.revision = '';
  $('detail').innerHTML = '<p class="muted" role="status">Loading investigation…</p>';
}
async function loadSelected() {
  if (!selected) return;
  const id = selected, request = ++detailRequest;
  try {
    const [value, review, recall] = await Promise.all([api(`/api/incidents/${id}`), api(`/api/incidents/${id}/review`), tab === 'recall' ? api(`/api/incidents/${id}/recall`) : null]);
    if (selected === id && request === detailRequest) {
      detailIncident = value;
      detailReview = review.incident_updated_at === value.updated_at ? review : null;
      detailRecall = recall?.incident_updated_at === value.updated_at ? recall : null;
      renderDetail();
    }
  } catch(e) { if (selected === id && request === detailRequest) notice(e.message,true); }
}
function renderDetail() {
  const i = detailIncident?.id === selected ? detailIncident : null;
  if (!i) return;
  const draft = drafts.get(i.id) || '';
  const sameIncident = $('detail').dataset.incident === i.id;
  const input = sameIncident ? $('followup-input') : null;
  const cursor = input && document.activeElement === input ? [input.selectionStart, input.selectionEnd, input.selectionDirection] : null;
  const expanded = sameIncident ? [...$('detail').querySelectorAll('details[open][id]')].map(el => el.id) : [];
  $('detail').dataset.incident = i.id;
  $('detail').dataset.revision = `${i.id}:${i.updated_at}:${tab}`;
  let content = '';
  if (tab === 'overview') {
    const r = i.report;
    if (i.status === 'failed') content += `<p class="gap">Investigation failed: ${escapeHtml(i.events.filter(e => e.kind === 'investigation_failed').at(-1)?.message || 'See activity for details.')}${r ? ' The previous report is shown below.' : ''}</p>`;
    if (i.status === 'investigating') content += '<p class="gap">Investigation is running. This view updates automatically; completed tools are checkpointed as the run progresses.</p>';
    if (r && detailReview) content += `<div class="review-callout"><span>${detailReview.repeated_payloads} repeated payloads · Check current evidence and finding support.</span><button id="review-button">Review evidence</button></div>`;
    content += r ? `<p class="summary">${escapeHtml(r.summary)}</p><p class="section-label">SUPPORTED HYPOTHESES</p>${r.findings.length ? r.findings.map(f => `<article class="finding"><h3><span class="confidence">${escapeHtml(f.confidence)} support</span>${escapeHtml(f.title)}</h3><p>${escapeHtml(f.explanation)}</p>${f.evidence_ids.map(id => `<button class="citation" data-evidence="${escapeHtml(id)}">${escapeHtml(id)} ↗</button>`).join('')}</article>`).join('') : '<p class="muted">No supported findings yet.</p>'}<p class="section-label">PROPOSED NEXT STEPS · OPERATOR REVIEW</p><ol class="steps">${r.next_steps.map(s => `<li>${escapeHtml(s)}</li>`).join('')}</ol><p class="section-label">WHAT WE STILL DON’T KNOW</p>${r.gaps.map(g => `<p class="gap">${escapeHtml(g)}</p>`).join('')}` : '<div class="empty"><div class="empty-icon">⌁</div><h2>Ready to gather context.</h2><p>Run this investigation to query configured sources and review the evidence.</p></div>';
    if (!['resolved','investigating'].includes(i.status)) content += '<form class="followup" id="followup-form"><input id="followup-input" required maxlength="4000" placeholder="Ask a follow-up question…" aria-label="Follow-up question"><button class="secondary" type="submit">Investigate →</button></form>';
  } else if (tab === 'recall') {
    content = detailRecall ? renderRecall(detailRecall) : '<p class="muted">Loading past incident matches…</p>';
  } else if (tab === 'review') {
    content = detailReview ? renderReview(detailReview) : '<p class="muted">Evidence is updating. The review will refresh with the saved investigation.</p>';
  } else if (tab === 'evidence') {
    content = i.evidence.length ? i.evidence.map(e => `<details class="evidence-item" id="evidence-${escapeHtml(e.id)}"><summary><span class="citation">${escapeHtml(e.id)}</span> ${escapeHtml(e.summary)}</summary><small>${escapeHtml(e.source)} · collected ${when(e.observed_at)}</small><pre>${escapeHtml(JSON.stringify(e.data, null, 2))}</pre></details>`).join('') : '<p class="muted">No evidence collected yet. Configure a source or run the bundled demo.</p>';
  } else if (tab === 'trace') {
    content = (i.trace || []).length ? i.trace.map(t => `<article class="event"><strong>${escapeHtml(t.phase.replaceAll('_',' '))}</strong><p>${escapeHtml(t.operation)} · ${escapeHtml(t.elapsed_ms)} ms from run start${t.request_bytes ? ` · ${escapeHtml(t.request_bytes)} request bytes` : ''}</p><small>Run ${escapeHtml((t.run_id || '').slice(0,8))}${t.evidence_ids.length ? ` · ${t.evidence_ids.map(escapeHtml).join(', ')}` : ''}</small></article>`).join('') : '<p class="muted">No runtime trace recorded for this investigation.</p>';
  } else content = i.events.map(e => `<article class="event"><strong>${escapeHtml(e.kind.replaceAll('_',' '))}</strong><p>${escapeHtml(e.message)}</p><small>${when(e.at)}</small></article>`).join('');
  $('detail').innerHTML = `<div class="detail-header"><div class="detail-meta">${badge(i.status)}<span>${escapeHtml(i.service)}</span><span>${i.id.slice(0,8)}</span></div><h2>${escapeHtml(i.title)}</h2><div class="detail-actions"><button class="secondary" id="run-button" ${['investigating','resolved'].includes(i.status) ? 'disabled' : ''}>↻ ${i.report ? 'Run again' : 'Investigate'}</button><button id="resolve-button" ${['investigating','resolved'].includes(i.status) ? 'disabled' : ''}>✓ Mark resolved</button><button id="export-button">↓ Export JSON</button><button id="handoff-button">↓ Handoff</button>${i.status === 'investigating' ? '<button id="cancel-button">Cancel run</button>' : '<button id="compact-button">Compact context</button>'}</div></div><nav class="tabs" aria-label="Investigation details">${['overview','review','recall','evidence','activity','trace'].map(t => `<button data-tab="${t}" class="${tab === t ? 'active' : ''}">${t[0].toUpperCase()+t.slice(1)}${t === 'evidence' ? ` (${i.evidence.length})` : ''}</button>`).join('')}</nav><div class="detail-body">${content}<p class="usage">${i.usage.tool_calls} tool calls · ${i.usage.model_calls} model calls · ${i.usage.input_tokens.toLocaleString()} input / ${i.usage.output_tokens.toLocaleString()} output tokens</p></div>`;
  if (!detailReview || (tab === 'recall' && !detailRecall)) $('detail').dataset.revision = '';
  document.querySelectorAll('[data-tab]').forEach(b => b.onclick = () => { tab = b.dataset.tab; if (['review','recall'].includes(tab)) loadSelected(); else renderDetail(); });
  document.querySelectorAll('[data-evidence]').forEach(b => b.onclick = () => { const id = b.dataset.evidence; tab = 'evidence'; renderDetail(); const el = $(`evidence-${id}`); if (el) { el.open = true; el.scrollIntoView({behavior:'smooth',block:'nearest'}); } });
  document.querySelectorAll('[data-past-id]').forEach(b => b.onclick = async () => {
    const id = b.dataset.pastId, evidence = b.dataset.pastEvidence;
    selectIncident(id); tab = evidence ? 'evidence' : 'overview'; renderList(); await loadSelected();
    if (selected === id && evidence) { const el = $(`evidence-${evidence}`); if (el) { el.open = true; el.scrollIntoView({block:'nearest'}); } }
  });
  if ($('review-button')) $('review-button').onclick = () => { tab = 'review'; loadSelected(); };
  $('run-button').onclick = () => run(i.id);
  for (const action of ['cancel','compact']) { const button = $(`${action}-button`); if(button) button.onclick=async()=>{try{await api(`/api/incidents/${i.id}/${action}`,{method:'POST'});await refresh();notice(action==='cancel'?'Cancellation requested.':'Context compacted; evidence retained.');}catch(e){notice(e.message,true);}}; }
  $('resolve-button').onclick = async () => { try { await api(`/api/incidents/${i.id}/resolve`, {method:'POST'}); await refresh(); notice('Incident marked resolved.'); } catch(e) { notice(e.message,true); } };
  $('export-button').onclick = () => { const url = URL.createObjectURL(new Blob([JSON.stringify(i,null,2)],{type:'application/json'})); const link = document.createElement('a'); link.href=url; link.download=`incident-${i.id}.json`; link.click(); setTimeout(() => URL.revokeObjectURL(url), 1000); };
  $('handoff-button').onclick = async () => {
    try {
      const {markdown} = await api(`/api/incidents/${i.id}/handoff`);
      const url = URL.createObjectURL(new Blob([markdown], {type:'text/markdown;charset=utf-8'}));
      const link = document.createElement('a'); link.href = url; link.download = `handoff-${i.id}.md`; link.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
      notice('Handoff downloaded with evidence references and outstanding checks.');
    } catch(e) { notice(e.message, true); }
  };
  for (const id of expanded) { const el = $(id); if (el) el.open = true; }
  if ($('followup-form')) {
    const input = $('followup-input'); input.value = draft;
    input.oninput = () => drafts.set(i.id, input.value);
    if (cursor) { input.focus({preventScroll:true}); input.setSelectionRange(...cursor); }
    $('followup-form').onsubmit = e => { e.preventDefault(); run(i.id, input.value); };
  }
}
function renderRecall(recall) {
  const matches = (items, past) => items.map(m => `<p><strong>${escapeHtml(m.label)}</strong><br>Current: ${m.current_ids.map(id => `<button class="citation" data-evidence="${escapeHtml(id)}">${escapeHtml(id)} ↗</button>`).join(' ')}<br>Past: ${m.past_ids.map(id => `<button class="citation" data-past-id="${escapeHtml(past)}" data-past-evidence="${escapeHtml(id)}">${escapeHtml(id)} ↗</button>`).join(' ')}</p>`).join('');
  return `<p class="summary">Use previous investigations to decide what to check next.</p><p class="muted">${escapeHtml(recall.reason)}</p>
    ${recall.search_truncated ? '<p class="gap">Search limited to the 100 most recently opened eligible cases for this service.</p>' : ''}
    <p class="gap">A past resolution does not verify a fix for this incident. The matched run may precede the case’s latest report. Check its evidence and differences before acting.</p>
    ${recall.checks.map(c => `<p class="gap">${escapeHtml(c)}</p>`).join('')}
    ${recall.cases.map(c => `<article class="finding recall-case"><h3>${escapeHtml(c.title)}</h3><p>${badge(c.status)} · Updated ${when(c.updated_at)}</p><p class="muted">Matched run ${escapeHtml(c.matched_run.slice(0,8))} · ${c.exact_matches.length} distinct exact payload matches · ${c.shared_symptoms.length} shared symptom rules</p>
      <details id="recall-${escapeHtml(c.incident_id)}"><summary>Why this case matches</summary><h4>Exact retained payloads</h4>${matches(c.exact_matches,c.incident_id) || '<p class="muted">None</p>'}<h4>Shared symptom rules</h4>${matches(c.shared_symptoms,c.incident_id) || '<p class="muted">None</p>'}</details>
      <p>Current-only symptoms: ${escapeHtml(c.current_only_symptoms.join('; ') || 'None detected')}</p><p>Past-only symptoms: ${escapeHtml(c.past_only_symptoms.join('; ') || 'None detected')}</p>
      <button data-past-id="${escapeHtml(c.incident_id)}">Open past case ↗</button></article>`).join('')}
    <details id="recall-limits" class="review-limits"><summary>How recall works</summary>${recall.limitations.map(l => `<p class="muted">${escapeHtml(l)}</p>`).join('')}</details>`;
}
function renderReview(review) {
  const citations = ids => ids.length ? ids.map(id => `<button class="citation" data-evidence="${escapeHtml(id)}">${escapeHtml(id)} ↗</button>`).join('') : '<span class="muted">None</span>';
  const changes = review.changes;
  return `<p class="summary">Check the evidence behind the report before deciding what to do.</p>
    <div class="review-stats"><div><strong>${review.observations}</strong> observations</div><div><strong>${review.distinct_payloads}</strong> distinct payloads</div><div><strong>${review.repeated_payloads}</strong> repeated payloads</div><div><strong>${review.context_items}</strong> context items</div></div>
    <h2>Evidence changes</h2><p class="muted">${escapeHtml(changes.reason)}</p>
    ${changes.available ? `<div class="review-change"><h3>Added observations</h3>${citations(changes.added)}</div><div class="review-change"><h3>Not repeated in this run</h3>${citations(changes.not_repeated)}</div><div class="review-change"><h3>Same retained payload</h3>${changes.repeated.length ? changes.repeated.map(group => `<p>${citations(group.before)} → ${citations(group.after)}</p>`).join('') : '<p class="muted">No matching payloads.</p>'}</div>` : ''}
    <p class="gap">Not repeated does not mean recovered. Queries and source coverage may differ.</p>
    <p class="section-label">FINDING SUPPORT</p>
    ${review.findings.map(f => `<article class="finding"><h3>${escapeHtml(f.title)}</h3><p>Distinct payloads: ${f.distinct_payloads} · Source labels: ${f.source_count}</p><p>Latest-run observations: ${citations(f.current_observations)}</p>${f.concerns.map(c => `<p class="gap">${escapeHtml(c)}</p>`).join('')}</article>`).join('') || '<p class="muted">No report findings to review yet.</p>'}
    <p class="section-label">CHECKS FOR THE NEXT OPERATOR</p><ul class="steps">${review.checks.map(c => `<li>${escapeHtml(c)}</li>`).join('')}</ul>
    <details class="review-limits"><summary>How to interpret this review</summary>${review.limitations.map(c => `<p class="muted">${escapeHtml(c)}</p>`).join('')}<p class="muted">Generated ${when(review.generated_at)}. Collection age is checked when the review is loaded.</p></details>`;
}
async function run(id, question) {
  try { await api(`/api/incidents/${id}/investigate`,{method:'POST',body:JSON.stringify(question ? {question} : {})}); if (question && drafts.get(id) === question) { drafts.delete(id); if (selected === id && $('followup-input')) $('followup-input').value=''; } await refresh(); notice('Investigation started.'); } catch(e) { notice(e.message,true); }
}
function openCreate() { $('create-error').textContent = ''; $('create-dialog').showModal(); }
$('new-button').onclick = openCreate;
$('empty-new').onclick = openCreate;
$('close-create').onclick = () => $('create-dialog').close();
$('create-form').onsubmit = async e => {
  e.preventDefault(); const button = e.target.querySelector('[type=submit]'); button.disabled=true;
  try { const data = Object.fromEntries(new FormData(e.target)); const i = await api('/api/incidents',{method:'POST',body:JSON.stringify(data)}); selectIncident(i.id); $('create-dialog').close(); e.target.reset(); await refresh(); await run(i.id); } catch(error) { $('create-error').textContent=error.message; } finally { button.disabled=false; }
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
