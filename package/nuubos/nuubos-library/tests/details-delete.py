#!/usr/bin/env python3
# Host test: libraryd DETAILS / STATS_RESET / DELETE.
# Run: python3 package/nuubos/nuubos-library/tests/details-delete.py
import os, socket, subprocess, sys, tempfile, time

here = os.path.dirname(os.path.abspath(__file__))
src = os.path.join(here, '..', 'src')
t = tempfile.mkdtemp()
run, ud, share = t + '/run', t + '/userdata', t + '/share'
user = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'
for d in (run + '/user', ud + '/roms/psx/ff7', ud + '/roms/psx/.FF9', ud + '/roms/snes', share,
          ud + '/library/metadata/snes', ud + '/users/' + user + '/saves/snes'):
    os.makedirs(d, exist_ok=True)
open(share + '/systems.conf', 'w').write(open(src + '/systems.conf').read())
open(run + '/user/active', 'w').write(user + '\n')
def w(p, data): open(ud + '/roms/' + p, 'w').write(data)
w('snes/Mario (USA).sfc', 'x' * 100)
w('snes/Zelda.sfc', 'y' * 200)
w('psx/ff7/FF7 (Disc 1).bin', 'a' * 50)
w('psx/ff7/FF7 (Disc 2).bin', 'b' * 60)
w('psx/ff7/FF7 (Disc 1).cue', 'FILE "FF7 (Disc 1).bin" BINARY\n')
w('psx/ff7/FF7 (Disc 2).cue', 'FILE "FF7 (Disc 2).bin" BINARY\n')
w('psx/ff7/FF7.m3u', 'FF7 (Disc 1).cue\nFF7 (Disc 2).cue\n')
# One one-line playlist per disc (disc set): one game, the "(Disc 1)" one.
for d in (1, 2, 3):
    w('psx/.FF9/FF9 (Disc %d).bin' % d, 'c' * (70 + d))
    w('psx/.FF9/FF9 (Disc %d).cue' % d, 'FILE "FF9 (Disc %d).bin" BINARY\n' % d)
    w('psx/FF9 (Disc %d).m3u' % d, '.FF9/FF9 (Disc %d).cue\r\n' % d)
# Only the second disc at first: a game of its own until the first comes.
w('psx/DQ (Disc 2).cue', 'FILE "DQ (Disc 2).bin" BINARY\n')
w('psx/DQ (Disc 2).bin', 'd' * 90)
w('psx/DQ (Disc 2).m3u', 'DQ (Disc 2).cue\n')
open(ud + '/library/metadata/snes/Mario (USA).txt', 'w').write(
    'title=Super Mario World\ndeveloper=Nintendo\nbad key=x\nplayers=1-2\n')
open(ud + '/users/' + user + '/saves/snes/Mario (USA).srm', 'w').write('save')

exe = t + '/libraryd'
subprocess.check_call(['gcc', '-std=c11', '-D_GNU_SOURCE', '-Wall', '-Wextra', '-Werror',
    '-DRUN_ROOT="%s"' % run, '-DUSERDATA_ROOT="%s"' % ud, '-DSHARE_ROOT="%s"' % share,
    '-DSTATE_ROOT="%s"' % (t + '/state'), src + '/libraryd.c', '-o', exe, '-lpthread'])
proc = subprocess.Popen([exe], stderr=open(t + '/log', 'w'))
fails = 0
def check(name, cond):
    global fails
    if not cond:
        print('[FAIL]', name); fails += 1

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
        txt = data.decode()
        if txt.endswith('end=1\n') or (not txt.startswith(('game=', 'user=')) and txt.endswith('\n')):
            break
    s.close()
    return data.decode()

