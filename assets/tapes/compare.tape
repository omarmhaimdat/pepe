# Compare: two reports under the same load, the second of an endpoint that
# answers slower, then what moved. Regenerate with assets/record.sh.
Output assets/compare.gif
Set Shell "bash"
Set FontSize 15
Set Width 1280
Set Height 720
Set Framerate 20
Set TypingSpeed 50ms
Set Padding 16
Set Theme "Catppuccin Mocha"
Env PEPE_NO_UPDATE_CHECK "1"
Hide
Type "export PATH=$PWD/target/release:$PATH PS1='$ ' && mkdir -p /tmp/pepe-compare && cd /tmp/pepe-compare && clear"
Enter
Show
Type "pepe --json -n 3000 -c 20 'http://127.0.0.1:8099/slow?ms=5' > before.json"
Sleep 600ms
Enter
Sleep 5s
Type "pepe --json -n 3000 -c 20 'http://127.0.0.1:8099/slow?ms=9' > after.json"
Sleep 600ms
Enter
Sleep 5s
Type "pepe compare before.json after.json"
Sleep 800ms
Enter
Sleep 7s
