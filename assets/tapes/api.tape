# API mode: the endpoints of an OpenAPI spec, picked on screen.
Output assets/api.gif
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
Type "pepe api assets/demo/openapi.yaml -c 20 -z 8s"
Sleep 800ms
Enter
Sleep 2.5s
Space
Sleep 800ms
Down
Sleep 400ms
Down
Sleep 400ms
Enter
Sleep 2.5s
Escape
Sleep 800ms
Type "g"
Sleep 9s
Type "q"
Sleep 2.5s
