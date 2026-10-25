const {chromium} = require('playwright');
const {randomUUID} = require('node:crypto');
const assert = require('node:assert/strict');
(async () => {
  const browser = await chromium.launch({executablePath:process.env.CHROMIUM_EXECUTABLE_PATH || '/usr/bin/chromium',headless:true,args:['--no-sandbox']});
  try {
    const page=await browser.newPage({viewport:{width:1440,height:1100}});
    page.setDefaultTimeout(15000);
    const errors=[];page.on('pageerror',e=>errors.push(e.message));
    const base=process.env.SRE_TEST_URL || 'http://127.0.0.1:8080';
    async function investigate(title) {
      const created=await page.request.post(`${base}/api/incidents`,{data:{title,service:'checkout-api'}});assert.equal(created.status(),201);
      const i=await created.json();assert.equal((await page.request.post(`${base}/api/incidents/${i.id}/investigate`,{data:{}})).status(),202);
      for(let n=0;n<100;n++) {
        const saved=await (await page.request.get(`${base}/api/incidents/${i.id}`)).json();
        if(saved.status==='triaged')return saved;
        assert.notEqual(saved.status,'failed');await new Promise(r=>setTimeout(r,100));
      }
      throw new Error('Synthetic investigation did not finish');
    }
    const past=await investigate(`Outcome provenance example ${Date.now()}`);
    const input={id:randomUUID(),run_id:past.run_id,actor:'operator <b>self-reported</b>',claim:'<img src=x onerror=alert(1)> A rollback fixed the incident',verdict:'supported',rationale:'Initial operator interpretation of the retained event.',evidence_ids:['E1']};
    const endpoint=`${base}/api/incidents/${past.id}/outcomes`;
    assert.equal((await page.request.post(endpoint,{data:input})).status(),201);
    assert.equal((await page.request.post(endpoint,{data:input})).status(),200);
    const correction={...input,id:randomUUID(),verdict:'inconclusive',rationale:'The event establishes memory exhaustion, but does not verify rollback or recovery.',supersedes:input.id};
    const response=await page.request.post(endpoint,{data:correction});assert.equal(response.status(),201);
    const saved=await response.json();const fingerprint=saved.outcome.evidence[0].payload_sha256;
    assert.equal(fingerprint.length,64);
    assert.equal((await page.request.post(`${base}/api/incidents/${past.id}/resolve`)).status(),200);
    const current=await investigate(`Current incident must remain independent ${Date.now()}`);
    await page.goto(base);
    await page.getByRole('heading',{name:current.title,exact:true}).waitFor();
    await page.locator('[data-tab=outcomes]').click();
    await page.getByText('No operator assessments recorded.',{exact:true}).waitFor();
    await page.locator(`[data-id="${past.id}"]`).click();
    await page.getByRole('heading',{name:past.title,exact:true}).waitFor();
    await page.locator('[data-tab=outcomes]').click();
    await page.locator('.outcome-card').first().waitFor();
    assert.equal(await page.locator('.outcome-card').count(),2);
    assert.equal(await page.locator('.outcome-card img,.outcome-card b').count(),0);
    assert.ok((await page.locator('.outcome-card').nth(1).innerText()).includes('Superseded'));
    await page.locator('.outcome-card').first().getByText('Evidence provenance',{exact:true}).click();
    assert.ok((await page.locator('.outcome-card').first().innerText()).includes(fingerprint));
    await page.evaluate(()=>refresh(true));
    assert.equal(await page.locator('.outcome-card details').first().getAttribute('open'),'');
    await page.screenshot({path:'.ai-sre/outcomes-desktop.png',fullPage:true});
    await page.setViewportSize({width:390,height:844});
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
    await page.screenshot({path:'.ai-sre/outcomes-mobile.png',fullPage:true});
    await page.locator(`[data-id="${current.id}"]`).click();
    await page.getByRole('heading',{name:current.title,exact:true}).waitFor();
    await page.locator('[data-tab=recall]').click();
    const card=page.locator('.recall-case').filter({hasText:past.title});await card.waitFor();
    assert.equal(await card.locator('.outcome-card').count(),1);
    assert.ok((await card.locator('.outcome-card').innerText()).includes('inconclusive'));
    await card.getByText('Historical assessment. It does not verify the current incident or run.',{exact:true}).waitFor();
    await card.locator('.outcome-card').getByText('Evidence provenance',{exact:true}).click();
    await card.locator('.outcome-card [data-past-evidence="E1"]').click();
    await page.getByRole('heading',{name:past.title,exact:true}).waitFor();
    await page.locator('#evidence-E1[open]').waitFor();
    const unchanged=await (await page.request.get(`${base}/api/incidents/${current.id}`)).json();
    assert.deepEqual(unchanged,current);
    assert.deepEqual(errors,[]);
    console.log('PASS: explicit operator records, idempotent submission, retained corrections, escaped text, provenance, historical warnings, current-incident isolation, and mobile layout');
  } finally {await browser.close();}
})().catch(e=>{console.error(e);process.exit(1)});
