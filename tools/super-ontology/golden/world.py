"""Content of the synthetic golden set: a fictional 9-person company (all names invented).

Blocks (per document): ("h1"|"h2"|"p"|"li", text, tag?) ; ("trow"|"row", [cells], tag?) ;
("slide", title, [bullets], tag?). `tag` lets labels point at a block; gen.py resolves the
tag to a real anchor (lines / paragraph / cell range / slide / page+bbox) while rendering.
"""

# ---------------------------------------------------------------- entities
# key: (type, name, aliases, extra)
ENT = {
    # people
    "lin": ("Person", "林远舟", ["老林", "林总", "Evan Lin", "Evan"], {}),
    "zhou": ("Person", "周若溪", ["若溪", "Ruby Zhou", "Ruby"], {}),
    "chen": ("Person", "陈子墨", ["小陈", "子墨", "Ethan Chen", "Ethan"], {}),
    "sun": ("Person", "孙晓棠", ["小棠", "Tina Sun", "Tina"], {}),
    "wu": ("Person", "吴启明", ["老吴", "Kevin Wu", "Kevin"], {}),
    "zheng": ("Person", "郑雨桐", ["雨桐", "Yutong Zheng"], {}),
    "han": ("Person", "韩立", ["立哥", "Leo Han", "Leo"], {}),
    "xu": ("Person", "许嘉怡", ["嘉嘉", "Jiayi Xu", "Jiayi"], {}),
    "he": ("Person", "贺文博", ["文博", "Ben He"], {}),
    # organizations
    "qinglan": ("Organization", "青岚智造", ["青岚", "QingLan Tech"], {}),
    "chenghai": ("Organization", "澄海物流", ["澄海", "Chenghai Logistics"], {}),
    "yuanfan": ("Organization", "远帆咨询", ["远帆", "Yuanfan Consulting"], {}),
    "beichen": ("Organization", "北辰云", ["北辰", "Beichen Cloud"], {}),
    # departments of the people table (10 5.1 xlsx.people: the department column is a member_of Organization)
    "dept_mgmt": ("Organization", "管理", [], {}),
    "dept_prod": ("Organization", "产品", [], {}),
    "dept_rd": ("Organization", "研发", [], {}),
    "dept_design": ("Organization", "设计", [], {}),
    "dept_data": ("Organization", "数据", [], {}),
    "dept_biz": ("Organization", "商务", [], {}),
    "dept_gen": ("Organization", "综合", [], {}),
    # projects
    "xingtu": ("Project", "星图看板", ["星图", "Starmap"], {}),
    "lighthouse": ("Project", "灯塔迁移", ["灯塔", "Lighthouse"], {}),
    "zhiliao": ("Project", "知了客服", ["知了", "Zhiliao Bot"], {}),
    "website": ("Project", "官网改版", ["官网项目", "Website Revamp"], {}),
    # topics
    "weekly": ("Topic", "周报", ["每周汇报"], {}),
    "chartsel": ("Topic", "图表选型", ["图表库选型"], {}),
    "csat": ("Topic", "客户满意度", ["满意度"], {}),
    "office": ("Topic", "办公迁址", ["搬迁办公室"], {}),
    "migration": ("Topic", "数据迁移", ["机房迁移"], {}),
    # terms
    "selfengine": ("Term", "自研图表引擎", ["自研引擎"], {}),
    "smallmodel": ("Term", "自建小模型", ["自建模型"], {}),
    "oss": ("Term", "开源图表库", [], {}),
    "gray": ("Term", "灰度发布", ["灰度上线"], {}),
    "northstar": ("Term", "北极星指标", ["核心指标"], {}),
    # events
    "kickoff": ("Event", "年度计划会", ["年度规划会"], {}),
    "retro": ("Event", "星图复盘会", ["星图复盘"], {}),
    "webkick": ("Event", "官网改版启动会", ["官网启动会"], {}),
    "teambuild": ("Event", "年度团建", ["秋季团建"], {}),
    "salon": ("Event", "客户沙龙", ["用户沙龙"], {}),
    # decisions
    "dec1": ("Decision", "周报每周五发送", ["周五发周报"], {}),
    "dec2": ("Decision", "周报改为每周四发送", ["周四发周报"], {}),
    "dec3": ("Decision", "取消周报改用看板自动汇总", ["取消周报"], {}),
    "dec4": ("Decision", "星图看板采用自研图表引擎", ["星图自研方案"], {}),
    "dec5": ("Decision", "星图看板改用开源图表库", ["换图表库"], {}),
    "dec6": ("Decision", "知了客服采购外部模型服务", ["客服外采"], {}),
    "dec7": ("Decision", "办公室迁至滨江园区", ["搬家滨江"], {}),
    "pref1": ("Decision", "会议控制在三十分钟内", ["会议不超过半小时"], {"subkind": "preference"}),
    "pref2": ("Decision", "文档先写结论再写过程", ["结论先行"], {"subkind": "preference"}),
    "pref3": ("Decision", "沟通优先使用文字而非电话", ["少打电话"], {"subkind": "preference"}),
}

# ---------------------------------------------------------------- block helpers
def H1(t, tag=None): return ("h1", t, tag)
def H2(t, tag=None): return ("h2", t, tag)
def P(t, tag=None): return ("p", t, tag)
def LI(t, tag=None): return ("li", t, tag)
def TR(cells, tag=None): return ("trow", cells, tag)
def ROW(cells, tag=None): return ("row", cells, tag)
def SL(title, bullets, tag=None): return ("slide", title, bullets, tag)

DOCS = []

def doc(path, title, blocks, about=(), fm=None, created="2026-01-01"):
    DOCS.append({"path": path, "title": title, "blocks": blocks, "about": list(about),
                 "fm": fm, "created": created})

