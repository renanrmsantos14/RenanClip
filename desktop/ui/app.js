'use strict';
const T = window.__TAURI__;
const invoke = (c, a) => T.core.invoke('rpc', { cmd: c, args: a || {} });
const $ = (s, r = document) => r.querySelector(s);
const el = (tag, props = {}, ...kids) => {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (k === 'class') n.className = v;
    else if (k.startsWith('on')) n.addEventListener(k.slice(2), v);
    else if (v === true) n.setAttribute(k, '');
    else if (v !== false && v != null) n.setAttribute(k, v);
  }
  for (const k of kids.flat()) if (k != null) n.append(k);
  return n;
};
// Todo conteúdo vindo da área de transferência entra por textContent (nunca innerHTML).

const IMG = (id) => (navigator.userAgent.includes('Windows') ? 'http://rc.localhost/' : 'rc://localhost/') + id;
const state = { tab: 'historico', kind: 'all', items: [], sel: 0, settings: null, info: null, popup: false, pairing: null };

// ───────── utilidades ─────────
let toastT;
function toast(msg, ms = 2800) {
  const t = $('#toast');
  t.textContent = msg; t.hidden = false;
  clearTimeout(toastT); toastT = setTimeout(() => (t.hidden = true), ms);
}
const err = (e) => toast(typeof e === 'string' ? e : (e && e.message) || 'Algo deu errado');
function ago(ts) {
  const s = Math.max(0, (Date.now() - ts) / 1000);
  if (s < 45) return 'agora';
  if (s < 3600) return Math.round(s / 60) + ' min';
  if (s < 86400) return Math.round(s / 3600) + ' h';
  return new Date(ts).toLocaleDateString('pt-BR', { day: '2-digit', month: '2-digit' });
}
function size(b) {
  if (b < 1024) return b + ' B';
  if (b < 1048576) return (b / 1024).toFixed(0) + ' KB';
  return (b / 1048576).toFixed(1) + ' MB';
}
const debounce = (f, ms) => { let t; return (...a) => { clearTimeout(t); t = setTimeout(() => f(...a), ms); }; };

// ───────── abas ─────────
function setTab(name) {
  state.tab = name;
  for (const b of document.querySelectorAll('nav [role=tab]')) b.setAttribute('aria-selected', String(b.dataset.tab === name));
  for (const s of document.querySelectorAll('.tab')) s.hidden = s.id !== 'tab-' + name;
  if (name === 'historico') { $('#q').focus(); loadHistory(); }
  if (name === 'aparelhos') { loadPeers(); openVisible(); }
  if (name === 'ajustes') renderSettings();
}
document.querySelectorAll('nav [role=tab]').forEach((b) => b.addEventListener('click', () => setTab(b.dataset.tab)));

// ───────── histórico ─────────
let histSeq = 0, peersSeq = 0; // só a resposta do pedido mais recente vale
async function loadHistory() {
  const my = ++histSeq;
  let items;
  try {
    items = await invoke('history', { query: $('#q').value.trim(), kind: state.kind });
  } catch (e) { return err(e); }
  if (my !== histSeq) return;
  state.items = items;
  if (state.sel >= state.items.length) state.sel = Math.max(0, state.items.length - 1);
  renderList();
}
const loadHistoryDebounced = debounce(() => { state.sel = 0; loadHistory(); }, 90);

