# Ramp mode: raise the load step by step.
Output assets/ramp.gif
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
Type "pepe ramp http://127.0.0.1:8099/json --from 10 --to 60 --step 10 --every 3s"
Sleep 800ms
Enter
Sleep 20s
Type "q"
Sleep 2.5s
