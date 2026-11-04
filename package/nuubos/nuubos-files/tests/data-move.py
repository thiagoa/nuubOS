#!/usr/bin/env python3
# Host test: nuubos-data-move (Data on SD2, GitHub #24). Checks: files of
# every pair move, nested folders too; a file the destination already has
# with the same content only goes from the source; a different one stays at
# the destination and the source's copy lands in the conflicts folder; the
# source folders stay (emptied), no staging left; moving back works; no
# room refuses before touching anything.
# Run: python3 package/nuubos/nuubos-files/tests/data-move.py
import os, subprocess, sys, tempfile

here = os.path.dirname(os.path.abspath(__file__))
src = os.path.join(here, '..', 'src', 'data-move.c')
t = tempfile.mkdtemp()
exe = t + '/data-move'
subprocess.check_call(['gcc', '-std=c11', '-D_GNU_SOURCE', '-Wall', '-Wextra', '-Werror', src, '-o', exe])
fails = 0
def check(name, cond):
    global fails
    if not cond:
        print('[FAIL]', name); fails += 1
def w(path, data):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    open(path, 'w').write(data)
def r(path):
    try: return open(path).read()
    except OSError: return None
def files(root):
    out = []
    for d, _, fs in os.walk(root):
        out += [os.path.relpath(os.path.join(d, f), root) for f in fs]
    return sorted(out)

sd1, sd2 = t + '/sd1', t + '/sd2/nuubOS'
w(sd1 + '/bios/scph1001.bin', 'bios'); w(sd1 + '/bios/fbneo/neogeo.zip', 'neo')
w(sd1 + '/users/u1/saves/snes/Zelda.srm', 'zelda-sd1')
w(sd1 + '/users/u1/saves/gba/Same.sav', 'same')
w(sd1 + '/users/u1/saves/gba/Clash.sav', 'clash-sd1')
w(sd2 + '/users/u1/saves/gba/Same.sav', 'same')
w(sd2 + '/users/u1/saves/gba/Clash.sav', 'clash-sd2')
conf = sd2 + '/conflicts/20261029-120000'

args = [exe, conf, 'bios', sd1 + '/bios', sd2 + '/bios',
        'users/u1/saves', sd1 + '/users/u1/saves', sd2 + '/users/u1/saves']
out = subprocess.run(args, capture_output=True, text=True)
check('done, one conflict', out.returncode == 0 and '@done 1' in out.stdout)
check('progress reported', '@progress 100' in out.stdout)
check('files moved', r(sd2 + '/bios/scph1001.bin') == 'bios' and r(sd2 + '/bios/fbneo/neogeo.zip') == 'neo'
      and r(sd2 + '/users/u1/saves/snes/Zelda.srm') == 'zelda-sd1')
check('destination copy kept', r(sd2 + '/users/u1/saves/gba/Clash.sav') == 'clash-sd2')
check('conflicting copy kept aside', r(conf + '/users/u1/saves/gba/Clash.sav') == 'clash-sd1')
check('same file kept once', r(sd2 + '/users/u1/saves/gba/Same.sav') == 'same')
check('sources emptied, roots stay', files(sd1) == [] and os.path.isdir(sd1 + '/bios')
      and os.path.isdir(sd1 + '/users/u1/saves') and os.listdir(sd1 + '/bios') == [])
check('no staging left', not [n for n in os.listdir(sd2) + os.listdir(sd2 + '/users/u1') if n.endswith('.nuubos-move')])

# Back to SD1: everything moves (the conflicts folder stays on SD2).
out = subprocess.run([exe, sd1 + '/conflicts/x', 'bios', sd2 + '/bios', sd1 + '/bios',
                      'users/u1/saves', sd2 + '/users/u1/saves', sd1 + '/users/u1/saves'], capture_output=True, text=True)
check('back, no conflicts', out.returncode == 0 and '@done 0' in out.stdout
      and r(sd1 + '/users/u1/saves/gba/Clash.sav') == 'clash-sd2' and r(sd1 + '/bios/fbneo/neogeo.zip') == 'neo'
      and files(sd2 + '/bios') == [] and files(sd2 + '/users/u1/saves') == [])

# Missing source: nothing to do.
out = subprocess.run([exe, conf, 'x', t + '/none', t + '/dest'], capture_output=True, text=True)
check('missing source', out.returncode == 0 and '@done 0' in out.stdout)

# No room: refused before anything changes (a sparse 1 PiB file).
d = t + '/big'
os.makedirs(d)
with open(d + '/huge.bin', 'wb') as f:
    f.truncate(1 << 50)
out = subprocess.run([exe, conf, 'big', d, t + '/dest2'], capture_output=True, text=True)
check('no space: refused, source intact', out.returncode != 0 and '@error space' in out.stdout
      and os.path.exists(d + '/huge.bin') and not os.path.exists(t + '/.dest2.nuubos-move'))

print('[PASS] data-move' if not fails else '[FAIL] %d checks' % fails)
sys.exit(1 if fails else 0)
