import { open } from "@tauri-apps/plugin-dialog";
import { api, type Project } from "./api";

/** Ask the user for a folder and import it. Resolves to null if they cancel. */
export async function importProject(): Promise<Project | null> {
  const dir = await open({ directory: true, multiple: false, title: "Import a project folder" });
  if (typeof dir !== "string") return null;
  return api.addProject(dir);
}
