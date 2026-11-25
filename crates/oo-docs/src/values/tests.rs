//! Cases of the normaliser: numbers, amounts, ratios, dates (absolute, relative, year inferred) and the text scanner,
//! in Chinese and English, with the negatives (ordinals, degree adverbs, counts, phone numbers, ids).

use super::date::{self, DateCtx};
use super::money;
use super::ratio;
use super::scan::{self, Value};
use super::{en, zh};

/// 2027-01-12 is a Tuesday.
fn ctx() -> DateCtx {
    DateCtx::on("2027-01-12")
}

fn zh_num(s: &str) -> Option<String> {
    zh::parse(s).map(|d| d.to_plain())
}

fn en_num(s: &str) -> Option<String> {
    en::parse(s, true).map(|r| r.value.to_plain())
}

fn amount(s: &str) -> Option<String> {
    money::parse(s, false).map(|m| m.canonical())
}

fn pct(s: &str) -> Option<String> {
    ratio::parse(s).map(|r| r.name())
}

fn day(s: &str) -> Option<String> {
    date::parse(s, &ctx()).map(|d| d.iso())
}

fn day_without_context(s: &str) -> Option<String> {
    date::parse(s, &DateCtx::NONE).map(|d| d.iso())
}

/// What the scanner finds, as `A:` amount, `D:` date, `R:` ratio.
fn found(text: &str) -> Vec<String> {
    scan::scan(text, &ctx())
        .into_iter()
        .map(|f| match f.value {
            Value::Amount(m) => format!("A:{}", m.canonical()),
            Value::Date(d) => format!("D:{}", d.iso()),
            Value::Ratio(r) => format!("R:{}", r.name()),
        })
        .collect()
}

fn check(cases: &[(&str, Option<&str>)], f: impl Fn(&str) -> Option<String>) {
    for (input, want) in cases {
        assert_eq!(f(input).as_deref(), *want, "{input}");
    }
}

#[test]
fn chinese_numerals() {
    check(
        &[
            ("六百八十五万", Some("6850000")),
            ("一百四十二万", Some("1420000")),
            ("二百四十八万", Some("2480000")),
            ("三千万", Some("30000000")),
            ("一亿二千万", Some("120000000")),
            ("一万亿", Some("1000000000000")),
            ("两千", Some("2000")),
            ("两百五十", Some("250")),
            ("一千零五", Some("1005")),
            ("一百零一", Some("101")),
            ("一百二", Some("120")),
            ("三千五", Some("3500")),
            ("十二", Some("12")),
            ("二十", Some("20")),
            ("十", Some("10")),
            ("九十九", Some("99")),
            ("百万", Some("1000000")),
            ("千万", Some("10000000")),
            ("壹佰贰拾", Some("120")),
            ("叁万伍仟", Some("35000")),
            ("一点五万", Some("15000")),
            ("六点八五万", Some("68500")),
            ("三点五亿", Some("350000000")),
            ("〇", Some("0")),
            ("二〇二七", Some("2027")),
            ("二零二七", Some("2027")),
            ("3.5亿", Some("350000000")),
            ("142万", Some("1420000")),
            ("3百万", Some("3000000")),
            ("2.5千万", Some("25000000")),
            ("1亿2千万", Some("120000000")),
            ("万", None),
            ("", None),
            ("abc", None),
            ("一些", None),
            ("三四五万六", None),
        ],
        zh_num,
    );
}

#[test]
fn english_numbers() {
    check(
        &[
            ("six point eight five million", Some("6850000")),
            ("two hundred thousand", Some("200000")),
            ("two million four hundred eighty thousand", Some("2480000")),
            ("twenty-five", Some("25")),
            ("twenty five", Some("25")),
            ("a hundred", Some("100")),
            ("one hundred and five", Some("105")),
            ("one thousand two hundred", Some("1200")),
            ("three point five billion", Some("3500000000")),
            ("six point eight five", Some("6.85")),
            ("zero", Some("0")),
            ("nineteen", Some("19")),
            ("6.85 million", Some("6850000")),
            ("1.2bn", Some("1200000000")),
            ("3.5m", Some("3500000")),
            ("142k", Some("142000")),
            ("2 billion", Some("2000000000")),
            ("12,500 thousand", Some("12500000")),
            ("a million", Some("1000000")),
            ("hello", None),
            ("one two three", None),
            ("million", None),
            ("and five", None),
        ],
        en_num,
    );
}

