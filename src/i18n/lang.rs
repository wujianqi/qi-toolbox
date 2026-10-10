//! 国际化文案（对齐 windui 0.19 内建 i18n 方案）。
//!
//! 译文存放在 `src/i18n/zh-CN.toml` 与 `src/i18n/en.toml`，构建期内嵌，
//! 经 [`windui::i18n::install`] 装载；运行期用 [`windui::i18n::LocaleHandle`]
//! 热切换，`current()` 拿到的目录快照按 key 查文。
//!
//! 每个 `lang::XXX()` 保持原函数名：无参文案返回 `String`；带参文案直接收
//! 参数（内部走 `Message` 位置占位 `{0}..{n}`），调用点不再手工 `replace("{}")`。

// 文案函数名刻意与 i18n key 大写风格一致（如 `SFTP_HOST()` ↔ key `sftp.host`），
// 集中豁免蛇形命名检查。
#![allow(non_snake_case)]
// 本模块是 i18n key 的完整镜像目录：所有 key 都生成对应函数，部分暂未被 UI
// 引用属预期，统一豁免死代码检查。
#![allow(dead_code)]

use windui::i18n::{ArgValue, Catalog, Initial, Locales, Message};

/// 内嵌的两份译文文件（语言 id 取自各文件 `[meta] locale`）。
const ZH_CN: &str = include_str!("zh-CN.toml");
const EN: &str = include_str!("en.toml");

/// 装载译文并选定启动语言（跟随系统偏好，中文系统→zh-CN，否则→en）。
/// 在创建 `App` 之前调用一次。
pub fn install() {
    windui::i18n::install(
        Locales::builder()
            .embed(ZH_CN)
            .embed(EN)
            .initial(Initial::System)
            .build(),
    );
}

/// 运行期语言句柄：克隆进控件回调，`set` 热切换，下一帧整树跟随。
#[allow(unused_imports)] // re-export：供 layout.rs 等以 lang::LocaleHandle 引用
pub use windui::i18n::LocaleHandle;
pub fn is_zh() -> bool {
    windui::i18n::language().starts_with("zh")
}

fn cat() -> Catalog {
    (*windui::i18n::current()).clone()
}

fn text(key: &str) -> String {
    windui::i18n::current().text(key)
}

fn msg(key: &str) -> Message {
    Message::new(key.to_string())
}

// ────────────────────── 软件名称 ──────────────────────
pub fn APP_NAME() -> String {
    text("app.name")
}

// ────────────────────── 侧栏菜单 ──────────────────────
pub fn TAB_2FA() -> String {
    text("tab.two_fa")
}
pub fn TAB_PASSWORD() -> String {
    text("tab.password")
}
pub fn TAB_TURSO() -> String {
    text("tab.turso")
}
pub fn TAB_SFTP() -> String {
    text("tab.sftp")
}
pub fn TAB_REMOTE() -> String {
    text("tab.remote")
}
pub fn TAB_ABOUT() -> String {
    text("tab.about")
}

// ────────────────────── 侧栏底部 toggle ──────────────────────
pub fn TOGGLE_LANG() -> String {
    text("toggle.lang")
}
pub fn TOGGLE_THEME() -> String {
    text("toggle.theme")
}
pub fn TOGGLE_SIDEBAR() -> String {
    text("toggle.sidebar")
}

// ────────────────────── 管理弹窗共用 ──────────────────────
pub fn MGR_LIST() -> String {
    text("mgr.list")
}
pub fn MGR_FORM() -> String {
    text("mgr.form")
}
pub fn MGR_EMPTY() -> String {
    text("mgr.empty")
}

