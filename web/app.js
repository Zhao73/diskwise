const $ = id => document.getElementById(id);

const I18N = {
  en: {
    biggest: 'Biggest anywhere', browse: 'Browse folders', filesOnly: 'Files only',
    allCategories: 'All categories', everything: 'everything', filterPath: 'filter by path…',
    reclaimable: 'reclaimable here', size: 'Size', name: 'Name', whatItIs: 'What it is',
    path: 'Path', rescan: 'Rescan', disk: 'Disk', processes: 'Processes',
    byCpu: 'By CPU', byMem: 'By memory', byUptime: 'By uptime', anyAge: 'any age',
    over12h: 'running > 12h', over1d: 'running > 1 day', over3d: 'running > 3 days',
    over7d: 'running > 7 days', onlyMine: 'only mine', filterProc: 'filter by name or path…',
    pid: 'PID', memory: 'Memory', uptime: 'Uptime', process: 'Process',
    quit: 'Quit', force: 'Force', protectedTag: 'protected',
    shown: 'shown', resident: 'resident', oldest: 'oldest',
    scanning: 'scanning', firstTime: '— this takes about a minute the first time…',
    rescanning: 'rescanning…', inFiles: 'in {n} files', empty: 'Nothing here above the size filter.',
    fullDisk: '{n} paths were unreadable. Grant Full Disk Access in System Settings → Privacy & Security to see all of ~/Library.',
    settings: 'Settings', language: 'Language', close: 'Close',
    policyTitle: 'Safety policy', policyFile: 'Config file', mode: 'Mode',
    ceiling: 'Unattended ceiling', archivesAt: 'Archives are kept in',
    modeConfirm: 'confirm — cleanup always needs a human',
    modeReadonly: 'readonly — nothing can be modified',
    modeAuto: 'auto — unattended within the allowlist',
    selectToClean: 'Select rows to clean', selected: '{n} selected', clean: 'Clean selected…',
    cancel: 'Cancel', planTitle: 'Review this plan', planNothing: 'Nothing in this selection is reclaimable.',
    wouldFree: 'Would free {s} across {n} paths.',
    trashNote: 'Deletions go to the Trash. Archives are verified before the original is released.',
    confirmBtn: 'Apply', applying: 'Applying…', freed: 'Freed {s}.', failed: 'failed',
    killConfirm: '{a} {name} (pid {pid})?', killForceNote: 'SIGKILL gives it no chance to save anything.',
    actArchive: 'archive', actTrash: 'trash', actReview: 'review', actNever: 'never',
    noRule: 'no rule', nFiles: '{n} files', lastWritten: 'last written {d} ({ago})',
    future: 'future timestamp', today: 'today', daysAgo: '{n} days ago', monthsAgo: '{n} months ago', yearsAgo: '{n}y ago',
    clickToOpen: 'click to open',
    askPlaceholder: 'Ask about this disk…', askBtn: 'Ask {agent}',
    askHint: 'runs the {agent} CLI you are signed into — it costs that quota',
    asking: 'asking {agent}…', askCost: '{agent} · {n} tokens of your quota',
    expand: 'Look inside', insideOf: 'Inside {name}',
    nothingInside: 'The tool that owns this could not be reached.',
  },
  zh: {
    biggest: '全盘最大', browse: '浏览文件夹', filesOnly: '只看文件',
    allCategories: '全部类别', everything: '不限大小', filterPath: '按路径筛选…',
    reclaimable: '此处可回收', size: '大小', name: '名称', whatItIs: '这是什么',
    path: '路径', rescan: '重新扫描', disk: '磁盘', processes: '进程',
    byCpu: '按 CPU', byMem: '按内存', byUptime: '按运行时长', anyAge: '不限时长',
    over12h: '运行超过 12 小时', over1d: '运行超过 1 天', over3d: '运行超过 3 天',
    over7d: '运行超过 7 天', onlyMine: '只看我的', filterProc: '按名称或路径筛选…',
    pid: '进程号', memory: '内存', uptime: '运行时长', process: '进程',
    quit: '退出', force: '强制', protectedTag: '受保护',
    shown: '项', resident: '常驻内存', oldest: '最久',
    scanning: '正在扫描', firstTime: '—— 首次约需一分钟…',
    rescanning: '重新扫描中…', inFiles: '共 {n} 个文件', empty: '此处没有超过筛选大小的内容。',
    fullDisk: '有 {n} 个路径无法读取。请在「系统设置 → 隐私与安全性 → 完全磁盘访问权限」中授权，才能看到完整的 ~/Library。',
    settings: '设置', language: '语言', close: '关闭',
    policyTitle: '安全策略', policyFile: '配置文件', mode: '模式',
    ceiling: '无人值守上限', archivesAt: '归档存放于',
    modeConfirm: 'confirm —— 清理始终需要人确认',
    modeReadonly: 'readonly —— 任何内容都不会被修改',
    modeAuto: 'auto —— 在白名单内可无人值守执行',
    selectToClean: '勾选要清理的行', selected: '已选 {n} 项', clean: '清理所选…',
    cancel: '取消', planTitle: '请确认这份计划', planNothing: '所选内容中没有可回收的项目。',
    wouldFree: '将释放 {s}，共 {n} 个路径。',
    trashNote: '删除一律进废纸篓。归档会先完整校验通过，才释放原文件。',
    confirmBtn: '执行', applying: '执行中…', freed: '已释放 {s}。', failed: '失败',
    killConfirm: '{a} {name}（进程号 {pid}）？', killForceNote: 'SIGKILL 不会给它任何保存机会。',
    actArchive: '归档', actTrash: '废纸篓', actReview: '待定', actNever: '受保护',
    noRule: '无规则', nFiles: '{n} 个文件', lastWritten: '最后修改 {d}（{ago}）',
    future: '时间戳在未来', today: '今天', daysAgo: '{n} 天前', monthsAgo: '{n} 个月前', yearsAgo: '{n} 年前',
    clickToOpen: '点击进入',
    askPlaceholder: '就这块磁盘提问…', askBtn: '问 {agent}',
    askHint: '调用你本机已登录的 {agent}，消耗的是它的额度',
    asking: '正在询问 {agent}…', askCost: '{agent} · 消耗你 {n} tokens',
    expand: '看看里面', insideOf: '{name} 内部',
    nothingInside: '无法连接到管理它的工具。',
  },
};
const CATEGORY_ZH = {
  'agent-session': 'AI 会话', 'agent-cache': 'AI 缓存', 'agent-artifact': 'AI 产物',
  'build': '构建产物', 'toolchain-cache': '工具链缓存', 'toolchain': '工具链',
  'simulator': '模拟器', 'vm': '虚拟机', 'protected': '受保护',
};
let LANG = (() => { try { return localStorage.getItem('diskwise-lang') || 'en'; } catch { return 'en'; } })();
const t = (k, vars) => {
  let v = (I18N[LANG] && I18N[LANG][k]) || I18N.en[k] || k;
  for (const [n, val] of Object.entries(vars || {})) v = v.replaceAll('{' + n + '}', val);
  return v;
};
const catLabel = c => LANG === 'zh' ? (CATEGORY_ZH[c] || c) : c;
const suggestLabel = sg => t('act' + sg[0].toUpperCase() + sg.slice(1));
const noteOf = v => (LANG === 'zh' && v.note_zh) ? v.note_zh : v.note;
const state = { mode: 'tree', dir: null, root: '', user: '', scanning: false, category: '', min: 10485760, contains: '' };
const fmt = b => {
  const u = ['B','KB','MB','GB','TB'];
  let i = 0; while (b >= 1000 && i < u.length - 1) { b /= 1000; i++; }
  return (i === 0 ? b : b.toFixed(b < 10 ? 2 : 1)) + ' ' + u[i];
};
const colorOf = cat => getComputedStyle(document.documentElement)
  .getPropertyValue('--c-' + (cat || 'none')).trim() || 'var(--c-none)';

