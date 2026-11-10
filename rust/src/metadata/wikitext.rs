//! Just enough wikitext for album articles: templates with their
//! parameters, and readable text from links, lists and formatting.
//! The Mac companion's `WikiText` reads articles by the same rules.
use regex::Regex;
use std::{collections::HashMap, sync::LazyLock};

static COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<!--[\s\S]*?-->").unwrap());
static REF_EMPTY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<ref[^>]*/>").unwrap());
static REF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<ref[^>]*>[\s\S]*?</ref>").unwrap());
static FILE_LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\[\[(?:file|image|immagine|category|categoria):[^\[\]]*(?:\[\[[^\[\]]*\]\][^\[\]]*)*\]\]",
    )
    .unwrap()
});
static LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[(?:[^\[\]|]*\|)?([^\[\]]*)\]\]").unwrap());
static EXTERNAL_LABEL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[https?://[^\s\]]+\s+([^\]]+)\]").unwrap());
static EXTERNAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[https?://[^\]]*\]").unwrap());
static EMPHASIS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"'{2,}").unwrap());
static BREAK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>").unwrap());
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
static LIST_MARK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*[*#]+\s*").unwrap());
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
static COMMAS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:\s*,\s*){2,}").unwrap());
static TEMPLATE_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*template\s*:").unwrap());

pub(super) struct Template {
    pub name: String,
    pub positional: Vec<String>,
    pub named: HashMap<String, String>,
}
impl Template {
    /// The first of `keys` with a value.
    pub fn value(&self, keys: &[&str]) -> &str {
        keys.iter()
            .filter_map(|key| self.named.get(*key))
            .find(|value| !value.trim().is_empty())
            .map_or("", String::as_str)
    }
}
/// A template and its byte range, from "{{" to just after "}}".
pub(super) struct Span {
    pub template: Template,
    pub start: usize,
    pub end: usize,
}

/// Comments and references hold markup and pipes that are not part of the values.
pub(super) fn cleaned(text: &str) -> String {
    let text = COMMENT.replace_all(text, "");
    let text = REF_EMPTY.replace_all(&text, "");
    REF.replace_all(&text, "").into_owned()
}

/// Every template in document order, including nested ones. The delimiters
/// are ASCII, so byte offsets always fall on character boundaries.
pub(super) fn spans(text: &str) -> Vec<Span> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b'{' && bytes[index + 1] == b'{' {
            if let Some(end) = closing(bytes, index + 2) {
                spans.push(Span {
                    template: template(&text[index + 2..end - 2]),
                    start: index,
                    end,
                });
                index += 2;
                continue;
            }
        }
        index += 1;
    }
    spans
}

pub(super) fn templates(text: &str) -> impl Iterator<Item = Template> {
    spans(text).into_iter().map(|span| span.template)
}

/// Readable text: links keep their labels, list templates keep their items,
/// and other templates, tags and formatting are removed.
pub(super) fn plain(text: &str) -> String {
    let value = flatten(&cleaned(text));
    let value = FILE_LINK.replace_all(&value, "");
    let value = LINK.replace_all(&value, "${1}");
    let value = EXTERNAL_LABEL.replace_all(&value, "${1}");
    let value = EXTERNAL.replace_all(&value, "");
    let value = EMPHASIS.replace_all(&value, "");
    let value = BREAK.replace_all(&value, ", ");
    let value = TAG.replace_all(&value, "");
    let mut value = LIST_MARK.replace_all(&value, ", ").into_owned();
    for (entity, character) in [
        ("&nbsp;", " "),
        ("&amp;", "&"),
        ("&ndash;", "–"),
        ("&mdash;", "—"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&apos;", "'"),
    ] {
        value = value.replace(entity, character);
    }
    let value = SPACES.replace_all(&value, " ");
    let value = COMMAS.replace_all(&value, ", ");
    value
        .trim_matches(|c: char| c == ',' || c.is_whitespace())
        .to_string()
}

fn flatten(text: &str) -> String {
    if !text.contains("{{") {
        return text.into();
    }
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut index = 0;
    let mut copied = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b'{' && bytes[index + 1] == b'{' {
            if let Some(end) = closing(bytes, index + 2) {
                output.push_str(&text[copied..index]);
                output.push_str(&readable(&template(&text[index + 2..end - 2])));
                index = end;
                copied = end;
                continue;
            }
        }
        index += 1;
    }
    output.push_str(&text[copied..]);
    output
}

