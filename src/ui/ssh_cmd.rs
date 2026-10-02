//! SFTP 的「SSH 命令」独立子窗口 UI：命令输入 + 常用命令模板（可折叠）+ 输出区。
//!
//! 子窗与主窗共享 [`super::sftp::SftpUi`] 的信号句柄（Signal 为 Copy）：SFTP worker
//! 通道注册在 App 级，`ExecDone` 消息写入 `cmd_output`，子窗经信号实时刷新。
//! 独立成文件以控制 [`super::sftp`] 页代码体量，便于按板块扩展模板。

use std::sync::atomic::Ordering;

use windui::prelude::*;

use super::{icons, input_dialog, select_text, sftp::SftpUi};
use crate::core::sftp;
use crate::lang;
use crate::widgets::syntax_input::{LexerKind, SyntaxInput};

/// 「SSH 命令」子窗口内容（独立窗口：主窗工具栏「命令」按钮经
/// `EventCtx::open_window` 打开，见 [`super::sftp::build_sftp_tab`]）。
/// 输出区用 multiline `text_input` 承载：文本可拖选、Ctrl+C / 右键复制。
pub(super) fn cmd_window(ui: &SftpUi) -> Element {
    let cmd = ui.cmd();
    let cmd_input = ui.cmd_input;
    let cmd_output = ui.cmd_output;
    let connected = ui.connected;
    let cmd_running = ui.cmd_running;
    let cancel = ui.cancel();
    let custom_cmds = ui.custom_cmds;
    let cmd_add_show = ui.cmd_add_show;
    let cmd_add_input = ui.cmd_add_input;

    // 首次使用播种内置命令（表内已有分组命令则跳过），随后全部从库读取
    let _ = crate::core::store::ssh_seed_if_empty(&builtin_seed_rows());
    let all_cmds = crate::core::store::ssh_list().unwrap_or_default();

    // 按分组聚合（保库内顺序）；category 空串 = 「我的命令」。
    // 注意：构建期禁止写信号（windui 会 panic），custom_cmds 已在 SftpUi::new
    // 时从库加载，这里只读取聚合，不做 set 回填。
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for c in &all_cmds {
        if c.category.is_empty() {
            continue; // 「我的命令」由 custom_cmds 信号承载（list_signal 数据驱动）
        } else if let Some(g) = groups.iter_mut().find(|(t, _)| *t == c.category) {
            g.1.push(c.command.clone());
        } else {
            groups.push((c.category.clone(), vec![c.command.clone()]));
        }
    }

    // 执行按钮：复用现有 SSH 会话执行命令（需在线、输入非空、且当前无命令在跑；
    // 窗口可在断连后仍开着，无门控时 send 会静默失败，故与主页按钮一致检查 connected）
    let exec_btn = Element::button(lang::SFTP_CMD_EXEC())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::TERMINAL))
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
    // 命令存于 store.db 的 ssh_cmds 表（首次启动播种内置命令，之后以库为准可增删）。
    let template_groups: Vec<(String, Vec<String>)> = groups;
    // ── 常用命令选择器：分组下拉 + 搜索过滤，点行即填入命令输入框 ──
    // 分组下拉（0=全部，末项=「我的命令」）；搜索关键字非空时跨全部组过滤（忽略
    // 分组选择）。行显隐用 visible_when 逐帧求值：百余个轻量闭包调用开销可忽略，
    // 避免为跨信号过滤引入派生信号（map）的版本失效坑。
    let tpl_search = ui.tpl_search;
    let tpl_group = ui.tpl_group;
    let mut group_names: Vec<String> = vec![lang::SSH_TPL_ALL().to_string()];
    group_names.extend(template_groups.iter().map(|(t, _)| t.clone()));
    let my_group = group_names.len(); // 「我的命令」= 末项索引
    group_names.push(lang::SSH_TPL_MY().to_string());

    let mut tpl_col = Element::col().width_match().spacing(2);
    for (gi, (title, cmds)) in template_groups.iter().enumerate() {
        let gi = gi + 1; // 0 已被「全部」占用
        let title = title.clone();
        let cmds = cmds.clone();
        // 组标题：未搜索时随分组选择显隐；搜索时该组有匹配才显示
        let header_cmds = cmds.clone();
        let header_vis = move || {
            let q = tpl_search.get();
            if q.is_empty() {
                let g = tpl_group.get();
                g == 0 || g == gi
            } else {
                header_cmds
                    .iter()
                    .any(|c| c.to_lowercase().contains(&q.to_lowercase()))
            }
        };
        tpl_col = tpl_col.child(
            Element::label(title)
                .font_size(11.0)
                .font_weight(600)
                .fg_role(Role::TextMuted)
                .padding_xy(6, 3)
                .visible_when(header_vis),
        );
        for cmd in cmds {
            let show_cmd = cmd.clone();
            let click_cmd = cmd.clone();
            let show = move || {
                let q = tpl_search.get();
                if q.is_empty() {
                    let g = tpl_group.get();
                    g == 0 || g == gi
                } else {
                    show_cmd.to_lowercase().contains(&q.to_lowercase())
                }
            };
            tpl_col = tpl_col.child(
                Element::row()
                    .width_match()
                    .height(24)
                    .corner(4.0)
                    .cross(Align::Center)
                    .padding_xy(8, 0)
                    .clickable()
                    .visible_when(show)
                    .on_click(move |_| {
                        // 填入命令输入框（Signal 为 Copy，各闭包持有一份）
                        cmd_input.set(click_cmd.clone());
                    })
                    .child(
                        Element::label(cmd.clone())
                            .font_size(12.0)
                            .font_family("Consolas")
                            .fg_role(Role::TextMuted)
                            .max_lines(1)
                            .truncate(Truncate::End),
                    ),
            );
        }
    }

    // 无匹配占位（仅在搜索无结果时出现）
    let groups_for_empty = template_groups.to_vec();
    let tpl_empty = Element::label(lang::SSH_TPL_EMPTY())
        .font_size(12.0)
        .fg_role(Role::TextMuted)
        .padding_xy(6, 8)
        .visible_when(move || {
            let q = tpl_search.get();
            if q.is_empty() {
                return false;
            }
            let ql = q.to_lowercase();
            let mut any = groups_for_empty
                .iter()
                .any(|(_, cmds)| cmds.iter().any(|c| c.to_lowercase().contains(&ql)));
            if !any {
                any = custom_cmds
                    .get()
                    .iter()
                    .any(|c| c.to_lowercase().contains(&ql));
            }
            !any
        });

    // ── 模板区工具行：分组下拉 + 搜索框（搜索非空时忽略分组选择，跨组过滤）──
    let tpl_toolbar = Element::row()
        .width_match()
        .spacing(6)
        .cross(Align::Center)
        .child(
            Element::label(lang::SSH_TPL_GROUP())
                .font_size(12.0)
                .fg_role(Role::TextMuted),
        )
        .child(Element::dropdown(group_names, tpl_group).width(150))
        .child(
            Element::text_input(tpl_search, lang::SSH_TPL_SEARCH())
                .font_size(12.0)
                .weight(1.0),
        );

    // ──「我的命令」自定义模板区：随分组选择/搜索显隐；行列表 list_signal 数据驱动，
    // 增删即重建。行 = 可点「填入」区 + ✕ 删除钮（并排不嵌套，避免 Clickable 截断冒泡）；
    // 区尾「＋ 添加命令」打开弹窗。增删都即时持久化到 AppData。──
    let tpl_search_my = tpl_search;
    let tpl_group_my = tpl_group;
    let my_vis = move || {
        let q = tpl_search_my.get();
        if q.is_empty() {
            let g = tpl_group_my.get();
            g == 0 || g == my_group
        } else {
            custom_cmds
                .get()
                .iter()
                .any(|c| c.to_lowercase().contains(&q.to_lowercase()))
        }
    };
    let custom_header = Element::label(lang::SSH_TPL_MY())
        .font_size(11.0)
        .font_weight(600)
        .fg_role(Role::TextMuted)
        .padding_xy(6, 3)
        .visible_when(move || my_vis() && !custom_cmds.get().is_empty());
    let custom_rows = Element::list_signal(
        custom_cmds,
        |c: &String| c.clone(), // 命令文本即身份键（添加时已去重，无重复冲突）
        move |c: String| {
            // 行随搜索关键字过滤（分组下拉下本组本来只显「我的命令」，无需再判）。
            // c 被多个闭包独占持有，各给一份克隆。
            let c_vis = c.clone();
            let c_fill = c.clone();
            let c_del = c.clone();
            Element::row()
                .width_match()
                .height(24)
                .spacing(2)
                .visible_when(move || {
                    let q = tpl_search.get();
                    q.is_empty() || c_vis.to_lowercase().contains(&q.to_lowercase())
                })
                .child(
                    Element::row()
                        .weight(1.0)
                        .height(24)
                        .corner(4.0)
                        .cross(Align::Center)
                        .padding_xy(8, 0)
                        .clickable()
                        .on_click(move |_| cmd_input.set(c_fill.clone()))
                        .child(
                            Element::label(&c_del)
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
                            let _ = crate::core::store::ssh_del_custom(&c_del);
                            custom_cmds.update(|v| {
                                if let Some(pos) = v.iter().position(|s| s == &c_del) {
                                    v.remove(pos);
                                }
                            });
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
        620,
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
            .child(
                Element::text_input(cmd_add_input, lang::SFTP_CMD_HINT())
                    .autofocus()
                    .width_match(),
            ),
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
            .child(Element::button(lang::SFTP_OK()).small().on_click(move |_| {
                let c = cmd_add_input.get().trim().to_string();
                if !c.is_empty()
                    && crate::core::store::ssh_add_custom(&c).is_ok()
                    && !custom_cmds.get().iter().any(|s| s == &c)
                {
                    custom_cmds.update(|v| v.push(c.clone()));
                }
                cmd_add_input.set(String::new());
                cmd_add_show.set(false);
            })),
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
                    // 可回车换行；执行仍走「执行」按钮。weight(1.0) 占满行内剩余宽度，
                    // 高度固定 84（约 3 行）：不固定会被 measure 的"填满分配区"视口
                    // 模型撑满整窗，把下方模板区/输出区挤出视口。
                    Element::leaf()
                        .widget(SyntaxInput::new(
                            cmd_input,
                            lang::SFTP_CMD_HINT(),
                            LexerKind::Shell,
                        ))
                        .font_family("Consolas")
                        .font_size(12.0)
                        .weight(1.0)
                        .height(84),
                )
                .child(exec_btn)
                .child(stop_btn)
                .child(running_label),
        )
        // 模板选择器：分组下拉 + 搜索过滤（点命令行即填入输入框）
        .child(
            Element::col()
                .width_match()
                .spacing(4)
                .child(tpl_toolbar)
                .child(
                    Element::scroll()
                        .width_match()
                        .height(190)
                        .corner(8.0)
                        .bg_role(Role::SurfaceAlt)
                        .child(tpl_col.padding_xy(6, 6))
                        .child(tpl_empty)
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
    Element::stack().fill().child(body).child(add_dialog)
}

/// 内置常用命令播种数据：首次启动写入 ssh_cmds 表（分组标题沿用 i18n 文案，
/// 存库后以库为准，i18n 只影响播种值）。
fn builtin_seed_rows() -> Vec<(String, String)> {
    let cats: [(&str, &[&str]); 10] = [
        (&lang::SSH_TPL_SYS(), &[
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
        (&lang::SSH_TPL_SVC(), &[
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
        (&lang::SSH_TPL_SEC(), &[
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
        (&lang::SSH_TPL_APP(), &[
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
        (&lang::SSH_TPL_DEPLOY(), &[
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
        (&lang::SSH_TPL_NET(), &[
            "ip addr show",
            "ping -c 4 baidu.com",
            "curl -sI https://example.com",
            "curl -sI http://127.0.0.1",
            "ss -tlnp | head -20",
            "curl -so /dev/null -w 'HTTP %{http_code} DNS %{time_namelookup}s TTFB %{time_starttransfer}s Total %{time_total}s\\n' https://example.com",
            "getent hosts example.com",
        ]),
        (&lang::SSH_TPL_LOG(), &[
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
        (&lang::SSH_TPL_DB(), &[
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
        (&lang::SSH_TPL_ARCHIVE(), &[
            "tar -czvf archive-$(date +%F).tar.gz /path/to/dir",
            "tar -xzvf archive.tar.gz -C /path/to/dest",
            "tar -czvf logs-$(date +%F).tar.gz /var/log/nginx",
            "tar -tzf archive.tar.gz | head -20",
            "zip -r archive.zip /path/to/dir",
            "unzip archive.zip -d /path/to/dest",
            "unzip -l archive.zip | head -20",
            "gzip -k file.txt",
            "gunzip -k file.txt.gz",
            "xz -z -k file.txt",
            "xz -d -k file.txt.xz",
            "tar -cJvf archive.tar.xz /path/to/dir",
            "tar -xJvf archive.tar.xz -C /path/to/dest",
            "zcat access.log.gz | head -20",
        ]),
        (&lang::SSH_TPL_RUNTIME(), &[
            "java -version 2>&1",
            "node -v && npm -v",
            "python3 --version && pip3 --version",
            "go version",
            "php -v",
            "ps -ef | grep -E 'java|node|python|php' | grep -v grep",
            "for c in java node python3 go php; do command -v $c 2>/dev/null; done",
        ]),
    ];
    cats.iter()
        .flat_map(|(cat, cmds)| {
            cmds.iter()
                .map(move |c| ((*cat).to_string(), (*c).to_string()))
        })
        .collect()
}
