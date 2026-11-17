import type { Workspace } from "../App";
import { Select } from "./ui";

export function ProjectPicker({ ws, className }: { ws: Workspace; className?: string }) {
  return (
    <Select
      aria-label="Project"
      className={className ?? "w-56!"}
      title="Current project"
      value={ws.projectId ?? ""}
      onChange={(e) => ws.selectProject(e.target.value || null)}
    >
      {ws.projects.map((p) => (
        <option key={p.id} value={p.id}>
          {p.name}
        </option>
      ))}
    </Select>
  );
}
