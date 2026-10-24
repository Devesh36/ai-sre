const {chromium} = require('playwright');
const assert = require('node:assert/strict');
(async () => {
  const browser = await chromium.launch({executablePath:process.env.CHROMIUM_EXECUTABLE_PATH || '/usr/bin/chromium',headless:true,args:['--no-sandbox']});
  try {
    const page = await browser.newPage({viewport:{width:1440,height:1100}});
    page.setDefaultTimeout(15000);
    const errors=[]; page.on('pageerror',e=>errors.push(e.message));
    const base=process.env.SRE_TEST_URL || 'http://127.0.0.1:8080';
    async function investigate(title) {
      const response=await page.request.post(`${base}/api/incidents`,{data:{title,service:'checkout-api'}});
      assert.equal(response.status(),201);
      const incident=await response.json();
      const run=await page.request.post(`${base}/api/incidents/${incident.id}/investigate`,{data:{}});
      assert.equal(run.status(),202);
      for(let n=0;n<100;n++) {
        const current=await (await page.request.get(`${base}/api/incidents/${incident.id}`)).json();
        if(current.status==='triaged') return current;
        assert.notEqual(current.status,'failed');
        await new Promise(resolve=>setTimeout(resolve,100));
      }
      throw new Error('Synthetic investigation did not finish');
    }
    const pastTitle=`Previous checkout <img src=x onerror=alert(1)> ${Date.now()}`;
    const past=await investigate(pastTitle);
    assert.equal((await page.request.post(`${base}/api/incidents/${past.id}/resolve`)).status(),200);
    const current=await investigate(`Recurrent checkout ${Date.now()}`);
    await page.goto(base);
    await page.getByRole('heading',{name:current.title,exact:true}).waitFor();
    await page.locator('[data-tab=recall]').click();
    const card=page.locator('.recall-case').filter({hasText:pastTitle});
    await card.waitFor();
    assert.ok((await card.innerText()).includes('3 distinct exact payload matches'));
    assert.equal(await card.locator('img').count(),0,'titles must remain escaped text');
    await card.getByText('Why this case matches',{exact:true}).click();
    await page.evaluate(() => refresh(true));
    assert.equal(await card.locator('details').getAttribute('open'),'');
    assert.equal(await card.locator('[data-past-evidence="E1"]').count(),2); // exact + symptom explanations
    await page.screenshot({path:'.ai-sre/recall-desktop.png',fullPage:true});
    await card.locator('[data-past-evidence="E1"]').first().click();
    await page.getByRole('heading',{name:pastTitle,exact:true}).waitFor();
    await page.locator('#evidence-E1[open]').waitFor();
    await page.locator(`[data-id="${current.id}"]`).click();
    await page.getByRole('heading',{name:current.title,exact:true}).waitFor();
    await page.locator('[data-tab=recall]').click();
    await card.waitFor();
    await page.setViewportSize({width:390,height:844});
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
    await page.screenshot({path:'.ai-sre/recall-mobile.png',fullPage:true});
    assert.deepEqual(errors,[]);
    console.log('PASS: recurring incident recall, explicit match reasons, resolved-case caveat, escaped titles, historical citations, and mobile layout');
  } finally { await browser.close(); }
})().catch(e=>{console.error(e);process.exit(1)});
