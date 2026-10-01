#[derive(Clone, Copy, Debug)]
pub struct Col {
    pub name: &'static str,
    pub sql: &'static str,
    pub nullable: bool,
}

pub const fn col(name: &'static str, sql: &'static str, nullable: bool) -> Col {
    Col {
        name,
        sql,
        nullable,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Table {
    pub name: &'static str,
    pub source: &'static str,
    pub dialect: &'static str,
    pub columns: &'static [Col],
    pub pk: &'static [&'static str],
    pub file: &'static str,
}

pub struct DictRow {
    pub kind: &'static str,
    pub code: &'static str,
    pub from: &'static str,
    pub to: &'static str,
    pub label: &'static str,
}

const fn d(
    kind: &'static str,
    code: &'static str,
    from: &'static str,
    to: &'static str,
    label: &'static str,
) -> DictRow {
    DictRow {
        kind,
        code,
        from,
        to,
        label,
    }
}

pub static DICT: &[DictRow] = &[
    d("khzt", "0", "2000-01-01", "9999-12-31", "停用"),
    d("khzt", "1", "2000-01-01", "9999-12-31", "正常"),
    d("khzt", "2", "2000-01-01", "2023-12-31", "冻结"),
    d("khzt", "2", "2024-01-01", "9999-12-31", "停用待复核"),
    d("ydzt", "10", "2000-01-01", "9999-12-31", "在途"),
    d("ydzt", "20", "2000-01-01", "9999-12-31", "签收"),
    d("ydzt", "30", "2000-01-01", "9999-12-31", "异常"),
    d("hb", "CNY", "2000-01-01", "9999-12-31", "人民币"),
    d("hb", "USD", "2000-01-01", "9999-12-31", "美元"),
    d("hb", "EUR", "2000-01-01", "9999-12-31", "欧元"),
    d("jsdm", "ddy", "2000-01-01", "9999-12-31", "调度"),
    d("jsdm", "cw_east", "2000-01-01", "9999-12-31", "华东财务"),
    d("jsdm", "cw_west", "2000-01-01", "9999-12-31", "华西财务"),
    d("jsdm", "jl_east", "2000-01-01", "9999-12-31", "华东经理"),
    d("jsdm", "ck", "2000-01-01", "9999-12-31", "仓库"),
    d("jsdm", "sj", "2000-01-01", "9999-12-31", "审计"),
];

fn names(columns: &[Col]) -> Vec<&'static str> {
    columns.iter().map(|col| col.name).collect()
}

pub fn header(table: &Table) -> Vec<&'static str> {
    names(table.columns)
}

pub static KH: Table = Table {
    name: "kh",
    source: "mysql-erp",
    dialect: "mysql",
    pk: &["khid"],
    file: "source-inputs/mysql-erp/tables/kh.csv",
    columns: &[
        col("khid", "varchar(32)", false),
        col("khmc", "varchar(200)", false),
        col("nsrsbh", "varchar(32)", true),
        col("szdq", "varchar(16)", false),
        col("zt", "tinyint", false),
        col("scbz", "tinyint", false),
        col("csbz", "tinyint", false),
        col("khfl", "varchar(8)", false),
        col("cjsj", "datetime", false),
        col("qdrq_raw", "char(10)", false),
        col("lxdh", "varchar(32)", true),
        col("hb", "char(3)", false),
        col("je_fen", "bigint", false),
        col("hlje", "decimal(40,12)", true),
        col("lsid", "bigint unsigned", false),
        col("khzt_enum", "enum('0','1','2')", false),
        col("khbq", "set('vip','delay','trial')", false),
        col("bz1", "varchar(500)", true),
        col("ext_json", "json", true),
    ],
};

pub static KH_BM: Table = Table {
    name: "kh_bm",
    source: "mysql-erp",
    dialect: "mysql",
    pk: &["khid", "bmxh"],
    file: "source-inputs/mysql-erp/tables/kh_bm.csv",
    columns: &[
        col("khid", "varchar(32)", false),
        col("bmxh", "int", false),
        col("bm", "varchar(200)", false),
    ],
};

pub static HT: Table = Table {
    name: "ht",
    source: "mysql-erp",
    dialect: "mysql",
    pk: &["htid", "bb"],
    file: "source-inputs/mysql-erp/tables/ht.csv",
    columns: &[
        col("htid", "varchar(32)", false),
        col("bb", "int", false),
        col("htbh", "varchar(64)", false),
        col("khid", "varchar(32)", false),
        col("qdrq", "date", false),
        col("jsrq", "date", false),
        col("je_fen", "bigint", false),
        col("hb", "char(3)", false),
        col("zt", "tinyint", false),
    ],
};

