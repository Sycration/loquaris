#!/usr/bin/env python3
"""Compare sentence tags and per-word quantity patterns against Qt Collatinus."""
import argparse
import collections
import html
import json
import pathlib
import re
import select
import subprocess
from compare import ROOT, build_reference, run


def corpus_lines(path):
    raw = path.read_bytes().decode('utf-8', errors='surrogateescape')
    text = ''.join(bytes([ord(c)-0xdc00]).decode('cp1252') if 0xdc80 <= ord(c) <= 0xdcff else c for c in raw)
    for line in text.splitlines()[1:]:
        if '<https://' in line or '<http://' in line:
            break  # Latin Library navigation footer, not source text.
        line = re.sub(r'<#[^>]*>|\[[0-9]+\]|\*[IVXLCDM]+\*|\d+', ' ', line)
        line = line.strip()
        if re.search(r'[^\W\d_]', line, re.UNICODE):
            yield line

def corpus_sentences(path):
    # Join wrapped prose/verse lines before splitting on the punctuation used by
    # the original tagger. Abbreviation periods are retained within a sentence.
    text = ' '.join(corpus_lines(path))
    abbreviations = {line.strip() for line in (ROOT/'data/abreviations.la').read_text(encoding='utf8').splitlines() if line and not line.startswith('!')}
    start = 0
    for match in re.finditer(r'[.!?;]+(?:\s+|$)', text):
        end = match.end()
        if match.group().startswith('.'):
            previous = re.search(r'([^\W\d_]+)\.$', text[start:match.start()+1], re.UNICODE)
            if previous and previous.group(1) in abbreviations:
                continue
        sentence = text[start:end].strip()
        if re.search(r'[^\W\d_]', sentence, re.UNICODE):
            yield sentence
        start = end
    tail = text[start:].strip()
    if re.search(r'[^\W\d_]', tail, re.UNICODE):
        yield tail


def reference_tags(fragment):
    # Unknown words have their own distinct HTML element.
    pattern = r"<li id='S_\d+_W_\d+'><strong>(.*?)</strong> (.{3})|<li id='unknown'>(.*?) : non trouvé</li>"
    return [(html.unescape(word), tag) if word else (html.unescape(unknown), '???') for word, tag, unknown in re.findall(pattern, fragment)]


def reference_scan(fragment):
    match = re.search(r'&nbsp;<small>(.*?)</small>', fragment, re.S)
    if not match:
        return []
    text = html.unescape(match.group(1)).replace('∪̲', '*').replace('∪', '-').replace('‑', '+')
    # A word with no detectable vowels has an empty pattern. Preserve those
    # fields; ordinary split() would shift all later words on the line.
    parts = text.split(' ')
    if parts and parts[-1] == '': parts.pop()
    return parts