// ────────────────────── SFTP 页 ──────────────────────
pub fn SFTP_HOST() -> String {
    text("sftp.host")
}
pub fn SFTP_PORT() -> String {
    text("sftp.port")
}
pub fn SFTP_USER() -> String {
    text("sftp.user")
}
pub fn SFTP_PASS() -> String {
    text("sftp.pass")
}
pub fn SFTP_KEY_PATH() -> String {
    text("sftp.key_path")
}
pub fn SFTP_KEY_READ_FAIL(path: String, e: String) -> String {
    cat().format(&msg("sftp.key_read_fail").push(path).push(e))
}
pub fn SFTP_KEY_DECODE_FAIL(e: String) -> String {
    cat().format(&msg("sftp.key_decode_fail").push(e))
}
pub fn SFTP_HOST_CHANGED(host: String, fp: String, saved: String) -> String {
    cat().format(&msg("sftp.host_changed").push(host).push(fp).push(saved))
}
pub fn SFTP_CONNECT() -> String {
    text("sftp.connect")
}
pub fn SFTP_DISCONNECT() -> String {
    text("sftp.disconnect")
}
pub fn SFTP_CONNECTED() -> String {
    text("sftp.connected")
}
pub fn SFTP_PATH() -> String {
    text("sftp.path")
}
pub fn SFTP_UP() -> String {
    text("sftp.up")
}
pub fn SFTP_REFRESH() -> String {
    text("sftp.refresh")
}
pub fn SFTP_MKDIR() -> String {
    text("sftp.mkdir")
}
pub fn SFTP_UPLOAD() -> String {
    text("sftp.upload")
}
pub fn SFTP_DOWNLOAD() -> String {
    text("sftp.download")
}
pub fn SFTP_DELETE() -> String {
    text("sftp.delete")
}
pub fn SFTP_PICK_UPLOAD() -> String {
    text("sftp.pick_upload")
}
pub fn SFTP_PICK_DOWNLOAD() -> String {
    text("sftp.pick_download")
}
pub fn SFTP_MKDIR_TITLE() -> String {
    text("sftp.mkdir")
}
pub fn SFTP_MKDIR_HINT() -> String {
    text("sftp.mkdir_hint")
}
pub fn SFTP_OK() -> String {
    text("sftp.ok")
}
pub fn SFTP_CANCEL() -> String {
    text("sftp.cancel")
}
pub fn SFTP_NO_CONN() -> String {
    text("sftp.no_conn")
}
pub fn SFTP_SITE_NONE() -> String {
    text("sftp.site_none")
}
pub fn SFTP_SITE_NEW() -> String {
    text("sftp.site_new")
}
pub fn SFTP_SITE_EDIT() -> String {
    text("sftp.site_edit")
}
pub fn SFTP_SITE_DEL() -> String {
    text("sftp.site_del")
}
pub fn SFTP_SITE_MGR() -> String {
    text("sftp.site_mgr")
}
pub fn SFTP_SITE_SAVE() -> String {
    text("sftp.site_save")
}
pub fn SFTP_SITE_TITLE() -> String {
    text("sftp.site_title")
}
pub fn SFTP_SITE_NAME() -> String {
    text("sftp.site_name")
}
pub fn SFTP_NO_SELECT() -> String {
    text("sftp.no_select")
}
pub fn SFTP_NO_FILE_SELECT() -> String {
    text("sftp.no_file_select")
}
pub fn SFTP_DELETE_CONFIRM_MULTI(count: impl Into<ArgValue>) -> String {
    cat().format(&msg("sftp.delete_confirm_multi").push(count))
}
pub fn SFTP_DELETE_DIR() -> String {
    text("sftp.delete_dir")
}
pub fn SFTP_ERR_CONNECT(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("sftp.err_connect").push(e))
}
pub fn SFTP_ERR_AUTH() -> String {
    text("sftp.err_auth")
}
pub fn SFTP_ERR_LIST(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("sftp.err_list").push(e))
}
pub fn SFTP_ERR_MKDIR(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("sftp.err_mkdir").push(e))
}
pub fn SFTP_ERR_DELETE(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("sftp.err_delete").push(e))
}
pub fn SFTP_ERR_UPLOAD(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("sftp.err_upload").push(e))
}
pub fn SFTP_ERR_DOWNLOAD(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("sftp.err_download").push(e))
}
pub fn SFTP_ERR_CHMOD(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("sftp.err_chmod").push(e))
}
pub fn SFTP_DONE_CHMOD() -> String {
    text("sftp.done_chmod")
}
pub fn SFTP_PERM() -> String {
    text("sftp.perm")
}
pub fn SFTP_PERM_TITLE() -> String {
    text("sftp.perm_title")
}
pub fn SFTP_PERM_OWNER() -> String {
    text("sftp.perm_owner")
}
pub fn SFTP_PERM_GROUP() -> String {
    text("sftp.perm_group")
}
pub fn SFTP_PERM_OTHERS() -> String {
    text("sftp.perm_others")
}
pub fn SFTP_PERM_READ() -> String {
    text("sftp.perm_read")
}
pub fn SFTP_PERM_WRITE() -> String {
    text("sftp.perm_write")
}
pub fn SFTP_PERM_EXEC() -> String {
    text("sftp.perm_exec")
}
pub fn SFTP_PERM_OF(name: impl Into<ArgValue>) -> String {
    cat().format(&msg("sftp.perm_of").push(name))
}
pub fn SFTP_PERM_NO_PERM() -> String {
    text("sftp.perm_no_perm")
}
pub fn SFTP_PERM_APPLY_TIP() -> String {
    text("sftp.perm_apply_tip")
}
pub fn SFTP_DONE_MKDIR() -> String {
    text("sftp.done_mkdir")
}
pub fn SFTP_DONE_DELETE() -> String {
    text("sftp.done_delete")
}
pub fn SFTP_DONE_UPLOAD(name: impl Into<ArgValue>) -> String {
    cat().format(&msg("sftp.done_upload").push(name))
}
pub fn SFTP_DONE_DOWNLOAD(name: impl Into<ArgValue>) -> String {
    cat().format(&msg("sftp.done_download").push(name))
}
pub fn SFTP_CMD() -> String {
    text("sftp.cmd")
}
pub fn SFTP_CMD_WIN_TITLE() -> String {
    text("sftp.cmd_win_title")
}
pub fn SFTP_CMD_HINT() -> String {
    text("sftp.cmd_hint")
}
pub fn SFTP_CMD_EXEC() -> String {
    text("sftp.cmd_exec")
}
pub fn SFTP_ERR_EXEC(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("sftp.err_exec").push(e))
}
pub fn SSH_CMD_STOP() -> String {
    text("ssh.cmd_stop")
}
pub fn SSH_CMD_RUNNING() -> String {
    text("ssh.cmd_running")
}
pub fn SSH_CMD_STOP_TIP() -> String {
    text("ssh.cmd_stop_tip")
}
pub fn SSH_CMD_ABORTED() -> String {
    text("ssh.cmd_aborted")
}
pub fn SSH_CMD_ADD() -> String {
    text("ssh.cmd_add")
}
pub fn SSH_CMD_ADD_BTN() -> String {
    text("ssh.cmd_add_btn")
}
pub fn SSH_CMD_ADD_HINT() -> String {
    text("ssh.cmd_add_hint")
}
pub fn SSH_CMD_REMOVE_TIP() -> String {
    text("ssh.cmd_remove_tip")
}
pub fn SSH_TPL_SYS() -> String {
    text("ssh.tpl_sys")
}
pub fn SSH_TPL_SVC() -> String {
    text("ssh.tpl_svc")
}
pub fn SSH_TPL_SEC() -> String {
    text("ssh.tpl_sec")
}
pub fn SSH_TPL_APP() -> String {
    text("ssh.tpl_app")
}
pub fn SSH_TPL_DEPLOY() -> String {
    text("ssh.tpl_deploy")
}
pub fn SSH_TPL_NET() -> String {
    text("ssh.tpl_net")
}
pub fn SSH_TPL_LOG() -> String {
    text("ssh.tpl_log")
}
pub fn SSH_TPL_DB() -> String {
    text("ssh.tpl_db")
}
pub fn SSH_TPL_RUNTIME() -> String {
    text("ssh.tpl_runtime")
}
pub fn SSH_TPL_ARCHIVE() -> String {
    text("ssh.tpl_archive")
}
pub fn SSH_TPL_MY() -> String {
    text("ssh.tpl_my")
}
pub fn SSH_TPL_GROUP() -> String {
    text("ssh.tpl_group")
}
pub fn SSH_TPL_ALL() -> String {
    text("ssh.tpl_all")
}
pub fn SSH_TPL_SEARCH() -> String {
    text("ssh.tpl_search")
}
pub fn SSH_TPL_EMPTY() -> String {
    text("ssh.tpl_empty")
}

