//! 主口令门控：启动时首次强制设置 / 换环境强制解锁的模态弹窗
//!
//! 决策在 [`MasterUi::init`]（AppState::new 内、settings::load 之前）：
//! - 未设置主口令 → 设置模式（两次输入 + 遗忘警告；弹窗不可关闭）
//! - 已设置且本机解锁器有效（`try_silent_unlock`）→ 不弹，同环境免输
//! - 已设置但静默解锁失败（换机/换用户/恢复备份）→ 解锁模式（提示输原口令）

use windui::prelude::*;

use crate::lang;

/// 主口令门控状态
#[derive(Clone)]
pub struct MasterGate {
    /// 弹窗显示中
    pub show: Signal<bool>,
    /// 0 = 首次设置（两个输入框）1 = 解锁（单个输入框）
    pub mode: Signal<usize>,
    pub pass: Signal<String>,
    pub confirm: Signal<String>,
    /// 就地错误提示（口令过短/不一致/口令错误）
    pub error: Signal<String>,
}

impl MasterGate {
    /// 启动决策：静默解锁成功则不弹；否则按「未设置/需解锁」弹对应模式。
    /// 必须在 settings::load 之前调用（解锁后敏感记忆项才能正常解密回填）。
    pub fn init() -> Self {
        let gate = Self {
            show: signal(false),
            mode: signal(0usize),
            pass: signal(String::new()),
            confirm: signal(String::new()),
            error: signal(String::new()),
        };
        if crate::core::master::is_set() {
            if !crate::core::master::try_silent_unlock() {
                gate.mode.set(1);
                gate.show.set(true);
            }
        } else {
            gate.mode.set(0);
            gate.show.set(true);
        }
        gate
    }

    fn confirm_pass(&self) {
        self.error.set(String::new());
        let pass = self.pass.get();
        if self.mode.get() == 0 {
            // 设置模式：两次一致 + 长度校验
            if self.confirm.get() != pass {
                self.error.set(lang::MASTER_PASS_MISMATCH());
                return;
            }
            if let Err(e) = crate::core::master::setup(&pass) {
                self.error.set(e);
                return;
            }
        } else if let Err(e) = crate::core::master::unlock(&pass) {
            self.error.set(e);
            return;
        }
        self.show.set(false);
        self.pass.set(String::new());
        self.confirm.set(String::new());
    }
}

/// 构建主口令门控浮层：主界面之上的自绘半透明遮罩 + 居中面板（不走
/// Element::dialog——那会把弹窗登记成模态，窗体 ✕ 被框架吞掉无法退出应用，
/// 见 layout::build_ui）。设置模式不可关闭：只能输入口令完成。
pub fn build_gate(gate: &MasterGate) -> Element {
    let body = Element::col()
        .width_match()
        .spacing(10)
        // 设置模式显示遗忘警告；解锁模式显示「来自其它环境」提示
        .child(
            Element::label_signal(gate.mode.map(|m| {
                if *m == 0 {
                    lang::MASTER_SETUP_WARN()
                } else {
                    lang::MASTER_UNLOCK_HINT()
                }
            }))
            .font_size(13.0)
            .fg_role(Role::Danger),
        )
        .child(
            Element::text_input(gate.pass, lang::MASTER_PASS_LABEL())
                .password()
                .autofocus()
                .width_match(),
        )
        // 确认框仅设置模式显示（模式启动时确定，构建期条件生成即可）
        .child(if gate.mode.get() == 0 {
            Element::text_input(gate.confirm, lang::MASTER_PASS_CONFIRM())
                .password()
                .width_match()
        } else {
            Element::col()
        })
        .child(
            Element::label_signal(gate.error)
                .font_size(12.0)
                .fg_role(Role::Danger),
        );

    // 设置/解锁模式均无关闭路径：只能输入口令完成；点窗体 ✕ 直接退出应用
    // 模式在启动时确定后不再变更，标题构建期取值即可（标题随构建期语言刷新）
    let title = if gate.mode.get() == 0 {
        lang::MASTER_SETUP_TITLE()
    } else {
        lang::MASTER_UNLOCK_TITLE()
    };
    // 门控为自绘遮罩浮层：半透明黑底铺满整窗、吞掉指针事件挡住底下界面的交互
    // （visible_when 由调用方挂接 show 信号），背后主界面照常渲染可见
    Element::col()
        .fill()
        .bg(Color::rgba(0, 0, 0, 120))
        .cross(Align::Center)
        .child(Element::flex_spacer())
        .child(
            Element::col()
                .width(620)
                .bg_role(Role::Surface)
                .corner(12.0)
                .padding(24)
                .spacing(16)
                .child(
                    Element::label(title)
                        .font_size(18.0)
                        .font_weight(700)
                        .fg_role(Role::Text),
                )
                .child(body)
                .child(
                    Element::row()
                        .width_match()
                        .child(Element::flex_spacer())
                        .child(
                            Element::button(lang::MASTER_BTN_OK())
                                .small()
                                .on_click({
                                    let gate = gate.clone();
                                    move |_| gate.confirm_pass()
                                }),
                        ),
                ),
        )
        .child(Element::flex_spacer())
}
