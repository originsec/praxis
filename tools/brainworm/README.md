# Brainworm
Brainworm is computer-use agent (CUA) native promptware, introduced in our [blog post](https://originhq.com/blog/brainworm). It leverages memory files for persistence, and re-uses tool calls to communicate with the Praxis C2 server.

To deploy Brainworm, you will need to update the default credentials to your local instance Praxis (by default `praxis:praxis`), and the base url within the `AGENTS.md` file.

Finally, place it in the user directory, depending on the computer use agent you wish to infect, renaming the file to match the expected format. 
- `~/.claude/CLAUDE.md`
- `~/.codex/AGENTS.md`
- `~/.gemini/GEMINI.md`

You can also place it in a project root instead.