// Deterministic browser regressions: controlled responses force races without timing sleeps.
const {chromium} = require('playwright');
const assert = require('node:assert/strict');
const path = require('node:path');
const seed = require('../docs/examples/live-incidents.json')[0];
const watchdog = setTimeout(() => { console.error('Dashboard regression check timed out'); process.exit(1); }, 60000);
const deferred = () => { let resolve; const promise = new Promise(r => resolve = r); return {promise, resolve}; };
(async () => {
  const browser = await chromium.launch({executablePath:process.env.CHROMIUM_EXECUTABLE_PATH || '/usr/bin/chromium',headless:true,args:['--no-sandbox']});
  try {
    const page = await browser.newPage();
    const errors = []; page.on('pageerror', e => errors.push(e.message));
    await page.addInitScript(() => { window.setInterval = () => 0; });
    const first = {...seed, title:'First incident'};
    const second = {...seed, id:'11111111-1111-4111-8111-111111111111', title:'Second incident'};
    let hold = null, overviewHold = null;
    await page.route('**/app.js', route => route.fulfill({path:path.join(__dirname,'../web/app.js'),contentType:'application/javascript'}));
    await page.route('**/api/**', async route => {
      const url = new URL(route.request().url());
      let body;
      if (url.pathname === '/api/overview') {
        if (overviewHold) { const gate = overviewHold; overviewHold = null; gate.arrived.resolve(); await gate.release.promise; }
        body = [first,second].map(i => ({...i,evidence_count:i.evidence.length}));
      } else if (url.pathname === '/api/integrations') body = {mode:'offline',connectors:[],connections:[],mcp_servers:[]};
      else if (url.pathname.endsWith('/review')) body = {incident_updated_at:seed.updated_at,repeated_payloads:3};
      else {
        body = url.pathname.endsWith(second.id) ? second : first;
        if (hold) { const gate = hold; hold = null; gate.arrived.resolve(); await gate.release.promise; body = {...body,title:gate.title || body.title}; }
      }
      await route.fulfill({json:body});
    });
    await page.goto(process.env.SRE_TEST_URL || 'http://127.0.0.1:8080');
    await page.getByRole('heading',{name:'First incident',exact:true}).waitFor();
    const draft = 'Keep my unfinished investigation question';
    await page.locator('#followup-input').fill(draft);
    await page.locator('[data-tab="evidence"]').click();
    await page.locator('[data-tab="overview"]').click();
    assert.equal(await page.locator('#followup-input').inputValue(), draft, 'tab navigation must retain drafts');
    await page.locator('#followup-input').focus();
    await page.locator('#followup-input').evaluate(el => el.setSelectionRange(5,12));
    await page.evaluate(() => loadSelected());
    assert.deepEqual(await page.locator('#followup-input').evaluate(el => [document.activeElement===el,el.selectionStart,el.selectionEnd]),[true,5,12]);
    await page.locator('[data-tab="evidence"]').click();
    await page.locator('#evidence-E1 summary').click();
    await page.evaluate(() => loadSelected());
    assert.equal(await page.locator('#evidence-E1').getAttribute('open'),'');
    // A slow previous request for the same incident must not overwrite a newer response.
    hold = {arrived:deferred(),release:deferred(),title:'Stale response'};
    const slow = hold;
    await page.evaluate(() => { window.slowRequest = loadSelected(); });
    await slow.arrived.promise;
    await page.evaluate(() => loadSelected());
    slow.release.resolve();
    await page.evaluate(() => window.slowRequest);
    assert.equal(await page.locator('.detail-header h2').textContent(),'First incident');
    // Selection must remove old action buttons immediately while the next response is pending.
    hold = {arrived:deferred(),release:deferred()};
    const selection = hold;
    await page.locator(`[data-id="${second.id}"]`).click();
    await selection.arrived.promise;
    assert.equal(await page.locator('#run-button').count(),0, 'old incident actions must not remain under a new selection');
    selection.release.resolve();
    await page.getByRole('heading',{name:'Second incident',exact:true}).waitFor();
    await page.locator(`[data-id="${first.id}"]`).click();
    await page.getByRole('heading',{name:'First incident',exact:true}).waitFor();
    assert.equal(await page.locator('#followup-input').inputValue(),draft, 'drafts belong to their incident');
    // Finishing a submission after selection changes must not clear the other draft.
    hold = {arrived:deferred(),release:deferred()};
    const submission = hold;
    await page.evaluate(({id,draft}) => { window.submission = run(id,draft); }, {id:first.id,draft});
    await submission.arrived.promise;
    await page.locator(`[data-id="${second.id}"]`).click();
    await page.getByRole('heading',{name:'Second incident',exact:true}).waitFor();
    await page.locator('#followup-input').fill('An independent second draft');
    submission.release.resolve();
    await page.evaluate(() => window.submission);
    assert.equal(await page.locator('#followup-input').inputValue(),'An independent second draft');
    await page.locator(`[data-id="${first.id}"]`).click();
    await page.getByRole('heading',{name:'First incident',exact:true}).waitFor();
    assert.equal(await page.locator('#followup-input').inputValue(),'');
    // A requested refresh during polling must trigger a subsequent fetch, not be dropped.
    overviewHold = {arrived:deferred(),release:deferred()};
    const polling = overviewHold;
    await page.evaluate(() => { window.pollRequest = refresh(true); });
    await polling.arrived.promise;
    await page.evaluate(() => refresh());
    first.title = 'Fresh after queued refresh';
    polling.release.resolve();
    await page.evaluate(() => window.pollRequest);
    await page.getByRole('heading',{name:first.title,exact:true}).waitFor();
    assert.deepEqual(errors,[]);
    console.log('PASS: drafts, cursor, expanded evidence, stale responses, safe selection, and queued refresh');
  } finally { clearTimeout(watchdog); await browser.close(); }
})().catch(e => { console.error(e); process.exit(1); });
