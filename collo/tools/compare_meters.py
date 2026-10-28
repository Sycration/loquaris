#!/usr/bin/env python3
"""Compare whole-line meters with the original C++ foot-search routine."""
import argparse
import collections
import json
import pathlib
import subprocess
from compare import ROOT, build_reference, run
from compare_context import corpus_lines, query_reference

def valid_hexameter(schema):
    quantities = ''.join(c for c in schema if c in '+-*')
    def matches(value, expected): return value in (expected, '*')
    def feet(remaining, count):
        if count == 0: return len(remaining) == 2 and matches(remaining[0], '+')
        if len(remaining) >= 2 and matches(remaining[0], '+') and matches(remaining[1], '+') and feet(remaining[2:], count-1): return True
        return len(remaining) >= 3 and matches(remaining[0], '+') and matches(remaining[1], '-') and matches(remaining[2], '-') and feet(remaining[3:], count-1)
    return feet(quantities, 5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=pathlib.Path,default=pathlib.Path('/tmp/opencode/collo-meters'))
    parser.add_argument('--limit',type=int,default=None,help='Maximum lines per poetry text')
    parser.add_argument('--reference-timeout',type=float,default=5)
    args=parser.parse_args(); args.output.mkdir(parents=True,exist_ok=True)
    lines=[]
    for path in sorted((ROOT/'texts').glob('*.txt')):
        if path.name not in ('aeneis10.txt', 'Horace_ Odes I.txt'): continue
        selected=list(corpus_lines(path))
        if args.limit is not None: selected=selected[:args.limit]
        lines.extend((path.name,line) for line in selected)
    reference=build_reference(args.output.resolve(),meter=True)
    run(['cargo','build','--quiet','--example','meter_adapter'])
    expected=query_reference(reference,lines,args.output,args.reference_timeout)
    result=run([str(ROOT/'target/debug/examples/meter_adapter')],input=''.join(line+'\n' for _,line in lines),text=True,stdout=subprocess.PIPE)
    actual=result.stdout.splitlines()
    if len(actual)!=len(lines): raise RuntimeError('Rust output count mismatch')
    totals=collections.defaultdict(collections.Counter)
    with (args.output/'differences.tsv').open('w',encoding='utf8') as out:
        out.write('file\ttext\tcpp_feet\trust_meters\n')
        for (name,text),raw,rust in zip(lines,expected,actual):
            total=totals[name]; total['lines']+=1
            if raw is None: total['failures']+=1; continue
            feet=json.loads(raw)
            if 'unavailable' in feet or 'indeterminate' in feet:
                total['indeterminate']+=1; continue
            schema = next((value.removeprefix('schema:') for value in feet if value.startswith('schema:')), '')
            feet = [value for value in feet if not value.startswith('schema:')]
            # The source accepts spondees in the second half of a pentameter;
            # classify those separately because the Rust API uses the
            # conventional two-dactyl requirement.
            if any(f.endswith('Z') and any(c in 'Ss' for c in f[3:5]) for f in feet):
                total['nonstandard_pentameters']+=1
            original=('H' if any(f.endswith('X') for f in feet) else '') + ('P' if any(f.endswith('Z') and all(c in 'Dd' for c in f[3:5]) for f in feet) else '')
            total['compared']+=1
            if 'H' in rust and 'H' not in original and valid_hexameter(schema):
                total['source_hexameter_misses'] += 1
            if original==rust: total['matches']+=1
            else: out.write(f'{name}\t{text}\t{",".join(feet)}\t{rust}\n')
    for name,total in totals.items():
        print(f"{name}: {total['matches']}/{total['compared']} whole-line meter classifications agree; {total['source_hexameter_misses']} C++ hexameter misses with a valid quantity pattern, {total['indeterminate']} indeterminate reference lines, {total['nonstandard_pentameters']} nonstandard C++ pentameters, {total['failures']} C++ failures")
    print(f'Differences: {args.output/"differences.tsv"}')

if __name__=='__main__':main()
