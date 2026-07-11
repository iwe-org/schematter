//! Token counting.
//!
//! `maxTokens` budgets are measured with the same tokenizer iwe uses for its
//! retrieve budgets — OpenAI's `o200k_base` BPE — so counts line up with the
//! rest of the toolchain. The builder counts tokens over raw markdown source
//! spans (see [`crate::builder`]); this module only supplies the counter.

use std::sync::OnceLock;

use tiktoken_rs::{o200k_base, CoreBPE};

fn bpe() -> &'static CoreBPE {
    static BPE: OnceLock<CoreBPE> = OnceLock::new();
    BPE.get_or_init(|| o200k_base().expect("load o200k_base"))
}

/// Count the `o200k_base` tokens in `text`.
pub fn count_tokens(text: &str) -> usize {
    bpe().count_ordinary(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_known_strings() {
        assert_eq!(count_tokens(""), 0);
        assert_eq!(count_tokens("hello"), 1);
        assert_eq!(count_tokens("hello world"), 2);
    }
}