async function loadStatus() {
  const s = await (await fetch('/api/status')).json();
  state.root = s.root;
  state.user = s.user;
  if (!state.dir) state.dir = s.root;
  $('summary').innerHTML = s.total === 0 && s.scanning
    ? `${t('scanning')} <b>${s.root}</b> ${t('firstTime')}`
    : `<b>${fmt(s.total)}</b> ${t('inFiles', { n: s.files.toLocaleString() })}
       · ${s.root} ${s.scanning ? `· <b>${t('rescanning')}</b>` : ''}`;
  $('warn').innerHTML = s.denied > 20
    ? `<div class="warn">${t('fullDisk', { n: s.denied })}</div>` : '';
  const sel = $('category');
  if (sel.options.length === 1) {
    for (const c of s.categories) sel.add(new Option(c, c));
  }
  // Pull the rows in as soon as a background scan finishes.
  if (s.scanning) setTimeout(loadStatus, 2000);
  else if (state.scanning) load();
  state.scanning = s.scanning;
}

async function load() {
  const p = new URLSearchParams({ min: state.min, limit: 200 });
  if (state.mode === 'dir') p.set('dir', state.dir);
  if (state.mode === 'files') p.set('files', 'true');
  if (state.category) p.set('category', state.category);
  if (state.contains) p.set('contains', state.contains);
  const data = await (await fetch('/api/rows?' + p)).json();
  $('reclaim').innerHTML = data.reclaimable
    ? `${t('reclaimable')}: <b style="color:var(--accent)">${fmt(data.reclaimable)}</b>` : '';
  renderCrumbs();
  renderTable(data.rows);
  renderTreemap(data.rows);
}

