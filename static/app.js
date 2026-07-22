/* Content Sync Web UI */
const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];

const state = {
  sessionId: localStorage.getItem("sa_session") || "",
  user: null,
  modalMode: null,
  modalId: null,
  /** Login bootstrap: whether any auth tokens exist in config DB */
  hasAuthTokens: true,
};

/** Client-side list pagination (API returns full arrays) */
const PAGE_SIZE = 15;
const listCache = { logs: [], files: [], connections: [], auth: [] };
const listPage = { logs: 1, files: 1, connections: 1, auth: 1 };
/** Search / filter state per list (client-side on cached arrays) */
const listFilter = {
  logs: { q: "", level: "" },
  files: { q: "", connectionId: "" },
  connections: { q: "", driver: "", status: "" },
  auth: { q: "", status: "" },
};

function paginate(items, page, pageSize = PAGE_SIZE) {
  const total = items.length;
  const totalPages = Math.max(1, Math.ceil(total / pageSize) || 1);
  const p = Math.min(Math.max(1, page || 1), totalPages);
  const start = (p - 1) * pageSize;
  return {
    page: p,
    totalPages,
    total,
    pageSize,
    items: items.slice(start, start + pageSize),
    from: total === 0 ? 0 : start + 1,
    to: Math.min(start + pageSize, total),
  };
}

function normQ(s) {
  return String(s ?? "").trim().toLowerCase();
}

function includesQ(haystack, q) {
  if (!q) return true;
  return String(haystack ?? "").toLowerCase().includes(q);
}

function filterLogs(items) {
  const q = normQ(listFilter.logs.q);
  const level = listFilter.logs.level;
  return items.filter((r) => {
    if (level && r.level !== level) return false;
    if (!q) return true;
    return includesQ(r.message, q) || includesQ(r.level, q) || includesQ(r.created_at, q);
  });
}

function filterFiles(items) {
  const q = normQ(listFilter.files.q);
  const connId = listFilter.files.connectionId;
  return items.filter((f) => {
    if (connId && (f.connection_id || "") !== connId) return false;
    if (!q) return true;
    return (
      includesQ(f.file_name, q) ||
      includesQ(f.file_path, q) ||
      includesQ(f.connection_name, q) ||
      includesQ(f.connection_id, q)
    );
  });
}

function filterConnections(items) {
  const q = normQ(listFilter.connections.q);
  const driver = listFilter.connections.driver;
  const status = listFilter.connections.status;
  return items.filter((c) => {
    if (driver && (c.driver || "sql_api") !== driver) return false;
    if (status === "on" && !c.enabled) return false;
    if (status === "off" && c.enabled) return false;
    if (!q) return true;
    return (
      includesQ(c.name, q) ||
      includesQ(c.url, q) ||
      includesQ(c.table_name, q) ||
      includesQ(c.watch_dir, q) ||
      includesQ(c.driver, q) ||
      includesQ(c.last_error, q)
    );
  });
}

function filterAuth(items) {
  const q = normQ(listFilter.auth.q);
  const status = listFilter.auth.status;
  return items.filter((tok) => {
    if (status === "on" && !tok.enabled) return false;
    if (status === "off" && tok.enabled) return false;
    if (!q) return true;
    return includesQ(tok.name, q) || includesQ(tok.token_prefix, q);
  });
}

/** Wire search/filter controls once; re-render current list without refetch */
function initListFilters() {
  const bind = (searchId, filterKeys, onChange) => {
    const searchEl = $(searchId);
    if (searchEl) {
      let timer = null;
      const fire = () => {
        clearTimeout(timer);
        timer = setTimeout(onChange, 120);
      };
      searchEl.addEventListener("input", fire);
      searchEl.addEventListener("search", () => {
        clearTimeout(timer);
        onChange();
      });
    }
    filterKeys.forEach(({ id, apply }) => {
      const el = $(id);
      if (el) el.addEventListener("change", () => { apply(el); onChange(); });
    });
  };

  bind("#logs-search", [
    { id: "#logs-filter-level", apply: (el) => { listFilter.logs.level = el.value; } },
  ], () => {
    listFilter.logs.q = $("#logs-search")?.value || "";
    listPage.logs = 1;
    renderLogsPage();
  });

  bind("#files-search", [
    { id: "#files-filter-conn", apply: (el) => { listFilter.files.connectionId = el.value; } },
  ], () => {
    listFilter.files.q = $("#files-search")?.value || "";
    listPage.files = 1;
    renderFilesPage();
  });

  bind("#conn-search", [
    { id: "#conn-filter-driver", apply: (el) => { listFilter.connections.driver = el.value; } },
    { id: "#conn-filter-status", apply: (el) => { listFilter.connections.status = el.value; } },
  ], () => {
    listFilter.connections.q = $("#conn-search")?.value || "";
    listPage.connections = 1;
    renderConnectionsPage();
  });

  bind("#auth-search", [
    { id: "#auth-filter-status", apply: (el) => { listFilter.auth.status = el.value; } },
  ], () => {
    listFilter.auth.q = $("#auth-search")?.value || "";
    listPage.auth = 1;
    renderAuthTokensPage();
  });

  const clear = (ids, reset, pageKey, render) => {
    const btn = $(ids.btn);
    if (!btn) return;
    btn.onclick = () => {
      reset();
      if (ids.search) {
        const s = $(ids.search);
        if (s) s.value = "";
      }
      (ids.selects || []).forEach((sid) => {
        const el = $(sid);
        if (el) el.value = "";
      });
      listPage[pageKey] = 1;
      render();
    };
  };

  clear(
    { btn: "#logs-clear", search: "#logs-search", selects: ["#logs-filter-level"] },
    () => { listFilter.logs = { q: "", level: "" }; },
    "logs",
    renderLogsPage
  );
  clear(
    { btn: "#files-clear", search: "#files-search", selects: ["#files-filter-conn"] },
    () => { listFilter.files = { q: "", connectionId: "" }; },
    "files",
    renderFilesPage
  );
  clear(
    { btn: "#conn-clear", search: "#conn-search", selects: ["#conn-filter-driver", "#conn-filter-status"] },
    () => { listFilter.connections = { q: "", driver: "", status: "" }; },
    "connections",
    renderConnectionsPage
  );
  clear(
    { btn: "#auth-clear", search: "#auth-search", selects: ["#auth-filter-status"] },
    () => { listFilter.auth = { q: "", status: "" }; },
    "auth",
    renderAuthTokensPage
  );
}

