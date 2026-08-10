// ---- 国际化：启动时按 Windows 显示语言切换全部文案 ----
// 用法：
//   await detectLanguage();  // 启动时先调用（后端 get_os_language，兜底 navigator.language）
//   t('key', ...args)        // 动态文案（支持 {0} {1} 占位）
//   applyI18n()              // 静态文案（[data-i18n] 文本 / [data-i18n-title] 悬停提示）
// 新增窗口/文案：在 I18N_DICT 两个语言里加 key，静态处用 data-i18n，动态处用 t()。

const I18N_DICT = {
  'zh-CN': {
    // 主窗口
    'titlebar.opacity': '窗口透明度',
    'titlebar.agents': 'Agent 安装中心',
    'titlebar.settings': '设置',
    // 设置面板
    'settings.title': '⚙ 设置',
    'settings.opacity': '窗口透明度',
    'settings.font': '字体',
    'settings.fontsize': '字号',
    'settings.fg': '前景色',
    'settings.bg': '背景色',
    'settings.cursor': '光标色',
    'settings.reset': '恢复默认',
    'settings.close': '关闭',
    'settings.font.yh': '微软雅黑',
    'settings.font.mono': '默认等宽',
    // Agent 安装中心
    'agent.title': '▦ Agent 安装中心',
    'agent.close': '关闭 (Esc)',
    'agent.install': '安装',
    'agent.installing': '安装中…',
    'agent.installed': '已安装 ✓',
    'agent.retry': '重试',
    'agent.uninstall': '卸载',
    'agent.uninstalling': '卸载中…',
    'agent.installedStatus': '已安装',
    'agent.starting': '启动…',
    'agent.installFail': '安装失败 (exit {0})',
    'agent.uninstallFail': '卸载失败 (exit {0})',
    // 视频播放器
    'video.close': '关闭',
    // Tab/面板
    'tab.new': '新建 tab (Ctrl+Shift+T)',
    // Shell 错误
    'shell.startFail': '启动 shell 失败: {0}',
    'shell.restartFail': '重启 shell 失败: {0}',
  },
  'en-US': {
    // Main window
    'titlebar.opacity': 'Window opacity',
    'titlebar.agents': 'Agent Center',
    'titlebar.settings': 'Settings',
    // Settings panel
    'settings.title': '⚙ Settings',
    'settings.opacity': 'Window opacity',
    'settings.font': 'Font',
    'settings.fontsize': 'Font size',
    'settings.fg': 'Foreground',
    'settings.bg': 'Background',
    'settings.cursor': 'Cursor',
    'settings.reset': 'Reset',
    'settings.close': 'Close',
    'settings.font.yh': 'Microsoft YaHei',
    'settings.font.mono': 'Default Monospace',
    // Agent Center
    'agent.title': '▦ Agent Center',
    'agent.close': 'Close (Esc)',
    'agent.install': 'Install',
    'agent.installing': 'Installing…',
    'agent.installed': 'Installed ✓',
    'agent.retry': 'Retry',
    'agent.uninstall': 'Uninstall',
    'agent.uninstalling': 'Uninstalling…',
    'agent.installedStatus': 'Installed',
    'agent.starting': 'Starting…',
    'agent.installFail': 'Install failed (exit {0})',
    'agent.uninstallFail': 'Uninstall failed (exit {0})',
    // Video player
    'video.close': 'Close',
    // Tabs / panels
    'tab.new': 'New tab (Ctrl+Shift+T)',
    // Shell errors
    'shell.startFail': 'Failed to start shell: {0}',
    'shell.restartFail': 'Failed to restart shell: {0}',
  },
};

// 繁体中文用户沿用简体文案（无独立繁体字典）
I18N_DICT['zh-TW'] = I18N_DICT['zh-CN'];

let I18N_LANG = 'zh-CN';

// 取当前语言文案（key 缺失时回退 zh-CN，再缺失返回 key 本身）
function t(key, ...args) {
  const dict = I18N_DICT[I18N_LANG] || I18N_DICT['zh-CN'];
  let s = dict[key] !== undefined ? dict[key] : key;
  args.forEach((a, i) => {
    s = s.replace('{' + i + '}', String(a));
  });
  return s;
}

// 启动时检测：优先后端（Windows 显示语言），失败兜底 navigator.language
async function detectLanguage() {
  try {
    if (window.__TAURI__ && window.__TAURI__.tauri) {
      const code = await window.__TAURI__.tauri.invoke('get_os_language');
      if (code && I18N_DICT[code]) {
        I18N_LANG = code;
        return;
      }
    }
  } catch (_) { /* 后端不可用时兜底 */ }
  const nav = String(navigator.language || '').toLowerCase();
  I18N_LANG = nav.startsWith('zh') ? 'zh-CN' : 'en-US';
}

// 应用静态文案：<el data-i18n="key"> 文本；<el data-i18n-title="key"> 悬停提示
function applyI18n() {
  document.documentElement.lang = I18N_LANG;
  document.querySelectorAll('[data-i18n]').forEach((el) => {
    el.textContent = t(el.dataset.i18n);
  });
  document.querySelectorAll('[data-i18n-title]').forEach((el) => {
    el.title = t(el.dataset.i18nTitle);
  });
}

// 设置面板等动态重建的容器：重建后重新应用
function currentLang() {
  return I18N_LANG;
}