def meeting(path, title, date, place, rec, who, body, table, about=()):
    blocks = [H1(title), P(f"时间：{date}　地点：{place}　记录：{rec}"), H2("参会人员"), P(who), H2("议题与结论")]
    blocks += body
    blocks += [H2("待办事项"), TR(["事项", "负责人", "截止日期"])] + table
    doc(path, title, blocks, about, created=date)

ALL = "林远舟、周若溪、陈子墨、孙晓棠、吴启明、郑雨桐、韩立、许嘉怡、贺文博。"

# ---------------------------------------------------------------- docx meeting notes (10)
meeting("Meetings/2026-01-08-年度计划会.docx", "2026年度计划会纪要", "2026-01-08", "A会议室", "许嘉怡", ALL, [
    P("青岚智造本年度三个重点项目：星图看板、灯塔迁移、知了客服。", "plan"),
    P("星图看板自2026年1月1日起由陈子墨负责，Ethan 带领吴启明、郑雨桐组成核心小组。", "own_xt"),
    P("林总强调北极星指标为月活跃团队数，所有项目都要围绕它汇报。", "northstar"),
    P("周报沿用现有格式，由许嘉怡每周汇总。", "weekly"),
], [TR(["整理年度OKR", "周若溪", "2026-01-15"]), TR(["确认第一季度预算", "许嘉怡", "2026-01-20"])],
    about=["kickoff", "xingtu"])
meeting("Meetings/2026-01-10-周会.docx", "1月第二周周会纪要", "2026-01-10", "B会议室", "许嘉怡", "林远舟、周若溪、陈子墨、许嘉怡、韩立。", [
    P("经讨论，周报统一在每周五下班前发送，由林远舟拍板。", "dec"),
    P("本周星图看板完成数据接入联调，吴启明负责后端接口。", "prog"),
], [TR(["发布周报模板", "许嘉怡", "2026-01-12"])], about=["weekly"])
meeting("Meetings/2026-02-15-星图技术评审.docx", "星图看板技术评审纪要", "2026-02-15", "A会议室", "贺文博", "林远舟、陈子墨、吴启明、郑雨桐、贺文博。", [
    P("经评审，星图看板采用自研图表引擎，陈子墨负责方案，林远舟批准。", "dec"),
    P("老吴评估自研引擎的工作量约六人周。", "est"),
    P("灯塔迁移自2026年2月1日起由陈子墨兼任负责人，目标是把旧机房数据迁到北辰云。", "own_lh"),
], [TR(["输出引擎设计文档", "陈子墨", "2026-02-28"]), TR(["梳理图表类型清单", "郑雨桐", "2026-02-25"])],
    about=["xingtu", "selfengine"])
meeting("Meetings/2026-03-03-客服方案评审.docx", "知了客服方案评审纪要", "2026-03-03", "A会议室", "许嘉怡", "林远舟、周若溪、陈子墨、吴启明、许嘉怡。", [
    P("知了客服自2026年3月1日起由周若溪负责。", "own_zl"),
    P("此前团队尝试过自建小模型，因为准确率只有六成，已经放弃。", "fail"),
    P("会议决定知了客服采购外部模型服务，不再自建小模型，由若溪牵头对接远帆咨询评估供应商。", "dec"),
], [TR(["供应商评估表", "周若溪", "2026-03-20"]), TR(["接口预研", "吴启明", "2026-03-25"])],
    about=["zhiliao", "smallmodel"])
meeting("Meetings/2026-04-02-周会.docx", "4月第一周周会纪要", "2026-04-02", "B会议室", "许嘉怡", "林远舟、周若溪、陈子墨、韩立、许嘉怡。", [
    P("周报发送时间由周五改为周四，因为客户周五下午很少回复，林远舟确认。", "dec"),
    P("韩立（Leo）反馈澄海物流希望每月增加一次对账沟通。", "cust"),
], [TR(["更新周报模板发送时间", "许嘉怡", "2026-04-03"])], about=["weekly", "chenghai"])
meeting("Meetings/2026-05-20-星图复盘会.docx", "星图复盘会纪要", "2026-05-20", "A会议室", "贺文博", "林远舟、周若溪、陈子墨、吴启明、郑雨桐、贺文博。", [
    P("复盘结论：自研图表引擎维护成本过高，星图看板改用开源图表库，原自研方案作废，林远舟与陈子墨共同确认。", "dec"),
    P("自研引擎在大数据量下渲染卡顿，属于已经尝试且失败的路线。", "fail"),
    P("迁移到开源图表库预计两周完成，由小陈和雨桐分工。", "plan"),
], [TR(["完成图表库迁移", "陈子墨", "2026-06-05"]), TR(["回归测试", "郑雨桐", "2026-06-10"])],
    about=["xingtu", "retro", "oss"])
meeting("Meetings/2026-06-01-项目分工调整.docx", "项目分工调整会纪要", "2026-06-01", "B会议室", "许嘉怡", "林远舟、周若溪、陈子墨、孙晓棠、许嘉怡。", [
    P("自2026年6月1日起，星图看板负责人由陈子墨调整为周若溪，陈子墨专注灯塔迁移。", "own_xt"),
    P("官网改版筹备由孙晓棠牵头，7月正式启动。", "web"),
], [TR(["交接星图看板文档", "陈子墨", "2026-06-07"])], about=["xingtu", "lighthouse"])
meeting("Meetings/2026-06-15-迁址沟通会.docx", "办公迁址沟通会纪要", "2026-06-15", "A会议室", "许嘉怡", ALL, [
    P("公司决定于2026年7月起搬到滨江园区办公，林远舟拍板。", "dec"),
    P("许嘉怡负责与园区物业对接，贺文博协助搬运清单。", "task"),
], [TR(["签署租赁合同", "许嘉怡", "2026-06-30"])], about=["office"])
meeting("Meetings/2026-07-06-官网改版启动会.docx", "官网改版启动会纪要", "2026-07-06", "滨江会议室", "贺文博", "林远舟、周若溪、孙晓棠、韩立、贺文博。", [
    P("官网改版自2026年7月1日起由孙晓棠负责，Tina 统筹设计，贺文博负责前端实现。", "own_web"),
    P("本次会议即官网改版启动会，目标在十月上线新官网。", "kick"),
], [TR(["输出设计稿", "孙晓棠", "2026-07-31"]), TR(["搭建前端骨架", "贺文博", "2026-08-10"])],
    about=["website", "webkick"])
