# The docs' home page: a ping to see where the time goes, then a run with
# the dashboard, ended with the verdict in the shell. Regenerate with
# assets/record.sh.
Output assets/home.gif
Set Shell "bash"
Set FontSize 15
Set Width 1280
Set Height 720
Set Framerate 20
Set TypingSpeed 45ms
Set Padding 16
Set Theme "Catppuccin Mocha"
Env PEPE_NO_UPDATE_CHECK "1"
Hide
Type "export PATH=$PWD/target/release:$PATH PS1='$ ' && clear"
Enter
Show
Type "pepe ping https://example.com --once"
Sleep 600ms
Enter
Sleep 4.5s
Type "pepe -z 8s -c 50 http://127.0.0.1:8099/json"
Sleep 600ms
Enter
Sleep 4s
Type "3"
Sleep 2.5s
Type "1"
Sleep 4s
Type "q"
Sleep 3s
