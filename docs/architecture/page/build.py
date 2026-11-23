"""Build the architecture page: the template with the data file set into it."""
import json, sys
data = json.load(open('arch.json', encoding='utf-8'))
nodes = data['nodes']
# checks: every id named in a layout or an edge exists; ids are plain names
import re
bad = []
for nid, n in nodes.items():
    if not re.fullmatch(r'[a-z0-9][a-z0-9-]*', nid): bad.append('id not plain: ' + nid)
    ids = [i for r in n.get('rows', []) for c in r['cols'] for i in c['ids']]
    for i in ids:
        if i not in nodes: bad.append(f'{nid}: unknown part {i}')
    for e in n.get('edges', []):
        for i in e[:2]:
            if i not in ids: bad.append(f'{nid}: edge names {i}, not in this view')
    for s in n.get("steps", []) + [x for w in n.get("walks", []) for x in w["steps"]]:
        for i in s.get('at', []):
            if i not in ids: bad.append(f'{nid}: step names {i}, not in this view')
if bad:
    print('\n'.join(bad)); sys.exit(1)
# how much of the roadmap is done, from tools/progress.py
import os
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..', 'tools'))
try:
    import progress
    _, total, done = progress.count()
    data['progress'] = f"{sum(done.values())} of {sum(total.values())} packages done ({100 * sum(done.values()) // sum(total.values())}%)"
except Exception:  # the page is still built where the roadmap cannot be read
    data.setdefault('progress', '')
page = open('template.html', encoding='utf-8').read().replace('/*DATA*/null/*END*/', json.dumps(data, ensure_ascii=False).replace('</', '<\\/'))
open(sys.argv[1] if len(sys.argv) > 1 else 'A2MArchitecture.html', 'w', encoding='utf-8').write(page)
print('views:', sum(1 for n in nodes.values() if n.get('rows')), 'parts:', len(nodes), 'bytes:', len(page))
