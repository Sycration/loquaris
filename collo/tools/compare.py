#!/usr/bin/env python3
"""Compare C++ and Rust analyses for unique words in the supplied texts.

Requires Qt 5 development packages, moc, a C++ compiler, Python 3 and Cargo.
The corpus files are read only. Outputs are written under the selected output
folder (default /tmp/opencode/collo-comparison).
"""
import argparse
import collections
import pathlib
import re
import shlex
import subprocess
import unicodedata

ROOT = pathlib.Path(__file__).resolve().parent.parent


def run(command, **kwargs):
    return subprocess.run(command, check=True, cwd=ROOT, **kwargs)


def build_reference(directory, context=False, meter=False):
    flags = shlex.split(subprocess.check_output(["pkg-config", "--cflags", "--libs", "Qt5Core", "Qt5Widgets"], text=True))
    source_names = ["lemCore", "lemme", "modele", "irregs", "ch"]
    if context:
        source_names += ["tagueur", "mot", "scandeur"]
    elif meter:
        source_names += ["scandeur"]
    adapter = 'tools/context_reference.cpp' if context else ('tools/meter_reference.cpp' if meter else 'tools/reference.cpp')
    sources = [ROOT / adapter] + [ROOT / "cpp-src" / f"{name}.cpp" for name in source_names]
    for name in ["lemCore", "lemme", "modele", "irregs"] + (["mot"] if context else []):
        output = directory / f"moc_{name}.cpp"
        run(["moc", str(ROOT / f"cpp-src/{name}.h"), "-o", str(output)])
        sources.append(output)
    executable = directory / ("context_reference" if context else ("meter_reference" if meter else "reference"))
    run(["c++", "-std=c++11", "-O2", "-fPIC", "-I", str(ROOT / "cpp-src"), *map(str, sources), "-o", str(executable), *flags])
    return executable


def words_from(path):
    # Some files mix UTF-8 Latin letters and Windows-1252 punctuation.
    raw = path.read_bytes().decode("utf-8", errors="surrogateescape")
    text = "".join(bytes([ord(c) - 0xdc00]).decode("cp1252") if 0xdc80 <= ord(c) <= 0xdcff else c for c in raw)
    lines = text.splitlines()
    # Remove navigation links, stand-alone poem/chapter headers and the title.
    # Keep the surrounding original prose and verse. Roman numerals are omitted
    # because C++ synthesizes mutable pseudo-lemmas and Rust exposes them through
    # a separate numeric API, not as lexical analyses.
    for line in lines[1:]:
        if '<https://' in line or '<http://' in line:
            break  # Latin Library navigation footer, not source text.
        line = re.sub(r"<#[^>]*>|\[[0-9]+\]|\*[IVXLCDM]+\*", " ", line)
        for match in re.finditer(r"[^\W\d_]+(?:[\u0300-\u036f][^\W\d_]*)*", line, re.UNICODE):
            word = unicodedata.normalize("NFC", match.group())
            if re.fullmatch(r"[IVXLCDM]+", word):
                continue
            yield word


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=pathlib.Path, default=pathlib.Path("/tmp/opencode/collo-comparison"))
    parser.add_argument("--limit", type=int, default=None, help="Limit unique words per text (for quick runs)")
    parser.add_argument("--sentence-start", action="store_true", help="Treat each word as sentence-initial")
    parser.add_argument("--extension", action="store_true", help="Enable the extension lexicon in both analyzers")
    parser.add_argument("--forms", action="store_true", help="Also compare quantified forms and enclitic suffixes")
    parser.add_argument("--tags", action="store_true", help="Compare possible morphological tags for each analysis")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    corpus = {}
    for path in sorted((ROOT / "texts").glob("*.txt")):
        counts = collections.Counter(words_from(path))
        if args.limit is not None:
            counts = collections.Counter(dict(sorted(counts.items())[:args.limit]))
        corpus[path.name] = counts
    all_words = sorted({word for counts in corpus.values() for word in counts})
    executable = build_reference(args.output.resolve())
    run(["cargo", "build", "--quiet", "--example", "reference_adapter"])
    request = "".join(word + "\n" for word in all_words)
    options = (["--sentence-start"] if args.sentence_start else []) + (["--extension"] if args.extension else []) + (["--forms"] if args.forms else []) + (["--tags"] if args.tags else [])
    cpp = run([str(executable), str(ROOT / "data"), *options], input=request, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    rust = run([str(ROOT / "target/debug/examples/reference_adapter"), *options], input=request, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    expected, actual = cpp.stdout.splitlines(), rust.stdout.splitlines()
    if len(expected) != len(all_words) or len(actual) != len(all_words):
        raise RuntimeError(f"output count mismatch: words={len(all_words)}, C++={len(expected)}, Rust={len(actual)}; C++ stderr: {cpp.stderr[:1000]}")
    comparisons = {word: (set(a.split('\t')) - {''}, set(b.split('\t')) - {''}) for word, a, b in zip(all_words, expected, actual)}
    with (args.output / "differences.tsv").open("w", encoding="utf-8") as output:
        output.write("word\tcpp_only\trust_only\n")
        for word, (a, b) in comparisons.items():
            if a != b:
                output.write(f"{word}\t{','.join(sorted(a-b))}\t{','.join(sorted(b-a))}\n")
    fields = 'lemma key, morphology ID'
    if args.forms: fields += ', form, suffix'
    if args.tags: fields += ', morphological tag'
    print(f"Comparison: unique word forms, exact ({fields}) sets; sentence_start={args.sentence_start}; extension={args.extension}")
    for name, counts in corpus.items():
        equal = sum(comparisons[word][0] == comparisons[word][1] for word in counts)
        missed = sum(bool(comparisons[word][0]) and not comparisons[word][1] for word in counts)
        extra = sum(not comparisons[word][0] and bool(comparisons[word][1]) for word in counts)
        print(f"{name}: {equal}/{len(counts)} exact ({100*equal/max(1,len(counts)):.1f}%); C++ only recognized {missed}, Rust only recognized {extra}; {sum(counts.values())} tokens")
    print(f"Differences: {args.output / 'differences.tsv'}")
    if cpp.stderr.strip():
        (args.output / "reference-stderr.txt").write_text(cpp.stderr, encoding="utf-8")


if __name__ == "__main__":
    main()
