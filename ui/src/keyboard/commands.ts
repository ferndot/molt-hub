/**
 * commands.ts — command registry for the command palette.
 *
 * Pure data + filtering — no DOM, testable in node environment.
 */

export interface Command {
  id: string;
  label: string;
  description?: string;
  category: "navigation" | "action";
  keywords?: string[];
}

export const COMMANDS: Command[] = [
  {
    id: "goto-triage",
    label: "Go to Triage",
    description: "Open the triage queue",
    category: "navigation",
    keywords: ["triage", "queue", "tasks"],
  },
  {
    id: "goto-board",
    label: "Go to Workboard",
    description: "Open the main workboard (home)",
    category: "navigation",
    keywords: ["board", "kanban", "columns", "home", "workboard"],
  },
  {
    id: "goto-boards-list",
    label: "Go to Boards list",
    description: "View all boards and create new ones",
    category: "navigation",
    keywords: ["boards", "list", "create"],
  },
  {
    id: "goto-agents",
    label: "Go to Agents",
    description: "View agent list",
    category: "navigation",
    keywords: ["agents", "workers", "runners"],
  },
  {
    id: "goto-code-chat",
    label: "Go to Claude Code",
    description: "Project chat — Claude CLI session in your repo",
    category: "navigation",
    keywords: ["claude", "code", "chat", "terminal", "cli", "copilot"],
  },
  {
    id: "approve-item",
    label: "Approve Selected",
    description: "Approve the selected triage item",
    category: "action",
    keywords: ["approve", "accept", "ok"],
  },
  {
    id: "reject-item",
    label: "Reject Selected",
    description: "Reject the selected triage item",
    category: "action",
    keywords: ["reject", "deny", "decline"],
  },
  {
    id: "show-help",
    label: "Show Keyboard Shortcuts",
    description: "Display all available keyboard shortcuts",
    category: "action",
    keywords: ["help", "shortcuts", "keys", "hotkeys"],
  },
];

/**
 * filterCommands performs a simple fuzzy match of `query` against command labels,
 * descriptions, and keywords. Returns matching commands in priority order.
 */
export function filterCommands(query: string): Command[] {
  const q = query.trim().toLowerCase();
  if (q === "") return COMMANDS;

  return COMMANDS.filter((cmd) => {
    const searchable = [
      cmd.label,
      cmd.description ?? "",
      ...(cmd.keywords ?? []),
    ]
      .join(" ")
      .toLowerCase();
    return searchable.includes(q);
  });
}