meeting("Meetings/2026-08-20-周会.docx", "8月第三周周会纪要", "2026-08-20", "滨江会议室", "许嘉怡", "林远舟、周若溪、陈子墨、韩立、许嘉怡。", [
    P("从下周起取消周报，改由星图看板自动汇总，林远舟确认。", "dec"),
    P("九月计划举办年度团建，由嘉嘉统筹。", "tb"),
], [TR(["配置看板自动汇总", "郑雨桐", "2026-08-27"])], about=["weekly", "teambuild"])

# ---------------------------------------------------------------- md decisions (7)
def dec_md(path, title, status, date, owner, lines, about, extra_fm=None):
    fm = {"type": "decision", "status": status, "date": date, "owner": owner}
    if extra_fm:
        fm.update(extra_fm)
    doc(path, title, [H1(title)] + lines, about, fm=fm, created=date)

dec_md("Decisions/D-2026-01-周报每周五发送.md", "决定：周报每周五发送", "superseded", "2026-01-10", "林远舟", [
    P("林远舟于2026年1月10日决定：周报统一在每周五下班前发送。", "dec"),
    P("背景：此前周报发送时间不固定，客户反映难以跟进。"),
    P("后续：本决定已被“周报改为每周四发送”取代。", "sup")], ["weekly"])
dec_md("Decisions/D-2026-02-星图采用自研引擎.md", "决定：星图看板采用自研图表引擎", "superseded", "2026-02-15", "林远舟", [
    P("2026年2月15日技术评审后，林远舟批准星图看板采用自研图表引擎。", "dec"),
    P("理由：希望完全掌控渲染性能与交互。"),
    P("风险：团队人手有限，维护成本不确定。")], ["xingtu", "selfengine"])
dec_md("Decisions/D-2026-03-知了客服采购外部模型.md", "决定：知了客服采购外部模型服务", "active", "2026-03-03", "周若溪", [
    P("2026年3月3日，周若溪决定知了客服采购外部模型服务，不再自建小模型。", "dec"),
    P("被否决方案：自建小模型，准确率只有六成。", "rej"),
    P("供应商评估由远帆咨询协助。")], ["zhiliao", "smallmodel"])
dec_md("Decisions/D-2026-04-周报改为每周四发送.md", "决定：周报改为每周四发送", "superseded", "2026-04-02", "林远舟", [
    P("林远舟于2026年4月2日决定：周报改为每周四发送。", "dec"),
    P("本决定取代2026年1月10日“周报每周五发送”的决定。", "sup"),
    P("原因：客户周五下午很少回复。")], ["weekly"])
dec_md("Decisions/D-2026-05-星图改用开源图表库.md", "决定：星图看板改用开源图表库", "active", "2026-05-20", "林远舟", [
    P("2026年5月20日复盘后，林远舟决定星图看板改用开源图表库。", "dec"),
    P("本决定取代“星图看板采用自研图表引擎”，自研方案作废。", "sup"),
    P("本次图表选型的结论是不再自研，直接采用成熟的开源方案。"),
    P("被否决方案：自研图表引擎，大数据量下渲染卡顿，维护成本过高。", "rej")], ["xingtu", "chartsel", "oss"])
dec_md("Decisions/D-2026-06-办公迁至滨江.md", "决定：办公室迁至滨江园区", "active", "2026-06-15", "林远舟", [
    P("林远舟于2026年6月15日决定公司2026年7月起迁至滨江园区办公。", "dec"),
    P("许嘉怡负责搬迁落地，预算见年度预算表。")], ["office"])
dec_md("Decisions/D-2026-08-取消周报.md", "决定：取消周报改用看板自动汇总", "active", "2026-08-20", "林远舟", [
    P("林远舟于2026年8月20日决定取消周报，改由星图看板自动汇总。", "dec"),
    P("本决定取代“周报改为每周四发送”。", "sup"),
    P("郑雨桐负责配置自动汇总。")], ["weekly", "xingtu"])

# ---------------------------------------------------------------- md people / projects / preferences (11)
doc("People/林远舟.md", "林远舟", [H1("林远舟"), P("职位：创始人兼CEO，英文名 Evan Lin，同事称呼老林。", "id"),
    P("2025年创办青岚智造，主要关注北极星指标与客户关系。", "org")], ["qinglan", "northstar"],
    fm={"type": "person", "role": "CEO"}, created="2026-01-05")
doc("People/周若溪.md", "周若溪", [H1("周若溪"), P("职位：产品负责人，英文名 Ruby Zhou。", "id"),
    P("目前负责知了客服与星图看板，擅长梳理需求。", "own")], ["zhiliao", "xingtu"],
    fm={"type": "person", "role": "产品负责人"}, created="2026-01-05")
doc("People/陈子墨.md", "陈子墨", [H1("陈子墨"), P("职位：技术负责人，英文名 Ethan Chen，团队里也叫小陈。", "id"),
    P("目前负责灯塔迁移，此前负责星图看板。", "own")], ["lighthouse", "xingtu"],
    fm={"type": "person", "role": "技术负责人"}, created="2026-01-05")
