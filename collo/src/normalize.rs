/// Normalize Latin spelling for lookup, ignoring vowel quantities and i/j, u/v distinctions.
pub fn normalize(input: &str) -> String {
    let mut out = String::new();
    for c in input.chars() {
        match c {
            'ā' | 'ă' | 'á' => out.push('a'),
            'Ā' | 'Ă' | 'Á' => out.push('A'),
            'ē' | 'ĕ' | 'é' => out.push('e'),
            'Ē' | 'Ĕ' | 'É' => out.push('E'),
            'ī' | 'ĭ' | 'í' | 'ı' => out.push('i'),
            'Ī' | 'Ĭ' | 'Í' => out.push('I'),
            'ō' | 'ŏ' | 'ó' => out.push('o'),
            'Ō' | 'Ŏ' | 'Ó' => out.push('O'),
            'ū' | 'ŭ' | 'ú' | 'ụ' => out.push('u'),
            'Ū' | 'Ŭ' | 'Ú' => out.push('U'),
            'ȳ' | 'ў' => out.push('y'),
            'Ȳ' | 'Ў' => out.push('Y'),
            'j' => out.push('i'),
            'J' => out.push('I'),
            'v' => out.push('u'),
            'V' => out.push('U'),
            'æ' => out.push_str("ae"),
            'Æ' => out.push_str("Ae"),
            'œ' => out.push_str("oe"),
            'Œ' => out.push_str("Oe"),
            '\u{0304}' | '\u{0306}' | '\u{0301}' => (),
            _ => out.push(c),
        }
    }
    out
}
