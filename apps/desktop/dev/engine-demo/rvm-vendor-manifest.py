from pathlib import Path
import hashlib
vendor = Path(__file__).resolve().parents[2] / 'ui/engine/vendor'
p = vendor / 'README.md'
# Preserve the original MediaPipe table; replace only the final RVM table.
text = p.read_text()
marker = 'New assets, including the documented derived graph'
start = text.index(marker)
body = text[:start] + text[start:].split('| File | Bytes | SHA-256 |', 1)[0]
body += '| File | Bytes | SHA-256 |\n| --- | ---: | --- |\n'
total = 0
for folder in ['ort', 'rvm']:
    for f in sorted((vendor/folder).iterdir()):
        if not f.is_file():
            continue
        b = f.read_bytes()
        total += len(b)
        body += f'| `{f.relative_to(vendor)}` | {len(b)} | `{hashlib.sha256(b).hexdigest()}` |\n'
body += f'\nAdded RVM/runtime assets: **{total:,} bytes**.\n'
p.write_text(body)
print(total)
