import type { Project } from "./types";

export function resolveDefaultProjectId(projects: Project[], requestedProjectId?: string) {
  if (requestedProjectId && projects.some((project) => project.id === requestedProjectId)) {
    return requestedProjectId;
  }
  return projects[0]?.id ?? null;
}
