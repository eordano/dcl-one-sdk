(() => {
  'use strict';
  let data;
  const readData = () => {
    const tag = document.getElementById('edit-data');
    if (!tag) return false;
    data = JSON.parse(tag.textContent);
    return true;
  };
  if (!readData()) return;
  const api = (path) => (data.prefix || '') + path;
  const OFFLINE = 'The preview server did not answer';

  let toastTimer;
  const toast = (message, isError, holdMs) => {
    let el = document.querySelector('.toast');
    if (!el) {
      el = document.createElement('div');
      el.className = 'toast';
      document.body.appendChild(el);
    }
    el.classList.toggle('toast--err', Boolean(isError));
    el.textContent = message;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => el.remove(), holdMs || (isError ? 6000 : 2200));
  };

  const save = async (patch) => {
    const response = await fetch(api('/scene-json'), {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(patch),
    }).catch(() => null);
    if (!response || !response.ok) {
      toast(response ? (await response.text()).trim() : OFFLINE, true);
      return false;
    }
    return true;
  };

  const enable = () => {
    document.body.classList.add('editing');
    for (const el of document.querySelectorAll(
      '.chip[data-perm], .chip[data-spawn], #spawn-add, #cover-input'
    )) {
      el.disabled = false;
    }
    for (const id of ['edit-title', 'edit-desc']) {
      const el = document.getElementById(id);
      if (!el) continue;
      el.setAttribute('contenteditable', 'plaintext-only');
      el.spellcheck = false;
    }
    const copy = document.getElementById('copy-link');
    if (copy) copy.hidden = false;
    const tags = document.getElementById('edit-tags');
    if (tags) tags.title = 'Click to edit the tags';
    const apply = document.querySelector('form.side .knob__go');
    if (apply) apply.hidden = true;
  };

  let morphSeq = 0;
  /* The server stays the single renderer: after a change the page re-fetches
     itself and swaps the main element in place, never navigating. */
  const morph = async (url, track) => {
    const seq = ++morphSeq;
    const response = await fetch(url, { headers: { accept: 'text/html' } }).catch(
      () => null
    );
    if (seq !== morphSeq) return;
    if (!response || !response.ok) {
      toast(response ? (await response.text()).trim() : OFFLINE, true);
      return;
    }
    const doc = new DOMParser().parseFromString(await response.text(), 'text/html');
    const nextMain = doc.querySelector('main.dash');
    const liveMain = document.querySelector('main.dash');
    const nextData = doc.getElementById('edit-data');
    if (!nextMain || !liveMain || !nextData) return;
    liveMain.replaceWith(nextMain);
    const liveData = document.getElementById('edit-data');
    if (liveData) liveData.textContent = nextData.textContent;
    readData();
    document.title = doc.title;
    if (track) history.replaceState(null, '', url);
    enable();
  };
  const refresh = () => morph(location.href, false);

  const tagChips = () => {
    const chips = (data.tags || []).map((tag) => {
      const chip = document.createElement('span');
      chip.className = 'tag';
      chip.textContent = tag;
      return chip;
    });
    if (!chips.length) {
      const add = document.createElement('span');
      add.className = 'tag tag--add';
      add.textContent = '+ Add tags';
      chips.push(add);
    }
    return chips;
  };

  const permWarned = () => {
    const key = 'dclOneSdkPermissionWarned';
    try {
      if (localStorage.getItem(key)) return true;
      localStorage.setItem(key, '1');
    } catch {
      /* no storage: warn every time rather than never */
    }
    return false;
  };

  const mid = (v) => (Array.isArray(v) ? (Number(v[0]) + Number(v[1])) / 2 : Number(v ?? 0));
  const spawnEditor = (index) => {
    const mount = document.getElementById('spawn-editor');
    if (!mount) return;
    const spawns = data.spawnPoints || [];
    const existing = index < spawns.length;
    const spawn = existing
      ? spawns[index]
      : { name: 'spawn-' + (spawns.length + 1), position: { x: 8, y: 0, z: 8 } };

    const box = document.createElement('div');
    box.className = 'spawn-editor';
    const field = (label, type, value) => {
      const wrap = document.createElement('label');
      wrap.append(label);
      const input = document.createElement('input');
      input.type = type;
      if (type === 'number') input.step = 'any';
      input.value = value;
      wrap.append(input);
      box.append(wrap);
      return input;
    };
    const name = field('Name', 'text', spawn.name || '');
    const position = ['x', 'y', 'z'].map((axis) =>
      field(axis.toUpperCase(), 'number', mid(spawn.position && spawn.position[axis]))
    );
    const target = ['x', 'y', 'z'].map((axis) =>
      field(
        'Looks at ' + axis.toUpperCase(),
        'number',
        spawn.cameraTarget ? mid(spawn.cameraTarget[axis]) : ''
      )
    );
    const checkWrap = document.createElement('label');
    checkWrap.className = 'spawn-editor__chk';
    const isDefault = document.createElement('input');
    isDefault.type = 'checkbox';
    isDefault.checked = Boolean(spawn.default);
    checkWrap.append(isDefault, 'Default spawn');
    box.append(checkWrap);

    const buttons = document.createElement('div');
    buttons.className = 'spawn-editor__btns';
    const button = (label, act) => {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'knob__go';
      b.textContent = label;
      b.addEventListener('click', act);
      buttons.append(b);
    };
    const commit = async (next) => {
      if (await save({ spawnPoints: next })) refresh();
    };
    button('Save', async () => {
      if (!name.value.trim()) {
        toast('A spawn point needs a name', true);
        return;
      }
      const numbers = position.map((input) => input.valueAsNumber);
      if (numbers.some((n) => !Number.isFinite(n))) {
        toast('The position needs all three coordinates', true);
        return;
      }
      const looks = target.map((input) => input.valueAsNumber);
      const looking = target.some((input) => input.value.trim() !== '');
      if (looking && looks.some((n) => !Number.isFinite(n))) {
        toast('Looks-at needs all three coordinates, or none', true);
        return;
      }
      const entry = {
        name: name.value.trim(),
        position: { x: numbers[0], y: numbers[1], z: numbers[2] },
      };
      if (looking) entry.cameraTarget = { x: looks[0], y: looks[1], z: looks[2] };
      if (isDefault.checked) entry.default = true;
      const next = spawns.slice();
      if (isDefault.checked) for (const s of next) delete s.default;
      if (existing) next[index] = entry;
      else next.push(entry);
      commit(next);
    });
    if (existing) {
      button('Remove', () => {
        const next = spawns.slice();
        next.splice(index, 1);
        commit(next);
      });
    }
    button('Cancel', () => mount.replaceChildren());
    box.append(buttons);
    mount.replaceChildren(box);
    name.focus();
  };

  /* Every handler below is delegated so a morph never needs to re-bind. */
  document.addEventListener('click', async (event) => {
    const t = event.target;
    if (t.closest && t.closest('#copy-link')) {
      const link = document.getElementById('deep-link');
      if (!link) return;
      try {
        await navigator.clipboard.writeText(link.textContent);
      } catch {
        const range = document.createRange();
        range.selectNodeContents(link);
        const selection = getSelection();
        selection.removeAllRanges();
        selection.addRange(range);
        document.execCommand('copy');
      }
      toast('Deep link copied');
      return;
    }
    const cell = t.closest && t.closest('.map svg rect');
    if (cell) {
      if (cell.dataset.base) {
        toast('The base parcel anchors the scene and stays', true);
        return;
      }
      const added = cell.dataset.add;
      const removed = cell.dataset.parcel;
      if (!added && !removed) return;
      const parcels = added
        ? data.parcels.concat(added)
        : data.parcels.filter((p) => p !== removed);
      if (await save({ parcels })) refresh();
      return;
    }
    const perm = t.closest && t.closest('.chip[data-perm]');
    if (perm) {
      const key = perm.dataset.perm;
      const pressed = perm.getAttribute('aria-pressed') !== 'true';
      const list = (data.permissions || []).filter((p) => p !== key);
      if (pressed) list.push(key);
      if (!(await save({ requiredPermissions: list }))) return;
      data.permissions = list;
      perm.setAttribute('aria-pressed', String(pressed));
      perm.classList.toggle('perm--off', !pressed);
      if (pressed && !permWarned()) {
        toast(
          'Saved. Players may be asked to approve this permission when they enter the scene.',
          false,
          8000
        );
      } else {
        toast('Saved to scene.json');
      }
      refresh();
      return;
    }
    const spawnChip = t.closest && t.closest('.chip[data-spawn]');
    if (spawnChip) {
      spawnEditor(Number(spawnChip.dataset.spawn));
      return;
    }
    if (t.closest && t.closest('#spawn-add')) {
      spawnEditor((data.spawnPoints || []).length);
      return;
    }
    const tags = t.closest && t.closest('#edit-tags');
    if (tags && !tags.querySelector('.tags-input')) {
      const input = document.createElement('input');
      input.className = 'tags-input';
      input.value = (data.tags || []).join(', ');
      input.placeholder = 'tags, separated by commas';
      tags.replaceChildren(input);
      input.focus();
    }
  });

  document.addEventListener('change', async (event) => {
    const t = event.target;
    if (t.id === 'cover-input') {
      const file = t.files && t.files[0];
      if (!file) return;
      if (file.size > 2 * 1024 * 1024) {
        toast('A thumbnail caps at 2 MB', true);
        return;
      }
      const response = await fetch(api('/scene-thumbnail'), {
        method: 'POST',
        headers: { 'content-type': file.type },
        body: file,
      }).catch(() => null);
      if (!response || !response.ok) {
        toast(response ? (await response.text()).trim() : OFFLINE, true);
        return;
      }
      refresh();
      return;
    }
    const knobs = t.closest && t.closest('form.side');
    if (knobs) {
      const url = knobs.action + '?' + new URLSearchParams(new FormData(knobs));
      morph(url, true);
    }
  });

  const editBase = new Map();
  document.addEventListener('focusin', (event) => {
    const t = event.target;
    if (t.id === 'edit-title' || t.id === 'edit-desc') {
      editBase.set(t.id, t.textContent);
    }
  });

  document.addEventListener('keydown', (event) => {
    const t = event.target;
    if (t.id === 'edit-title' || t.id === 'edit-desc') {
      if (event.key === 'Escape') {
        t.textContent = editBase.get(t.id) ?? t.textContent;
        t.blur();
      }
      if (t.id === 'edit-title' && event.key === 'Enter') {
        event.preventDefault();
        t.blur();
      }
      return;
    }
    if (t.classList && t.classList.contains('tags-input')) {
      if (event.key === 'Enter') t.blur();
      if (event.key === 'Escape') {
        t.dataset.esc = '1';
        t.blur();
      }
    }
  });

  document.addEventListener('focusout', async (event) => {
    const t = event.target;
    if (t.id === 'edit-title' || t.id === 'edit-desc') {
      const text = t.textContent.trim();
      t.textContent = text;
      const last = editBase.get(t.id) ?? text;
      if (text === last.trim()) return;
      const patch = t.id === 'edit-title' ? { title: text } : { description: text };
      if (await save(patch)) {
        toast('Saved to scene.json');
        refresh();
      } else {
        t.textContent = last;
      }
      return;
    }
    if (t.classList && t.classList.contains('tags-input')) {
      const wrap = t.closest('#edit-tags');
      const list = t.value.split(',').map((s) => s.trim()).filter(Boolean);
      const changed = list.join(' ') !== (data.tags || []).join(' ');
      if (!t.dataset.esc && changed && (await save({ tags: list }))) {
        data.tags = list;
        toast('Saved to scene.json');
        if (wrap) wrap.replaceChildren(...tagChips());
        refresh();
        return;
      }
      if (wrap) wrap.replaceChildren(...tagChips());
    }
  });

  enable();
})();