/** Populate files connection filter from current file list (unique ids) */
function syncFilesConnFilterOptions() {
  const sel = $("#files-filter-conn");
  if (!sel) return;
  const prev = listFilter.files.connectionId || sel.value || "";
  const map = new Map();
  (listCache.files || []).forEach((f) => {
    const id = f.connection_id || "";
    if (!id || map.has(id)) return;
    map.set(id, f.connection_name || id);
  });
  const opts = [`<option value="">${esc(t("filter_all_connections"))}</option>`];
  [...map.entries()]
    .sort((a, b) => String(a[1]).localeCompare(String(b[1])))
    .forEach(([id, name]) => {
      opts.push(`<option value="${escAttr(id)}">${esc(name)}</option>`);
    });
  sel.innerHTML = opts.join("");
  // Keep selection if still present
  if (prev && map.has(prev)) {
    sel.value = prev;
    listFilter.files.connectionId = prev;
  } else {
    sel.value = "";
    listFilter.files.connectionId = "";
  }
}

function renderPager(el, meta, onPage) {
  if (!el) return;
  const { page, totalPages, total, from, to } = meta;
  if (total === 0) {
    el.innerHTML = "";
    el.classList.add("hidden");
    return;
  }
  el.classList.remove("hidden");
  el.innerHTML = `
    <div class="pager-info">${esc(t("pager_info", { from, to, total }))}</div>
    <div class="pager-btns">
      <button type="button" class="btn sm" data-pager="first" title="${escAttr(t("pager_first"))}" ${page <= 1 ? "disabled" : ""}>«</button>
      <button type="button" class="btn sm" data-pager="prev" title="${escAttr(t("pager_prev"))}" ${page <= 1 ? "disabled" : ""}>‹</button>
      <span class="pager-page">${esc(t("pager_page", { page, totalPages }))}</span>
      <button type="button" class="btn sm" data-pager="next" title="${escAttr(t("pager_next"))}" ${page >= totalPages ? "disabled" : ""}>›</button>
      <button type="button" class="btn sm" data-pager="last" title="${escAttr(t("pager_last"))}" ${page >= totalPages ? "disabled" : ""}>»</button>
    </div>`;
  const go = (n) => {
    if (n < 1 || n > totalPages || n === page) return;
    onPage(n);
  };
  el.querySelector('[data-pager="first"]').onclick = () => go(1);
  el.querySelector('[data-pager="prev"]').onclick = () => go(page - 1);
  el.querySelector('[data-pager="next"]').onclick = () => go(page + 1);
  el.querySelector('[data-pager="last"]').onclick = () => go(totalPages);
}

// ── Language toggle ───────────────────────────────────────────
function initLangToggles() {
  applyI18n();
  $$(".lang-switch").forEach((input) => {
    input.checked = getLang() === "vi";
    input.onchange = () => {
      setLang(input.checked ? "vi" : "en");
      // keep all toggles in sync
      $$(".lang-switch").forEach((el) => {
        el.checked = input.checked;
      });
      applyI18n();
      // refresh dynamic lists if logged in
      if (state.sessionId) {
        const active = $(".nav.active");
        if (active) active.click();
      }
    };
  });
}

async function api(path, opts = {}) {
  const headers = { "Content-Type": "application/json", ...(opts.headers || {}) };
  if (state.sessionId) headers["Authorization"] = `Bearer ${state.sessionId}`;
  const res = await fetch(path, { ...opts, headers, credentials: "same-origin" });
  const text = await res.text();
  let data = null;
  try { data = text ? JSON.parse(text) : null; } catch { data = { raw: text }; }
  if (!res.ok) {
    const err = new Error((data && data.error) || res.statusText || "request failed");
    err.status = res.status;
    err.data = data;
    throw err;
  }
  return data;
}

function toast(msg, isErr = false) {
  const el = $("#toast");
  el.textContent = msg;
  el.classList.toggle("err", isErr);
  el.classList.remove("hidden");
  clearTimeout(el._t);
  el._t = setTimeout(() => el.classList.add("hidden"), 3200);
}

async function loadBootstrap() {
  try {
    const b = await fetch("/api/bootstrap", { credentials: "same-origin" }).then(async (res) => {
      const text = await res.text();
      let data = null;
      try { data = text ? JSON.parse(text) : null; } catch { data = null; }
      if (!res.ok) throw new Error((data && data.error) || res.statusText || "bootstrap failed");
      return data;
    });
    state.hasAuthTokens = !!b.has_auth_tokens;
  } catch {
    // Fail closed for import UX (require token if we cannot tell)
    state.hasAuthTokens = true;
  }
  const hint = $("#login-import-hint");
  if (hint) {
    hint.classList.toggle("first-boot", !state.hasAuthTokens);
  }
}

