// tinux 终端：多 panel —— 每个 panel 一个 xterm.js + 一个 ConPTY shell 会话
const { appWindow } = window.__TAURI__.window;
const { invoke } = window.__TAURI__.tauri;
const { listen } = window.__TAURI__.event;

const terminalRoot = document.getElementById('terminal');
const opacitySlider = document.getElementById('opacity-slider');
const opacityValue = document.getElementById('opacity-value');

// 标签栏 + 面板区（terminalRoot 内：tabbar 顶部，panel-area 填充）
const tabbar = document.createElement('div');
tabbar.className = 'tabbar';
const panelArea = document.createElement('div');
panelArea.className = 'panel-area';
terminalRoot.appendChild(tabbar);
terminalRoot.appendChild(panelArea);

// ---------- Panel 管理 ----------
let panelSeq = 0;
const panels = new Map(); // id -> { id, term, fit, container, outBuf, exited }
let currentPanelId = null;
let tabs = [];
let activeTabId = null;
let tabSeq = 0;

function newPanelId() {
  return `panel-${++panelSeq}`;
}

// 创建一个 panel：容器 + xterm + 会话
function createPanel(id) {
  const container = document.createElement('div');
  container.className = 'panel';

  const term = new Terminal({
    cursorBlink: true,
    fontSize: SETTINGS.fontSize,
    fontFamily: SETTINGS.fontFamily,
    allowTransparency: true,
    scrollback: 5000,
    theme: {
      background: hexToRgba(SETTINGS.bg, 0.7),
      foreground: SETTINGS.fg,
      cursor: SETTINGS.cursor,
      selectionBackground: hexToRgba(SETTINGS.cursor, 0.25)
    }
  });
  const fit = new FitAddon.FitAddon();
  term.loadAddon(fit);
  term.open(container);
  fit.fit();

  // IME 缓解：xterm 5.3 宽字符 composition 提交时会有一帧渲染偏移（横移一个汉字宽）。
  // 合成结束立即强制全量重绘，尽快回到正确画面。
  const ta = term.textarea;
  if (ta) {
    ta.addEventListener('compositionend', () => {
      try { term.refresh(0, term.rows - 1); } catch (_) { /* 尺寸未就绪 */ }
    });
  }

  const panel = { id, term, fit, container, outBuf: '', exited: false, starting: false, syncOn: false, syncBuf: '', syncTail: '' };
  panels.set(id, panel);

  // 输入 → 对应会话（退出后按任意键重启，防重入）
  term.onData((data) => {
    if (panel.exited) {
      if (panel.starting) return;
      panel.exited = false;
      panel.starting = true;
      invoke('shell_start', { sessionId: id, rows: term.rows, cols: term.cols })
        .then(() => { panel.starting = false; })
        .catch((err) => {
          panel.starting = false;
          term.write(`\r\n\x1b[31m${t('shell.restartFail', err)}\x1b[0m\r\n`);
        });
      return;
    }
    invoke('shell_write', { sessionId: id, data });
  });

  // 尺寸变化 → 后端
  term.onResize(({ rows, cols }) => {
    invoke('shell_resize', { sessionId: id, rows, cols });
  });

  // 点击聚焦
  container.addEventListener('mousedown', () => focusPanel(id));

  // 启动会话
  panel.starting = true;
  invoke('shell_start', { sessionId: id, rows: term.rows, cols: term.cols })
    .then(() => {
      panel.starting = false;
      // 会话就绪后校准尺寸：首次 fit 时容器可能未定型（行数偏小/偏大），
      // 且 shell_start 完成前的 shell_resize 会被后端静默丢弃（竞态），
      // 导致 ConPTY 尺寸与前端不一致（如 stty 报 24 行、实际 23 行），
      // 清屏（clear）只清 ConPTY 视口、前端画面留残影。这里补一次对齐。
      requestAnimationFrame(() => {
        try { panel.fit.fit(); } catch (_) { /* 尺寸未就绪 */ }
        invoke('shell_resize', { sessionId: id, rows: term.rows, cols: term.cols });
      });
    })
    .catch((err) => {
      panel.starting = false;
      term.write(`\r\n\x1b[31m${t('shell.startFail', err)}\x1b[0m\r\n`);
    });

  return panel;
}