// ────────────────────── 远程检测页 ──────────────────────
pub fn REMOTE_CONNECT() -> String {
    text("remote.connect")
}
pub fn REMOTE_DIALOG_TITLE() -> String {
    text("remote.dialog_title")
}
pub fn REMOTE_URL_LABEL() -> String {
    text("remote.url_label")
}
pub fn REMOTE_URL_HINT() -> String {
    text("remote.url_hint")
}
pub fn REMOTE_URL_SUPPORT() -> String {
    text("remote.url_support")
}
pub fn REMOTE_PING() -> String {
    text("remote.ping")
}
pub fn REMOTE_SSL() -> String {
    text("remote.ssl")
}
pub fn REMOTE_WEB() -> String {
    text("remote.web")
}
pub fn REMOTE_ENDPOINT() -> String {
    text("remote.endpoint")
}
pub fn REMOTE_CARD_RESULT() -> String {
    text("remote.card_result")
}
pub fn REMOTE_RESULT_HINT() -> String {
    text("remote.result_hint")
}
pub fn REMOTE_CONNECTED(rtt: impl Into<ArgValue>, ip: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.connected").push(rtt).push(ip))
}
pub fn REMOTE_PING_OK(
    min: impl Into<ArgValue>,
    avg: impl Into<ArgValue>,
    max: impl Into<ArgValue>,
    count: impl Into<ArgValue>,
) -> String {
    cat().format(
        &msg("remote.ping_ok")
            .push(min)
            .push(avg)
            .push(max)
            .push(count),
    )
}
pub fn REMOTE_BUSY() -> String {
    text("remote.busy")
}
pub fn REMOTE_SSL_TITLE() -> String {
    text("remote.ssl_title")
}
pub fn REMOTE_SSL_SUBJECT() -> String {
    text("remote.ssl_subject")
}
pub fn REMOTE_SSL_ISSUER() -> String {
    text("remote.ssl_issuer")
}
pub fn REMOTE_SSL_VALID(from: impl Into<ArgValue>, to: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.ssl_valid").push(from).push(to))
}
pub fn REMOTE_SSL_DAYS(days: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.ssl_days").push(days))
}
pub fn REMOTE_SSL_EXPIRED(days: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.ssl_expired").push(days))
}
pub fn REMOTE_WEB_OK(
    code: impl Into<ArgValue>,
    reason: impl Into<ArgValue>,
    bytes: impl Into<ArgValue>,
    rtt: impl Into<ArgValue>,
) -> String {
    cat().format(
        &msg("remote.web_ok")
            .push(code)
            .push(reason)
            .push(bytes)
            .push(rtt),
    )
}
pub fn REMOTE_WEB_REDIRECTS() -> String {
    text("remote.web_redirects")
}
pub fn REMOTE_WEB_FINAL() -> String {
    text("remote.web_final")
}
pub fn REMOTE_WEB_CTYPE() -> String {
    text("remote.web_ctype")
}
pub fn REMOTE_WEB_SERVER() -> String {
    text("remote.web_server")
}
pub fn REMOTE_QR() -> String {
    text("remote.qr")
}
pub fn REMOTE_QR_TITLE() -> String {
    text("remote.qr_title")
}
pub fn REMOTE_QR_GEN() -> String {
    text("remote.qr_gen")
}
pub fn REMOTE_QR_EMPTY() -> String {
    text("remote.qr_empty")
}
pub fn REMOTE_QR_SUPPORT() -> String {
    text("remote.qr_support")
}
pub fn REMOTE_NOT_TLS() -> String {
    text("remote.not_tls")
}
pub fn REMOTE_WEB_HTTP_ONLY() -> String {
    text("remote.web_http_only")
}
pub fn REMOTE_ERR_EMPTY() -> String {
    text("remote.err_empty")
}
pub fn REMOTE_ERR_PARSE(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.err_parse").push(e))
}
pub fn REMOTE_ERR_SCHEME(scheme: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.err_scheme").push(scheme))
}
pub fn REMOTE_ERR_HOST() -> String {
    text("remote.err_host")
}
pub fn REMOTE_ERR_DNS(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.err_dns").push(e))
}
pub fn REMOTE_ERR_CONNECT(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.err_connect").push(e))
}
pub fn REMOTE_ERR_TLS(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.err_tls").push(e))
}
pub fn REMOTE_ERR_NO_CERT() -> String {
    text("remote.err_no_cert")
}
pub fn REMOTE_ERR_CERT(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.err_cert").push(e))
}
pub fn REMOTE_ERR_HTTP(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.err_http").push(e))
}
pub fn REMOTE_ERR_BAD_RESPONSE() -> String {
    text("remote.err_bad_response")
}
pub fn REMOTE_ERR_READ(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.err_read").push(e))
}
pub fn REMOTE_PANIC() -> String {
    text("remote.panic")
}

