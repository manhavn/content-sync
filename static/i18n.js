/* Content Sync i18n — default English */
const I18N = {
  en: {
    // brand / login
    app_name: "Content Sync",
    brand_tagline: "Raw file sync · Bunny / libSQL",
    access_token: "Access token",
    login: "Sign in",
    login_hint: "Create a token via CLI: <code>content-sync token create --name admin</code>",
    enter_token: "Enter token",

    // nav
    nav_dashboard: "Dashboard",
    nav_files: "Files",
    nav_connections: "Connections",
    nav_auth_tokens: "Auth Tokens",
    nav_settings: "Settings",
    logout: "Sign out",

    // dashboard
    dashboard: "Dashboard",
    sync_now: "Sync now",
    sync_log: "Sync log",
    th_time: "Time",
    th_level: "Level",
    th_message: "Message",
    stat_engine: "Sync engine",
    stat_running: "Running",
    stat_idle: "Idle",
    stat_local_files: "Local files",
    stat_enabled_conns: "Enabled connections",
    stat_last_sync: "Last sync",
    no_logs: "No logs yet",
    sync_done: "Sync complete",

    // files
    files_title: "Files",
    files_add: "+ Add file",
    files_help: "Each file belongs to one connection’s watch directory. Content is raw text (any format).",
    th_file: "File",
    th_connection: "Connection",
    th_path: "Path",
    th_size: "Size",
    th_updated: "Updated",
    no_files: "No files yet — pick a connection and add one",
    edit: "Edit",
    delete: "Delete",
    confirm_delete_file: "Delete file {name}?",
    deleted: "Deleted",
    saved_file: "File saved",
    modal_file_new: "Add file",
    modal_file_edit: "Edit file: {name}",
    label_file_name: "File name",
    label_raw_content: "Content (raw text)",
    label_connection: "Connection",
    file_name_required: "file_name required",
    connection_required: "connection required",
    need_connection: "Add a connection first",

    // connections
    conn_title: "Database Connections",
    conn_add: "+ Connection",
    conn_help: "Each connection = one DB + one table + one local watch directory. Enable only the pipelines you want to run.",
    th_name: "Name",
    th_driver: "Driver",
    th_table: "Table / coll.",
    th_watch_dir: "Watch dir",
    th_url: "URL",
    th_token: "Token",
    th_status: "Status",
    th_last_sync: "Last sync",
    no_conn: "No connections — add a driver + URL + watch dir",
    label_watch_dir_conn: "Local watch directory",
    on: "On",
    off: "Off",
    connected: "connected",
    test_migrate: "Test / migrate",
    confirm_delete_conn: "Delete this connection?",
    saved_conn: "Connection saved",
    modal_conn_new: "Add connection",
    modal_conn_edit: "Edit connection",
    label_name: "Name",
    label_driver: "Driver",
    label_table: "Table / collection name",
    label_db_url: "Database URL / DSN",
    label_access_token: "Access token / password",
    label_leave_blank: "(leave blank to keep)",
    label_enabled: "Enabled (connect & sync)",
    conn_sdk_hint: "sql_api/libsql need access token. SQLite: file path. Postgres/MySQL/MariaDB/Mongo: password optional if already in DSN. Test/migrate creates table/collection + indexes.",
    name_url_required: "name and url are required",
    token_required: "access_token is required for sql_api and libsql",
    driver_sql: "sql_api — HTTP /v2/pipeline (sql-api.md)",
    driver_libsql: "libsql — SDK remote (sdk-rust.md)",
    driver_sqlite: "sqlite — file path / sqlite:…",
    driver_postgres: "postgres — postgresql://…",
    driver_mysql: "mysql — mysql://…",
    driver_mariadb: "mariadb — mysql://…",
    driver_mongodb: "mongodb — mongodb://… / mongodb+srv://…",

    // auth tokens
    auth_title: "Auth Tokens (Web UI login)",
    auth_add: "+ Create token",
    auth_help: "Tokens used to sign in to the Web UI / API. The raw token is shown only once when created.",
    th_prefix: "Prefix",
    th_enabled: "Enabled",
    th_created: "Created",
    th_last_used: "Last used",
    no_auth: "No auth tokens yet",
    enable: "Enable",
    disable: "Disable",
    confirm_delete_auth: "Delete this auth token?",
    modal_auth_new: "Create auth token",
    label_desc_name: "Name (description)",
    name_required: "name required",
    raw_token_copy: "Raw token (copy now):",
    token_created: "Token created — copy the raw token",

    // settings
    settings_title: "Settings",
    label_watch_dir: "Watch directory (local files to sync)",
    label_poll: "Poll interval (seconds) — periodic pull when healthy",
    label_backoff: "Error backoff base (seconds) — wait longer after remote failures (rate limit)",
    label_backoff_max: "Error backoff max (seconds) — exponential backoff cap",
    label_log_retention: "Log retention (hours) — auto-delete older sync logs (default 48; 0 = disable age cleanup)",
    label_web_bind: "Web bind (restart CLI to apply)",
    save_settings: "Save settings",
    settings_saved_msg: "Saved. Watcher will reload; web_bind needs a CLI restart.",
    settings_saved: "Settings saved",

    // modal
    cancel: "Cancel",
    save: "Save",
  },
  vi: {
    app_name: "Content Sync",
    brand_tagline: "Đồng bộ file raw · Bunny / libSQL",
    access_token: "Access token",
    login: "Đăng nhập",
    login_hint: "Tạo token bằng CLI: <code>content-sync token create --name admin</code>",
    enter_token: "Nhập token",

    nav_dashboard: "Dashboard",
    nav_files: "Files",
    nav_connections: "Connections",
    nav_auth_tokens: "Auth Tokens",
    nav_settings: "Settings",
    logout: "Đăng xuất",

    dashboard: "Dashboard",
    sync_now: "Đồng bộ ngay",
    sync_log: "Nhật ký sync",
    th_time: "Thời gian",
    th_level: "Mức",
    th_message: "Nội dung",
    stat_engine: "Sync engine",
    stat_running: "Đang chạy",
    stat_idle: "Chờ",
    stat_local_files: "File local",
    stat_enabled_conns: "Connection đang bật",
    stat_last_sync: "Sync gần nhất",
    no_logs: "Chưa có log",
    sync_done: "Sync hoàn tất",

    files_title: "Files",
    files_add: "+ Thêm file",
    files_help: "Mỗi file thuộc một connection (thư mục watch riêng). Nội dung raw text.",
    th_file: "File",
    th_connection: "Connection",
    th_path: "Đường dẫn",
    th_size: "Kích thước",
    th_updated: "Cập nhật",
    no_files: "Chưa có file — chọn connection rồi thêm mới",
    edit: "Sửa",
    delete: "Xoá",
    confirm_delete_file: "Xoá file {name}?",
    deleted: "Đã xoá",
    saved_file: "Đã lưu file",
    modal_file_new: "Thêm file",
    modal_file_edit: "Sửa file: {name}",
    label_file_name: "Tên file",
    label_raw_content: "Nội dung (raw text)",
    label_connection: "Connection",
    file_name_required: "file_name bắt buộc",
    connection_required: "cần chọn connection",
    need_connection: "Hãy thêm connection trước",

    conn_title: "Database Connections",
    conn_add: "+ Connection",
    conn_help: "Mỗi connection = một DB + một bảng + một thư mục local. Bật những pipeline cần sync.",
    th_name: "Tên",
    th_driver: "Driver",
    th_table: "Bảng / coll.",
    th_watch_dir: "Watch dir",
    th_url: "URL",
    th_token: "Token",
    th_status: "Trạng thái",
    th_last_sync: "Sync gần nhất",
    no_conn: "Chưa có connection — thêm driver + URL + watch dir",
    label_watch_dir_conn: "Thư mục local (watch)",
    on: "Bật",
    off: "Tắt",
    connected: "connected",
    test_migrate: "Test / migrate",
    confirm_delete_conn: "Xoá connection này?",
    saved_conn: "Đã lưu connection",
    modal_conn_new: "Thêm connection",
    modal_conn_edit: "Sửa connection",
    label_name: "Tên",
    label_driver: "Driver",
    label_table: "Tên bảng / collection",
    label_db_url: "Database URL / DSN",
    label_access_token: "Access token / password",
    label_leave_blank: "(để trống nếu không đổi)",
    label_enabled: "Enabled (connect & sync)",
    conn_sdk_hint: "sql_api/libsql cần access token. SQLite: đường dẫn file. Postgres/MySQL/MariaDB/Mongo: password tùy chọn nếu đã có trong DSN. Test/migrate tạo table/collection + index.",
    name_url_required: "name và url bắt buộc",
    token_required: "access_token bắt buộc với sql_api và libsql",
    driver_sql: "sql_api — HTTP /v2/pipeline (sql-api.md)",
    driver_libsql: "libsql — SDK remote (sdk-rust.md)",
    driver_sqlite: "sqlite — đường dẫn file / sqlite:…",
    driver_postgres: "postgres — postgresql://…",
    driver_mysql: "mysql — mysql://…",
    driver_mariadb: "mariadb — mysql://…",
    driver_mongodb: "mongodb — mongodb://… / mongodb+srv://…",

    auth_title: "Auth Tokens (đăng nhập Web UI)",
    auth_add: "+ Tạo token",
    auth_help: "Token dùng để đăng nhập Web UI / API. Raw token chỉ hiện một lần khi tạo.",
    th_prefix: "Prefix",
    th_enabled: "Bật",
    th_created: "Tạo lúc",
    th_last_used: "Dùng gần nhất",
    no_auth: "Chưa có auth token",
    enable: "Bật",
    disable: "Tắt",
    confirm_delete_auth: "Xoá auth token này?",
    modal_auth_new: "Tạo auth token",
    label_desc_name: "Tên (mô tả)",
    name_required: "name bắt buộc",
    raw_token_copy: "Raw token (copy ngay):",
    token_created: "Token đã tạo — hãy copy raw token",

    settings_title: "Settings",
    label_watch_dir: "Watch directory (file local để sync)",
    label_poll: "Poll interval (giây) — pull định kỳ khi healthy",
    label_backoff: "Error backoff base (giây) — chờ lâu hơn khi remote fail (tránh rate limit)",
    label_backoff_max: "Error backoff max (giây) — trần exponential backoff",
    label_log_retention: "Log retention (giờ) — tự xoá sync log cũ hơn (mặc định 48; 0 = tắt age cleanup)",
    label_web_bind: "Web bind (cần restart CLI để áp dụng)",
    save_settings: "Lưu settings",
    settings_saved_msg: "Đã lưu. Watcher sẽ reload; web_bind cần restart CLI.",
    settings_saved: "Đã lưu settings",

    cancel: "Huỷ",
    save: "Lưu",
  },
};