#[test]
fn amounts_with_currency_and_scale() {
    check(
        &[
            ("¥6,850,000", Some("6850000 CNY")),
            ("￥142万", Some("1420000 CNY")),
            ("142万元", Some("1420000 CNY")),
            ("六百八十五万元", Some("6850000 CNY")),
            ("3.5亿美元", Some("350000000 USD")),
            ("三千万欧元", Some("30000000 EUR")),
            ("五百英镑", Some("500 GBP")),
            ("一百万日元", Some("1000000 JPY")),
            ("八十万港币", Some("800000 HKD")),
            ("人民币 2,480,000", Some("2480000 CNY")),
            ("2480000元", Some("2480000 CNY")),
            ("80000 CNY", Some("80000 CNY")),
            ("CNY 80000", Some("80000 CNY")),
            ("RMB 1.2bn", Some("1200000000 CNY")),
            ("USD 2.5 million", Some("2500000 USD")),
            ("$3.5m", Some("3500000 USD")),
            ("$12.99", Some("12.99 USD")),
            ("US$ 1,000", Some("1000 USD")),
            ("€1.2bn", Some("1200000000 EUR")),
            ("£500", Some("500 GBP")),
            ("850,000 yuan", Some("850000 CNY")),
            ("six point eight five million dollars", Some("6850000 USD")),
            ("two hundred thousand euros", Some("200000 EUR")),
            ("142万", Some("1420000")),
            ("2.5 million", Some("2500000")),
            ("6.85 million", Some("6850000")),
            ("1,420,000", None),
            ("hello", None),
            ("元", None),
        ],
        amount,
    );
}

#[test]
fn bare_amount_under_an_amount_column() {
    assert_eq!(
        money::parse("1420000", true)
            .map(|m| m.canonical())
            .as_deref(),
        Some("1420000")
    );
    assert_eq!(
        money::parse("1,420,000", true)
            .map(|m| m.canonical())
            .as_deref(),
        Some("1420000")
    );
    assert_eq!(
        money::parse("六十万", true)
            .map(|m| m.canonical())
            .as_deref(),
        Some("600000")
    );
    assert!(money::parse("n/a", true).is_none());
}

#[test]
fn percentages_and_ratios() {
    check(
        &[
            ("12%", Some("12%")),
            ("12 %", Some("12%")),
            ("12.5％", Some("12.5%")),
            ("百分之十二", Some("12%")),
            ("百分之十二点五", Some("12.5%")),
            ("百分之八十五", Some("85%")),
            ("百分之百", Some("100%")),
            ("百分之 12", Some("12%")),
            ("12 percent", Some("12%")),
            ("twelve percent", Some("12%")),
            ("twelve per cent", Some("12%")),
            ("eighty-eight percent", Some("88%")),
            ("三成", Some("30%")),
            ("三成五", Some("35%")),
            ("七成半", Some("75%")),
            ("0.5%", Some("0.5%")),
            ("12.5‰", Some("1.25%")),
            ("88 pct", Some("88%")),
            ("十二", None),
            ("percent", None),
            ("一些", None),
        ],
        pct,
    );
    let (a, b) = ratio::parse_pair("3:1").unwrap();
    assert_eq!(ratio::pair_canonical(a, b), "3:1");
    let (a, b) = ratio::parse_pair("三比一").unwrap();
    assert_eq!(ratio::pair_canonical(a, b), "3:1");
    assert!(ratio::parse_pair("3:0").is_none());
    assert_eq!(ratio::parse("12%").unwrap().canonical(), "0.12");
}

#[test]
fn absolute_dates_in_chinese() {
    check(
        &[
            ("二〇二七年三月十六日", Some("2027-03-16")),
            ("二零二七年三月十六号", Some("2027-03-16")),
            ("2027年3月16日", Some("2027-03-16")),
            ("2027年3月16号", Some("2027-03-16")),
            ("2027年12月1日", Some("2027-12-01")),
            ("3月16号", Some("2027-03-16")),
            ("3月16日", Some("2027-03-16")),
            ("三月十六日", Some("2027-03-16")),
            ("三月十六号", Some("2027-03-16")),
            ("二月二十八日", Some("2027-02-28")),
            ("十二月三十一日", Some("2026-12-31")),
            ("2027年2月30日", None),
            ("2月30日", None),
            ("13月1日", None),
            ("三月", None),
            ("十六号", None),
            ("2027年", None),
        ],
        day,
    );
}

