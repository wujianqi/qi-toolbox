#!/usr/bin/env python3
"""静态检查：ui 的 build_* 构建期函数内禁止新建 signal()。

windui 整树重建（theme_epoch host_signal）时，build_* 内 signal() 新建的
信号会被 dispose 成死句柄（panic signal.rs:545）。凡跨重建存活的状态必须
放 XxxUi::new() 启动期信号或 thread_local 池。
详见 ui/mod.rs / nav.rs / table.rs / sql.rs / sftp.rs 先例。
排查基准模式：grep `let .* = signal\\(`。

用法：python3 scripts/check_signals.py [src 目录，默认 src]
发现违规则打印位置并以非 0 退出；通过则静默。
"""
import re
import sys
from pathlib import Path

# build_* 函数定义行（含 pub fn）
BUILD_FN = re.compile(r"^\s*(?:pub(?:\(crate\))?\s+)?fn\s+build_\w+")
# 信号新建调用：signal(（Element 的 *_signal( 构造器不含裸 signal 词元，天然排除）
SIGNAL_NEW = re.compile(r"(?<![_\w])signal\s*\(")


def check_file(path: Path) -> list[str]:
    hits: list[str] = []
    depth = 0
    in_build = False
    for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        code = line.split("//", 1)[0]  # 去行注释
        if in_build:
            depth += code.count("{") - code.count("}")
            if depth <= 0:
                in_build = False
                continue
            if SIGNAL_NEW.search(code):
                hits.append(f"{path}:{lineno}: {line.strip()}")
        elif BUILD_FN.match(code):
            in_build = True
            depth = code.count("{") - code.count("}")
            # 单行定义 `{}` 同行闭合的情形
            if depth <= 0:
                in_build = False
    return hits


def main() -> int:
    root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("src")
    ui = root / "ui"
    files = sorted(ui.glob("*.rs")) if ui.is_dir() else sorted(root.rglob("*.rs"))
    bad: list[str] = []
    for f in files:
        bad.extend(check_file(f))
    if bad:
        print("build_* 内新建 signal()（整树重建会变成死句柄，需移入 XxxUi::new() 或 thread_local 池）：")
        print("\n".join(bad))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
