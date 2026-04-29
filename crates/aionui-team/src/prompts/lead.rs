//! Lead prompt template constant.
//!
//! Byte-for-byte copy of AionUi `leadPrompt.ts` template literal
//! (the string returned by `buildLeaderPrompt`). The `${...}` placeholders
//! are preserved verbatim so the builder in D5b-2 can substitute them.
//! Do not modify the template text: it is treated as raw material, not code.

pub const LEAD_PROMPT_TEMPLATE: &str = include_str!("prompt_templates/lead.txt");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lead_prompt_template_contains_header() {
        assert!(LEAD_PROMPT_TEMPLATE.contains("You are the Team Leader"));
    }
}
