"""Held-out variant of the golden world (`gen.py --variant holdout --seed N`).

The held-out set keeps the formats, the block structure and the label schema of the golden set
but is a different fictional company: every person, organisation, project, topic, term, event
and place gets a new invented name (and new nicknames and English names) drawn from seeded
pools, the calendar moves one year on, the weekday words change, and a few documents that no
label depends on are left out. All label records (entities, facts, supersede chains, the
conflict, the red team) are derived from the transformed world by `gen.py`, exactly as for the
golden set, so they stay consistent.

Nothing here reads the golden tree; it only reuses the golden world's *structure* (`world.py`)
and renames it. Deterministic for a given seed.
"""
import copy
import random
import re
from types import SimpleNamespace

import world as golden

SURNAMES = list("方石蒋屈卢耿范宋尹邵康宁甘祁闵彭游薛")
GIVEN = ["望舒", "知远", "清岚", "景行", "亦安", "承泽", "语桐", "怀瑾", "星河", "明哲", "安澜", "书逸", "芷若", "辰宇", "予宁",
         "清禾", "泽川", "念安", "云帆", "思齐", "嘉木", "时雨", "长风", "静姝", "若水", "修远", "弘毅", "含章"]
ENG_FIRST = ["Adam", "Bella", "Carl", "Dora", "Eric", "Fiona", "Glen", "Hana", "Ivan", "Jade", "Kyle", "Lena", "Mark", "Nina",
             "Owen", "Pia", "Quinn", "Rosa", "Seth", "Tara"]
ENG_LAST = ["Fang", "Shi", "Jiang", "Qu", "Lu", "Geng", "Fan", "Song", "Yin", "Shao", "Kang", "Ning", "Gan", "Qi", "Min"]
COMPANIES = ["云栖数联", "松石科技", "远岫网络", "澜舟智能", "知行云科", "星野数据", "墨川软件", "归一科技"]
ORGS = ["恒岳物流", "朗月传媒", "致远药业", "鸿渐教育", "同舟能源", "博雅出版", "巨浪渔业", "鼎盛家居", "瑞祥金融", "海纳医疗",
        "鲲鹏云", "天衡云", "苍穹云", "磐安咨询", "明镜咨询", "启明咨询"]
PROJECTS = ["海图平台", "航标系统", "曜石中台", "松果商城", "千帆报表", "织网计划", "磐石风控", "南风工单", "雁归门户", "青藤数据湖"]
TOPICS = ["预算评审", "招聘节奏", "客户续约", "代码评审", "发布窗口", "安全巡检", "差旅政策", "绩效校准", "供应商准入", "知识沉淀"]
TOPIC_SHORT = ["月报", "简报", "晨报", "日报"]
TERMS = ["双活架构", "蓝绿部署", "增长飞轮", "零信任", "事件溯源", "特征平台", "边缘缓存", "影子流量", "混沌演练", "指标字典"]
EVENTS = ["季度战略会", "产品发布会", "技术开放日", "年终答谢会", "客户恳谈会", "开源峰会", "校招宣讲会", "供应链大会"]
PARKS = ["湖畔", "云谷", "海岸", "山麓", "江北"]
ENG_ORG = ["Zenith", "Orbit", "Cobalt", "Ember", "Quartz", "Helix", "Nimbus", "Vertex"]
ENG_PROJ = ["Compass", "Beacon", "Granite", "Pinecone", "Sailor", "Loom", "Bedrock", "Zephyr"]
ROOMS = [("A会议室", "东会议室"), ("B会议室", "西会议室")]


def _pick(r, pool, n):
    return r.sample(pool, n)


def _cjk(s):
    return re.search(r"[\u4e00-\u9fff]", s) is not None


