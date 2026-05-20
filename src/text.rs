//! Small text-shaping helpers shared by the reading view and the splash.

/// Greedy whitespace-respecting word wrap. Splits `text` into lines no
/// wider than `max_width` characters; a word longer than `max_width`
/// becomes its own (over-long) line rather than being broken.
///
/// `max_width == 0` returns a single line containing the input unchanged.
pub fn word_wrap(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current = word.to_string();
            continue;
        }
        // +1 for the joining space.
        if current.chars().count() + 1 + word.chars().count() <= max_width {
            current.push(' ');
            current.push_str(word);
        } else {
            out.push(std::mem::take(&mut current));
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_long_paragraph() {
        let out = word_wrap("the quick brown fox jumps", 10);
        assert!(out.iter().all(|l| l.chars().count() <= 10));
        assert_eq!(out.join(" "), "the quick brown fox jumps");
    }

    #[test]
    fn preserves_words_longer_than_width() {
        let out = word_wrap("supercalifragilistic", 5);
        assert_eq!(out, vec!["supercalifragilistic".to_string()]);
    }

    #[test]
    fn zero_width_returns_input() {
        let out = word_wrap("a b c", 0);
        assert_eq!(out, vec!["a b c".to_string()]);
    }
}
