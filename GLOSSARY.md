# bitplane-cc (bitplane Command Center)

A desktop workspace for bitplane: pick a plane, work in terminal sessions rooted in it.

## Language

**Plane**:
A bitplane plane, used exactly as bitplane defines it: a named set of git worktrees sharing one lifecycle, living in its own plane directory.
_Avoid_: Workspace, project

**Member**:
A bitplane member, used exactly as bitplane defines it: one git worktree belonging to a plane.
_Avoid_: Worktree (on its own), project, repo (when meaning a plane's member)

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

**Draft plane**:
A folder in the planes directory that has no plane file yet; the app shows it and opens sessions in it like a plane, until `bp create --here` turns it into one.
_Avoid_: Empty plane, pending plane

**Busy session**:
A session whose terminal is running something other than its idle shell (e.g. Claude Code, an editor, a build); closing it asks first.
_Avoid_: Active session, running session

**Exited session**:
A session whose shell ended with an error or never started; its tab stays open showing why, and can be restarted.
_Avoid_: Dead session, crashed tab

**Agent session**:
One conversation of a coding agent such as Claude Code, run inside a session and resumable by its own id.
_Avoid_: Session (when meaning the agent's conversation), chat

**Agent link**:
The record of which agent session a session last ran; restoring the session resumes that agent session.
_Avoid_: Agent binding, resume token

**Restore**:
Reopening, on launch, the tabs that were open when the app last stopped, each with a fresh shell in its plane and its agent session resumed.
_Avoid_: Reattach (a reload reattaches to running sessions), persist

**Sidebar**:
The resizable plane list on the left edge, where planes and their tabs are picked.
_Avoid_: Plane list, navigator

**Tool bar**:
The vertical strip of icons on the right edge; always visible, it switches the tool panel between tools and holds plane actions such as opening the plane in VS Code.
_Avoid_: Sidebar (that is the plane list on the left), activity bar

**Tool**:
One view of the active plane that the tool bar can show in the tool panel, such as Source control or Files.
_Avoid_: Panel, view, extension

**Tool panel**:
The resizable area next to the tool bar that shows the active tool for the plane of the active tab; collapsed when no tool is active.
_Avoid_: Right sidebar, drawer
