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
const parsePage = (html) => {
  const doc = new DOMParser().parseFromString(html, 'text/html');
  for (const n of doc.querySelectorAll('noscript')) n.remove();
  return doc;
};
(() => {
  const wallets = document.querySelectorAll('[data-wallet]');
  if (!wallets.length) return;
  if (!window.ethereum) {
    for (const wallet of wallets) {
      wallet.disabled = true;
      wallet.title = 'No browser wallet found';
    }
    return;
  }
  const connect = async (wallet) => {
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
  };
  for (const wallet of wallets) wallet.addEventListener('click', () => connect(wallet));
})();

(() => {
  if (document.getElementById('page-warming')) setTimeout(() => location.reload(), 1200);
})();