function renderList() {
  const ul = $('#list');
  ul.replaceChildren();
  const empty = $('#empty');
  empty.hidden = state.items.length > 0;
  if (!state.items.length) {
    empty.textContent = $('#q').value ? 'Nada encontrado para essa busca.' : 'Nada copiado ainda. Copie algo em qualquer aparelho pareado e ele aparece aqui.';
  }
  state.items.forEach((it, i) => {
    const ico = it.kind === 'image' ? el('div', { class: 'ico' }, el('img', { src: IMG(it.id), alt: '', loading: 'lazy' })) : el('div', { class: 'ico' }, it.kind === 'file' ? 'ARQ' : 'TXT');
    const label = it.kind === 'text' ? (it.text || '') : it.kind === 'file' ? it.name : 'Imagem';
    const meta = [it.pinned ? '📌 fixado' : null, ago(it.ts), it.kind === 'text' ? null : size(it.size), it.origin ? null : null].filter(Boolean).join(' · ');
    const li = el('li', { role: 'option', 'aria-selected': String(i === state.sel), tabindex: '-1', class: i === state.sel ? 'sel' : '', 'data-i': i },
      ico,
      el('div', { class: 'body' }, el('div', { class: 'txt' }, label.slice(0, 400)), el('div', { class: 'meta' }, meta)),
      el('div', { class: 'acts' },
        el('button', { title: it.pinned ? 'Desafixar (Ctrl+P)' : 'Fixar (Ctrl+P)', class: it.pinned ? 'on' : '', 'aria-label': 'Fixar', onclick: (e) => { e.stopPropagation(); pin(it); } }, '📌'),
        el('button', { title: 'Apagar (Del)', 'aria-label': 'Apagar', onclick: (e) => { e.stopPropagation(); del(it); } }, '🗑')));
    li.addEventListener('click', () => { state.sel = i; pick(false); });
    ul.append(li);
  });
  scrollSel();
}
function select(i) {
  if (!state.items.length) return;
  state.sel = Math.max(0, Math.min(state.items.length - 1, i));
  document.querySelectorAll('#list li').forEach((li, k) => { li.classList.toggle('sel', k === state.sel); li.setAttribute('aria-selected', String(k === state.sel)); });
  scrollSel();
}
function scrollSel() { const li = $('#list li.sel'); if (li) li.scrollIntoView({ block: 'nearest' }); }
async function pick(onlyCopy) {
  const it = state.items[state.sel];
  if (!it) return;
  try { await invoke('copy_item', { id: it.id, paste: !onlyCopy, close: state.popup }); if (state.tab === 'historico' && !state.popup) toast(onlyCopy ? 'Copiado' : 'Copiado e colado'); } catch (e) { err(e); }
}
async function pin(it) { try { await invoke('pin', { id: it.id, pinned: !it.pinned }); loadHistory(); } catch (e) { err(e); } }
async function del(it) { try { await invoke('delete_item', { id: it.id }); loadHistory(); } catch (e) { err(e); } }

$('#q').addEventListener('input', loadHistoryDebounced);
$('#filters').addEventListener('click', (e) => {
  const b = e.target.closest('button'); if (!b) return;
  setKind(b.dataset.k);
});
function setKind(k) {
  state.kind = k; state.sel = 0;
  document.querySelectorAll('#filters button').forEach((x) => x.setAttribute('aria-pressed', String(x.dataset.k === k)));
  loadHistory();
}

