# The setup screen: every option as a field, the command as you go.
Output assets/setup.gif
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
Type "pepe"
Sleep 500ms
Enter
Sleep 1.5s
Type "http://127.0.0.1:8099/json"
Sleep 800ms
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 500ms
Backspace 2
Type "50"
Sleep 600ms
Down
Sleep 400ms
Right
Sleep 800ms
Down
Sleep 400ms
Backspace 3
Type "10s"
Sleep 800ms
Ctrl+T
Sleep 3s
Enter
Sleep 7s
Type "q"
Sleep 2s