def _mapping(r):
    """Old string -> new string for every name, alias and place of the golden world; plus unused pool items."""
    m = {}
    surn = _pick(r, SURNAMES, 11)
    given = _pick(r, GIVEN, 9)
    efirst = _pick(r, ENG_FIRST, 11)
    elast = _pick(r, ENG_LAST, 11)
    people_keys = [k for k, v in golden.ENT.items() if v[0] == "Person"]
    for i, key in enumerate(people_keys):
        _t, name, aliases, _x = golden.ENT[key]
        s, g = surn[i], given[i]
        m[name] = s + g
        old_sur, old_given = name[0], name[1:]
        for a in aliases:
            if a == old_given:
                m[a] = g
            elif a == "老" + old_sur:
                m[a] = "老" + s
            elif a == old_sur + "总":
                m[a] = s + "总"
            elif a == "小" + old_sur:
                m[a] = "小" + s
            elif a.startswith("小"):
                m[a] = "小" + g[-1]
            elif a.endswith("哥"):
                m[a] = g[-1] + "哥"
            elif len(a) == 2 and a[0] == a[1]:
                m[a] = g[0] * 2
            elif re.fullmatch(r"[A-Za-z]+ [A-Za-z]+", a):
                m[a] = f"{efirst[i]} {elast[i]}"
            elif re.fullmatch(r"[A-Za-z]+", a):
                m[a] = efirst[i]
            else:
                raise SystemExit(f"holdout: no alias rule for {a!r}")
    company = r.choice(COMPANIES)
    orgs = _pick(r, ORGS, 6)
    eorg = _pick(r, ENG_ORG, 4)
    org_keys = [k for k, v in golden.ENT.items() if v[0] == "Organization"]
    for key, new, eng in zip(org_keys, [company, orgs[0], orgs[1], orgs[2][:2] + "云"], eorg):
        _t, name, aliases, _x = golden.ENT[key]
        m[name] = new
        for a in aliases:
            m[a] = new[:2] if _cjk(a) else f"{eng} {a.split()[-1]}"
    projects = _pick(r, PROJECTS, 4)
    eproj = _pick(r, ENG_PROJ, 4)
    for key, new, eng in zip([k for k, v in golden.ENT.items() if v[0] == "Project"], projects, eproj):
        _t, name, aliases, _x = golden.ENT[key]
        m[name] = new
        for a in aliases:
            m[a] = new[:2] if _cjk(a) else (eng + " " + a.split()[-1] if " " in a else eng)
    topics = _pick(r, TOPICS, 4)
    short = r.choice(TOPIC_SHORT)
    for key, new in zip([k for k, v in golden.ENT.items() if v[0] == "Topic"], [short] + topics):
        _t, name, aliases, _x = golden.ENT[key]
        m[name] = new
        for a in aliases:
            m[a] = new + "汇总"
    terms = _pick(r, TERMS, 5)
    for key, new in zip([k for k, v in golden.ENT.items() if v[0] == "Term"], terms):
        _t, name, aliases, _x = golden.ENT[key]
        m[name] = new
        for a in aliases:
            m[a] = new + "版"
    events = _pick(r, EVENTS, 5)
    for key, new in zip([k for k, v in golden.ENT.items() if v[0] == "Event"], events):
        _t, name, aliases, _x = golden.ENT[key]
        m[name] = new
        for a in aliases:
            m[a] = new + "（场次）"
    park = r.choice(PARKS)
    m["滨江园区"] = park + "园区"
    m["滨江会议室"] = park + "会议室"
    m["滨江"] = park
    for old, new in ROOMS:
        m[old] = new
    m["青岚智造综合办"] = company + "综合办"
    spare = {"surn": surn[9:], "efirst": efirst[9:], "elast": elast[9:], "given": [x for x in GIVEN if x not in given], "orgs": orgs[3:]}
    return m, {"company": company, "park": park, "short": short}, spare


def _words(r):
    """Weekday and phrasing words that move with the topic of the weekly report."""
    day_a, day_b = r.choice([("周三", "周二"), ("周二", "周一"), ("周六", "周五")])
    return {"每周五": "每" + day_a, "每周四": "每" + day_b, "周五": day_a, "周四": day_b}


