'use strict';
let opsTab='work';
const fields={
  work:[['service','Service'],['text','Work item']],
  memory:[['service','Service'],['key','Topic'],['text','Knowledge to retain']],
  schedules:[['service','Service'],['question','Investigation question'],['expression','Cron expression (seconds first)'],['timezone','Timezone']],
  actions:[['incident_id','Incident UUID'],['template','Configured action template'],['arguments','Arguments (JSON object)'],['rationale','Reason and supporting evidence']]
};
function opsForm(){
  $('ops-form-title').textContent={work:'Add work item',memory:'Add service memory',schedules:'Schedule an investigation',actions:'Propose an action'}[opsTab];
  $('ops-form').innerHTML=fields[opsTab].map(([name,label])=>`<label>${label}${['text','question','arguments','rationale'].includes(name)?`<textarea name="${name}" required maxlength="4000" rows="3">${name==='arguments'?'{}':''}</textarea>`:`<input name="${name}" required maxlength="${['service','key'].includes(name)?128:256}" value="${name==='expression'?'0 */15 * * * *':name==='timezone'?'UTC':''}">`}</label>`).join('')+'<button class="primary" type="submit">'+(opsTab==='actions'?'Create proposal':'Save')+'</button>';
  $('ops-error').textContent='';
}
async function loadOps(){
  try{
    const requested=opsTab;const items=await api(`/api/${requested}`);if(requested!==opsTab)return;
    $('ops-items').innerHTML=items.length?items.map(item=>{
      if(opsTab==='work')return `<article class="note ops-card"><div class="card-top"><strong>${escapeHtml(item.service)}</strong>${badge(item.done?'done':'open')}</div><p>${escapeHtml(item.text)}</p>${!item.done?`<button class="secondary" data-complete="${item.id}">Mark complete</button>`:''}</article>`;
      if(opsTab==='memory')return `<article class="note ops-card"><div class="card-top"><strong>${escapeHtml(item.key)}</strong><span>${escapeHtml(item.service)}</span></div><p>${escapeHtml(item.text)}</p><button data-remove="${item.id}">Remove note</button></article>`;
      if(opsTab==='schedules')return `<article class="note ops-card"><strong>${escapeHtml(item.question)}</strong><p>${escapeHtml(item.service)} · <code>${escapeHtml(item.expression)}</code> · ${escapeHtml(item.timezone)}</p><p>Next: ${when(item.next_at)}</p><button data-remove="${item.id}">Remove schedule</button></article>`;
      return `<article class="note ops-card"><div class="card-top"><strong>${escapeHtml(item.template.name)}</strong>${badge(item.status)}</div><p>${escapeHtml(item.rationale)}</p><details><summary>Review exact request and conditions</summary><pre>${escapeHtml(JSON.stringify({method:item.template.method,url:item.template.url,body:item.rendered_body,precondition:item.template.precondition,verification:item.template.verification,rollback:item.template.rollback},null,2))}</pre></details><p class="muted">Incident ${escapeHtml(item.incident_id)} · ${when(item.created_at)}</p>${item.status==='pending'?`<label class="reviewer">Reviewer name<input data-actor="${item.id}" maxlength="128" aria-label="Reviewer name"></label><button class="secondary" data-approve="${item.id}">Approve this request</button> <button data-reject="${item.id}">Reject</button>`:item.status==='approved'?`<p>Approved by ${escapeHtml(item.actor)}. Approval expires after 15 minutes.</p><button class="secondary" data-execute="${item.id}">Execute approved action</button>`:''}${item.result?`<p>${escapeHtml(item.result)}</p>`:''}</article>`;
    }).join(''):'<div class="empty"><div class="empty-icon">▤</div><h2>Nothing here yet.</h2><p>Use the form to create the first item.</p></div>';
    $('ops-items').querySelectorAll('[data-complete]').forEach(b=>b.onclick=()=>opsRequest(`/api/work/${b.dataset.complete}/complete`,'POST'));
    $('ops-items').querySelectorAll('[data-remove]').forEach(b=>b.onclick=()=>opsRequest(`/api/${opsTab}/${b.dataset.remove}`,'DELETE'));
    for(const [attribute,approve]of [['approve',true],['reject',false]])$('ops-items').querySelectorAll(`[data-${attribute}]`).forEach(b=>b.onclick=()=>{
      const id=b.dataset[attribute],actor=$('ops-items').querySelector(`[data-actor="${id}"]`).value;
      if(!actor.trim()){notice('Enter a reviewer name.',true);return;}
      opsRequest(`/api/actions/${id}/review`,'POST',{actor,approve});
    });
    $('ops-items').querySelectorAll('[data-execute]').forEach(b=>b.onclick=()=>{
      if(window.confirm('Execute this approved request against its configured destination? Review the exact request and rollback procedure first.'))opsRequest(`/api/actions/${b.dataset.execute}/execute`,'POST');
    });
  }catch(e){notice(e.message,true);}
}
async function opsRequest(path,method,body){try{await api(path,{method,...(body?{body:JSON.stringify(body)}:{})});await loadOps();notice('Saved.');}catch(e){notice(e.message,true);}}
$('operations-nav').onclick=()=>{page(false);$('incident-page').hidden=true;$('operations-page').hidden=false;$('incidents-nav').classList.remove('active');$('operations-nav').classList.add('active');$('page-label').textContent='Operations';opsForm();loadOps();};
$('ops-tabs').querySelectorAll('[data-ops]').forEach(b=>b.onclick=()=>{opsTab=b.dataset.ops;$('ops-tabs').querySelectorAll('button').forEach(t=>t.classList.toggle('active',t===b));opsForm();loadOps();});
$('ops-refresh').onclick=loadOps;
$('ops-form').onsubmit=async e=>{
  e.preventDefault();const button=e.target.querySelector('[type=submit]');button.disabled=true;
  try{const input=Object.fromEntries(new FormData(e.target));if(opsTab==='actions')input.arguments=JSON.parse(input.arguments);await api(`/api/${opsTab}`,{method:'POST',body:JSON.stringify(input)});opsForm();await loadOps();notice('Saved.');}catch(error){$('ops-error').textContent=error.message;}finally{button.disabled=false;}
};
