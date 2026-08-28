# Read-only viewer — editing is delegated to $EDITOR

agentdocs never edits a file in place but launches `$EDITOR` and re-reads on return, because embedding an editor would mean owning text input, cursor movement, undo, saving, and conflicts with external writes — none of it what a viewer is for, all of it already present in the editor a terminal user has chosen — accepting that fixing a single typo means leaving the process and that the screen must be torn down and restored around the editor.

The rejected alternative is an **embedded editor**. chops, our motif, works that way — but chops is a macOS GUI app where the native text view already supplies input, cursor, and undo for free. A terminal grants none of that, and the work would pile up far from what we are actually trying to deliver: finding and reading scattered documentation on one screen.

The second rejected option is **no editing path at all**. It is the simplest, but a reader who spots a typo would have to leave the terminal and navigate back to a path we already know in full. Withholding a path we are holding is not restraint; it is waste.
