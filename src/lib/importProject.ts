import { open } from "@tauri-apps/plugin-dialog";
import { api, type Project } from "./api";

/** Ask the user for a folder and import it. Resolves to null if they cancel. */
export async function importProject(): Promise<Project | null> {
  const dir = await open({ directory: true, multiple: false, title: "Import a project folder" });
  if (typeof dir !== "string") return null;
  return api.addProject(dir);
}

/** Native confirmation dialog (window.confirm is unreliable inside webviews). */
export async function confirmAction(message: string): Promise<boolean> {
  const { confirm } = await import("@tauri-apps/plugin-dialog");
  return confirm(message, { title: "NEXUS", kind: "warning" });
}