// ───────── aparelhos ─────────
async function loadPeers() {
  let peers = [], near = [];
  const my = ++peersSeq;
  try { [peers, near] = await Promise.all([invoke('peers'), invoke('nearby')]); } catch (e) { return err(e); }
  if (my !== peersSeq) return;
  const online = peers.filter((p) => p.online).length;
  $('#dot').classList.toggle('on', online > 0);
  $('#dot').title = online + ' online';
  const ul = $('#peers'); ul.replaceChildren();
  if (!peers.length) ul.append(el('li', { class: 'muted small' }, 'Nenhum aparelho pareado ainda.'));
  for (const p of peers) {
    ul.append(el('li', { class: 'card' },
      el('span', { class: 'dot' + (p.online ? ' on' : ''), title: p.online ? 'online' : 'offline' }),
      el('div', { class: 'nm' }, el('b', {}, p.alias || p.name), el('span', { class: 'muted small' }, (p.alias ? p.original_name + ' · ' : '') + (p.online ? 'online' : 'offline') + (p.addr ? ' · ' + p.addr : ''))),
      el('button', { class: 'btn small', onclick: () => rename(p) }, 'Renomear'),
      el('button', { class: 'btn small danger', onclick: () => revoke(p) }, 'Revogar')));
  }
  const nu = $('#near'); nu.replaceChildren();
  const fresh = near.filter((n) => !n.paired);
  if (!fresh.length) nu.append(el('li', { class: 'muted small' }, 'Ninguém por perto no momento.'));
  for (const n of fresh) {
    nu.append(el('li', { class: 'card' },
      el('div', { class: 'nm' }, el('b', {}, n.name), el('span', { class: 'muted small' }, n.addr)),
      el('span', { class: 'tag' }, n.via === 'tailnet' ? 'Tailscale' : 'Rede local'),
      el('button', { class: 'btn small primary', onclick: () => startPair(n.addr) }, 'Parear')));
  }
}
function rename(p) {
  const inp = el('input', { type: 'text', value: p.alias || p.name, 'aria-label': 'Novo nome' });
  openModal({
    title: 'Renomear aparelho', text: 'Só você vê esse nome.', body: inp,
    actions: [['Cancelar'], ['Salvar', async () => { await invoke('rename_peer', { id: p.id, alias: inp.value.trim() }); loadPeers(); }, 'primary']],
  });
  setTimeout(() => inp.select(), 30);
}
function revoke(p) {
  openModal({
    title: 'Revogar "' + (p.alias || p.name) + '"?', text: 'Ele deixa de receber e enviar. Para voltar, será preciso parear de novo.',
    actions: [['Cancelar'], ['Revogar', async () => { await invoke('revoke_peer', { id: p.id }); loadPeers(); }, 'danger']],
  });
}
async function startPair(host) {
  try {
    const id = await invoke('pair_with', { host });
    state.pairing = id;
    openModal({ title: 'Conectando…', text: 'Aguardando o outro aparelho.', actions: [['Cancelar']] });
  } catch (e) { err(e); }
}
$('#addform').addEventListener('submit', (e) => {
  e.preventDefault();
  const v = $('#addhost').value.trim();
  if (!v) return toast('Digite o nome ou o IP do aparelho');
  startPair(v);
});
// Enquanto a aba Aparelhos está aberta, este PC aceita ser pareado (a confirmação do código continua obrigatória nos dois lados).
let visibleT;
async function openVisible() {
  try { await invoke('open_pairing', { secs: 120 }); } catch (e) { return err(e); }
  $('#visible-msg').textContent = 'Visível por 2 min. No outro aparelho, escolha este PC em "Por perto".';
  clearTimeout(visibleT); visibleT = setTimeout(() => ($('#visible-msg').textContent = ''), 120000);
}
$('#visible').addEventListener('click', openVisible);

function pairError(e = '') {
  if (/fill whole buffer|reset|broken pipe|eof/i.test(e)) return 'O outro aparelho não aceitou. Abra a aba Aparelhos nele e tente de novo.';
  if (/refused|recusad/i.test(e)) return 'Não consegui conectar. Confira se o RenanClip está aberto lá e se estão na mesma rede (ou no Tailscale).';
  return 'Pareamento não concluído' + (e ? ': ' + e : '');
}
// ───────── modal ─────────
let modalCancel = null;
function openModal({ title, text, sas, body, actions, onClose }) {
  $('#m-title').textContent = title;
  $('#m-text').textContent = text || '';
  const s = $('#m-sas'); s.hidden = !sas; s.textContent = sas || '';
  document.querySelectorAll('#m-body').forEach((n) => n.remove());
  if (body) { body.id = 'm-body'; $('#m-text').after(body); }
  const a = $('#m-actions'); a.replaceChildren();
  modalCancel = onClose || null;
  actions.forEach(([label, fn, cls], i) => {
    const b = el('button', { class: 'btn ' + (cls || ''), type: 'button' }, label);
    b.addEventListener('click', async () => {
      try { if (fn) await fn(); } catch (e) { err(e); }
      closeModal(true);
    });
    a.append(b);
    if (i === 0) setTimeout(() => (body ? body.focus() : b.focus()), 20);
  });
  $('#modal').hidden = false;
}
function closeModal(done) {
  $('#modal').hidden = true;
  if (!done && modalCancel) modalCancel();
  modalCancel = null;
  if (state.tab === 'historico') $('#q').focus();
}
$('#modal').addEventListener('mousedown', (e) => { if (e.target.id === 'modal') closeModal(false); });

