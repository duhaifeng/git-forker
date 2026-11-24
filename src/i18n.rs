//! 界面文案的多语言支持。
//!
//! 源码里的简体中文文案就是查找键，其他语言的译文放在 `locales/*.json`。
//! 缺少译文时退回简体中文，所以新增文案不会让界面出现空白。

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

/// 用户在设置里选择的语言。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Language {
    #[default]
    System,
    SimplifiedChinese,
    TraditionalChinese,
    English,
    Japanese,
}

/// 实际用于显示的语言。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    SimplifiedChinese,
    TraditionalChinese,
    English,
    Japanese,
}

impl Language {
    pub const ALL: [Language; 5] = [
        Language::System,
        Language::SimplifiedChinese,
        Language::TraditionalChinese,
        Language::English,
        Language::Japanese,
    ];

    /// 菜单里显示的名字。各语言用自己的写法，方便看不懂当前语言的人找到母语。
    pub fn native_name(self) -> &'static str {
        match self {
            Language::System => crate::tr!("跟随系统"),
            Language::SimplifiedChinese => "简体中文",
            Language::TraditionalChinese => "繁體中文",
            Language::English => "English",
            Language::Japanese => "日本語",
        }
    }

    pub fn resolve(self) -> Locale {
        match self {
            Language::System => system_locale(),
            Language::SimplifiedChinese => Locale::SimplifiedChinese,
            Language::TraditionalChinese => Locale::TraditionalChinese,
            Language::English => Locale::English,
            Language::Japanese => Locale::Japanese,
        }
    }
}

fn system_locale() -> Locale {
    sys_locale::get_locales()
        .find_map(|tag| parse_locale(&tag))
        .unwrap_or(Locale::English)
}

/// 把 BCP 47 语言标签（例如 `zh-Hant-TW`、`ja_JP`）映射到支持的语言。
fn parse_locale(tag: &str) -> Option<Locale> {
    let tag = tag.replace('_', "-").to_ascii_lowercase();
    let mut parts = tag.split('-');
    match parts.next()? {
        "zh" => {
            let rest: Vec<&str> = parts.collect();
            let traditional = rest
                .iter()
                .any(|part| matches!(*part, "hant" | "tw" | "hk" | "mo"));
            Some(if traditional {
                Locale::TraditionalChinese
            } else {
                Locale::SimplifiedChinese
            })
        }
        "en" => Some(Locale::English),
        "ja" => Some(Locale::Japanese),
        _ => None,
    }
}

const NO_LOCALE: u8 = u8::MAX;
static CURRENT: AtomicU8 = AtomicU8::new(NO_LOCALE);

pub fn set_language(language: Language) {
    CURRENT.store(language.resolve() as u8, Ordering::Relaxed);
}

pub fn locale() -> Locale {
    match CURRENT.load(Ordering::Relaxed) {
        0 => Locale::SimplifiedChinese,
        1 => Locale::TraditionalChinese,
        2 => Locale::English,
        3 => Locale::Japanese,
        _ => {
            let locale = system_locale();
            CURRENT.store(locale as u8, Ordering::Relaxed);
            locale
        }
    }
}

type Table = HashMap<&'static str, &'static str>;

/// 译文表：每行依次是简体中文（键）、繁體中文、English、日本語，用制表符分隔。
const TRANSLATIONS: &str = include_str!("../locales/translations.tsv");

fn unescape(cell: &str) -> String {
    cell.replace("\\n", "\n")
}

/// 按行拆出（键，各语言译文）。第一行是表头。
fn rows() -> impl Iterator<Item = Vec<&'static str>> {
    TRANSLATIONS
        .lines()
        .skip(1)
        .filter(|line| !line.is_empty())
        .map(|line| line.split('\t').collect())
}

fn parse_table(column: usize) -> Table {
    // 译文表只在首次使用时解析一次，泄漏成 'static 可以让 `tr!` 返回 &'static str。
    let leak = |text: &str| -> &'static str { Box::leak(unescape(text).into_boxed_str()) };
    rows()
        .filter_map(|cells| Some((leak(cells.first()?), leak(cells.get(column)?))))
        .collect()
}

fn table(locale: Locale) -> Option<&'static Table> {
    static TRADITIONAL: OnceLock<Table> = OnceLock::new();
    static ENGLISH: OnceLock<Table> = OnceLock::new();
    static JAPANESE: OnceLock<Table> = OnceLock::new();
    match locale {
        Locale::SimplifiedChinese => None,
        Locale::TraditionalChinese => Some(TRADITIONAL.get_or_init(|| parse_table(1))),
        Locale::English => Some(ENGLISH.get_or_init(|| parse_table(2))),
        Locale::Japanese => Some(JAPANESE.get_or_init(|| parse_table(3))),
    }
}