function showLogin() {
  closeSidebar();
  $("#login-view").classList.remove("hidden");
  $("#main-view").classList.add("hidden");
  loadBootstrap();
}

function showMain() {
  $("#login-view").classList.add("hidden");
  $("#main-view").classList.remove("hidden");
}

// ── Mobile off-canvas sidebar ─────────────────────────────────
function isMobileNav() {
  return window.matchMedia("(max-width: 800px)").matches;
}

function openSidebar() {
  if (!isMobileNav()) return;
  document.body.classList.add("sidebar-open");
  const btn = $("#btn-mobile-menu");
  const backdrop = $("#sidebar-backdrop");
  if (btn) btn.setAttribute("aria-expanded", "true");
  if (backdrop) backdrop.hidden = false;
}

function closeSidebar() {
  document.body.classList.remove("sidebar-open");
  const btn = $("#btn-mobile-menu");
  const backdrop = $("#sidebar-backdrop");
  if (btn) btn.setAttribute("aria-expanded", "false");
  if (backdrop) backdrop.hidden = true;
}

function toggleSidebar() {
  if (document.body.classList.contains("sidebar-open")) closeSidebar();
  else openSidebar();
}

function initSidebarDrawer() {
  const menuBtn = $("#btn-mobile-menu");
  const logoBtn = $("#btn-sidebar-logo");
  const backdrop = $("#sidebar-backdrop");

  if (menuBtn) {
    menuBtn.onclick = (e) => {
      e.preventDefault();
      toggleSidebar();
    };
  }
  // Logo inside drawer closes on mobile (desktop: no-op)
  if (logoBtn) {
    logoBtn.onclick = (e) => {
      e.preventDefault();
      if (isMobileNav()) closeSidebar();
    };
  }
  if (backdrop) {
    backdrop.onclick = () => closeSidebar();
  }
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") closeSidebar();
  });
  window.addEventListener("resize", () => {
    if (!isMobileNav()) closeSidebar();
  });
}

// ── Auth ──────────────────────────────────────────────────────
async function trySession() {
  if (!state.sessionId) { showLogin(); return false; }
  try {
    const me = await api("/api/me");
    state.user = me;
    const uname = me.name || "user";
    $("#user-name").textContent = uname;
    const unameMobile = $("#user-name-mobile");
    if (unameMobile) unameMobile.textContent = uname;
    showMain();
    await refreshAll();
    return true;
  } catch {
    state.sessionId = "";
    localStorage.removeItem("sa_session");
    showLogin();
    return false;
  }
}

$("#btn-login").onclick = async () => {
  const token = $("#login-token").value.trim();
  const errEl = $("#login-error");
  if (errEl) {
    errEl.textContent = "";
    errEl.classList.add("error");
    errEl.classList.remove("ok-msg");
  }
  if (!token) {
    if (errEl) errEl.textContent = t("enter_token");
    return;
  }
  try {
    const data = await api("/api/login", { method: "POST", body: JSON.stringify({ token }) });
    state.sessionId = data.session_id;
    localStorage.setItem("sa_session", data.session_id);
    await trySession();
  } catch (e) {
    if (errEl) errEl.textContent = e.message;
  }
};

$("#login-token").addEventListener("keydown", (e) => {
  if (e.key === "Enter") $("#btn-login").click();
});

$("#btn-logout").onclick = async () => {
  try { await api("/api/logout", { method: "POST" }); } catch {}
  state.sessionId = "";
  localStorage.removeItem("sa_session");
  showLogin();
};

// ── Tabs ──────────────────────────────────────────────────────
$$(".nav").forEach((btn) => {
  btn.onclick = () => {
    $$(".nav").forEach((b) => b.classList.remove("active"));
    btn.classList.add("active");
    $$(".tab").forEach((tEl) => tEl.classList.add("hidden"));
    $(`#tab-${btn.dataset.tab}`).classList.remove("hidden");
    closeSidebar();
    if (btn.dataset.tab === "dashboard") loadDashboard();
    if (btn.dataset.tab === "files") loadFiles();
    if (btn.dataset.tab === "connections") loadConnections();
    if (btn.dataset.tab === "auth-tokens") loadAuthTokens();
    if (btn.dataset.tab === "settings") loadSettings();
  };
});

async function refreshAll() {
  await loadDashboard();
}

// ── Dashboard ─────────────────────────────────────────────────
async function loadDashboard() {
  try {
    const s = await api("/api/status");
    const watchDirs = (s.watch_dirs || []).join(" · ") || "—";
    const lastMsg = s.last_sync_message || "";
    $("#stats").innerHTML = `
      <div class="stat">
        <div class="label" title="${escAttr(t("stat_engine"))}">${esc(t("stat_engine"))}</div>
        <div class="value"><span class="dot ${s.running ? "on" : "off"}"></span>${esc(s.running ? t("stat_running") : t("stat_idle"))}</div>
      </div>
      <div class="stat">
        <div class="label" title="${escAttr(t("stat_local_files"))}">${esc(t("stat_local_files"))}</div>
        <div class="value">${s.local_file_count}</div>
        <div class="sub" title="${escAttr(watchDirs)}">${esc(watchDirs)}</div>
      </div>
      <div class="stat">
        <div class="label" title="${escAttr(t("stat_enabled_conns"))}">${esc(t("stat_enabled_conns"))}</div>
        <div class="value">${s.connections_enabled}</div>
      </div>
      <div class="stat">
        <div class="label" title="${escAttr(t("stat_last_sync"))}">${esc(t("stat_last_sync"))}</div>
        <div class="value" style="font-size:1rem">${esc(s.last_sync_at || "—")}</div>
        <div class="sub" title="${escAttr(lastMsg)}">${esc(lastMsg)}</div>
      </div>`;
    const log = await api("/api/sync/log");
    listCache.logs = log.logs || [];
    renderLogsPage();
  } catch (e) { toast(e.message, true); }
}