function renderCrumbs() {
  const el = $('crumbs');
  if (state.mode !== 'dir') { el.innerHTML = ''; return; }
  const rel = state.dir.startsWith(state.root) ? state.dir.slice(state.root.length) : state.dir;
  const parts = rel.split('/').filter(Boolean);
  let acc = state.root;
  const links = [`<a data-p="${state.root}">${state.root}</a>`];
  for (const part of parts) {
    acc += '/' + part;
    links.push(`<a data-p="${acc}">${part}</a>`);
  }
  el.innerHTML = links.join('<span>/</span>');
  el.querySelectorAll('a').forEach(a => a.onclick = () => { state.dir = a.dataset.p; load(); });
}

function renderTable(rows) {
  $('empty').hidden = rows.length > 0;
  const max = rows.length ? rows[0].size : 1;
  $('rows').innerHTML = rows.map(r => {
    const v = r.verdict;
    const c = colorOf(v && v.category);
    const what = v
      ? `<span class="tag" style="background:${c}22;color:${c}">${catLabel(v.category)} · ${suggestLabel(v.suggest)}</span>
         <div class="note">${escapeHtml(noteOf(v))}</div>`
      : `<span class="tag" style="background:#ffffff10;color:var(--dim)">${t('noRule')}</span>
         <div class="note">${facts(r)}</div>`;
    // Only rows the rules call reclaimable can be selected; everything else has
    // no checkbox at all, so there is nothing to tick by mistake.
    const pickable = r.is_dir && v && (v.suggest === 'trash' || v.suggest === 'archive');
    return `<tr class="${r.is_dir ? 'dir' : 'file'}">
      <td class="pick">${pickable
        ? `<input type="checkbox" data-path="${escapeHtml(r.path)}"${sel.has(r.path) ? ' checked' : ''}>`
        : ''}</td>
      <td class="size">${r.human}</td>
      <td class="gauge"><div style="width:${Math.max(2, r.size / max * 100)}%;background:${c}"></div></td>
      <td class="name" data-p="${r.is_dir ? r.path : ''}">
        <span class="kind">${r.is_dir ? '📁' : '📄'}</span>${escapeHtml(r.name)}</td>
      <td>${what}${v && v.inspect
        ? `<div><button class="inspect" data-p="${escapeHtml(r.path)}" data-k="${v.inspect}">${t('expand')}</button></div>`
        : ''}</td>
      <td class="path">${escapeHtml(r.path)}</td>
    </tr>`;
  }).join('');
  bindCheckboxes();
  $('rows').querySelectorAll('button.inspect').forEach(b => {
    b.onclick = async e => {
      e.stopPropagation();
      $('a-title').textContent = t('insideOf', { name: b.closest('tr').querySelector('td.name').textContent.trim() });
      $('a-body').innerHTML = `<span class="spin">◐</span>`;
      $('askmodal').hidden = false;
      const r = await fetch(`/api/inspect?path=${encodeURIComponent(b.dataset.p)}&kind=${b.dataset.k}`);
      if (!r.ok) { $('a-body').textContent = await r.text(); return; }
      const i = await r.json();
      $('a-body').innerHTML = (i.rows.length
        ? `<ul>${i.rows.map(x => `<li><span class="m">${escapeHtml(x.size)}</span> · ${escapeHtml(x.label)}
             <span class="note">${escapeHtml(x.detail)}</span></li>`).join('')}</ul>`
        : `<p class="note">${t('nothingInside')}</p>`) +
        `<div class="cost">${escapeHtml(i.note)}</div>`;
      $('a-close').textContent = t('close');
    };
  });
  $('rows').querySelectorAll('td.name[data-p]:not([data-p=""])').forEach(td => {
    td.onclick = () => { state.mode = 'dir'; setMode(); state.dir = td.dataset.p; load(); };
  });
}

