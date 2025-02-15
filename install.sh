#!/bin/sh

set -e

# Function to check if we have sudo access
has_sudo() {
    if command -v sudo >/dev/null 2>&1; then
        if sudo -n true 2>/dev/null; then
            return 0
        fi
    fi
    return 1
}

if [ "$(uname)" = "Darwin" ]; then
    if command -v brew &> /dev/null; then
        echo "Homebrew found, installing pepe using Homebrew..."
        brew install omarmhaimdat/pepe/pepe
        echo "pepe installed successfully using Homebrew!"
    else
        echo "Homebrew not found, downloading pepe for macOS..."
        curl -L -o pepe "https://pepe.mhaimdat.com/0.2.7/$(uname -m)-apple-darwin/pepe"
        chmod +x pepe
        mv pepe /usr/local/bin/
        echo "pepe for macOS downloaded and installed successfully!"
    fi
elif [ "$(uname -s)" = "Linux" ]; then
    # Try multiple locations for os-release file
    if [ -f /etc/os-release ]; then
        OS_RELEASE="/etc/os-release"
    elif [ -f /usr/lib/os-release ]; then
        OS_RELEASE="/usr/lib/os-release"
    else
        echo "Could not find os-release file in standard locations."
        exit 1
    fi

    # Source the os-release file
    . "$OS_RELEASE"
    
    # Function to check if the distribution matches
    is_distro() {
        local check_value=$1
        [ "$ID" = "$check_value" ] || echo "$ID_LIKE" | grep -q "$check_value" 2>/dev/null
    }
    
    # Print detected distribution info for debugging
    echo "Detected Linux distribution:"
    echo "ID: $ID"
    [ -n "$ID_LIKE" ] && echo "ID_LIKE: $ID_LIKE"
    
    TEMP_DIR=$(mktemp -d)
    cd "$TEMP_DIR"
    
    echo "Downloading binary..."
    curl -L -O "https://pepe.mhaimdat.com/0.2.7/x86_64-unknown-linux-gnu/pepe"
    curl -L -O "https://pepe.mhaimdat.com/0.2.7/x86_64-unknown-linux-gnu/pepe.sha256"
    
    echo "Verifying binary integrity..."
    EXPECTED_CHECKSUM=$(cat pepe.sha256)
    ACTUAL_CHECKSUM=$(sha256sum pepe | awk '{print $1}')
    
    if [ "$EXPECTED_CHECKSUM" != "$ACTUAL_CHECKSUM" ]; then
        echo "Checksum verification failed!"
        echo "Expected: $EXPECTED_CHECKSUM"
        echo "Got: $ACTUAL_CHECKSUM"
        cd -
        rm -rf "$TEMP_DIR"
        exit 1
    fi
    
    echo -e "\033[1;32m✓ Checksum verified successfully.\033[0m"
    chmod +x pepe

    # Create user binary directory if it doesn't exist
    USER_BIN_DIR="$HOME/.local/bin"
    mkdir -p "$USER_BIN_DIR"

    # Add to PATH if not already there
    PROFILE_FILE="$HOME/.profile"
    if [ ! -f "$PROFILE_FILE" ] || ! grep -q "$USER_BIN_DIR" "$PROFILE_FILE"; then
        echo "export PATH=\"$USER_BIN_DIR:$PATH\"" >> "$PROFILE_FILE"
        echo "Added $USER_BIN_DIR to your PATH in $PROFILE_FILE"
        if has_sudo; then
            source $PROFILE_FILE
        else
            echo -e "\033[1;33m⚠️  Please run this command to update your PATH:\033[0m"
            echo -e "\033[1;32msource $PROFILE_FILE\033[0m"
        fi
    fi

    # Install the binary
    mv pepe "$USER_BIN_DIR/"
    echo -e "\033[1;32m✓ pepe has been installed to $USER_BIN_DIR/pepe\033[0m"
    
    cd -
    rm -rf "$TEMP_DIR"

    # Installation complete message with next steps
    printf "\033[1;32m✓ pepe installed successfully in user space!\033[0m\n"
    printf "\033[1;33m➜ To complete installation, run:\033[0m\n"
    printf "\033[1;32m    source ~/.profile\033[0m\n\n"
    printf "\033[1;33m➜ Then verify with:\033[0m\n"
    printf "\033[1;32m    pepe --help\033[0m\n"
    
    # Check if pepe is in PATH
    if ! echo "/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:/Users/omarmhaimdat/.opam/default/bin:/opt/homebrew/opt/curl/bin:/opt/homebrew/opt/pyqt@5/5.15.4_1/bin:/opt/homebrew/opt/qt@5/bin:/opt/homebrew/opt/openjdk/bin:/Users/omarmhaimdat/Documents/cliTest/cliTests:/Users/omarmhaimdat/.opam/default/bin:/opt/homebrew/opt/curl/bin:/opt/homebrew/opt/pyqt@5/5.15.4_1/bin:/opt/homebrew/opt/qt@5/bin:/opt/homebrew/opt/openjdk/bin:/Users/omarmhaimdat/Documents/cliTest/cliTests:/Users/omarmhaimdat/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin:/Users/omarmhaimdat/.fig/bin:/Users/omarmhaimdat/.local/bin:/Library/TeX/texbin:/usr/local/bin/musl-gcc:/Library/TeX/texbin:/usr/local/bin/musl-gcc:/Applications/Visual Studio Code.app/Contents/Resources/app/bin:/usr/local/bin:/Users/omarmhaimdat/go/bin" | grep -q ""; then
        printf "\033[1;31m! WARNING:  is not in your PATH\033[0m\n"
        printf "\033[1;33m➜ Add this line to your shell config (~/.zshrc, ~/.bashrc):\033[0m\n"
        printf "\033[1;32m    export PATH=\":$PATH\"\033[0m\n"
    fi
else
    echo -e "\033[1;31mUnsupported platform: $(uname)\033[0m"
    exit 1
fi
