import { Check, CircleDashed, FolderPlus } from "lucide-react";
import { useEffect, useState } from "react";
import type { Workspace } from "../App";
import { api, type PlatformInfo, type RuntimeReport } from "../lib/api";
import { importProject } from "../lib/importProject";
import { errorText } from "../lib/util";
import { Button, ErrorBanner } from "./ui";

function Row({ ok, title, children }: { ok: boolean | undefined; title: string; children: React.ReactNode }) {
  return (
    <div className="flex gap-3 border-t border-line py-3 first:border-t-0">
      {ok ? <Check size={15} className="mt-0.5 shrink-0 text-ok" /> : <CircleDashed size={15} className="mt-0.5 shrink-0 text-faint" />}
      <div className="min-w-0">
        <div className="text-[13px] font-medium">{title}</div>
        <div className="mt-0.5 text-xs leading-relaxed text-dim">{children}</div>
      </div>
    </div>
  );
}

/** First-run welcome. Everything on it is optional; "Start using NEXUS" closes it for good. */
export function Welcome({ ws, onDone }: { ws: Workspace; onDone: () => void }) {
  const [info, setInfo] = useState<PlatformInfo>();
  const [runtimes, setRuntimes] = useState<RuntimeReport>();
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.platformInfo().then(setInfo).catch(() => undefined);
    // Looks only at this machine's default local ports and installed programs.
    api.labRuntimes().then(setRuntimes).catch(() => undefined);
  }, []);

  const finish = () => void api.onboardingFinish().catch(() => undefined).then(onDone);
  const onImport = async () => {
    setBusy(true);
    setError(undefined);
    try {
      const p = await importProject();
      if (p) {
        await ws.reloadProjects();
        ws.selectProject(p.id);
      }
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const found = runtimes?.runtimes.filter((r) => r.installed !== null || r.endpoints.some((e) => e.running)) ?? [];
  return (
    <div className="absolute inset-0 z-30 overflow-auto bg-bg" data-testid="welcome">
      <div className="mx-auto max-w-xl px-6 py-12">
        <div className="mb-1 flex items-center gap-2.5">
          <div className="grid size-7 place-items-center rounded-md bg-accent text-sm font-black text-black">N</div>
          <h1 className="text-lg font-semibold tracking-wide">Welcome to NEXUS</h1>
        </div>
        <p className="mb-6 text-[13px] leading-relaxed text-dim">
          A developer workspace that runs on your machine: understand a codebase, search it, inspect Git, run a real terminal,
          work with local models, and let agents propose changes you approve. This is a public beta.
        </p>

        <div className="rounded-lg border border-line bg-panel px-4">
          <Row ok title="Local-first">
            Your projects, index, terminal output and settings stay in a folder on this computer. There is no account, no analytics
            and nothing is uploaded. Requests leave the machine only when you choose a remote AI provider and send one.
          </Row>
          <Row ok={ws.projects.length > 0} title={ws.projects.length > 0 ? `Project added: ${ws.projects.map((p) => p.name).join(", ")}` : "Add a project"}>
            Pick a folder with code in it. NEXUS reads it; it does not move or change your files. To start a new project, create
            the folder first, then add it here.
            <div className="mt-2"><Button size="sm" busy={busy} onClick={() => void onImport()}><FolderPlus size={13} /> Choose a folder…</Button></div>
          </Row>
          <Row ok={info ? info.git !== null : undefined} title={info?.git_version ? `Git found (${info.git_version})` : info ? "Git not found" : "Looking for Git…"}>
            {info && !info.git
              ? "History, diffs and agents need Git. Install it and restart NEXUS; everything else works without it."
              : "Used for status, history and diffs, and for the isolated workspaces agents work in."}
          </Row>
          <Row
            ok={runtimes ? found.length > 0 : undefined}
            title={runtimes ? (found.length > 0 ? `Local model runtime found: ${found.map((r) => r.name).join(", ")}` : "No local model runtime found (optional)") : "Looking for local model runtimes…"}
          >
            Model Lab works with Ollama, llama.cpp servers and OpenAI-compatible endpoints. Nothing is installed or downloaded for
            you, and no paid API key is needed. You can connect one later in Model Lab.
          </Row>
          <Row ok title="Integrations are opt-in">
            AI providers, agents and model downloads stay off until you set them up. API keys go into {info?.secure_storage ?? "your system's secure storage"}, never
            into a file.
          </Row>
        </div>

        <div className="mt-3"><ErrorBanner message={error} /></div>
        <div className="mt-5 flex items-center gap-3">
          <Button variant="primary" onClick={finish}>Start using NEXUS</Button>
          <span className="text-xs text-faint">Documentation and feedback are under “About & feedback” in the sidebar.</span>
        </div>
      </div>
    </div>
  );
}