// ── 安全检测 ──
pub fn REMOTE_SEC() -> String {
    text("remote.sec")
}
pub fn SEC_DISCLAIMER() -> String {
    text("remote.sec_disclaimer")
}
pub fn REMOTE_EXPORT_REPORT() -> String {
    text("remote.export_md")
}
pub fn REMOTE_EXPORT_REPORT_TITLE() -> String {
    text("remote.export_md_title")
}
pub fn REMOTE_EXPORT_REPORT_EMPTY() -> String {
    text("remote.export_md_empty")
}
pub fn REMOTE_EXPORT_REPORT_DONE(path: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.export_md_done").push(path))
}
pub fn SFTP_BMK_ADD() -> String {
    text("remote.bmk_add")
}
pub fn SFTP_BMK_ADDED() -> String {
    text("remote.bmk_added")
}
pub fn SFTP_BMK_NONE() -> String {
    text("remote.bmk_none")
}
pub fn SFTP_BMK_DEL() -> String {
    text("remote.bmk_del")
}
pub fn SQL_HISTORY() -> String {
    text("remote.sql_history")
}
pub fn SQL_SAVED() -> String {
    text("sql.saved")
}
pub fn SQL_SAVED_EMPTY() -> String {
    text("sql.saved_empty")
}
pub fn SQL_SAVED_ADD() -> String {
    text("sql.saved_add")
}
pub fn SQL_SAVED_DEL_BTN() -> String {
    text("sql.saved_del_btn")
}
pub fn SQL_SAVED_DONE(name: String) -> String {
    cat().format(&msg("sql.saved_done").push(name))
}
pub fn SQL_SAVED_UNTITLED() -> String {
    text("sql.saved_untitled")
}
pub fn SQL_MGR_BTN() -> String {
    text("sql.mgr_btn")
}
pub fn SQL_MGR_TITLE() -> String {
    text("sql.mgr_title")
}
pub fn SQL_READONLY() -> String {
    text("sql.readonly")
}
pub fn SQL_READONLY_ON() -> String {
    text("sql.readonly_on")
}
pub fn SQL_READONLY_OFF() -> String {
    text("sql.readonly_off")
}
pub fn SQL_READONLY_BLOCKED() -> String {
    text("sql.readonly_blocked")
}
pub fn SQL_HISTORY_EMPTY() -> String {
    text("remote.sql_history_empty")
}
pub fn SQL_HISTORY_CLEARED() -> String {
    text("remote.sql_history_cleared")
}
pub fn SQL_HISTORY_CLEAR_BTN() -> String {
    text("remote.sql_history_clear_btn")
}
pub fn SEC_TITLE() -> String {
    text("remote.sec_title")
}
pub fn SEC_OK(count: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.sec_ok").push(count))
}
pub fn SEC_SUMMARY(high: impl Into<ArgValue>, mid: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.sec_summary").push(high).push(mid))
}
pub fn SEC_PATH_EXPOSED(path: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.sec_path_exposed").push(path))
}
pub fn SEC_PATH_DETAIL(code: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.sec_path_detail").push(code))
}
pub fn SEC_HSTS() -> String {
    text("remote.sec_hsts")
}
pub fn SEC_CSP() -> String {
    text("remote.sec_csp")
}
pub fn SEC_XFO() -> String {
    text("remote.sec_xfo")
}
pub fn SEC_XCTO() -> String {
    text("remote.sec_xcto")
}
pub fn SEC_REFERRER() -> String {
    text("remote.sec_referrer")
}
pub fn SEC_HEADER_MISSING(name: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.sec_header_missing").push(name))
}
pub fn SEC_HEADER_DETAIL(name: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.sec_header_detail").push(name))
}
pub fn SEC_PORT_OPEN(port: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.sec_port_open").push(port))
}
pub fn SEC_PORT_DETAIL(port: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.sec_port_detail").push(port))
}
pub fn SEC_TLS10() -> String {
    text("remote.sec_tls10")
}
pub fn SEC_TLS11() -> String {
    text("remote.sec_tls11")
}
pub fn SEC_TLS_OLD(ver: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.sec_tls_old").push(ver))
}
pub fn SEC_TLS_DETAIL() -> String {
    text("remote.sec_tls_detail")
}

// ── 响应体 / SEO 分析 ──
pub fn REMOTE_BODY() -> String {
    text("remote.body")
}
pub fn SEO_BAD_STATUS(code: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.seo_bad_status").push(code))
}
pub fn SEO_TITLE() -> String {
    text("remote.seo_title")
}
pub fn SEO_TITLE_LABEL() -> String {
    text("remote.seo_title_label")
}
pub fn SEO_DESCRIPTION() -> String {
    text("remote.seo_description")
}
pub fn SEO_KEYWORDS() -> String {
    text("remote.seo_keywords")
}
pub fn SEO_CANONICAL() -> String {
    text("remote.seo_canonical")
}
pub fn SEO_VIEWPORT() -> String {
    text("remote.seo_viewport")
}
pub fn SEO_H1() -> String {
    text("remote.seo_h1")
}
pub fn SEO_IMAGES_N(total: impl Into<ArgValue>, no_alt: impl Into<ArgValue>) -> String {
    cat().format(&msg("remote.seo_images").push(total).push(no_alt))
}
pub fn SEO_HTML_SIZE() -> String {
    text("remote.seo_html_size")
}
pub fn SEO_MISSING() -> String {
    text("remote.seo_missing")
}

// ────────────────────── 关于 ──────────────────────
pub fn ABOUT_DESC() -> String {
    text("about.desc")
}
pub fn ABOUT_AUTHOR() -> String {
    text("about.author")
}
pub fn ABOUT_BUILT() -> String {
    text("about.built")
}
pub fn ABOUT_LICENSE_TITLE() -> String {
    text("about.license_title")
}
pub fn ABOUT_LICENSE() -> String {
    text("about.license")
}
pub fn ABOUT_NEW_VERSION(ver: impl Into<ArgValue>) -> String {
    cat().format(&msg("about.new_version").push(ver))
}
pub fn ABOUT_GOTO_RELEASES() -> String {
    text("about.goto_releases")
}
pub fn ABOUT_CHECK_UPDATE() -> String {
    text("about.check_update")
}
pub fn ABOUT_OPEN_LOGS() -> String {
    text("about.open_logs")
}
pub fn ABOUT_UP_TO_DATE() -> String {
    text("about.up_to_date")
}
pub fn ABOUT_CHECK_FAILED() -> String {
    text("about.check_failed")
}
pub fn ABOUT_SPONSOR() -> String {
    text("about.sponsor")
}

// ────────────────────── 密码页 ──────────────────────
pub fn PWD_CARD_SETUP() -> String {
    text("pwd.card_setup")
}
pub fn PWD_CARD_OUTPUT() -> String {
    text("pwd.card_output")
}
pub fn PWD_PLATFORM() -> String {
    text("pwd.platform")
}
pub fn PWD_ALGO() -> String {
    text("pwd.algo")
}
pub fn PWD_GEN_8() -> String {
    text("pwd.gen_8")
}
pub fn PWD_GEN_12() -> String {
    text("pwd.gen_12")
}
pub fn PWD_GEN_16() -> String {
    text("pwd.gen_16")
}
pub fn PWD_INPUT_LABEL() -> String {
    text("pwd.input_label")
}
pub fn PWD_INPUT_HINT() -> String {
    text("pwd.input_hint")
}
pub fn PWD_ENCRYPT() -> String {
    text("pwd.encrypt")
}
pub fn PWD_OUTPUT_LABEL() -> String {
    text("pwd.output_label")
}
pub fn PWD_SQL_LABEL() -> String {
    text("pwd.sql_label")
}
pub fn PWD_SAVE() -> String {
    text("pwd.save")
}
pub fn PWD_SAVE_NEED_ENCRYPT() -> String {
    text("pwd.save_need_encrypt")
}
pub fn PWD_USED_FOR() -> String {
    text("pwd.used_for")
}
pub fn PWD_USED_FOR_HINT() -> String {
    text("pwd.used_for_hint")
}
pub fn PWD_SAVED_LABEL() -> String {
    text("pwd.saved_label")
}
pub fn PWD_SAVED_EMPTY() -> String {
    text("pwd.saved_empty")
}
pub fn PWD_SAVED_FAIL(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("pwd.saved_fail").push(e))
}
pub fn PWD_SAVED_DONE(name: impl Into<ArgValue>) -> String {
    cat().format(&msg("pwd.saved_done").push(name))
}
pub fn PWD_COPIED() -> String {
    text("pwd.copied")
}
pub fn PWD_COPY() -> String {
    text("pwd.copy")
}
pub fn PWD_DELETE() -> String {
    text("pwd.delete")
}
pub fn PWD_CARD_SAVE() -> String {
    text("pwd.card_save")
}
pub fn REMOTE_URL_NONE() -> String {
    text("remote.url_none")
}
pub fn REMOTE_URL_SAVE() -> String {
    text("remote.url_save")
}
pub fn REMOTE_URL_DEL() -> String {
    text("remote.url_del")
}

