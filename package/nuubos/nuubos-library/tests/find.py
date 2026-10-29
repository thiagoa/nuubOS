#!/usr/bin/env python3
# Host test: libraryd SEARCH / FILTERS / previews / tags / MIGRATE_FAVORITES.
# Run: python3 package/nuubos/nuubos-library/tests/find.py
import os, socket, subprocess, sys, tempfile, time

here = os.path.dirname(os.path.abspath(__file__))
src = os.path.join(here, '..', 'src')
t = tempfile.mkdtemp()
run, ud, share = t + '/run', t + '/userdata', t + '/share'
user = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'
for d in (run + '/user', ud + '/roms/snes', ud + '/roms/gba', share, ud + '/library/metadata/snes',
          ud + '/library/covers/snes', ud + '/users/' + user + '/library'):
    os.makedirs(d, exist_ok=True)
open(share + '/systems.conf', 'w').write(open(src + '/systems.conf').read())
open(run + '/user/active', 'w').write(user + '\n')
for name, size in (('Super Mario World', 100), ('Zelda', 200), ('Mario Kart', 300)):
    open(ud + '/roms/snes/%s.sfc' % name, 'w').write('x' * size)
open(ud + '/roms/gba/Metroid Fusion.gba', 'w').write('m' * 50)
os.makedirs(ud + '/roms/snes/Homebrew', exist_ok=True)
open(ud + '/roms/snes/Homebrew/.noscrape', 'w').close()
open(ud + '/roms/snes/Homebrew/Homebrew Demo.sfc', 'w').write('h' * 10)
open(ud + '/library/covers/snes/Zelda.png', 'w').write('png')
open(ud + '/library/covers/snes/Mario Kart.png', 'w').write('png')
meta = ud + '/library/metadata/snes/'
open(meta + 'Super Mario World.txt', 'w').write('release=1990-11-21\ngenre=Platform, Action\nplayers=1-2\n')
open(meta + 'Mario Kart.txt', 'w').write('release=19920827T000000\ngenre=Racing\nplayers=1-4\n')
open(meta + 'Zelda.txt', 'w').write('release=1991\ngenre=Action / Adventure\nplayers=1\n'
    'description=An adventure\nlang=en\ndescription@en=An adventure\ndescription@it=Un\'avventura\ngenre@it=Azione\n')
os.makedirs(t + '/state/users/' + user, exist_ok=True)
open(t + '/state/users/' + user + '/localization.conf', 'w').write('LANGUAGE=it\n')

exe = t + '/libraryd'
subprocess.check_call(['gcc', '-std=c11', '-D_GNU_SOURCE', '-Wall', '-Wextra', '-Werror',
    '-DRUN_ROOT="%s"' % run, '-DUSERDATA_ROOT="%s"' % ud, '-DSHARE_ROOT="%s"' % share, '-DSTATE_ROOT="%s"' % (t + '/state'),
    src + '/libraryd.c', '-o', exe, '-lpthread'])

def start():
    return subprocess.Popen([exe], stderr=open(t + '/log', 'a'))

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
        if txt.endswith('end=1\n') or (txt.startswith(('OK', 'ERR')) and txt.endswith('\n')):
            break
    s.close()
    return data.decode()

def titles(reply):
    return [l.split('\t')[2] for l in reply.splitlines() if l.startswith('game=')]

fails = 0
def check(name, cond):
    global fails
    if not cond:
        print('[FAIL]', name); fails += 1

proc = start()
for _ in range(50):
    if 'scanning=0' in cmd('STATUS') and 'games=4' in cmd('STATUS'):
        break
    time.sleep(0.1)

