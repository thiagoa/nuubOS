#!/bin/sh
# framebench.sh SECONDS [GAME:MODE ...]: presented frame times of the heavy
# games (ftrun.sh + frametime.so) and, in the middle of each run, a profile
# of the two busiest threads (capture.sh + pcsample) into /tmp/fb/<game>/.
# Bench states of the other profile are copied in with Syncthing frozen and
# removed afterwards (as heavybench.sh); every session ends with kill -9.
# Copy ftrun.sh, capture.sh, frametime.so, frametime-report.sh, pcsample and
# opts.sh to /tmp with it. Never while the user plays.
T=$1; shift
O=/userdata/users/90e7f192-ffd8-4ca5-8e5d-1d5a8fea1711/states; A=/userdata/users/$(cat /run/nuubos/user/active)/states
GAMES="$*"
[ -n "$GAMES" ] || GAMES="6017d7ab30333f54:slot:8 583fd36fe06c8cd6:slot:8 27415d60aff88447:slot:8 994b11440a59be40:slot:8 b1454f1344673a6e:slot:9"
nuubos-emuctl status | grep -q "^state=idle" || { echo BUSY; exit 1; }
ST=$(pidof syncthing); [ -n "$ST" ] && kill -STOP $ST
mkdir -p $A/saturn; added=""
for f in "dreamcast/Dead or Alive 2 (Europe).state8" "dreamcast/Shenmue II (Europe) (EnFrDeEs).state8" "dreamcast/Test Drive Le Mans (USA) (EnFrEs).state8" "saturn/Virtua Fighter 2.state8"; do
	[ -e "$A/$f" ] || { cp "$O/$f" "$A/$f"; added="$added|$f"; }
done
U=/userdata/users/$(cat /run/nuubos/user/active)/appdata/nuubos-emulation/settings.conf; cp $U /tmp/fb-settings.conf
for g in $GAMES; do
	G=${g%%:*}; D=/tmp/fb/$G; rm -rf $D; mkdir -p $D
	echo "== $g $(nuubos-libraryctl search "q=" 2>/dev/null | grep -m1 "game=$G" | cut -f2,3)"
	( sleep $((T / 2)); sh /tmp/capture.sh 6 > $D/capture.txt 2>&1; cp /tmp/cap.maps $D/maps; for c in /tmp/cap.[0-9]*; do cp $c $D/; done ) &
	sh /tmp/ftrun.sh $T 120 $g 2>&1 | grep -v "^fps:"
	wait
	cp /tmp/frametime.bin $D/
	head -1 $D/capture.txt
	sleep 3
done
cp /tmp/fb-settings.conf $U
echo "$added" | tr '|' '\n' | while read -r f; do [ -n "$f" ] && rm -f "$A/$f"; done
[ -n "$ST" ] && kill -CONT $ST
ls $A/saturn; ls $A/dreamcast | grep -c state8
