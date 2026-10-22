"""Rename presentation strings without invalidating existing device trust or origins."""
from pathlib import Path
import json

root = Path(__file__).resolve().parent.parent
for folder in ['apps/desktop/src', 'apps/ipad/src']:
    for p in (root / folder).rglob('*'):
        if p.suffix not in ('.tsx', '.ts', '.css'): continue
        if p.name == 'platform.ts': continue  # Wire product name is backwards compatible.
        s = p.read_text(encoding='utf-8')
        p.write_text(s.replace('Vision-Pro', 'Record OS').replace('Vision Pro', 'Record OS'), encoding='utf-8')
for rel in ['apps/desktop/index.html', 'apps/ipad/index.html', 'apps/ipad/public/manifest.webmanifest',
            'apps/desktop/src-tauri/src/main.rs', 'agent/crates/vp-agent/src/bootstrap.html',
            'agent/crates/vp-agent/src/bootstrap.js', 'agent/crates/vp-agent/src/tls.rs']:
    p = root / rel
    p.write_text(p.read_text(encoding='utf-8').replace('Vision-Pro', 'Record OS'), encoding='utf-8')
for rel in ['agent/crates/vp-agent/src/agent.rs', 'agent/crates/vp-agent/src/relay.rs', 'agent/crates/vp-agent/src/server.rs']:
    p = root / rel
    s = p.read_text(encoding='utf-8').replace('Vision-Pro', 'Record OS')
    s = s.replace('"Record OS/1 relay-register"', '"Vision-Pro/1 relay-register"')
    p.write_text(s, encoding='utf-8')
# Preserve protocol labels, IndexedDB name, data directories, package names and public relay URL.
p = root / 'apps/desktop/src-tauri/tauri.conf.json'
c = json.loads(p.read_text(encoding='utf-8'))
c['productName'] = 'Record OS'
c['version'] = '0.1.4'
c['app']['windows'][0]['title'] = 'Record OS'
c['bundle']['publisher'] = 'Record OS'
c['bundle']['copyright'] = 'Copyright © 2026 Record OS'
c['bundle']['shortDescription'] = 'Votre PC, à portée de main depuis votre iPad.'
c['bundle']['windows']['nsis']['languages'] = ['French']
c['bundle']['windows']['nsis']['installerHooks'] = 'installer/record-os.nsh'
p.write_text(json.dumps(c, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
p = root / 'apps/desktop/src-tauri/Cargo.toml'
p.write_text(p.read_text(encoding='utf-8').replace('version = "0.1.3"', 'version = "0.1.4"').replace('description = "Vision-Pro Windows desktop agent"', 'description = "Record OS Windows desktop agent"'), encoding='utf-8')
p = root / 'apps/ipad/public/sw.js'
p.write_text(p.read_text(encoding='utf-8').replace('vision-pro-shell-v11', 'vision-pro-shell-v12'), encoding='utf-8')
