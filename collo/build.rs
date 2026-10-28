#[path = "src/normalize.rs"]
mod normalize;
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
fn main() {
    println!("cargo:rerun-if-changed=data");
    println!("cargo:rerun-if-changed=src/normalize.rs");
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
    writeln!(output, "pub static STEMS: &[Stem] = &[").unwrap();
    for (key, id, num, text) in stems {
        writeln!(
            output,
            "Stem {{ key:{key:?}, lemma:{id}, number:{num}, text:{text:?} }},"
        )
        .unwrap();
    }
    writeln!(output, "]; ").unwrap();
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
    for (key, model, stem, morphology, text) in endings {
        writeln!(output,"Ending {{ key:{key:?}, model:{model}, stem:{stem}, morphology:{morphology}, text:{text:?} }},").unwrap();
    }
    writeln!(output, "]; ").unwrap();
    writeln!(output, "pub static MODEL_NAMES: &[&str] = &{:?};", names).unwrap();
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
    writeln!(
        output,
        "pub static TAG_COUNTS: &[(&str,u64)] = &{:?};",
        counts.into_iter().collect::<Vec<_>>()
    )
    .unwrap();
    writeln!(
        output,
        "pub static TRIGRAM_COUNTS: &[(&str,u64)] = &{:?};",
        trigrams.into_iter().collect::<Vec<_>>()
    )
    .unwrap();
    let abbreviations = lines("data/abreviations.la");
    writeln!(
        output,
        "pub static ABBREVIATIONS: &[&str] = &{:?};",
        abbreviations
    )
    .unwrap();
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("data.rs"),
        output,
    )
    .unwrap();
}
