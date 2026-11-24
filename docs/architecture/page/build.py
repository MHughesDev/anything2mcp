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
    # The graph view draws every arrow as a link between two parts, and lights a step's arrows by their ends.
    pairs = set()
    for e in n.get('edges', []):
        if len(e) < 2 or e[0] not in nodes or e[1] not in nodes or e[0] == e[1]:
            bad.append(f'{nid}: arrow {e[:2]} does not join two different existing parts')
        elif len(e) > 2 and not isinstance(e[2], str):
            bad.append(f'{nid}: arrow {e[:2]} says what it carries in something other than text')
        pairs.add((e[0], e[1]) if len(e) >= 2 else None)
    for s in n.get('steps', []) + [x for w in n.get('walks', []) for x in w['steps']]:
        for i in s.get('at', []):
            if i not in ids:
                bad.append(f'{nid}: step names {i}, not in this view')
        for e in s.get('edges', []):
            if (e[0], e[1]) not in pairs and (e[1], e[0]) not in pairs:
                bad.append(f'{nid}: a step lights {e[0]} to {e[1]}, which is no arrow of this view')

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

# 4. The seam: the outline names the one part that is the boundary between the kernel and the packs. It sits in a
#    station that is followed by another (the packs hang off it), and its view has a row of extension points.
seam = outline.get('seam')
if seam:
    holders = [i for i, st in enumerate(outline['stations']) if any(seam in g['ids'] for g in st['groups'])]
    if len(holders) != 1:
        bad.append(f'outline: the seam {seam} is not in exactly one station')
    elif holders[0] + 1 >= len(outline['stations']):
        bad.append(f'outline: the seam {seam} is in the last station, so nothing hangs off it')
    if not any(re.search('extension', r.get('title', ''), re.I) for r in nodes.get(seam, {}).get('rows', [])):
        bad.append(f'outline: the seam {seam} has no row of extension points to draw on it')

# 5. Every decision record a part cites by number exists, so the reading panel can link to it.
here = os.path.dirname(os.path.abspath(__file__))
decisions = os.path.join(here, '..', '..', 'decisions')
records = {f[:4]: f for f in os.listdir(decisions) if re.match(r'\d{4}-.*\.md$', f)}
cited = set()
for nid, n in nodes.items():
    for key in ('decided', 'weighing'):
        for line in n.get(key) or []:
            for num in re.findall(r'\b(0\d{3})\b', line):
                if num not in records:
                    bad.append(f'{nid}: cites decision {num}, which has no record')
                cited.add(num)
data['records'] = {k: records[k] for k in sorted(cited) if k in records}

# 6. The page itself: the three panels, the map, the one script and its version, and the title the host expects.
template = open('template.html', encoding='utf-8').read()
for need in ('<title>A2MArchitecture</title>', 'id="left"', 'id="graph"', 'id="right"', 'id="reader"', 'id="tree"',
             'https://cdnjs.cloudflare.com/ajax/libs/d3/7.9.0/d3.min.js', ':root:not([data-theme="light"])', ':root[data-theme="dark"]'):
    if need not in template:
        bad.append(f'template.html: missing {need}')
for src in re.findall(r'<script[^>]+src="([^"]+)"|\.src=\'([^\']+)\'', template):
    url = src[0] or src[1]
    if not re.match(r'https://(cdnjs\.cloudflare\.com|cdn\.jsdelivr\.net/npm|unpkg\.com)/.*\d+\.\d+\.\d+', url):
        bad.append(f'template.html: script {url} is not from an allowed host at an exact version')

if bad:
    print('\n'.join(bad))
    sys.exit(1)

# 7. How much of the roadmap is done, from tools/progress.py.
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
if template.count('/*DATA*/null/*END*/') != 1:
    print('template.html must hold the data marker exactly once')
    sys.exit(1)
page = template.replace('/*DATA*/null/*END*/', json.dumps(data, ensure_ascii=False).replace('</', '<\\/'))
open(sys.argv[1] if len(sys.argv) > 1 else 'A2MArchitecture.html', 'w', encoding='utf-8').write(page)
print('stations:', len(outline['stations']), 'shelves:', len(outline['shelves']),
      'views:', sum(1 for n in nodes.values() if n.get('rows')), 'parts:', len(nodes), 'bytes:', len(page))
