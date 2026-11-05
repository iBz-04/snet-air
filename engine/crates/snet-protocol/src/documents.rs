const MAX_QUESTION_CHARACTERS: usize = 4_000;
const MAX_NAME_CHARACTERS: usize = 256;

/// One local document: its name stays on this machine, its text is read only by the
/// local model.
#[derive(Clone)]
pub struct Document {
    pub name: String,
    pub text: String,
}

impl std::fmt::Debug for Document {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Document")
            .field("name_characters", &self.name.chars().count())
            .field("bytes", &self.text.len())
            .finish()
    }
}

/// A pseudonym bracket or one of its look-alikes.
pub fn holds_bracket(text: &str) -> bool {
    text.chars()
        .any(|character| matches!(character, '⟦' | '⟧' | '〚' | '〛'))
}

/// 1 to 4 000 scalar values, not blank, without brackets.
pub fn question_is_valid(question: &str) -> bool {
    let characters = question.chars().count();
    (1..=MAX_QUESTION_CHARACTERS).contains(&characters)
        && !question.trim().is_empty()
        && !holds_bracket(question)
}

/// 1 to 256 scalar values without control characters.
pub fn name_is_valid(name: &str) -> bool {
    (1..=MAX_NAME_CHARACTERS).contains(&name.chars().count()) && !name.chars().any(char::is_control)
}
