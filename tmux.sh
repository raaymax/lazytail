#!/bin/bash

SESSION="lazytail"
DIR="$(cd "$(dirname "$0")" && pwd)"

if tmux has-session -t "$SESSION" 2>/dev/null; then
    tmux attach-session -t "$SESSION"
    exit 0
fi

tmux new-session -d -s "$SESSION" -c "$DIR"

tmux rename-window -t "$SESSION:1" "nvim"
tmux send-keys -t "$SESSION:1" "nvim" Enter

tmux new-window -t "$SESSION" -n "claude" -c "$DIR"
tmux send-keys -t "$SESSION:2" "claude --dangerously-skip-permissions" Enter

tmux new-window -t "$SESSION" -n "shell" -c "$DIR"

tmux select-window -t "$SESSION:1"
tmux attach-session -t "$SESSION"
