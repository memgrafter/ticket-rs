#!/usr/bin/env bash
# macOS launchd user agents for the tk queue:
#   local.tk.queue-serve        tk queue serve on 127.0.0.1:7878
#   local.tk.queue-tunnel-vert  ssh -R so vert's 127.0.0.1:7878 reaches it
# Logs: ~/Library/Logs/tk/. Needs ~/.local/bin/tk with `tk queue` (setup/install.sh).
# Usage: setup/queue.sh install|uninstall|status
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
agents_dir="$HOME/Library/LaunchAgents"
log_dir="$HOME/Library/Logs/tk"
domain="gui/$(id -u)"
labels=(local.tk.queue-serve local.tk.queue-tunnel-vert)
port=7878
tunnel_host=vert

is_loaded() { launchctl print "$domain/$1" >/dev/null 2>&1; }

install_agents() {
    if ! "$HOME/.local/bin/tk" queue --help >/dev/null 2>&1; then
        echo "~/.local/bin/tk has no 'queue' command; run setup/install.sh first" >&2
        exit 1
    fi
    mkdir -p "$agents_dir" "$log_dir"
    for label in "${labels[@]}"; do
        plist="$agents_dir/$label.plist"
        if is_loaded "$label"; then
            launchctl bootout "$domain/$label"
        fi
        sed -e "s|__HOME__|$HOME|g" "$here/launchd/$label.plist" > "$plist"
        plutil -lint "$plist" >/dev/null
        launchctl bootstrap "$domain" "$plist"
        echo "loaded $label"
    done
}

uninstall_agents() {
    for label in "${labels[@]}"; do
        if is_loaded "$label"; then
            launchctl bootout "$domain/$label"
            echo "unloaded $label"
        fi
        rm -f "$agents_dir/$label.plist"
    done
}

show_status() {
    for label in "${labels[@]}"; do
        if is_loaded "$label"; then
            echo "$label: $(launchctl print "$domain/$label" | awk -F' = ' '/^\tstate|^\tpid|^\tlast exit code/ {gsub(/\t/, ""); printf "%s=%s ", $1, $2}')"
        else
            echo "$label: not loaded"
        fi
    done
    printf 'server on this Mac: '
    curl -fsS --max-time 5 "http://127.0.0.1:$port/health" || echo "DOWN"
    echo
    printf 'through the tunnel on %s: ' "$tunnel_host"
    ssh -o BatchMode=yes -o ConnectTimeout=10 "$tunnel_host" \
        "curl -fsS --max-time 5 http://127.0.0.1:$port/health" || echo "DOWN"
    echo
}

case "${1:-}" in
    install) install_agents; sleep 2; show_status ;;
    uninstall) uninstall_agents ;;
    status) show_status ;;
    *) echo "usage: $0 install|uninstall|status" >&2; exit 2 ;;
esac
