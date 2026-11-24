#!/bin/bash
R=$(cd "$(dirname "$0")/../.." && pwd)   # the repo (build first: cargo build --release --bin net_bench, and the relay in rust/relay)
# The edge scenario: 4 phones in one room + 5 AI cars driven by the host.
#   P0 Android on 4G (host)   P1 Android on school wifi   P2 iPhone (web) on 4G   P3 iPhone (web) on school wifi
# Each phone's network is emulated both ways (what it receives and what it sends), on every path (relay lanes, UDP/WebRTC).
# usage: G4="ms,jit,loss" WIFI="ms,jit,loss" LINKS=direct|relay NAME=... /tmp/worst.sh
SECS=${SECS:-60}; G4=${G4:-65,45,0.03}; WIFI=${WIFI:-25,35,0.04}; LINKS=${LINKS:-direct}
pkill -x fogwood_relay; pkill -x net_bench; sleep 0.3; rm -f /tmp/bench_code /tmp/bench_*.log
RELAY_UDP=127.0.0.1:7777 RELAY_WS=127.0.0.1:8080 $R/rust/relay/target/release/fogwood_relay > /tmp/relay.log 2>&1 &
sleep 0.3
export FW_BROKERS="" FW_LOOPBACK=1 FW_NOSTUN=1 FW_RELAY=ws://127.0.0.1:8080/ws FW_WSDEBUG=1
[ "$LINKS" = relay ] && export FW_NODIRECT=1 FW_NORTC=1
net() { echo "FW_UDPLAG=$1 FW_UDPUPLAG=$1 FW_LANELAG=$1 FW_UPLAG=$1"; }
ENV0="$(net $G4) BOTS=5"; ENV1="$(net $WIFI) BOTS=5"; ENV2="$(net $G4) FW_FAKEWEB=1 BOTS=5"; ENV3="$(net $WIFI) FW_FAKEWEB=1 BOTS=5"
for k in 0 1 2 3; do eval "e=\$ENV$k"; env $e $R/rust/target/release/net_bench $k $SECS /tmp/bench_code > /tmp/bench_$k.log 2>&1 & sleep 0.1; done
wait $(jobs -p | tail -n 4) 2>/dev/null
pkill -x fogwood_relay
who=("Android 4G (host)" "Android wifi" "iPhone 4G" "iPhone wifi")
printf "\n=== %s | 4G %s  wifi %s  links %s\n" "${NAME:-run}" "$G4" "$WIFI" "$LINKS"
cat /tmp/bench_*.log | grep "shake" | awk '{s50+=$3; s99+=$5; p+=$7; p99+=$9; d+=$11; n++; if ($9>mp) mp=$9; if ($5>ms) ms=$5} END {printf "all %d views | shake p50 %.1f p99 %.1f (worst car %.0f) | path rms %.2f m p99 %.2f m (worst car %.2f m) | delay %.0f ms\n", n, s50/n, s99/n, ms, p/n, p99/n, mp, d/n}'
for k in 0 1 2 3; do grep "shake" /tmp/bench_$k.log | awk -v w="${who[$k]}" '{p99+=$9; s99+=$5; d+=$11; n++; if ($9>mp) {mp=$9; mc=$1}} END {printf "  %-18s sees: shake p99 %5.1f  path p99 %.2f m (worst %s %.2f m)  delay %3.0f ms", w, s99/n, p99/n, mc, mp, d/n}'
  grep "^P$k up" /tmp/bench_$k.log | awk '{printf "  | %4.1f MB/h  late %s", ($3+$6)*3600/1e6, $17} END {}'; echo -n "  | links:"; grep -o "P[0-9]:[A-Za-z]*" /tmp/bench_$k.log | tail -3 | tr '\n' ' '; echo; done
grep -h "^  A[0-9]" /tmp/bench_[1-3].log | awk '{p99+=$9; s99+=$5; n++} END {if (n) printf "  AI cars (seen by the 3 guests): shake p99 %.1f  path p99 %.2f m\n", s99/n, p99/n}'
