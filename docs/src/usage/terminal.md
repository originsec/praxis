# Terminal

The terminal feature gives you direct shell access to nodes. This is a full PTY terminal - a separate shell on the target system.

## Opening a Terminal

From a node:

1. Click the **Terminal** button
2. A terminal panel opens
3. You have a shell on that node

The terminal uses xterm.js for rendering, so you get proper terminal emulation with colors, cursor movement, and escape sequences.

## What You Can Do

This is a real shell. You can:

- Run commands on the target system
- Navigate the filesystem
- View and edit files
- Run scripts
- Check system status

The shell runs as the same user that runs the Praxis node.

## Terminal vs Agent Session

These are different things:

| Terminal | Agent Session |
|----------|---------------|
| Direct shell access | AI agent interaction |
| Raw commands | Natural language prompts |
| System-level | Agent-level |
| No AI involved | AI processes requests |

Use the terminal for direct system work. Use sessions for agent interaction.

## Use Cases

**Debugging** - Check logs, inspect files, verify the node is working correctly.

**Preparation** - Set up environments, install dependencies, configure the system before running operations.

**Manual Operations** - Sometimes you just need a shell. The terminal is there when you need it.

**Verification** - After an operation runs, verify the results directly.

## Terminal Persistence

The terminal session persists while you have the panel open. Closing the panel ends the shell session. There's no background persistence-this is an interactive terminal.

## Limitations

- One terminal per node at a time
- Runs as the node's user
- Subject to the node's environment and permissions

## Troubleshooting

### Terminal won't connect

- Verify the node is online
- Check RabbitMQ connectivity
- Look at node logs

### Commands not working

- Check the node's environment
- Verify PATH settings
- Ensure required tools are installed

### Display issues

- Terminal size may need adjustment
- Some applications may not render correctly
- Try simpler commands to verify basic function
