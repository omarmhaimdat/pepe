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
        curl -L -o pepe "https://pepe.mhaimdat.com/0.2.0/$(uname -m)-apple-darwin/pepe"
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
    curl -L -O "https://pepe.mhaimdat.com/0.2.0/x86_64-unknown-linux-gnu/pepe"
    curl -L -O "https://pepe.mhaimdat.com/0.2.0/x86_64-unknown-linux-gnu/pepe.sha256"
    
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
    
    echo "Checksum verified successfully."
    chmod +x pepe

    # Create user binary directory if it doesn't exist
    USER_BIN_DIR="$HOME/.local/bin"
    mkdir -p "$USER_BIN_DIR"

    # Add to PATH if not already there
    PROFILE_FILE="$HOME/.profile"
    if [ ! -f "$PROFILE_FILE" ] || ! grep -q "$USER_BIN_DIR" "$PROFILE_FILE"; then
        echo "export PATH=\"$USER_BIN_DIR:$PATH\"" >> "$PROFILE_FILE"
        echo "Added $USER_BIN_DIR to your PATH in $PROFILE_FILE"
        echo "Please run 'source $PROFILE_FILE' to update your PATH"
    fi

    # Install the binary
    mv pepe "$USER_BIN_DIR/"
    echo "pepe has been installed to $USER_BIN_DIR/pepe"
    
    cd -
    rm -rf "$TEMP_DIR"
    echo "pepe installed successfully in user space!"
    echo "You may need to restart your terminal or run 'source $PROFILE_FILE' to use pepe"
else
    echo "Unsupported platform: $(uname)"
    exit 1
fi