// ------------------------------------------------------------ cleaning

const sel = new Set();

function refreshSelection() {
  $('cleanbar').classList.toggle('on', sel.size > 0);
  $('selcount').textContent = t('selected', { n: sel.size });
  $('cleango').textContent = t('clean');
  $('selclear').textContent = t('cancel');
}

function bindCheckboxes() {
  $('rows').querySelectorAll('input[type=checkbox][data-path]').forEach(cb => {
    cb.onclick = e => e.stopPropagation();
    cb.onchange = () => {
      cb.checked ? sel.add(cb.dataset.path) : sel.delete(cb.dataset.path);
      refreshSelection();
    };
  });
}

$('selclear').onclick = () => { sel.clear(); refreshSelection(); load(); };

$('cleango').onclick = async () => {
  const res = await fetch('/api/plan', {
    method: 'POST', headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ paths: [...sel] }),
  });
  if (!res.ok) return alert(await res.text());
  showPlan(await res.json());
};

// Two steps, always: this shows what would happen, and only the button in here
// actually does it.
function showPlan(plan) {
  const total = plan.items.reduce((a, i) => a + i.size, 0);
  $('p-title').textContent = t('planTitle');
  $('p-cancel').textContent = t('cancel');
  $('p-go').textContent = t('confirmBtn');
  $('p-go').disabled = plan.items.length === 0;
  $('p-body').innerHTML = plan.items.length === 0
    ? `<p class="note">${t('planNothing')}</p>`
    : `<ul>${plan.items.map(i => `<li><span class="m">${fmt(i.size)}</span>
         · ${suggestLabel(i.action)} · ${escapeHtml(i.path)}</li>`).join('')}</ul>
       <p><b>${t('wouldFree', { s: fmt(total), n: plan.items.length })}</b></p>
       <p class="note">${t('trashNote')}</p>`;
  $('planmodal').hidden = false;
  $('p-go').onclick = () => applyPlan(plan.id);
}

async function applyPlan(planId) {
  $('p-go').disabled = true;
  $('p-go').textContent = t('applying');
  const res = await fetch('/api/confirm', {
    method: 'POST', headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ plan_id: planId }),
  });
  if (!res.ok) { alert(await res.text()); $('p-go').disabled = false; return; }
  const outcomes = await res.json();
  const freed = outcomes.reduce((a, o) => a + o.freed, 0);
  const failures = outcomes.filter(o => o.error);
  $('p-body').innerHTML = `<p><b>${t('freed', { s: fmt(freed) })}</b></p>` +
    (failures.length
      ? `<ul>${failures.map(f => `<li>${t('failed')}: ${escapeHtml(f.path)} — ${escapeHtml(f.error)}</li>`).join('')}</ul>`
      : '');
  $('p-go').hidden = true;
  $('p-cancel').textContent = t('close');
  sel.clear();
  refreshSelection();
  loadStatus();
}

$('p-cancel').onclick = () => {
  $('planmodal').hidden = true;
  $('p-go').hidden = false;
  $('p-go').disabled = false;
  $('p-go').textContent = t('confirmBtn');
  load();
};

// No rule matched — so state facts instead of inventing a description. A big
// folder diskwise doesn't recognise is almost always your own data.
function facts(r) {
  const bits = [];
  if (r.is_dir) bits.push(t('nFiles', { n: r.files.toLocaleString() }));
  if (r.newest > 0) bits.push(t('lastWritten', { d: dateStr(r.newest), ago: ago(r.newest) }));
  return bits.join(' · ');
}

function dateStr(secs) {
  return new Date(secs * 1000).toLocaleDateString(LANG === 'zh' ? 'zh-CN' : 'en-CA');
}

function ago(secs) {
  const days = Math.floor((Date.now() / 1000 - secs) / 86400);
  // Some installers write timestamps years into the future. Saying "today"
  // about a 2029 date is worse than admitting the clock is wrong.
  if (days < 0) return t('future');
  if (days < 1) return t('today');
  if (days < 30) return t('daysAgo', { n: days });
  const months = Math.floor(days / 30);
  return months < 12 ? t('monthsAgo', { n: months }) : t('yearsAgo', { n: Math.floor(days / 365) });
}

function escapeHtml(s) {
  return s.replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
}

