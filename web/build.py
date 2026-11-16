"""Builds the game from rally_template.html:
  dist/fogwood-rally.html        standalone browser build (three.js from CDN)
  dist/fogwood-rally-debug.html  same with the tuning panel
  ../android/app/src/main/assets/index.html   app build (three.js bundled, offline)
"""
import base64, os
D = os.path.dirname(os.path.abspath(__file__)) + '/'
s = open(D + 'rally_template.html').read()
# the sound engine compiled to WebAssembly (CI builds it from rust/audio; the browser runs it in an AudioWorklet,
# the app uses the native build of the same code and only falls back to this)
W = D + 'fogwood_audio.wasm'
s = s.replace('__AUDIOWASM__', base64.b64encode(open(W, 'rb').read()).decode() if os.path.exists(W) else '')
for k, f in [('__WIND__', 'wind.mp3'), ('__BIRDS__', 'birds.mp3'), ('__ENGINE__', 'engine.wav'), ('__SKID__', 'skid.wav'), ('__IMPACT__', 'impact.wav'),
             ('__WATER_A__', 'snd/water_a.mp3'), ('__WATER_B__', 'snd/water_b.mp3'), ('__TRAFFIC__', 'snd/traffic.mp3'), ('__SPLASH__', 'snd/splash.mp3'),
             ('__METAL__', 'snd/metal.mp3'), ('__PLANK__', 'snd/plank.mp3'), ('__SOFT__', 'snd/soft.mp3'), ('__WIND2__', 'snd/wind2.mp3')]:
    s = s.replace(k, base64.b64encode(open(D + f, 'rb').read()).decode())
HEAD = '<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">'
# home-screen web app (iPhone: Safari > Share > Add to Home Screen opens it full screen, with the icon)
HEAD += ('<meta name="apple-mobile-web-app-capable" content="yes"><meta name="mobile-web-app-capable" content="yes">'
         '<meta name="apple-mobile-web-app-status-bar-style" content="black-translucent"><meta name="apple-mobile-web-app-title" content="Fogwood">'
         '<meta name="theme-color" content="#0b100b"><link rel="icon" href="icon-180.png"><link rel="apple-touch-icon" href="icon-180.png"><link rel="manifest" href="manifest.webmanifest">')
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
# browser build of the native game (GitHub Pages): same page, its main script started by web_boot.js once the
# WebAssembly engine is ready (CI adds fogwood_core.js / fogwood_core_bg.wasm from rust/)
WEB = D + 'dist-web/'; os.makedirs(WEB, exist_ok=True)
APPA = D + '../android/app/src/main/assets/'
web = main.replace(CDN, './three.module.min.js')
k = web.index('<script type="module">')
web = web[:k] + '<script type="text/fogwood" id="fwmain">' + web[k + len('<script type="module">'):]
web = web.replace('</body>', '<script type="module" src="web_boot.js"></script></body>')
open(WEB + 'index.html', 'w').write(web)
import shutil
for f in ['three.module.min.js', 'icon-180.png', 'icon-512.png', 'manifest.webmanifest']: shutil.copy(APPA + f, WEB + f)
shutil.copy(D + 'web_boot.js', WEB + 'web_boot.js')
print('built', len(main), 'bytes')
