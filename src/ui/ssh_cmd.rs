//! SFTP 的「SSH 命令」独立子窗口 UI：命令输入 + 常用命令模板（可折叠）+ 输出区。
//!
//! 子窗与主窗共享 [`super::sftp::SftpUi`] 的信号句柄（Signal 为 Copy）：SFTP worker
//! 通道注册在 App 级，`ExecDone` 消息写入 `cmd_output`，子窗经信号实时刷新。
//! 独立成文件以控制 [`super::sftp`] 页代码体量，便于按板块扩展模板。

use std::sync::atomic::Ordering;

use windui::prelude::*;

use super::syntax_input::{LexerKind, SyntaxInput};
use super::{icons, input_dialog, select_text, sftp::SftpUi};
use crate::core::sftp;
use crate::lang;

/// 「SSH 命令」子窗口内容（独立窗口：主窗工具栏「命令」按钮经
/// `EventCtx::open_window` 打开，见 [`super::sftp::build_sftp_tab`]）。
/// 输出区用 multiline `text_input` 承载：文本可拖选、Ctrl+C / 右键复制。
pub(super) fn cmd_window(ui: &SftpUi) -> Element {
    let cmd = ui.cmd();
    let cmd_input = ui.cmd_input;
    let cmd_output = ui.cmd_output;
    let show_tpl = ui.show_tpl;
    let connected = ui.connected;
    let cmd_running = ui.cmd_running;
    let cancel = ui.cancel();
    let custom_cmds = ui.custom_cmds;
    let cmd_add_show = ui.cmd_add_show;
    let cmd_add_input = ui.cmd_add_input;

    // 执行按钮：复用现有 SSH 会话执行命令（需在线、输入非空、且当前无命令在跑；
    // 窗口可在断连后仍开着，无门控时 send 会静默失败，故与主页按钮一致检查 connected）
    let exec_btn = Element::button(lang::SFTP_CMD_EXEC())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::TERMINAL, Some(16)))
        .enabled_when(move || {
            connected.get() && !cmd_input.get().trim().is_empty() && !cmd_running.get()
        })
        .on_click({
            let cmd = cmd.clone();
            move |_| {
                let command = cmd_input.get().trim().to_string();
                if !command.is_empty() {
                    cmd_running.set(true);
                    let _ = cmd.send(sftp::SftpCmd::Exec { command });
                }
            }
        });

    // 停止按钮：命令执行中可用。服务器卡死/命令挂起时强制中断当前执行：
    // 置位共享取消标志，worker 的 exec 等待循环 ~100ms 内感知并返回，随后可重新执行
    let stop_btn = Element::button(lang::SSH_CMD_STOP())
        .small()
        .neutral()
        .tooltip(lang::SSH_CMD_STOP_TIP())
        .enabled_when(move || cmd_running.get())
        .on_click(move |_| cancel.store(true, Ordering::Relaxed));

    // 运行中状态提示（与「停止」按钮可用态一致，运行结束随 ExecDone 消失）
    let running_label = Element::label(lang::SSH_CMD_RUNNING())
        .font_size(11.0)
        .fg_role(Role::Accent)
        .visible_when(move || cmd_running.get());

    // ── 常用命令模板：分组可点击行，点击即填入命令输入框 ──
    // 命令文本即展示标签（命令本身通用，无需本地化）；模板含示例名/路径，
    // 填入后可按需修改再执行。均为非交互式单行命令（exec 无 TTY）。
    let template_groups: [(&str, &[&str]); 9] = [
        (lang::SSH_TPL_SYS(), &[
            "uname -a && cat /etc/os-release",
            "uptime",
            "free -h",
            "df -h",
            "ps aux --sort=-%cpu | head -10",
            "ps aux --sort=-%mem | head -10",
            "du -sh * | sort -rh | head -10",
            "df -i | head -20",
            "sudo du -h --max-depth=1 -x / 2>/dev/null | sort -rh | head -20",
            "vmstat 1 5",
            "timedatectl && date",
        ]),
        (lang::SSH_TPL_SVC(), &[
            "systemctl status nginx",
            "sudo systemctl restart nginx",
            "systemctl is-active nginx",
            "systemctl list-units --type=service --state=running | head -20",
            "journalctl -u nginx -n 100 --no-pager",
            "ss -tlnp",
            "ps -ef | grep java",
            "systemctl --failed",
            "systemctl cat nginx",
            "journalctl -p err -b --no-pager | tail -50",
            "systemctl list-timers --no-pager | head -20",
            "crontab -l",
            "sudo crontab -l",
        ]),
        // 安全增强：SSH 加固 / 防火墙 / fail2ban / 登录审计 / 系统更新 / SUID 排查。
        // 尽量用只读检查；加固类命令按需修改后执行（exec 无 TTY，避免交互式提示）。
        (lang::SSH_TPL_SEC(), &[
            "sudo sshd -T | grep -E 'permitrootlogin|passwordauthentication|port'",
            "sudo sed -i -E 's/^#?(PermitRootLogin|PasswordAuthentication) .*/\\1 no/' /etc/ssh/sshd_config && sudo systemctl restart sshd",
            "sudo last -20 && sudo lastb -20",
            "sudo grep -E 'Failed password|Invalid user' /var/log/auth.log | tail -20",
            "sudo systemctl status fail2ban --no-pager && sudo fail2ban-client status sshd",
            "sudo ufw status verbose && sudo ufw app list",
            "sudo iptables -L -n --line-numbers | head -30",
            "sudo find / -xdev -perm -4000 -type f 2>/dev/null",
            "apt list --upgradable 2>/dev/null | head -20",
            "sudo apt-get update && sudo apt-get upgrade --dry-run",
        ]),
        (lang::SSH_TPL_APP(), &[
            "cd /var/www/app && git pull && sudo systemctl restart app",
            "docker ps",
            "docker ps -a",
            "docker stats --no-stream",
            "docker logs --tail 100 nginx",
            "docker system df",
            "docker images | head -20",
            "docker restart nginx",
            "cd /var/www/app && docker compose up -d --build",
            "pm2 list",
            "pm2 restart app && pm2 logs app --lines 50 --nostream",
            "sudo supervisorctl status",
            "sudo supervisorctl restart app",
        ]),
        // 应用环境部署：网页服务器 / TLS 证书（certbot）/ 常用软件 SDK / 数据库运行环境安装。
        // 均为非交互式命令（exec 无 TTY）；域名、库名等示例值填入后按需修改。
        (lang::SSH_TPL_DEPLOY(), &[
            "sudo apt-get install -y nginx && sudo systemctl enable --now nginx",
            "sudo nginx -t && sudo systemctl reload nginx",
            "sudo apt-get install -y apache2 && sudo systemctl enable --now apache2",
            "sudo apache2ctl configtest && sudo systemctl reload apache2",
            "sudo snap install caddy",
            "sudo ln -sf /etc/nginx/sites-available/example.com /etc/nginx/sites-enabled/example.com && sudo nginx -t && sudo systemctl reload nginx",
            "sudo a2ensite example.com && sudo apache2ctl configtest && sudo systemctl reload apache2",
            "sudo certbot --nginx -d example.com",
            "sudo certbot certonly --standalone -d example.com",
            "sudo certbot certificates",
            "sudo certbot renew --dry-run",
            "echo | openssl s_client -connect example.com:443 -servername example.com 2>/dev/null | openssl x509 -noout -dates -subject",
            "curl -fsSL https://deb.nodesource.com/setup_lts.x | sudo -E bash - && sudo apt-get install -y nodejs",
            "sudo apt-get install -y openjdk-17-jdk maven gradle",
            "sudo apt-get install -y python3-pip python3-venv && pip3 install --upgrade pip",
            "sudo apt-get install -y php-cli php-fpm php-mysql",
            "sudo systemctl enable --now mysql && sudo systemctl restart mysql",
            "mysql -uroot -e 'CREATE DATABASE IF NOT EXISTS appdb DEFAULT CHARACTER SET utf8mb4;'",
            "psql -U postgres -c 'CREATE DATABASE appdb;'",
        ]),
        (lang::SSH_TPL_NET(), &[
            "ip addr show",
            "ping -c 4 baidu.com",
            "curl -sI https://example.com",
            "curl -sI http://127.0.0.1",
            "ss -tlnp | head -20",
            "curl -so /dev/null -w 'HTTP %{http_code} DNS %{time_namelookup}s TTFB %{time_starttransfer}s Total %{time_total}s\\n' https://example.com",
            "getent hosts example.com",
        ]),
        // 日志查看：系统 + 常用网页服务器 / 数据库（路径按发行版与安装方式可调整）
        (lang::SSH_TPL_LOG(), &[
            "ls -lh /var/log | head -20",
            "tail -n 100 /var/log/syslog",
            "tail -n 100 /var/log/messages",
            "tail -n 100 /var/log/nginx/error.log",
            "tail -n 100 /var/log/nginx/access.log",
            "journalctl -b -n 100 --no-pager",
            "dmesg -T | tail -n 30",
            "sudo logrotate -d /etc/logrotate.conf 2>&1 | head -40",
            "sudo tail -n 100 /var/log/apache2/error.log",
            "sudo tail -n 100 /var/log/apache2/access.log",
            "sudo tail -n 100 /var/log/caddy/access.log 2>/dev/null || sudo journalctl -u caddy --no-pager | tail -100",
            "sudo tail -n 100 /var/log/php*-fpm.log",
            "sudo tail -n 100 /var/log/mysql/error.log",
            "sudo journalctl -u mysql --since today --no-pager | tail -100",
            "sudo tail -n 100 /var/log/redis/redis-server.log",
            "sudo tail -n 100 /var/log/postgresql/postgresql-*.log",
            "sudo tail -n 100 /var/log/mongodb/mongod.log",
        ]),
        // 数据库：连接/状态/备份恢复类命令均为非交互单行（带密码的实例在无 TTY 的
        // exec 下会立即报错而非挂起，可按需改成 -p<密码> 或客户端配置文件方式）。
        // 备份/恢复含示例库名与日期占位，填入后按需修改（/backup 目录自动创建）。
        (lang::SSH_TPL_DB(), &[
            "mysql -uroot -e 'SHOW DATABASES;'",
            "mysql -uroot -e 'SHOW PROCESSLIST;'",
            "psql -U postgres -c '\\l'",
            "psql -U postgres -c '\\dt'",
            "redis-cli ping",
            "redis-cli INFO server",
            "sqlite3 /var/www/app/app.db '.tables'",
            "mkdir -p /backup && mysqldump -uroot appdb | gzip > /backup/appdb-$(date +%F).sql.gz",
            "zcat /backup/appdb-2026-09-06.sql.gz | mysql -uroot appdb",
            "mkdir -p /backup && pg_dump -U postgres appdb | gzip > /backup/appdb-$(date +%F).sql.gz",
            "gunzip -c /backup/appdb-2026-09-06.sql.gz | psql -U postgres appdb",
            "mkdir -p /backup && redis-cli --rdb /backup/redis-$(date +%F).rdb",
        ]),
        // 运行时（编程语言）：版本 / 路径 / 进程检查
        (lang::SSH_TPL_RUNTIME(), &[
            "java -version 2>&1",
            "node -v && npm -v",
            "python3 --version && pip3 --version",
            "go version",
            "php -v",
            "ps -ef | grep -E 'java|node|python|php' | grep -v grep",
            "for c in java node python3 go php; do command -v $c 2>/dev/null; done",
        ]),
    ];
    let mut tpl_col = Element::col().width_match().spacing(2);
    for (title, cmds) in template_groups {
        tpl_col = tpl_col.child(
            Element::label(title)
                .font_size(11.0)
                .font_weight(600)
                .fg_role(Role::TextMuted)
                .padding_xy(6, 3),
        );
        for cmd in cmds {
            let cmd = *cmd;
            tpl_col = tpl_col.child(
                Element::row()
                    .width_match()
                    .height(24)
                    .corner(4.0)
                    .cross(Align::Center)
                    .padding_xy(8, 0)
                    .clickable()
                    .on_click(move |_| {
                        // 填入命令输入框（Signal 为 Copy，各闭包持有一份）
                        cmd_input.set(cmd.to_string());
                    })
                    .child(
                        Element::label(cmd)
                            .font_size(12.0)
                            .font_family("Consolas")
                            .fg_role(Role::TextMuted)
                            .max_lines(1)
                            .truncate(Truncate::End),
                    ),
            );
        }
    }

    // ── 模板区标题行：点击展开/收起（▾ 展开态 / ▸ 收起态互斥显示，默认收起）──
    let tpl_header_open = Element::row()
        .width_match()
        .height(26)
        .corner(4.0)
        .cross(Align::Center)
        .spacing(8)
        .clickable()
        .on_click(move |_| show_tpl.set(false))
        .child(
            Element::label(lang::SSH_TPL_TITLE())
                .font_size(12.0)
                .font_weight(600)
                .fg_role(Role::Text)
                .padding_xy(4, 0),
        )
        .child(Element::flex_spacer())
        .child(
            Element::label("▾")
                .font_size(12.0)
                .fg_role(Role::TextMuted)
                .padding_xy(4, 0),
        )
        .visible_when(move || show_tpl.get());

    let tpl_header_closed = Element::row()
        .width_match()
        .height(26)
        .corner(4.0)
        .cross(Align::Center)
        .spacing(8)
        .clickable()
        .on_click(move |_| show_tpl.set(true))
        .child(
            Element::label(lang::SSH_TPL_TITLE())
                .font_size(12.0)
                .font_weight(500)
                .fg_role(Role::TextMuted)
                .padding_xy(4, 0),
        )
        .child(Element::flex_spacer())
        .child(
            Element::label("▸")
                .font_size(12.0)
                .fg_role(Role::TextMuted)
                .padding_xy(4, 0),
        )
        .visible_when(move || !show_tpl.get());

    let tpl_header = Element::stack()
        .width_match()
        .height(26)
        .child(tpl_header_open)
        .child(tpl_header_closed);

    // ──「我的命令」自定义模板区：标题随有无命令显隐；行列表 list_signal 数据驱动，
    // 增删即重建。行 = 可点「填入」区 + ✕ 删除钮（并排不嵌套，避免 Clickable 截断冒泡）；
    // 区尾「＋ 添加命令」打开弹窗。增删都即时持久化到 AppData。──
    let custom_header = Element::label(lang::SSH_TPL_MY())
        .font_size(11.0)
        .font_weight(600)
        .fg_role(Role::TextMuted)
        .padding_xy(6, 3)
        .visible_when(move || !custom_cmds.get().is_empty());
    let custom_rows = Element::list_signal(
        custom_cmds,
        |c: &String| c.clone(), // 命令文本即身份键（添加时已去重，无重复冲突）
        move |c: String| {
            Element::row()
                .width_match()
                .height(24)
                .spacing(2)
                .child(
                    Element::row()
                        .weight(1.0)
                        .height(24)
                        .corner(4.0)
                        .cross(Align::Center)
                        .padding_xy(8, 0)
                        .clickable()
                        .on_click({
                            let c_fill = c.clone();
                            move |_| cmd_input.set(c_fill.clone())
                        })
                        .child(
                            Element::label(&c)
                                .font_size(12.0)
                                .font_family("Consolas")
                                .fg_role(Role::TextMuted)
                                .max_lines(1)
                                .truncate(Truncate::End),
                        ),
                )
                .child(
                    Element::icon_button("\u{2715}")
                        .size(20, 20)
                        .fg_role(Role::TextMuted)
                        .tooltip(lang::SSH_CMD_REMOVE_TIP())
                        .on_click(move |_| {
                            custom_cmds.update(|v| {
                                if let Some(pos) = v.iter().position(|s| s == &c) {
                                    v.remove(pos);
                                }
                            });
                            persist_custom(custom_cmds);
                        }),
                )
        },
    );
    let add_btn = Element::button(lang::SSH_CMD_ADD_BTN())
        .small()
        .outline_soft()
        .neutral()
        .on_click(move |_| {
            cmd_add_input.set(String::new());
            cmd_add_show.set(true);
        });
    let custom_section = Element::col()
        .width_match()
        .spacing(2)
        .child(custom_header)
        .child(custom_rows)
        .child(add_btn);

    // ── 添加自定义命令弹窗：输入命令 → 去重追加 + 持久化 ──
    let add_dialog = input_dialog(
        cmd_add_show,
        lang::SSH_CMD_ADD(),
        460,
        move |_| {
            cmd_add_input.set(String::new());
            cmd_add_show.set(false);
        },
        Element::col()
            .width_match()
            .spacing(8)
            .child(
                Element::label(lang::SSH_CMD_ADD_HINT())
                    .font_size(12.0)
                    .fg_role(Role::TextMuted),
            )
            .child(Element::text_input(cmd_add_input, lang::SFTP_CMD_HINT()).width_match()),
        Element::row()
            .width_match()
            .child(Element::flex_spacer())
            .child(
                Element::button(lang::SFTP_CANCEL())
                    .small()
                    .neutral()
                    .on_click(move |_| {
                        cmd_add_input.set(String::new());
                        cmd_add_show.set(false);
                    }),
            )
            .child(
                Element::button(lang::SFTP_OK())
                    .small()
                    .on_click(move |_| {
                        let c = cmd_add_input.get().trim().to_string();
                        if !c.is_empty() {
                            custom_cmds.update(|v| {
                                if !v.iter().any(|s| s == &c) {
                                    v.push(c.clone());
                                }
                            });
                            persist_custom(custom_cmds);
                        }
                        cmd_add_input.set(String::new());
                        cmd_add_show.set(false);
                    }),
            ),
    );

    let body = Element::col()
        .fill()
        .padding(14)
        .spacing(8)
        .child(
            Element::row()
                .width_match()
                .spacing(6)
                .cross(Align::Center)
                .child(
                    // 多行输入（SyntaxInput，Shell 语法高亮）：长命令 / 粘贴的多行脚本
                    // 可回车换行；执行仍走「执行」按钮。weight(1.0) 占满行内剩余宽度。
                    Element::leaf()
                        .widget(SyntaxInput::new(cmd_input, lang::SFTP_CMD_HINT(), LexerKind::Shell))
                        .font_family("Consolas")
                        .font_size(12.0)
                        .weight(1.0),
                )
                .child(exec_btn)
                .child(stop_btn)
                .child(running_label),
        )
        // 模板区（可折叠）：点标题行展开/收起；收起时不占空间
        .child(
            Element::col()
                .width_match()
                .spacing(4)
                .child(tpl_header)
                .child(
                    Element::scroll()
                        .width_match()
                        .height(170)
                        .corner(8.0)
                        .bg_role(Role::SurfaceAlt)
                        .visible_when(move || show_tpl.get())
                        .child(tpl_col.padding_xy(6, 6))
                        .child(custom_section.padding_xy(6, 6)),
                ),
        )
        // 输出区：占满剩余高度。用只读可选文本（select_text）承载——内建鼠标拖选 +
        // Ctrl+C / 右键「复制」菜单，输出可选中拷贝且不可编辑；此前借 multiline
        // text_input 承载（windui 无只读+可选中纯文本控件），现由 select_text 补齐。
        .child(
            // 水平铺满窗口（此前缺 width_match，内容窄时盒子横向不满）
            select_text(cmd_output)
                .width_match()
                .font_size(12.0)
                .weight(1.0),
        );

    // 弹窗浮层：添加自定义命令（叠在窗口内容之上）
    Element::stack()
        .fill()
        .child(body)
        .child(add_dialog)
}

/// 持久化「我的命令」列表（尽力而为：写失败静默，下次启动以文件为准）
fn persist_custom(cmds: Signal<Vec<String>>) {
    let joined = cmds.get().join("\n");
    crate::core::settings::commit(&[("ssh.custom_cmds", Some(joined.as_str()))]);
}
