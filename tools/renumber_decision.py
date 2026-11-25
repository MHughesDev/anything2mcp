"""Renumber a decision record, run from the root of a copy of the repository.
A builder numbers its new record 0999; the lead gives it the next free number on merge.
Usage: python3 tools/renumber_decision.py <old-file-name> <new-number>
Renames the file, its heading, every link to it, the 'decision NNNN'/'[NNNN]' text of those links,
and the index row; then sets docs/README.md's decision count to the number of record files."""
import glob, os, re, sys

old, new = sys.argv[1], sys.argv[2]
oldnum = old[:4]
newname = new + old[4:]
os.rename(f'docs/decisions/{old}', f'docs/decisions/{newname}')
p = f'docs/decisions/{newname}'
s = open(p).read()
s = re.sub(rf'^# {oldnum}\.', f'# {new}.', s, count=1, flags=re.M)
open(p, 'w').write(s)
files = [f for pat in ('**/*.md', '**/*.rs', '**/*.py', '**/*.toml', '**/*.json') for f in glob.glob(pat, recursive=True)
         if not f.startswith(('target/', '.claude/'))]
for f in files:
    try:
        s = open(f).read()
    except (UnicodeDecodeError, IsADirectoryError):
        continue
    if old not in s:
        continue
    s = s.replace(old, newname)
    # the visible number of a link to the renamed file
    s = re.sub(rf'(decision |decisions? |\[){oldnum}(\]?\]?\([^)]*{re.escape(newname)})', rf'\g<1>{new}\2', s)
    s = re.sub(rf'\[decision {oldnum}\](\([^)]*{re.escape(newname)})', rf'[decision {new}]\1', s)
    s = re.sub(rf'\| {oldnum} \| (\[[^\]]*\]\({re.escape(newname)}\))', rf'| {new} | \1', s)
    open(f, 'w').write(s)
n = len([f for f in os.listdir('docs/decisions') if re.match(r'\d{4}-', f)])
p = 'docs/README.md'
s = open(p).read()
s = re.sub(r'(\| Decision records: context, choice, consequences, alternatives \| )\d+ \|', lambda m: f'{m.group(1)}{n} |', s)
open(p, 'w').write(s)
print('renamed to', newname, '; records:', n)