fn readable(template: &Template) -> String {
    let items: Vec<String> = template
        .positional
        .iter()
        .map(|item| flatten(item).trim().to_string())
        .filter(|item| !item.is_empty())
        .collect();
    match template.name.as_str() {
        "hlist" | "flatlist" | "plainlist" | "ubl" | "unbulleted list" | "flat list"
        | "plain list" => items.join(", "),
        "nowrap" | "small" | "nobold" | "sic" | "nihongo" | "nihongo2" | "nihongo3" | "korean"
        | "smallcaps" => items.into_iter().next().unwrap_or_default(),
        "start date" | "release date" | "start date and age" | "film date" | "dts" | "date" => {
            items.join(" ")
        }
        name if name == "lang" || name.starts_with("lang-") => {
            items.into_iter().last().unwrap_or_default()
        }
        _ => String::new(),
    }
}

/// The index just after the "}}" that closes a template opened before `start`.
fn closing(bytes: &[u8], start: usize) -> Option<usize> {
    let mut depth = 1;
    let mut index = start;
    while index + 1 < bytes.len() {
        match (bytes[index], bytes[index + 1]) {
            (b'{', b'{') => {
                depth += 1;
                index += 2;
            }
            (b'}', b'}') => {
                depth -= 1;
                index += 2;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => index += 1,
        }
    }
    None
}

fn template(body: &str) -> Template {
    let bytes = body.as_bytes();
    let mut parts = Vec::new();
    let (mut braces, mut links, mut index, mut from) = (0_i32, 0_i32, 0, 0);
    while index < bytes.len() {
        let pair = (bytes[index], bytes.get(index + 1).copied());
        match pair {
            (b'{', Some(b'{')) => braces += 1,
            (b'}', Some(b'}')) => braces -= 1,
            (b'[', Some(b'[')) => links += 1,
            (b']', Some(b']')) => links -= 1,
            (b'|', _) if braces <= 0 && links <= 0 => {
                parts.push(&body[from..index]);
                from = index + 1;
                index += 1;
                continue;
            }
            _ => {
                index += 1;
                continue;
            }
        }
        index += 2;
    }
    parts.push(&body[from..]);
    let name = TEMPLATE_PREFIX
        .replace(&parts[0].replace('_', " "), "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let mut positional = Vec::new();
    let mut named = HashMap::new();
    for part in &parts[1..] {
        if let Some(equals) = top_level_equals(part) {
            named.insert(
                part[..equals].trim().to_lowercase(),
                part[equals + 1..].trim().to_string(),
            );
        } else {
            positional.push((*part).to_string());
        }
    }
    Template {
        name,
        positional,
        named,
    }
}

fn top_level_equals(part: &str) -> Option<usize> {
    let bytes = part.as_bytes();
    let mut depth = 0_i32;
    let mut previous = 0_u8;
    for (index, &byte) in bytes.iter().enumerate() {
        if matches!(byte, b'{' | b'[') && previous == byte {
            depth += 1;
        }
        if matches!(byte, b'}' | b']') && previous == byte {
            depth -= 1;
        }
        if byte == b'=' && depth <= 0 {
            let key = part[..index].trim();
            return (!key.is_empty() && !key.contains('\n')).then_some(index);
        }
        previous = byte;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_keep_nested_values_and_plain_text_keeps_labels() {
        let text = "{{Infobox album | name = OK Computer | artist = [[Radiohead]] \
                    | released = {{start date|1997|5|21}} | genre = {{hlist|[[Alternative rock]]|[[art rock]]}}<ref>x|y</ref> }}";
        let infobox = templates(&cleaned(text)).next().unwrap();
        assert_eq!(infobox.name, "infobox album");
        assert_eq!(plain(infobox.value(&["name", "titolo"])), "OK Computer");
        assert_eq!(plain(infobox.value(&["artist"])), "Radiohead");
        assert_eq!(plain(infobox.value(&["released"])), "1997 5 21");
        assert_eq!(
            plain(infobox.value(&["genre"])),
            "Alternative rock, art rock"
        );
        assert_eq!(
            plain("''[[Airbag / How Am I Driving?|Airbag]]''&nbsp;<small>x</small>"),
            "Airbag x"
        );
    }
}
