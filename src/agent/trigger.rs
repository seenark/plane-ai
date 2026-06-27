use regex::Regex;
use std::sync::LazyLock;

static MULTISPACE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+").expect("valid whitespace regex"));

pub fn contains_agent_mention(comment_html: &str, mentions: &[String]) -> bool {
    let plain = strip_html_for_mentions(comment_html).to_lowercase();
    mentions
        .iter()
        .any(|mention| mention_matches(&plain, &mention.to_lowercase()))
}

pub fn strip_html_for_mentions(comment_html: &str) -> String {
    let rendered = html2text::from_read(comment_html.as_bytes(), usize::MAX);
    MULTISPACE_RE.replace_all(rendered.trim(), " ").into_owned()
}

fn mention_matches(plain_text: &str, mention: &str) -> bool {
    let mut search_start = 0;
    while let Some(relative_index) = plain_text[search_start..].find(mention) {
        let index = search_start + relative_index;
        let before = plain_text[..index].chars().next_back();
        let after = plain_text[index + mention.len()..].chars().next();
        let before_ok = before.is_none_or(is_boundary_char);
        let after_ok = after.is_none_or(is_boundary_char);
        if before_ok && after_ok {
            return true;
        }
        search_start = index + mention.len();
    }
    false
}

fn is_boundary_char(value: char) -> bool {
    !value.is_ascii_alphanumeric() && value != '_' && value != '-'
}