// Squarified treemap: keeps rectangles close to square so small items stay clickable.
function squarify(items, x, y, w, h, out) {
  if (!items.length) return out;
  const total = items.reduce((a, i) => a + i.size, 0);
  if (total <= 0 || w < 1 || h < 1) return out;
  const horizontal = w >= h;
  const side = horizontal ? h : w;
  let row = [], rowSum = 0, best = Infinity;
  const worst = (sum, side, mn, mx) => {
    const s2 = sum * sum, sd2 = side * side;
    return Math.max((sd2 * mx) / s2, s2 / (sd2 * mn));
  };
  let i = 0;
  for (; i < items.length; i++) {
    const next = rowSum + items[i].size;
    const scale = (horizontal ? w : h) * side / total;
    const mn = Math.min(...row.map(r => r.size), items[i].size) * scale;
    const mx = Math.max(...row.map(r => r.size), items[i].size) * scale;
    const w2 = worst(next * scale, side, mn, mx);
    if (row.length && w2 > best) break;
    row.push(items[i]); rowSum = next; best = w2;
  }
  const frac = rowSum / total;
  const thick = (horizontal ? w : h) * frac;
  let off = 0;
  for (const item of row) {
    const len = side * (item.size / rowSum);
    out.push(horizontal
      ? { ...item, x, y: y + off, w: thick, h: len }
      : { ...item, x: x + off, y, w: len, h: thick });
    off += len;
  }
  return horizontal
    ? squarify(items.slice(i), x + thick, y, w - thick, h, out)
    : squarify(items.slice(i), x, y + thick, w, h - thick, out);
}

// One tooltip for every chart on the page: name, where it lives, what it costs.
const tip = $('tip');
function showTip(e, html) {
  tip.innerHTML = html;
  tip.style.display = 'block';
  const pad = 14, w = tip.offsetWidth, h = tip.offsetHeight;
  let x = e.clientX + pad, y = e.clientY + pad;
  if (x + w > innerWidth) x = e.clientX - w - pad;
  if (y + h > innerHeight) y = e.clientY - h - pad;
  tip.style.left = x + 'px'; tip.style.top = y + 'px';
}
function hideTip() { tip.style.display = 'none'; }
function bindTip(el, html) {
  el.onmousemove = e => showTip(e, html);
  el.onmouseleave = hideTip;
}

function renderTreemap(rows) {
  const svg = $('treemap');
  const W = svg.clientWidth || 900, H = 300;
  svg.setAttribute('viewBox', `0 0 ${W} ${H}`);
  const items = rows.slice(0, 40).filter(r => r.size > 0).map((r, idx) => ({ ...r, idx }));
  const boxes = squarify(items, 0, 0, W, H, []);
  svg.innerHTML = boxes.map(b => {
    const c = colorOf(b.verdict && b.verdict.category);
    const label = b.w > 60 && b.h > 22
      ? `<text x="${b.x + 6}" y="${b.y + 16}">${escapeHtml(b.name.slice(0, Math.floor(b.w / 7)))}</text>
         ${b.h > 36 ? `<text class="sub" x="${b.x + 6}" y="${b.y + 30}">${b.human}</text>` : ''}`
      : '';
    return `<g><rect x="${b.x + .5}" y="${b.y + .5}" width="${Math.max(0, b.w - 1)}"
      height="${Math.max(0, b.h - 1)}" fill="${c}" rx="2" data-i="${b.idx}"
      data-p="${b.is_dir ? b.path : ''}"></rect>${label}</g>`;
  }).join('');
  svg.querySelectorAll('rect').forEach(r => {
    const b = boxes[+r.dataset.i];
    bindTip(r, `<b>${escapeHtml(b.name)}</b>
      <div class="p">${escapeHtml(b.path)}</div>
      <div class="m">${b.human}${b.is_dir ? ` · ${b.files.toLocaleString()} files` : ' · file'}</div>
      ${b.verdict ? `<div>${catLabel(b.verdict.category)} · <b style="display:inline">${suggestLabel(b.verdict.suggest)}</b></div>
        <div class="p">${escapeHtml(noteOf(b.verdict))}</div>` : ''}
      ${b.verdict ? '' : `<div class="p">${t('noRule')} · ${facts(b)}</div>`}
      ${b.is_dir ? `<div class="p">${t('clickToOpen')}</div>` : ''}`);
    if (r.dataset.p) r.onclick = () => {
      hideTip(); state.mode = 'dir'; setMode(); state.dir = r.dataset.p; load();
    };
  });
}

// ---------------------------------------------------------------- processes

const pstate = { sort: 'cpu', days: 0, mine: true, find: '', rows: [], timer: null };

