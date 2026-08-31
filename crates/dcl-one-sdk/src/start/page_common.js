'use strict';
const PAGE_OFFLINE = 'The preview server did not answer';
const pageToast = (() => {
  let timer;
  return (message, isError, holdMs) => {
    let el = document.querySelector('.toast');
    if (!el) {
      el = document.createElement('div');
      el.className = 'toast';
      document.body.appendChild(el);
    }
    el.classList.toggle('toast--err', Boolean(isError));
    el.textContent = message;
    clearTimeout(timer);
    timer = setTimeout(() => el.remove(), holdMs || (isError ? 6000 : 2200));
  };
})();
/* DOMParser parses with scripting off, so noscript children come out as real
   nodes — morphing one in would hand the live page a working meta refresh. */
const parsePage = (html) => {
  const doc = new DOMParser().parseFromString(html, 'text/html');
  for (const n of doc.querySelectorAll('noscript')) n.remove();
  return doc;
};
/* The bar's wallet half: eth_requestAccounts, then the same gated POST the
   pasted-address route takes — the token rides in from the sibling DCL form,
   the prefix from its action. No signature: revealing an address is all a
   direct wallet connect can honestly claim. */
(() => {
  const wallet = document.getElementById('bar-wallet');
  if (!wallet) return;
  if (!window.ethereum) {
    wallet.disabled = true;
    wallet.title = 'No browser wallet found';
    return;
  }
  wallet.addEventListener('click', async () => {
    try {
      const accounts = await window.ethereum.request({ method: 'eth_requestAccounts' });
      const form = wallet.nextElementSibling;
      const token = form.querySelector('input[name="token"]').value;
      const action = form.getAttribute('action').replace(/\/target\/connect$/, '/target/address');
      const res = await fetch(action, {
        method: 'POST',
        headers: { 'content-type': 'application/x-www-form-urlencoded' },
        body: new URLSearchParams({ token, address: accounts[0] }),
      });
      if (res.ok) location.reload();
      else pageToast(await res.text(), true);
    } catch {
      pageToast('The wallet did not answer', true);
    }
  });
})();

// A page rendered before its remote answers arrived marks itself with
// #page-warming; the server is warming the caches in the background, so a
// short-fuse reload lands on the full render. Failure sentences are cached
// values too, so this always terminates.
(() => {
  if (document.getElementById('page-warming')) setTimeout(() => location.reload(), 1200);
})();