// 聚焦某个 panel
function focusPanel(id) {
  const p = panels.get(id);
  if (!p) return;
  currentPanelId = id;
  for (const pp of panels.values()) {
    pp.container.classList.toggle('active', pp.id === id);
  }
  p.term.focus();
}

// ---------- Tab 管理（每个 tab 一个独立 layout，panel 全局存活） ----------
function newTabId() { return `tab-${++tabSeq}`; }

function currentTab() {
  return tabs.find((t) => t.id === activeTabId);
}

function collectPanelIds(node, acc = []) {
  if (!node) return acc;
  if (node.panelId) { acc.push(node.panelId); return acc; }
  for (const c of node.children) collectPanelIds(c, acc);
  return acc;
}

function firstPanelId(node) {
  if (!node) return null;
  if (node.panelId) return node.panelId;
  for (const c of node.children) {
    const r = firstPanelId(c);
    if (r) return r;
  }
  return null;
}

function createTab() {
  const id = newTabId();
  const panelId = newPanelId();
  tabs.push({ id, layout: { panelId } });
  activeTabId = id;
  createPanel(panelId);
  renderTabs();
  render();
  focusPanel(panelId);
}

function renderTabs() {
  tabbar.innerHTML = '';
  tabs.forEach((t, i) => {
    const btn = document.createElement('button');
    btn.className = 'tab' + (t.id === activeTabId ? ' active' : '');
    btn.textContent = String(i + 1);
    btn.title = t.id;
    btn.addEventListener('click', () => switchTab(t.id));
    tabbar.appendChild(btn);
  });
  const add = document.createElement('button');
  add.className = 'tab add';
  add.textContent = '+';
  add.title = t('tab.new');
  add.addEventListener('click', () => createTab());
  tabbar.appendChild(add);
}

function switchTab(id) {
  if (id === activeTabId) return;
  activeTabId = id;
  renderTabs();
  render();
  const first = firstPanelId(currentTab().layout);
  if (first) {
    focusPanel(first);
    // 切回时把该 tab 所有 panel 滚动到底部，避免手动拖
    collectPanelIds(currentTab().layout).forEach((pid) => {
      const p = panels.get(pid);
      if (p) p.term.scrollToBottom();
    });
  }
}

function cycleTab(delta) {
  if (tabs.length < 2) return;
  const i = tabs.findIndex((t) => t.id === activeTabId);
  switchTab(tabs[(i + delta + tabs.length) % tabs.length].id);
}

function closeTab() {
  if (tabs.length <= 1) return; // 至少保留一个 tab
  const idx = tabs.findIndex((t) => t.id === activeTabId);
  const t = tabs[idx];
  collectPanelIds(t.layout).forEach((id) => {
    invoke('shell_stop', { sessionId: id });
    panels.delete(id);
  });
  tabs.splice(idx, 1);
  activeTabId = tabs[Math.max(0, idx - 1)].id;
  renderTabs();
  render();
  const first = firstPanelId(currentTab().layout);
  if (first) focusPanel(first);
}

// ---------- 布局树（二分分割：叶子 = panel，内点 = row/col 分割） ----------

function render() {
  panelArea.innerHTML = '';
  const tab = currentTab();
  if (tab) panelArea.appendChild(buildDom(tab.layout));
  requestAnimationFrame(() => {
    for (const p of panels.values()) safeFit(p);
  });
}

function buildDom(node) {
  if (!node) return document.createElement('div');
  if (node.panelId) return panels.get(node.panelId).container;
  const div = document.createElement('div');
  div.className = 'split';
  div.style.flexDirection = node.dir === 'row' ? 'row' : 'column';
  for (const child of node.children) div.appendChild(buildDom(child));
  return div;
}

function safeFit(p) {
  if (p.container.offsetWidth > 0 && p.container.offsetHeight > 0) {
    // 字体/字号变更后 fit 会按新字符尺寸 resize（隐藏 tab 切回时同样处理）
    try { p.fit.fit(); } catch (_) { /* 尺寸未就绪 */ }
    forceTermRedraw(p);
  }
}

// 强制渲染服务整屏重绘：xterm 5.3 没有公开 refresh()，且新旧字体度量相同时
// fit 不会触发 resize，画面会停留在旧字体。访问 _core._renderService 与
// fit addon 自身一致（vendor 版本固定，可安全使用）。
function forceTermRedraw(p) {
  try {
    const svc = p.term._core && p.term._core._renderService;
    if (svc && typeof svc._fullRefresh === 'function') svc._fullRefresh();
  } catch (_) { /* 内部 API 变动时静默降级（fit 仍会处理度量不同的字体） */ }
}

