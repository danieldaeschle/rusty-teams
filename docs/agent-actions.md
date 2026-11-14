# Agent actions

Delegate a message to a coding-agent CLI. Hover a message, click the robot button, pick an action.

## Setup

| Step | What |
|---|---|
| 1 | Install the agent CLI and log in once, inside WSL for the WSL presets |
| 2 | WSL: set `networkingMode=mirrored` in `%UserProfile%\.wslconfig`, so the agent reaches the app's MCP endpoint on `127.0.0.1` |
| 3 | Settings > Agents: pick or add an agent, press Test |
| 4 | Settings > Agents: add actions. Their order is the order in the toolbar menu |

## Flow

| Where | What |
|---|---|
| Hover toolbar | Robot button before "..." lists the actions, "Manage actions..." opens Settings > Agents |
| Start dialog | Only for actions with "Ask for extra instructions", or for chats agents cannot see yet. Ctrl+Enter starts, Esc cancels, "Show prompt" shows the rendered prompt |
| Chip under the message | Step, time, Stop. Click expands the last 6 steps. Failed: Log, Retry |
| Rail > Agents | Own page like Saved, Ctrl+6. Shown when at least one action exists. Badge = running runs. Running and Done lists, row click jumps to the message |
| Toast | On finish or failure: Open chat, Open session (Retry when failed) |
| Open session | Starts the agent's resume command in a terminal. Runs stay after a restart (last 50) |

## Placeholders

| Placeholder | Where | Value |
|---|---|---|
| `{message}` | prompt, extra args | Text of the message |
| `{sender}` | prompt, extra args | Sender of the message |
| `{chat}` | prompt, extra args | Chat or channel title |
| `{thread}` | prompt, extra args | Up to 20 earlier messages, oldest first |
| `{extra}` | prompt, extra args | Text from the start dialog |
| `{conversation_id}` | prompt, extra args | Teams conversation id |
| `{message_id}` | prompt, extra args | Teams message id |
| `{session_id}` | all | New session id of the run |
| `{agent}` | all | Agent id |
| `{prompt}` | command | Rendered prompt as one argument. Without it the prompt goes to stdin |
| `{cwd}` | command, open session | Working dir of the action |

## Presets

| Preset | Runs in | Output | Open session |
|---|---|---|---|
| Claude Code (WSL) | WSL default distro | Claude stream-json | `wt.exe wsl.exe --cd {cwd} -e claude --resume {session_id}` |
| Claude Code (Windows) | Windows | Claude stream-json | `wt.exe -d {cwd} claude --resume {session_id}` |
| Copilot CLI (WSL) | WSL default distro | Copilot JSON | `wt.exe wsl.exe --cd {cwd} -e copilot --resume {session_id}` |
| Custom | Windows | Plain text | empty |

## Limits

| Topic | State |
|---|---|
| Replies | Agents never send on their own. They write through the MCP tool `write_message`, and the user reviews each message unless review is off (Settings > MCP) |
