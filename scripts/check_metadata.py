#!/usr/bin/env python3
"""校验卡池元数据包 metadata/。

manifest.json 结构（见架构文档 2 节与 SRS）：
    {"version": "...", "minAppVersion": "...",
     "files": [{"path": "genshin.json", "sha256": "<hex>"}, ...]}

用法：
    python scripts/check_metadata.py           校验所有文件存在、可解析、SHA-256 与 manifest 一致，
                                               物品头像 icons/<game>/<sha256>.webp 存在且内容一致
    python scripts/check_metadata.py --fix     按当前文件内容重写 manifest 中的 sha256
    python scripts/check_metadata.py --schema  额外用 metadata/schema/ 下的 JSON Schema 校验（需 check-jsonschema）：
                                               manifest.json → manifest.schema.json，各游戏文件 → game.schema.json
"""
import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / "metadata"
MANIFEST = ROOT / "manifest.json"


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--fix", action="store_true")
    ap.add_argument("--schema", action="store_true")
    args = ap.parse_args()

    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    errors = []
    for key in ("version", "files"):
        if key not in manifest:
            errors.append(f"manifest.json 缺少字段 {key}")
    files = manifest.get("files", [])

    for entry in files:
        rel = entry.get("path", "")
        path = ROOT / rel
        if not rel or not path.is_file():
            errors.append(f"{rel or '<空路径>'}: 文件不存在")
            continue
        if path.suffix == ".json":
            try:
                json.loads(path.read_text(encoding="utf-8"))
            except json.JSONDecodeError as e:
                errors.append(f"{rel}: JSON 解析失败 ({e})")
                continue
        actual = sha256(path)
        if args.fix:
            entry["sha256"] = actual
        elif entry.get("sha256") != actual:
            errors.append(f"{rel}: sha256 不一致，请运行 python scripts/check_metadata.py --fix")

    # 头像不登记在 manifest 里，由游戏文件中物品的 icon（SHA-256）校验：文件要存在且内容一致
    referenced = set()
    for entry in files:
        path = ROOT / entry.get("path", "")
        if path.suffix != ".json" or not path.is_file():
            continue
        try:
            pack = json.loads(path.read_text(encoding="utf-8"))
        except json.JSONDecodeError:
            continue
        game = pack.get("game", "")
        for item in pack.get("items", []):
            icon = item.get("icon")
            if not icon:
                continue
            rel = f"icons/{game}/{icon}.webp"
            referenced.add(rel)
            icon_path = ROOT / rel
            if not icon_path.is_file():
                errors.append(f"{entry['path']}: 物品 {item.get('id')} 的头像 {rel} 不存在")
            elif sha256(icon_path) != icon:
                errors.append(f"{rel}: 内容与文件名中的 sha256 不一致")
    for path in sorted(ROOT.glob("icons/*/*")):
        rel = path.relative_to(ROOT).as_posix()
        if rel not in referenced:
            errors.append(f"{rel}: 没有物品引用这个头像，请删除或运行 scripts/build_icons.py")

    listed = {e.get("path") for e in files}
    for path in ROOT.glob("*.json"):
        if path.name != "manifest.json" and path.name not in listed:
            errors.append(f"{path.name}: 未登记到 manifest.json")

    if args.schema:
        # manifest.json 用 manifest.schema.json，各游戏文件用 game.schema.json
        games = [ROOT / e["path"] for e in files if (ROOT / e.get("path", "")).is_file()]
        for schema, targets in (("manifest.schema.json", [MANIFEST]), ("game.schema.json", games)):
            schema_path = ROOT / "schema" / schema
            if not schema_path.is_file() or not targets:
                continue
            r = subprocess.run(["check-jsonschema", "--schemafile", str(schema_path), *map(str, targets)])
            if r.returncode != 0:
                errors.append(f"有文件不符合 {schema}")

    if args.fix and not errors:
        MANIFEST.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print("manifest.json 已更新")

    for e in errors:
        print(f"::error::{e}")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
