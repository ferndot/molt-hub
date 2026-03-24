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