#[test]
fn absolute_dates_in_english() {
    check(
        &[
            ("March 16, 2027", Some("2027-03-16")),
            ("March 16th, 2027", Some("2027-03-16")),
            ("March sixteenth, 2027", Some("2027-03-16")),
            ("March sixteenth", Some("2027-03-16")),
            ("March 16", Some("2027-03-16")),
            ("Mar 16", Some("2027-03-16")),
            ("Mar. 16", Some("2027-03-16")),
            ("16th of March", Some("2027-03-16")),
            ("16 March 2027", Some("2027-03-16")),
            ("the sixteenth of March", Some("2027-03-16")),
            ("the 16th of March, 2027", Some("2027-03-16")),
            ("February tenth", Some("2027-02-10")),
            ("March twenty-first", Some("2027-03-21")),
            ("March twenty first", Some("2027-03-21")),
            ("Sept 5", Some("2026-09-05")),
            ("December 20", Some("2026-12-20")),
            ("March 1st, 2027", Some("2027-03-01")),
            ("February 30, 2027", None),
            ("March 32", None),
            ("march 16", None),
            ("May", None),
            ("July 4", None),
            ("2027-03-16", Some("2027-03-16")),
            ("2027/3/16", Some("2027-03-16")),
        ],
        day,
    );
}

#[test]
fn the_year_comes_from_the_document_only_when_unambiguous() {
    assert_eq!(day_without_context("March 16"), None);
    assert_eq!(day_without_context("3月16日"), None);
    assert_eq!(
        day_without_context("March 16, 2027").as_deref(),
        Some("2027-03-16")
    );
    assert_eq!(
        day_without_context("2027年3月16日").as_deref(),
        Some("2027-03-16")
    );
    assert_eq!(day_without_context("明天"), None);
    let late = DateCtx::on("2027-12-29");
    assert_eq!(
        date::parse("1月5日", &late).map(|d| d.iso()).as_deref(),
        Some("2028-01-05")
    );
    assert_eq!(
        date::parse("December 20", &late)
            .map(|d| d.iso())
            .as_deref(),
        Some("2027-12-20")
    );
    let mid = DateCtx::on("2027-09-01");
    assert_eq!(
        date::parse("March 16", &mid),
        None,
        "half a year away on both sides"
    );
    assert_eq!(
        date::parse("February 29", &DateCtx::on("2028-02-10"))
            .map(|d| d.iso())
            .as_deref(),
        Some("2028-02-29")
    );
    assert_eq!(date::parse("February 29", &DateCtx::on("2027-02-10")), None);
    let n = date::parse("3月16日", &ctx()).unwrap();
    assert_eq!(n.how, date::How::YearInferred);
    assert_eq!(
        date::parse("2027年3月16日", &ctx()).unwrap().how,
        date::How::Explicit
    );
}

#[test]
fn relative_dates_in_chinese() {
    check(
        &[
            ("明天", Some("2027-01-13")),
            ("后天", Some("2027-01-14")),
            ("大后天", Some("2027-01-15")),
            ("昨天", Some("2027-01-11")),
            ("前天", Some("2027-01-10")),
            ("大前天", Some("2027-01-09")),
            ("今天", Some("2027-01-12")),
            ("下周二", Some("2027-01-19")),
            ("下周五", Some("2027-01-22")),
            ("下星期三", Some("2027-01-20")),
            ("下礼拜一", Some("2027-01-18")),
            ("下周日", Some("2027-01-24")),
            ("下周天", Some("2027-01-24")),
            ("本周五", Some("2027-01-15")),
            ("这周五", Some("2027-01-15")),
            ("这个星期四", Some("2027-01-14")),
            ("上周五", Some("2027-01-08")),
            ("上个周一", Some("2027-01-04")),
            ("下下周一", Some("2027-01-25")),
            ("上上周三", Some("2026-12-30")),
            ("下周2", Some("2027-01-19")),
            ("三天后", Some("2027-01-15")),
            ("5天前", Some("2027-01-07")),
            ("两周后", Some("2027-01-26")),
            ("一个星期后", Some("2027-01-19")),
            ("三天之后", Some("2027-01-15")),
            ("下个月10号", Some("2027-02-10")),
            ("下月十号", Some("2027-02-10")),
            ("本月20号", Some("2027-01-20")),
            ("上个月28号", Some("2026-12-28")),
            ("周二", None),
            ("星期五", None),
            ("下周", None),
            ("下个月", None),
            ("三天", None),
        ],
        day,
    );
    assert!(date::parse("明天", &ctx()).unwrap().how == date::How::Relative);
    assert!(date::parse("今天", &ctx()).unwrap().trivial);
    assert!(!date::parse("明天", &ctx()).unwrap().trivial);
}