def _compile(mapping):
    keys = sorted(mapping, key=len, reverse=True)
    rx = re.compile("|".join(re.escape(k) for k in keys))
    return lambda s: rx.sub(lambda mo: mapping[mo.group(0)], s)


def _walk(obj, f):
    if isinstance(obj, str):
        return f(obj)
    if isinstance(obj, list):
        return [_walk(x, f) for x in obj]
    if isinstance(obj, tuple):
        return tuple(_walk(x, f) for x in obj)
    if isinstance(obj, dict):
        return {k: _walk(v, f) for k, v in obj.items()}
    return obj


def build(seed):
    r = random.Random(seed)
    mapping, facts, spare = _mapping(r)
    mapping = {**mapping, **_words(r)}
    # Years move one on (hire dates one more), in the same single pass.
    mapping.update({"2026": "2027", "2025": "2026"})
    t = _compile(mapping)
    ent = _walk(copy.deepcopy(golden.ENT), t)
    docs = _walk(copy.deepcopy(golden.DOCS), t)
    # Documents that no fact points at may be left out, as long as every entity still appears somewhere.
    used = {p for f in golden.FACTS for p, _tag in f[5]}
    cand = [d["path"] for d in golden.DOCS if d["path"] not in used and d["path"].startswith(("Notes/", "Weekly/", "Data/", "Proposals/"))]
    for _attempt in range(80):
        drop = set(r.sample(cand, 5))
        keep = [d for d, od in zip(docs, golden.DOCS) if od["path"] not in drop]
        text = "\n".join(str(d["blocks"]) + d["title"] for d in keep)
        miss = [k for k, v in ent.items() if v[0] != "Decision" and not any(n in text for n in [v[1]] + list(v[2]))]
        if not miss:
            break
    else:
        raise SystemExit("holdout: could not keep every entity anchored")
    docs = keep
    kept = {d["path"] for d in docs}
    facts_rows = [(p, s, o, vf, vt, [(t(path), tag) for path, tag in tags], label) for p, s, o, vf, vt, tags, label in golden.FACTS]
    facts_rows = [f for f in facts_rows if all(p in kept for p, _tag in f[5])]
    labels = {f[6] for f in facts_rows if f[6]}
    supersede = [(a, b) for a, b in golden.SUPERSEDE if a in labels and b in labels]
    conflicts = [(c, fl) for c, fl in golden.CONFLICTS if all(x in labels for x in fl)]
    # Red team: its own invented names, disjoint from the cast above; the decoy keeps its golden
    # meaning (same name as an in-scope person) because it goes through the same mapping.
    rr = random.Random(seed + 1)
    rt_map = {
        "秦沐阳": spare["surn"][0] + rr.choice(spare["given"]), "岳清河": spare["surn"][1] + rr.choice(spare["given"]),
        "老秦": "老" + spare["surn"][0], "Marcus Qin": f"{spare['efirst'][0]} {spare['elast'][0]}",
        "Frank Yue": f"{spare['efirst'][1]} {spare['elast'][1]}",
        "玄武资本": rr.choice(["紫微资本", "天枢资本", "开阳资本"]), "Xuanwu Capital": "Ziwei Capital",
        "夜航计划": rr.choice(["破晓计划", "拂晓计划", "星火计划"]), "Night Flight": "Daybreak",
    }
    rt = _compile({**mapping, **rt_map})
    rt_docs = _walk(copy.deepcopy(golden.RT_DOCS), rt)
    rt_ent = [(k, ty, rt(n), [rt(a) for a in al], only, note) for k, ty, n, al, only, note in golden.RT_ENT]
    return SimpleNamespace(
        ENT=ent, DOCS=docs, FACTS=facts_rows, SUPERSEDE=supersede, CONFLICTS=conflicts,
        RT_DOCS=rt_docs, RT_ENT=rt_ent, SCOPES=golden.SCOPES, CREATOR=facts["company"] + "综合办",
        MIN_DOCS=40, DROPPED=sorted(drop),
    )
