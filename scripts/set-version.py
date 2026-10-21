#!/usr/bin/env python3
"""把 Cargo.toml 和 Cargo.lock 里的版本号一起改成指定值。

用法: python3 scripts/set-version.py <新版本号>
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TOML = ROOT / "Cargo.toml"
LOCK = ROOT / "Cargo.lock"


def set_toml(version: str) -> None:
    text = TOML.read_text(encoding="utf-8")
    new, count = re.subn(
        r'^version = "[^"]*"',
        f'version = "{version}"',
        text,
        count=1,
        flags=re.M,
    )
    if count != 1:
        raise SystemExit(f"Cargo.toml 里找不到唯一的 version 字段")
    TOML.write_text(new, encoding="utf-8")


def set_lock(version: str) -> None:
    if not LOCK.exists():
        return
    text = LOCK.read_text(encoding="utf-8")
    new, count = re.subn(
        r'(\[\[package\]\]\nname = "music-shell"\nversion = ")[^"]+(")',
        lambda m: m.group(1) + version + m.group(2),
        text,
    )
    if count != 1:
        raise SystemExit("Cargo.lock 里找不到 music-shell 包的版本字段")
    LOCK.write_text(new, encoding="utf-8")


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("用法: set-version.py <新版本号>")
    version = sys.argv[1].strip()
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise SystemExit(f"版本号格式不对: {version}（应该是 x.y.z）")
    set_toml(version)
    set_lock(version)
    print(f"版本号已设为 {version}")


if __name__ == "__main__":
    main()