function renderLogsPage() {
  const filtered = filterLogs(listCache.logs);
  const meta = paginate(filtered, listPage.logs);
  listPage.logs = meta.page;
  const emptyMsg = listCache.logs.length && !filtered.length ? t("no_filter_results") : t("no_logs");
  $("#sync-log-body").innerHTML = meta.items.map((r) => `
    <tr>
      <td class="muted">${esc(r.created_at)}</td>
      <td><span class="badge ${r.level === "error" ? "err" : "on"}">${esc(r.level)}</span></td>
      <td>${esc(r.message)}</td>
    </tr>`).join("") || `<tr><td colspan="3" class="muted">${esc(emptyMsg)}</td></tr>`;
  renderPager($("#logs-pager"), meta, (n) => {
    listPage.logs = n;
    renderLogsPage();
  });
}

$("#btn-sync").onclick = async () => {
  try {
    await api("/api/sync", { method: "POST" });
    toast(t("sync_done"));
    await loadDashboard();
  } catch (e) { toast(e.message, true); }
};

// ── Files (raw content, per-connection) ───────────────────────
async function loadFiles() {
  try {
    const data = await api("/api/files");
    listCache.files = data.files || [];
    syncFilesConnFilterOptions();
    renderFilesPage();
  } catch (e) { toast(e.message, true); }
}

function renderFilesPage() {
  const filtered = filterFiles(listCache.files);
  const meta = paginate(filtered, listPage.files);
  listPage.files = meta.page;
  const emptyMsg = listCache.files.length && !filtered.length ? t("no_filter_results") : t("no_files");
  $("#files-body").innerHTML = meta.items.map((f) => `
    <tr>
      <td><strong>${esc(f.file_name)}</strong></td>
      <td class="muted" title="${escAttr(f.connection_id || "")}">${esc(f.connection_name || (f.connection_id ? t("conn_missing") : "—"))}</td>
      <td class="muted" style="max-width:240px;overflow:hidden;text-overflow:ellipsis" title="${escAttr(f.file_path || "")}">
        <code>${esc(f.file_path || "—")}</code>
      </td>
      <td class="muted">${esc(String(f.size ?? 0))}</td>
      <td class="muted">${esc(f.updated_at || "—")}</td>
      <td>
        <div class="btn-row">
          <button class="btn sm" data-edit-file="${esc(f.file_name)}" data-conn="${esc(f.connection_id || "")}">${esc(t("edit"))}</button>
          <button class="btn sm danger" data-del-file="${esc(f.file_name)}" data-conn="${esc(f.connection_id || "")}">${esc(t("delete"))}</button>
        </div>
      </td>
    </tr>`).join("") || `<tr><td colspan="6" class="muted">${esc(emptyMsg)}</td></tr>`;

  $$("#files-body [data-edit-file]").forEach((b) => {
    b.onclick = () => openFileModal(b.dataset.conn, b.dataset.editFile);
  });
  $$("#files-body [data-del-file]").forEach((b) => {
    b.onclick = async () => {
      if (!confirm(t("confirm_delete_file", { name: b.dataset.delFile }))) return;
      try {
        await api(`/api/files/${encodeURIComponent(b.dataset.conn)}/${encodeURIComponent(b.dataset.delFile)}`, { method: "DELETE" });
        toast(t("deleted"));
        loadFiles();
      } catch (e) { toast(e.message, true); }
    };
  });
  renderPager($("#files-pager"), meta, (n) => {
    listPage.files = n;
    renderFilesPage();
  });
}

$("#btn-file-new").onclick = () => openFileModal(null, null);

async function openFileModal(connectionId, fileName) {
  state.modalMode = "file";
  let conns = [];
  try { conns = await api("/api/connections"); } catch (e) { toast(e.message, true); return; }
  if (!conns.length) {
    toast(t("need_connection"), true);
    return;
  }

  $("#modal-title").textContent = fileName
    ? t("modal_file_edit", { name: fileName })
    : t("modal_file_new");
  let content = "";
  let name = fileName || "";
  let connId = connectionId || conns[0].id;
  if (fileName && connectionId) {
    try {
      const f = await api(`/api/files/${encodeURIComponent(connectionId)}/${encodeURIComponent(fileName)}`);
      content = f.content || "";
      name = f.file_name || fileName;
      connId = f.connection_id || connectionId;
    } catch (e) { toast(e.message, true); return; }
  }
  const connOpts = conns.map((c) =>
    `<option value="${escAttr(c.id)}" ${c.id === connId ? "selected" : ""}>${esc(c.name)} — ${esc(c.watch_dir)}</option>`
  ).join("");
  $("#modal-body").innerHTML = `
    <label>${esc(t("label_connection"))}</label>
    <select id="m-file-conn" ${fileName ? "disabled" : ""}>${connOpts}</select>
    <label>${esc(t("label_file_name"))}</label>
    <input id="m-file-name" value="${escAttr(name)}" ${fileName ? "readonly" : ""} placeholder="token.json" />
    <label>${esc(t("label_raw_content"))}</label>
    <textarea id="m-file-content" rows="24">${esc(content)}</textarea>`;
  openModal(async () => {
    const body = {
      connection_id: $("#m-file-conn").value,
      file_name: $("#m-file-name").value.trim(),
      content: $("#m-file-content").value,
    };
    if (!body.file_name) throw new Error(t("file_name_required"));
    if (!body.connection_id) throw new Error(t("connection_required"));
    if (fileName && connectionId) {
      await api(`/api/files/${encodeURIComponent(connectionId)}/${encodeURIComponent(fileName)}`, {
        method: "PUT", body: JSON.stringify(body),
      });
    } else {
      await api("/api/files", { method: "POST", body: JSON.stringify(body) });
    }
    toast(t("saved_file"));
    loadFiles();
  });
}

