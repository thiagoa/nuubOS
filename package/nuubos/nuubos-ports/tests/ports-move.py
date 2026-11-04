#!/usr/bin/env python3
# Host test: nuubos-ports-move (Ports on SD2, GitHub #21). Checks: ports
# and their folders move, a port already at the destination stays there
# (the source's copy too, counted as a conflict), the newer PortMaster wins,
# .nuubos merges without overwriting, the source keeps only what did not
# move, no staging left, and a destination without room refuses before
# touching anything.
# Run: python3 package/nuubos/nuubos-ports/tests/ports-move.py
import os, subprocess, sys, tempfile

here = os.path.dirname(os.path.abspath(__file__))
src = os.path.join(here, '..', 'src', 'ports-move.c')
t = tempfile.mkdtemp()
exe = t + '/ports-move'
subprocess.check_call(['gcc', '-std=c11', '-D_GNU_SOURCE', '-Wall', '-Wextra', '-Werror',
                       '-Wno-format-truncation', src, '-o', exe])
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

a, b = t + '/sd1/ports', t + '/sd2/ports'
w(a + '/Alpha.sh', 'alpha'); w(a + '/alpha/data.bin', 'x' * 5000)
w(a + '/Both.sh', 'both-sd1'); w(a + '/both/v.txt', 'sd1')
w(a + '/PortMaster/version', '2026.10.24-1611\n'); w(a + '/PortMaster/PortMaster.sh', 'new')
w(a + '/.nuubos/home/alpha/save.dat', 'alpha-save'); w(a + '/.nuubos/home/both/save.dat', 'sd1-save')
w(b + '/Both.sh', 'both-sd2'); w(b + '/both/v.txt', 'sd2')
w(b + '/PortMaster/version', '2026.06.23-0015\n'); w(b + '/PortMaster/PortMaster.sh', 'old')
w(b + '/.nuubos/home/both/save.dat', 'sd2-save')

out = subprocess.run([exe, a, b], capture_output=True, text=True)
check('done, one conflict', out.returncode == 0 and '@done 1' in out.stdout)
check('progress reported', '@progress 100' in out.stdout)
check('port and folder moved', r(b + '/Alpha.sh') == 'alpha' and r(b + '/alpha/data.bin') == 'x' * 5000
      and not os.path.exists(a + '/Alpha.sh') and not os.path.exists(a + '/alpha'))
check('destination copy kept', r(b + '/Both.sh') == 'both-sd2' and r(b + '/both/v.txt') == 'sd2')
check('source copy of a conflict stays', r(a + '/Both.sh') == 'both-sd1' and r(a + '/both/v.txt') == 'sd1')
check('newer PortMaster wins', r(b + '/PortMaster/PortMaster.sh') == 'new' and not os.path.exists(a + '/PortMaster'))
check('.nuubos merged, nothing overwritten', r(b + '/.nuubos/home/alpha/save.dat') == 'alpha-save'
      and r(b + '/.nuubos/home/both/save.dat') == 'sd2-save' and r(a + '/.nuubos/home/both/save.dat') == 'sd1-save'
      and not os.path.exists(a + '/.nuubos/home/alpha'))
check('no staging left', not os.path.exists(b + '/.nuubos-move'))

# Back: into an empty folder everything moves.
c = t + '/sd1b/ports'
out = subprocess.run([exe, b, c], capture_output=True, text=True)
check('back to an empty folder', out.returncode == 0 and '@done 0' in out.stdout and r(c + '/Alpha.sh') == 'alpha'
      and r(c + '/PortMaster/PortMaster.sh') == 'new' and os.listdir(b) == [])

# No room: refused before anything changes (an impossible margin: a huge file).
d = t + '/big/ports'
w(d + '/Huge.sh', 'huge')
with open(d + '/huge.bin', 'wb') as f:
    f.truncate(1 << 50)  # sparse, 1 PiB apparent size
out = subprocess.run([exe, d, t + '/dest'], capture_output=True, text=True)
check('no space: refused, source intact', out.returncode != 0 and '@error space' in out.stdout
      and os.path.exists(d + '/huge.bin') and not os.path.exists(t + '/dest/.nuubos-move'))

print('[PASS] ports-move' if not fails else '[FAIL] %d checks' % fails)
sys.exit(1 if fails else 0)
