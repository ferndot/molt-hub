import { For, Show } from "solid-js";
import { projectState, switchProject } from "../../stores/projectStore";

export function ProjectSwitcher() {
  return (
    <Show when={projectState.projects.length > 1}>
      <div class="project-switcher">
        <select
          value={projectState.activeProjectId}
          onChange={(e) => switchProject(e.currentTarget.value)}
        >
          <For each={projectState.projects}>
            {(p) => <option value={p.id}>{p.name}</option>}
          </For>
        </select>
      </div>
    </Show>
  );
}