// ────────────────────── 平台预设标签 ──────────────────────
pub fn PLAT_NONE() -> String {
    text("plat.none")
}
pub fn PLAT_LARAVEL() -> String {
    text("plat.laravel")
}
pub fn PLAT_DJANGO() -> String {
    text("plat.django")
}
pub fn PLAT_SPRING() -> String {
    text("plat.spring")
}
pub fn PLAT_EXPRESS() -> String {
    text("plat.express")
}
pub fn PLAT_DOTNET() -> String {
    text("plat.dotnet")
}
pub fn PLAT_RAILS() -> String {
    text("plat.rails")
}
pub fn PLAT_WORDPRESS() -> String {
    text("plat.wordpress")
}
pub fn PLAT_GO() -> String {
    text("plat.go")
}

// ────────────────────── TOTP 页 ──────────────────────
pub fn TOTP_CARD_SETUP() -> String {
    text("totp.card_setup")
}
pub fn TOTP_CARD_OUTPUT() -> String {
    text("totp.card_output")
}
pub fn TOTP_GEN_KEY() -> String {
    text("totp.gen_key")
}
pub fn TOTP_GEN_HINT() -> String {
    text("totp.gen_hint")
}
pub fn TOTP_BITS_32() -> String {
    text("totp.bits_32")
}
pub fn TOTP_BITS_16() -> String {
    text("totp.bits_16")
}
pub fn TOTP_KEY_LABEL() -> String {
    text("totp.key_label")
}
pub fn TOTP_KEY_WARN() -> String {
    text("totp.key_warn")
}
pub fn TOTP_ACCOUNT() -> String {
    text("totp.account")
}
pub fn TOTP_ACCOUNT_HINT() -> String {
    text("totp.account_hint")
}
pub fn TOTP_ISSUER() -> String {
    text("totp.issuer")
}
pub fn TOTP_ISSUER_HINT() -> String {
    text("totp.issuer_hint")
}
pub fn TOTP_KEY_INPUT_HINT() -> String {
    text("totp.key_input_hint")
}
pub fn TOTP_GENERATE() -> String {
    text("totp.generate")
}
pub fn TOTP_SAVE() -> String {
    text("totp.save")
}
pub fn TOTP_SAVED(key: impl Into<ArgValue>) -> String {
    cat().format(&msg("totp.saved").push(key))
}
pub fn TOTP_SAVE_FAIL(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("totp.save_fail").push(e))
}
pub fn TOTP_QR() -> String {
    text("totp.qr")
}
pub fn TOTP_SCAN_HINT() -> String {
    text("totp.scan_hint")
}
pub fn TOTP_KEYS_LABEL() -> String {
    text("totp.keys_label")
}
pub fn TOTP_KEYS_NONE() -> String {
    text("totp.keys_none")
}
pub fn TOTP_KEY_PICK() -> String {
    text("totp.key_pick")
}
pub fn TOTP_KEY_DEL() -> String {
    text("totp.key_del")
}
pub fn TOTP_SAVE_PNG() -> String {
    text("totp.save_png")
}
pub fn TOTP_PNG_SAVED(path: &str) -> String {
    cat().format(&msg("totp.png_saved").push(path))
}
pub fn TOTP_PNG_SAVE_FAIL(e: String) -> String {
    cat().format(&msg("totp.png_save_fail").push(e))
}
pub fn TOTP_IMPORT() -> String {
    text("totp.import")
}
pub fn TOTP_IMPORT_HINT() -> String {
    text("totp.import_hint")
}
pub fn TOTP_IMPORT_DONE(name: String) -> String {
    cat().format(&msg("totp.import_done").push(name))
}
pub fn TOTP_IMPORT_BAD_PREFIX() -> String {
    text("totp.import_bad_prefix")
}
pub fn TOTP_IMPORT_UNSUPPORTED(t: String) -> String {
    cat().format(&msg("totp.import_unsupported").push(t))
}
pub fn TOTP_IMPORT_NO_SECRET() -> String {
    text("totp.import_no_secret")
}
pub fn TOTP_COPIED() -> String {
    text("totp.copied")
}
pub fn TOTP_REFRESH_IN(secs: impl Into<ArgValue>) -> String {
    cat().format(&msg("totp.refresh_in").push(secs))
}
pub fn TOTP_KEY_INVALID() -> String {
    text("totp.key_invalid")
}
pub fn TOTP_LIVE_HINT() -> String {
    text("totp.live_hint")
}
pub fn TOTP_AUTO_COPY_LABEL() -> String {
    text("totp.auto_copy_label")
}
pub fn TOTP_AUTO_COPY_ON() -> String {
    text("totp.auto_copy_on")
}
pub fn TOTP_AUTO_COPY_OFF() -> String {
    text("totp.auto_copy_off")
}