/// 返回当前语言下的文案。
pub fn text(key: &'static str) -> &'static str {
    table(locale())
        .and_then(|table| table.get(key).copied())
        .unwrap_or(key)
}

/// 把模板里的 `{name}` 换成参数。参数名在模板中不存在时忽略。
pub fn fill(template: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) => {
                let name = &after[..end];
                match args.iter().find(|(arg, _)| *arg == name) {
                    Some((_, value)) => out.push_str(&value.to_string()),
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// 只有文案时返回 `&'static str`；带 `name = value` 参数时填好模板返回 `String`。
#[macro_export]
macro_rules! tr {
    ($key:literal) => {
        $crate::i18n::text($key)
    };
    ($key:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::fill(
            $crate::i18n::text($key),
            &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),+],
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_tags_map_to_supported_languages() {
        assert_eq!(parse_locale("zh-Hans-CN"), Some(Locale::SimplifiedChinese));
        assert_eq!(parse_locale("zh_TW"), Some(Locale::TraditionalChinese));
        assert_eq!(parse_locale("zh-Hant"), Some(Locale::TraditionalChinese));
        assert_eq!(parse_locale("en-GB"), Some(Locale::English));
        assert_eq!(parse_locale("ja-JP"), Some(Locale::Japanese));
        assert_eq!(parse_locale("de-DE"), None);
    }

    #[test]
    fn fill_replaces_named_arguments() {
        let count = 3;
        assert_eq!(
            fill("{n} files in {name}", &[("n", &count), ("name", &"src")]),
            "3 files in src"
        );
        assert_eq!(fill("keep {unknown}", &[]), "keep {unknown}");
    }

    /// 译文表覆盖源码里的全部文案，每种语言都有译文并保留同样的占位符。
    #[test]
    fn every_locale_translates_every_key() {
        let keys = source_keys();
        assert!(!keys.is_empty());
        let mut seen = std::collections::BTreeSet::new();
        for cells in rows() {
            assert_eq!(cells.len(), 4, "row needs four columns: {cells:?}");
            let key = unescape(cells[0]);
            assert!(seen.insert(key.clone()), "duplicate key {key}");
            for cell in &cells[1..] {
                assert!(!cell.is_empty(), "empty translation for {key}");
                assert_eq!(placeholders(&key), placeholders(&unescape(cell)), "{key}");
            }
        }
        let missing: Vec<&String> = keys.difference(&seen).collect();
        assert!(missing.is_empty(), "untranslated keys {missing:#?}");
        let stale: Vec<&String> = seen.difference(&keys).collect();
        assert!(stale.is_empty(), "unused keys {stale:#?}");
    }

    fn placeholders(text: &str) -> Vec<String> {
        let mut names: Vec<String> = text
            .split('{')
            .skip(1)
            .filter_map(|part| part.split_once('}').map(|(name, _)| name.to_string()))
            .collect();
        names.sort();
        names
    }

    /// 从源码里收集所有 `tr!("…")` 的键。
    fn source_keys() -> std::collections::BTreeSet<String> {
        let mut keys = std::collections::BTreeSet::new();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let source = std::fs::read_to_string(&path).unwrap();
                    let code = source.split("#[cfg(test)]").next().unwrap_or_default();
                    let mut rest = code;
                    while let Some(start) = rest.find("tr!(") {
                        let macro_call = start == 0
                            || !rest.as_bytes()[start - 1].is_ascii_alphanumeric()
                                && rest.as_bytes()[start - 1] != b'_';
                        rest = rest[start + 4..].trim_start();
                        if !macro_call || !rest.starts_with('"') {
                            continue;
                        }
                        rest = &rest[1..];
                        let mut key = String::new();
                        let mut chars = rest.char_indices();
                        while let Some((index, ch)) = chars.next() {
                            match ch {
                                '\\' => {
                                    if let Some((_, escaped)) = chars.next() {
                                        key.push(match escaped {
                                            'n' => '\n',
                                            't' => '\t',
                                            other => other,
                                        });
                                    }
                                }
                                '"' => {
                                    rest = &rest[index..];
                                    break;
                                }
                                other => key.push(other),
                            }
                        }
                        keys.insert(key);
                    }
                }
            }
        }
        keys
    }
}
