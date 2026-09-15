(() => {
  const target = document.querySelector('.tgt');
  if (!target) return;
  const key = 'target-tab:' + location.pathname + ':' + target.dataset.targetKind;
  try {
    const value = sessionStorage.getItem(key);
    const tab = [...target.querySelectorAll('input[name=tgt]')].find((t) => t.value === value);
    if (tab) tab.checked = true;
  } catch {}
  target.addEventListener('change', (event) => {
    if (event.target.name === 'tgt') {
      try { sessionStorage.setItem(key, event.target.value); } catch {}
    }
  });
  target.addEventListener('click', (event) => {
    const button = event.target.closest('[data-map-viewbox]');
    if (!button) return;
    const controls = button.closest('.tgt__map-controls');
    const svg = controls.nextElementSibling;
    svg.setAttribute('viewBox', button.dataset.mapViewbox);
    const view = button.dataset.mapViewbox.split(' ').map(Number);
    for (const label of svg.querySelectorAll('text')) label.setAttribute('font-size', Math.max(view[2], view[3]) * .025);
    for (const peer of controls.querySelectorAll('button')) peer.setAttribute('aria-pressed', String(peer === button));
  });
  document.addEventListener('submit', async (event) => {
    const form = event.target;
    if (form.action.endsWith('/target/base') && new FormData(form).get('destination') === 'land') {
      try { sessionStorage.setItem('target-tab:' + location.pathname + ':land', 'land'); } catch {}
    }
    if (form.action.endsWith('/target/point')) {
      const world = new FormData(form).get('world');
      const nextKind = world ? 'world' : 'land';
      try { sessionStorage.setItem('target-tab:' + location.pathname + ':' + nextKind, nextKind); } catch {}
    }
    if (form.action.endsWith('/target/connect')) {
      // Keep the selected destination visible while authorization opens separately.
      form.target = '_blank';
      setTimeout(() => location.reload(), 3000);
    }
    if (form.matches('.tgt__scene-remove')) {
      event.preventDefault();
      const button = form.querySelector('button');
      const status = form.querySelector('[role="status"]');
      const body = new URLSearchParams(new FormData(form));
      const send = async () => {
        const response = await fetch(form.action, { method: 'POST', body });
        if (!response.ok) throw new Error((await response.text()).trim());
        return response.json();
      };
      button.disabled = true;
      try {
        if (!window.ethereum) throw new Error('Connect a browser wallet with permission to remove this scene.');
        status.textContent = 'Connect your wallet to remove the scene at ' + body.get('coordinate') + '.';
        const accounts = await window.ethereum.request({method:'eth_requestAccounts'});
        const prepared = await send();
        status.textContent = 'Sign removal of this scene. Other scenes will stay.';
        const signature = await window.ethereum.request({method:'personal_sign',params:[prepared.payload,accounts[0]]});
        body.set('timestamp', prepared.timestamp);
        body.set('entity_id', prepared.entity_id);
        body.set('address', accounts[0]);
        body.set('signature', signature);
        status.textContent = 'Removing scene…';
        await send();
        location.reload();
      } catch (error) {
        status.textContent = error.message || String(error);
        button.disabled = false;
      }
      return;
    }
    if (!form.matches('.tgt__entrance')) return;
    event.preventDefault();
    const button = form.querySelector('button');
    const status = form.querySelector('[role="status"]');
    const body = new URLSearchParams(new FormData(form));
    const send = async () => {
      const response = await fetch(form.action, { method: 'POST', body });
      if (!response.ok) throw new Error((await response.text()).trim());
      return response.json();
    };
    button.disabled = true;
    let saved = false;
    try {
      if (!window.ethereum) throw new Error('Connect a browser wallet owned by the World owner to change its entrance.');
      status.textContent = 'Requesting the World owner’s wallet. This changes the entrance only.';
      const accounts = await window.ethereum.request({ method: 'eth_requestAccounts' });
      const prepared = await send();
      status.textContent = 'Sign the World entrance update in your wallet.';
      const signature = await window.ethereum.request({ method: 'personal_sign', params: [prepared.payload, accounts[0]] });
      body.set('timestamp', prepared.timestamp);
      body.set('address', accounts[0]);
      body.set('signature', signature);
      status.textContent = 'Saving the World entrance…';
      await send();
      saved = true;
      document.querySelector('[data-world-entrance]').textContent = body.get('base');
      document.querySelector('[data-world-arrival]').textContent = 'Visitors arrive in this scene.';
      for (const scene of document.querySelectorAll('[data-parcels]')) {
        scene.querySelector('[data-entrance-marker]').textContent = scene.dataset.parcels.split(' · ').includes(body.get('base')) ? ' · World entrance' : '';
      }
      status.textContent = 'World entrance updated to ' + body.get('base') + '.';
      location.reload();
    } catch (error) {
      status.textContent = error.message || String(error);
    } finally {
      button.disabled = saved;
    }
  });
})();
