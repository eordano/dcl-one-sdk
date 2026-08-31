/* The wallet hand-off. Every deployment fact — scene, target, parcels, entity
   id, payload, the delete payload and the jump-in link — is server-rendered
   into #sign-panel's rows and data attributes, so the only work left here is
   the one thing a server cannot do: ask the wallet. */
const signInit = () => {
  const panel = document.getElementById('sign-panel');
  if (!panel || panel.dataset.armed || !panel.dataset.entityId) return;
  panel.dataset.armed = '1';
  const go = document.getElementById('sign-go');
  const status = (tone, message) => {
    const s = document.getElementById('sign-status');
    s.hidden = false;
    s.className = 'note sign-status sign-status--' + tone;
    s.textContent = message;
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
      /* The moment between "which wallet" and "sign" is the last one where a
         refusal costs nothing: ask the server whether this address may
         publish to the declared target, and stop BEFORE the signature when
         the answer is no — a catalyst saying it after is the incident this
         exists to prevent. An unanswered check never blocks: the server
         still enforces, this is the courtesy copy. */
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
      status('info', 'Uploading…');
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
      if (r.ok) {
        status('ok', '✓ ' + r.message + ' — jump in: ' + panel.dataset.deepLink);
      } else {
        status('err', '✗ ' + r.error);
        if (!r.fatal) go.disabled = false;
      }
    } catch (e) {
      status('err', '✗ ' + (e && e.message ? e.message : e));
      go.disabled = false;
    } finally {
      window.__signBusy = false;
      if (window.__signSettled) window.__signSettled();
    }
  });
};
signInit();