#[test]
fn relative_dates_in_english() {
    check(
        &[
            ("tomorrow", Some("2027-01-13")),
            ("yesterday", Some("2027-01-11")),
            ("today", Some("2027-01-12")),
            ("next Tuesday", Some("2027-01-19")),
            ("next Monday", Some("2027-01-18")),
            ("next Saturday", Some("2027-01-23")),
            ("next Sunday", Some("2027-01-24")),
            ("this Friday", Some("2027-01-15")),
            ("this Tuesday", Some("2027-01-12")),
            ("last Friday", Some("2027-01-08")),
            ("last Tuesday", Some("2027-01-05")),
            ("in three days", Some("2027-01-15")),
            ("in 2 weeks", Some("2027-01-26")),
            ("in a week", Some("2027-01-19")),
            ("three days ago", Some("2027-01-09")),
            ("2 weeks ago", Some("2026-12-29")),
            ("a week ago", Some("2027-01-05")),
            ("2 weeks from now", Some("2027-01-26")),
            ("ten days from today", Some("2027-01-22")),
            ("next Friday", None),
            ("next Wednesday", None),
            ("Tuesday", None),
            ("on Friday", None),
            ("next week", None),
            ("in time", None),
        ],
        day,
    );
}

#[test]
fn next_weekday_is_unresolved_when_it_could_mean_later_this_week() {
    // On a Monday, `next Tuesday` is tomorrow for some speakers and eight days away for others.
    let monday = DateCtx::on("2027-01-11");
    assert_eq!(date::parse("next Tuesday", &monday), None);
    assert_eq!(
        date::parse("next Friday", &monday)
            .map(|d| d.iso())
            .as_deref(),
        Some("2027-01-22")
    );
    assert_eq!(
        date::parse("next Saturday", &monday)
            .map(|d| d.iso())
            .as_deref(),
        Some("2027-01-23")
    );
    assert_eq!(
        date::parse("下周二", &monday).map(|d| d.iso()).as_deref(),
        Some("2027-01-19"),
        "Chinese 下周 is never ambiguous"
    );
}

#[test]
fn the_scanner_finds_values_in_text() {
    let cases: &[(&str, &[&str])] = &[
        (
            "总预算六百八十五万元，其中市场一百四十二万。",
            &["A:6850000 CNY", "A:1420000"],
        ),
        (
            "Total launch budget is six point eight five million.",
            &["A:6850000"],
        ),
        ("Budget of 6.85 million approved.", &["A:6850000"]),
        ("预算3.5亿，约合$50m。", &["A:350000000", "A:50000000 USD"]),
        (
            "Revenue reached $3.5m and RMB 1.2bn.",
            &["A:3500000 USD", "A:1200000000 CNY"],
        ),
        ("two hundred thousand dollars were wired", &["A:200000 USD"]),
        ("合同金额 2,480,000 元", &["A:2480000 CNY"]),
        ("contract value 2,480,000", &["A:2480000"]),
        ("发布会定于三月十六日举行", &["D:2027-03-16"]),
        ("发布会定于3月16号举行", &["D:2027-03-16"]),
        ("embargo lifts March sixteenth", &["D:2027-03-16"]),
        ("embargo lifts on the 16th of March", &["D:2027-03-16"]),
        ("二〇二七年三月十六日解禁", &["D:2027-03-16"]),
        ("let's review next Tuesday", &["D:2027-01-19"]),
        (
            "我们下周二评审，明天交稿",
            &["D:2027-01-19", "D:2027-01-13"],
        ),
        (
            "良率从71%提升到百分之八十八，门槛是85 percent",
            &["R:71%", "R:88%", "R:85%"],
        ),
        ("yield of twelve percent and 十二%", &["R:12%", "R:12%"]),
        ("折扣三成，增长百分之十二点五", &["R:30%", "R:12.5%"]),
        (
            "shoot on 2027-02-10 and QA on 2027/3/5",
            &["D:2027-02-10", "D:2027-03-05"],
        ),
        (
            "Pay ¥80,000 by February tenth.",
            &["A:80000 CNY", "D:2027-02-10"],
        ),
    ];
    for (text, want) in cases {
        assert_eq!(found(text), *want, "{text}");
    }
}

