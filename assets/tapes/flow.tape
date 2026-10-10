# A flow: three steps, each fed by the one before, each a row on the
# dashboard. Regenerate with assets/record.sh.
Output assets/flow.gif
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
Type "cat assets/demo/checkout.toml"
Enter
Sleep 4s
Type "clear && pepe flow assets/demo/checkout.toml -c 20 -z 10s"
Sleep 800ms
Enter
Sleep 5s
Type "2"
Sleep 3s
Type "3"
Sleep 2.5s
Type "1"
Sleep 2s
Type "q"
Sleep 2.5s