// ── Connections ───────────────────────────────────────────────
async function loadConnections() {
  try {
    listCache.connections = await api("/api/connections");
    renderConnectionsPage();
  } catch (e) { toast(e.message, true); }
}

function renderConnectionsPage() {
  const list = listCache.connections;
  const filtered = filterConnections(list);
  const meta = paginate(filtered, listPage.connections);
  listPage.connections = meta.page;
  const emptyMsg = list.length && !filtered.length ? t("no_filter_results") : t("no_conn");
  $("#conn-body").innerHTML = meta.items.map((c) => `
    <tr>
      <td><strong>${esc(c.name)}</strong></td>
      <td><code>${esc(c.driver || "sql_api")}</code></td>
      <td><code>${esc(c.table_name || "content_syncs")}</code></td>
      <td class="muted" style="max-width:160px;overflow:hidden;text-overflow:ellipsis" title="${escAttr(c.watch_dir || "")}"><code>${esc(c.watch_dir || "—")}</code></td>
      <td class="muted" style="max-width:160px;overflow:hidden;text-overflow:ellipsis">${esc(c.url)}</td>
      <td>
        <span class="badge ${c.enabled ? "on" : "off"}">${esc(c.enabled ? t("connected") : t("off"))}</span>
        ${c.last_error ? `<div class="muted small" title="${escAttr(c.last_error)}">⚠ error</div>` : ""}
      </td>
      <td class="muted">${esc(c.last_sync_at || "—")}</td>
      <td>
        <div class="btn-row">
          <button class="btn sm" data-toggle-conn="${c.id}">${esc(c.enabled ? t("off") : t("on"))}</button>
          <button class="btn sm" data-test-conn="${c.id}">${esc(t("test_migrate"))}</button>
          <button class="btn sm" data-clone-conn="${c.id}">${esc(t("clone"))}</button>
          <button class="btn sm" data-edit-conn="${c.id}">${esc(t("edit"))}</button>
          <button class="btn sm danger" data-del-conn="${c.id}">${esc(t("delete"))}</button>
        </div>
      </td>
    </tr>`).join("") || `<tr><td colspan="8" class="muted">${esc(emptyMsg)}</td></tr>`;

  function toastPipelineConflicts(r) {
    const names = r && Array.isArray(r.disabled_conflicts) ? r.disabled_conflicts : [];
    if (!names.length) return;
    toast(t("conn_conflicts_off").replace("{names}", names.join(", ")));
  }

  $$("#conn-body [data-toggle-conn]").forEach((b) => {
    b.onclick = async () => {
      try {
        const r = await api(`/api/connections/${b.dataset.toggleConn}/toggle`, { method: "POST" });
        loadConnections();
        toastPipelineConflicts(r);
      } catch (e) { toast(e.message, true); }
    };
  });
  $$("#conn-body [data-test-conn]").forEach((b) => {
    b.onclick = async () => {
      try {
        const r = await api(`/api/connections/${b.dataset.testConn}/test`, { method: "POST" });
        toast(r.message || "OK");
        loadConnections();
      } catch (e) { toast(e.message, true); }
    };
  });
  $$("#conn-body [data-clone-conn]").forEach((b) => {
    b.onclick = async () => {
      try {
        const r = await api(`/api/connections/${b.dataset.cloneConn}/clone`, { method: "POST" });
        toast(t("conn_cloned").replace("{name}", r.name || "copy"));
        loadConnections();
      } catch (e) { toast(e.message, true); }
    };
  });
  $$("#conn-body [data-edit-conn]").forEach((b) => {
    b.onclick = () => openConnModal(b.dataset.editConn, list.find((x) => x.id === b.dataset.editConn));
  });
  $$("#conn-body [data-del-conn]").forEach((b) => {
    b.onclick = async () => {
      if (!confirm(t("confirm_delete_conn"))) return;
      try {
        await api(`/api/connections/${b.dataset.delConn}`, { method: "DELETE" });
        toast(t("deleted"));
        loadConnections();
      } catch (e) { toast(e.message, true); }
    };
  });
  renderPager($("#conn-pager"), meta, (n) => {
    listPage.connections = n;
    renderConnectionsPage();
  });
}

$("#btn-conn-new").onclick = () => openConnModal(null, null);