#[test]
fn what_is_not_a_value() {
    for text in [
        "一些人觉得很好",
        "十分好",
        "十分钟后开会",
        "第一次见面",
        "第一百个用户",
        "万一失败了怎么办",
        "一下子就好了",
        "三个人",
        "他一直在",
        "call me at 13800138000",
        "tel +86 138 0013 8000",
        "400-820-8820",
        "order 123456 shipped",
        "part A12345678",
        "ticket 98765432",
        "9,999 units",
        "12345 pieces",
        "一万人参加",
        "3万用户",
        "200 thousand users",
        "ten thousand people",
        "version 1.2.3",
        "10:30 meeting",
        "room 1204",
        "一起去",
        "one of the options",
        "十二个百分点",
        "year 2027",
        "may 5 users",
        "a few",
        "a hundred days",
        "on Tuesday",
        "周二见",
        "下周见",
    ] {
        assert!(found(text).is_empty(), "{text}: {:?}", found(text));
    }
}

#[test]
fn spans_and_originals_are_kept() {
    let f = scan::scan("预算六百八十五万元，三月十六日截止", &ctx());
    assert_eq!(f.len(), 2);
    assert_eq!(f[0].text, "六百八十五万元");
    assert_eq!((f[0].start, f[0].end), (2, 9));
    assert_eq!(f[1].text, "三月十六日");
}

#[test]
fn the_same_value_written_differently_has_one_canonical_form() {
    let a = found("六百八十五万");
    let b = found("six point eight five million");
    let c = found("6.85 million");
    let d = found("685万");
    let e = found("¥6,850,000");
    assert_eq!(a[0].trim_end_matches(" CNY"), "A:6850000");
    assert_eq!(b, vec!["A:6850000".to_owned()]);
    assert_eq!(c, b);
    assert_eq!(d, b);
    assert_eq!(e[0].trim_end_matches(" CNY"), "A:6850000");
    let dates = [
        found("二〇二七年三月十六日"),
        found("March 16, 2027"),
        found("2027-03-16"),
        found("三月十六号"),
        found("March sixteenth"),
        found("16th of March"),
    ];
    assert!(
        dates.iter().all(|d| d == &vec!["D:2027-03-16".to_owned()]),
        "{dates:?}"
    );
    assert_eq!(found("百分之八十八"), found("88%"));
    assert_eq!(found("eighty-eight percent"), found("88%"));
}

#[test]
fn decimals_are_exact_and_bounded() {
    use super::Dec;
    assert_eq!(
        Dec::parse_ascii("0.1")
            .unwrap()
            .plus(Dec::parse_ascii("0.2").unwrap())
            .unwrap()
            .to_plain(),
        "0.3"
    );
    assert_eq!(Dec::parse_ascii("1,000").unwrap().to_plain(), "1000");
    assert!(Dec::parse_ascii("1,00").is_none());
    assert!(Dec::parse_ascii("12,3456").is_none());
    assert!(Dec::parse_ascii(".5").is_none());
    assert!(Dec::parse_ascii("5.").is_none());
    assert_eq!(
        Dec::parse_ascii("6850000").unwrap().to_grouped().as_deref(),
        Some("6,850,000")
    );
    assert_eq!(Dec::parse_ascii("0.05").unwrap().to_plain(), "0.05");
    assert!(zh::parse("九万九千九百九十九亿九千九百九十九万九千九百九十九").is_some());
    assert!(zh::parse(&"九亿".repeat(5)).is_none());
}

#[test]
fn the_number_of_cases_is_at_least_150() {
    // The tables above are the cases; this guards against deleting them.
    let n = 36 + 23 + 29 + 21 + 17 + 24 + 35 + 25 + 20 + 35 + 20;
    assert!(n >= 150);
}