async function loadProcs() {
  const all = await (await fetch('/api/procs')).json();
  pstate.rows = all;
  renderProcs();
}

function visibleProcs() {
  let rows = pstate.rows;
  if (pstate.mine) rows = rows.filter(p => p.user === state.user);
  if (pstate.days) rows = rows.filter(p => p.uptime >= pstate.days * 86400);
  if (pstate.find) {
    const n = pstate.find.toLowerCase();
    rows = rows.filter(p => (p.name + ' ' + p.command).toLowerCase().includes(n));
  }
  const key = { cpu: p => p.cpu, mem: p => p.rss, up: p => p.uptime }[pstate.sort];
  return [...rows].sort((a, b) => key(b) - key(a)).slice(0, 60);
}

function renderProcs() {
  const rows = visibleProcs();
  const mem = rows.reduce((a, p) => a + p.rss, 0);
  const oldest = rows.reduce((a, p) => Math.max(a, p.uptime), 0);
  $('psummary').innerHTML = `${rows.length} ${t('shown')} · <b>${fmt(mem)}</b> ${t('resident')} ·
    ${t('oldest')} <b>${humanDur(oldest)}</b>`;

  const maxCpu = Math.max(1, ...rows.map(p => p.cpu));
  $('prows').innerHTML = rows.map((p, i) => `<tr data-i="${i}">
    <td class="num">${p.pid}</td>
    <td class="num">${p.cpu.toFixed(1)}</td>
    <td class="gauge"><div style="width:${Math.max(2, p.cpu / maxCpu * 100)}%;
      background:${p.uptime > 86400 ? 'var(--c-agent-session)' : 'var(--c-build)'}"></div></td>
    <td class="num">${fmt(p.rss)}</td>
    <td class="num">${p.uptime_human}</td>
    <td>${escapeHtml(p.name)}<div class="note">${escapeHtml(p.command.slice(0, 90))}</div></td>
    <td class="act">${p.protected
      ? `<span class="locked">${t('protectedTag')}</span>`
      : `<button data-pid="${p.pid}">${t('quit')}</button>
         <button class="danger" data-pid="${p.pid}" data-force="1">${t('force')}</button>`}</td>
  </tr>`).join('');

  $('prows').querySelectorAll('tr').forEach(tr => {
    const p = rows[+tr.dataset.i];
    bindTip(tr, `<b>${escapeHtml(p.name)}</b>
      <div class="p">${escapeHtml(p.path || '(no path)')}</div>
      <div class="m">pid ${p.pid} · ${p.cpu.toFixed(1)}% cpu · ${fmt(p.rss)} · up ${p.uptime_human}</div>
      <div class="p">user ${escapeHtml(p.user)} · parent pid ${p.ppid}</div>
      <div class="p">${escapeHtml(p.command)}</div>`);
  });
  $('prows').querySelectorAll('button[data-pid]').forEach(b => {
    b.onclick = async () => {
      const force = b.dataset.force === '1';
      const p = pstate.rows.find(x => x.pid === +b.dataset.pid);
      const ask = t('killConfirm', { a: force ? t('force') : t('quit'), name: p.name, pid: p.pid });
      if (!confirm(ask + (force ? '\n\n' + t('killForceNote') : ''))) return;
      const r = await fetch(`/api/kill?pid=${p.pid}&force=${force}`, { method: 'POST' });
      if (!r.ok) alert(await r.text());
      hideTip();
      setTimeout(loadProcs, 400);
    };
  });
  renderCpuMap(rows);
}

// Same treemap, sized by memory so the biggest resident hogs are obvious.
function renderCpuMap(rows) {
  const svg = $('cpumap');
  const W = svg.clientWidth || 900, H = 200;
  svg.setAttribute('viewBox', `0 0 ${W} ${H}`);
  const items = rows.slice(0, 30).filter(p => p.rss > 0)
    .map((p, idx) => ({ ...p, size: p.rss, idx }));
  const boxes = squarify(items, 0, 0, W, H, []);
  svg.innerHTML = boxes.map(b => {
    const c = b.uptime > 86400 ? 'var(--c-agent-session)'
      : b.cpu > 20 ? 'var(--c-protected)' : 'var(--c-build)';
    const label = b.w > 60 && b.h > 22
      ? `<text x="${b.x + 6}" y="${b.y + 16}">${escapeHtml(b.name.slice(0, Math.floor(b.w / 7)))}</text>
         ${b.h > 36 ? `<text class="sub" x="${b.x + 6}" y="${b.y + 30}">${fmt(b.rss)} · ${b.uptime_human}</text>` : ''}`
      : '';
    return `<g><rect x="${b.x + .5}" y="${b.y + .5}" width="${Math.max(0, b.w - 1)}"
      height="${Math.max(0, b.h - 1)}" fill="${c}" rx="2" data-i="${b.idx}"></rect>${label}</g>`;
  }).join('');
  svg.querySelectorAll('rect').forEach(r => {
    const b = boxes.find(x => x.idx === +r.dataset.i);
    bindTip(r, `<b>${escapeHtml(b.name)}</b>
      <div class="p">${escapeHtml(b.path || '(no path)')}</div>
      <div class="m">${fmt(b.rss)} resident · ${b.cpu.toFixed(1)}% cpu · up ${b.uptime_human}</div>
      <div class="p">${escapeHtml(b.command)}</div>`);
  });
}

