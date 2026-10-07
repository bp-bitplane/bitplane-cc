# bitplane-cc (bitplane Command Center)

A desktop workspace for bitplane: pick a plane, work in terminal sessions rooted in it.

## Language

**Plane**:
A bitplane plane, used exactly as bitplane defines it: a named set of git worktrees sharing one lifecycle, living in its own plane directory.
_Avoid_: Workspace, project

**Plane root**:
The plane's directory; every new session starts here.
_Avoid_: Plane path, plane folder

**Session**:
One shell process with its own terminal, belonging to exactly one plane.
_Avoid_: Terminal, shell, tab

**Tab**:
The UI handle of one session; there is always exactly one tab per session.
_Avoid_: Session (when meaning the UI element)

**Gone plane**:
A plane that disappeared or was renamed while it still has open sessions; its sessions keep running.
_Avoid_: Dead plane, orphan

**Busy session**:
A session whose terminal is running something other than its idle shell (e.g. Claude Code, an editor, a build); closing it asks first.
_Avoid_: Active session, running session

**Exited session**:
A session whose shell ended with an error or never started; its tab stays open showing why, and can be restarted.
_Avoid_: Dead session, crashed tab