check('title words', titles(cmd('SEARCH\tq=mario')) == ['Mario Kart', 'Super Mario World'])
check('two words', titles(cmd('SEARCH\tq=super mario')) == ['Super Mario World'])
check('system', titles(cmd('SEARCH\tsystem=gba')) == ['Metroid Fusion'])
check('noscrape folder still listed', 'Homebrew Demo' in titles(cmd('GAMES\tsystem:snes')))
homebrew = [l for l in cmd('GAMES\tsystem:snes').splitlines() if l.startswith('game=') and '\tHomebrew Demo\t' in l][0]
check('noscrape flag set', homebrew.split('\t')[13] == '1')
other = [l for l in cmd('GAMES\tsystem:snes').splitlines() if l.startswith('game=') and '\tZelda\t' in l][0]
check('noscrape flag unset elsewhere', other.split('\t')[13] == '0')
check('year range', titles(cmd('SEARCH\tyear=1991-1999')) == ['Mario Kart', 'Zelda'])
check('genre', titles(cmd('SEARCH\tgenre=action')) == ['Super Mario World', 'Zelda'])
check('players', titles(cmd('SEARCH\tplayers=3')) == ['Mario Kart'])
check('combined', titles(cmd('SEARCH\tq=mario|players=2|year=1990-1990')) == ['Super Mario World'])
check('random one', len(titles(cmd('SEARCH\trandom=1'))) == 1)
cmd('SESSION_RECORD\t' + cmd('SEARCH\tq=zelda').split('\t')[0].split('=')[1] + '\t60')
check('unplayed', 'Zelda' not in titles(cmd('SEARCH\tunplayed=1')) and len(titles(cmd('SEARCH\tunplayed=1'))) == 4)
check('random unplayed', titles(cmd('SEARCH\tunplayed=1|random=1|q=zelda')) == [])
zid = cmd('SEARCH\tq=zelda').split('\t')[0].split('=')[1]
d = cmd('DETAILS\t' + zid)
check('user language text', "meta_description=Un'avventura" in d and 'meta_genre=Azione' in d and 'An adventure' not in d)
f = cmd('FILTERS')
check('decades', 'decade=1990\t3' in f)
check('genres first word', 'genre=Action\t1' in f and 'genre=Platform\t1' in f and 'genre=Racing\t1' in f)
check('players max', 'players=4' in f)
st = cmd('STATUS')
snes = [l for l in st.splitlines() if l.startswith('system=snes')][0].split('\t')
check('system previews: played first', snes[6:] == [ud + '/library/covers/snes/Zelda.png', ud + '/library/covers/snes/Mario Kart.png'])
check('no favorites collection', 'collection=favorites' not in st)

# Tags: per user, one status at a time, searchable, in the game line.
check('tag unknown', cmd('TAG_SET\t' + zid + '\tshiny').startswith('ERR'))
check('tag set', cmd('TAG_SET\t' + zid + '\tplaying') == 'OK\n')
check('tag in line', [l for l in cmd('SEARCH\tq=zelda').splitlines() if l.startswith('game=')][0].split('\t')[12] == 'playing')
cmd('TAG_SET\t' + zid + '\tbroken')
cmd('TAG_SET\t' + zid + '\tcompleted')
zline = [l for l in cmd('SEARCH\tq=zelda').splitlines() if l.startswith('game=')][0]
check('status replaces status', zline.split('\t')[12] == 'completed,broken')
check('tag search', titles(cmd('SEARCH\ttag=completed')) == ['Zelda'] and titles(cmd('SEARCH\ttag=playing')) == [])
check('tag unknown search', titles(cmd('SEARCH\ttag=nope')) == [])
check('tag filters', 'tag=completed\t1' in cmd('FILTERS') and 'tag=playing' not in cmd('FILTERS'))
check('tag file', open(ud + '/users/' + user + '/library/tags.tsv').read().endswith(zid + '\tcompleted,broken\n'))
check('tag clear', cmd('TAG_CLEAR\t' + zid + '\tbroken') == 'OK\n' and titles(cmd('SEARCH\ttag=broken')) == [])
proc.kill(); proc.wait()
proc = start()
time.sleep(0.3)
check('tag reloaded', titles(cmd('SEARCH\ttag=completed')) == ['Zelda'])
cmd('TAG_CLEAR\t' + zid + '\tcompleted')
check('tag file emptied', zid not in open(ud + '/users/' + user + '/library/tags.tsv').read())

# Migration of an older favorites list.
proc.kill(); proc.wait()
proc = start()
time.sleep(0.3)
mario = [l for l in cmd('SEARCH\tq=kart').splitlines() if l.startswith('game=')][0].split('\t')[0][5:]
proc.kill(); proc.wait()
open(ud + '/users/' + user + '/library/favorites.txt', 'w').write(mario + '\n')
proc = start()
time.sleep(0.3)
check('pending shown', 'favorites_pending=1' in cmd('STATUS'))
r = cmd('MIGRATE_FAVORITES\tPreferiti')
check('migrated', r.startswith('OK c'))
st = cmd('STATUS')
coll = [l for l in st.splitlines() if l.startswith('collection=')]
check('collection made', len(coll) == 1 and '\tPreferiti\t1\t' in coll[0] and coll[0].endswith('Mario Kart.png'))
check('pending gone', 'favorites_pending' not in st)
check('file removed', not os.path.exists(ud + '/users/' + user + '/library/favorites.txt'))
cid = r.split()[1]
check('collection filter', titles(cmd('SEARCH\tcollection=' + cid)) == ['Mario Kart'])
check('collection + words', titles(cmd('SEARCH\tcollection=' + cid + '|q=zelda')) == [])
check('unknown collection', titles(cmd('SEARCH\tcollection=nope')) == [])
check('favorite command gone', cmd('FAVORITE\t' + mario + '\t1').startswith('ERR'))
proc.kill(); proc.wait()
if fails:
    print(open(t + '/log').read()[-2000:])
    sys.exit(1)
print('[PASS] library find')
