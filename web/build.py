"""Builds the game from rally_template.html:
  dist/fogwood-rally.html        standalone browser build (three.js from CDN)
  dist/fogwood-rally-debug.html  same with the tuning panel
  ../android/app/src/main/assets/index.html   app build (three.js bundled, offline)
"""
import base64, os
D = os.path.dirname(os.path.abspath(__file__)) + '/'
s = open(D + 'rally_template.html').read()
for k, f in [('__WIND__', 'wind.mp3'), ('__BIRDS__', 'birds.mp3'), ('__ENGINE__', 'engine.wav'), ('__SKID__', 'skid.wav'), ('__IMPACT__', 'impact.wav')]:
    s = s.replace(k, base64.b64encode(open(D + f, 'rb').read()).decode())
HEAD = '<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">'
def page(x):
    i = x.index('</style>') + len('</style>')
    return HEAD + x[:i] + '</head><body>' + x[i:] + '</body></html>'
main = page(s.replace('/*__DEBUG_PANEL__*/', ''))
dbg = page(s.replace('/*__DEBUG_PANEL__*/', open(D + 'debug_panel.js').read()).replace('<title>Fogwood Rally</title>', '<title>Fogwood Rally Debug</title>', 1))
os.makedirs(D + 'dist', exist_ok=True)
open(D + 'dist/fogwood-rally.html', 'w').write(main)
open(D + 'dist/fogwood-rally-debug.html', 'w').write(dbg)
CDN = 'https://cdn.jsdelivr.net/npm/three@0.160.0/build/three.module.min.js'
open(D + '../android/app/src/main/assets/index.html', 'w').write(main.replace(CDN, './three.module.min.js'))
print('built', len(main), 'bytes')