doc("People/孙晓棠.md", "孙晓棠", [H1("孙晓棠"), P("职位：设计师，英文名 Tina Sun，大家叫她小棠。", "id"),
    P("目前负责官网改版。", "own")], ["website"],
    fm={"type": "person", "role": "设计师"}, created="2026-01-05")
doc("Projects/星图看板.md", "星图看板", [H1("星图看板"), P("面向客户的数据看板产品，英文代号 Starmap。", "intro"),
    P("2026-01-01 至 2026-05-31 由陈子墨负责。", "hist"),
    P("2026-06-01 起由周若溪负责。", "cur"),
    P("核心成员：吴启明、郑雨桐。", "team"),
    P("图表方案：2026年5月起使用开源图表库，此前的自研图表引擎已作废。", "chart")],
    ["xingtu", "oss", "selfengine"], fm={"type": "project", "status": "active"}, created="2026-06-01")
doc("Projects/灯塔迁移.md", "灯塔迁移", [H1("灯塔迁移"), P("这是一次数据迁移：把旧机房的数据与服务迁移到北辰云，Lighthouse 为英文代号。", "intro"),
    P("负责人：陈子墨（2026-02-01 起）。", "owner"),
    P("成员：吴启明负责数据库迁移，上线采用灰度发布。", "team")],
    ["lighthouse", "migration", "gray"], fm={"type": "project", "status": "active"}, created="2026-02-01")
doc("Projects/知了客服.md", "知了客服", [H1("知了客服"), P("基于大模型的在线客服机器人，Zhiliao Bot。", "intro"),
    P("负责人：周若溪（2026-03-01 起）。", "owner"),
    P("方案演进：自建小模型失败，改为采购外部模型服务。", "fail")],
    ["zhiliao", "smallmodel"], fm={"type": "project", "status": "active"}, created="2026-03-01")
doc("Projects/官网改版.md", "官网改版", [H1("官网改版"), P("重做青岚智造官方网站，英文名 Website Revamp。", "intro"),
    P("负责人：孙晓棠（2026-07-01 起）。", "owner"),
    P("成员：贺文博负责前端；韩立负责客户案例素材。", "team")],
    ["website", "qinglan"], fm={"type": "project", "status": "active"}, created="2026-07-01")
doc("Preferences/林远舟-会议偏好.md", "林远舟的会议偏好", [H1("林远舟的会议偏好"),
    P("林远舟的要求：会议控制在三十分钟内，超时必须拆会。", "pref")], [],
    fm={"type": "preference", "owner": "林远舟"}, created="2026-02-01")
doc("Preferences/周若溪-文档偏好.md", "周若溪的文档偏好", [H1("周若溪的文档偏好"),
    P("周若溪偏好文档先写结论再写过程，结论放在第一段。", "pref")], [],
    fm={"type": "preference", "owner": "周若溪"}, created="2026-02-01")
doc("Preferences/陈子墨-沟通偏好.md", "陈子墨的沟通偏好", [H1("陈子墨的沟通偏好"),
    P("子墨希望沟通优先使用文字而非电话，紧急情况除外。", "pref")], [],
    fm={"type": "preference", "owner": "陈子墨"}, created="2026-02-01")

# ---------------------------------------------------------------- xlsx (4)
doc("Tables/人员表.xlsx", "人员表", [
    ROW(["姓名", "英文名", "昵称", "部门", "职位", "入职日期"]),
    ROW(["林远舟", "Evan Lin", "老林", "管理", "CEO", "2025-03-01"], "r_lin"),
    ROW(["周若溪", "Ruby Zhou", "若溪", "产品", "产品负责人", "2025-04-10"], "r_zhou"),
    ROW(["陈子墨", "Ethan Chen", "小陈", "研发", "技术负责人", "2025-03-15"], "r_chen"),
    ROW(["孙晓棠", "Tina Sun", "小棠", "设计", "设计师", "2025-06-01"], "r_sun"),
    ROW(["吴启明", "Kevin Wu", "老吴", "研发", "后端工程师", "2025-05-06"], "r_wu"),
    ROW(["郑雨桐", "Yutong Zheng", "雨桐", "数据", "数据分析师", "2025-08-18"], "r_zheng"),
    ROW(["韩立", "Leo Han", "立哥", "商务", "商务经理", "2025-07-01"], "r_han"),
    ROW(["许嘉怡", "Jiayi Xu", "嘉嘉", "综合", "行政财务", "2025-03-20"], "r_xu"),
    ROW(["贺文博", "Ben He", "文博", "研发", "前端工程师", "2026-01-05"], "r_he"),
], ["qinglan"], created="2026-01-06")
doc("Tables/项目负责人表.xlsx", "项目负责人表", [
    ROW(["项目", "负责人", "生效日期", "备注"]),
    ROW(["星图看板", "周若溪", "2026-06-01", "接替陈子墨"], "r_xt"),
    ROW(["灯塔迁移", "陈子墨", "2026-02-01", ""], "r_lh"),
    ROW(["知了客服", "周若溪", "2026-03-01", ""], "r_zl"),
    ROW(["官网改版", "韩立", "2026-07-01", "商务侧登记"], "r_web"),
], [], created="2026-07-02")
doc("Tables/客户与供应商.xlsx", "客户与供应商", [
    ROW(["名称", "类型", "联系人", "备注"]),
    ROW(["澄海物流", "客户", "韩立", "星图看板试用客户"], "r_ch"),
    ROW(["远帆咨询", "供应商", "周若溪", "模型供应商评估顾问"], "r_yf"),
    ROW(["北辰云", "供应商", "陈子墨", "灯塔迁移目标云平台"], "r_bc"),
], ["qinglan"], created="2026-03-05")
doc("Tables/年度预算表.xlsx", "年度预算表", [
    ROW(["项目", "预算（万元）", "季度"]),
    ROW(["星图看板", 60, "Q1-Q2"]), ROW(["灯塔迁移", 45, "Q1-Q3"]), ROW(["知了客服", 30, "Q2-Q3"]),
    ROW(["官网改版", 20, "Q3-Q4"]), ROW(["办公迁址", 35, "Q3"], "r_office"),
], [], created="2026-01-20")

