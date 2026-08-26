/* /deploy's enhancement, in the landing script's mould: the server stays the
   single renderer. The script posts the same form the no-JS page posts, then
   re-fetches the page and swaps it in place, so a run reads as live status
   instead of navigations; while a run is in flight it polls. Without it the
   form POSTs plainly and a <noscript> meta refresh follows the run. */
(() => {
  'use strict';
  if (!document.getElementById('run-status')) return;

  let toastTimer;
  const toast = (message) => {
    let el = document.querySelector('.toast');
    if (!el) {
      el = document.createElement('div');
      el.className = 'toast toast--err';
      document.body.appendChild(el);
    }
    el.textContent = message;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => el.remove(), 6000);
  };

  const stateOf = () => {
    const region = document.getElementById('run-status');
    return region ? region.dataset.state : 'idle';
  };

  /* The signing page is opened from here rather than by the deploy process:
     the page owns the flow, and it opens the tab exactly once per run, on the
     first poll that knows the URL. A popup blocker leaves the panel's link. */
  let openedSigning = false;
  const maybeOpenSigning = () => {
    const region = document.getElementById('run-status');
    if (!region || region.dataset.state !== 'running') return;
    const url = region.dataset.signing;
    if (url && !openedSigning) {
      openedSigning = true;
      window.open(url, '_blank', 'noopener');
    }
  };

  let pollTimer;
  const settle = () => {
    maybeOpenSigning();
    if (stateOf() === 'running') {
      clearTimeout(pollTimer);
      pollTimer = setTimeout(refresh, 1800);
    }
  };

  /* Swapping the whole main would eat the server field mid-keystroke, so a
     poll that lands while the visitor types swaps only the status region. */
  const morph = (html) => {
    const doc = new DOMParser().parseFromString(html, 'text/html');
    const active = document.activeElement;
    const typing = active && active.tagName === 'INPUT' && active.type === 'text';
    const nextMain = doc.querySelector('main.dash');
    const liveMain = document.querySelector('main.dash');
    if (typing || !nextMain || !liveMain) {
      const next = doc.getElementById('run-status');
      const live = document.getElementById('run-status');
      if (next && live) live.replaceWith(next);
    } else {
      liveMain.replaceWith(nextMain);
    }
    settle();
  };

  let seq = 0;
  const refresh = async () => {
    const mine = ++seq;
    const response = await fetch(location.href, { headers: { accept: 'text/html' } }).catch(
      () => null
    );
    if (!response || !response.ok || mine !== seq) {
      settle();
      return;
    }
    morph(await response.text());
  };

  document.addEventListener('submit', async (event) => {
    const form = event.target.closest && event.target.closest('#publish');
    if (!form) return;
    event.preventDefault();
    const button = form.querySelector('.jn__cta');
    if (button) button.disabled = true;
    openedSigning = false;
    const response = await fetch(form.action, {
      method: 'POST',
      body: new URLSearchParams(new FormData(form)),
    }).catch(() => null);
    if (button) button.disabled = false;
    if (!response) {
      toast('The preview server did not answer');
      return;
    }
    if (!response.ok) {
      toast((await response.text()).trim());
      return;
    }
    /* The POST redirects back to the page, and fetch followed it: what came
       back IS the fresh page, so it morphs directly and the poll takes over. */
    morph(await response.text());
  });

  settle();
})();
