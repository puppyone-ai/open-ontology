//! A small pinyin table for personal names (10 §5.4, cross-language names). Pure.
//!
//! Only corroborates: a Chinese name whose pinyin equals a Latin name is evidence for "same person", never proof
//! (two people can share a pinyin). A character not in the table makes the whole name unknown, never a guess.
//! Tones are not written; `ü` is `u` (`lv` and `lu` both read `lu`), as in passport romanisation.

use std::collections::HashMap;
use std::sync::OnceLock;

/// `char:pinyin` pairs, surnames first, then common given-name characters.
const TABLE: &str = "\
王:wang 李:li 张:zhang 刘:liu 陈:chen 杨:yang 黄:huang 赵:zhao 吴:wu 周:zhou 徐:xu 孙:sun 马:ma 朱:zhu 胡:hu 郭:guo 何:he 高:gao 林:lin \
罗:luo 郑:zheng 梁:liang 谢:xie 宋:song 唐:tang 许:xu 韩:han 冯:feng 邓:deng 曹:cao 彭:peng 曾:zeng 肖:xiao 萧:xiao 田:tian 董:dong 袁:yuan \
潘:pan 于:yu 蒋:jiang 蔡:cai 余:yu 杜:du 叶:ye 程:cheng 苏:su 魏:wei 吕:lu 丁:ding 任:ren 沈:shen 姚:yao 卢:lu 姜:jiang 崔:cui 钟:zhong \
谭:tan 陆:lu 汪:wang 范:fan 金:jin 石:shi 廖:liao 贾:jia 夏:xia 韦:wei 付:fu 方:fang 白:bai 邹:zou 孟:meng 熊:xiong 秦:qin 邱:qiu \
江:jiang 尹:yin 薛:xue 闫:yan 段:duan 雷:lei 侯:hou 龙:long 史:shi 陶:tao 黎:li 贺:he 顾:gu 毛:mao 郝:hao 龚:gong 邵:shao 万:wan \
钱:qian 严:yan 覃:qin 武:wu 戴:dai 莫:mo 孔:kong 向:xiang 汤:tang 乔:qiao 柯:ke 倪:ni 柳:liu 康:kang 洪:hong 庞:pang 常:chang \
欧阳:ouyang 司马:sima 上官:shangguan 诸葛:zhuge 慕容:murong \
明:ming 远:yuan 若:ruo 冰:bing 婉:wan 清:qing 子:zi 谦:qian 天:tian 佑:you 伟:wei 芳:fang 娜:na 敏:min 静:jing 丽:li 强:qiang 磊:lei \
军:jun 洋:yang 勇:yong 艳:yan 杰:jie 涛:tao 超:chao 秀:xiu 英:ying 华:hua 慧:hui 玉:yu 萍:ping 红:hong 娟:juan 鹏:peng 辉:hui 刚:gang \
平:ping 霞:xia 燕:yan 玲:ling 桂:gui 云:yun 志:zhi 浩:hao 宇:yu 轩:xuan 然:ran 欣:xin 怡:yi 梓:zi 涵:han 睿:rui 思:si 雨:yu 晨:chen \
阳:yang 博:bo 文:wen 建:jian 国:guo 俊:jun 凯:kai 昊:hao 泽:ze 嘉:jia 诗:shi 悦:yue 琪:qi 佳:jia 宁:ning 安:an 晓:xiao 东:dong 南:nan \
北:bei 海:hai 山:shan 川:chuan 峰:feng 飞:fei 翔:xiang 鑫:xin 君:jun 凤:feng 兰:lan 梅:mei 莉:li 琳:lin 倩:qian 薇:wei 颖:ying 洁:jie \
雪:xue 月:yue 星:xing 光:guang 辰:chen 瑞:rui 祥:xiang 福:fu 德:de 仁:ren 义:yi 礼:li 智:zhi 信:xin 忠:zhong 孝:xiao 健:jian 乐:le \
新:xin 春:chun 秋:qiu 冬:dong 河:he 湖:hu 洲:zhou 岚:lan 菲:fei 妍:yan 彤:tong 瑶:yao 蕾:lei 蓉:rong 璐:lu 媛:yuan 婷:ting 茜:qian \
芸:yun 珊:shan 雯:wen 琦:qi 馨:xin 铭:ming 熙:xi 恒:heng 毅:yi 豪:hao 彬:bin 斌:bin 帆:fan 航:hang 泉:quan 柏:bai 松:song 青:qing \
永:yong 立:li 成:cheng 达:da 昌:chang 盛:sheng 兴:xing 世:shi 宏:hong 力:li 家:jia 学:xue 勤:qin 美:mei 贞:zhen 淑:shu 珍:zhen \
翠:cui 巧:qiao 小:xiao 大:da 中:zhong 正:zheng 和:he 景:jing 元:yuan 全:quan 保:bao 利:li 才:cai 汉:han 孟:meng 伯:bo 仲:zhong";