try:
    for _ in range(50):
        st = cmd('STATUS')
        if 'scanning=0' in st and 'games=' in st and 'games=0' not in st: break
        time.sleep(0.1)
    games = cmd('GAMES\tsystem:snes')
    ids = {l.split('\t')[2]: l.split('=')[1].split('\t')[0] for l in games.splitlines() if l.startswith('game=')}
    mario, zelda = ids.get('Mario'), ids.get('Zelda')
    check('two snes games', mario and zelda)
    psx = cmd('GAMES\tsystem:psx')
    ff7 = [l.split('=')[1].split('\t')[0] for l in psx.splitlines() if l.startswith('game=')]
    dq2 = [l.split('=')[1].split('\t')[0] for l in psx.splitlines() if l.startswith('game=') and l.split('\t')[2] == 'DQ']
    check('lone second disc is a game', len(dq2) == 1)
    ff7 = [g for g in ff7 if g not in dq2]
    check('one psx game per m3u game', len(ff7) == 2)
    ff9 = [l.split('=')[1].split('\t')[0] for l in psx.splitlines() if l.startswith('game=') and l.split('\t')[2] == 'FF9']
    check('disc set is one game', len(ff9) == 1)
    ff7 = [g for g in ff7 if g not in ff9]
    r = cmd('RESOLVE\t' + ff9[0]).rstrip('\n').split('\t')
    check('disc set resolves to a playlist of all discs', len(r) == 3 and r[0] == 'OK psx'
          and r[1] == run + '/library-discs/FF9 (Disc 1).m3u'
          and r[2] == ud + '/roms/psx/FF9 (Disc 1).m3u'
          and open(r[1]).read() == ''.join('%s/roms/psx/.FF9/FF9 (Disc %d).cue\n' % (ud, d) for d in (1, 2, 3)))
    r = cmd('RESOLVE\t' + ff7[0]).rstrip('\n').split('\t')
    check('other m3u resolves to itself', r == ['OK psx', ud + '/roms/psx/ff7/FF7.m3u'])

    cmd('SESSION_RECORD\t%s\t120' % mario)
    d = cmd('DETAILS\t' + mario)
    check('details sessions', 'sessions=1' in d)
    check('details metadata', 'meta_developer=Nintendo' in d and 'meta_players=1-2' in d)
    check('details rejects bad keys', 'bad key' not in d)
    check('details path', 'path=snes/Mario (USA).sfc' in d)


    check('stats reset', cmd('STATS_RESET\t' + mario).startswith('OK'))
    check('stats gone', 'sessions=0' in cmd('DETAILS\t' + mario))

    check('delete needs confirm', cmd('DELETE\t' + mario).startswith('ERR'))
    check('delete ok', cmd('DELETE\t%s\tCONFIRM' % mario).startswith('OK'))
    check('rom removed', not os.path.exists(ud + '/roms/snes/Mario (USA).sfc'))
    check('metadata removed', not os.path.exists(ud + '/library/metadata/snes/Mario (USA).txt'))
    check('save kept', os.path.exists(ud + '/users/' + user + '/saves/snes/Mario (USA).srm'))
    check('other rom kept', os.path.exists(ud + '/roms/snes/Zelda.sfc'))
    check('gone from catalog', 'ERR' in cmd('DETAILS\t' + mario))

    # An unavailable game: only its entry, cover and metadata go.
    open(ud + '/library/metadata/snes/Zelda.txt', 'w').write('title=Zelda\n')
    open(ud + '/library/metadata/snes/Zelda.tried', 'w').write('')
    os.rename(ud + '/roms/snes/Zelda.sfc', t + '/Zelda.sfc')
    w('psx/DQ (Disc 1).cue', 'FILE "DQ (Disc 1).bin" BINARY\n')
    w('psx/DQ (Disc 1).bin', 'e' * 80)
    w('psx/DQ (Disc 1).m3u', 'DQ (Disc 1).cue\n')
    cmd('SCAN')
    for _ in range(50):
        if 'scanning=0' in cmd('STATUS') and cmd('DETAILS\t' + zelda).split('\n')[0].split('\t')[8] == '0':
            break
        time.sleep(0.1)
    psx = cmd('GAMES\tsystem:psx')
    check('second disc joins the set', dq2[0] not in psx and 'ERR' in cmd('DETAILS\t' + dq2[0])
          and sum(1 for l in psx.splitlines() if l.startswith('game=') and l.split('\t')[2] == 'DQ') == 1)
    check('zelda is unavailable', cmd('DETAILS\t' + zelda).split('\n')[0].split('\t')[8] == '0')
    st = cmd('STATUS')
    check('unavailable hidden by default', 'show_unavailable=0' in st and zelda not in cmd('GAMES\tsystem:snes'))
    check('show unavailable', cmd('SHOW_UNAVAILABLE\t1').startswith('OK') and zelda in cmd('GAMES\tsystem:snes')
          and 'show_unavailable=1' in cmd('STATUS'))
    check('setting saved', 'SHOW_UNAVAILABLE=1' in open(t + '/state/config/nuubos-library.conf').read())
    check('bad value refused', cmd('SHOW_UNAVAILABLE\t2').startswith('ERR'))
    # Home rows going round (GitHub #28): on by default, saved with the rest.
    check('wrap rows on by default', 'wrap_rows=1' in st)
    check('wrap rows off', cmd('WRAP_ROWS\t0').startswith('OK') and 'wrap_rows=0' in cmd('STATUS')
          and 'SHOW_UNAVAILABLE=1\nWRAP_ROWS=0\n' == open(t + '/state/config/nuubos-library.conf').read())
    check('wrap rows bad value refused', cmd('WRAP_ROWS\tx').startswith('ERR'))
    check('unavailable game deleted', cmd('DELETE\t%s\tCONFIRM' % zelda).startswith('OK'))
    check('unavailable: metadata and tried mark removed', not os.path.exists(ud + '/library/metadata/snes/Zelda.txt')
          and not os.path.exists(ud + '/library/metadata/snes/Zelda.tried'))
    check('unavailable: gone from catalog', 'ERR' in cmd('DETAILS\t' + zelda))

    check('delete multi-disc', cmd('DELETE\t%s\tCONFIRM' % ff7[0]).startswith('OK'))
    left = os.listdir(ud + '/roms/psx/ff7')
    check('m3u deleted', 'FF7.m3u' not in left)
    check('cues deleted', not any(f.endswith('.cue') for f in left))
    check('tracks deleted', left == [])
    check('delete disc set', cmd('DELETE\t%s\tCONFIRM' % ff9[0]).startswith('OK'))
    check('disc set files deleted', os.listdir(ud + '/roms/psx/.FF9') == []
          and not any(f.startswith('FF9') for f in os.listdir(ud + '/roms/psx')))
finally:
    proc.terminate(); proc.wait()
print('[PASS] libraryd details/delete' if not fails else '[FAIL] %d checks' % fails)
sys.exit(1 if fails else 0)
