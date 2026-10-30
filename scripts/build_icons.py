#!/usr/bin/env python3
"""把一个目录里的头像图片转换后写入元数据包（T-60）。

图片按物品命名：文件名（不含扩展名）是物品 ID 或任一语言的名称，例如 `10000002.png`、
`神里绫华.webp`。每张图缩放到 64×64（保持比例，透明底居中），转成 WebP，按内容的 SHA-256
存为 `metadata/icons/<game>/<sha256>.webp`，并把哈希写到游戏文件对应物品的 `icon` 字段。
图片来源与授权由维护者自行确认；本脚本不联网。

用法：
    python scripts/build_icons.py <game> <图片目录>   如 python scripts/build_icons.py genshin ~/icons/genshin
    python scripts/build_icons.py <game> --prune       只删除该游戏没有物品引用的头像
    python scripts/check_metadata.py --fix             然后更新 manifest 中游戏文件的 sha256，并升级 manifest 版本

需要 Pillow（pip install pillow）。
"""
import argparse
import hashlib
import io
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / "metadata"
SIZE = 64
EXTS = {".png", ".jpg", ".jpeg", ".webp", ".avif", ".bmp", ".gif"}


def convert(path: Path) -> bytes:
    from PIL import Image

    with Image.open(path) as img:
        img = img.convert("RGBA")
        img.thumbnail((SIZE, SIZE), Image.LANCZOS)
        canvas = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
        canvas.paste(img, ((SIZE - img.width) // 2, (SIZE - img.height) // 2), img)
        out = io.BytesIO()
        canvas.save(out, "WEBP", quality=85, method=6)
        return out.getvalue()


def prune(game: str, pack: dict) -> int:
    used = {i["icon"] for i in pack["items"] if i.get("icon")}
    removed = 0
    for f in (ROOT / "icons" / game).glob("*.webp"):
        if f.stem not in used:
            f.unlink()
            removed += 1
    return removed


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("game")
    ap.add_argument("src", nargs="?", type=Path)
    ap.add_argument("--prune", action="store_true")
    args = ap.parse_args()

    pack_path = ROOT / f"{args.game}.json"
    if not pack_path.is_file():
        print(f"找不到 {pack_path}", file=sys.stderr)
        return 1
    pack = json.loads(pack_path.read_text(encoding="utf-8"))

    if args.src:
        lookup = {}
        for item in pack["items"]:
            for key in [item["id"], *item.get("names", {}).values()]:
                lookup.setdefault(key, item)
        out_dir = ROOT / "icons" / args.game
        out_dir.mkdir(parents=True, exist_ok=True)
        done, unmatched = 0, []
        for f in sorted(args.src.iterdir()):
            if f.suffix.lower() not in EXTS:
                continue
            item = lookup.get(f.stem)
            if item is None:
                unmatched.append(f.name)
                continue
            data = convert(f)
            sha = hashlib.sha256(data).hexdigest()
            (out_dir / f"{sha}.webp").write_bytes(data)
            item["icon"] = sha
            done += 1
        pack_path.write_text(json.dumps(pack, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
        print(f"{args.game}: 写入 {done} 个头像")
        if unmatched:
            print(f"找不到对应物品，已跳过：{', '.join(unmatched)}", file=sys.stderr)

    if args.src or args.prune:
        removed = prune(args.game, pack)
        if removed:
            print(f"{args.game}: 删除 {removed} 个不再引用的头像")
    return 0


if __name__ == "__main__":
    sys.exit(main())
