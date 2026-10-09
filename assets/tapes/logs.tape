# Reading nginx logs: the dashboard, then traffic, paths, errors and the log.
# The logs are made by assets/record.sh (bench/gen-logs.py).
Output assets/logs.gif
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
Type "pepe logs /tmp/nginx/access.log /tmp/nginx/error.log"
Sleep 800ms
Enter
Sleep 5s
Type "2"
Sleep 2.5s
Type "h"
Sleep 2s
Type "3"
Sleep 2.5s
Type "4"
Sleep 2.5s
Type "5"
Sleep 2.5s
Type "q"
Sleep 2.5s