function showPair(p) {
  state.pairing = p.pair_id;
  openModal({
    title: p.inbound ? '"' + p.peer_name + '" quer parear' : 'Parear com "' + p.peer_name + '"',
    text: 'Confira se este código é o MESMO que aparece no outro aparelho.', sas: p.sas.replace(/(\d{3})(\d{3})/, '$1 $2'),
    actions: [['Não é o mesmo', () => invoke('pair_decide', { pair_id: p.pair_id, accept: false }), 'danger'], ['É o mesmo', () => invoke('pair_decide', { pair_id: p.pair_id, accept: true }), 'primary']],
    onClose: () => invoke('pair_decide', { pair_id: p.pair_id, accept: false }),
  });
}
T.event.listen('pair-prompt', ({ payload: p }) => showPair(p));
T.event.listen('pair-done', ({ payload: p }) => {
  if (!$('#modal').hidden) closeModal(true);
  toast(p.ok ? '"' + p.peer_name + '" pareado' : pairError(p.error), 5000);
  loadPeers();
});
T.event.listen('history', () => { if (state.tab === 'historico') loadHistory(); });
T.event.listen('peers', () => { loadPeers(); });
T.event.listen('settings', () => { loadSettings(); });
T.event.listen('goto', ({ payload }) => setTab(payload));
T.event.listen('update-available', ({ payload }) => showUpdate(payload));

