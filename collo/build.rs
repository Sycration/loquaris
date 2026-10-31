#[path = "src/normalize.rs"]
mod normalize;
#[path = "src/hash.rs"]
mod hash;
use normalize::normalize;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env,
    fmt::Write,
    fs,
    path::PathBuf,
};
#[derive(Clone, Debug)]
struct Ending {
    morphology: u16,
    stem: u8,
    text: String,
}
#[derive(Clone, Default)]
struct Model {
    stems: BTreeMap<u8, String>,
    endings: Vec<Ending>,
    part: char,
    absent: BTreeSet<u16>,
}
fn lines(path: &str) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|s| !s.is_empty() && !s.starts_with('!'))
        .map(str::to_owned)
        .collect()
}
fn numbers(s: &str) -> Vec<u16> {
    s.split(',')
        .flat_map(|part| {
            if let Some((a, b)) = part.split_once('-') {
                let a: u16 = a.parse().unwrap();
                let b: u16 = b.parse().unwrap();
                (a..=b).collect::<Vec<_>>()
            } else {
                vec![part.parse().unwrap()]
            }
        })
        .collect()
}
fn expand(line: &str, vars: &HashMap<String, String>) -> String {
    let mut result = line.to_owned();
    while let Some(start) = result.find('$') {
        let end = result[start..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '$' || c == '_'))
            .map(|n| start + n)
            .unwrap_or(result.len());
        let value = &vars[&result[start..end]];
        let before = &result[..start];
        let prefix = before.rsplit([':', ';']).next().unwrap();
        let prefix = prefix.strip_suffix('+').unwrap_or(prefix);
        let expanded = value.replace(';', &format!(";{prefix}"));
        result.replace_range(start..end, &expanded);
    }
    result
}
fn lengthen_before_consonant(text: &mut String) {
    let mut chars: Vec<char> = text.chars().collect();
    if chars.len() < 2 {
        return;
    }
    let last = chars.len() - 1;
    if !"bcdfgjklmnpqrstvxz".contains(chars[last]) {
        return;
    }
    chars[last - 1] = match chars[last - 1] {
        'a' | 'ă' => 'ā',
        'e' | 'ĕ' => 'ē',
        'i' | 'ĭ' => 'ī',
        'o' | 'ŏ' => 'ō',
        'u' | 'ŭ' => 'ū',
        'y' | 'ў' => 'ȳ',
        c => c,
    };
    *text = chars.into_iter().collect();
}
// Collatinus marks vowels without an explicit quantity as common in stems.
fn common_vowels(text: &str) -> String {
    let initial_uppercase = text.chars().next().is_some_and(char::is_uppercase);
    let mut result = String::new();
    for c in text.to_lowercase().chars() {
        let previous = result.chars().last();
        let long = match c {
            'a' => Some('ā'),
            'e' if !matches!(previous, Some('ā' | 'ă' | 'ō')) => Some('ē'),
            'i' => Some('ī'),
            'o' => Some('ō'),
            'u' if !matches!(previous, Some('ā' | 'ē' | 'q')) => Some('ū'),
            'y' if previous != Some('ā') => Some('ȳ'),
            _ => None,
        };
        if let Some(long) = long {
            result.push(long);
            result.push('\u{0306}');
        } else {
            result.push(c);
        }
    }
    if initial_uppercase && let Some(first) = result.chars().next() {
        let length = first.len_utf8();
        result.replace_range(..length, &first.to_uppercase().collect::<String>());
    }
    result
}
/// Open-addressing index from each distinct key of a sorted table to the
/// position of its first entry.
fn hash_index<'a>(keys: impl Iterator<Item = &'a str>) -> Vec<u32> {
    let keys: Vec<_> = keys.collect();
    let distinct = keys.windows(2).filter(|w| w[0] != w[1]).count() + 1;
    let len = (distinct * 2).next_power_of_two();
    let mut table = vec![hash::EMPTY; len];
    for (i, key) in keys.iter().enumerate() {
        if i > 0 && keys[i - 1] == *key {
            continue;
        }
        let mut slot = hash::slot(key, len);
        while table[slot] != hash::EMPTY {
            slot = (slot + 1) & (len - 1);
        }
        table[slot] = i as u32;
    }
    table
}
/// Strings for the private tables, stored once in a single blob so that table
/// entries hold offsets rather than pointers needing load-time relocation.
#[derive(Default)]
struct Blob {
    text: String,
    offsets: HashMap<String, u32>,
}
impl Blob {
    /// `(start, length)` of `s` in the blob, written as a Rust tuple.
    fn add(&mut self, s: &str) -> String {
        let start = *self.offsets.entry(s.to_owned()).or_insert_with(|| {
            let start = self.text.len() as u32;
            self.text.push_str(s);
            start
        });
        let length = u16::try_from(s.len()).unwrap();
        format!("{start},{length}")
    }
}
fn main() {
    println!("cargo:rerun-if-changed=data");
    println!("cargo:rerun-if-changed=src/normalize.rs");
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=src/hash.rs");
    let mut vars = HashMap::new();
    let mut groups: Vec<Vec<String>> = Vec::new();
    for line in lines("data/modeles.la") {
        if line.starts_with('$') {
            let (k, v) = line.split_once('=').unwrap();
            vars.insert(k.to_owned(), v.to_owned());
        } else if line.starts_with("modele:") {
            groups.push(vec![line]);
        } else {
            groups.last_mut().unwrap().push(line);
        }
    }
    let mut models: BTreeMap<String, Model> = BTreeMap::new();
    for group in groups {
        let name = group[0].split(':').nth(1).unwrap().to_owned();
        let mut model = Model::default();
        let mut parent: Option<Model> = None;
        let mut own = BTreeSet::new();
        let mut suffixes = Vec::new();
        let mut inherited_suffix = String::new();
        for raw in &group[1..] {
            let line = expand(raw, &vars);
            let fields: Vec<_> = line.split(':').collect();
            match fields[0] {
                "pere" => {
                    let p = models
                        .get(fields[1])
                        .unwrap_or_else(|| panic!("missing parent {}", fields[1]))
                        .clone();
                    model.part = p.part;
                    model.stems = p.stems.clone();
                    parent = Some(p);
                }
                "pos" => model.part = fields[1].chars().next().unwrap(),
                "R" => {
                    model
                        .stems
                        .insert(fields[1].parse().unwrap(), fields[2].to_owned());
                }
                "abs" => model.absent = numbers(fields[1]).into_iter().collect(),
                "abs+" => model.absent.extend(numbers(fields[1])),
                "des" | "des+" => {
                    let nums = numbers(fields[1]);
                    let options: Vec<_> = fields[3].split(';').collect();
                    for (i, n) in nums.iter().enumerate() {
                        own.insert(*n);
                        for text in options[i.min(options.len() - 1)].split(',') {
                            let mut text = text.to_owned();
                            if text
                                .chars()
                                .last()
                                .is_some_and(|c| ('1'..='9').contains(&c))
                            {
                                text.pop();
                            }
                            if text == "-" {
                                text.clear();
                            }
                            model.endings.push(Ending {
                                morphology: *n,
                                stem: fields[2].parse().unwrap(),
                                text,
                            });
                        }
                        if fields[0] == "des+"
                            && let Some(p) = &parent
                        {
                            model.endings.extend(
                                p.endings
                                    .iter()
                                    .filter(|e| e.morphology == *n)
                                    .cloned()
                                    .map(|mut ending| {
                                        ending.text.push_str(&inherited_suffix);
                                        ending
                                    }),
                            );
                        }
                    }
                }
                "suf" => suffixes.push((numbers(fields[1]), fields[2].to_owned())),
                "sufd" => {
                    inherited_suffix = fields[1].to_owned();
                    if let Some(p) = &parent {
                        for e in &p.endings {
                            if !model.absent.contains(&e.morphology) {
                                let mut e = e.clone();
                                lengthen_before_consonant(&mut e.text);
                                e.text.push_str(fields[1]);
                                own.insert(e.morphology);
                                model.endings.push(e);
                            }
                        }
                    }
                }
                other => panic!("unknown model directive {other}: {line}"),
            }
        }
        if let Some(p) = parent {
            model.endings.extend(
                p.endings.into_iter().filter(|e| {
                    !own.contains(&e.morphology) && !model.absent.contains(&e.morphology)
                }),
            );
        }
        for (nums, suffix) in suffixes {
            let added: Vec<_> = model
                .endings
                .iter()
                .filter(|e| nums.contains(&e.morphology))
                .cloned()
                .map(|mut e| {
                    e.text.push_str(&suffix);
                    e
                })
                .collect();
            model.endings.extend(added);
        }
        models.insert(name, model);
    }
    let names: Vec<_> = models.keys().cloned().collect();
    let model_ids: HashMap<_, _> = names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i))
        .collect();
    let mut output = String::new();
    let mut keys = HashMap::new();
    let mut stems = Vec::new();
    let mut lemmas = Vec::new();
    for (extension, path) in [(false, "data/lemmes.la"), (true, "data/lem_ext.la")] {
        for line in lines(path) {
            let fields: Vec<_> = line.split('|').collect();
            assert!(fields.len() >= 6, "{line}");
            let (key, canonical) = fields[0].split_once('=').unwrap_or((fields[0], fields[0]));
            let key = normalize(key);
            let canonical = canonical.trim_end_matches(|c: char| c.is_ascii_digit());
            let model = models
                .get(fields[1])
                .unwrap_or_else(|| panic!("unknown model {}", fields[1]));
            let id = lemmas.len();
            keys.insert((extension, key.clone()), id);
            lemmas.push((
                key,
                canonical.to_owned(),
                fields[1].to_owned(),
                fields[4].to_owned(),
                fields[5].parse::<u32>().unwrap_or(0),
                extension,
                model.part,
            ));
            for (&number, rule) in &model.stems {
                let explicit = if number == 1 {
                    fields[2]
                } else if number == 2 {
                    fields[3]
                } else {
                    ""
                };
                let values: Vec<String> = if !explicit.is_empty() {
                    explicit.split(',').map(str::to_owned).collect()
                } else if rule == "-" {
                    vec![]
                } else {
                    canonical
                        .split(',')
                        .map(|word| {
                            if rule == "K" {
                                return word.to_owned();
                            }
                            let (remove, add) = rule.split_once(',').unwrap();
                            let mut word = word.trim_end_matches('\u{0306}').to_owned();
                            for _ in 0..remove.parse::<usize>().unwrap() {
                                word.pop();
                            }
                            if add != "0" {
                                word.push_str(add);
                            }
                            word
                        })
                        .collect()
                };
                for text in values {
                    stems.push((normalize(&text), id, number, common_vowels(&text)));
                }
            }
            for number in [1u8, 2] {
                if !model.stems.contains_key(&number) && !fields[number as usize + 1].is_empty() {
                    for text in fields[number as usize + 1].split(',') {
                        stems.push((normalize(text), id, number, common_vowels(text)));
                    }
                }
            }
        }
    }
    let mut irregulars = Vec::new();
    let mut exclusions = Vec::new();
    for line in lines("data/irregs.la") {
        let f: Vec<_> = line.split(':').collect();
        if let Some(&id) = keys.get(&(false, normalize(f[1]))) {
            let exclusive = f[0].ends_with('*');
            let text = f[0].trim_end_matches('*');
            for n in numbers(f[2]) {
                irregulars.push((normalize(text), id, n, text.to_owned()));
                if exclusive {
                    exclusions.push((id, n));
                }
            }
        }
    }
    let mut translations: HashMap<usize, BTreeMap<String, String>> = HashMap::new();
    for (prefix, langs) in [
        (
            "lemmes",
            vec!["en", "fr", "es", "de", "it", "pt", "ca", "gl"],
        ),
        ("lem_ext", vec!["en", "fr"]),
    ] {
        for lang in langs {
            let path = format!("data/{prefix}.{lang}");
            for line in lines(&path) {
                if let Some((key, value)) = line.split_once(':')
                    && let Some(&id) = keys.get(&(prefix == "lem_ext", normalize(key)))
                {
                    translations
                        .entry(id)
                        .or_default()
                        .insert(lang.to_owned(), value.to_owned());
                }
            }
        }
    }
    writeln!(output, "pub static LEMMAS: &[Lemma] = &[").unwrap();
    for (id, (key, canonical, model, details, count, extension, part)) in lemmas.iter().enumerate()
    {
        let ts = translations.get(&id);
        write!(output,"Lemma {{ key: {key:?}, headword: {canonical:?}, model: {model:?}, details: {details:?}, frequency: {count}, extension: {extension}, part_of_speech: {part:?}, translations: &[").unwrap();
        if let Some(ts) = ts {
            for (lang, value) in ts {
                write!(output, "({lang:?},{value:?}),").unwrap();
            }
        }
        writeln!(output, "] }},").unwrap();
    }
    writeln!(output, "]; ").unwrap();
    stems.sort();
    let mut blob = Blob::default();
    writeln!(output, "pub static STEMS: &[Stem] = &[").unwrap();
    for (key, id, num, text) in &stems {
        let lower = text.to_lowercase();
        let (_, _, model, _, _, extension, _) = &lemmas[*id];
        writeln!(
            output,
            "Stem {{ key:Text({}), lemma:{id}, model:{}, number:{num}, extension:{extension}, v:{}, ae:{}, oe:{}, text:Text({}) }},",
            blob.add(key),
            model_ids[model],
            lower.matches('v').count(),
            lower.matches("āe").count() + lower.matches("prăe").count(),
            lower.matches("ōe").count(),
            blob.add(text),
        )
        .unwrap();
    }
    writeln!(output, "]; ").unwrap();
    let index = hash_index(stems.iter().map(|s| s.0.as_str()));
    writeln!(output, "pub static STEM_INDEX: &[u32] = &{index:?};").unwrap();
    // Stem indices grouped by lemma, for generation.
    let mut by_lemma: Vec<u32> = (0..stems.len() as u32).collect();
    by_lemma.sort_by_key(|&i| (stems[i as usize].1, i));
    let mut starts = vec![0u32; lemmas.len() + 1];
    for &i in &by_lemma {
        starts[stems[i as usize].1 + 1] += 1;
    }
    for i in 1..starts.len() {
        starts[i] += starts[i - 1];
    }
    writeln!(output, "pub static STEMS_BY_LEMMA: &[u32] = &{by_lemma:?};").unwrap();
    writeln!(output, "pub static LEMMA_STEM_STARTS: &[u32] = &{starts:?};").unwrap();
    let mut endings = Vec::new();
    for (name, model) in &models {
        for e in &model.endings {
            endings.push((
                normalize(&e.text),
                model_ids[name],
                e.stem,
                e.morphology,
                e.text.clone(),
            ));
        }
    }
    endings.sort();
    writeln!(output, "pub static ENDINGS: &[Ending] = &[").unwrap();
    for (key, model, stem, morphology, text) in &endings {
        writeln!(output,"Ending {{ key:Text({}), model:{model}, stem:{stem}, morphology:{morphology}, v:{}, text:Text({}) }},", blob.add(key), text.matches('v').count(), blob.add(text)).unwrap();
    }
    writeln!(output, "]; ").unwrap();
    let index = hash_index(endings.iter().map(|e| e.0.as_str()));
    writeln!(output, "pub static ENDING_INDEX: &[u32] = &{index:?};").unwrap();
    let longest = endings.iter().map(|e| e.0.len()).max().unwrap_or(0);
    writeln!(output, "pub const LONGEST_ENDING: usize = {longest};").unwrap();
    // Ending indices grouped by model, for generation.
    let mut model_starts = vec![0u32; names.len() + 1];
    let mut by_model: Vec<u32> = (0..endings.len() as u32).collect();
    by_model.sort_by_key(|&i| (endings[i as usize].1, i));
    for &i in &by_model {
        model_starts[endings[i as usize].1 + 1] += 1;
    }
    for i in 1..model_starts.len() {
        model_starts[i] += model_starts[i - 1];
    }
    writeln!(output, "pub static ENDINGS_BY_MODEL: &[u32] = &{by_model:?};").unwrap();
    writeln!(output, "pub static MODEL_ENDING_STARTS: &[u32] = &{model_starts:?};").unwrap();
    writeln!(output, "pub static TEXT: &str = {:?};", blob.text).unwrap();
    let lemma_models: Vec<usize> = lemmas.iter().map(|l| model_ids[&l.2]).collect();
    writeln!(output, "pub static LEMMA_MODELS: &[u16] = &{lemma_models:?};").unwrap();
    irregulars.sort();
    writeln!(output, "pub static IRREGULARS: &[Irregular] = &[").unwrap();
    for (key, lemma, morphology, text) in irregulars {
        writeln!(
            output,
            "Irregular {{key:{key:?},lemma:{lemma},morphology:{morphology},text:{text:?}}},"
        )
        .unwrap();
    }
    writeln!(output, "]; ").unwrap();
    exclusions.sort();
    exclusions.dedup();
    writeln!(
        output,
        "pub static EXCLUSIONS: &[(usize,u16)] = &{:?};",
        exclusions
    )
    .unwrap();
    for lang in ["en", "fr", "es", "k9"] {
        let data = lines(&format!("data/morphos.{lang}"));
        let values: Vec<_> = data
            .iter()
            .take_while(|s| {
                s.split_once(':')
                    .is_some_and(|(n, _)| n.parse::<u16>().is_ok())
            })
            .map(|s| s.split_once(':').unwrap().1)
            .collect();
        writeln!(
            output,
            "pub static MORPHOLOGY_{}: &[&str] = &{:?};",
            lang.to_uppercase(),
            values
        )
        .unwrap();
    }
    // Tag components derived from each English morphology description, as in
    // `tagging::analysis_tags`: a verb tag (or zeros), a case number (0 when
    // none), and plurality.
    write!(output, "pub static MORPHOLOGY_TAGS: &[([u8;3],u8,bool)] = &[").unwrap();
    for line in lines("data/morphos.en") {
        let Some((number, description)) = line.split_once(':') else {
            break;
        };
        if number.parse::<u16>().is_err() {
            break;
        }
        let modes = ["indicative", "subjunctive", "imperative", "infinitive"];
        let cases = [
            "nominative",
            "vocative",
            "accusative",
            "genitive",
            "dative",
            "ablative",
        ];
        let verb = if description.contains(" -u") {
            "v3 ".to_owned()
        } else if let Some(index) = modes.iter().position(|m| description.contains(m)) {
            format!(
                "v{}{}",
                index + 1,
                if description.contains("present") { '1' } else { ' ' }
            )
        } else {
            "\0\0\0".to_owned()
        };
        let case = cases
            .iter()
            .position(|c| description.contains(c))
            .map_or(0, |i| i + 1);
        write!(
            output,
            "(*b{verb:?},{case},{}),",
            description.contains("plural")
        )
        .unwrap();
    }
    writeln!(output, "];").unwrap();
    // Part-of-speech bits from lemma notes, in `tagging::PART_NEEDLES` order.
    write!(output, "pub static LEMMA_PARTS: &[u16] = &[").unwrap();
    for lemma in &lemmas {
        let details = &lemma.3;
        let mut mask = 0u16;
        for (bit, needles) in [
            &["adj."][..],
            &["conj"],
            &["excl."],
            &["interj"],
            &["num."],
            &["pron."],
            &["prép"],
            &["adv"],
            &[" nom ", "npr."],
        ]
        .iter()
        .enumerate()
        {
            if needles.iter().any(|n| details.contains(n)) {
                mask |= 1 << bit;
            }
        }
        write!(output, "{mask},").unwrap();
    }
    writeln!(output, "];").unwrap();
    let mut by_key: Vec<usize> = (0..lemmas.len()).collect();
    by_key.sort_by(|&a, &b| lemmas[a].0.cmp(&lemmas[b].0).then(a.cmp(&b)));
    writeln!(output, "pub static LEMMAS_BY_KEY: &[u32] = &{by_key:?};").unwrap();
    for (file, name) in [
        ("assimilations.la", "ASSIMILATIONS"),
        ("contractions.la", "CONTRACTIONS"),
    ] {
        let mut values: Vec<_> = lines(&format!("data/{file}"))
            .into_iter()
            .filter_map(|l| {
                l.split_once(':').map(|(a, b)| {
                    if name == "CONTRACTIONS" {
                        (a.to_owned(), b.to_owned())
                    } else {
                        (normalize(a), normalize(b))
                    }
                })
            })
            .collect();
        values.sort();
        writeln!(output, "pub static {name}: &[(&str,&str)] = &{:?};", values).unwrap();
    }
    let mut quantified_assimilations: Vec<_> = lines("data/assimilations.la")
        .into_iter()
        .filter_map(|line| {
            line.split_once(':')
                .map(|(a, b)| (a.to_owned(), b.to_owned()))
        })
        .collect();
    quantified_assimilations.sort();
    writeln!(
        output,
        "pub static QUANTIFIED_ASSIMILATIONS: &[(&str,&str)] = &{:?};",
        quantified_assimilations
    )
    .unwrap();
    let raw = fs::read_to_string("data/tags.la").unwrap();
    let mut in_trigrams = false;
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut trigrams: BTreeMap<String, u64> = BTreeMap::new();
    for line in raw.lines() {
        if line.starts_with("! ---") {
            in_trigrams = true;
            continue;
        }
        if line.is_empty() || line.starts_with('!') {
            continue;
        }
        let (key, count) = line.split_once(',').expect("invalid tag statistics");
        let table = if in_trigrams {
            &mut trigrams
        } else {
            &mut counts
        };
        *table.entry(key.to_owned()).or_default() +=
            count.parse::<u64>().expect("invalid tag count");
    }
    // Tag vocabulary: every three-character tag in the unigram or n-gram
    // statistics. Index 0 is reserved for the empty tag preceding a sentence
    // and index `len + 1` for tags absent from the statistics.
    let mut vocabulary: BTreeSet<String> = counts.keys().cloned().collect();
    for key in trigrams.keys() {
        let bytes = key.as_bytes();
        assert!(
            (bytes.len() == 7 && bytes[3] == b' ')
                || (bytes.len() == 11 && bytes[3] == b' ' && bytes[7] == b' '),
            "unexpected n-gram key {key:?}"
        );
        for start in (0..bytes.len()).step_by(4) {
            vocabulary.insert(key[start..start + 3].to_owned());
        }
    }
    for tag in &vocabulary {
        assert!(tag.len() == 3 && tag.is_ascii(), "unexpected tag {tag:?}");
    }
    let vocabulary: Vec<String> = vocabulary.into_iter().collect();
    let tag_index = |tag: &str| vocabulary.binary_search_by(|t| t.as_str().cmp(tag)).unwrap() + 1;
    let size = vocabulary.len() + 2;
    let mut table = vec![0u32; size * size * size];
    for (key, &count) in &trigrams {
        let index = if key.len() == 7 {
            tag_index(&key[0..3]) * size + tag_index(&key[4..7])
        } else {
            (tag_index(&key[0..3]) * size + tag_index(&key[4..7])) * size + tag_index(&key[8..11])
        };
        table[index] = u32::try_from(count).expect("trigram count exceeds u32");
    }
    let bytes: Vec<u8> = table.iter().flat_map(|n| n.to_le_bytes()).collect();
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("trigrams.bin"),
        bytes,
    )
    .unwrap();
    let fractions: Vec<u64> = vocabulary
        .iter()
        .map(|tag| {
            let occurrences = counts.get(tag).copied().unwrap_or(0);
            if occurrences == 0 {
                return 1024;
            }
            let part = tag.chars().next().unwrap();
            let total: u64 = counts
                .iter()
                .filter(|(key, _)| key.starts_with(part))
                .map(|(_, n)| *n)
                .sum();
            let factor = match part {
                'a' | 'p' | 'w' => 341,
                'v' if tag.ends_with('1') => 512,
                'v' if tag.ends_with(' ') => 256,
                'n' => 1024,
                _ => return 1024,
            };
            (occurrences * factor / total.max(1)).max(1)
        })
        .collect();
    write!(output, "pub static TAG_VOCABULARY: &[[u8;3]] = &[").unwrap();
    for tag in &vocabulary {
        write!(output, "*b{tag:?},").unwrap();
    }
    writeln!(output, "];").unwrap();
    writeln!(output, "pub static TAG_FRACTIONS: &[u64] = &{fractions:?};").unwrap();
    writeln!(
        output,
        "pub static TRIGRAM_TABLE: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/trigrams.bin\"));"
    )
    .unwrap();
    writeln!(
        output,
        "pub static TAG_COUNTS: &[(&str,u64)] = &{:?};",
        counts.into_iter().collect::<Vec<_>>()
    )
    .unwrap();
    let abbreviations = lines("data/abreviations.la");
    writeln!(
        output,
        "pub static ABBREVIATIONS: &[&str] = &{:?};",
        abbreviations
    )
    .unwrap();
    let mut fingerprint = 0xcbf29ce484222325u64;
    for byte in output
        .as_bytes()
        .iter()
        .chain(include_bytes!("src/lib.rs"))
        .chain(include_bytes!("src/normalize.rs"))
    {
        fingerprint ^= u64::from(*byte);
        fingerprint = fingerprint.wrapping_mul(0x100000001b3);
    }
    writeln!(
        output,
        "pub const ANALYSIS_CACHE_FINGERPRINT: u64 = {fingerprint};"
    )
    .unwrap();
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("data.rs"),
        output,
    )
    .unwrap();
}