def query_reference(executable, lines, directory, timeout, extra_args=()):
    """Isolate a reference hang/crash to one input and continue the corpus."""
    answers = []
    process = None
    count = 0
    with (directory / 'reference-stderr.txt').open('w', encoding='utf8') as errors:
        def stop():
            nonlocal process
            if process is not None:
                process.kill()
                process.wait()
                process.stdin.close()
                process.stdout.close()
                process = None
        try:
            for _, text in lines:
                if process is None or count >= 250:
                    stop()
                    process = subprocess.Popen([str(executable), str(ROOT/'data'), *extra_args], cwd=ROOT,
                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=errors,
                        text=True, encoding='utf8', bufsize=1)
                    count = 0
                try:
                    process.stdin.write(text + '\n')
                    process.stdin.flush()
                    ready, _, _ = select.select([process.stdout], [], [], max(timeout, 15) if count == 0 else timeout)
                    answer = process.stdout.readline() if ready else ''
                except (BrokenPipeError, OSError):
                    answer = ''
                count += 1
                if not answer:
                    answers.append(None)
                    stop()
                else:
                    answers.append(answer.rstrip('\n'))
        finally:
            stop()
    return answers


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=pathlib.Path, default=pathlib.Path('/tmp/opencode/collo-context'))
    parser.add_argument('--limit', type=int, default=None, help='Maximum lines per text')
    parser.add_argument('--sentences', action='store_true', help='Join wrapped lines and compare complete sentences')
    parser.add_argument('--reference-timeout', type=float, default=5, help='Seconds to allow the C++ reference for each input')
    args=parser.parse_args(); args.output.mkdir(parents=True, exist_ok=True)
    lines=[]
    for path in sorted((ROOT/'texts').glob('*.txt')):
        selected=list(corpus_sentences(path) if args.sentences else corpus_lines(path))
        if args.limit is not None: selected=selected[:args.limit]
        lines.extend((path.name, line) for line in selected)
    reference=build_reference(args.output.resolve(), context=True)
    run(['cargo','build','--quiet','--example','context_adapter'])
    request=''.join(line+'\n' for _,line in lines)
    reference_lines = query_reference(reference, lines, args.output, args.reference_timeout)
    rust=run([str(ROOT/'target/debug/examples/context_adapter')],input=request,text=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
    rust_lines=rust.stdout.splitlines()
    if len(reference_lines)!=len(lines) or len(rust_lines)!=len(lines):
        raise RuntimeError(f'Output count mismatch: inputs={len(lines)} C++={len(reference_lines)} Rust={len(rust_lines)}')
    totals=collections.defaultdict(collections.Counter)
    abbreviations = {line.strip() for line in (ROOT/'data/abreviations.la').read_text(encoding='utf8').splitlines() if line and not line.startswith('!')}
    with (args.output/'differences.tsv').open('w',encoding='utf8') as out:
        out.write('file\ttext\tcomponent\tindex\tword\tcpp\trust\n')
        for (name,text),cpp_line,rust_line in zip(lines,reference_lines,rust_lines):
            count=totals[name]; count['lines']+=1
            if cpp_line is None:
                count['reference_failures'] += 1
                out.write(f'{name}\t{text}\treference-failure\t\t\ttimeout or crash\t\n')
                continue
            tagged,scanned=json.loads(cpp_line); ctags=reference_tags(tagged); cs=reference_scan(scanned)
            rtags,rscan,rwords=rust_line.split('\t'); rtags=rtags.split('|') if rtags else []; rscan=rscan.split('|') if rscan else []; rwords=rwords.split('|') if rwords else []
            # The original skips words with analyses but no tag while building
            # its path, then incorrectly consumes a tag for them when rendering
            # HTML. Its printed labels after that word are shifted and do not
            # represent the actual Viterbi path, so do not score that sentence.
            if '( ? )' in tagged or any(tag == '???' and word in abbreviations for word, tag in ctags):
                count['tag_unassignable_sequences'] += 1
                out.write(f'{name}\t{text}\ttag-unassignable\t\t\tC++ rendered a shifted sequence\t\n')
            elif len(ctags)!=len(rtags) or [word for word, _ in ctags] != rwords:
                count['tag_alignment_failures']+=1
                out.write(f'{name}\t{text}\ttag-alignment\t\t\t{[word for word, _ in ctags]}\t{rwords}\n')
            else:
                for i,((word,a),b) in enumerate(zip(ctags,rtags)):
                    count['tags']+=1
                    if a==b: count['tag_matches']+=1
                    else: out.write(f'{name}\t{text}\ttag\t{i}\t{word}\t{a}\t{b}\n')
            if len(cs)!=len(rscan):
                count['scan_alignment_failures']+=1
                out.write(f'{name}\t{text}\tscan-alignment\t\t\t{len(cs)}\t{len(rscan)}\n')
            else:
                for i,(a,b) in enumerate(zip(cs,rscan)):
                    if a.startswith(('@','?')): count['scan_indeterminate']+=1; continue
                    count['scan_words']+=1
                    if a==b: count['scan_matches']+=1
                    else: out.write(f'{name}\t{text}\tscan\t{i}\t{ctags[i][0] if i<len(ctags) else ""}\t{a}\t{b}\n')
    for name,c in totals.items():
        print(f"{name}: {c['lines']} lines; C++ failures {c['reference_failures']}; tags {c['tag_matches']}/{c['tags']} ({100*c['tag_matches']/max(1,c['tags']):.1f}%), alignment failures {c['tag_alignment_failures']}, C++ unassignable sequences {c['tag_unassignable_sequences']}; quantities {c['scan_matches']}/{c['scan_words']} ({100*c['scan_matches']/max(1,c['scan_words']):.1f}%), indeterminate {c['scan_indeterminate']}, alignment failures {c['scan_alignment_failures']}")
    print(f'Differences: {args.output / "differences.tsv"}')

if __name__=='__main__': main()
