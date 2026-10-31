"""Turns the text of an xcursorgen build script into [[pack.xcursor]] tables. The script is only read, never run.

usage: python3 -I xcursorgen2toml.py SCRIPT_TEXT_FILE  > out.toml
Understands: echo "SIZE XHOT YHOT ../DIR/FILE.png DELAY" >(>) cursors/NAME.in ; xcursorgen NAME.in NAME ; ln -s[f] TARGET ALIAS
"""
import re, sys
text = open(sys.argv[1], encoding="utf-8").read()
cfg, built, aliases = {}, [], {}
for m in re.finditer(r'echo\s+"\s*(\d+)\s+(\d+)\s+(\d+)\s+(?:\.\./)?([A-Za-z0-9_./+-]+\.png)\s*(\d*)\s*"\s*>>?\s*cursors/([A-Za-z0-9_.+-]+)\.in', text):
    size, xh, yh, f, delay, name = m.groups()
    cfg.setdefault(name, []).append(f"{size} {xh} {yh} {f}" + (f" {delay}" if delay and delay != "0" else ""))
for m in re.finditer(r'^\s*xcursorgen\s+([A-Za-z0-9_.+-]+)\.in\s+([A-Za-z0-9_.+-]+)\s*$', text, re.M):
    if m.group(1) == m.group(2) and m.group(1) not in built:
        built.append(m.group(1))
for m in re.finditer(r'^\s*ln\s+-sf?\s+([A-Za-z0-9_.+-]+)\s+([A-Za-z0-9_.+-]+)\s*$', text, re.M):
    aliases.setdefault(m.group(1), []).append(m.group(2))
missing = [n for n in built if n not in cfg]
if missing:
    sys.exit(f"no config lines for {missing}")
for n in built:
    print("[[pack.xcursor]]")
    print(f'name = "{n}"')
    print('config = """')
    print("\n".join(cfg[n]))
    print('"""')
    al = [a for a in aliases.get(n, []) if a not in built]
    if al:
        print("aliases = [" + ", ".join(f'"{a}"' for a in al) + "]")
    print()
unresolved = {t: a for t, a in aliases.items() if t not in built}
print(f"# {len(built)} cursors, {sum(len(v) for v in aliases.values())} alias lines; aliases of unknown targets: {unresolved}", file=sys.stderr)
