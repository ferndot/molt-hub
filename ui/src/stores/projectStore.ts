import { createStore } from "solid-js/store";

export interface Project {
  id: string;
  name: string;
  repo_path: string;
  status: "active" | "archived";
  created_at: string;
  updated_at: string;
}

interface ProjectState {
  projects: Project[];
  activeProjectId: string;
  loaded: boolean;
}

const [projectState, setProjectState] = createStore<ProjectState>({
  projects: [],
  activeProjectId: "default",
  loaded: false,
});

export { projectState };

export async function loadProjects(): Promise<void> {
  try {
    const res = await fetch("/api/projects");
    if (!res.ok) return;
    const data = (await res.json()) as { projects: Project[] };
    setProjectState("projects", data.projects);
    // If there's exactly one project and no active selection, use it
    if (
      data.projects.length === 1 &&
      projectState.activeProjectId === "default"
    ) {
      setProjectState("activeProjectId", data.projects[0].id);
    }
  } catch {
    // backend unavailable — keep defaults
  } finally {
    setProjectState("loaded", true);
  }
}

export function switchProject(id: string): void {
  setProjectState("activeProjectId", id);
}

/** Create a project via POST /api/projects (snake_case body for Rust API). */
export async function createProject(
  name: string,
  repoPath: string,
): Promise<{ ok: true; project: Project } | { ok: false; error: string }> {
  const trimmedName = name.trim();
  const trimmedPath = repoPath.trim();
  if (!trimmedName || !trimmedPath) {
    return { ok: false, error: "Name and repository path are required." };
  }
  try {
    const res = await fetch("/api/projects", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: trimmedName, repo_path: trimmedPath }),
    });
    const data = (await res.json()) as Project | { error: string };
    if (!res.ok) {
      const msg =
        typeof data === "object" && data && "error" in data
          ? String((data as { error: string }).error)
          : `HTTP ${res.status}`;
      return { ok: false, error: msg };
    }
    const project = data as Project;
    setProjectState("projects", (p) => [...p, project]);
    setProjectState("activeProjectId", project.id);
    return { ok: true, project };
  } catch (e) {
    return {
      ok: false,
      error: e instanceof Error ? e.message : "Network error",
    };
  }
}