// ────────────────────── S3 页 ──────────────────────
pub fn S3_TAB() -> String {
    text("s3.tab")
}
pub fn S3_SITE_NONE() -> String {
    text("s3.site_none")
}
pub fn S3_SITE_NEW() -> String {
    text("s3.site_new")
}
pub fn S3_SITE_EDIT() -> String {
    text("s3.site_edit")
}
pub fn S3_SITE_DEL() -> String {
    text("s3.site_del")
}
pub fn S3_SITE_MGR() -> String {
    text("s3.site_mgr")
}
pub fn S3_SITE_TITLE() -> String {
    text("s3.site_title")
}
pub fn S3_OPEN() -> String {
    text("s3.open")
}
pub fn S3_UP() -> String {
    text("s3.up")
}
pub fn S3_REFRESH() -> String {
    text("s3.refresh")
}
pub fn S3_UPLOAD() -> String {
    text("s3.upload")
}
pub fn S3_DOWNLOAD() -> String {
    text("s3.download")
}
pub fn S3_PRESIGN() -> String {
    text("s3.presign")
}
pub fn S3_PRESIGN_DONE() -> String {
    text("s3.presign_done")
}
pub fn S3_PRESIGN_NO_KEY() -> String {
    text("s3.presign_no_key")
}
pub fn S3_FILTER_HINT() -> String {
    text("s3.filter_hint")
}
pub fn S3_UPLOADING() -> String {
    text("s3.uploading")
}
pub fn S3_DOWNLOADING() -> String {
    text("s3.downloading")
}
pub fn S3_UPLOADED(key: &str) -> String {
    cat().format(&msg("s3.uploaded").push(key))
}
pub fn S3_DOWNLOADED(path: &str) -> String {
    cat().format(&msg("s3.downloaded").push(path))
}
pub fn S3_MKDIR() -> String {
    text("s3.mkdir")
}
pub fn S3_MKDIR_HINT() -> String {
    text("s3.mkdir_hint")
}
pub fn S3_DELETE() -> String {
    text("s3.delete")
}
pub fn S3_DELETE_CONFIRM(n: usize) -> String {
    cat().format(&msg("s3.delete_confirm").push(n))
}
pub fn S3_DIR_TAG() -> String {
    text("s3.dir_tag")
}
pub fn S3_DIR_DELETED(n: usize) -> String {
    cat().format(&msg("s3.dir_deleted").push(n))
}
pub fn S3_DIR_MADE(name: &str) -> String {
    cat().format(&msg("s3.dir_made").push(name))
}
pub fn S3_DELETED(key: &str) -> String {
    cat().format(&msg("s3.deleted").push(key))
}
pub fn S3_ENDPOINT() -> String {
    text("s3.endpoint")
}
pub fn S3_REGION() -> String {
    text("s3.region")
}
pub fn S3_BUCKET() -> String {
    text("s3.bucket")
}
pub fn S3_ACCESS() -> String {
    text("s3.access")
}
pub fn S3_SECRET() -> String {
    text("s3.secret")
}
pub fn S3_PATH_STYLE() -> String {
    text("s3.path_style")
}
pub fn S3_VHOST_STYLE() -> String {
    text("s3.vhost")
}
pub fn S3_PATH_STYLE_OPT() -> String {
    text("s3.path_style_opt")
}
pub fn S3_BAK() -> String {
    text("s3.bak")
}
pub fn S3_BAK_OK(key: &str) -> String {
    cat().format(&msg("s3.bak_ok").push(key))
}
pub fn S3_RESTORE() -> String {
    text("s3.restore")
}
pub fn S3_RESTORE_CONFIRM(name: &str) -> String {
    cat().format(&msg("s3.restore_confirm").push(name))
}
pub fn S3_RESTORE_OK(path: &str) -> String {
    cat().format(&msg("s3.restore_ok").push(path))
}

// ────────────────────── 主口令 ──────────────────────
pub fn MASTER_SETUP_TITLE() -> String {
    text("master.setup_title")
}
pub fn MASTER_UNLOCK_TITLE() -> String {
    text("master.unlock_title")
}
pub fn MASTER_PASS_LABEL() -> String {
    text("master.pass")
}
pub fn MASTER_PASS_CONFIRM() -> String {
    text("master.pass_confirm")
}
pub fn MASTER_PASS_HINT() -> String {
    text("master.pass_hint")
}
pub fn MASTER_PASS_MISMATCH() -> String {
    text("master.pass_mismatch")
}
pub fn MASTER_PASS_SHORT() -> String {
    text("master.pass_short")
}
pub fn MASTER_WRONG() -> String {
    text("master.wrong")
}
pub fn MASTER_SETUP_WARN() -> String {
    text("master.setup_warn")
}
pub fn MASTER_UNLOCK_HINT() -> String {
    text("master.unlock_hint")
}
pub fn MASTER_BTN_OK() -> String {
    text("master.btn_ok")
}
pub fn MASTER_BTN_CANCEL() -> String {
    text("master.btn_cancel")
}

// ────────────────────── Turso 页 ──────────────────────
pub fn TURSO_OPEN() -> String {
    text("turso.open")
}
pub fn TURSO_FILE_TITLE() -> String {
    text("turso.file_title")
}
pub fn TURSO_FILE_FILTER1() -> String {
    text("turso.file_filter1")
}
pub fn TURSO_FILE_FILTER2() -> String {
    text("turso.file_filter2")
}
pub fn TURSO_MODE_LOCAL() -> String {
    text("turso.mode_local")
}
pub fn TURSO_MODE_REMOTE() -> String {
    text("turso.mode_remote")
}
pub fn TURSO_URL() -> String {
    text("turso.url")
}
pub fn TURSO_PATH_HINT() -> String {
    text("turso.path_hint")
}
pub fn TURSO_URL_HINT() -> String {
    text("turso.url_hint")
}
pub fn TURSO_TOKEN_HINT() -> String {
    text("turso.token_hint")
}
pub fn TURSO_DB_TITLE() -> String {
    text("turso.db_title")
}
pub fn TURSO_CONNECT() -> String {
    text("turso.connect")
}
pub fn TURSO_DISCONNECT() -> String {
    text("turso.disconnect")
}
pub fn TURSO_CONNECTED(name: impl Into<ArgValue>, tables: impl Into<ArgValue>) -> String {
    cat().format(&msg("turso.connected").push(name).push(tables))
}
pub fn TURSO_REFRESHED() -> String {
    text("turso.refreshed")
}
pub fn TURSO_REFRESH() -> String {
    text("turso.refresh")
}
pub fn TURSO_OPEN_SQL() -> String {
    text("turso.open_sql")
}
pub fn TURSO_SQL_GROUP() -> String {
    text("turso.sql_group")
}
pub fn TURSO_SQL_EXEC() -> String {
    text("turso.sql_exec")
}
pub fn TURSO_SQL_STATUS(cols: impl Into<ArgValue>, rows: impl Into<ArgValue>) -> String {
    cat().format(&msg("turso.sql_status").push(cols).push(rows))
}
pub fn TURSO_SQL_CLEAR() -> String {
    text("turso.sql_clear")
}
pub fn TURSO_UNKNOWN_PANIC() -> String {
    text("turso.unknown_panic")
}