fn table() -> &'static HashMap<&'static str, &'static str> {
    static T: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    T.get_or_init(|| {
        TABLE
            .split_whitespace()
            .filter_map(|p| p.split_once(':'))
            .collect()
    })
}

/// A character of the CJK Unified Ideographs block.
pub fn is_han(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}')
}

/// `(surname, given)` pinyin of a Chinese personal name (2 to 4 Han characters, no other letters); `None` when a
/// character is not in the table.
pub fn name_pinyin(name: &str) -> Option<(String, String)> {
    let chars: Vec<char> = name.chars().filter(|c| !c.is_whitespace()).collect();
    if !(2..=4).contains(&chars.len()) || !chars.iter().all(|c| is_han(*c)) {
        return None;
    }
    let t = table();
    let one = |s: &[char]| -> Option<String> {
        s.iter()
            .map(|c| t.get(c.to_string().as_str()).copied())
            .collect::<Option<Vec<_>>>()
            .map(|v| v.concat())
    };
    // A two-character surname first (欧阳), then a one-character one.
    if chars.len() >= 3 {
        let two: String = chars[..2].iter().collect();
        if let Some(s) = t.get(two.as_str())
            && let Some(g) = one(&chars[2..])
        {
            return Some(((*s).to_owned(), g));
        }
    }
    Some((one(&chars[..1])?, one(&chars[1..])?))
}

/// Lower-case ASCII letters of a romanised name word: tone marks and diaeresis removed, `v` read as `u`.
fn plain(word: &str) -> String {
    word.chars()
        .filter_map(|c| match c {
            'ā' | 'á' | 'ǎ' | 'à' => Some('a'),
            'ē' | 'é' | 'ě' | 'è' => Some('e'),
            'ī' | 'í' | 'ǐ' | 'ì' => Some('i'),
            'ō' | 'ó' | 'ǒ' | 'ò' => Some('o'),
            'ū' | 'ú' | 'ǔ' | 'ù' | 'ǖ' | 'ǘ' | 'ǚ' | 'ǜ' | 'ü' => Some('u'),
            'v' | 'V' => Some('u'),
            c if c.is_ascii_alphabetic() => Some(c.to_ascii_lowercase()),
            _ => None,
        })
        .collect()
}

/// True when the Latin name is the pinyin of the Chinese name: the surname is one word and the given name is the
/// rest of the words joined (`Bai Ruobing`, `Ruo Bing Bai`, `Ruobing Bai`), in either order.
pub fn matches_latin(han: &str, latin: &str) -> bool {
    let Some((surname, given)) = name_pinyin(han) else {
        return false;
    };
    let words: Vec<String> = latin
        .split(|c: char| c.is_whitespace() || c == '-' || c == '·')
        .map(plain)
        .filter(|w| !w.is_empty())
        .collect();
    if words.len() < 2 {
        return false;
    }
    let (first, rest) = words.split_first().expect("two words");
    let (last, init) = words.split_last().expect("two words");
    (*first == surname && rest.concat() == given) || (*last == surname && init.concat() == given)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chinese_name_reads_as_surname_and_given() {
        assert_eq!(
            name_pinyin("白若冰"),
            Some(("bai".into(), "ruobing".into()))
        );
        assert_eq!(
            name_pinyin("欧阳明远"),
            Some(("ouyang".into(), "mingyuan".into()))
        );
        assert_eq!(name_pinyin("白"), None);
        assert_eq!(name_pinyin("白若冰Bai"), None);
        assert_eq!(
            name_pinyin("白龘"),
            None,
            "an unknown character is no guess"
        );
    }

    #[test]
    fn latin_matches_in_either_order_and_split_given_names() {
        assert!(matches_latin("白若冰", "Ruobing Bai"));
        assert!(matches_latin("白若冰", "Bai Ruobing"));
        assert!(matches_latin("白若冰", "Ruo-Bing Bai"));
        assert!(matches_latin("邵婉清", "Wanqing Shao"));
        assert!(
            !matches_latin("吕静", "Jing Lyu"),
            "the lyu spelling is not matched"
        );
        assert!(!matches_latin("白若冰", "Ruobin Bai"));
        assert!(!matches_latin("白若冰", "Bai"));
        assert!(!matches_latin("白若冰", "Ruobing Bao"));
    }
}
