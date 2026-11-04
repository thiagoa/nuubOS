#!/usr/bin/env python3
# Host test: a file inside a system's folder belongs to that system or to
# none (GitHub #31: the .col stages of a PSP port in roms/psp were listed as
# ColecoVision games); loose files still go by their extension.
# Run: python3 package/nuubos/nuubos-library/tests/scan-folders.py
import os, socket, subprocess, sys, tempfile, time

here = os.path.dirname(os.path.abspath(__file__))
src = os.path.join(here, '..', 'src')
t = tempfile.mkdtemp()
run, ud, share = t + '/run', t + '/userdata', t + '/share'
user = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'
for d in (run + '/user', ud + '/roms/psp/JetSetRadio/stages', ud + '/roms/coleco', ud + '/roms/Misc', share,
          ud + '/users/' + user + '/library'):
    os.makedirs(d, exist_ok=True)
open(share + '/systems.conf', 'w').write(open(src + '/systems.conf').read())
open(run + '/user/active', 'w').write(user + '\n')
for f in ('psp/JetSetRadio/EBOOT.PBP', 'psp/JetSetRadio/stages/stage1.col', 'psp/JetSetRadio/stages/stage2.col',
          'coleco/Zaxxon.col', 'Misc/Frogger.col'):
    open(ud + '/roms/' + f, 'w').write('x' * 64)

exe = t + '/libraryd'
subprocess.check_call(['gcc', '-std=c11', '-D_GNU_SOURCE', '-Wall', '-Wextra', '-Werror',
    '-DRUN_ROOT="%s"' % run, '-DUSERDATA_ROOT="%s"' % ud, '-DSHARE_ROOT="%s"' % share, '-DSTATE_ROOT="%s"' % (t + '/state'),
    src + '/libraryd.c', '-o', exe, '-lpthread'])

def cmd(line):
    s = socket.socket(socket.AF_UNIX)
    for _ in range(50):
        try:
            s.connect(run + '/libraryd.sock'); break
        except OSError:
            time.sleep(0.1)
    s.sendall((line + '\n').encode())
    data = b''
    s.settimeout(2)
    while True:
        try:
            chunk = s.recv(65536)
        except socket.timeout:
            break
        if not chunk: break
        data += chunk
        if data.decode().endswith('end=1\n'):
            break
    s.close()
    return data.decode()

fails = 0
def check(name, cond):
    global fails
    if not cond:
        print('[FAIL]', name); fails += 1

def names(system):
    return sorted(l.split('\t')[1] for l in cmd('GAMES\tsystem:' + system).splitlines() if l.startswith('game='))

def catalog_mentions(word):
    for root, _, files in os.walk(ud):
        for f in files:
            p = os.path.join(root, f)
            if '/roms/' not in p and word in open(p, errors='replace').read():
                return True
    return False

def run_scan(binary):
    # With a catalog the start scan waits for nuubUI (boot-scan.py).
    open(run + '/ui-ready', 'w').write('')
    scans = open(t + '/log').read().count('scan: ') if os.path.exists(t + '/log') else 0
    proc = subprocess.Popen([binary], stderr=open(t + '/log', 'a'))
    try:
        for _ in range(100):
            if open(t + '/log').read().count('scan: ') > scans and 'scanning=0' in cmd('STATUS'):
                break
            time.sleep(0.1)
        return names('coleco'), names('psp')
    finally:
        proc.terminate()
        proc.wait()

# A catalog from before the fix (libraryd of cf05a5c, which still lists
# the stages) loses those entries at the next scan.
old_src = t + '/libraryd-old.c'
old = subprocess.run(['git', 'show', 'cf05a5c:package/nuubos/nuubos-library/src/libraryd.c'], cwd=here,
                     capture_output=True, text=True).stdout
stale = False
if old and 'return system_accepts(folder_sys, name) ? folder_sys : -1;' not in old:
    open(old_src, 'w').write(old)
    subprocess.check_call(['gcc', '-std=c11', '-D_GNU_SOURCE', '-w',
        '-DRUN_ROOT="%s"' % run, '-DUSERDATA_ROOT="%s"' % ud, '-DSHARE_ROOT="%s"' % share,
        '-DSTATE_ROOT="%s"' % (t + '/state'), old_src, '-o', t + '/libraryd-old', '-lpthread'])
    stale = len(run_scan(t + '/libraryd-old')[0]) == 4
seen_old = catalog_mentions('stage1.col')
coleco, psp = run_scan(exe)
check('psp folder keeps only its game', len(psp) == 1)
check('coleco: its folder and the loose file, no PSP stages', len(coleco) == 2 and not any('stage' in n for n in coleco))
if stale:
    check('old catalog had the stages', seen_old)
check('stale stage entries dropped, not kept unavailable', not catalog_mentions('stage1.col'))
if fails:
    print('[FAIL] %d checks' % fails)
    print(open(t + '/log').read())
    sys.exit(1)
print('[PASS] library system folders')
