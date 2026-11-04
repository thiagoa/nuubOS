#!/bin/sh
# heavybench.sh [GAME:MODE ...]: heavy-system baseline (perf-power.md) on the
# dev profile. Bench states of the other profile are copied in while
# Syncthing is frozen and removed afterwards; every run ends with kill -9
# (dcbench.sh), so nothing is quit-saved. Copy dcbench.sh, opts.sh and
# gpubusy.sh to /tmp with it. Never while the user plays.
O=/userdata/users/90e7f192-ffd8-4ca5-8e5d-1d5a8fea1711/states; A=/userdata/users/$(cat /run/nuubos/user/active)/states
GAMES="$*"
[ -n "$GAMES" ] || GAMES="27415d60aff88447:slot:8 994b11440a59be40:slot:8 52c1f96b96d08784:slot:8 583fd36fe06c8cd6:slot:8 6017d7ab30333f54:slot:8 b1454f1344673a6e:slot:9 559fb7c454c44b85:slot:9 c584f2a440826af6:new"
nuubos-emuctl status | grep -q "^state=idle" || { echo BUSY; exit 1; }
ST=$(pidof syncthing); [ -n "$ST" ] && kill -STOP $ST
mkdir -p $A/saturn; added=""
for f in "dreamcast/Dead or Alive 2 (Europe).state8" "dreamcast/Shenmue II (Europe) (EnFrDeEs).state8" "dreamcast/Test Drive Le Mans (USA) (EnFrEs).state8" "saturn/Virtua Fighter 2.state8"; do
	[ -e "$A/$f" ] || { cp "$O/$f" "$A/$f"; added="$added|$f"; }
done
U=/userdata/users/$(cat /run/nuubos/user/active)/appdata/nuubos-emulation/settings.conf; cp $U /tmp/hb-settings.conf
for g in $GAMES; do
	echo "== $g $(nuubos-libraryctl search "q=" 2>/dev/null | grep -m1 "game=${g%%:*}" | cut -f2,3)"
	sh /tmp/dcbench.sh 12 30 $g 2>&1 | awk '/^fps:/{n=0;s=0;m=999;for(i=2;i<=NF;i++){s+=$i;n++; if($i<m)m=$i}; printf "fps avg %.1f min %.1f\n", s/n, m; next} /^mhz:/{c=0;for(i=2;i<=NF;i++) if($i>=1416)c++; printf "cpu at >=1416: %d of %d s\n", c, NF-1; next} {print}'
	sleep 3
done
cp /tmp/hb-settings.conf $U
echo "$added" | tr '|' '\n' | while read -r f; do [ -n "$f" ] && rm -f "$A/$f"; done
[ -n "$ST" ] && kill -CONT $ST
ls $A/saturn; ls $A/dreamcast | grep -c state8
