# A plain run with the live dashboard. Regenerate with assets/record.sh.
Output assets/run.gif
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
Type "pepe -z 12s -c 50 http://127.0.0.1:8099/json"
Sleep 800ms
Enter
Sleep 5s
Type "3"
Sleep 2s
Enter
Sleep 1.5s
Type "]"
Sleep 3s
Escape
Sleep 1s
Type "1"
Sleep 4s
Type "q"
Sleep 2.5s
