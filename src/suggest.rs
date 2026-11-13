#[derive(Debug, Clone)]
pub struct Suggester {
    history: Vec<String>,
}

impl Suggester {
    pub fn new() -> Self {
        Self { history: Vec::new() }
    }

    pub fn push(&mut self, line: &str) {
        let line = line.to_string();
        if self.history.last().is_some_and(|last| last == &line) {
            return;
        }

        self.history.push(line);
        if self.history.len() > 1000 {
            let overflow = self.history.len() - 1000;
            self.history.drain(..overflow);
        }
    }

    pub fn suggest(&self, buffer: &str) -> Option<String> {
        if buffer.trim().is_empty() {
            return None;
        }

        for entry in self.history.iter().rev() {
            if entry.starts_with(buffer) && entry.len() > buffer.len() {
                return Some(entry[buffer.len()..].to_string());
            }
        }

        None
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.history.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.history.is_empty()
    }
}

impl Default for Suggester {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::Suggester;

    #[test]
    fn exact_prefix_match_returns_missing_suffix() {
        let mut suggester = Suggester::new();
        suggester.push("echo hello");

        assert_eq!(suggester.suggest("echo"), Some(" hello".to_string()));
    }

    #[test]
    fn newest_match_wins_when_multiple_entries_match() {
        let mut suggester = Suggester::new();
        suggester.push("echo hello");
        suggester.push("echo hi");

        assert_eq!(suggester.suggest("echo"), Some(" hi".to_string()));
    }

    #[test]
    fn empty_buffer_returns_none() {
        let suggester = Suggester::new();
        assert_eq!(suggester.suggest(""), None);
    }

    #[test]
    fn whitespace_buffer_returns_none() {
        let suggester = Suggester::new();
        assert_eq!(suggester.suggest("   \t\n "), None);
    }

    #[test]
    fn exact_buffer_match_is_not_suggested() {
        let mut suggester = Suggester::new();
        suggester.push("echo");

        assert_eq!(suggester.suggest("echo"), None);
    }

    #[test]
    fn history_caps_at_1000_entries_and_drops_oldest() {
        let mut suggester = Suggester::new();
        for i in 0..1005 {
            suggester.push(&format!("entry{i}"));
        }

        assert_eq!(suggester.len(), 1000);
        assert_eq!(suggester.suggest("entry0"), None);
    }

    #[test]
    fn consecutive_repeats_are_deduped() {
        let mut suggester = Suggester::new();
        suggester.push("same");
        suggester.push("same");

        assert_eq!(suggester.len(), 1);
    }

    #[test]
    fn matching_is_case_sensitive() {
        let mut suggester = Suggester::new();
        suggester.push("Echo");

        assert_eq!(suggester.suggest("echo"), None);
    }

    #[test]
    fn multiline_entries_are_matched_as_raw_strings() {
        let mut suggester = Suggester::new();
        suggester.push("cmd\nnext");

        assert_eq!(suggester.suggest("cmd"), Some("\nnext".to_string()));
    }
}