function openConnModal(id, existing) {
  state.modalMode = "conn";
  state.modalId = id;
  $("#modal-title").textContent = id ? t("modal_conn_edit") : t("modal_conn_new");
  const drv = existing?.driver || "sql_api";
  $("#modal-body").innerHTML = `
    <label>${esc(t("label_name"))}</label>
    <input id="m-name" value="${escAttr(existing?.name || "")}" placeholder="prod-db" />
    <label>${esc(t("label_driver"))}</label>
    <select id="m-driver">
      <option value="sql_api" ${drv === "sql_api" ? "selected" : ""}>${esc(t("driver_sql"))}</option>
      <option value="libsql" ${drv === "libsql" ? "selected" : ""}>${esc(t("driver_libsql"))}</option>
      <option value="sqlite" ${drv === "sqlite" ? "selected" : ""}>${esc(t("driver_sqlite"))}</option>
      <option value="postgres" ${drv === "postgres" ? "selected" : ""}>${esc(t("driver_postgres"))}</option>
      <option value="mysql" ${drv === "mysql" ? "selected" : ""}>${esc(t("driver_mysql"))}</option>
      <option value="mariadb" ${drv === "mariadb" ? "selected" : ""}>${esc(t("driver_mariadb"))}</option>
      <option value="mongodb" ${drv === "mongodb" ? "selected" : ""}>${esc(t("driver_mongodb"))}</option>
    </select>
    <label>${esc(t("label_table"))}</label>
    <input id="m-table" value="${escAttr(existing?.table_name || "")}" placeholder="content_syncs" />
    <label>${esc(t("label_watch_dir_conn"))}</label>
    <input id="m-watch" value="${escAttr(existing?.watch_dir || "")}" placeholder="~/.content-sync/files/prod" />
    <label>${esc(t("label_db_url"))}</label>
    <input id="m-url" value="${escAttr(existing?.url || "")}" placeholder="…/v2/pipeline · libsql:// · sqlite:path · postgresql:// · mysql:// · mongodb://" />
    <label>${esc(t("label_access_token"))}${id ? " " + esc(t("label_leave_blank")) : ""}</label>
    <input id="m-token" type="password" value="" placeholder="${id ? "••••" : "token / password (optional for sqlite)"}" />
    <label class="check-row" for="m-enabled">
      <input id="m-enabled" type="checkbox" ${!existing || existing.enabled ? "checked" : ""} />
      <span>${esc(t("label_enabled"))}</span>
    </label>
    <p class="muted small">${t("conn_sdk_hint")}</p>`;
  openModal(async () => {
    const tableName = $("#m-table").value.trim();
    const body = {
      name: $("#m-name").value.trim(),
      driver: $("#m-driver").value,
      watch_dir: $("#m-watch").value.trim() || undefined,
      url: $("#m-url").value.trim(),
      access_token: $("#m-token").value.trim(),
      enabled: $("#m-enabled").checked,
    };
    // Only send when set — create uses API default if omitted; edit keeps existing if cleared
    if (tableName) body.table_name = tableName;
    if (!body.name || !body.url) throw new Error(t("name_url_required"));
    const needsToken = body.driver === "sql_api" || body.driver === "libsql";
    let res;
    if (id) {
      if (!body.access_token) delete body.access_token;
      res = await api(`/api/connections/${id}`, { method: "PUT", body: JSON.stringify(body) });
    } else {
      if (needsToken && !body.access_token) throw new Error(t("token_required"));
      res = await api("/api/connections", { method: "POST", body: JSON.stringify(body) });
    }
    toast(t("saved_conn"));
    const names = res && Array.isArray(res.disabled_conflicts) ? res.disabled_conflicts : [];
    if (names.length) {
      toast(t("conn_conflicts_off").replace("{names}", names.join(", ")));
    }
    loadConnections();
  });
}

// ── Auth tokens ───────────────────────────────────────────────
async function loadAuthTokens() {
  try {
    listCache.auth = await api("/api/auth-tokens");
    renderAuthTokensPage();
  } catch (e) { toast(e.message, true); }
}

function renderAuthTokensPage() {
  const filtered = filterAuth(listCache.auth);
  const meta = paginate(filtered, listPage.auth);
  listPage.auth = meta.page;
  const emptyMsg = listCache.auth.length && !filtered.length ? t("no_filter_results") : t("no_auth");
  $("#auth-body").innerHTML = meta.items.map((tok) => `
    <tr>
      <td><strong>${esc(tok.name)}</strong></td>
      <td><code>${esc(tok.token_prefix)}…</code></td>
      <td><span class="badge ${tok.enabled ? "on" : "off"}">${esc(tok.enabled ? t("on") : t("off"))}</span></td>
      <td class="muted">${esc(tok.created_at)}</td>
      <td class="muted">${esc(tok.last_used_at || "—")}</td>
      <td>
        <div class="btn-row">
          <button class="btn sm" data-toggle-auth="${tok.id}" data-en="${tok.enabled}">${esc(tok.enabled ? t("disable") : t("enable"))}</button>
          <button class="btn sm danger" data-del-auth="${tok.id}">${esc(t("delete"))}</button>
        </div>
      </td>
    </tr>`).join("") || `<tr><td colspan="6" class="muted">${esc(emptyMsg)}</td></tr>`;

  $$("#auth-body [data-toggle-auth]").forEach((b) => {
    b.onclick = async () => {
      const enabled = b.dataset.en !== "true";
      try {
        await api(`/api/auth-tokens/${b.dataset.toggleAuth}`, {
          method: "PUT", body: JSON.stringify({ enabled }),
        });
        loadAuthTokens();
      } catch (e) { toast(e.message, true); }
    };
  });
  $$("#auth-body [data-del-auth]").forEach((b) => {
    b.onclick = async () => {
      if (!confirm(t("confirm_delete_auth"))) return;
      try {
        await api(`/api/auth-tokens/${b.dataset.delAuth}`, { method: "DELETE" });
        toast(t("deleted"));
        loadAuthTokens();
      } catch (e) { toast(e.message, true); }
    };
  });
  renderPager($("#auth-pager"), meta, (n) => {
    listPage.auth = n;
    renderAuthTokensPage();
  });
}

