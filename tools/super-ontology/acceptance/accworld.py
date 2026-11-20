"""Synthetic world of the acceptance folder: a fictional smart-glasses launch (all names invented).

Facts live here once; gen.py renders them into files and gold. No customer data.
"""
LAUNCH_OLD, LAUNCH_NEW = "2027-03-04", "2027-03-18"
TOTAL = 6850000
BUDGET = {  # category -> {line: amount}
    "Hardware": {"Lens (Hengding)": 2480000, "Frames": 620000},
    "Marketing": {"Video production": 580000, "Print": 140000, "Events": 400000, "Digital ads": 300000},
    "Logistics": {"Air freight": 520000, "Warehousing": 260000, "Last-mile": 200000},
    "Packaging": {"Retail box": 240000, "Inserts": 120000},
    "Certification and QA": {"Certification": 340000, "Field testing": 200000},
    "Contingency": {"Reserve": 450000},
}
CAT_ZH = {"Hardware": "硬件", "Marketing": "市场", "Logistics": "物流", "Packaging": "包装",
          "Certification and QA": "认证与质量", "Contingency": "备用金"}
SUM = {k: sum(v.values()) for k, v in BUDGET.items()}
assert sum(SUM.values()) == TOTAL and SUM["Marketing"] == 1420000 and SUM["Hardware"] == 3100000

# key: (zh, en, role_zh, role_en, team, email)
PEOPLE = {
    "gu": ("顾明远", "Mingyuan Gu", "产品副总裁", "VP Product", "Product", "mingyuan.gu@northport-optics.example"),
    "bai": ("白若冰", "Ruobing Bai", "项目经理", "Program Manager", "Program Office", "ruobing.bai@northport-optics.example"),
    "ortiz": ("丹尼尔·奥尔蒂斯", "Daniel Ortiz", "硬件工程负责人", "Hardware Lead", "Engineering", "daniel.ortiz@northport-optics.example"),
    "shao": ("邵婉清", "Wanqing Shao", "财务经理", "Finance Manager", "Finance", "wanqing.shao@northport-optics.example"),
    "raman": ("普丽娅·拉曼", "Priya Raman", "市场负责人", "Head of Marketing", "Marketing", "priya.raman@northport-optics.example"),
    "jiang": ("蒋天佑", "Tianyou Jiang", "供应链经理", "Supply Chain Manager", "Operations", "tianyou.jiang@northport-optics.example"),
    "cole": ("汉娜·科尔", "Hannah Cole", "法务顾问", "Legal Counsel", "Legal", "hannah.cole@northport-optics.example"),
    "ke": ("柯子骞", "Ziqian Ke", "质量工程师", "QA Engineer", "Quality", "ziqian.ke@northport-optics.example"),
    "mao": ("毛晓岚", "Xiaolan Mao", "设计主管", "Design Lead", "Design", "xiaolan.mao@northport-optics.example"),
}
VENDORS = {  # key: (zh, en, category, owner key, contract CNY, status)
    "hengding": ("恒鼎光电", "Hengding Optoelectronics", "Lens", "jiang", 2480000, "Signed 2027-01-20"),
    "meridian": ("子午线包装", "Meridian Packaging", "Packaging", "jiang", 240000, "Quoted"),
    "suda": ("速达快运", "Suda Express", "Logistics", "jiang", 980000, "Signed 2027-02-03"),
    "yunfan": ("云帆传播", "Yunfan Media", "Video and ads", "raman", 880000, "Signed 2027-01-27"),
}
COMPANY = ("北港光学", "Northport Optics")
PRODUCT = ("极光 NP-7", "Aurora NP-7")
