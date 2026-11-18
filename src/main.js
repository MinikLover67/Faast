import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { open } from '@tauri-apps/plugin-dialog';
import { icon } from './icons.js';

let downloads = [];
let activeFilter = 'all';
let activeCategory = null;
let activeView = 'list'; // 'list' | 'files'
// Tracks which terminal toasts were already shown, so a progress event and a
// list refresh can't double-toast (and a fast download can't be missed).
const announced = new Set();

const $list = document.getElementById('downloadList');
const $filesView = document.getElementById('filesView');
const $filesList = document.getElementById('filesList');
const $filesEmpty = document.getElementById('filesEmpty');
const $filesPath = document.getElementById('filesPath');
const $btnFilesBack = document.getElementById('btnFilesBack');
const $search = document.getElementById('searchInput');
const $btnNew = document.getElementById('btnNew');
const $btnPause = document.getElementById('btnPause');
const $btnResume = document.getElementById('btnResume');
const $btnRemove = document.getElementById('btnRemove');
const $modalBackdrop = document.getElementById('modalBackdrop');
const $modalUrl = document.getElementById('downloadUrl');
const $modalAdd = document.getElementById('btnModalAdd');
const $scheduleStart = document.getElementById('scheduleStart');
const $modalCancel = document.getElementById('btnModalCancel');
const $modalBrowse = document.getElementById('btnModalBrowse');
const $modalSaveDirBtn = document.getElementById('btnModalSaveDir');
const $saveDirLabel = document.getElementById('saveDirLabel');
const $toastWrap = document.getElementById('toastWrap');
const $formatRow = document.getElementById('formatRow');
const $mediaFormat = document.getElementById('mediaFormat');
const $routingBackdrop = document.getElementById('routingBackdrop');
const $routingRoot = document.getElementById('routingRoot');
const $routingEnabled = document.getElementById('routingEnabled');
const $routingRules = document.getElementById('routingRules');
const $btnFolders = document.getElementById('btnFolders');
const $btnRoutingAdd = document.getElementById('btnRoutingAdd');
const $btnRoutingSave = document.getElementById('btnRoutingSave');
const $btnRoutingCancel = document.getElementById('btnRoutingCancel');
const $btnRoutingRoot = document.getElementById('btnRoutingRoot');
const $tabLibrary = document.getElementById('tabLibrary');
const $tabFiles = document.getElementById('tabFiles');

// Draft copy of the routing config while the modal is open. Edits are only
// written back on Save, so Cancel genuinely discards changes.
let routingDraft = null;

// Mirrors the backend's YT_DLP_HOSTS list. Used only to decide whether to
// show the format picker — the backend is what actually routes the URL, so a
// miss here degrades to "no picker", never to a wrong download.
const MEDIA_HOSTS = [
  'youtube.com', 'youtu.be', 'm.youtube.com', 'music.youtube.com',
  'vimeo.com', 'dailymotion.com', 'facebook.com', 'tiktok.com',
  'twitch.tv', 'soundcloud.com', 'bandcamp.com', 'reddit.com',
  'instagram.com', 'twitter.com', 'x.com',
];

function looksLikeMediaUrl(s) {
  const t = String(s || '').trim().toLowerCase();
  if (!t.startsWith('http://') && !t.startsWith('https://')) return false;
  try {
    const host = new URL(t).hostname.replace(/^www\./, '');
    return MEDIA_HOSTS.some((h) => host === h || host.endsWith('.' + h));
  } catch {
    return false;
  }
}

function syncFormatVisibility() {
  if (!$formatRow || !$mediaFormat) return;
  const show = looksLikeMediaUrl($modalUrl.value);
  $formatRow.hidden = !show;
  // Audio picks only make sense for media links; reset to video otherwise so
  // a stale "MP3" selection can't be applied to a ZIP URL later.
  if (!show) $mediaFormat.value = 'best';
}

function chosenMediaFormat() {
  const v = $mediaFormat ? String($mediaFormat.value || '') : '';
  return v || 'best';
}

let selectedSaveDir = null; // null = use default Downloads folder

/* ── Toast notifications ────────────────────────────────────── */

function toast(title, msg, type = 'info', timeout = 4200) {
  if (!$toastWrap) return;
  const iconName = type === 'success' ? 'check-circle' : type === 'error' ? 'alert-circle' : 'info';
  const el = document.createElement('div');
  el.className = `toast ${type}`;
  el.innerHTML = `
    <span class="toast-icon">${icon(iconName)}</span>
    <div class="toast-body">
      <div class="toast-title">${esc(title)}</div>
      ${msg ? `<div class="toast-msg">${esc(msg)}</div>` : ''}
    </div>
    <button class="toast-close" aria-label="Dismiss">×</button>`;
  const dismiss = () => {
    el.classList.add('leaving');
    setTimeout(() => el.remove(), 220);
  };
  el.querySelector('.toast-close').addEventListener('click', dismiss);
  $toastWrap.appendChild(el);
  setTimeout(dismiss, timeout);
}

/* ── Formatting helpers ─────────────────────────────────────── */

