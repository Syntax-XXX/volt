#!/bin/sh
# Benchmarks: Volt (tcc-Debug und gcc -O2) gegen C (gcc -O2), Go und Python. Aufruf: bench/run.sh
# Jedes Programm läuft 3-mal, es zählt die beste Zeit; die Ausgaben müssen übereinstimmen.
cd "$(dirname "$0")"; export PATH="$HOME/.cargo/bin:$PATH"; mkdir -p build
best() { b=999999; for _ in 1 2 3; do s=$(date +%s%N); "$@" >build/out.txt 2>&1; t=$(( ($(date +%s%N)-s)/1000000 )); [ $t -lt $b ] && b=$t; done; echo $b; }
printf "%-9s %9s %9s %9s %9s %9s %9s\n" Programm "Volt-O2" "Volt-tcc" "C-O2" "Go" "Python" "Ausgabe"
for p in fib sieve nbody strings; do
  VOLT_CC=gcc volt build $p.vt --release -o build/$p-volt >/dev/null || exit 1
  volt build $p.vt -o build/$p-vtcc >/dev/null || exit 1
  gcc -O2 -fwrapv c/$p.c -o build/$p-c -lm; tv=$(best build/$p-volt); ref=$(cat build/out.txt)
  tt=$(best build/$p-vtcc); tc=$(best build/$p-c); ok=ja
  [ "$(cat build/out.txt)" = "$ref" ] || ok=NEIN
  if [ -f go/$p.go ]; then go build -o build/$p-go go/$p.go && tg=$(best build/$p-go); else tg=-; fi
  tp=$(best python3 py/$p.py); [ "$(cat build/out.txt)" = "$ref" ] || ok="Python:NEIN"
  printf "%-9s %7sms %7sms %7sms %7sms %7sms %9s\n" $p $tv $tt $tc $tg $tp $ok
done