$("#btn-auth-new").onclick = () => {
  state.modalMode = "auth";
  $("#modal-title").textContent = t("modal_auth_new");
  $("#modal-body").innerHTML = `
    <label>${esc(t("label_desc_name"))}</label>
    <input id="m-auth-name" placeholder="admin / laptop / …" />`;
  openModal(async () => {
    const name = $("#m-auth-name").value.trim();
    if (!name) throw new Error(t("name_required"));
    const r = await api("/api/auth-tokens", { method: "POST", body: JSON.stringify({ name }) });
    const banner = $("#raw-token-banner");
    banner.classList.remove("hidden");
    banner.innerHTML = `<strong>${esc(t("raw_token_copy"))}</strong><br><code>${esc(r.raw_token)}</code><br><span class="muted">${esc(r.warning || "")}</span>`;
    toast(t("token_created"));
    loadAuthTokens();
  });
};

// ── Settings ──────────────────────────────────────────────────
async function loadSettings() {
  try {
    const s = await api("/api/settings");
    $("#set-auto-poll").checked = s.auto_poll !== false;
    $("#set-poll").value = s.poll_interval_secs || 30;
    $("#set-error-backoff").value = s.error_backoff_secs || 120;
    $("#set-error-backoff-max").value = s.error_backoff_max_secs || 900;
    $("#set-log-retention").value =
      s.log_retention_hours === 0 || s.log_retention_hours
        ? s.log_retention_hours
        : 48;
    $("#set-bind").value = s.web_bind || "";
  } catch (e) { toast(e.message, true); }
}

$("#btn-save-settings").onclick = async () => {
  try {
    const body = {
      auto_poll: $("#set-auto-poll").checked,
      poll_interval_secs: Number($("#set-poll").value) || 30,
      error_backoff_secs: Number($("#set-error-backoff").value) || 120,
      error_backoff_max_secs: Number($("#set-error-backoff-max").value) || 900,
      log_retention_hours: (() => {
        const v = $("#set-log-retention").value;
        if (v === "" || v === null || v === undefined) return 48;
        const n = Number(v);
        return Number.isFinite(n) ? Math.max(0, n) : 48;
      })(),
      web_bind: $("#set-bind").value.trim(),
    };
    await api("/api/settings", { method: "PUT", body: JSON.stringify(body) });
    $("#settings-msg").textContent = t("settings_saved_msg");
    toast(t("settings_saved"));
  } catch (e) { toast(e.message, true); }
};

/** Build browser URL for a saved web_bind (host/port). Wildcard binds keep current hostname. */
function urlFromWebBind(bind) {
  const raw = String(bind || "").trim();
  if (!raw) return window.location.origin + "/";
  let host = raw;
  let port = "";
  if (raw.startsWith("[")) {
    const m = raw.match(/^\[([^\]]+)\]:(\d+)$/);
    if (m) {
      host = m[1];
      port = m[2];
    }
  } else {
    const idx = raw.lastIndexOf(":");
    if (idx > 0) {
      host = raw.slice(0, idx);
      port = raw.slice(idx + 1);
    }
  }
  if (!port || !/^\d+$/.test(port)) {
    return window.location.origin + "/";
  }
  const wild =
    host === "0.0.0.0" ||
    host === "::" ||
    host === "[::]" ||
    host === "*" ||
    host === "";
  const h = wild ? window.location.hostname : host;
  const proto = window.location.protocol || "http:";
  const needBrackets = h.includes(":") && !h.startsWith("[");
  const hostPart = needBrackets ? `[${h}]` : h;
  return `${proto}//${hostPart}:${port}/`;
}

async function waitForServer(url, timeoutMs = 20000) {
  const base = url.replace(/\/$/, "");
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const res = await fetch(`${base}/api/bootstrap`, {
        credentials: "same-origin",
        cache: "no-store",
      });
      if (res.ok || res.status === 401 || res.status === 404) return true;
    } catch {
      // still down
    }
    await new Promise((r) => setTimeout(r, 400));
  }
  return false;
}

$("#btn-restart-app").onclick = async () => {
  if (!confirm(t("restart_confirm"))) return;
  const msg = $("#settings-msg");
  const btn = $("#btn-restart-app");
  if (btn) btn.disabled = true;
  try {
    const r = await api("/api/system/restart", { method: "POST" });
    const target = urlFromWebBind(r.web_bind || $("#set-bind")?.value);
    const waitMs = Number(r.reconnect_in_ms) || 2000;
    if (msg) msg.textContent = t("restart_started");
    toast(t("restart_started"));
    // Give the old process time to exit, then poll until the new one answers.
    await new Promise((r) => setTimeout(r, waitMs));
    const up = await waitForServer(target, 25000);
    if (up) {
      window.location.href = target;
    } else {
      // Still try navigate — user may refresh manually if bind changed
      window.location.href = target;
    }
  } catch (e) {
    if (msg) msg.textContent = e.message || t("restart_failed");
    toast(e.message || t("restart_failed"), true);
    if (btn) btn.disabled = false;
  }
};

/** Filename: export.content.sync.YYYY-MM-DD.HH-MM-SS.json (no spaces; specials → -) */
function configExportFilename(d = new Date()) {
  const pad = (n) => String(n).padStart(2, "0");
  const date = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  const time = `${pad(d.getHours())}-${pad(d.getMinutes())}-${pad(d.getSeconds())}`;
  return `export.content.sync.${date}.${time}.json`;
}

