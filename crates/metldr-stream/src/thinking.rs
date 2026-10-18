const TAGS: [(&str, &str); 4] = [
    ("<think>", "</think>"),
    ("<thinking>", "</thinking>"),
    ("<reason>", "</reason>"),
    ("<reasoning>", "</reasoning>"),
];

pub(crate) struct ThinkingFilter {
    pending: String,
    closing: Option<&'static str>,
}

impl ThinkingFilter {
    pub(crate) fn new() -> Self {
        Self {
            pending: String::new(),
            closing: None,
        }
    }

    pub(crate) fn push(&mut self, text: &str) -> String {
        self.pending.push_str(text);
        let mut output = String::new();
        while !self.pending.is_empty() {
            if let Some(closing) = self.closing {
                let lower = self.pending.to_ascii_lowercase();
                if let Some(index) = lower.find(closing) {
                    self.pending.drain(..index + closing.len());
                    self.closing = None;
                    continue;
                }
                let keep = (1..closing.len())
                    .rev()
                    .find(|&length| {
                        lower.len() >= length
                            && lower.is_char_boundary(lower.len() - length)
                            && closing.starts_with(&lower[lower.len() - length..])
                    })
                    .unwrap_or(0);
                self.pending.drain(..self.pending.len() - keep);
                break;
            }

            let Some(index) = self.pending.find('<') else {
                output.push_str(&std::mem::take(&mut self.pending));
                break;
            };
            output.push_str(&self.pending[..index]);
            self.pending.drain(..index);
            let lower = self.pending.to_ascii_lowercase();
            if let Some((opening, closing)) =
                TAGS.iter().find(|(opening, _)| lower.starts_with(opening))
            {
                self.pending.drain(..opening.len());
                self.closing = Some(closing);
            } else if TAGS.iter().any(|(opening, _)| opening.starts_with(&lower)) {
                break;
            } else {
                output.push('<');
                self.pending.drain(..1);
            }
        }
        output
    }

    pub(crate) fn finish(&mut self) -> String {
        let pending = std::mem::take(&mut self.pending);
        if self.closing.is_none() {
            pending
        } else {
            String::new()
        }
    }
}
