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
  const toast = pageToast;

  const save = async (patch) => {
    const response = await fetch(api('/scene-json'), {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(patch),
    }).catch(() => null);
    if (!response || !response.ok) {
      toast(response ? (await response.text()).trim() : PAGE_OFFLINE, true);
      return false;
    }
    return true;
  };

  const GAP = () => (data.grid && data.grid.gap) || 3;
  let layTab = null;
  let layPick = false;
  let laySel = null;
  let layDraft = null;
  let layArmed = false;
  let layPitch = 22;
  let layDrag = null;
  let layEd = null;

  const layGrid = () => document.getElementById('lay-grid');
  const baseXY = () => (data.base || '0,0').split(',').map(Number);
  /* The schema allows number OR number[] of any length — [85] is a legal
     point — so mirror the server's coord_range instead of assuming a pair. */
  const nums = (v) => {
    const a = (Array.isArray(v) ? v : [v]).map((n) => Number(n ?? 0));
    return a.length ? a : [0];
  };
  const mid = (v) => nums(v).reduce((s, n) => s + n, 0) / nums(v).length;
  const num = (v) => Number(v) || 0;
  const lo = (v) => Math.min(...nums(v));
  const hi = (v) => Math.max(...nums(v));

  const layFromSpawn = (i) => {
    const s = (data.spawnPoints || [])[i];
    if (!s) return null;
    const p = s.position || {};
    const t = s.cameraTarget;
    return {
      idx: i,
      name: s.name || 'spawn',
      x0: lo(p.x), x1: hi(p.x), z0: lo(p.z), z1: hi(p.z), y: mid(p.y),
      lx: t ? mid(t.x) : '', ly: t ? mid(t.y) : '', lz: t ? mid(t.z) : '',
      def: Boolean(s.default),
    };
  };

  const layHintText = () => {
    if (layTab === 'info') {
      return 'Click the title, description, tags or cover to edit — changes save to scene.json';
    }
    if (layTab === 'parcels') {
      return layPick
        ? 'Click any parcel in the scene to make it the base parcel'
        : 'Drag across parcels to erase them, start on a dashed cell to paint new ones — the base parcel stays';
    }
    if (layTab === 'spawns') {
      if (layArmed) return 'Drag anywhere on the grid to draw the new spawn area';
      return layDraft
        ? 'Drag to redraw this area — it snaps to world metres, not to parcels'
        : 'Drag on the grid to draw an area, or click one to edit it';
    }
    return 'Permissions apply to the whole scene';
  };
  const layHint = () => {
    const hint = document.getElementById('lay-hint');
    if (hint) hint.textContent = layHintText();
  };

  const layParcelRect = (d) => {
    const [bx, by] = baseXY();
    const x0 = Math.floor((bx * 16 + Math.min(d.x0, d.x1)) / 16);
    const z0 = Math.floor((by * 16 + Math.min(d.z0, d.z1)) / 16);
    return {
      x0, z0,
      x1: Math.max(x0, Math.ceil((bx * 16 + Math.max(d.x0, d.x1)) / 16) - 1),
      z1: Math.max(z0, Math.ceil((by * 16 + Math.max(d.z0, d.z1)) / 16) - 1),
    };
  };
  const layOut = (d) => {
    const members = new Set(data.parcels);
    const r = layParcelRect(d);
    for (let x = r.x0; x <= r.x1; x++) {
      for (let z = r.z0; z <= r.z1; z++) {
        if (!members.has(x + ',' + z)) return true;
      }
    }
    return false;
  };

  /* The draft box is placed with the same --lay-step calc the server uses
     for saved areas, so a refit moves them all together. */
  const layDraftBox = () => {
    const g = layGrid();
    if (!g) return;
    let box = document.getElementById('lay-draft');
    for (const el of g.querySelectorAll('.lay__area[data-area]')) {
      el.hidden = Boolean(layDraft && layDraft.idx != null && Number(el.dataset.area) === layDraft.idx);
    }
    if (!layDraft) {
      if (box) box.remove();
      return;
    }
    if (!box) {
      box = document.createElement('div');
      box.id = 'lay-draft';
      box.className = 'lay__area lay__area--sel';
      const label = document.createElement('span');
      label.className = 'lay__area-name';
      box.append(label);
      g.append(box);
    }
    box.firstChild.textContent = layDraft.name || '';
    const b = data.grid;
    const [bx, by] = baseXY();
    const n = num;
    const x0 = Math.min(n(layDraft.x0), n(layDraft.x1));
    const x1 = Math.max(n(layDraft.x0), n(layDraft.x1));
    const z0 = Math.min(n(layDraft.z0), n(layDraft.z1));
    const z1 = Math.max(n(layDraft.z0), n(layDraft.z1));
    const m = (v) => 'calc(var(--lay-step)*' + v + '/16)';
    box.style.left = m((bx - b.x0) * 16 + x0);
    box.style.top = m((b.y1 + 1 - by) * 16 - z1);
    box.style.width = 'max(5px,' + m(x1 - x0) + ')';
    box.style.height = 'max(5px,' + m(z1 - z0) + ')';
  };

  const laySync = () => {
    layDraftBox();
    if (!layEd || !layDraft) return;
    const d = layDraft;
    const n = num;
    layEd.size.textContent =
      Math.abs(n(d.x1) - n(d.x0)) + ' × ' + Math.abs(n(d.z1) - n(d.z0)) + ' m';
    const r = layParcelRect(d);
    layEd.par.textContent =
      r.x0 === r.x1 && r.z0 === r.z1
        ? 'inside parcel ' + r.x0 + ',' + r.z0
        : 'covers parcels ' + r.x0 + ',' + r.z0 + ' → ' + r.x1 + ',' + r.z1;
    layEd.oob.hidden = !layOut(d);
  };
  const layEdSet = () => {
    if (!layEd || !layDraft) return;
    for (const k of ['x0', 'x1', 'z0', 'z1', 'y']) layEd[k].value = layDraft[k];
    laySync();
  };

  const laySpawnSave = async () => {
    const d = layDraft;
    if (!d) return;
    if (!String(d.name || '').trim()) {
      toast('A spawn area needs a name', true);
      return;
    }
    for (const k of ['x0', 'x1', 'z0', 'z1']) {
      if (d[k] === '' || !Number.isFinite(Number(d[k]))) {
        toast('The area needs both ends of its X and Z ranges', true);
        return;
      }
    }
    const looks = [d.lx, d.ly, d.lz];
    const looking = looks.some((v) => v !== '' && v != null);
    if (looking && looks.some((v) => v === '' || !Number.isFinite(Number(v)))) {
      toast('Looks-at needs all three coordinates, or none', true);
      return;
    }
    const span = (a, b) => {
      const p = Math.min(Number(a), Number(b));
      const q = Math.max(Number(a), Number(b));
      return p === q ? p : [p, q];
    };
    const entry = {
      name: String(d.name).trim(),
      position: { x: span(d.x0, d.x1), y: Number(d.y) || 0, z: span(d.z0, d.z1) },
    };
    if (looking) entry.cameraTarget = { x: Number(d.lx), y: Number(d.ly), z: Number(d.lz) };
    if (d.def) entry.default = true;
    const next = (data.spawnPoints || []).map((s) => JSON.parse(JSON.stringify(s)));
    if (d.def) for (const s of next) delete s.default;
    if (d.idx == null) next.push(entry);
    else next[d.idx] = entry;
    if (await save({ spawnPoints: next })) {
      laySel = d.idx == null ? next.length - 1 : d.idx;
      layDraft = null;
      layArmed = false;
      toast('Saved to scene.json');
      refresh();
    }
  };
  const laySpawnRemove = async () => {
    const d = layDraft;
    laySel = null;
    layDraft = null;
    layArmed = false;
    if (!d || d.idx == null) {
      layApply();
      return;
    }
    const next = (data.spawnPoints || []).map((s) => JSON.parse(JSON.stringify(s)));
    next.splice(d.idx, 1);
    if (next.length && !next.some((s) => s.default)) next[0].default = true;
    if (await save({ spawnPoints: next })) refresh();
    else layApply();
  };

  const layEditor = () => {
    const mount = document.getElementById('lay-sed');
    const empty = document.getElementById('lay-sempty');
    layEd = null;
    if (!mount) return;
    if (!layDraft) {
      mount.replaceChildren();
      if (empty) empty.hidden = false;
      return;
    }
    if (empty) empty.hidden = true;
    const d = layDraft;
    const box = document.createElement('div');
    box.className = 'lay__ed';
    const cap = (text) => {
      const s = document.createElement('span');
      s.className = 'knob__k';
      s.textContent = text;
      return s;
    };
    const tag = (cls, text) => {
      const s = document.createElement('span');
      s.className = cls;
      s.textContent = text;
      return s;
    };
    const field = (key, numeric) => {
      const input = document.createElement('input');
      input.type = numeric ? 'number' : 'text';
      if (numeric) input.step = 'any';
      input.value = d[key] == null ? '' : d[key];
      input.addEventListener('input', () => {
        if (numeric) {
          const v = input.valueAsNumber;
          d[key] = Number.isFinite(v) ? v : '';
        } else {
          d[key] = input.value;
        }
        laySync();
      });
      return input;
    };
    const name = field('name', false);
    box.append(cap('Name'), name);

    const areaCap = document.createElement('div');
    areaCap.className = 'lay__ed-cap';
    const size = tag('lay__ed-size', '');
    areaCap.append(cap('Area · world m'), size);
    box.append(areaCap);
    const f = {};
    for (const k of ['x0', 'x1', 'z0', 'z1', 'y']) f[k] = field(k, true);
    const grid = document.createElement('div');
    grid.className = 'lay__ed-grid';
    grid.append(
      tag('lay__ed-ax', 'X'), f.x0, tag('lay__ed-to', 'to'), f.x1,
      tag('lay__ed-ax', 'Z'), f.z0, tag('lay__ed-to', 'to'), f.z1,
      tag('lay__ed-ax', 'Y'), f.y, tag('lay__ed-gh', 'ground height')
    );
    box.append(grid);
    const par = document.createElement('div');
    par.className = 'note';
    box.append(par);
    const oob = document.createElement('div');
    oob.className = 'lay__oob';
    oob.textContent = 'Reaches outside the scene';
    oob.hidden = true;
    box.append(oob);

    box.append(cap('Looks at'));
    const looks = document.createElement('div');
    looks.className = 'lay__ed-3';
    for (const [k, label] of [['lx', 'X'], ['ly', 'Y'], ['lz', 'Z']]) {
      const wrap = document.createElement('div');
      wrap.className = 'lay__ed-look';
      wrap.append(tag('lay__ed-ax', label), field(k, true));
      looks.append(wrap);
    }
    box.append(looks);

    const defWrap = document.createElement('label');
    defWrap.className = 'lay__ed-def';
    const def = document.createElement('input');
    def.className = 'sw';
    def.type = 'checkbox';
    def.checked = Boolean(d.def);
    def.addEventListener('change', () => {
      d.def = def.checked;
    });
    defWrap.append(def, 'Default spawn');
    box.append(defWrap);

    const btns = document.createElement('div');
    btns.className = 'lay__ed-btns';
    const button = (label, cls, act) => {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = cls;
      b.textContent = label;
      b.addEventListener('click', act);
      btns.append(b);
    };
    button('Save', 'knob__go', laySpawnSave);
    if (d.idx != null) button('Remove', 'knob__go lay__ed-danger', laySpawnRemove);
    button('Cancel', 'knob__go', () => {
      laySel = null;
      layDraft = null;
      layArmed = false;
      layApply();
    });
    box.append(btns);
    mount.replaceChildren(box);
    layEd = { name, x0: f.x0, x1: f.x1, z0: f.z0, z1: f.z1, y: f.y, size, par, oob };
    laySync();
  };

  const layFit = () => {
    const map = document.querySelector('.lay__map');
    const b = data.grid;
    if (!map || !b) return;
    const cols = b.x1 - b.x0 + 1;
    const rows = b.y1 - b.y0 + 1;
    const narrow = matchMedia('(max-width: 860px)').matches;
    const paneW = Math.max(120, map.clientWidth - 27);
    const maxH = narrow ? 420 : 660;
    layPitch = Math.max(7, Math.min(26,
      Math.floor((paneW - GAP() * (cols - 1)) / cols),
      Math.floor((maxH - GAP() * (rows - 1)) / rows)));
    map.style.setProperty('--lay-pitch', layPitch + 'px');
    map.classList.toggle('lay--thin', layPitch < 12);
    const showAll = layPitch >= 15;
    const relabel = (span, v) => {
      span.textContent = span.dataset.n !== '' && (showAll || v % 5 === 0) ? span.dataset.n : '';
    };
    for (const s of map.querySelectorAll('.lay__ruler span')) relabel(s, Number(s.dataset.x));
    for (const s of map.querySelectorAll('.lay__nums span')) relabel(s, Number(s.dataset.y));
  };

  const layApply = () => {
    const card = document.getElementById('scene-layout');
    if (!card || !data.grid) return;
    if (layTab == null) {
      const first = card.querySelector('.lay__tab[aria-selected="true"]');
      layTab = (first && first.dataset.laytab) || 'info';
    }
    if (laySel != null && !(data.spawnPoints || [])[laySel]) {
      laySel = null;
      layDraft = null;
    }
    if (laySel != null && !layDraft) layDraft = layFromSpawn(laySel);
    for (const tab of card.querySelectorAll('.lay__tab')) {
      tab.setAttribute('aria-selected', String(tab.dataset.laytab === layTab));
    }
    for (const pane of card.querySelectorAll('.lay__pane')) {
      pane.hidden = pane.dataset.pane !== layTab;
    }
    card.classList.toggle('lay--info', layTab === 'info');
    card.classList.toggle('lay--parcels', layTab === 'parcels');
    card.classList.toggle('lay--spawns', layTab === 'spawns');
    card.classList.toggle('lay--pick', layPick);
    const pick = document.getElementById('lay-base-pick');
    if (pick) {
      pick.textContent = layPick ? 'Pick on grid' : 'Change';
      pick.setAttribute('aria-pressed', String(layPick));
    }
    const add = document.getElementById('lay-sadd');
    if (add) {
      add.textContent = layArmed ? 'Drawing — drag on the grid' : '+ Add spawn area';
      add.setAttribute('aria-pressed', String(layArmed));
    }
    for (const row of card.querySelectorAll('.lay__srow')) {
      row.classList.toggle('lay__srow--sel', Number(row.dataset.spawn) === laySel);
    }
    layEditor();
    layDraftBox();
    layFit();
    layHint();
  };

  const layWorldAt = (e) => {
    const g = layGrid();
    const b = data.grid;
    if (!g || !b) return null;
    const r = g.getBoundingClientRect();
    const step = layPitch + GAP();
    const [bx, by] = baseXY();
    const cx = (e.clientX - r.left) / step;
    const cy = (e.clientY - r.top) / step;
    const clamp = (v, a, c) => Math.max(a, Math.min(c, v));
    return {
      x: clamp(Math.round((b.x0 + cx - bx) * 16), (b.x0 - bx) * 16, (b.x1 + 1 - bx) * 16),
      z: clamp(Math.round((b.y1 + 1 - cy - by) * 16), (b.y0 - by) * 16, (b.y1 + 1 - by) * 16),
    };
  };

  const layPaint = (cell) => {
    if (!layDrag || !cell.dataset.cell || cell.dataset.cell === data.base) return;
    if (layDrag.kind === 'add' && cell.classList.contains('lay__cell--add')) {
      cell.classList.remove('lay__cell--add');
      cell.classList.add('lay__cell--in');
      layDrag.keys.add(cell.dataset.cell);
    }
    if (layDrag.kind === 'remove' && cell.classList.contains('lay__cell--in')) {
      cell.classList.remove('lay__cell--in');
      cell.classList.add('lay__cell--add');
      layDrag.keys.add(cell.dataset.cell);
    }
  };

  const layPickBase = async (key) => {
    layPick = false;
    if (key === data.base) {
      layApply();
      return;
    }
    if (await save({ base: key })) refresh();
    else layApply();
  };

  const enable = () => {
    document.body.classList.add('editing');
    for (const el of document.querySelectorAll(
      '#cover-input, .lay__tab, .lay__srow, #lay-sadd, #lay-base-pick, .lay__perm .sw'
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
    layApply();
  };

  let morphSeq = 0;
  const morph = async (url, track) => {
    const seq = ++morphSeq;
    const response = await fetch(url, { headers: { accept: 'text/html' } }).catch(
      () => null
    );
    if (seq !== morphSeq) return;
    if (!response || !response.ok) {
      toast(response ? (await response.text()).trim() : PAGE_OFFLINE, true);
      return;
    }
    const doc = parsePage(await response.text());
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

  /* Every handler below is delegated so a morph never needs to re-bind. */
  document.addEventListener('click', async (event) => {
    const t = event.target;
    const hit = (sel) => (t.closest ? t.closest(sel) : null);
    if (hit('#copy-link')) {
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
    const tab = hit('.lay__tab');
    if (tab && !tab.disabled) {
      layTab = tab.dataset.laytab;
      if (layTab !== 'parcels') layPick = false;
      if (layTab !== 'spawns') layArmed = false;
      layApply();
      return;
    }
    const pick = hit('#lay-base-pick');
    if (pick && !pick.disabled) {
      layPick = !layPick;
      layApply();
      return;
    }
    const srow = hit('.lay__srow');
    if (srow && !srow.disabled) {
      laySel = Number(srow.dataset.spawn);
      layDraft = null;
      layArmed = false;
      layApply();
      return;
    }
    const sadd = hit('#lay-sadd');
    if (sadd && !sadd.disabled) {
      layArmed = !layArmed;
      laySel = null;
      layDraft = null;
      layApply();
      return;
    }
    const tags = hit('#edit-tags');
    if (tags && !tags.querySelector('.tags-input')) {
      const input = document.createElement('input');
      input.className = 'tags-input';
      input.value = (data.tags || []).join(', ');
      input.placeholder = 'tags, separated by commas';
      tags.replaceChildren(input);
      input.focus();
    }
  });

  document.addEventListener('pointerdown', (event) => {
    if (!document.body.classList.contains('editing')) return;
    if (!(event.target.closest && event.target.closest('#lay-grid'))) return;
    if (layTab === 'parcels') {
      const cell = event.target.closest('.lay__cell');
      if (!cell || !cell.dataset.cell) return;
      event.preventDefault();
      if (layPick) {
        if (
          cell.classList.contains('lay__cell--in') ||
          cell.classList.contains('lay__cell--base')
        ) {
          layPickBase(cell.dataset.cell);
        }
        return;
      }
      if (cell.dataset.cell === data.base) {
        toast('The base parcel anchors the scene and stays', true);
        return;
      }
      if (cell.classList.contains('lay__cell--add')) layDrag = { kind: 'add', keys: new Set() };
      else if (cell.classList.contains('lay__cell--in')) layDrag = { kind: 'remove', keys: new Set() };
      else return;
      layPaint(cell);
    } else if (layTab === 'spawns') {
      const p = layWorldAt(event);
      if (!p) return;
      event.preventDefault();
      layDrag = {
        kind: 'spawn',
        anchor: p,
        moved: false,
        mode: layArmed ? 'new' : layDraft ? 'edit' : 'probe',
      };
    }
  });

  document.addEventListener('pointerover', (event) => {
    if (!layDrag || layDrag.kind === 'spawn') return;
    const cell = event.target.closest && event.target.closest('#lay-grid .lay__cell');
    if (cell) layPaint(cell);
  });

  document.addEventListener('pointermove', (event) => {
    if (!layDrag || layDrag.kind !== 'spawn') return;
    const p = layWorldAt(event);
    if (!p) return;
    const a = layDrag.anchor;
    if (!layDrag.moved && Math.abs(p.x - a.x) < 2 && Math.abs(p.z - a.z) < 2) return;
    layDrag.moved = true;
    const rect = {
      x0: Math.min(a.x, p.x), x1: Math.max(a.x, p.x),
      z0: Math.min(a.z, p.z), z1: Math.max(a.z, p.z),
    };
    if (layDrag.mode === 'edit' && layDraft) {
      Object.assign(layDraft, rect);
    } else {
      layDrag.mode = 'new';
      if (!layDraft || layDraft.idx != null) {
        laySel = null;
        layArmed = false;
        layDraft = Object.assign({
          idx: null,
          name: 'SpawnArea' + ((data.spawnPoints || []).length + 1),
          y: 0, lx: '', ly: '', lz: '',
          def: !(data.spawnPoints || []).length,
        }, rect);
        layApply();
      } else {
        Object.assign(layDraft, rect);
      }
    }
    if (layEd) layEdSet();
    else layDraftBox();
  });

  document.addEventListener('pointerup', async () => {
    const d = layDrag;
    if (!d) return;
    layDrag = null;
    if (d.kind === 'add' || d.kind === 'remove') {
      if (!d.keys.size) return;
      let parcels = data.parcels.slice();
      if (d.kind === 'add') {
        for (const k of d.keys) if (!parcels.includes(k)) parcels.push(k);
      } else {
        parcels = parcels.filter((p) => !d.keys.has(p));
      }
      await save({ parcels });
      refresh();
      return;
    }
    if (!d.moved) {
      const a = d.anchor;
      let found = null;
      (data.spawnPoints || []).forEach((s, i) => {
        const p = s.position || {};
        if (
          a.x >= lo(p.x) - 2 && a.x <= hi(p.x) + 2 &&
          a.z >= lo(p.z) - 2 && a.z <= hi(p.z) + 2
        ) {
          found = i;
        }
      });
      laySel = found;
      layDraft = null;
      layArmed = false;
    }
    layApply();
  });
  document.addEventListener('pointercancel', () => {
    layDrag = null;
  });
  window.addEventListener('resize', () => {
    layFit();
    layDraftBox();
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
        toast(response ? (await response.text()).trim() : PAGE_OFFLINE, true);
        return;
      }
      refresh();
      return;
    }
    if (t.classList && t.classList.contains('sw') && t.dataset.perm) {
      const key = t.dataset.perm;
      const granting = t.checked;
      const list = (data.permissions || []).filter((p) => p !== key);
      if (granting) list.push(key);
      if (!(await save({ requiredPermissions: list }))) {
        t.checked = !granting;
        return;
      }
      data.permissions = list;
      if (granting && !permWarned()) {
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
        refresh();
        return;
      }
      if (wrap) wrap.replaceChildren(...tagChips());
    }
  });

  enable();
})();