# ---------------------------------------------------------------- pdf proposals (7)
doc("Proposals/星图看板产品方案.pdf", "星图看板产品方案", [H1("星图看板产品方案（第二版）"),
    P("本方案面向客户经理与运营人员，提供团队活跃度、任务进展等看板。", "scope"),
    P("图表层采用开源图表库，替代最初设想的自研图表引擎，该调整于2026年5月确认。", "chart"),
    P("产品负责人：周若溪；技术负责人：陈子墨（Ethan）。", "people")], ["xingtu", "oss"], created="2026-06-10")
doc("Proposals/灯塔迁移方案.pdf", "灯塔迁移方案", [H1("灯塔迁移方案"),
    P("目标：2026年第三季度前，把旧机房的数据与服务迁移到北辰云。", "goal"),
    P("项目负责人为陈子墨，数据库迁移由吴启明实施，上线采用灰度发布，每次放量不超过百分之十。", "owner"),
    P("回滚预案：保留旧机房两周，出现异常即切回。")], ["lighthouse", "migration", "gray"], created="2026-02-10")
doc("Proposals/知了客服方案.pdf", "知了客服方案", [H1("知了客服方案"),
    P("知了客服面向澄海物流等客户，提供自动应答与工单转接。", "scope"),
    P("模型来源：采购外部模型服务，由远帆咨询协助评估；早期自建小模型的尝试已终止。", "model"),
    P("负责人：周若溪。", "owner")], ["zhiliao", "smallmodel"], created="2026-03-12")
doc("Proposals/官网改版方案.pdf", "官网改版方案", [H1("官网改版方案"),
    P("目标：2026年十月上线新官网，突出星图看板与知了客服两款产品。", "goal"),
    P("项目负责人：孙晓棠；前端：贺文博。", "owner")], ["website", "xingtu", "zhiliao"], created="2026-07-10")
doc("Proposals/澄海物流合作方案.pdf", "澄海物流合作方案", [H1("澄海物流合作方案"),
    P("澄海物流为星图看板的首批试用客户，商务对接人为韩立。", "scope"),
    P("双方约定每月一次对账沟通，并参加青岚智造举办的客户沙龙。", "salon")], ["chenghai", "xingtu", "salon"], created="2026-04-15")
doc("Proposals/年度客户调研方案.pdf", "年度客户调研方案", [H1("年度客户调研方案"),
    P("调研目标：衡量客户满意度，并与北极星指标对照。", "goal"),
    P("执行：郑雨桐负责问卷与数据分析，结果在客户沙龙上分享。", "owner")], ["csat", "northstar", "salon"], created="2026-04-20")
doc("Proposals/办公迁址方案.pdf", "办公迁址方案", [H1("办公迁址方案"),
    P("公司计划于2026年7月迁入滨江园区，面积约三百平方米。", "scope"),
    P("许嘉怡负责合同与装修，贺文博协助设备搬运。", "owner")], ["office"], created="2026-06-20")

# ---------------------------------------------------------------- pptx weekly reports (6)
doc("Weekly/2026-W03-周报.pptx", "第3周周报", [
    SL("第3周周报", ["青岚智造 · 2026年1月", "汇报人：许嘉怡"]),
    SL("本周进展", ["星图看板：数据接入联调完成（陈子墨、吴启明）", "知了客服：需求梳理中（周若溪）"], "prog"),
    SL("下周计划", ["确认第一季度预算", "招聘前端工程师"])], ["weekly", "xingtu"], created="2026-01-16")
doc("Weekly/2026-W10-周报.pptx", "第10周周报", [
    SL("第10周周报", ["青岚智造 · 2026年3月", "汇报人：许嘉怡"]),
    SL("本周进展", ["知了客服：确定采购外部模型服务", "灯塔迁移：北辰云环境开通（Ethan）"], "prog"),
    SL("风险", ["自建小模型路线已放弃，需尽快完成供应商评估"], "risk")], ["weekly", "zhiliao", "beichen"], created="2026-03-12")
doc("Weekly/2026-W15-周报.pptx", "第15周周报", [
    SL("第15周周报", ["青岚智造 · 2026年4月", "汇报人：许嘉怡"]),
    SL("本周进展", ["周报改为每周四发送，本期为首期", "星图看板：自研引擎渲染卡顿，评估替代方案"], "prog"),
    SL("客户", ["澄海物流提出每月对账沟通（韩立跟进）"])], ["weekly", "xingtu", "chenghai"], created="2026-04-09")
doc("Weekly/2026-W22-周报.pptx", "第22周周报", [
    SL("第22周周报", ["青岚智造 · 2026年5月", "汇报人：许嘉怡"]),
    SL("本周进展", ["星图复盘会完成，改用开源图表库", "星图看板负责人：周若溪（6月起），陈子墨交接"], "own"),
    SL("下周计划", ["官网改版筹备", "迁址方案定稿"])], ["weekly", "xingtu", "retro"], created="2026-05-28")
doc("Weekly/2026-W28-周报.pptx", "第28周周报", [
    SL("第28周周报", ["青岚智造 · 2026年7月", "汇报人：许嘉怡"]),
    SL("本周进展", ["公司已迁入滨江园区", "官网改版启动，负责人：韩立（商务侧）"], "own"),
    SL("风险", ["官网改版负责人表述不一致，待确认"], "risk")], ["weekly", "website", "office"], created="2026-07-10")