// ───────── ajustes ─────────
const FIELDS = [
  ['Geral', [
    ['device_name', 'text', 'Nome deste aparelho', 'Como ele aparece nos outros.'],
    ['theme', 'select', 'Tema', '', [['auto', 'Automático (sistema)'], ['claro', 'Claro'], ['escuro', 'Escuro']]],
    ['hotkey', 'hotkey', 'Atalho global', 'Abre o histórico em qualquer lugar. Clique e pressione as teclas.'],
    ['auto_paste', 'check', 'Colar direto ao escolher um item', 'Envia Ctrl+V para o programa em foco.'],
    ['autostart', 'check', 'Iniciar com o sistema'],
    ['auto_update', 'check', 'Atualizar automaticamente', 'Verifica o GitHub a cada 6 h e avisa quando houver versão nova.'],
  ]],
  ['Sincronização', [
    ['paused', 'check', 'Pausar sincronização', 'Não envia nem registra novas cópias.'],
    ['sync_images', 'check', 'Sincronizar imagens'],
    ['sync_files', 'check', 'Sincronizar arquivos pequenos'],
    ['max_blob_mb', 'number', 'Tamanho máximo de imagem/arquivo (MB)', '', [1, 100]],
    ['tailscale', 'check', 'Procurar aparelhos pelo Tailscale', 'Usa a rede da tailnet quando você está fora de casa. Sem Tailscale instalado, só a rede local.'],
  ]],
  ['Privacidade e espaço', [
    ['retention_days', 'number', 'Guardar histórico por (dias)', 'Itens fixados nunca expiram.', [1, 3650]],
    ['store_cap_mb', 'number', 'Espaço máximo para imagens e arquivos (MB)', '', [10, 10000]],
    ['ignored_apps', 'lines', 'Programas ignorados', 'Um por linha. Cópias vindas deles (ex.: gerenciadores de senha) não são guardadas nem enviadas.'],
  ]],
];
async function loadSettings() {
  try { state.settings = await invoke('get_settings'); } catch (e) { return err(e); }
  document.documentElement.dataset.theme = state.settings.theme === 'auto' ? '' : state.settings.theme;
  if (state.tab === 'ajustes') renderSettings();
}
function renderSettings() {
  const s = state.settings; if (!s) return;
  const f = $('#settings'); f.replaceChildren();
  for (const [title, fields] of FIELDS) {
    const fs = el('fieldset', {}, el('legend', {}, title));
    for (const [key, type, label, hint, extra] of fields) {
      let ctl;
      if (type === 'check') {
        ctl = el('label', { class: 'c' }, el('input', { type: 'checkbox', checked: !!s[key], 'data-k': key }), el('span', {}, label, hint ? el('div', { class: 'hint' }, hint) : null));
        fs.append(ctl); continue;
      }
      let input;
      if (type === 'select') input = el('select', { 'data-k': key }, extra.map(([v, t]) => el('option', { value: v, selected: s[key] === v }, t)));
      else if (type === 'number') input = el('input', { type: 'number', value: s[key], min: extra[0], max: extra[1], 'data-k': key });
      else if (type === 'lines') { input = el('textarea', { 'data-k': key, rows: 4 }); input.value = (s[key] || []).join('\n'); }
      else if (type === 'hotkey') { input = el('input', { type: 'text', value: s[key], readonly: true, 'data-k': key, placeholder: 'Clique e pressione as teclas' }); input.addEventListener('keydown', captureHotkey); }
      else input = el('input', { type: 'text', value: s[key], 'data-k': key });
      fs.append(el('label', { class: 'f' }, label, input, hint ? el('span', { class: 'hint' }, hint) : null));
    }
    f.append(fs);
  }
  f.append(el('div', { class: 'row gap' }, el('button', { class: 'btn', type: 'button', onclick: checkUpdate }, 'Procurar atualização'), el('button', { class: 'btn danger', type: 'button', onclick: clearAll }, 'Limpar histórico')));
}
function captureHotkey(e) {
  if (e.key === 'Tab') return;
  e.preventDefault();
  e.stopPropagation();
  if (e.key === 'Escape') return e.target.blur();
  if (e.key === 'Backspace' || e.key === 'Delete') { e.target.value = ''; return e.target.dispatchEvent(new Event('change', { bubbles: true })); }
  if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return;
  const parts = [];
  if (e.ctrlKey) parts.push('Ctrl'); if (e.altKey) parts.push('Alt'); if (e.shiftKey) parts.push('Shift'); if (e.metaKey) parts.push('Super');
  if (!parts.length) return toast('Use pelo menos uma tecla modificadora (Ctrl, Alt, Shift)');
  let k = e.code.startsWith('Key') ? e.code.slice(3) : e.code.startsWith('Digit') ? e.code.slice(5) : e.code;
  parts.push(k);
  e.target.value = parts.join('+');
  e.target.dispatchEvent(new Event('change', { bubbles: true }));
}
$('#settings').addEventListener('change', async (e) => {
  const k = e.target.dataset.k; if (!k) return;
  const s = { ...state.settings };
  const v = e.target.type === 'checkbox' ? e.target.checked : e.target.type === 'number' ? Number(e.target.value) : e.target.value;
  s[k] = k === 'ignored_apps' ? v.split('\n').map((x) => x.trim()).filter(Boolean) : v;
  try { state.settings = await invoke('save_settings', { s }); toast('Salvo', 1200); }
  catch (er) { err(er); state.settings = await invoke('get_settings'); }
  document.documentElement.dataset.theme = state.settings.theme === 'auto' ? '' : state.settings.theme;
  renderSettings();
});
function clearAll() {
  openModal({ title: 'Limpar histórico?', text: 'Apaga tudo, menos os itens fixados. Nos outros aparelhos também.', actions: [['Cancelar'], ['Limpar', async () => { const n = await invoke('clear_history'); toast(n + ' itens apagados'); loadHistory(); }, 'danger']] });
}

