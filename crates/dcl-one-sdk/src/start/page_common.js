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
const pageSyncHeader = (doc) => {
  for (const selector of ['.bar__acct', '#deploy-badge']) {
    const current = document.querySelector(selector);
    const next = doc.querySelector(selector);
    if (current && next && current.outerHTML !== next.outerHTML) current.replaceWith(next);
  }
};
const pageRememberWallet = async (address) => {
  if (!/^0x[0-9a-f]{40}$/i.test(address || '')) throw new Error('The wallet returned no account');
  const form = document.querySelector('.bar__acct form');
  if (!form) return;
  const token = form.querySelector('input[name="token"]').value;
  const action = form.getAttribute('action').replace(/\/target\/connect$/, '/target/address');
  const res = await fetch(action, {
    method: 'POST',
    headers: { 'content-type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({ token, address }),
  });
  if (!res.ok) throw new Error(await res.text());
  pageSyncHeader(parsePage(await res.text()));
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
      await pageRememberWallet(accounts[0]);
      if (!document.getElementById('run-status')) location.reload();
    } catch {
      pageToast('The wallet did not answer', true);
    }
  };
  document.addEventListener('click', (event) => {
    const wallet = event.target.closest('[data-wallet]');
    if (wallet && !wallet.disabled) connect(wallet);
  });
})();

(() => {
  if (document.getElementById('page-warming')) setTimeout(() => location.reload(), 1200);
})();