// 查找叶子节点及其父节点
function findLeafParent(node, panelId, parent, idx) {
  if (node.panelId) {
    return node.panelId === panelId ? { node, parent, idx } : null;
  }
  for (let i = 0; i < node.children.length; i++) {
    const r = findLeafParent(node.children[i], panelId, node, i);
    if (r) return r;
  }
  return null;
}

// 分割当前 panel：dir='row' 左右 / 'col' 上下
function splitPanel(dir) {
  const cur = currentPanelId;
  const tab = currentTab();
  if (!cur || !tab) return;
  const loc = findLeafParent(tab.layout, cur, null, -1);
  if (!loc) return;
  const newId = newPanelId();
  const splitNode = { dir, children: [loc.node, { panelId: newId }] };
  if (loc.parent) {
    loc.parent.children[loc.idx] = splitNode;
  } else {
    tab.layout = splitNode;
  }
  createPanel(newId);
  render();
  focusPanel(newId);
}

// 关闭当前 panel（最后一个不允许关）
function closePanel() {
  const cur = currentPanelId;
  const tab = currentTab();
  if (!cur || !tab) return;
  // tab 只剩一个 panel → 关闭整个 tab
  if (collectPanelIds(tab.layout).length <= 1) {
    closeTab();
    return;
  }
  const loc = findLeafParent(tab.layout, cur, null, -1);
  if (!loc) return;
  invoke('shell_stop', { sessionId: cur });
  panels.delete(cur);
  if (loc.parent) {
    loc.parent.children = [loc.parent.children[1 - loc.idx]];
  } else {
    tab.layout = null;
  }
  render();
  const first = firstPanelId(tab.layout);
  if (first) focusPanel(first);
}

// 切换聚焦（按插入顺序循环）
function cycleFocus(delta) {
  const ids = [...panels.keys()];
  if (!ids.length) return;
  const i = ids.indexOf(currentPanelId);
  focusPanel(ids[(i + delta + ids.length) % ids.length]);
}

// ---------- 设置（字体/颜色/透明度，localStorage 持久化） ----------
const DEFAULT_SETTINGS = {
  opacity: 85,
  fontFamily: 'Consolas, Monaco, "Courier New", monospace',
  fontSize: 13,
  fg: '#00ff88',
  bg: '#141423',
  cursor: '#00ff88'
};

let SETTINGS = (() => {
  try {
    const raw = localStorage.getItem('feier-settings');
    if (raw) return Object.assign({}, DEFAULT_SETTINGS, JSON.parse(raw));
  } catch (_) { /* 忽略损坏数据 */ }
  return Object.assign({}, DEFAULT_SETTINGS);
})();

function saveSettings() {
  try { localStorage.setItem('feier-settings', JSON.stringify(SETTINGS)); } catch (_) {}
}

