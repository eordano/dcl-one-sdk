const signInit = () => {
  const panel = document.getElementById('sign-panel');
  if (!panel || panel.dataset.armed || !panel.dataset.entityId) return;
  panel.dataset.armed = '1';
  const go = document.getElementById('sign-go');
  const $ = (id) => document.getElementById(id);
  const status = (tone, message) => {
    const s = $('sign-status');
    if (!s) return;
    s.hidden = false;
    s.className = 'note sign-status sign-status--' + tone;
    s.textContent = message;
  };
  const progressUrl = panel.dataset.api.replace(/\/sign$/, '/progress');
  const target = (() => {
    try {
      return new URL(panel.dataset.target || '').host;
    } catch {
      return '';
    }
  })();

  const size = (n) =>
    n >= 1e6 ? (n / 1e6).toFixed(1) + ' MB' : n >= 1e3 ? (n / 1e3).toFixed(1) + ' KB' : n + ' bytes';
  const secs = (ms) => {
    const s = Math.max(0, Math.round(ms / 1000));
    return s < 60 ? s + ' s' : Math.floor(s / 60) + ' min ' + (s % 60) + ' s';
  };

  const draw = (p) => {
    const box = $('sign-progress');
    if (!box) return;
    box.hidden = false;
    box.className = 'sign-progress sign-progress--' + p.phase;
    const now = Date.now();
    const elapsed = p.started_ms ? now - p.started_ms : 0;
    const counted = p.total > 0 && p.carrier !== 'curl';
    const pct = counted ? Math.min(100, Math.floor((p.sent / p.total) * 100)) : 0;
    const files = p.files ? `${p.files_sent} of ${p.files} files` : '';
    const home = p.reuse || '';
    let big = '';
    let pctText = '';
    let fill = 0;
    const meta = [];
    if (p.phase === 'checking') {
      big = `Asking ${target || 'the server'} what it already has…`;
      meta.push(`${p.files} files (${size(p.total)})`);
    } else if (p.phase === 'staging') {
      big = p.files
        ? `Preparing ${p.files} files (${size(p.total)})…`
        : `Preparing the entity (${size(p.total)})…`;
    } else if (p.phase === 'uploading') {
      if (counted) {
        big = `${size(p.sent)} of ${size(p.total)} uploaded`;
        pctText = pct + '%';
        fill = pct;
        meta.push(files);
        const rate = elapsed > 800 ? p.sent / (elapsed / 1000) : 0;
        if (rate > 0) {
          meta.push(size(rate) + '/s');
          meta.push('about ' + secs(((p.total - p.sent) / rate) * 1000) + ' left');
        }
        if (p.current) meta.push({ file: p.current });
      } else {
        big = `Uploading ${size(p.total)} · ${secs(elapsed)} so far`;
        meta.push(`${p.files} files`);
        meta.push('curl is carrying this upload and reports no progress');
      }
    } else if (p.phase === 'validating') {
      const sentAt = p.sent_ms || now;
      big = `Uploaded ${size(p.total)} in ${secs(sentAt - p.started_ms)}`;
      pctText = '100%';
      fill = 100;
      meta.push(`${p.files} files`);
      meta.push(
        `${target || 'the server'} is checking the deployment · ${secs(now - sentAt)} so far`
      );
    } else if (p.phase === 'done') {
      big = `Uploaded ${size(p.total)} in ${secs((p.sent_ms || now) - p.started_ms)}`;
      pctText = '100%';
      fill = 100;
      meta.push(`${p.files} files`);
    } else if (p.phase === 'failed') {
      const sentAll = !counted || p.sent >= p.total;
      big = sentAll
        ? `Uploaded ${size(p.total)} — the server refused it`
        : `Upload stopped at ${size(p.sent)} of ${size(p.total)}`;
      pctText = counted ? pct + '%' : '';
      fill = sentAll ? 100 : pct;
      meta.push(files);
      if (!sentAll && p.current) meta.push({ file: p.current });
    } else {
      box.hidden = true;
      return;
    }
    if (home) meta.push(home);
    $('sign-progress-big').textContent = big;
    $('sign-progress-pct').textContent = pctText;
    $('sign-progress-fill').style.width = fill + '%';
    const m = $('sign-progress-meta');
    m.textContent = '';
    for (const item of meta.filter(Boolean)) {
      const span = document.createElement('span');
      if (typeof item === 'object') {
        span.className = 'sign-progress__file';
        span.textContent = '↑ ' + item.file;
      } else {
        span.textContent = item;
      }
      m.appendChild(span);
    }
  };
  let poll = null;
  const progress = async () => {
    try {
      const res = await fetch(progressUrl, { cache: 'no-store' });
      const p = res.ok ? await res.json() : null;
      return p && p.phase && p.phase !== 'idle' ? p : null;
    } catch {
      return null;
    }
  };
  const stopPolling = () => {
    if (poll) clearTimeout(poll);
    poll = null;
  };
  const startPolling = () => {
    stopPolling();
    const tick = async () => {
      const p = await progress();
      if (p) {
        draw(p);
        const s = $('sign-status');
        if (s && s.classList.contains('sign-status--info')) s.hidden = true;
      }
      poll = setTimeout(tick, 400);
    };
    tick();
  };

  const rebuild = async () => {
    status('info', 'That signing request expired on the server. Rebuilding it…');
    let doc = null;
    try {
      const res = await fetch(location.href, { headers: { accept: 'text/html' } });
      if (res.ok) doc = parsePage(await res.text());
    } catch {}
    const fresh = doc && doc.getElementById('sign-panel');
    if (fresh && fresh.dataset.entityId) {
      panel.replaceWith(fresh);
      signInit();
      const again = $('sign-go');
      again.textContent = 'Sign again';
      status('info', 'Rebuilt with a fresh entity id — press Sign again and approve the wallet prompt.');
      again.focus();
      return;
    }
    window.__signBusy = false;
    if (window.__signSettled) await window.__signSettled();
    if (typeof pageToast === 'function') {
      pageToast('The publish run is gone — press Publish again to rebuild it.', true);
    }
  };

  go.addEventListener('click', async () => {
    try {
      if (!window.ethereum) {
        status('err', 'No wallet found — this browser needs MetaMask or another EIP-1193 wallet.');
        return;
      }
      go.disabled = true;
      window.__signBusy = true;
      status('info', 'Requesting wallet…');
      const accounts = await window.ethereum.request({ method: 'eth_requestAccounts' });
      const address = accounts[0];
      try {
        const pf = await (
          await fetch(panel.dataset.api.replace(/\/sign$/, '/preflight'), {
            method: 'POST',
            headers: { 'content-type': 'application/json' },
            body: JSON.stringify({ address }),
          })
        ).json();
        if (pf.verdict === 'may_not') {
          status('err', '✗ ' + pf.why + (pf.remedy ? ' — ' + pf.remedy : ''));
          go.disabled = false;
          return;
        }
      } catch {}
      status('info', 'Signing with ' + address + '…');
      const signature = await window.ethereum.request({
        method: 'personal_sign',
        params: [panel.dataset.entityId, address],
      });
      let deleteSignature = null;
      if (panel.dataset.deletePayload) {
        status('info', 'Signing the scene-removal authorization…');
        deleteSignature = await window.ethereum.request({
          method: 'personal_sign',
          params: [panel.dataset.deletePayload, address],
        });
      }
      status('info', 'Sending the signed deployment…');
      startPolling();
      const r = await (
        await fetch(panel.dataset.api, {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({
            address,
            signature,
            entityId: panel.dataset.entityId,
            deleteSignature,
          }),
        })
      ).json();
      stopPolling();
      const last = await progress();
      if (last) draw(last);
      if (r.ok) {
        status('ok', '✓ ' + r.message + ' — jump in: ' + panel.dataset.deepLink);
      } else if (r.stale) {
        await rebuild();
      } else {
        status('err', '✗ ' + r.error);
        if (!r.fatal) go.disabled = false;
      }
    } catch (e) {
      stopPolling();
      status('err', '✗ ' + (e && e.message ? e.message : e));
      go.disabled = false;
    } finally {
      stopPolling();
      window.__signBusy = false;
      if (window.__signSettled) window.__signSettled();
    }
  });
};
signInit();
