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
    
   TEMP_DIR=/var/folders/dd/6w95nrsn6jn2qd5w439kzdzr0000gn/T/tmp.Bjv8tI4Qmq
    cd ""

    echo "Downloading binary..."
    curl -L -O "https://pepe.mhaimdat.com/0.2.7/x86_64-unknown-linux-gnu/pepe"
    curl -L -O "https://pepe.mhaimdat.com/0.2.7/x86_64-unknown-linux-gnu/pepe.sha256"

    echo "Verifying binary integrity..."
    EXPECTED_CHECKSUM=
    ACTUAL_CHECKSUM=

    if [ "" != "" ]; then
        echo "Checksum verification failed!"
        echo "Expected: "
        echo "Got: "
        cd -
        rm -rf ""
        exit 1
    fi

    echo -e "\033[1;32m✓ Checksum verified successfully.\033[0m"
    chmod +x pepe

    # Function to add PATH if missing
    add_to_path_if_missing() {
        local file=
        if [ -f "" ] && ! grep -q "" ""; then
            echo "export PATH=\":$PATH\"" >> ""
            echo "Added  to "
        fi
    }

    # Check if sudo is available
    if command -v sudo >/dev/null 2>&1 && sudo -n true 2>/dev/null; then
        echo "Installing pepe to /usr/local/bin (requires sudo)..."
        sudo mv pepe /usr/local/bin/
        echo -e "\033[1;32m✓ pepe has been installed to /usr/local/bin/pepe\033[0m"
    else
        echo "Installing pepe to user space in /Users/omarmhaimdat/.local/bin/"
        USER_BIN_DIR="/Users/omarmhaimdat/.local/bin"
        mkdir -p ""
        mv pepe "/"

        # Update PATH in all relevant shell config files
        add_to_path_if_missing "/Users/omarmhaimdat/.profile"
        add_to_path_if_missing "/Users/omarmhaimdat/.bashrc"
        add_to_path_if_missing "/Users/omarmhaimdat/.bash_profile"
        add_to_path_if_missing "/Users/omarmhaimdat/.zshrc"

        echo -e "\033[1;32m✓ pepe installed successfully in user space!\033[0m"
        echo -e "\033[1;33m➜ Restart your shell or run:\033[0m"
        echo -e "\033[1;32m    source ~/.profile\033[0m"
        echo -e "\033[1;32m    source ~/.bashrc  # If using Bash\033[0m"
        echo -e "\033[1;32m    source ~/.zshrc    # If using Zsh\033[0m"
    fi

    cd -
    rm -rf ""
else
    echo -e "\033[1;31mUnsupported platform: $(uname)\033[0m"
    exit 1
fi
