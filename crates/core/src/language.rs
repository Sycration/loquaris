//! Which of collo's languages to show translations and morphology in.

/// The collo translation language matching the project's L1 name.
pub fn translation_language(l1_name: &str) -> &'static str {
    match l1_name.trim().to_lowercase().as_str() {
        "french" | "français" | "francais" | "fr" => "fr",
        "german" | "deutsch" | "de" => "de",
        "spanish" | "español" | "espanol" | "es" => "es",
        "italian" | "italiano" | "it" => "it",
        "portuguese" | "português" | "portugues" | "pt" => "pt",
        "catalan" | "català" | "catala" | "ca" => "ca",
        "galician" | "galego" | "gl" => "gl",
        _ => "en",
    }
}

/// Morphology descriptions are available in fewer languages than
/// translations: French and Spanish, else English.
pub fn morphology_language(translation: &str) -> &'static str {
    match translation {
        "fr" => "fr",
        "es" => "es",
        _ => "en",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l1_names_choose_translation_and_morphology_languages() {
        for (name, translation, morphology) in [
            ("English", "en", "en"),
            (" Français ", "fr", "fr"),
            ("francais", "fr", "fr"),
            ("Deutsch", "de", "en"),
            ("Español", "es", "es"),
            ("italian", "it", "en"),
            ("Português", "pt", "en"),
            ("català", "ca", "en"),
            ("Galego", "gl", "en"),
            ("Klingon", "en", "en"),
            ("", "en", "en"),
        ] {
            assert_eq!(translation_language(name), translation, "{name}");
            assert_eq!(
                morphology_language(translation_language(name)),
                morphology,
                "{name}"
            );
        }
    }
}