// ───────── atualização ─────────
function showUpdate(info) {
  $('#upd').hidden = false;
  $('#upd-txt').textContent = 'Versão ' + info.version + ' disponível';
  $('#upd-go').onclick = () => openModal({ title: 'Atualizar para ' + info.version + '?', text: (info.notes || '').slice(0, 300) + '\nO app reinicia.', actions: [['Depois'], ['Atualizar', async () => { toast('Baixando…', 20000); await invoke('install_update'); }, 'primary']] });
}
async function checkUpdate() {
  try { const i = await invoke('check_update'); if (i) showUpdate(i); else toast('Você já está na versão mais recente'); } catch (e) { err(e); }
}

// ───────── teclado ─────────
document.addEventListener('keydown', (e) => {
  if (!$('#modal').hidden) {
    if (e.key === 'Escape') { e.preventDefault(); closeModal(false); }
    return;
  }
  const inField = ['INPUT', 'TEXTAREA', 'SELECT'].includes(document.activeElement.tagName) && document.activeElement.id !== 'q';
  if (e.ctrlKey && ['1', '2', '3'].includes(e.key)) { e.preventDefault(); return setTab(['historico', 'aparelhos', 'ajustes'][+e.key - 1]); }
  if (e.key === 'Escape') {
    if (state.tab === 'historico' && $('#q').value) { $('#q').value = ''; loadHistory(); } else T.core.invoke('quit_ui');
    return;
  }
  if (state.tab !== 'historico' || inField) return;
  const k = e.key;
  const typing = document.activeElement.id === 'q' && $('#q').value !== ''; // Del/Ctrl+setas editam o texto
  if (typing && (k === 'Delete' || (e.ctrlKey && ['p', 'P', 'ArrowLeft', 'ArrowRight'].includes(k)))) return;
  if (k === 'ArrowDown') { e.preventDefault(); select(state.sel + 1); }
  else if (k === 'ArrowUp') { e.preventDefault(); select(state.sel - 1); }
  else if (k === 'PageDown') { e.preventDefault(); select(state.sel + 6); }
  else if (k === 'PageUp') { e.preventDefault(); select(state.sel - 6); }
  else if (k === 'Home' && e.ctrlKey) { e.preventDefault(); select(0); }
  else if (k === 'Enter') { e.preventDefault(); pick(e.shiftKey); }
  else if (k === 'Delete' && state.items[state.sel]) { e.preventDefault(); del(state.items[state.sel]); }
  else if (e.ctrlKey && k.toLowerCase() === 'p' && state.items[state.sel]) { e.preventDefault(); pin(state.items[state.sel]); }
  else if (e.ctrlKey && k === 'ArrowRight') { e.preventDefault(); cycleKind(1); }
  else if (e.ctrlKey && k === 'ArrowLeft') { e.preventDefault(); cycleKind(-1); }
  else if (e.altKey && /^[1-9]$/.test(k)) { e.preventDefault(); const n = +k - 1; if (state.items[n]) { state.sel = n; pick(false); } }
});
function cycleKind(d) {
  const ks = ['all', 'text', 'image', 'file'];
  setKind(ks[(ks.indexOf(state.kind) + d + ks.length) % ks.length]);
}

// ───────── início ─────────
(async function init() {
  try {
    state.info = await invoke('info');
    $('#ver').textContent = 'v' + state.info.version + ' · ' + state.info.built;
    $('#me').textContent = 'ID ' + state.info.id.slice(0, 8) + ' · porta ' + state.info.port;
    await loadSettings();
    const q = new URLSearchParams(location.search);
    const tab = q.get('tab') || 'historico';
    state.popup = q.get('popup') === '1';
    setTab(tab);
    loadPeers();
    const pend = await invoke('pending');
    if (pend.prompt) showPair(pend.prompt);
    if (pend.update) showUpdate(pend.update);
  } catch (e) { err(e); }
})();
