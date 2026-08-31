(() => {
  'use strict';
  if (!document.getElementById('run-status')) return;

  const shape = (doc) => {
    const region = doc.getElementById('run-status');
    return region ? region.dataset.state + '|' + (region.dataset.signing || '') : '';
  };

  let pollTimer;
  const settle = () => {
    const region = document.getElementById('run-status');
    if (!region || region.dataset.state !== 'running') return;
    clearTimeout(pollTimer);
    pollTimer = setTimeout(refresh, 1800);
  };

  /* The wallet panel lives inside the polled region, so swapping it would
     wipe a prompt mid-answer. While a signer is live and the run shape is
     unchanged, the fresh render is identical anyway and is dropped. */
  const morph = (html) => {
    const doc = parsePage(html);
    const live = shape(document);
    const next = shape(doc);
    const signing = live.startsWith('running|') && live !== 'running|';
    if (window.__signBusy || (signing && live === next)) {
      settle();
      return;
    }
    /* Still waiting: update ONLY the feedback region, in place. Replacing the
       whole page every poll reflowed the card and the payload list around it,
       so the progress landed in a spot that jumped each tick. Swapping just
       #run-status keeps everything else fixed and loads the real feedback
       where it already sits. A genuine state change (idle<->running<->done)
       still renders the whole card, so the publish button and the final
       result arrive together. */
    const liveRegion = document.getElementById('run-status');
    const nextRegion = doc.getElementById('run-status');
    const bothRunning = live.startsWith('running|') && next.startsWith('running|');
    if (bothRunning && liveRegion && nextRegion) {
      liveRegion.replaceWith(nextRegion);
      signInit();
      settle();
      return;
    }
    const nextMain = doc.querySelector('main.dash');
    const liveMain = document.querySelector('main.dash');
    if (nextMain && liveMain) liveMain.replaceWith(nextMain);
    signInit();
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
  window.__signSettled = refresh;

  document.addEventListener('submit', async (event) => {
    const form = event.target.closest && event.target.closest('#publish');
    if (!form) return;
    event.preventDefault();
    const button = form.querySelector('.jn__cta');
    if (button) button.disabled = true;
    const response = await fetch(form.action, {
      method: 'POST',
      body: new URLSearchParams(new FormData(form)),
    }).catch(() => null);
    if (button) button.disabled = false;
    if (!response) {
      pageToast(PAGE_OFFLINE, true);
      return;
    }
    if (!response.ok) {
      pageToast((await response.text()).trim(), true);
      return;
    }
    morph(await response.text());
  });

  settle();
})();