pub static DD: Table = Table {
    name: "dd",
    source: "mysql-erp",
    dialect: "mysql",
    pk: &["ddid"],
    file: "source-inputs/mysql-erp/tables/dd.csv",
    columns: &[
        col("ddid", "varchar(32)", false),
        col("khid", "varchar(32)", false),
        col("htid", "varchar(32)", false),
        col("bb", "int", false),
        col("ddrq", "date", false),
        col("je_fen", "bigint", false),
        col("hb", "char(3)", false),
        col("zt", "varchar(8)", false),
        col("bz1", "varchar(200)", true),
        col("ext_json", "json", true),
    ],
};

pub static YS: Table = Table {
    name: "ys",
    source: "mysql-erp",
    dialect: "mysql",
    pk: &["ysid"],
    file: "source-inputs/mysql-erp/tables/ys.csv",
    columns: &[
        col("ysid", "varchar(32)", false),
        col("ddid", "varchar(32)", false),
        col("khid", "varchar(32)", false),
        col("ysje_fen", "bigint", false),
        col("kjq", "char(7)", false),
        col("zt", "varchar(8)", false),
    ],
};

pub static SK: Table = Table {
    name: "sk",
    source: "mysql-erp",
    dialect: "mysql",
    pk: &["skid"],
    file: "source-inputs/mysql-erp/tables/sk.csv",
    columns: &[
        col("skid", "varchar(32)", false),
        col("ysid", "varchar(32)", false),
        col("ddid", "varchar(32)", false),
        col("khid", "varchar(32)", false),
        col("skje_fen", "bigint", false),
        col("skrq", "date", false),
        col("jzbz", "tinyint", false),
        col("hb", "char(3)", false),
        col("zt", "varchar(8)", false),
    ],
};

pub static TK: Table = Table {
    name: "tk",
    source: "mysql-erp",
    dialect: "mysql",
    pk: &["tkid"],
    file: "source-inputs/mysql-erp/tables/tk.csv",
    columns: &[
        col("tkid", "varchar(32)", false),
        col("skid", "varchar(32)", false),
        col("ddid", "varchar(32)", false),
        col("tkje_fen", "bigint", false),
        col("tkrq", "date", false),
        col("hb", "char(3)", false),
    ],
};

pub static FP: Table = Table {
    name: "fp",
    source: "mysql-erp",
    dialect: "mysql",
    pk: &["fpid"],
    file: "source-inputs/mysql-erp/tables/fp.csv",
    columns: &[
        col("fpid", "varchar(32)", false),
        col("ysid", "varchar(32)", false),
        col("ddid", "varchar(32)", false),
        col("khid", "varchar(32)", false),
        col("fpje_fen", "bigint", false),
        col("kprq", "date", false),
        col("hb", "char(3)", false),
        col("zt", "varchar(8)", false),
    ],
};

pub static ZD: Table = Table {
    name: "zd",
    source: "mysql-erp",
    dialect: "mysql",
    pk: &["zdlx", "zddm", "yxqq"],
    file: "source-inputs/mysql-erp/tables/zd.csv",
    columns: &[
        col("zdlx", "varchar(16)", false),
        col("zddm", "varchar(16)", false),
        col("yxqq", "date", false),
        col("yxqz", "date", false),
        col("zdz", "varchar(64)", false),
    ],
};

pub static YH: Table = Table {
    name: "yh",
    source: "mysql-erp",
    dialect: "mysql",
    pk: &["yhid"],
    file: "source-inputs/mysql-erp/tables/yh.csv",
    columns: &[
        col("yhid", "varchar(32)", false),
        col("yhm", "varchar(64)", false),
        col("jsdm", "varchar(16)", false),
        col("szdq", "varchar(16)", false),
        col("sjhm", "varchar(32)", true),
        col("scbz", "tinyint", false),
    ],
};

pub static YD: Table = Table {
    name: "yd",
    source: "postgres-tms",
    dialect: "postgres",
    pk: &["ydh"],
    file: "source-inputs/postgres-tms/tables/yd.csv",
    columns: &[
        col("ydh", "text", false),
        col("erp_ddh", "text", false),
        col("kh_dm", "text", false),
        col("fhrq", "varchar(32)", false),
        col("sfje", "bigint", false),
        col("sj_id", "text", false),
        col("cph", "text", false),
        col("xl_bh", "text", false),
        col("ck_dm", "text", false),
        col("zt", "varchar(8)", false),
        col("bz", "text", true),
        col("ext_json", "jsonb", true),
    ],
};

pub static YD_MX: Table = Table {
    name: "yd_mx",
    source: "postgres-tms",
    dialect: "postgres",
    pk: &["ydh", "hh"],
    file: "source-inputs/postgres-tms/tables/yd_mx.csv",
    columns: &[
        col("ydh", "text", false),
        col("hh", "integer", false),
        col("hwmc", "text", false),
        col("js", "integer", false),
        col("zl_kg", "numeric(12,3)", false),
        col("je_fen", "bigint", false),
    ],
};

