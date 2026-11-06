use snet_pseudonym::TaskMapping;

/// Text with every pseudonym token replaced by a space.
pub fn without_tokens(text: &str) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut inside = false;
    for character in text.chars() {
        match character {
            '⟦' => inside = true,
            '⟧' => {
                inside = false;
                kept.push(' ');
            }
            _ if !inside => kept.push(character),
            _ => {}
        }
    }
    kept
}

/// Whether pseudonymised text, its tokens removed, still spells a
/// value of the task's mapping.
pub fn spells_out_a_value(mapping: &TaskMapping, pseudonymised: &str) -> bool {
    mapping.holds_spelled_value(&without_tokens(pseudonymised))
}

/// Corrections a task may ask of the cloud in all.
pub const MAX_CORRECTIONS: u32 = 1;

/// What the cloud reads after a refused step. `detail` is one of the
/// protocol's own refusal reasons, which name rules and never material.
pub fn correction(detail: &str) -> String {
    format!(
        "Your last step was refused: {detail}. Send the step again, corrected, in the same schema. This is the only correction this task allows."
    )
}
