# Ping: two targets on the graph, then the phases of one, then the pings
# themselves with one opened. Regenerate with assets/record.sh.
Output assets/ping.gif
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
Type "export PATH=$PWD/target/release:$PATH PS1='$ ' && clear"
Enter
Show
Type "pepe ping https://example.com http://127.0.0.1:8099/json --name example.com,local --every 300ms"
Sleep 800ms
Enter
Sleep 9s
Type "2"
Sleep 3.5s
Type "3"
Sleep 2s
Enter
Sleep 3s
Escape
Sleep 800ms
Type "1"
Sleep 2.5s
Type "q"
Sleep 2.5s