doc("Weekly/2026-W34-周报.pptx", "第34周周报", [
    SL("第34周周报", ["青岚智造 · 2026年8月", "汇报人：许嘉怡"]),
    SL("本周进展", ["决定取消周报，改用星图看板自动汇总", "灯塔迁移：灰度发布进行到第二批（陈子墨）"], "prog"),
    SL("下周计划", ["筹备年度团建"])], ["weekly", "xingtu", "lighthouse", "teambuild"], created="2026-08-21")

# ---------------------------------------------------------------- csv (3) + txt (3)
doc("Data/任务清单.csv", "任务清单", [
    ROW(["编号", "任务", "负责人", "状态", "所属项目"]),
    ROW(["T-101", "整理年度OKR", "周若溪", "完成", "星图看板"]),
    ROW(["T-102", "北辰云环境开通", "陈子墨", "完成", "灯塔迁移"], "r_lh"),
    ROW(["T-103", "供应商评估表", "周若溪", "完成", "知了客服"], "r_zl"),
    ROW(["T-104", "官网设计稿", "孙晓棠", "进行中", "官网改版"], "r_web"),
    ROW(["T-105", "配置看板自动汇总", "郑雨桐", "完成", "星图看板"]),
], ["xingtu", "lighthouse", "zhiliao", "website"], created="2026-08-27")
doc("Data/会议室预订.csv", "会议室预订", [
    ROW(["日期", "会议室", "预订人", "用途"]),
    ROW(["2026-06-15", "A会议室", "许嘉怡", "迁址沟通会"]),
    ROW(["2026-07-06", "滨江会议室", "贺文博", "官网改版启动会"]),
    ROW(["2026-08-20", "滨江会议室", "许嘉怡", "周会"]),
], ["office"], created="2026-08-01")
doc("Data/客户反馈.csv", "客户反馈", [
    ROW(["日期", "客户", "反馈", "跟进人"]),
    ROW(["2026-04-08", "澄海物流", "希望每月增加对账沟通", "韩立"]),
    ROW(["2026-05-12", "澄海物流", "星图看板大图表加载很慢", "陈子墨"]),
    ROW(["2026-07-20", "澄海物流", "新版看板加载速度明显改善", "周若溪"]),
], ["chenghai", "csat", "xingtu"], created="2026-07-21")
doc("Notes/速记-0412.txt", "速记0412", [P("4月12日速记：老林说周报改周四之后，客户回复率明显变好。"),
    P("Ruby 提醒外采模型的合同要走远帆咨询的评估流程。"),
    P("立哥下周去澄海物流现场。")], ["weekly", "chenghai"], created="2026-04-12")
doc("Notes/速记-0601.txt", "速记0601", [P("6月1日速记：小陈把星图看板的交接清单发给了若溪。"),
    P("Tina 说官网改版的设计规范下周出。"),
    P("嘉嘉确认了滨江园区的签约时间。")], ["xingtu", "website", "office"], created="2026-06-01")
doc("Notes/速记-0828.txt", "速记0828", [P("8月28日速记：雨桐的自动汇总已经跑通，周报彻底取消。"),
    P("Kevin 完成灯塔迁移第二批灰度发布。"),
    P("嘉嘉在筹备年度团建，初步定在九月。")], ["weekly", "lighthouse", "teambuild"], created="2026-08-28")

