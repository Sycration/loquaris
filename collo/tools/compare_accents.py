#!/usr/bin/env python3
"""Compare the original's six accent modes on representative corpus words."""
import argparse
import collections
import html
import json
import pathlib
import re
import subprocess
import unicodedata
from compare import ROOT, build_reference, run, words_from
from compare_context import query_reference


def normalize(text):
    text = text.replace('æ','ae').replace('Æ','Ae').replace('œ','oe').replace('Œ','Oe')
    text = ''.join(c for c in unicodedata.normalize('NFD', text) if not unicodedata.combining(c))
    return text.translate(str.maketrans({'v':'u','V':'U','j':'i','J':'I','ı':'i','ụ':'u'})).lower()


def source_accent(output, syllables):
    # First output is the reference's best-ranked reading; parenthesized
    # alternatives follow. Unknown words are wrapped in <em>.
    first = re.sub(r'<[^>]*>', '', output).strip().split(' ', 1)[0]
    first = html.unescape(first)
    decomposed = unicodedata.normalize('NFD', first)
    accent = decomposed.find('\u0301')
    if accent < 0:
        if '<em>' not in output and len(syllables) == 2:
            # Collatinus leaves disyllables unmarked; their accent is implicit.
            return 0
        return None
    position = len(normalize(decomposed[:accent])) - 1
    if normalize(first) != ''.join(normalize(s) for s in syllables):
        return 'unaligned'
    total = 0
    for i, syllable in enumerate(syllables):
        total += len(normalize(syllable))
        if position < total: return i
    return 'unaligned'


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=pathlib.Path,default=pathlib.Path('/tmp/opencode/collo-accents'))
    parser.add_argument('--limit',type=int,default=2000,help='Evenly spaced unique words; 0 selects all')
    parser.add_argument('--reference-timeout',type=float,default=5)
    args=parser.parse_args(); args.output.mkdir(parents=True,exist_ok=True)
    corpus=collections.defaultdict(set)
    for path in sorted((ROOT/'texts').glob('*.txt')):
        for word in words_from(path): corpus[word].add(path.name)
    words=sorted(corpus)
    if args.limit and args.limit < len(words):
        words=[words[i*len(words)//args.limit] for i in range(args.limit)]
    executable=build_reference(args.output.resolve(),context=True)
    run(['cargo','build','--quiet','--example','accent_adapter'])
    cpp=query_reference(executable, [('',word) for word in words], args.output, args.reference_timeout, ['--accent'])
    rust=run([str(ROOT/'target/debug/examples/accent_adapter')],input=''.join(w+'\n' for w in words),text=True,stdout=subprocess.PIPE)
    rows=rust.stdout.splitlines()
    if len(rows)!=len(words): raise RuntimeError('Rust adapter output count mismatch')
    totals=collections.Counter()
    with (args.output/'differences.tsv').open('w',encoding='utf8') as out:
        out.write('word\tmode\tcpp\trust\tcpp_output\n')
        for word,reference,line in zip(words,cpp,rows):
            totals['words']+=1
            if reference is None: totals['failures']+=1; continue
            output=json.loads(reference)
            accents,readings=line.split('\t')
            for mode,(source,actual,reading) in enumerate(zip(output[2:],accents.split(','),readings.split(';'))):
                if '<em>' in source:
                    totals['unknown'] += 1
                    continue  # The original returns unknown words unchanged.
                parts=reading.split('|') if reading else []
                expected=source_accent(source,parts)
                if expected=='unaligned': totals['unaligned']+=1; continue
                actual=None if actual=='-' else int(actual)
                totals['compared']+=1
                if expected==actual: totals['matches']+=1
                else: out.write(f'{word}\t{[1,2,3,9,10,11][mode]}\t{expected}\t{actual}\t{source.strip()}\n')
    print(f"{totals['words']} sampled words; {totals['matches']}/{totals['compared']} accent modes agree; {totals['unknown']} unknown-mode outputs, {totals['unaligned']} unaligned spellings, {totals['failures']} C++ failures")
    print(f"Differences: {args.output/'differences.tsv'}")

if __name__=='__main__':main()