// ────────────────────── MySQL / PostgreSQL 页 ──────────────────────
pub fn MYSQL_TAB() -> String {
    text("mysql.tab")
}
pub fn MYSQL_CONN_MGR() -> String {
    text("mysql.conn_mgr")
}
pub fn MYSQL_SITE_TITLE() -> String {
    text("mysql.site_title")
}
pub fn MYSQL_HOST() -> String {
    text("mysql.host")
}
pub fn MYSQL_PORT() -> String {
    text("mysql.port")
}
pub fn MYSQL_USER() -> String {
    text("mysql.user")
}
pub fn MYSQL_PASS() -> String {
    text("mysql.pass")
}
pub fn MYSQL_CONNECTED(count: impl Into<ArgValue>) -> String {
    cat().format(&msg("mysql.connected").push(count))
}
pub fn MYSQL_UNKNOWN_PANIC() -> String {
    text("mysql.unknown_panic")
}
pub fn MYSQL_EXPORT_DONE(path: impl Into<ArgValue>) -> String {
    cat().format(&msg("mysql.export_done").push(path))
}
pub fn PG_TAB() -> String {
    text("pg.tab")
}
pub fn PG_CONN_MGR() -> String {
    text("pg.conn_mgr")
}
pub fn PG_SITE_TITLE() -> String {
    text("pg.site_title")
}
pub fn PG_HOST() -> String {
    text("pg.host")
}
pub fn PG_PORT() -> String {
    text("pg.port")
}
pub fn PG_USER() -> String {
    text("pg.user")
}
pub fn PG_PASS() -> String {
    text("pg.pass")
}
pub fn PG_DB() -> String {
    text("pg.dbname")
}
pub fn PG_CONNECTED(count: impl Into<ArgValue>) -> String {
    cat().format(&msg("pg.connected").push(count))
}
pub fn PG_UNKNOWN_PANIC() -> String {
    text("pg.unknown_panic")
}
pub fn PG_EXPORT_DONE(path: impl Into<ArgValue>) -> String {
    cat().format(&msg("pg.export_done").push(path))
}
pub fn PG_CONN_HINT() -> String {
    text("pg.conn_hint")
}
// ────────────────────── Redis 页 ──────────────────────
pub fn REDIS_TAB() -> String {
    text("redis.tab")
}
pub fn REDIS_CONN_MGR() -> String {
    text("redis.conn_mgr")
}
pub fn REDIS_SITE_TITLE() -> String {
    text("redis.site_title")
}
pub fn REDIS_CONN_HINT() -> String {
    text("redis.conn_hint")
}
pub fn REDIS_CONNECTED(count: impl Into<ArgValue>) -> String {
    cat().format(&msg("redis.connected").push(count))
}
pub fn REDIS_UNKNOWN_PANIC() -> String {
    text("redis.unknown_panic")
}
pub fn REDIS_DB_LIST() -> String {
    text("redis.db_list")
}
pub fn REDIS_DB_NONE() -> String {
    text("redis.db_none")
}
pub fn REDIS_PREFIX_LIST() -> String {
    text("redis.prefix_list")
}
pub fn REDIS_KEY_LIST(count: impl Into<ArgValue>) -> String {
    cat().format(&msg("redis.key_list").push(count))
}
pub fn REDIS_NO_KEYS() -> String {
    text("redis.no_keys")
}
pub fn REDIS_CMD_HINT() -> String {
    text("redis.cmd_hint")
}
pub fn REDIS_CMD_PANEL() -> String {
    text("redis.cmd_panel")
}
pub fn REDIS_READONLY() -> String {
    text("redis.readonly")
}
pub fn REDIS_TTL_FOREVER() -> String {
    text("redis.ttl_forever")
}
pub fn REDIS_TTL_NE() -> String {
    text("redis.ttl_ne")
}
pub fn SQL_AFFECTED(n: impl Into<ArgValue>) -> String {
    cat().format(&msg("sql.affected").push(n))
}

// ────────────────────── DataTable ──────────────────────
pub fn DT_TABLE_LIST(count: impl Into<ArgValue>) -> String {
    cat().format(&msg("dt.table_list").push(count))
}
pub fn DT_NO_TABLE() -> String {
    text("dt.no_table")
}
pub fn DT_COL_ROW(cols: impl Into<ArgValue>, rows: impl Into<ArgValue>) -> String {
    cat().format(&msg("dt.col_row").push(cols).push(rows))
}
pub fn DT_COL_MORE(cols: impl Into<ArgValue>) -> String {
    cat().format(&msg("dt.col_more").push(cols))
}
pub fn DT_COLS() -> String {
    text("dt.cols")
}
pub fn DT_PREV() -> String {
    text("dt.prev")
}
pub fn DT_PAGE(cur: impl Into<ArgValue>, total: impl Into<ArgValue>) -> String {
    cat().format(&msg("dt.page").push(cur).push(total))
}
pub fn DT_NEXT() -> String {
    text("dt.next")
}
pub fn DT_CLOSE() -> String {
    text("dt.close")
}
pub fn DT_BACK() -> String {
    text("dt.back")
}
pub fn DT_ROW_DETAIL(row: impl Into<ArgValue>) -> String {
    cat().format(&msg("dt.row_detail").push(row))
}
pub fn DT_EXPORT() -> String {
    text("dt.export")
}
pub fn DT_EXPORT_TITLE() -> String {
    text("dt.export_title")
}
pub fn DT_EXPORT_SQL() -> String {
    text("dt.export_sql")
}
pub fn DT_EXPORT_SCHEMA() -> String {
    text("dt.export_schema")
}
pub fn DT_EXPORT_SCHEMA_TITLE() -> String {
    text("dt.export_schema_title")
}
pub fn TURSO_EXPORT_STARTED(done: impl Into<ArgValue>, total: impl Into<ArgValue>) -> String {
    cat().format(&msg("dt.export_started").push(done).push(total))
}
pub fn TURSO_EXPORT_DONE(path: impl Into<ArgValue>) -> String {
    cat().format(&msg("dt.export_done").push(path))
}
pub fn TURSO_EXPORT_NO_COLS() -> String {
    text("dt.export_no_cols")
}
pub fn TURSO_EXPORT_TRUNCATED(rows: impl Into<ArgValue>) -> String {
    cat().format(&msg("dt.export_truncated").push(rows))
}
pub fn ERR_EXPORT_OPEN(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("err.export_open").push(e))
}
pub fn ERR_EXPORT_WRITE(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("err.export_write").push(e))
}