function downloadJsonBlob(obj, filename) {
  const blob = new Blob([JSON.stringify(obj, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}

async function parseConfigFile(file) {
  const text = await file.text();
  let data;
  try {
    data = JSON.parse(text);
  } catch {
    throw new Error(t("config_import_invalid"));
  }
  if (!data || typeof data !== "object" || !data.settings) {
    throw new Error(t("config_import_invalid"));
  }
  return data;
}

function formatImportOk(template, res, data) {
  return template
    .replace("{connections}", String(res.connections ?? data.connections?.length ?? 0))
    .replace("{tokens}", String(res.auth_tokens ?? data.auth_tokens?.length ?? 0));
}

/** POST /api/config/import. Optional raw access token for login-screen import. */
async function postConfigImport(data, accessToken) {
  const headers = { "Content-Type": "application/json" };
  if (accessToken) {
    headers["Authorization"] = `Bearer ${accessToken}`;
  } else if (state.sessionId) {
    headers["Authorization"] = `Bearer ${state.sessionId}`;
  }
  const res = await fetch("/api/config/import", {
    method: "POST",
    headers,
    credentials: "same-origin",
    body: JSON.stringify(data),
  });
  const text = await res.text();
  let body = null;
  try { body = text ? JSON.parse(text) : null; } catch { body = { raw: text }; }
  if (!res.ok) {
    const err = new Error((body && body.error) || res.statusText || "request failed");
    err.status = res.status;
    err.data = body;
    throw err;
  }
  return body;
}

$("#btn-export-config").onclick = async () => {
  const msg = $("#config-io-msg");
  try {
    const data = await api("/api/config/export");
    const name = configExportFilename();
    downloadJsonBlob(data, name);
    if (msg) msg.textContent = `${t("config_export_ok")}: ${name}`;
    toast(t("config_export_ok"));
  } catch (e) {
    if (msg) msg.textContent = e.message;
    toast(e.message, true);
  }
};

$("#btn-import-config").onclick = () => {
  const input = $("#import-config-file");
  if (input) {
    input.value = "";
    input.click();
  }
};

$("#import-config-file").onchange = async (ev) => {
  const file = ev.target.files && ev.target.files[0];
  if (!file) return;
  const msg = $("#config-io-msg");
  try {
    const data = await parseConfigFile(file);
    if (!confirm(t("config_import_confirm"))) return;
    const res = await postConfigImport(data);
    const okMsg = formatImportOk(t("config_import_ok"), res, data);
    if (msg) msg.textContent = okMsg;
    toast(okMsg);
    try {
      await loadSettings();
    } catch {
      // session may have been cleared — fall through to re-auth check
    }
    try {
      await api("/api/me");
    } catch {
      state.sessionId = "";
      localStorage.removeItem("sa_session");
      showLogin();
    }
  } catch (e) {
    if (msg) msg.textContent = e.message;
    toast(e.message, true);
  } finally {
    ev.target.value = "";
  }
};

// Login-screen import: confirm always; token required only when auth tokens already exist
$("#btn-login-import-config").onclick = () => {
  const input = $("#login-import-config-file");
  if (input) {
    input.value = "";
    input.click();
  }
};

$("#login-import-config-file").onchange = async (ev) => {
  const file = ev.target.files && ev.target.files[0];
  if (!file) return;
  const errEl = $("#login-error");
  if (errEl) errEl.textContent = "";
  try {
    // Refresh bootstrap so we know current token state
    await loadBootstrap();
    const data = await parseConfigFile(file);
    if (!confirm(t("config_import_confirm"))) return;

    let accessToken = "";
    if (state.hasAuthTokens) {
      accessToken = ($("#login-token")?.value || "").trim();
      if (!accessToken) {
        throw new Error(t("login_import_need_token"));
      }
    }

    const res = await postConfigImport(data, accessToken || undefined);
    const okMsg = formatImportOk(t("login_import_ok"), res, data);
    if (errEl) {
      errEl.textContent = okMsg;
      errEl.classList.remove("error");
      errEl.classList.add("ok-msg");
    }
    toast(okMsg);
    // Sessions were cleared by import — stay on login
    state.sessionId = "";
    localStorage.removeItem("sa_session");
    await loadBootstrap();
  } catch (e) {
    if (errEl) {
      errEl.textContent = e.message;
      errEl.classList.add("error");
      errEl.classList.remove("ok-msg");
    }
    toast(e.message, true);
  } finally {
    ev.target.value = "";
  }
};

// ── Modal helpers ─────────────────────────────────────────────
let modalSaveFn = null;

function openModal(onSave) {
  modalSaveFn = onSave;
  applyI18n($("#modal"));
  $("#modal").classList.remove("hidden");
}

function closeModal() {
  $("#modal").classList.add("hidden");
  modalSaveFn = null;
}

$("#modal-close").onclick = closeModal;
$("#modal-cancel").onclick = closeModal;
// Click backdrop (outside the card) to dismiss
$("#modal").addEventListener("click", (e) => {
  if (e.target === e.currentTarget) closeModal();
});
$("#modal-save").onclick = async () => {
  if (!modalSaveFn) return;
  try {
    await modalSaveFn();
    closeModal();
  } catch (e) { toast(e.message, true); }
};

function esc(s) {
  return String(s ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
function escAttr(s) { return esc(s).replace(/'/g, "&#39;"); }

// boot
initLangToggles();
initListFilters();
initSidebarDrawer();
trySession();
setInterval(() => {
  if (state.sessionId && !$("#tab-dashboard").classList.contains("hidden")) {
    loadDashboard();
  }
}, 15000);