function hexToRgba(hex, a) {
  const m = /^#?([0-9a-f]{6})$/i.exec(String(hex).trim());
  if (!m) return String(hex);
  const n = parseInt(m[1], 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${a})`;
}

function applySettings() {
  const s = SETTINGS;
  document.body.style.opacity = s.opacity / 100;
  opacitySlider.value = s.opacity;
  opacityValue.textContent = `${s.opacity}%`;
  for (const p of panels.values()) {
    p.term.options.fontFamily = s.fontFamily;
    p.term.options.fontSize = s.fontSize;
    p.term.options.theme = {
      background: hexToRgba(s.bg, 0.7),
      foreground: s.fg,
      cursor: s.cursor,
      selectionBackground: hexToRgba(s.cursor, 0.25)
    };
  }
  // 字体变更后 xterm 会自动重测字符宽度（fontFamily/fontSize 的 options 监听），
  // fit 会用新尺寸 resize 并触发全量重绘；度量相同时 fit 不生效，用 forceTermRedraw 兜底。
  requestAnimationFrame(() => {
    for (const p of panels.values()) {
      try { p.fit.fit(); } catch (_) { /* 隐藏 tab 容器无尺寸，切回时由 safeFit 补 */ }
      forceTermRedraw(p);
    }
  });
}

// 用系统全部字体填充设置面板的下拉（保留当前值，可能是自定义字体链）
async function populateFontList() {
  const sel = document.getElementById('set-font');
  if (!sel) return;
  try {
    const fonts = await window.__TAURI__.tauri.invoke('list_fonts');
    const current = SETTINGS.fontFamily;
    const first = document.createElement('option');
    first.value = current;
    first.textContent = current;
    sel.innerHTML = '';
    sel.appendChild(first);
    for (const name of fonts) {
      if (name === current) continue;
      const opt = document.createElement('option');
      opt.value = name;
      opt.textContent = name;
      sel.appendChild(opt);
    }
    sel.value = current;
  } catch (_) { /* 后端不可用时保留初始选项 */ }
}

// 设置面板（模态）
const settingsOverlay = document.createElement('div');
settingsOverlay.id = 'settings-overlay';
settingsOverlay.style.cssText = 'position:fixed;inset:0;background:rgba(0,0,0,0.55);z-index:999;display:none;align-items:center;justify-content:center;';
settingsOverlay.innerHTML = `
  <div class="settings-card">
    <h3 data-i18n="settings.title">⚙ 设置</h3>
    <label class="setting-row"><span data-i18n="settings.opacity">窗口透明度</span>
      <input type="range" id="set-opacity" min="10" max="100">
      <span id="set-opacity-v" class="setting-v"></span>
    </label>
    <label class="setting-row"><span data-i18n="settings.font">字体</span>
      <select id="set-font">
        <option value='Consolas, Monaco, "Courier New", monospace'>Consolas</option>
        <option value='"Microsoft YaHei", Consolas, monospace' data-i18n="settings.font.yh">微软雅黑</option>
        <option value='"Courier New", monospace'>Courier New</option>
        <option value='monospace' data-i18n="settings.font.mono">默认等宽</option>
      </select>
    </label>
    <label class="setting-row"><span data-i18n="settings.fontsize">字号</span>
      <input type="range" id="set-fontsize" min="10" max="24">
      <span id="set-fontsize-v" class="setting-v"></span>
    </label>
    <label class="setting-row"><span data-i18n="settings.fg">前景色</span> <input type="color" id="set-fg"></label>
    <label class="setting-row"><span data-i18n="settings.bg">背景色</span> <input type="color" id="set-bg"></label>
    <label class="setting-row"><span data-i18n="settings.cursor">光标色</span> <input type="color" id="set-cursor"></label>
    <div class="settings-actions">
      <button id="set-reset" data-i18n="settings.reset">恢复默认</button>
      <button id="set-close" data-i18n="settings.close">关闭</button>
    </div>
  </div>
`;
document.body.appendChild(settingsOverlay);

function syncSettingsPanel() {
  const s = SETTINGS;
  document.getElementById('set-opacity').value = s.opacity;
  document.getElementById('set-opacity-v').textContent = `${s.opacity}%`;
  document.getElementById('set-font').value = s.fontFamily;
  document.getElementById('set-fontsize').value = s.fontSize;
  document.getElementById('set-fontsize-v').textContent = `${s.fontSize}px`;
  document.getElementById('set-fg').value = s.fg;
  document.getElementById('set-bg').value = s.bg;
  document.getElementById('set-cursor').value = s.cursor;
}

function openSettings() {
  syncSettingsPanel();
  settingsOverlay.style.display = 'flex';
}

function closeSettings() {
  settingsOverlay.style.display = 'none';
}

document.getElementById('set-opacity').addEventListener('input', (e) => { SETTINGS.opacity = Number(e.target.value); applySettings(); saveSettings(); syncSettingsPanel(); });
document.getElementById('set-font').addEventListener('change', (e) => { SETTINGS.fontFamily = e.target.value; applySettings(); saveSettings(); });
document.getElementById('set-fontsize').addEventListener('input', (e) => { SETTINGS.fontSize = Number(e.target.value); applySettings(); saveSettings(); syncSettingsPanel(); });
document.getElementById('set-fg').addEventListener('input', (e) => { SETTINGS.fg = e.target.value; applySettings(); saveSettings(); });
document.getElementById('set-bg').addEventListener('input', (e) => { SETTINGS.bg = e.target.value; applySettings(); saveSettings(); });
document.getElementById('set-cursor').addEventListener('input', (e) => { SETTINGS.cursor = e.target.value; applySettings(); saveSettings(); });
document.getElementById('set-reset').addEventListener('click', () => { SETTINGS = Object.assign({}, DEFAULT_SETTINGS); applySettings(); saveSettings(); syncSettingsPanel(); });
document.getElementById('set-close').addEventListener('click', closeSettings);

// ---------- Agent 安装中心 ----------
const AGENTS = [
  { id: 'claude-code', name: 'Claude Code', pkg: '@anthropic-ai/claude-code', icon: 'assets/agents/01_Claude_Code.png' },
  { id: 'codex', name: 'Codex', pkg: '@openai/codex', icon: 'assets/agents/02_Codex_CLI.png' },
  { id: 'openclaw', name: 'OpenClaw', pkg: 'openclaw', icon: '🐾' },
  { id: 'hermes', name: 'Hermes', pkg: 'hermes-agent', icon: 'assets/agents/04_Hermes.png' },
  { id: 'opencode', name: 'OpenCode', pkg: 'opencode-ai', icon: 'assets/agents/03_OpenCode.png' },
  { id: 'kimi-code', name: 'Kimi Code', pkg: '@moonshot-ai/kimi-code', icon: 'assets/agents/08_Kimi_CLI.png' },
  { id: 'gemini-cli', name: 'Gemini CLI', pkg: '@google/gemini-cli', icon: 'assets/agents/06_Gemini_CLI.png' },
  { id: 'qwen-code', name: 'Qwen Code', pkg: '@qwen-code/qwen-code', icon: 'assets/agents/10_Qwen_Code.png' },
  { id: 'copilot', name: 'GitHub Copilot', pkg: '@github/copilot', icon: 'assets/agents/12_GitHub_Copilot.png' },
  { id: 'mistral-vibe', name: 'Mistral Vibe', pkg: 'mistral-vibe', icon: 'assets/agents/16_Mistral_Vibe.png' },
  { id: 'aider', name: 'Aider', pkg: 'aider-chat', icon: 'assets/agents/19_Aider.png' }
];

// agent 状态表: id -> { status: idle|installing|done|failed, msg, els }
const agentState = {};

const agentsOverlay = document.createElement('div');
agentsOverlay.id = 'agents-overlay';
agentsOverlay.style.cssText = 'position:fixed;inset:0;background:rgba(0,0,0,0.55);z-index:998;display:none;align-items:center;justify-content:center;';
const agentCard = document.createElement('div');
agentCard.className = 'agent-card';
agentCard.innerHTML = '<div class="agent-card-head"><h3 data-i18n="agent.title">▦ Agent 安装中心</h3><button id="agents-close" class="agent-close" data-i18n-title="agent.close" title="关闭 (Esc)">✕</button></div><div id="agent-list"></div>';
agentsOverlay.appendChild(agentCard);
document.body.appendChild(agentsOverlay);

function applyAgentState(st) {
  if (!st.els) return;
  const { fill, btn, status } = st.els;
  fill.className = 'fill';
  // 单按钮模式：已安装/卸载中/卸载失败 → 卸载样式（红）；其余 → 安装样式（绿）
  const uninstallMode =
    st.status === 'done' ||
    st.status === 'uninstalling' ||
    (st.status === 'failed' && st.busy === 'uninstall');
  btn.classList.toggle('agent-btn-uninstall', uninstallMode);
  if (st.status === 'installing') {
    fill.classList.add('working');
    btn.disabled = true;
    btn.textContent = t('agent.installing');
    status.textContent = st.msg || t('agent.installing');
  } else if (st.status === 'uninstalling') {
    fill.classList.add('working');
    btn.disabled = true;
    btn.textContent = t('agent.uninstalling');
    status.textContent = st.msg || t('agent.uninstalling');
  } else if (st.status === 'done') {
    fill.style.width = '100%';
    btn.disabled = false;
    btn.textContent = t('agent.uninstall');
    status.textContent = t('agent.installedStatus');
  } else if (st.status === 'failed') {
    fill.style.width = '0%';
    btn.disabled = false;
    btn.textContent = t('agent.retry');
    status.textContent = st.msg || t('agent.retry');
  } else {
    fill.style.width = '0%';
    btn.disabled = false;
    btn.textContent = t('agent.install');
    status.textContent = st.msg || '';
  }
}

function renderAgentRow(a) {
  const st = (agentState[a.id] = agentState[a.id] || { status: 'idle', msg: '', els: null });
  const row = document.createElement('div');
  row.className = 'agent-row';
  row.dataset.agent = a.id;
  row.innerHTML = `
    ${typeof a.icon === 'string' && a.icon.startsWith('assets/')
      ? `<img class="agent-icon-img" src="${a.icon}" alt="${a.name}">`
      : `<span class="agent-icon">${a.icon}</span>`}
    <div class="agent-info">
      <div class="agent-name">${a.name}</div>
      <div class="agent-pkg">${a.pkg}</div>
    </div>
    <div class="agent-status"></div>
    <div class="agent-bar"><div class="fill"></div></div>
    <button class="agent-btn">${t('agent.install')}</button>`;
  st.els = {
    fill: row.querySelector('.fill'),
    btn: row.querySelector('.agent-btn'),
    status: row.querySelector('.agent-status')
  };
  // 单按钮：按状态切换安装/卸载（已安装 → 卸载，否则 → 安装）
  st.els.btn.addEventListener('click', () => {
    const mode = st.status === 'done' ? 'uninstall' : 'install';
    st.busy = mode;
    st.status = mode === 'uninstall' ? 'uninstalling' : 'installing';
    st.msg = t('agent.starting');
    applyAgentState(st);
    invoke(mode === 'uninstall' ? 'agent_uninstall' : 'agent_install', { agent: a.id })
      .catch((err) => {
        st.status = 'failed';
        st.msg = String(err);
        applyAgentState(st);
      });
  });
  // 初始已安装检测（仅在空闲时生效，避免覆盖用户点击）
  invoke('agent_installed', { agent: a.id })
    .then((ok) => {
      if (ok && st.status === 'idle') {
        st.status = 'done';
        applyAgentState(st);
      }
    })
    .catch(() => {});
  applyAgentState(st);
  return row;
}

function renderAgentList() {
  const list = document.getElementById('agent-list');
  list.innerHTML = '';
  for (const a of AGENTS) list.appendChild(renderAgentRow(a));
}

function openAgents() {
  renderAgentList();
  agentsOverlay.style.display = 'flex';
}

function closeAgents() {
  agentsOverlay.style.display = 'none';
}

// ---------- 输出路由（按 session_id 分发，含内置命令标记剥离） ----------
// 写入终端（含 __FEIER__video: 标记剥离 + 行缓冲）
function writeChunk(p, text) {
  if (!text) return;
  p.outBuf += text;
  let idx;
  while ((idx = p.outBuf.indexOf('\n')) !== -1) {
    const line = p.outBuf.slice(0, idx);
    p.outBuf = p.outBuf.slice(idx + 1);
    if (line.startsWith('__FEIER__video:')) {
      openVideo(line.slice(15).trim());
    } else {
      // 保留原始行尾：split 是按 \n 切的，\r（若原始流为 CRLF）已留在 line 里。
      // 不能统一补 '\r\n' —— opencode 等 TUI 用裸 LF（xterm 语义：只换行、列不变）
      // 做相对定位，补 CR 会把列重置到 1，导致欢迎界面 "Ask 上一行" 整行左移 2 列。
      p.term.write(line + '\n');
    }
  }
  // 残余无换行的内容：标记开头则等完整行，否则直接渲染（保持交互性）
  if (p.outBuf) {
    if (p.outBuf.startsWith('__FEIER__')) return;
    p.term.write(p.outBuf);
    p.outBuf = '';
  }
}

// 同步输出 (DECSET 2026) 处理：帧缓冲 shim（旧版，从备份 bundle 恢复）。
// 2026h/l 各 8 字节，可能跨 16KB 分块截断，仅在结尾疑似前缀时暂存尾巴。
// 帧内内容进 syncBuf，2026l 闭合时整帧一次提交（保留帧缓冲性能，输入不卡）。
// 已修复：sync 帧内跨块内容此前绕过 syncBuf 直接渲染，帧内转义序列被 16KB 分块
// 截断成字面文本/错位（opencode 欢迎界面错行）；现在 syncOn 期间统一进帧缓冲。
const SYNC_SEQ = ['\x1b[?2026h', '\x1b[?2026l', '\x1b[?2026', '\x1b[?202', '\x1b[?20', '\x1b[?2', '\x1b[?', '\x1b['];
const SYNC_LEN = 8;

function trailingSyncTail(s) {
  for (const pr of SYNC_SEQ) {
    if (s.endsWith(pr)) return pr.length;
  }
  return 0;
}

function onShellOutput(sid, text) {
  const p = panels.get(sid);
  if (!p) return;
  const combined = p.syncTail + text;
  p.syncTail = '';

  if (!combined.includes('\x1b[?2026')) {
    if (p.syncOn) {
      // 仍在同步帧内：整段进帧缓冲，帧闭合（2026l）时统一提交（与下方
      // syncOn 分支一致，避免帧内容跨分块时被直接渲染而错位）。
      const tail = trailingSyncTail(combined);
      if (tail) {
        p.syncBuf += combined.slice(0, -tail);
        p.syncTail = combined.slice(-tail);
      } else {
        p.syncBuf += combined;
      }
      return;
    }
    const tail = trailingSyncTail(combined);
    if (tail) {
      p.syncTail = combined.slice(-tail);
      writeChunk(p, combined.slice(0, -tail));
    } else {
      writeChunk(p, combined);
    }
    return;
  }

  let rest = combined;
  let guard = 0;
  while (rest.length && guard++ < 2000) {
    if (p.syncOn) {
      const end = rest.indexOf('\x1b[?2026l');
      if (end === -1) {
        const tail = trailingSyncTail(rest);
        if (tail) {
          p.syncBuf += rest.slice(0, -tail);
          p.syncTail = rest.slice(-tail);
        } else {
          p.syncBuf += rest;
        }
        break;
      }
      p.syncBuf += rest.slice(0, end);
      p.syncOn = false;
      writeChunk(p, p.syncBuf);
      p.syncBuf = '';
      rest = rest.slice(end + SYNC_LEN);
    } else {
      const start = rest.indexOf('\x1b[?2026h');
      if (start === -1) {
        const tail = trailingSyncTail(rest);
        if (tail) {
          writeChunk(p, rest.slice(0, -tail));
          p.syncTail = rest.slice(-tail);
        } else {
          writeChunk(p, rest);
        }
        break;
      }
      writeChunk(p, rest.slice(0, start));
      p.syncOn = true;
      rest = rest.slice(start + SYNC_LEN);
    }
  }
}

// ---------- 初始化 ----------
document.addEventListener('DOMContentLoaded', async () => {
  // 国际化：先按 Windows 显示语言切文案，再构建 UI
  await detectLanguage();
  applyI18n();
  populateFontList(); // 系统字体下拉（异步填充，打开设置时已就绪）

  // 标题栏
  document.getElementById('titlebar-minimize').addEventListener('click', () => appWindow.minimize());
  document.getElementById('titlebar-maximize').addEventListener('click', () => appWindow.toggleMaximize());
  document.getElementById('titlebar-close').addEventListener('click', () => appWindow.close());
  document.getElementById('titlebar-settings').addEventListener('click', openSettings);
  document.getElementById('titlebar-agents').addEventListener('click', openAgents);
  document.getElementById('agents-close').addEventListener('click', closeAgents);

  // 版本号（构建时间戳，精确到秒）
  const BUILD_TS = '2026-08-13 12:09:10';
  try {
    const ver = await window.__TAURI__.app.getVersion();
    document.getElementById('titlebar-version').textContent = `v${ver} · ${BUILD_TS}`;
  } catch (_) {
    document.getElementById('titlebar-version').textContent = BUILD_TS;
  }

  // 透明度滑块（同步 SETTINGS）
  opacitySlider.addEventListener('input', (e) => {
    SETTINGS.opacity = Number(e.target.value);
    applySettings();
    saveSettings();
  });
  // 恢复持久化设置（含初始透明度）
  applySettings();

  // shell 输出流（按 session_id 路由）
  await listen('shell-output', (event) => {
    onShellOutput(event.payload.session_id, event.payload.text);
  });

  // shell 会话结束（静默标记，按任意键重启，无提示文字）
  await listen('shell-exit', (event) => {
    const p = panels.get(event.payload.session_id);
    if (!p) return;
    p.exited = true;
  });

  // Agent 安装输出（更新状态行）
  await listen('agent-install-output', (event) => {
    const st = agentState[event.payload.agent];
    if (!st) return;
    const line = String(event.payload.text).trim().split('\n').pop() || '';
    if (line) {
      st.msg = line.slice(0, 60);
      if ((st.status === 'installing' || st.status === 'uninstalling') && st.els) st.els.status.textContent = st.msg;
    }
  });

  // Agent 安装完成
  await listen('agent-install-done', (event) => {
    const st = agentState[event.payload.agent];
    if (!st) return;
    const ok = event.payload.code === 0;
    if (st.busy === 'uninstall') {
      st.status = ok ? 'idle' : 'failed';
      st.msg = ok ? '' : t('agent.uninstallFail', event.payload.code);
      if (ok) {
        // 卸载成功后复查实际状态（npm/uv 可能残留）
        st.busy = 'install';
        invoke('agent_installed', { agent: event.payload.agent }).then((inst) => {
          if (inst) st.status = 'done';
          applyAgentState(st);
        });
        return;
      }
    } else {
      st.status = ok ? 'done' : 'failed';
      st.msg = ok ? '' : t('agent.installFail', event.payload.code);
      st.busy = 'install';
    }
    applyAgentState(st);
  });

  // 窗口缩放 → 所有 panel 自适应
  window.addEventListener('resize', () => {
    for (const p of panels.values()) p.fit.fit();
  });

  // 布局稳定后校准一次尺寸（首次 fit 时容器可能未定型，避免 TUI 底部裁行）
  setTimeout(() => {
    for (const p of panels.values()) {
      try {
        p.fit.fit();
        invoke('shell_resize', { sessionId: p.id, rows: p.term.rows, cols: p.term.cols });
      } catch (_) { /* 尺寸未就绪 */ }
    }
  }, 300);

  // 默认启动一个 tab（内含一个 panel）
  createTab();
});

// ---------- 多 panel 快捷键（Ctrl+Shift 组合，capture 拦截） ----------
document.addEventListener('keydown', (e) => {
  // Tab 切换（Ctrl+Tab / Ctrl+Shift+Tab）
  if (e.key === 'Tab' && e.ctrlKey) {
    e.preventDefault();
    cycleTab(e.shiftKey ? -1 : 1);
    return;
  }
  if (!(e.ctrlKey && e.shiftKey)) return;
  switch (e.key) {
    case '\\':
      e.preventDefault();
      splitPanel('row');
      break;
    case '-':
    case '_':
      e.preventDefault();
      splitPanel('col');
      break;
    case 'W':
      e.preventDefault();
      closePanel();
      break;
    case 'T':
      e.preventDefault();
      createTab();
      break;
    case 'Q':
      e.preventDefault();
      closeTab();
      break;
    case '[':
      e.preventDefault();
      cycleFocus(-1);
      break;
    case ']':
      e.preventDefault();
      cycleFocus(1);
      break;
  }
}, true);

// ---------- 视频播放（shell 函数 video <URL> 经标记触发） ----------
const videoOverlay = document.createElement('div');
videoOverlay.id = 'video-overlay';
videoOverlay.style.cssText = 'position:fixed;inset:0;background:rgba(0,0,0,0.92);z-index:1000;display:none;flex-direction:column;align-items:center;justify-content:center;gap:12px;';
const videoEl = document.createElement('video');
videoEl.controls = true;
videoEl.autoplay = true;
videoEl.style.cssText = 'max-width:92vw;max-height:78vh;background:#000;border-radius:6px;';
const videoBar = document.createElement('div');
videoBar.style.cssText = 'display:flex;align-items:center;gap:12px;color:#888;font-size:12px;';
const videoClose = document.createElement('button');
videoClose.textContent = t('video.close');
videoClose.dataset.i18n = 'video.close'; // 语言检测后由 applyI18n 覆盖（脚本求值期 t() 还是默认语言）
videoClose.onclick = () => { videoOverlay.style.display = 'none'; videoEl.pause(); };
const videoHint = document.createElement('span');
videoBar.appendChild(videoClose);
videoBar.appendChild(videoHint);
videoOverlay.appendChild(videoEl);
videoOverlay.appendChild(videoBar);
document.body.appendChild(videoOverlay);

document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') {
    videoOverlay.style.display = 'none';
    videoEl.pause();
    settingsOverlay.style.display = 'none';
    agentsOverlay.style.display = 'none';
  }
});

function openVideo(url) {
  if (!url) return;
  videoEl.src = url;
  videoHint.textContent = url;
  videoOverlay.style.display = 'flex';
  videoEl.play().catch(() => {});
}
