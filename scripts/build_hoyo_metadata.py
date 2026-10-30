#!/usr/bin/env python3
"""从 HoYo.Gacha 的 GachaMetadata.json 生成米哈游三款的元数据包（metadata/{genshin,starrail,zzz}.json）。

数据来源：https://github.com/lgou2w/HoYo.Gacha （MIT OR Apache-2.0），见 metadata/NOTICE.md。
鸣潮、明日方舟、终末地的包手工维护，不由本脚本生成。

用法：
    python scripts/build_hoyo_metadata.py [GachaMetadata.json]   不给路径时从 GitHub 下载
    python scripts/check_metadata.py --fix                       然后更新 manifest 中的 sha256

同一卡池类型、同一时段的多个卡池（原神 301 / 400、崩铁多个角色池）合并为一期，UP 名单取并集：
一次抽卡只可能出自己卡池的 UP 或常驻，所以并集不会误判。
"""
import json
import sys
import urllib.request
from pathlib import Path

SOURCE = "https://raw.githubusercontent.com/lgou2w/HoYo.Gacha/main/tauri/src/assets/GachaMetadata.json"
OUT = Path(__file__).resolve().parent.parent / "metadata"
LANGS = {"zh-cn": "zh-cn", "zh-tw": "zh-tw", "en-us": "en-us", "ja-jp": "ja-jp"}

# Business -> (游戏 ID, 官方卡池类型 -> 本软件卡池类型, 常驻最高星的中文名)
GAMES = {
    0: (
        "genshin",
        {301: "character", 400: "character", 302: "weapon", 500: "chronicled"},
        ["琴", "迪卢克", "七七", "莫娜", "刻晴", "提纳里", "迪希雅", "梦见月瑞希",
         "风鹰剑", "天空之刃", "狼的末路", "天空之傲", "和璞鸢", "天空之脊",
         "四风原典", "天空之卷", "阿莫斯之弓", "天空之翼"],
    ),
    1: (
        "starrail",
        {11: "character", 12: "light_cone", 21: "collab_character", 22: "collab_light_cone"},
        ["姬子", "瓦尔特", "布洛妮娅", "杰帕德", "克拉拉", "彦卿", "白露",
         "银河铁道之夜", "无可取代的东西", "但战斗还未结束", "以世界之名",
         "制胜的瞬间", "如泥酣眠", "时节不居"],
    ),
    2: (
        "zzz",
        {2: "character", 3: "w_engine", 5: "bangboo",
         102: "exclusive_rescreening", 103: "w_engine_reverberation"},
        ["猫又", "「11号」", "珂蕾妲", "莱卡恩", "格莉丝", "丽娜", "佩洛伊斯",
         "钢铁肉垫", "硫磺石", "燃狱齿轮", "拘缚者", "嵌合编译器", "啜泣摇篮", "日冕遗蜕"],
    ),
}


def load(argv):
    if len(argv) > 1:
        return json.loads(Path(argv[1]).read_text(encoding="utf-8"))
    with urllib.request.urlopen(SOURCE) as r:
        return json.loads(r.read().decode("utf-8"))


def build(business):
    game, pool_map, standard_names = GAMES[business["Business"]]
    items, by_zh = [], {}
    for cat in business["Categories"]:
        i18n = cat["I18n"]
        for n, (item_id, rarity) in enumerate(cat["Entries"]):
            names = {LANGS[k]: v["Entries"][n] for k, v in i18n.items() if k in LANGS}
            items.append({"id": str(item_id), "rarity": rarity, "names": names})
            by_zh.setdefault(names.get("zh-cn"), str(item_id))
    missing = [n for n in standard_names if n not in by_zh]
    if missing:
        raise SystemExit(f"{game}: 常驻名单中找不到 {missing}")
    standard = [by_zh[n] for n in standard_names]

    merged = {}
    for b in business["Banners"]:
        pool = pool_map.get(b["GachaType"])
        if pool is None or not b["UpGolden"]:
            continue
        key = (pool, b["StartTime"], b["EndTime"])
        m = merged.setdefault(key, {"gacha_ids": [], "up_top": [], "up_second": [], "version": b.get("Version")})
        if b.get("GachaId"):
            m["gacha_ids"].append(str(b["GachaId"]))
        for src, dst in (("UpGolden", "up_top"), ("UpPurple", "up_second")):
            for i in b[src]:
                if str(i) not in m[dst]:
                    m[dst].append(str(i))

    banners, seen = [], set()
    for (pool, start, end), m in sorted(merged.items(), key=lambda kv: (kv[0][1], kv[0][0])):
        bid = m["gacha_ids"][0] if m["gacha_ids"] else f"{game}-{pool}-{start[:10]}"
        n = 2
        while bid in seen:
            bid = f"{bid.rsplit('#', 1)[0]}#{n}"
            n += 1
        seen.add(bid)
        banners.append({
            "id": bid, "pool_type": pool, "version": m["version"],
            "start": start, "end": end, "up_top": m["up_top"], "up_second": m["up_second"],
        })

    # 同一卡池类型的时段不应部分重叠，否则按时间查找卡池会选错
    by_pool = {}
    for b in banners:
        by_pool.setdefault(b["pool_type"], []).append(b)
    for pool, bs in by_pool.items():
        for a, b in zip(bs, bs[1:]):
            if b["start"] < a["end"]:
                print(f"warning: {game} {pool} {a['id']} 与 {b['id']} 时段重叠", file=sys.stderr)

    return game, {"game": game, "items": items, "standard": standard, "banners": banners}


def main():
    data = load(sys.argv)
    for business in data:
        if business["Business"] not in GAMES:
            continue
        game, pack = build(business)
        path = OUT / f"{game}.json"
        # 头像由 scripts/build_icons.py 维护，重新生成时保留已有的 icon 字段
        if path.is_file():
            icons = {i["id"]: i["icon"] for i in json.loads(path.read_text(encoding="utf-8"))["items"] if i.get("icon")}
            for item in pack["items"]:
                if item["id"] in icons:
                    item["icon"] = icons[item["id"]]
        path.write_text(json.dumps(pack, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
        print(f"{path.name}: {len(pack['items'])} items, {len(pack['banners'])} banners")


if __name__ == "__main__":
    main()