function formatBytes(bytes) {
  const b = Number(bytes) || 0;
  if (b <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(Math.floor(Math.log(b) / Math.log(1024)), units.length - 1);
  const v = b / Math.pow(1024, i);
  return `${v >= 100 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

function hasKnownSize(d) {
  return Number(d.totalBytes) > 0;
}

function sizeText(d) {
  return `${formatBytes(d.downloadedBytes)} / ${hasKnownSize(d) ? formatBytes(d.totalBytes) : '—'}`;
}

function isIndeterminate(d) {
  return d.status === 'downloading' && !hasKnownSize(d);
}

function formatSpeed(bps) {
  const v = Number(bps) || 0;
  if (v <= 0) return '0 B/s';
  if (v < 1024) return `${v.toFixed(0)} B/s`;
  if (v < 1024 * 1024) return `${(v / 1024).toFixed(1)} KB/s`;
  return `${(v / (1024 * 1024)).toFixed(1)} MB/s`;
}

function formatEta(item) {
  if (item.status === 'completed') return 'Done';
  if (item.status === 'failed') return 'Failed';
  if (item.status === 'paused') return 'Paused';
  const total = Number(item.totalBytes) || 0;
  const downloaded = Number(item.downloadedBytes) || 0;
  const speed = Number(item.speedBps) || 0;
  if (!total || total <= 0 || !speed || speed <= 0) return '—';
  const secs = Math.max(0, total - downloaded) / speed;
  if (secs > 3600) return `${Math.round(secs / 60)} h`;
  if (secs > 60) return `${Math.round(secs / 60)} min`;
  return `${Math.round(secs)} s`;
}

function esc(s) {
  return String(s ?? '')
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;');
}

/* ── Status helpers ─────────────────────────────────────────── */

function statusClass(status) {
  switch (status) {
    case 'downloading':
      return 'downloading';
    case 'completed':
      return 'completed';
    case 'paused':
      return 'paused';
    case 'failed':
      return 'failed';
    default:
      return 'stopped';
  }
}

function statusLabel(status) {
  const s = status || 'stopped';
  return s.charAt(0).toUpperCase() + s.slice(1);
}

function statusIcon(status) {
  switch (status) {
    case 'downloading':
      return 'arrow-down-circle';
    case 'completed':
      return 'check-circle';
    case 'paused':
      return 'pause-circle';
    default:
      return 'x-circle';
  }
}

function fileIconClass(category) {
  return { video: 'video', music: 'music', document: 'doc', app: 'app' }[category] || '';
}

function fileIconName(category) {
  return {
    video: 'file-video',
    music: 'music',
    document: 'file-text',
    app: 'app-window',
  }[category] || 'file';
}

// FDM gives torrents their own icon instead of borrowing the file-type one.
function isTorrent(d) {
  return d.sourceKind === 'torrent' || d.sourceKind === 'magnet';
}

function rowIconName(d) {
  if (isTorrent(d)) return 'torrent';
  if (d.sourceKind === 'ytdlp') return 'film';
  return fileIconName(d.category);
}

function rowIconClass(d) {
  return isTorrent(d) ? 'torrent' : fileIconClass(d.category);
}

function progressFillClass(status) {
  if (status === 'completed') return 'green';
  if (status === 'paused') return 'amber';
  if (status === 'failed' || status === 'stopped') return 'red';
  return '';
}

/* ── Rendering ───────────────────────────────────────────────── */

let renderedIds = [];
let renderedSig = null;

function folderOf(d) {
  const p = String(d.savePath || '');
  const idx = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
  return idx >= 0 ? p.slice(0, idx) : p || 'Downloads';
}

let filesCurrentFolder = null;
let filesSig = null;

function renderFiles() {
  if (!$filesList) return;
  // Group downloads by their containing folder.
  const groups = new Map();
  for (const d of downloads) {
    const f = folderOf(d);
    if (!groups.has(f)) groups.set(f, []);
    groups.get(f).push(d);
  }
  // Progress ticks don't change the folder tree. Rebuilding the whole list
  // (and reattaching every listener) on each tick was the main source of UI
  // jank; only rebuild when the structure or a file's status actually changes.
  const sig = JSON.stringify([
    filesCurrentFolder,
    [...groups.entries()].map(([f, items]) => [
      f,
      items.map((d) => [d.id, d.status, d.fileName]),
    ]),
  ]);
  if (sig === filesSig) return;
  filesSig = sig;
  if (!groups.size) {
    $filesList.innerHTML = '';
    if ($filesEmpty) $filesEmpty.hidden = false;
    const ic = document.getElementById('filesEmptyIcon');
    if (ic) ic.innerHTML = icon('folder');
    return;
  }
  if ($filesEmpty) $filesEmpty.hidden = true;

  // Folder list view
  if (filesCurrentFolder === null) {
    if ($btnFilesBack) $btnFilesBack.hidden = true;
    if ($filesPath) $filesPath.textContent = 'Downloads';
    $filesList.innerHTML = [...groups.entries()]
      .map(([folder, items]) => {
        const name = folder.split(/[\\/]/).filter(Boolean).pop() || folder;
        const count = items.length;
        const done = items.filter((d) => d.status === 'completed').length;
        return `
          <button class="file-row folder-row" type="button" data-folder="${esc(folder)}">
            <span class="file-icon folder">${icon('folder')}</span>
            <span class="file-name" title="${esc(folder)}">${esc(name)}</span>
            <span class="file-meta">${count} file${count === 1 ? '' : 's'} \u00b7 ${done} done</span>
            <span class="file-action">${icon('chevron-right')}</span>
          </button>`;
      })
      .join('');

    $filesList.querySelectorAll('.folder-row').forEach((row) => {
      row.addEventListener('click', () => {
        filesCurrentFolder = row.getAttribute('data-folder');
        renderFiles();
      });
    });
    return;
  }

  // Inside a folder: show its files
  const items = groups.get(filesCurrentFolder) || [];
  if ($btnFilesBack) $btnFilesBack.hidden = false;
  if ($filesPath) {
    const parts = String(filesCurrentFolder).split(/[\\/]/).filter(Boolean);
    $filesPath.textContent = parts.join(' \u203a ');
  }
  if (!items.length) {
    $filesList.innerHTML = '<p class="files-empty-inline">No files in this folder.</p>';
    return;
  }
  $filesList.innerHTML = items
    .map((d) => {
      const size = d.status === 'completed' ? sizeText(d) : '\u2014';
      return `
        <div class="file-row">
          <span class="file-icon ${fileIconClass(d.category)}">${icon(fileIconName(d.category))}</span>
          <span class="file-name" title="${esc(d.fileName)}">${esc(d.fileName)}</span>
          <span class="file-meta">${esc(size)} \u00b7 ${esc(statusLabel(d.status))}</span>
          <span class="file-actions">
            <button class="icon-btn" data-action="open-file" data-id="${esc(d.id)}"
                    title="Open file" ${d.status === 'completed' ? '' : 'hidden'}
                    aria-label="Open file">${icon('file')}</button>
            <button class="icon-btn" data-action="open-folder" data-id="${esc(d.id)}"
                    title="Open containing folder" aria-label="Open containing folder">${icon('external-link')}</button>
          </span>
        </div>`;
    })
    .join('');

  $filesList.querySelectorAll('button[data-action]').forEach((btn) => {
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      const id = btn.getAttribute('data-id');
      const action = btn.getAttribute('data-action');
      if (action === 'open-file') openFile(id);
      if (action === 'open-folder') openContainingFolder(id);
    });
  });
}

if ($btnFilesBack) {
  $btnFilesBack.addEventListener('click', () => {
    filesCurrentFolder = null;
    renderFiles();
  });
}


function clampPct(v) {
  return Math.max(0, Math.min(100, Number(v) || 0));
}

function rowHtml(d) {
  const pct = clampPct(d.percent);
  const isDl = d.status === 'downloading';
  const indeterminate = isIndeterminate(d);
  const canToggle = d.status !== 'completed';
  const fileIcon = `<div class="file-icon ${rowIconClass(d)}">${icon(rowIconName(d))}</div>`;
  const showErr = d.status === 'failed' && d.error;
  const sourceTag = isTorrent(d)
    ? `<div class="source-tag torrent">torrent</div>`
    : d.sourceKind === 'ytdlp'
      ? `<div class="source-tag">yt-dlp</div>`
      : '';
  return `
    <div class="download-row ${isDl ? 'is-active' : ''}" data-id="${esc(d.id)}">
      <div class="file-cell">
        ${fileIcon}
        <div class="file-meta">
          <div class="file-name" data-role="name">${esc(d.fileName)}</div>
          <div class="file-sub" data-role="path">${esc(d.savePath)}</div>
          ${sourceTag}
          <div class="file-error" data-role="error" ${showErr ? '' : 'hidden'} title="${esc(showErr ? d.error : '')}">${esc(showErr ? d.error : '')}</div>
        </div>
      </div>
      <div>
        <span class="status-pill ${statusClass(d.status)}" data-role="status" data-status="${esc(d.status)}">
          ${icon(statusIcon(d.status))} ${statusLabel(d.status)}
        </span>
      </div>
      <div class="progress-wrap">
        <div class="progress-bar-bg ${indeterminate ? 'indeterminate' : ''}" data-role="track">
          <div class="progress-bar-fill ${progressFillClass(d.status)}" data-role="fill" style="width:${pct}%"></div>
        </div>
        <div class="progress-text" data-role="pct">${indeterminate ? '—' : `${pct}%`}</div>
      </div>
      <div class="cell-muted" data-role="size">${sizeText(d)}</div>
      <div class="cell-muted" data-role="speed">${isDl ? formatSpeed(d.speedBps) : '—'}</div>
      <div class="cell-muted" data-role="eta">${formatEta(d)}</div>
      <div class="row-actions">
        <button class="icon-btn" data-role="action" data-action="${isDl ? 'pause' : 'resume'}" data-id="${esc(d.id)}" title="${isDl ? 'Pause' : 'Resume'}" ${canToggle ? '' : 'hidden'} aria-label="${isDl ? 'Pause' : 'Resume'} download">${icon(isDl ? 'pause' : 'play')}</button>
        <button class="icon-btn" data-action="open-folder" data-id="${esc(d.id)}" title="Open containing folder" aria-label="Open containing folder">${icon('external-link')}</button>
        <button class="icon-btn" data-action="open-file" data-id="${esc(d.id)}" title="Open file" ${d.status === 'completed' ? '' : 'hidden'} aria-label="Open file">${icon('file')}</button>
        <button class="icon-btn" data-action="retry" data-id="${esc(d.id)}" title="Retry" ${(d.status === 'failed' || d.status === 'stopped') ? '' : 'hidden'} aria-label="Retry download">${icon('rotate-ccw')}</button>
        <button class="icon-btn danger" data-action="remove" data-id="${esc(d.id)}" title="Remove" aria-label="Remove download">${icon('trash-2')}</button>
      </div>
    </div>`;
}

function patchRow(row, d) {
  const pct = clampPct(d.percent);
  const isDl = d.status === 'downloading';
  const indeterminate = isIndeterminate(d);

  const pill = row.querySelector('[data-role="status"]');
  if (pill && pill.dataset.status !== d.status) {
    // Re-parsing an SVG on every 400ms tick is wasted work — only rebuild the
    // pill when the status actually changes.
    pill.dataset.status = d.status;
    pill.className = `status-pill ${statusClass(d.status)}`;
    pill.innerHTML = `${icon(statusIcon(d.status))} ${statusLabel(d.status)}`;
  }

  const track = row.querySelector('[data-role="track"]');
  if (track) track.classList.toggle('indeterminate', indeterminate);

  const fill = row.querySelector('[data-role="fill"]');
  if (fill) {
    fill.style.width = `${pct}%`;
    fill.className = `progress-bar-fill ${progressFillClass(d.status)}`;
  }

  const pctEl = row.querySelector('[data-role="pct"]');
  if (pctEl) pctEl.textContent = indeterminate ? '—' : `${pct}%`;

  const size = row.querySelector('[data-role="size"]');
  if (size) size.textContent = sizeText(d);

  const speed = row.querySelector('[data-role="speed"]');
  if (speed) speed.textContent = isDl ? formatSpeed(d.speedBps) : '—';

  const eta = row.querySelector('[data-role="eta"]');
  if (eta) eta.textContent = formatEta(d);

  const err = row.querySelector('[data-role="error"]');
  if (err) {
    const show = d.status === 'failed' && d.error;
    err.hidden = !show;
    err.textContent = show ? d.error : '';
    err.title = show ? d.error : '';
  }

  // Torrent metadata can rename the row after the download starts.
  const nameEl = row.querySelector('[data-role="name"]');
  if (nameEl && nameEl.textContent !== String(d.fileName || '')) {
    nameEl.textContent = String(d.fileName || '');
  }
  const pathEl = row.querySelector('[data-role="path"]');
  if (pathEl && pathEl.textContent !== String(d.savePath || '')) {
    pathEl.textContent = String(d.savePath || '');
  }

  const act = row.querySelector('[data-role="action"]');
  if (act) {
    const canToggle = d.status !== 'completed';
    const action = isDl ? 'pause' : 'resume';
    act.hidden = !canToggle;
    if (act.dataset.action !== action) {
      act.dataset.action = action;
      act.title = isDl ? 'Pause' : 'Resume';
      act.innerHTML = icon(isDl ? 'pause' : 'play');
    }
  }

  row.classList.toggle('is-active', isDl);
}

let renderScheduled = false;
function scheduleRender() {
  if (renderScheduled) return;
  renderScheduled = true;
  requestAnimationFrame(() => {
    renderScheduled = false;
    render();
  });
}

function setText(id, value) {
  const el = document.getElementById(id);
  if (el) el.textContent = value;
}

// One pass over the list for every badge and status-bar value. Progress events
// arrive several times a second, so the old five separate filter() scans per
// render were pure overhead.
function updateCountsAndStats() {
  const counts = { downloading: 0, completed: 0, paused: 0, failed: 0 };
  let activeSpeed = 0;
  let totalDown = 0;
  for (const d of downloads) {
    if (counts[d.status] !== undefined) counts[d.status]++;
    if (d.status === 'downloading') activeSpeed += Number(d.speedBps) || 0;
    totalDown += Number(d.downloadedBytes) || 0;
  }
  setText('countAll', downloads.length);
  setText('countDownloading', counts.downloading);
  setText('countCompleted', counts.completed);
  setText('countPaused', counts.paused);
  setText('countFailed', counts.failed);
  setText('activeCount', counts.downloading);
  setText('globalSpeed', formatSpeed(activeSpeed));
  setText('totalDownloaded', formatBytes(totalDown));
}

function syncViewButtons() {
  const inFiles = activeView === 'files';
  if ($tabLibrary) $tabLibrary.classList.toggle('active', !inFiles);
  if ($tabFiles) $tabFiles.classList.toggle('active', inFiles);
  document.querySelectorAll('#navPrimary .nav-item').forEach((b) => {
    let active = false;
    if (inFiles) active = b.dataset.view === 'files';
    else if (b.dataset.filter) active = b.dataset.filter === activeFilter && !activeCategory;
    b.classList.toggle('active', active);
  });
}

function setView(view) {
  if (view === 'files') {
    activeView = 'files';
    activeCategory = null;
    document.querySelectorAll('#navCategories .nav-item').forEach((b) => b.classList.remove('active'));
  } else {
    activeView = 'list';
    activeFilter = 'all';
    activeCategory = null;
  }
  render();
}

function render() {
  syncViewButtons();
  // Toggle between the download list and the Files folder view. This is the
  // single source of truth — no second, disconnected view is created anywhere.
  if (activeView === 'files') {
    $list.hidden = true;
    if ($filesView) $filesView.hidden = false;
    renderFiles();
    updateCountsAndStats();
    return;
  }
  $list.hidden = false;
  if ($filesView) $filesView.hidden = true;

  const q = $search.value.trim().toLowerCase();
  const filtered = downloads.filter((d) => {
    if (activeFilter !== 'all' && d.status !== activeFilter) return false;
    if (activeCategory && d.category !== activeCategory) return false;
    if (
      q &&
      !String(d.fileName || '')
        .toLowerCase()
        .includes(q) &&
      !String(d.savePath || '')
        .toLowerCase()
        .includes(q)
    )
      return false;
    return true;
  });

  const sig = filtered.map((d) => d.id).join('|');

  if (sig !== renderedSig) {
    // Row set changed — rebuild, and animate only the rows that just appeared.
    const prev = new Set(renderedIds);
    const entering = filtered.filter((d) => !prev.has(d.id)).map((d) => d.id);

    if (filtered.length === 0) {
      $list.innerHTML = `<div class="empty-state"><div class="big">${icon('download')}</div><p>No downloads match this view.</p></div>`;
    } else {
      $list.innerHTML = filtered.map(rowHtml).join('');
    }

    let i = 0;
    for (const id of entering) {
      const row = $list.querySelector(`.download-row[data-id="${id}"]`);
      if (row) {
        row.classList.add('row-enter');
        row.style.animationDelay = `${Math.min(i * 40, 160)}ms`;
        setTimeout(() => row.classList.remove('row-enter'), 760);
      }
      i++;
    }

    renderedIds = filtered.map((d) => d.id);
    renderedSig = sig;
  } else {
    // Same row set — patch values in place so bars and counters animate smoothly.
    for (const d of filtered) {
      const row = $list.querySelector(`.download-row[data-id="${d.id}"]`);
      if (row) patchRow(row, d);
    }
  }

  updateCountsAndStats();
  syncToolbar();
}

function syncToolbar() {
  const canPause = downloads.some((d) => d.status === 'downloading');
  const canResume = downloads.some(
    (d) => d.status === 'paused' || d.status === 'failed' || d.status === 'stopped'
  );
  const canClear = downloads.some((d) => d.status === 'completed');

  if ($btnPause) {
    $btnPause.disabled = !canPause;
    $btnPause.title = canPause ? 'Pause all downloads' : 'Nothing is downloading';
  }
  if ($btnResume) {
    $btnResume.disabled = !canResume;
    $btnResume.title = canResume ? 'Resume all paused or failed downloads' : 'Nothing to resume';
  }
  if ($btnRemove) {
    $btnRemove.disabled = !canClear;
    $btnRemove.title = canClear ? 'Remove completed downloads from the list' : 'No completed downloads';
  }
}

/* ── Data actions ───────────────────────────────────────────── */

async function refresh() {
  try {
    downloads = await invoke('list_downloads');
  } catch (e) {
    // transient error (e.g. backend busy) — keep the last known list
  }
  announceTerminalStates();
  render();
}

function announceTerminalStates() {
  for (const d of downloads) {
    if (d.status === 'completed' || d.status === 'failed') {
      announceTerminal(d);
    }
  }
}

function announceTerminal(d) {
  const key = `${d.id}:${d.status}`;
  if (announced.has(key)) return;
  announced.add(key);
  if (d.status === 'completed') {
    toast('Download complete', d.fileName, 'success');
  } else if (d.status === 'failed') {
    toast('Download failed', d.error ? `${d.fileName} — ${d.error}` : d.fileName, 'error', 6500);
  }
}

async function handleProgress(e) {
  const p = e.payload;
  const d = downloads.find((x) => x.id === p.id);
  if (!d) {
    // Terminal event for a row we haven't listed yet (fast download) — pull the
    // list so the row and its toast aren't lost.
    if (p.status === 'completed' || p.status === 'failed') await refresh();
    return;
  }
  const prev = d.status;
  d.status = p.status;
  d.percent = p.percent ?? 0;
  d.downloadedBytes = p.downloadedBytes ?? 0;
  d.totalBytes = p.totalBytes ?? null;
  d.speedBps = p.speedBps ?? null;
  d.error = p.error ?? null;
  if (p.fileName) d.fileName = p.fileName;
  if (d.status === 'downloading' || d.status === 'paused' || d.status === 'stopped') {
    announced.delete(`${d.id}:completed`);
    announced.delete(`${d.id}:failed`);
  }
  scheduleRender();
  if (prev !== d.status) announceTerminal(d);
}

async function toggleDownload(id, action) {
  const d = downloads.find((x) => x.id === id);
  if (!d) return;
  if (action === 'pause') {
    if (d.status !== 'downloading') return;
    const prev = d.status;
    d.status = 'paused';
    d.speedBps = 0;
    render();
    try {
      await invoke('pause_download', { id });
    } catch (e) {
      d.status = prev;
      render();
      toast('Could not pause download', String(e), 'error', 6000);
    }
  } else {
    if (d.status !== 'paused' && d.status !== 'failed' && d.status !== 'stopped') return;
    const prev = d.status;
    d.status = 'downloading';
    d.error = null;
    render();
    try {
      await invoke('resume_download', { id });
      await refresh();
    } catch (e) {
      d.status = prev;
      render();
      toast('Could not resume download', String(e), 'error', 6000);
    }
  }
}

async function removeDownload(id) {
  const d = downloads.find((x) => x.id === id);
  try {
    await invoke('remove_download', { id });
    await refresh();
    toast('Removed from list', d ? d.fileName : '', 'info', 2500);
  } catch (e) {
    toast('Could not remove download', String(e), 'error', 6000);
  }
}

async function openContainingFolder(id) {
  try { await invoke('open_containing_folder', { id }); }
  catch (e) { toast('Could not open folder', String(e), 'error', 6000); }
}
async function openFile(id) {
  try { await invoke('open_file', { id }); }
  catch (e) { toast('Could not open file', String(e), 'error', 6000); }
}
async function retryDownloadRow(id) {
  try {
    await invoke('retry_download', { id });
    await refresh();
    toast('Download restarted', '', 'info', 3000);
  } catch (e) { toast('Could not retry download', String(e), 'error', 6000); }
}
async function setSpeedLimit(kbps) {
  try { await invoke('set_speed_limit', { limit_kbps: kbps }); }
  catch (e) { toast('Could not set speed limit', String(e), 'error', 6000); }
}
async function getSpeedLimit() {
  return await invoke('get_speed_limit');
}
async function setSchedule(id, startAt) {
  try { await invoke('set_schedule', { id, start_at: startAt }); }
  catch (e) { toast('Could not set schedule', String(e), 'error', 6000); }
}

async function addDownload(url, saveDir, startAt, mediaFormat) {
  const item = await invoke('add_download', {
    url,
    saveDir: saveDir ?? undefined,
    startAt: startAt ?? undefined,
    mediaFormat: mediaFormat ?? undefined,
  });
  downloads = await invoke('list_downloads');
  announceTerminalStates();
  render();
  const current = downloads.find((x) => x.id === item?.id);
  const alreadyDone = current && (current.status === 'completed' || current.status === 'failed');
  if (item && item.fileName && !alreadyDone) {
    toast('Download started', item.fileName, 'info', 3000);
  }
  return item;
}

function looksLikeDownloadSource(s) {
  const t = String(s || '').trim().toLowerCase();
  if (!t) return false;
  // Keep this in sync with the backend's `classify_source`: FTP is not
  // supported yet, so it must fall through to the manual input instead of
  // auto-submitting and failing.
  if (t.startsWith('magnet:')) return true;
  if (t.startsWith('http://') || t.startsWith('https://')) return true;
  if (t.endsWith('.torrent')) return true;
  return false;
}

async function readClipboardText() {
  // Native read first: works regardless of WebView focus and avoids the
  // clipboard permission prompt.
  try {
    const text = await invoke('read_clipboard');
    if (typeof text === 'string') return text;
  } catch (e) {
    // command unavailable — fall back to the web API
  }
  try {
    if (navigator.clipboard && typeof navigator.clipboard.readText === 'function') {
      const t = await navigator.clipboard.readText();
      return String(t || '');
    }
  } catch (e) {
    // clipboard unavailable / permission denied — treat as empty
  }
  return '';
}

let modalCloseTimer = null;
let modalSession = 0;

function setAdding(on) {
  if (!$modalAdd) return;
  $modalAdd.disabled = on;
  $modalAdd.classList.toggle('is-busy', on);
  $modalAdd.innerHTML = on ? `<span class="btn-spinner" aria-hidden="true"></span> Adding…` : 'Add download';
}

function openModal() {
  modalSession++;
  if (modalCloseTimer) {
    clearTimeout(modalCloseTimer);
    modalCloseTimer = null;
  }
  $modalBackdrop.classList.remove('closing');
  $modalBackdrop.hidden = false;
  setAdding(false);
  $saveDirLabel.textContent = selectedSaveDir ? shortDir(selectedSaveDir) : 'Downloads';
  $saveDirLabel.title = selectedSaveDir || '';
  $modalUrl.value = '';
  syncFormatVisibility();
  setTimeout(() => $modalUrl.focus(), 50);
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function shortDir(p) {
  const parts = String(p).split(/[\\/]/).filter(Boolean);
  return parts.length ? parts[parts.length - 1] : p;
}

function closeModal() {
  if ($modalBackdrop.hidden || $modalBackdrop.classList.contains('closing')) return;
  $modalBackdrop.classList.add('closing');
  modalCloseTimer = setTimeout(() => {
    $modalBackdrop.hidden = true;
    $modalBackdrop.classList.remove('closing');
    modalCloseTimer = null;
  }, 190);
}

/* ── Folder routing settings ─────────────────────────────────── */

const RULE_KINDS = [
  ['extension', 'File ends with'],
  ['keyword', 'Name contains'],
];

function escAttr(s) {
  return String(s).replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;');
}

function renderRoutingRules() {
  if (!$routingRules || !routingDraft) return;
  if (!routingDraft.rules.length) {
    $routingRules.innerHTML =
      '<p class="routing-empty">No rules yet. Everything lands in the root folder.</p>';
    return;
  }
  $routingRules.innerHTML = routingDraft.rules
    .map(
      (r, i) => `
      <div class="routing-rule" data-i="${i}">
        <input type="checkbox" class="rr-enabled" ${r.enabled ? 'checked' : ''}
               title="Enable this rule" aria-label="Enable rule" />
        <select class="rr-kind" aria-label="Rule type">
          ${RULE_KINDS.map(
            ([v, label]) =>
              `<option value="${v}"${r.kind === v ? ' selected' : ''}>${label}</option>`
          ).join('')}
        </select>
        <input type="text" class="rr-pattern" value="${escAttr(r.pattern)}"
               placeholder="e.g. game or .iso" spellcheck="false" aria-label="Rule pattern" />
        <span class="rr-arrow" aria-hidden="true">→</span>
        <input type="text" class="rr-folder" value="${escAttr(r.folder)}"
               placeholder="Subfolder" spellcheck="false" aria-label="Destination subfolder" />
        <button class="icon-btn danger rr-remove" title="Remove rule"
                aria-label="Remove rule">${icon('trash-2')}</button>
      </div>`
    )
    .join('');

  $routingRules.querySelectorAll('.routing-rule').forEach((row) => {
    const i = Number(row.getAttribute('data-i'));
    const rule = () => routingDraft.rules[i];
    row.querySelector('.rr-enabled').addEventListener('change', (e) => {
      rule().enabled = e.target.checked;
    });
    row.querySelector('.rr-kind').addEventListener('change', (e) => {
      rule().kind = e.target.value;
    });
    row.querySelector('.rr-pattern').addEventListener('input', (e) => {
      rule().pattern = e.target.value;
    });
    row.querySelector('.rr-folder').addEventListener('input', (e) => {
      rule().folder = e.target.value;
    });
    row.querySelector('.rr-remove').addEventListener('click', () => {
      routingDraft.rules.splice(i, 1);
      renderRoutingRules();
    });
  });
}

async function openRoutingModal() {
  try {
    routingDraft = await invoke('get_routing_config');
  } catch (e) {
    // Tauri IPC is unavailable (plain browser / dev preview). Fall back to
    // defaults so the panel still opens, matching how readClipboardText
    // degrades instead of blocking the UI entirely.
    routingDraft = { enabled: true, root: '', rules: [] };
  }
  if (!routingDraft) routingDraft = { enabled: true, root: '', rules: [] };
  if (!Array.isArray(routingDraft.rules)) routingDraft.rules = [];
  $routingRoot.value = routingDraft.root || '';
  $routingEnabled.checked = !!routingDraft.enabled;
  renderRoutingRules();
  $routingBackdrop.hidden = false;
}

function closeRoutingModal() {
  $routingBackdrop.hidden = true;
  routingDraft = null;
}

async function saveRoutingModal() {
  if (!routingDraft) return;
  routingDraft.root = $routingRoot.value.trim();
  routingDraft.enabled = $routingEnabled.checked;
  // Drop rules that match nothing and lead nowhere — they'd only slow the
  // walk down and clutter the list.
  routingDraft.rules = routingDraft.rules.filter(
    (r) => r.pattern.trim() && r.folder.trim()
  );
  try {
    routingDraft = await invoke('set_routing_config', { config: routingDraft });
    toast('Folder rules saved', 'New downloads will use them from now on.', 'success');
    closeRoutingModal();
  } catch (e) {
    toast('Could not save folder rules', String(e), 'error', 6000);
  }
}

/* ── Window controls ─────────────────────────────────────────── */

function setMaxIcon(maximized) {
  const btn = document.querySelector('.wbtn[data-action="maximize"]');
  if (!btn) return;
  const mx = btn.querySelector('.ic-max');
  const rs = btn.querySelector('.ic-restore');
  if (mx) mx.hidden = maximized;
  if (rs) rs.hidden = !maximized;
  const label = maximized ? 'Restore Down' : 'Maximize';
  btn.title = label;
  // Keep the accessible name in sync too — screen readers and UIA read
  // aria-label, not title, so it used to stay "Maximize" forever.
  btn.setAttribute('aria-label', label);
}

async function windowAction(action) {
  const win = getCurrentWindow();
  if (action === 'close') {
    await win.close();
  } else if (action === 'minimize') {
    await win.minimize();
  } else if (action === 'maximize') {
    await win.toggleMaximize();
    try {
      setMaxIcon(await win.isMaximized());
    } catch (e) {
      // ignore — icon toggle is cosmetic
    }
  }
}

/* ── Init ────────────────────────────────────────────────────── */

async function init() {
  try {
    await listen('download-progress', handleProgress);
  } catch (e) {
    // Tauri event API unavailable (e.g. plain browser context) — continue without live updates
  }

  // Drag & drop: accept .torrent files dropped anywhere in the window
  try {
    await listen('tauri://drag-drop', async (e) => {
      const paths = (e.payload && e.payload.paths) || [];
      let added = 0;
      for (const p of paths) {
        if (String(p).toLowerCase().endsWith('.torrent')) {
          try {
            await addDownload(String(p), selectedSaveDir);
            added++;
          } catch (err) {
            toast('Could not add torrent', String(err), 'error', 6000);
          }
        }
      }
      if (paths.length && added === 0) {
        toast('No torrent added', 'Only .torrent files can be dropped here.', 'info', 3500);
      }
    });
  } catch (e) {
    // drag-drop event API unavailable — ignore
  }

  await refresh();

  document.querySelectorAll('[data-icon]').forEach((el) => {
    const name = el.getAttribute('data-icon');
    if (name) el.innerHTML = icon(name);
  });

  document.querySelectorAll('#navPrimary .nav-item').forEach((btn) => {
    btn.addEventListener('click', () => {
      if (btn.id === 'btnFolders') return; // opens the routing modal, not a view
      if (btn.dataset.view === 'files') {
        setView('files');
        return;
      }
      activeView = 'list';
      activeFilter = btn.dataset.filter || 'all';
      activeCategory = null;
      document.querySelectorAll('#navCategories .nav-item').forEach((b) => b.classList.remove('active'));
      render();
    });
  });

  document.querySelectorAll('#navCategories .nav-item').forEach((btn) => {
    btn.addEventListener('click', () => {
      const cat = btn.dataset.category;
      document.querySelectorAll('#navCategories .nav-item').forEach((b) => b.classList.remove('active'));
      if (activeCategory === cat) {
        btn.classList.remove('active');
        activeCategory = null;
      } else {
        btn.classList.add('active');
        activeCategory = cat;
      }
      activeFilter = 'all';
      activeView = 'list';
      document.querySelectorAll('#navPrimary .nav-item').forEach((b) => b.classList.remove('active'));
      document.querySelector('#navPrimary .nav-item[data-filter="all"]').classList.add('active');
      render();
    });
  });

  $search.addEventListener('input', () => {
    render();
  });

  $btnNew.addEventListener('click', openModal);

  $btnPause.addEventListener('click', async () => {
    const actives = downloads.filter((d) => d.status === 'downloading');
    if (!actives.length) return;
    for (const d of actives) {
      d.status = 'paused';
      d.speedBps = 0;
    }
    render();
    try {
      for (const d of actives) {
        await invoke('pause_download', { id: d.id });
      }
    } catch (e) {
      toast('Could not pause downloads', String(e), 'error', 6000);
      await refresh();
    }
  });

  $btnResume.addEventListener('click', async () => {
    const targets = downloads.filter((d) => d.status === 'paused' || d.status === 'failed' || d.status === 'stopped');
    if (!targets.length) return;
    for (const d of targets) {
      d.status = 'downloading';
      d.error = null;
    }
    render();
    for (const d of targets) {
      try {
        await invoke('resume_download', { id: d.id });
      } catch (e) {
        toast('Could not resume download', `${d.fileName} — ${e}`, 'error', 6000);
      }
    }
    await refresh();
  });

  $btnRemove.addEventListener('click', async () => {
    const completed = downloads.filter((d) => d.status === 'completed');
    if (!completed.length) return;
    let removed = 0;
    for (const d of completed) {
      try {
        await invoke('remove_download', { id: d.id });
        removed++;
      } catch (e) {
        // keep going — remove the rest
      }
    }
    await refresh();
    if (removed > 0) {
      toast('Removed from list', `${removed} completed ${removed === 1 ? 'download' : 'downloads'}`, 'info', 2500);
    }
  });

  $modalAdd.addEventListener('click', async () => {
    if ($modalAdd.disabled) return;
    const url = $modalUrl.value.trim();
    if (!url) {
      toast('Nothing to add', 'Paste a link, magnet, or pick a .torrent file.', 'info', 3500);
      return;
    }
    const session = modalSession;
    const startedAt = performance.now();
    const startAt = $scheduleStart ? new Date($scheduleStart.value).getTime() : undefined;
    const mediaFormat = chosenMediaFormat();
    setAdding(true);
    try {
      await addDownload(url, selectedSaveDir, startAt, mediaFormat);
      await delay(Math.max(0, 620 - (performance.now() - startedAt)));
      if (session !== modalSession) return;
      setAdding(false);
      closeModal();
    } catch (e) {
      if (session !== modalSession) return;
      setAdding(false);
      toast('Could not add download', String(e), 'error', 6000);
    }
  });

  $modalSaveDirBtn.addEventListener('click', async () => {
    try {
      const dir = await open({ directory: true, title: 'Choose save folder' });
      if (dir) {
        selectedSaveDir = String(dir);
        $saveDirLabel.textContent = shortDir(selectedSaveDir);
        $saveDirLabel.title = selectedSaveDir;
      }
    } catch (e) {
      // dialog unavailable — ignore
    }
  });

  $modalCancel.addEventListener('click', closeModal);

  // ── Folder routing modal ──
  $btnFolders.addEventListener('click', openRoutingModal);
  $btnRoutingCancel.addEventListener('click', closeRoutingModal);
  $btnRoutingSave.addEventListener('click', saveRoutingModal);
  $btnRoutingAdd.addEventListener('click', () => {
    if (!routingDraft) return;
    routingDraft.rules.push({
      kind: 'keyword',
      pattern: '',
      folder: '',
      enabled: true,
    });
    renderRoutingRules();
    // Focus the new rule's pattern box so typing starts immediately.
    const rows = $routingRules.querySelectorAll('.rr-pattern');
    if (rows.length) rows[rows.length - 1].focus();
  });
  $btnRoutingRoot.addEventListener('click', async () => {
    try {
      const dir = await open({ directory: true, title: 'Choose the root download folder' });
      if (dir) $routingRoot.value = String(dir);
    } catch (e) {
      // dialog unavailable — the text field still allows manual entry
    }
  });
  $routingBackdrop.addEventListener('click', (e) => {
    if (e.target === $routingBackdrop) closeRoutingModal();
  });

  $modalUrl.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      $modalAdd.click();
    }
  });

  // Show the format picker only for links yt-dlp will actually handle, and
  // keep it in sync as the user types or pastes.
  $modalUrl.addEventListener('input', syncFormatVisibility);
  $modalUrl.addEventListener('paste', () => setTimeout(syncFormatVisibility, 0));

  $modalBrowse.addEventListener('click', async () => {
    try {
      const file = await open({
        multiple: false,
        title: 'Select a torrent file',
        filters: [{ name: 'Torrent files', extensions: ['torrent'] }],
      });
      if (file) {
        $modalUrl.value = String(file);
        $modalUrl.focus();
      }
    } catch (e) {
      // dialog unavailable — ignore
    }
  });

  $modalBackdrop.addEventListener('click', (e) => {
    if (e.target === $modalBackdrop) closeModal();
  });

  document.addEventListener('keydown', (e) => {
    if (e.key !== 'Escape') return;
    // Close the topmost open modal. The routing dialog must dismiss first so
    // one Escape never leaves a backdrop stranded on screen.
    if ($routingBackdrop && !$routingBackdrop.hidden) {
      closeRoutingModal();
      return;
    }
    if (!$modalBackdrop.hidden) {
      closeModal();
    }
  });

  $list.addEventListener('click', async (e) => {
    const btn = e.target.closest('.icon-btn');
    if (!btn) return;
    const id = btn.dataset.id;
    const action = btn.dataset.action;
    if (action === 'pause') await toggleDownload(id, 'pause');
    if (action === 'resume') await toggleDownload(id, 'resume');
    if (action === 'remove') await removeDownload(id);
    if (action === 'open-folder') await openContainingFolder(id);
    if (action === 'open-file') await openFile(id);
    if (action === 'retry') await retryDownloadRow(id);
  });

  document.querySelectorAll('.wbtn[data-action]').forEach((el) => {
    el.addEventListener('click', async () => {
      const action = el.dataset.action;
      if (action === 'close' || action === 'minimize' || action === 'maximize') {
        try {
          await windowAction(action);
        } catch (e) {
          // window action rejected (permission/teardown) — nothing useful to show
        }
      }
    });
  });

  // Keep the maximize/restore glyph in sync with the real window state,
  // including when the user double-clicks the titlebar.
  try {
    const win = getCurrentWindow();
    setMaxIcon(await win.isMaximized());
    await win.onResized(async () => {
      try {
        setMaxIcon(await win.isMaximized());
      } catch (e) {
        // ignore — icon is cosmetic
      }
    });
  } catch (e) {
    // window API unavailable (browser preview) — ignore
  }

  // Whole titlebar is draggable (incl. double-click to maximize); buttons opt out.
  const titlebar = document.querySelector('.titlebar');
  if (titlebar) titlebar.setAttribute('data-tauri-drag-region', 'deep');
}

/* ── Dark mode ── */
(function () {
  const $themeToggle = document.getElementById('btnThemeToggle');
  const saved = localStorage.getItem('faast-theme');
  // Dark is the default shell; light is opt-in via the theme toggle.
  const wantDark = saved !== 'light';
  if (wantDark) document.documentElement.dataset.theme = 'dark';
  $themeToggle?.addEventListener('click', () => {
    const isDark = document.documentElement.dataset.theme === 'dark';
    document.documentElement.dataset.theme = isDark ? '' : 'dark';
    localStorage.setItem('faast-theme', isDark ? 'light' : 'dark');
    $themeToggle.title = isDark ? 'Switch to dark mode' : 'Switch to light mode';
  });
})();

/* ── Global speed limit ── */
(function () {
  const $limitInput = document.getElementById('limitKbps');
  const $limitLabel = document.getElementById('globalLimit');
  async function syncSpeedLimit() {
    try {
      const kbps = await getSpeedLimit();
      const none = kbps == null || kbps <= 0;
      $limitLabel.textContent = none ? 'Unlimited' : Math.round(kbps);
      if ($limitInput) $limitInput.value = none ? '' : Math.round(kbps);
    } catch (e) {
      // backend unavailable in browser preview — ignore
    }
  }
  syncSpeedLimit();
  $limitInput?.addEventListener('change', async () => {
    const v = Number($limitInput.value);
    if (!Number.isFinite(v) || v < 0) return;
    await setSpeedLimit(v === 0 ? null : v);
    syncSpeedLimit();
  });
})();

/* ── Keyboard shortcuts ── */
(function () {
  const keyboardShortcuts = {
    // 37: left, 38: up, 39: right, 40: down
    // Not using arrow keys; reserving for future layout nav
    'KeyN': async () => { const $btn = document.getElementById('btnNew'); if ($btn) $btn.click(); },
    'KeyF': async () => { const $input = document.getElementById('searchInput'); if ($input) { $input.focus(); $input.select(); } },
    'Space': async (e) => {
      const $row = e.target.closest('.download-row');
      if ($row) { const $btn = $row.querySelector('[data-action="pause"],[data-action="resume"]'); if ($btn) $btn.click(); }
    },
    'Delete': async (e) => {
      const $row = e.target.closest('.download-row');
      if ($row) { const $btn = $row.querySelector('[data-action="remove"]'); if ($btn) $btn.click(); }
    }
  };

  document.addEventListener('keydown', e => {
    const handler = keyboardShortcuts[e.code];
    if (handler) {
      e.preventDefault();
      handler(e);
    }
  });
})();

/* Titlebar Library/Files tabs drive the same view state as the sidebar. */
$tabLibrary?.addEventListener('click', () => setView('library'));
$tabFiles?.addEventListener('click', () => setView('files'));

init();
