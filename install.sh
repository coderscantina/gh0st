#!/usr/bin/env bash
set -e

# gh0st Installation Script
# This script downloads and installs the latest release of gh0st

VERSION="${1:-latest}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
REPO="yourusername/gh0st"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

print_error() {
    echo -e "${RED}Error: $1${NC}" >&2
}

print_success() {
    echo -e "${GREEN}$1${NC}"
}

print_info() {
    echo -e "${YELLOW}$1${NC}"
}

# Detect OS and architecture
detect_platform() {
    local os
    local arch

    case "$(uname -s)" in
        Linux*)     os=linux ;;
        Darwin*)    os=macos ;;
        *)          print_error "Unsupported OS: $(uname -s)"; exit 1 ;;
    esac

    case "$(uname -m)" in
        x86_64)     arch=x86_64 ;;
        aarch64)    arch=aarch64 ;;
        arm64)      arch=aarch64 ;;
        *)          print_error "Unsupported architecture: $(uname -m)"; exit 1 ;;
    esac

    echo "${os}-${arch}"
}

# Get the latest release version
get_latest_version() {
    curl -s "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"v([^"]+)".*/\1/'
}

# Download and install
install_gh0st() {
    local platform
    local version
    local download_url
    local archive_name
    local binary_name="gh0st"

    platform=$(detect_platform)

    if [ "$VERSION" = "latest" ]; then
        print_info "Fetching latest version..."
        version=$(get_latest_version)
        if [ -z "$version" ]; then
            print_error "Could not determine latest version"
            exit 1
        fi
    else
        version="$VERSION"
    fi

    print_info "Installing gh0st v${version} for ${platform}..."

    archive_name="gh0st-${platform}.tar.gz"
    download_url="https://github.com/${REPO}/releases/download/v${version}/${archive_name}"

    # Create temporary directory
    tmp_dir=$(mktemp -d)
    trap "rm -rf $tmp_dir" EXIT

    # Download
    print_info "Downloading from ${download_url}..."
    if ! curl -fsSL "$download_url" -o "$tmp_dir/$archive_name"; then
        print_error "Failed to download gh0st"
        exit 1
    fi

    # Extract
    print_info "Extracting archive..."
    tar xzf "$tmp_dir/$archive_name" -C "$tmp_dir"

    # Create install directory if it doesn't exist
    mkdir -p "$INSTALL_DIR"

    # Install binary
    print_info "Installing to $INSTALL_DIR..."
    mv "$tmp_dir/$binary_name" "$INSTALL_DIR/$binary_name"
    chmod +x "$INSTALL_DIR/$binary_name"

    print_success "✓ gh0st v${version} installed successfully!"

    # Check if install directory is in PATH
    if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
        print_info ""
        print_info "Note: $INSTALL_DIR is not in your PATH."
        print_info "Add the following line to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
        print_info ""
        print_info "    export PATH=\"\$PATH:$INSTALL_DIR\""
        print_info ""
    fi

    print_info "Run 'gh0st --help' to get started!"
}

# Check dependencies
check_dependencies() {
    local missing_deps=()

    if ! command -v curl &> /dev/null; then
        missing_deps+=("curl")
    fi

    if ! command -v tar &> /dev/null; then
        missing_deps+=("tar")
    fi

    if [ ${#missing_deps[@]} -ne 0 ]; then
        print_error "Missing required dependencies: ${missing_deps[*]}"
        print_error "Please install them and try again."
        exit 1
    fi
}

main() {
    echo "gh0st Installation Script"
    echo "========================="
    echo ""

    check_dependencies
    install_gh0st
}

main