# ---------------------------------------------------------------- relations: explicit facts
# (subject, predicate, object, valid_from, valid_to, [(doc, tag)...], label)
IN = "Meetings/"
FACTS = [
    ("owns", "chen", "xingtu", "2026-01-01", "2026-05-31", [(IN + "2026-01-08-年度计划会.docx", "own_xt"), ("Projects/星图看板.md", "hist"), ("People/陈子墨.md", "own")], "own_xt_old"),
    ("owns", "zhou", "xingtu", "2026-06-01", None, [(IN + "2026-06-01-项目分工调整.docx", "own_xt"), ("Projects/星图看板.md", "cur"), ("Tables/项目负责人表.xlsx", "r_xt"), ("Weekly/2026-W22-周报.pptx", "own")], "own_xt_new"),
    ("owns", "chen", "lighthouse", "2026-02-01", None, [(IN + "2026-02-15-星图技术评审.docx", "own_lh"), ("Projects/灯塔迁移.md", "owner"), ("Tables/项目负责人表.xlsx", "r_lh"), ("Proposals/灯塔迁移方案.pdf", "owner")], "own_lh"),
    ("owns", "zhou", "zhiliao", "2026-03-01", None, [(IN + "2026-03-03-客服方案评审.docx", "own_zl"), ("Projects/知了客服.md", "owner"), ("Tables/项目负责人表.xlsx", "r_zl"), ("Proposals/知了客服方案.pdf", "owner")], "own_zl"),
    ("owns", "sun", "website", "2026-07-01", None, [(IN + "2026-07-06-官网改版启动会.docx", "own_web"), ("Projects/官网改版.md", "owner"), ("Proposals/官网改版方案.pdf", "owner")], "own_web_sun"),
    ("owns", "han", "website", "2026-07-01", None, [("Tables/项目负责人表.xlsx", "r_web"), ("Weekly/2026-W28-周报.pptx", "own")], "own_web_han"),
    # membership: company
    ("member_of", "lin", "qinglan", "2025-03-01", None, [("Tables/人员表.xlsx", "r_lin")], None),
    ("member_of", "zhou", "qinglan", "2025-04-10", None, [("Tables/人员表.xlsx", "r_zhou")], None),
    ("member_of", "chen", "qinglan", "2025-03-15", None, [("Tables/人员表.xlsx", "r_chen")], None),
    ("member_of", "sun", "qinglan", "2025-06-01", None, [("Tables/人员表.xlsx", "r_sun")], None),
    ("member_of", "wu", "qinglan", "2025-05-06", None, [("Tables/人员表.xlsx", "r_wu")], None),
    ("member_of", "zheng", "qinglan", "2025-08-18", None, [("Tables/人员表.xlsx", "r_zheng")], None),
    ("member_of", "han", "qinglan", "2025-07-01", None, [("Tables/人员表.xlsx", "r_han")], None),
    ("member_of", "xu", "qinglan", "2025-03-20", None, [("Tables/人员表.xlsx", "r_xu")], None),
    ("member_of", "he", "qinglan", "2026-01-05", None, [("Tables/人员表.xlsx", "r_he")], None),
    ("member_of", "lin", "dept_mgmt", "2025-03-01", None, [("Tables/人员表.xlsx", "r_lin")], None),
    ("member_of", "zhou", "dept_prod", "2025-04-10", None, [("Tables/人员表.xlsx", "r_zhou")], None),
    ("member_of", "chen", "dept_rd", "2025-03-15", None, [("Tables/人员表.xlsx", "r_chen")], None),
    ("member_of", "sun", "dept_design", "2025-06-01", None, [("Tables/人员表.xlsx", "r_sun")], None),
    ("member_of", "wu", "dept_rd", "2025-05-06", None, [("Tables/人员表.xlsx", "r_wu")], None),
    ("member_of", "zheng", "dept_data", "2025-08-18", None, [("Tables/人员表.xlsx", "r_zheng")], None),
    ("member_of", "han", "dept_biz", "2025-07-01", None, [("Tables/人员表.xlsx", "r_han")], None),
    ("member_of", "xu", "dept_gen", "2025-03-20", None, [("Tables/人员表.xlsx", "r_xu")], None),
    ("member_of", "he", "dept_rd", "2026-01-05", None, [("Tables/人员表.xlsx", "r_he")], None),
    # membership: projects
    ("member_of", "wu", "xingtu", "2026-01-01", None, [(IN + "2026-01-08-年度计划会.docx", "own_xt"), ("Projects/星图看板.md", "team")], None),
    ("member_of", "zheng", "xingtu", "2026-01-01", None, [(IN + "2026-01-08-年度计划会.docx", "own_xt"), ("Projects/星图看板.md", "team")], None),
    ("member_of", "wu", "lighthouse", "2026-02-01", None, [("Projects/灯塔迁移.md", "team"), ("Proposals/灯塔迁移方案.pdf", "owner")], None),
    ("member_of", "he", "website", "2026-07-01", None, [(IN + "2026-07-06-官网改版启动会.docx", "own_web"), ("Projects/官网改版.md", "team")], None),
    # decisions
    ("decides", "lin", "dec1", "2026-01-10", "2026-04-01", [(IN + "2026-01-10-周会.docx", "dec"), ("Decisions/D-2026-01-周报每周五发送.md", "dec")], "dec1"),
    ("decides", "lin", "dec2", "2026-04-02", "2026-08-19", [(IN + "2026-04-02-周会.docx", "dec"), ("Decisions/D-2026-04-周报改为每周四发送.md", "dec")], "dec2"),
    ("decides", "lin", "dec3", "2026-08-20", None, [(IN + "2026-08-20-周会.docx", "dec"), ("Decisions/D-2026-08-取消周报.md", "dec")], "dec3"),
    ("decides", "lin", "dec4", "2026-02-15", "2026-05-19", [(IN + "2026-02-15-星图技术评审.docx", "dec"), ("Decisions/D-2026-02-星图采用自研引擎.md", "dec")], "dec4"),
    ("decides", "lin", "dec5", "2026-05-20", None, [(IN + "2026-05-20-星图复盘会.docx", "dec"), ("Decisions/D-2026-05-星图改用开源图表库.md", "dec")], "dec5"),
    ("decides", "zhou", "dec6", "2026-03-03", None, [(IN + "2026-03-03-客服方案评审.docx", "dec"), ("Decisions/D-2026-03-知了客服采购外部模型.md", "dec")], "dec6"),
    ("decides", "lin", "dec7", "2026-06-15", None, [(IN + "2026-06-15-迁址沟通会.docx", "dec"), ("Decisions/D-2026-06-办公迁至滨江.md", "dec")], "dec7"),
    ("decides", "lin", "pref1", "2026-02-01", None, [("Preferences/林远舟-会议偏好.md", "pref")], None),
    ("decides", "zhou", "pref2", "2026-02-01", None, [("Preferences/周若溪-文档偏好.md", "pref")], None),
    ("decides", "chen", "pref3", "2026-02-01", None, [("Preferences/陈子墨-沟通偏好.md", "pref")], None),
    # supersedes (core relation between decisions)
    ("supersedes", "dec2", "dec1", "2026-04-02", None, [("Decisions/D-2026-04-周报改为每周四发送.md", "sup"), ("Decisions/D-2026-01-周报每周五发送.md", "sup")], "sup21"),
    ("supersedes", "dec3", "dec2", "2026-08-20", None, [("Decisions/D-2026-08-取消周报.md", "sup")], "sup32"),
    ("supersedes", "dec5", "dec4", "2026-05-20", None, [("Decisions/D-2026-05-星图改用开源图表库.md", "sup"), (IN + "2026-05-20-星图复盘会.docx", "dec")], "sup54"),
    # rejects / tried_and_failed
    ("rejects", "dec5", "selfengine", "2026-05-20", None, [("Decisions/D-2026-05-星图改用开源图表库.md", "rej")], None),
    ("rejects", "dec6", "smallmodel", "2026-03-03", None, [("Decisions/D-2026-03-知了客服采购外部模型.md", "rej"), (IN + "2026-03-03-客服方案评审.docx", "dec")], None),
    ("tried_and_failed", "xingtu", "selfengine", "2026-05-20", None, [(IN + "2026-05-20-星图复盘会.docx", "fail"), ("Projects/星图看板.md", "chart")], None),
    ("tried_and_failed", "zhiliao", "smallmodel", "2026-03-03", None, [(IN + "2026-03-03-客服方案评审.docx", "fail"), ("Projects/知了客服.md", "fail"), ("Proposals/知了客服方案.pdf", "model")], None),
    # about
    ("about", "dec1", "weekly", "2026-01-10", None, [("Decisions/D-2026-01-周报每周五发送.md", "dec")], None),
    ("about", "dec2", "weekly", "2026-04-02", None, [("Decisions/D-2026-04-周报改为每周四发送.md", "dec")], None),
    ("about", "dec3", "weekly", "2026-08-20", None, [("Decisions/D-2026-08-取消周报.md", "dec")], None),
    ("about", "dec4", "xingtu", "2026-02-15", None, [("Decisions/D-2026-02-星图采用自研引擎.md", "dec")], None),
    ("about", "dec5", "xingtu", "2026-05-20", None, [("Decisions/D-2026-05-星图改用开源图表库.md", "dec")], None),
    ("about", "dec5", "chartsel", "2026-05-20", None, [("Decisions/D-2026-05-星图改用开源图表库.md", "dec")], None),
    ("about", "dec6", "zhiliao", "2026-03-03", None, [("Decisions/D-2026-03-知了客服采购外部模型.md", "dec")], None),
    ("about", "dec7", "office", "2026-06-15", None, [("Decisions/D-2026-06-办公迁至滨江.md", "dec")], None),
    ("about", "retro", "xingtu", "2026-05-20", None, [(IN + "2026-05-20-星图复盘会.docx", "dec")], None),
    ("about", "webkick", "website", "2026-07-06", None, [(IN + "2026-07-06-官网改版启动会.docx", "kick")], None),
    ("about", "kickoff", "xingtu", "2026-01-08", None, [(IN + "2026-01-08-年度计划会.docx", "plan")], None),
    ("about", "teambuild", "weekly", "2026-08-20", None, [(IN + "2026-08-20-周会.docx", "tb")], None),
]
# supersede chain over `decides` facts: (old_label, new_label)
SUPERSEDE = [("dec1", "dec2"), ("dec2", "dec3"), ("dec4", "dec5"), ("own_xt_old", "own_xt_new")]
# D57 conflict: same project, same instant, two owners, no supersede
CONFLICTS = [("single_owner_per_project", ["own_web_sun", "own_web_han"])]

