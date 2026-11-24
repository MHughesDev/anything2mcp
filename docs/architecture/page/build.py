"""Build the architecture page: the template with the data and the outline set into it."""
import json, os, re, sys

data = json.load(open('arch.json', encoding='utf-8'))
outline = json.load(open('outline.json', encoding='utf-8'))
nodes = data['nodes']
bad = []


def parts_of(node):
    return [i for r in node.get('rows', []) for c in r['cols'] for i in c['ids']]


# 1. Every part a view names exists; an arrow and a step name only parts of their own view.
for nid, n in nodes.items():
    if not re.fullmatch(r'[a-z0-9][a-z0-9-]*', nid):
        bad.append('id not plain: ' + nid)
    ids = parts_of(n)
    for i in ids:
        if i not in nodes:
            bad.append(f'{nid}: unknown part {i}')
    for e in n.get('edges', []):
        for i in e[:2]:
            if i not in ids:
                bad.append(f'{nid}: edge names {i}, not in this view')
    for s in n.get('steps', []) + [x for w in n.get('walks', []) for x in w['steps']]:
        for i in s.get('at', []):
            if i not in ids:
                bad.append(f'{nid}: step names {i}, not in this view')

# 2. The outline: the first screen's stations and shelves. Every part of the root view is in
#    exactly one of them, so the short first screen leaves nothing out and shows nothing twice.
RESERVED = {'settings', 'sidebar', 'customize', 'upgrade', 'downloads', 'overview'}
placed = []
for kind in ('stations', 'shelves'):
    for st in outline[kind]:
        sid = st['id']
        if not re.fullmatch(r'[a-z][a-z0-9-]*', sid) or sid in nodes or sid in RESERVED or sid.startswith('comment-'):
            bad.append(f'outline: {sid} is not a free plain name')
        for key in ('name', 'short', 'text'):
            if not st.get(key):
                bad.append(f'outline: {sid} has no {key}')
        for g in st['groups']:
            for i in g['ids']:
                if i not in nodes:
                    bad.append(f'outline: {sid} names unknown part {i}')
                placed.append(i)
root_parts = parts_of(nodes[data['root']])
for i in sorted(set(root_parts) - set(placed)):
    bad.append(f'outline: {i} is in the root view and in no station or shelf')
for i in sorted(set(placed) - set(root_parts)):
    bad.append(f'outline: {i} is in a station or shelf and not in the root view')
for i in sorted({i for i in placed if placed.count(i) > 1}):
    bad.append(f'outline: {i} is placed twice')
if len(outline['stations']) > 6:
    bad.append('outline: more than six stations; the first screen is meant to be short')

# 3. Every part can be reached by opening parts from the first screen.
seen, todo = set(), list(placed)
while todo:
    i = todo.pop()
    if i in seen or i not in nodes:
        continue
    seen.add(i)
    todo.extend(parts_of(nodes[i]))
for i in sorted(set(nodes) - seen - {data['root']}):
    bad.append(f'{i} cannot be reached from the first screen')

if bad:
    print('\n'.join(bad))
    sys.exit(1)

# 4. How much of the roadmap is done, from tools/progress.py.
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..', 'tools'))
try:
    import progress
    _, total, done = progress.count()
    data['percent'] = 100 * sum(done.values()) // sum(total.values())
    data['progress'] = f"{sum(done.values())} of {sum(total.values())} packages"
except Exception:  # the page is still built where the roadmap cannot be read
    data.setdefault('progress', '')
    data.setdefault('percent', None)

data['outline'] = outline
template = open('template.html', encoding='utf-8').read()
if template.count('/*DATA*/null/*END*/') != 1:
    print('template.html must hold the data marker exactly once')
    sys.exit(1)
page = template.replace('/*DATA*/null/*END*/', json.dumps(data, ensure_ascii=False).replace('</', '<\\/'))
open(sys.argv[1] if len(sys.argv) > 1 else 'A2MArchitecture.html', 'w', encoding='utf-8').write(page)
print('stations:', len(outline['stations']), 'shelves:', len(outline['shelves']),
      'views:', sum(1 for n in nodes.values() if n.get('rows')), 'parts:', len(nodes), 'bytes:', len(page))