function humanDur(s) {
  const d = Math.floor(s / 86400), h = Math.floor(s % 86400 / 3600), m = Math.floor(s % 3600 / 60);
  return d ? `${d}d ${h}h` : h ? `${h}h ${m}m` : `${m}m`;
}

// Deep-linkable views: ?days=3&find=Chrome#processes shares exactly what you see.
function applyUrlParams() {
  const q = new URLSearchParams(location.search);
  if (q.has('find')) { pstate.find = q.get('find'); $('pfind').value = pstate.find; }
  if (q.has('days')) { pstate.days = +q.get('days'); $('pdays').value = q.get('days'); }
  if (q.has('mine')) { pstate.mine = q.get('mine') !== 'false'; $('pmine').checked = pstate.mine; }
  if (q.has('sort')) { pstate.sort = q.get('sort'); }
  if (q.has('lang')) { LANG = q.get('lang'); $('s-lang').value = LANG; applyLang(); }
  if (q.has('min')) { state.min = +q.get('min'); $('min').value = q.get('min'); }
  if (q.has('category')) { state.category = q.get('category'); $('category').value = state.category; }
  if (q.has('dir')) { state.dir = q.get('dir'); state.mode = 'dir'; setMode(); }
  if (q.has('files')) { state.mode = 'files'; setMode(); }
}

function showTab(which) {
  $('tab-disk').classList.toggle('on', which === 'disk');
  $('tab-proc').classList.toggle('on', which === 'proc');
  $('panel-disk').hidden = which !== 'disk';
  $('panel-proc').hidden = which !== 'proc';
  clearInterval(pstate.timer);
  if (which === 'proc') {
    loadProcs();
    pstate.timer = setInterval(loadProcs, 5000);
  }
}
$('tab-disk').onclick = () => { location.hash = 'disk'; showTab('disk'); };
$('tab-proc').onclick = () => { location.hash = 'processes'; showTab('proc'); };
addEventListener('hashchange', () => showTab(location.hash === '#processes' ? 'proc' : 'disk'));
for (const [k, id] of [['cpu', 'sort-cpu'], ['mem', 'sort-mem'], ['up', 'sort-up']]) {
  $(id).onclick = () => {
    pstate.sort = k;
    for (const x of ['sort-cpu', 'sort-mem', 'sort-up']) $(x).classList.toggle('on', $(x) === $(id));
    renderProcs();
  };
}
$('pdays').onchange = e => { pstate.days = +e.target.value; renderProcs(); };
$('pmine').onchange = e => { pstate.mine = e.target.checked; renderProcs(); };
let pt; $('pfind').oninput = e => {
  clearTimeout(pt); pt = setTimeout(() => { pstate.find = e.target.value; renderProcs(); }, 200);
};

// ------------------------------------------------------- ask an agent

let AGENTS = [];

async function loadAgents() {
  try { AGENTS = await (await fetch('/api/agents')).json(); } catch { AGENTS = []; }
  $('askbar').hidden = AGENTS.length === 0;
  applyAskLang();
}

function applyAskLang() {
  if (!AGENTS.length) return;
  $('askq').placeholder = t('askPlaceholder');
  $('askgo').textContent = t('askBtn', { agent: AGENTS[0] });
  $('askhint').textContent = t('askHint', { agent: AGENTS[0] });
  $('a-close').textContent = t('close');
}