// ────────────────────── 错误信息 ──────────────────────
pub fn ERR_RUNTIME(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("err.runtime").push(e))
}
pub fn ERR_CONNECT_DB(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("err.connect_db").push(e))
}
pub fn ERR_GET_CONN(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("err.get_conn").push(e))
}
pub fn ERR_THREAD_PANIC(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("err.thread_panic").push(e))
}
pub fn ERR_THREAD_PANIC_UNKNOWN() -> String {
    text("err.thread_panic_unknown")
}
pub fn ERR_QUERY_TABLES(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("err.query_tables").push(e))
}
pub fn ERR_TABLE_INFO(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("err.table_info").push(e))
}
pub fn ERR_QUERY_DATA(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("err.query_data").push(e))
}
pub fn ERR_EMPTY_SQL() -> String {
    text("err.empty_sql")
}
pub fn ERR_QUERY_FAIL(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("err.query_fail").push(e))
}
pub fn ERR_EXEC_FAIL(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("err.exec_fail").push(e))
}
pub fn ERR_SALT(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("err.salt").push(e))
}
pub fn ERR_ARGON2_PARAM(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("err.argon2_param").push(e))
}
pub fn ERR_ARGON2_HASH(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("err.argon2_hash").push(e))
}
pub fn ERR_BCRYPT_HASH(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("err.bcrypt_hash").push(e))
}
pub fn ERR_B64(e: impl Into<ArgValue>) -> String {
    cat().format(&msg("err.b64").push(e))
}
pub fn ERR_HASH_UNKNOWN() -> String {
    text("err.hash_unknown")
}
pub fn ERR_HASH_MALFORMED() -> String {
    text("err.hash_malformed")
}
pub fn PWD_VERIFY() -> String {
    text("pwd_verify.verify")
}
pub fn PWD_VERIFY_HASH_HINT() -> String {
    text("pwd_verify.verify_hash_hint")
}
pub fn PWD_VERIFY_OK(algo: String) -> String {
    cat().format(&msg("pwd_verify.verify_ok").push(algo))
}
pub fn PWD_VERIFY_FAIL(algo: String) -> String {
    cat().format(&msg("pwd_verify.verify_fail").push(algo))
}
pub fn PWD_HASH_UNKNOWN_ALGO() -> String {
    text("pwd_verify.hash_unknown_algo")
}

// ────────────────────── 通用（可选文本右键菜单）──────────────────────
pub fn MENU_COPY() -> String {
    text("menu.copy")
}
pub fn MENU_SELECT_ALL() -> String {
    text("menu.select_all")
}

// ────────────────────── 运维备忘页 ──────────────────────
pub fn MEMO_TAB() -> String {
    text("memo.tab")
}
pub fn MEMO_EMPTY() -> String {
    text("memo.empty")
}
pub fn MEMO_NEW() -> String {
    text("memo.new")
}
pub fn MEMO_SAVE() -> String {
    text("memo.save")
}
pub fn MEMO_DELETE() -> String {
    text("memo.delete")
}
pub fn MEMO_TITLE_PH() -> String {
    text("memo.title_ph")
}
pub fn MEMO_CONTENT_PH() -> String {
    text("memo.content_ph")
}
pub fn MEMO_DAY() -> String {
    text("memo.day")
}
pub fn MEMO_SAVED() -> String {
    text("memo.saved")
}
pub fn MEMO_DELETED() -> String {
    text("memo.deleted")
}
pub fn MEMO_CALENDAR() -> String {
    text("memo.calendar")
}
pub fn MEMO_TODAY() -> String {
    text("memo.today")
}
pub fn MEMO_ALL() -> String {
    text("memo.all")
}
pub fn MEMO_LIST_TITLE() -> String {
    text("memo.list_title")
}
pub fn MEMO_EMPTY_DAY(day: impl Into<ArgValue>) -> String {
    cat().format(&msg("memo.empty_day").push(day))
}
pub fn MEMO_EDIT_TITLE() -> String {
    text("memo.edit_title")
}
pub fn MEMO_NEW_TITLE() -> String {
    text("memo.new_title")
}

#[cfg(test)]
mod tests {
    use super::{EN, ZH_CN};
    use windui::i18n::lint;

    /// 两份译文必须可解析且互相一致：key 集合一致、占位符一致、变体齐全。
    /// TOML 写坏（语法错误、漏 key、占位符名字对不上）在这里当场报出，
    /// 而不是上线后界面冒出原文/空串。
    #[test]
    fn locale_files_parse_and_match() {
        let problems = lint::check(&[ZH_CN, EN]);
        assert!(
            problems.is_empty(),
            "i18n 译文存在不一致：\n{}",
            problems.join("\n")
        );
    }

    /// 用与 `install()` 相同的方式构建目录：每种语言的关键 key 应能取到非原文回退的译文。
    #[test]
    fn locales_build_and_resolve() {
        let locales = windui::i18n::Locales::builder()
            .embed(ZH_CN)
            .embed(EN)
            .build();
        // 语言 id 均来自各文件 [meta] locale
        let ids: Vec<String> = locales.available().into_iter().map(|l| l.id).collect();
        assert!(ids.contains(&"zh-CN".to_string()), "缺 zh-CN：{ids:?}");
        assert!(ids.contains(&"en".to_string()), "缺 en：{ids:?}");

        // zh-CN 缺的 key 回退到 en（meta.fallback = "en"）；双方都有则各自成文
        let zh = locales.catalog("zh-CN");
        assert_eq!(zh.text("app.name"), "奇兔宝");
        assert_eq!(zh.text("tab.about"), "关于软件");
        let en = locales.catalog("en");
        assert_eq!(en.text("app.name"), "Qi Toolbox");
        assert!(
            !en.text("tab.about").contains("tab.about"),
            "miss 时 key 原样返回"
        );
    }
}