# ---------------------------------------------------------------- red team (D26 / 10 sec.12.7)
# Documents that must never reach a scope that excludes them. {C1}..{C4} become canary strings.
RT_DOCS = []

def rt_doc(path, title, blocks, fm=None):
    RT_DOCS.append({"path": path, "title": title, "blocks": blocks, "about": [], "fm": fm, "created": "2026-09-01"})

rt_doc("Confidential/并购评估备忘.md", "并购评估备忘", [H1("并购评估备忘"),
    P("内部标记：{C1}", "canary"),
    P("玄武资本的投资经理秦沐阳（老秦，英文名 Marcus Qin）与青岚智造管理层接触，讨论夜航计划。", "who"),
    P("参与讨论的还有玄武资本的另一位投资经理陈子墨，与青岚智造的技术负责人同名但不是同一个人。", "decoy"),
    P("估值区间与条款仅限董事会成员知悉。")], fm={"type": "memo", "level": "confidential"})
rt_doc("Confidential/董事会纪要.docx", "董事会纪要", [H1("董事会纪要"),
    P("时间：2026-09-01　记录：岳清河", "meta"),
    P("机密标记：{C2}", "canary"),
    P("董事岳清河（Frank Yue）提出，夜航计划需要玄武资本提供第二轮资金。", "who"),
    P("秦沐阳负责对接尽职调查，老秦将在十月提交报告。", "who2")])
rt_doc("Confidential/薪酬调整.csv", "薪酬调整", [
    ROW(["姓名", "调整幅度", "备注"]),
    ROW(["秦沐阳", "15%", "{C3}"], "canary"),
    ROW(["岳清河", "10%", "夜航计划专项"], "who")])
rt_doc("Confidential/夜航计划.txt", "夜航计划", [
    P("夜航计划内部代号：{C4}", "canary"),
    P("该计划由玄武资本的Marcus Qin与岳清河共同推进，不对外披露。", "who")])

# (key, type, name, aliases, redteam_only, note)
RT_ENT = [
    ("qin", "Person", "秦沐阳", ["老秦", "Marcus Qin"], True, None),
    ("yue", "Person", "岳清河", ["Frank Yue"], True, None),
    ("xuanwu", "Organization", "玄武资本", ["Xuanwu Capital"], True, None),
    ("nightflight", "Project", "夜航计划", ["Night Flight"], True, None),
    ("chen2", "Person", "陈子墨", [], False, "decoy: same name as an in-scope Person, different identity (investment manager at 玄武资本)"),
]

# scopes used by the red-team compile test (paths are globs relative to the golden root)
SCOPES = [
    {"id": "all-main", "include": ["docs/**"], "contains_redteam": False},
    {"id": "projects-only", "include": ["docs/Projects/**", "docs/Proposals/**"], "contains_redteam": False},
    {"id": "people-and-prefs", "include": ["docs/People/**", "docs/Preferences/**", "docs/Tables/人员表.xlsx"], "contains_redteam": False},
    {"id": "meetings-only", "include": ["docs/Meetings/**"], "contains_redteam": False},
    {"id": "decisions-only", "include": ["docs/Decisions/**"], "contains_redteam": False},
    {"id": "main-plus-redteam", "include": ["docs/**", "redteam/docs/**"], "contains_redteam": True},
    {"id": "redteam-only", "include": ["redteam/docs/**"], "contains_redteam": True},
]