const LANG_KEY = "sa_lang";

function getLang() {
  const v = localStorage.getItem(LANG_KEY);
  return v === "vi" || v === "en" ? v : "en";
}

function setLang(lang) {
  const l = lang === "vi" ? "vi" : "en";
  localStorage.setItem(LANG_KEY, l);
  return l;
}

/** Translate key with optional {name} placeholders */
function t(key, vars = {}) {
  const lang = getLang();
  let s = (I18N[lang] && I18N[lang][key]) || (I18N.en && I18N.en[key]) || key;
  Object.keys(vars).forEach((k) => {
    s = s.replace(new RegExp(`\\{${k}\\}`, "g"), String(vars[k]));
  });
  return s;
}

function applyI18n(root = document) {
  const lang = getLang();
  document.documentElement.lang = lang;

  root.querySelectorAll("[data-i18n]").forEach((el) => {
    const key = el.getAttribute("data-i18n");
    if (!key) return;
    const html = el.hasAttribute("data-i18n-html");
    if (html) el.innerHTML = t(key);
    else el.textContent = t(key);
  });

  root.querySelectorAll("[data-i18n-placeholder]").forEach((el) => {
    const key = el.getAttribute("data-i18n-placeholder");
    if (key) el.setAttribute("placeholder", t(key));
  });

  // Toggle UI state
  document.querySelectorAll(".lang-toggle").forEach((wrap) => {
    wrap.classList.toggle("is-vi", lang === "vi");
    wrap.classList.toggle("is-en", lang === "en");
    const input = wrap.querySelector('input[type="checkbox"]');
    if (input) input.checked = lang === "vi";
  });
}