$('askgo').onclick = async () => {
  const q = $('askq').value.trim();
  if (!q) return;
  $('a-title').textContent = q;
  $('a-body').innerHTML = `<span class="spin">◐</span> ${t('asking', { agent: AGENTS[0] })}`;
  $('askmodal').hidden = false;
  const res = await fetch('/api/ask', {
    method: 'POST', headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ question: q }),
  });
  if (!res.ok) { $('a-body').textContent = await res.text(); return; }
  const a = await res.json();
  $('a-body').innerHTML = escapeHtml(a.text) +
    (a.tokens ? `<div class="cost">${t('askCost', { agent: a.agent, n: a.tokens.toLocaleString() })}</div>` : '');
};
$('askq').onkeydown = e => { if (e.key === 'Enter') $('askgo').click(); };
$('a-close').onclick = () => { $('askmodal').hidden = true; };

// ------------------------------------------------------------ settings

async function showSettings() {
  const p = await (await fetch('/api/policy')).json();
  const modeKey = { confirm: 'modeConfirm', readonly: 'modeReadonly', auto: 'modeAuto' }[p.mode];
  $('s-policy').innerHTML = `
    <dt>${t('mode')}</dt><dd>${t(modeKey || 'modeConfirm')}</dd>
    <dt>${t('ceiling')}</dt><dd>${p.max_auto_delete_gb} GB</dd>
    <dt>${t('policyFile')}</dt><dd>${escapeHtml(p.config_file)}</dd>
    <dt>${t('archivesAt')}</dt><dd>${escapeHtml(p.archives_dir)}</dd>`;
  $('settings').hidden = false;
}
$('gear').onclick = showSettings;
$('s-close').onclick = () => { $('settings').hidden = true; };
$('s-lang').value = LANG;
$('s-lang').onchange = e => {
  LANG = e.target.value;
  try { localStorage.setItem('diskwise-lang', LANG); } catch {}
  document.documentElement.lang = LANG === 'zh' ? 'zh-Hans' : 'en';
  applyLang();
  showSettings();
  load();
  if (!$('panel-proc').hidden) renderProcs();
};

// Every string that isn't rendered by a load() lives here.
function applyLang() {
  const set = (id, key) => { const el = $(id); if (el) el.textContent = t(key); };
  set('mode-tree', 'biggest'); set('mode-dir', 'browse'); set('mode-files', 'filesOnly');
  set('rescan', 'rescan'); set('tab-disk', 'disk'); set('tab-proc', 'processes');
  set('sort-cpu', 'byCpu'); set('sort-mem', 'byMem'); set('sort-up', 'byUptime');
  set('th-size', 'size'); set('th-name', 'name'); set('th-what', 'whatItIs'); set('th-path', 'path');
  set('empty', 'empty');
  set('s-title', 'settings'); set('s-lang-label', 'language');
  set('s-policy-title', 'policyTitle'); set('s-close', 'close');
  $('contains').placeholder = t('filterPath');
  $('pfind').placeholder = t('filterProc');
  $('category').options[0].textContent = t('allCategories');
  $('min').options[4].textContent = t('everything');
  const ages = ['anyAge', 'over12h', 'over1d', 'over3d', 'over7d'];
  [...$('pdays').options].forEach((o, i) => { o.textContent = t(ages[i]); });
  $('pmine').parentElement.lastChild.textContent = ' ' + t('onlyMine');
  const ph = ['pid', 'byCpu', '', 'memory', 'uptime', 'process', ''];
  document.querySelectorAll('#panel-proc thead th').forEach((th, i) => {
    if (ph[i]) th.textContent = i === 1 ? 'CPU%' : t(ph[i]);
  });
  refreshSelection();
  applyAskLang();
}

function setMode() {
  for (const m of ['tree', 'dir', 'files']) $('mode-' + m).classList.toggle('on', state.mode === m);
}
for (const m of ['tree', 'dir', 'files']) {
  $('mode-' + m).onclick = () => { state.mode = m; setMode(); load(); };
}
$('category').onchange = e => { state.category = e.target.value; load(); };
$('min').onchange = e => { state.min = +e.target.value; load(); };
let searchTimer;
$('contains').oninput = e => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => { state.contains = e.target.value; load(); }, 250);
};
$('rescan').onclick = async () => { await fetch('/api/rescan', { method: 'POST' }); loadStatus(); };
window.onresize = () => { load(); if (!$('panel-proc').hidden) renderProcs(); };

document.documentElement.lang = LANG === 'zh' ? 'zh-Hans' : 'en';
applyLang();
loadAgents();
loadStatus().then(() => { applyUrlParams(); load(); });
if (location.hash === '#processes') showTab('proc');