pub static GJ: Table = Table {
    name: "gj",
    source: "postgres-tms",
    dialect: "postgres",
    pk: &[],
    file: "source-inputs/postgres-tms/shards",
    columns: &[
        col("ydh", "text", false),
        col("evt_no", "bigint", false),
        col("sjsj", "varchar(32)", false),
        col("tzmc", "varchar(64)", false),
        col("sjc_text", "varchar(40)", false),
        col("ms", "text", true),
        col("ext_json", "jsonb", true),
    ],
};

pub static CL: Table = Table {
    name: "cl",
    source: "postgres-tms",
    dialect: "postgres",
    pk: &["cph"],
    file: "source-inputs/postgres-tms/tables/cl.csv",
    columns: &[
        col("cph", "text", false),
        col("cllx", "text", false),
        col("zt", "smallint", false),
        col("szdq", "text", false),
    ],
};

pub static SJ: Table = Table {
    name: "sj",
    source: "postgres-tms",
    dialect: "postgres",
    pk: &["sjid"],
    file: "source-inputs/postgres-tms/tables/sj.csv",
    columns: &[
        col("sjid", "text", false),
        col("xm", "text", false),
        col("sfzh", "text", true),
        col("sjhm", "text", true),
        col("cph", "text", false),
        col("zt", "smallint", false),
        col("szdq", "text", false),
    ],
};

pub static XL: Table = Table {
    name: "xl",
    source: "postgres-tms",
    dialect: "postgres",
    pk: &["xlbh"],
    file: "source-inputs/postgres-tms/tables/xl.csv",
    columns: &[
        col("xlbh", "text", false),
        col("qd", "text", false),
        col("zd", "text", false),
        col("lc_km", "numeric(8,1)", false),
        col("szdq", "text", false),
    ],
};

pub static CK: Table = Table {
    name: "ck",
    source: "postgres-tms",
    dialect: "postgres",
    pk: &["ckid"],
    file: "source-inputs/postgres-tms/tables/ck.csv",
    columns: &[
        col("ckid", "text", false),
        col("ckmc", "text", false),
        col("dq_dm", "text", false),
        col("zt", "smallint", false),
    ],
};

pub static KC: Table = Table {
    name: "kc",
    source: "postgres-tms",
    dialect: "postgres",
    pk: &["ckid", "sku", "yf"],
    file: "source-inputs/postgres-tms/tables/kc.csv",
    columns: &[
        col("ckid", "text", false),
        col("sku", "text", false),
        col("yf", "char(7)", false),
        col("sl", "integer", false),
        col("je_fen", "bigint", false),
    ],
};

pub static TXN: Table = Table {
    name: "txn_log",
    source: "workload",
    dialect: "csv",
    pk: &[],
    file: "source-inputs/workload/txn_log.csv",
    columns: &[
        col("epoch", "bigint", false),
        col("txn_id", "text", false),
        col("seq", "bigint", false),
        col("op", "text", false),
        col("rel", "text", false),
        col("pk_text", "text", false),
        col("pos", "bigint", false),
        col("note", "text", false),
    ],
};

static MYSQL_TABLES: [&Table; 10] = [&KH, &KH_BM, &HT, &DD, &YS, &SK, &TK, &FP, &ZD, &YH];
static POSTGRES_TABLES: [&Table; 8] = [&YD, &YD_MX, &GJ, &CL, &SJ, &XL, &CK, &KC];

pub fn mysql_tables() -> &'static [&'static Table] {
    &MYSQL_TABLES
}

pub fn postgres_tables() -> &'static [&'static Table] {
    &POSTGRES_TABLES
}

pub fn col_index(table: &Table, name: &str) -> Option<usize> {
    table.columns.iter().position(|col| col.name == name)
}

pub const BUSINESS_TABLES: &[&str] = &[
    "kh", "kh_bm", "ht", "dd", "ys", "sk", "tk", "fp", "zd", "yh", "yd", "yd_mx", "gj", "cl",
    "sj", "xl", "ck", "kc",
];

const MONEY_COLUMNS: &[&str] = &["je_fen", "ysje_fen", "skje_fen", "tkje_fen", "fpje_fen", "sfje"];

pub fn is_money_column(name: &str) -> bool {
    MONEY_COLUMNS.contains(&name)
}

/// `cents` on keeps the catalog type. `cents` off uses a dialect-specific
/// exact scale-2 type on the money columns only. `hlje` and `kc.sl` stay.
pub fn effective_sql(dialect: &str, column: &Col, integer_fen: bool) -> &'static str {
    if integer_fen || !is_money_column(column.name) {
        return column.sql;
    }
    match dialect {
        "mysql" => "decimal(20,2)",
        "postgres" => "numeric(20,2)",
        _ => column.sql,
    }
}
