use sha2::{Digest, Sha256};

use super::dictionary::{MAX_TERM_CHARACTERS, MIN_TERM_CHARACTERS};
use super::file::invalid;
use super::{MAX_TERMS, Policy, add_to_digest, check_policy_category, term_automaton};
use crate::errors::PolicyError;
use crate::text::{fold, nfc};

impl Policy {
    /// This policy with `terms` added for one task, each under its
    /// category, such as private details the local model found in a
    /// question. Terms follow the dictionary rules: 2 to 200 characters,
    /// starting and ending with a letter or digit. The digest names the
    /// added terms too.
    pub fn with_task_terms(&self, terms: &[(String, String)]) -> Result<Policy, PolicyError> {
        let mut categories = self.dictionary_categories.clone();
        let mut all_terms = self.terms.clone();
        let mut term_dictionary = self.term_dictionary.clone();
        let mut hasher = Sha256::new();
        hasher.update(self.digest);
        for (position, (category, written)) in terms.iter().enumerate() {
            check_policy_category(category, &format!("task_terms[{position}].category"))?;
            let term = nfc(written);
            let characters = term.chars().count();
            let alphanumeric = |character: Option<char>| {
                character
                    .is_some_and(|character| character.is_alphabetic() || character.is_numeric())
            };
            if !(MIN_TERM_CHARACTERS..=MAX_TERM_CHARACTERS).contains(&characters)
                || !alphanumeric(term.chars().next())
                || !alphanumeric(term.chars().next_back())
            {
                return Err(invalid(
                    format!("task_terms[{position}].text"),
                    "must be 2 to 200 characters, starting and ending with a letter or digit",
                ));
            }
            let index = match categories.iter().position(|known| known == category) {
                Some(index) => index,
                None => {
                    categories.push(category.clone());
                    categories.len() - 1
                }
            };
            let folded = fold(&term);
            add_to_digest(&mut hasher, category.as_bytes());
            add_to_digest(&mut hasher, folded.as_bytes());
            all_terms.push(folded);
            term_dictionary.push(index);
        }
        if all_terms.len() > MAX_TERMS {
            return Err(PolicyError::Limit {
                limit: "200 000 terms",
            });
        }
        Ok(Policy {
            enabled: self.enabled.clone(),
            dictionary_categories: categories,
            automaton: term_automaton(&all_terms)?,
            terms: all_terms,
            term_dictionary,
            patterns: self.patterns.clone(),
            passthrough: self.passthrough.clone(),
            digest: hasher.finalize().into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{Destination, Policy, TaskMapping};

    #[test]
    fn task_terms_are_replaced_and_restored_like_dictionary_terms() {
        let policy = Policy::builtin()
            .unwrap()
            .with_task_terms(&[
                ("PERSON".to_string(), "Ayşe Demir".to_string()),
                ("ORGANISATION".to_string(), "Kaya Lojistik".to_string()),
            ])
            .unwrap();
        let pseudonymiser = policy.for_destination(Destination::new("cloud", "api.example.com"));
        let mut mapping = TaskMapping::new();
        let sent = pseudonymiser
            .text(
                &mut mapping,
                "Should ayşe demir at Kaya Lojistik renegotiate the contract?",
            )
            .unwrap();
        let text = sent.as_str();
        assert!(!text.to_lowercase().contains("ayşe"), "{text}");
        assert!(!text.contains("Kaya"), "{text}");
        assert!(text.contains("⟦PERSON_1⟧") && text.contains("⟦ORGANISATION_1⟧"));
        let restored = mapping
            .restore("Yes, ⟦PERSON_1⟧ should call ⟦ORGANISATION_1⟧.")
            .unwrap();
        assert_eq!(
            restored.as_str(),
            "Yes, ayşe demir should call Kaya Lojistik."
        );
    }

    #[test]
    fn a_task_term_that_breaks_the_dictionary_rules_is_refused() {
        let policy = Policy::builtin().unwrap();
        assert!(
            policy
                .with_task_terms(&[("PERSON".to_string(), "J.".to_string())])
                .is_err()
        );
        assert!(
            policy
                .with_task_terms(&[("person".to_string(), "Jo Li".to_string())])
                .is_err()
        );
        assert_ne!(
            policy
                .with_task_terms(&[("PERSON".to_string(), "Jo Li".to_string())])
                .unwrap()
                .digest(),
            policy.digest()
        );
    }
}
