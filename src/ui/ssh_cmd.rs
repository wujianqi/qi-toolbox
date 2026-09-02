//! SFTP 的「SSH 命令」独立子窗口 UI：命令输入 + 常用命令模板（可折叠）+ 输出区。
//!
//! 子窗与主窗共享 [`super::sftp::SftpUi`] 的信号句柄（Signal 为 Copy）：SFTP worker
//! 通道注册在 App 级，`ExecDone` 消息写入 `cmd_output`，子窗经信号实时刷新。
//! 独立成文件以控制 [`super::sftp`] 页代码体量，便于按板块扩展模板。

use windui::prelude::*;

use super::{icons, sftp::SftpUi};
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

    // 执行按钮：复用现有 SSH 会话执行命令（需在线且输入非空；窗口可在断连后
    // 仍开着，无门控时 send 会静默失败，故与主页按钮一致检查 connected）
    let exec_btn = Element::button(lang::SFTP_CMD_EXEC())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::TERMINAL, Some(16)))
        .enabled_when(move || connected.get() && !cmd_input.get().trim().is_empty())
        .on_click({
            let cmd = cmd.clone();
            move |_| {
                let command = cmd_input.get().trim().to_string();
                if !command.is_empty() {
                    let _ = cmd.send(sftp::SftpCmd::Exec { command });
                }
            }
        });

    // ── 常用命令模板：分组可点击行，点击即填入命令输入框 ──
    // 命令文本即展示标签（命令本身通用，无需本地化）；模板含示例名/路径，
    // 填入后可按需修改再执行。均为非交互式单行命令（exec 无 TTY）。
    let template_groups: [(&str, &[&str]); 7] = [
        (lang::SSH_TPL_SYS(), &[
            "uname -a && cat /etc/os-release",
            "uptime",
            "free -h",
            "df -h",
            "ps aux --sort=-%cpu | head -10",
            "ps aux --sort=-%mem | head -10",
            "du -sh * | sort -rh | head -10",
        ]),
        (lang::SSH_TPL_SVC(), &[
            "systemctl status nginx",
            "sudo systemctl restart nginx",
            "systemctl is-active nginx",
            "systemctl list-units --type=service --state=running | head -20",
            "journalctl -u nginx -n 100 --no-pager",
            "ss -tlnp",
            "ps -ef | grep java",
        ]),
        (lang::SSH_TPL_APP(), &[
            "cd /var/www/app && git pull && sudo systemctl restart app",
            "docker ps",
            "docker ps -a",
            "docker stats --no-stream",
            "docker logs --tail 100 nginx",
            "docker system df",
        ]),
        (lang::SSH_TPL_NET(), &[
            "ip addr show",
            "ping -c 4 baidu.com",
            "curl -sI https://example.com",
            "curl -sI http://127.0.0.1",
            "ss -tlnp | head -20",
        ]),
        (lang::SSH_TPL_LOG(), &[
            "ls -lh /var/log | head -20",
            "tail -n 100 /var/log/syslog",
            "tail -n 100 /var/log/messages",
            "tail -n 100 /var/log/nginx/error.log",
            "tail -n 100 /var/log/nginx/access.log",
            "journalctl -b -n 100 --no-pager",
            "dmesg -T | tail -n 30",
        ]),
        // 数据库：连接/状态类命令均为非交互单行（带密码的实例在无 TTY 的 exec
        // 下会立即报错而非挂起，可按需改成 -p<密码> 或客户端配置文件方式）
        (lang::SSH_TPL_DB(), &[
            "mysql -uroot -e 'SHOW DATABASES;'",
            "mysql -uroot -e 'SHOW PROCESSLIST;'",
            "psql -U postgres -c '\\l'",
            "psql -U postgres -c '\\dt'",
            "redis-cli ping",
            "redis-cli INFO server",
            "sqlite3 /var/www/app/app.db '.tables'",
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

    Element::col()
        .fill()
        .padding(14)
        .spacing(8)
        .child(
            Element::row()
                .width_match()
                .spacing(6)
                .cross(Align::Center)
                .child(
                    Element::text_input(cmd_input, lang::SFTP_CMD_HINT())
                        .width_match()
                        .weight(1.0),
                )
                .child(exec_btn),
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
                        .child(tpl_col.padding_xy(6, 6)),
                ),
        )
        // 输出区：占满剩余高度。用 multiline text_input 承载（非 label）——
        // text_input 内建鼠标拖选 + Ctrl+C / 右键「复制」菜单，输出可选中拷贝；
        // windui 没有只读+可选中的纯文本控件，此为可复制文本的标准做法
        // （每条命令执行后整段覆盖，误编辑无副作用）。
        .child(
            Element::text_input(cmd_output, "")
                .multiline()
                .wrap(true)
                .font_size(12.0)
                .corner(8.0)
                .weight(1.0),
        )
}
