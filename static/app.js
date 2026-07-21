/* Content Sync Web UI */
const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];

const state = {
  sessionId: localStorage.getItem("sa_session") || "",
  user: null,
  modalMode: null,
  modalId: null,
};

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

function showLogin() {
  $("#login-view").classList.remove("hidden");
  $("#main-view").classList.add("hidden");
}

function showMain() {
  $("#login-view").classList.add("hidden");
  $("#main-view").classList.remove("hidden");
}

// ── Auth ──────────────────────────────────────────────────────
async function trySession() {
  if (!state.sessionId) { showLogin(); return false; }
  try {
    const me = await api("/api/me");
    state.user = me;
    $("#user-name").textContent = me.name || "user";
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
  $("#login-error").textContent = "";
  if (!token) { $("#login-error").textContent = t("enter_token"); return; }
  try {
    const data = await api("/api/login", { method: "POST", body: JSON.stringify({ token }) });
    state.sessionId = data.session_id;
    localStorage.setItem("sa_session", data.session_id);
    await trySession();
  } catch (e) {
    $("#login-error").textContent = e.message;
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
    $("#stats").innerHTML = `
      <div class="stat">
        <div class="label">${esc(t("stat_engine"))}</div>
        <div class="value"><span class="dot ${s.running ? "on" : "off"}"></span>${esc(s.running ? t("stat_running") : t("stat_idle"))}</div>
      </div>
      <div class="stat">
        <div class="label">${esc(t("stat_local_files"))}</div>
        <div class="value">${s.local_file_count}</div>
        <div class="sub">${esc((s.watch_dirs || []).join(" · ") || "—")}</div>
      </div>
      <div class="stat">
        <div class="label">${esc(t("stat_enabled_conns"))}</div>
        <div class="value">${s.connections_enabled}</div>
      </div>
      <div class="stat">
        <div class="label">${esc(t("stat_last_sync"))}</div>
        <div class="value" style="font-size:1rem">${esc(s.last_sync_at || "—")}</div>
        <div class="sub">${esc(s.last_sync_message || "")}</div>
      </div>`;
    const log = await api("/api/sync/log");
    $("#sync-log-body").innerHTML = (log.logs || []).map((r) => `
      <tr>
        <td class="muted">${esc(r.created_at)}</td>
        <td><span class="badge ${r.level === "error" ? "err" : "on"}">${esc(r.level)}</span></td>
        <td>${esc(r.message)}</td>
      </tr>`).join("") || `<tr><td colspan="3" class="muted">${esc(t("no_logs"))}</td></tr>`;
  } catch (e) { toast(e.message, true); }
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
    const files = data.files || [];
    $("#files-body").innerHTML = files.map((f) => `
      <tr>
        <td><strong>${esc(f.file_name)}</strong></td>
        <td class="muted">${esc(f.connection_name || f.connection_id || "—")}</td>
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
      </tr>`).join("") || `<tr><td colspan="6" class="muted">${esc(t("no_files"))}</td></tr>`;

    $$("[data-edit-file]").forEach((b) => {
      b.onclick = () => openFileModal(b.dataset.conn, b.dataset.editFile);
    });
    $$("[data-del-file]").forEach((b) => {
      b.onclick = async () => {
        if (!confirm(t("confirm_delete_file", { name: b.dataset.delFile }))) return;
        try {
          await api(`/api/files/${encodeURIComponent(b.dataset.conn)}/${encodeURIComponent(b.dataset.delFile)}`, { method: "DELETE" });
          toast(t("deleted"));
          loadFiles();
        } catch (e) { toast(e.message, true); }
      };
    });
  } catch (e) { toast(e.message, true); }
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
    <textarea id="m-file-content" rows="16" style="font-family:var(--mono);font-size:12px;line-height:1.45">${esc(content)}</textarea>`;
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
    const list = await api("/api/connections");
    $("#conn-body").innerHTML = list.map((c) => `
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
            <button class="btn sm" data-edit-conn="${c.id}">${esc(t("edit"))}</button>
            <button class="btn sm danger" data-del-conn="${c.id}">${esc(t("delete"))}</button>
          </div>
        </td>
      </tr>`).join("") || `<tr><td colspan="8" class="muted">${esc(t("no_conn"))}</td></tr>`;

    $$("[data-toggle-conn]").forEach((b) => {
      b.onclick = async () => {
        try {
          await api(`/api/connections/${b.dataset.toggleConn}/toggle`, { method: "POST" });
          loadConnections();
        } catch (e) { toast(e.message, true); }
      };
    });
    $$("[data-test-conn]").forEach((b) => {
      b.onclick = async () => {
        try {
          const r = await api(`/api/connections/${b.dataset.testConn}/test`, { method: "POST" });
          toast(r.message || "OK");
          loadConnections();
        } catch (e) { toast(e.message, true); }
      };
    });
    $$("[data-edit-conn]").forEach((b) => {
      b.onclick = () => openConnModal(b.dataset.editConn, list.find((x) => x.id === b.dataset.editConn));
    });
    $$("[data-del-conn]").forEach((b) => {
      b.onclick = async () => {
        if (!confirm(t("confirm_delete_conn"))) return;
        try {
          await api(`/api/connections/${b.dataset.delConn}`, { method: "DELETE" });
          toast(t("deleted"));
          loadConnections();
        } catch (e) { toast(e.message, true); }
      };
    });
  } catch (e) { toast(e.message, true); }
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
    <input id="m-table" value="${escAttr(existing?.table_name || "content_syncs")}" placeholder="content_syncs" />
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
    const body = {
      name: $("#m-name").value.trim(),
      driver: $("#m-driver").value,
      table_name: $("#m-table").value.trim() || "content_syncs",
      watch_dir: $("#m-watch").value.trim() || undefined,
      url: $("#m-url").value.trim(),
      access_token: $("#m-token").value.trim(),
      enabled: $("#m-enabled").checked,
    };
    if (!body.name || !body.url) throw new Error(t("name_url_required"));
    const needsToken = body.driver === "sql_api" || body.driver === "libsql";
    if (id) {
      if (!body.access_token) delete body.access_token;
      await api(`/api/connections/${id}`, { method: "PUT", body: JSON.stringify(body) });
    } else {
      if (needsToken && !body.access_token) throw new Error(t("token_required"));
      await api("/api/connections", { method: "POST", body: JSON.stringify(body) });
    }
    toast(t("saved_conn"));
    loadConnections();
  });
}

// ── Auth tokens ───────────────────────────────────────────────
async function loadAuthTokens() {
  try {
    const list = await api("/api/auth-tokens");
    $("#auth-body").innerHTML = list.map((tok) => `
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
      </tr>`).join("") || `<tr><td colspan="6" class="muted">${esc(t("no_auth"))}</td></tr>`;

    $$("[data-toggle-auth]").forEach((b) => {
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
    $$("[data-del-auth]").forEach((b) => {
      b.onclick = async () => {
        if (!confirm(t("confirm_delete_auth"))) return;
        try {
          await api(`/api/auth-tokens/${b.dataset.delAuth}`, { method: "DELETE" });
          toast(t("deleted"));
          loadAuthTokens();
        } catch (e) { toast(e.message, true); }
      };
    });
  } catch (e) { toast(e.message, true); }
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
    $("#set-watch-dir").value = s.default_files_root || s.watch_dir || "";
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
    const root = $("#set-watch-dir").value.trim();
    const body = {
      watch_dir: root,
      default_files_root: root,
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
trySession();
setInterval(() => {
  if (state.sessionId && !$("#tab-dashboard").classList.contains("hidden")) {
    loadDashboard();
  }
}, 15000);
