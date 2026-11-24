// First run: four short steps, each skippable. Everything here is a real action — importing,
// analysing and indexing a project, connecting a local model — so the user leaves with a project
// they can explore, not a tour of empty screens.
import clsx from "clsx";
import { Check, Cpu, FileText, FolderPlus, GitBranch, Lock, Network, TerminalSquare, Workflow } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import type { Workspace } from "../App";
import { api, type ConnectionTest, type IndexStatus, type LocalRuntime, type PlatformInfo, type Project, type Provider } from "../lib/api";
import { importProject } from "../lib/importProject";
import { errorText, formatNumber, primaryLanguage } from "../lib/util";
import { Button, ErrorBanner } from "./ui";

const STEPS = ["Welcome", "Your project", "A model (optional)", "Start"] as const;

/** Steps before the current one show a check only if they were completed, a dash if skipped. */
function Steps({ at, done }: { at: number; done: boolean[] }) {
  return (
    <ol className="mb-8 flex items-center gap-2 text-xs" aria-label="Setup steps">
      {STEPS.map((s, i) => (
        <li key={s} aria-current={i === at ? "step" : undefined} className="flex items-center gap-2">
          <span className={clsx("grid size-5 place-items-center rounded-full border text-2xs", i < at && done[i] ? "border-accent bg-accent text-black" : i === at ? "border-accent text-accent" : "border-line text-faint")}>
            {i < at ? (done[i] ? <Check size={11} /> : <span aria-label="skipped">–</span>) : i + 1}
          </span>
          <span className={i === at ? "text-ink" : "text-faint"}>{s}</span>
          {i < STEPS.length - 1 && <span className="mx-1 h-px w-6 bg-line" />}
        </li>
      ))}
    </ol>
  );
}

function Point({ icon, title, children }: { icon: ReactNode; title: string; children: ReactNode }) {
  return (
    <div className="flex gap-3">
      <div className="mt-0.5 text-accent">{icon}</div>
      <div>
        <div className="text-sm font-medium">{title}</div>
        <div className="text-sm leading-relaxed text-dim">{children}</div>
      </div>
    </div>
  );
}

function Next({ icon, title, hint, onClick }: { icon: ReactNode; title: string; hint: string; onClick: () => void }) {
  return (
    <button onClick={onClick} className="flex w-full items-start gap-3 rounded-lg border border-line bg-panel px-4 py-3 text-left hover:border-accent/50">
      <div className="mt-0.5 text-accent">{icon}</div>
      <div>
        <div className="text-sm font-medium">{title}</div>
        <div className="text-xs leading-relaxed text-dim">{hint}</div>
      </div>
    </button>
  );
}

/** First-run setup. "Skip setup" or finishing closes it for good. */
export function Welcome({ ws, onDone }: { ws: Workspace; onDone: () => void }) {
  const [step, setStep] = useState(0);
  const [info, setInfo] = useState<PlatformInfo>();
  const [project, setProject] = useState<Project>();
  const [index, setIndex] = useState<IndexStatus>();
  const [phase, setPhase] = useState<string>();
  const [local, setLocal] = useState<LocalRuntime[]>();
  const [providers, setProviders] = useState<Provider[]>([]);
  const [test, setTest] = useState<ConnectionTest>();
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.platformInfo().then(setInfo).catch(() => undefined);
    api.listProviders().then(setProviders).catch(() => undefined);
  }, []);
  useEffect(() => {
    // Looks only at this machine's default local ports.
    if (step === 2 && !local) api.detectLocal().then(setLocal).catch(() => setLocal([]));
  }, [step, local]);

  const finish = (then?: () => void) => void api.onboardingFinish().catch(() => undefined).then(() => {
    onDone();
    then?.();
  });

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    setError(undefined);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
      setPhase(undefined);
    }
  };

  const addProject = () => run(async () => {
    const p = await importProject();
    if (!p) return;
    setProject(p);
    await ws.reloadProjects();
    ws.selectProject(p.id);
    setPhase("Indexing symbols and imports…");
    setIndex(await api.rebuildIndex(p.id));
  });

  const connectOllama = (rt: LocalRuntime) => run(async () => {
    const existing = providers.find((p) => p.kind === "ollama" && p.base_url === rt.base_url);
    const provider = existing ?? (await api.saveProvider({ name: "Ollama (this computer)", kind: "ollama", base_url: rt.base_url }));
    setProviders(await api.listProviders());
    setTest(await api.testProvider(provider.id));
  });

  const startOllama = () => run(async () => {
    setPhase("Starting Ollama on this computer…");
    await api.labRuntimeStart({ runtime: "ollama", port: 11434, model_file: null, context: null });
    for (let i = 0; i < 20; i++) {
      await new Promise((r) => setTimeout(r, 500));
      const found = await api.detectLocal();
      if (found.some((r) => r.kind === "ollama" && r.running)) {
        setLocal(found);
        return;
      }
    }
    throw new Error("Ollama was started but did not answer yet. Check Model Lab → Runtimes for its log.");
  });

  const a = project?.analysis;
  const issues = a?.issues.filter((i) => i.severity === "high" || i.severity === "medium").length ?? 0;
  const ollama = local?.find((r) => r.kind === "ollama");
  const hasModel = providers.length > 0 || test?.ok;

  return (
    <div className="absolute inset-0 z-30 overflow-auto bg-bg" data-testid="welcome">
      <div className="mx-auto max-w-2xl px-8 py-12">
        <div className="mb-6 flex items-center justify-between">
          <div className="flex items-center gap-2.5">
            <div className="grid size-7 place-items-center rounded-md bg-accent text-sm font-black text-black">N</div>
            <span className="text-md font-semibold tracking-wide">NEXUS</span>
          </div>
          {step < 3 && <Button variant="ghost" size="sm" onClick={() => finish()}>Skip setup</Button>}
        </div>
        <Steps at={step} done={[true, !!project, !!hasModel, true]} />

        {step === 0 && (
          <section className="space-y-6">
            <div>
              <h1 className="text-lg font-semibold">Understand a codebase on your own machine — then change it only through you.</h1>
              <p className="mt-2 text-sm leading-relaxed text-dim">
                NEXUS maps any local project without AI: its modules, dependencies, cycles and hotspots. When you want help, agents
                plan and make changes in a separate copy of the project, and nothing reaches your files until you approve the diff.
              </p>
            </div>
            <div className="space-y-4">
              <Point icon={<Network size={16} />} title="Works without AI">Architecture map, symbol and reference search, cross-project search, Git and a real terminal need no model and no key.</Point>
              <Point icon={<Workflow size={16} />} title="Agents that ask first">The plan, every command and the final diff wait for your approval; commands run sandboxed where your system supports it.</Point>
              <Point icon={<Lock size={16} />} title="Local-first">No account, no telemetry. Your code is sent nowhere unless you pick a remote model and confirm it for that request. API keys go into {info?.secure_storage ?? "your system's secure storage"}.</Point>
            </div>
            <Button variant="primary" onClick={() => setStep(1)}>Get started</Button>
          </section>
        )}

        {step === 1 && (
          <section className="space-y-5">
            <div>
              <h1 className="text-lg font-semibold">Add a project</h1>
              <p className="mt-2 text-sm leading-relaxed text-dim">
                Choose a folder with code in it. NEXUS reads it and builds an index of its symbols and imports — that is what the map,
                search and questions use. Your files are not moved or changed.
                {info && !info.git && " Git was not found: history, diffs and agents need it; everything else works."}
              </p>
            </div>
            {!project && <Button variant="primary" busy={busy} onClick={() => void addProject()}><FolderPlus size={14} /> Choose a folder…</Button>}
            {phase && <div className="text-xs text-dim" role="status">{phase}</div>}
            {project && (
              <div className="rounded-lg border border-line bg-panel p-4" data-testid="welcome-project">
                <div className="flex items-center gap-2 text-sm font-medium"><Check size={14} className="text-ok" /> {project.name}</div>
                <div className="mt-1 font-mono text-xs text-faint">{project.path}</div>
                {a && (
                  <div className="mt-3 grid grid-cols-3 gap-3 text-sm">
                    <div><div className="font-semibold tabular-nums">{formatNumber(a.file_count)}</div><div className="text-xs text-dim">files · {primaryLanguage(a.languages) ?? "no code detected"}</div></div>
                    <div><div className="font-semibold tabular-nums">{index ? formatNumber(index.symbols) : "…"}</div><div className="text-xs text-dim">symbols indexed</div></div>
                    <div><div className="font-semibold tabular-nums">{issues}</div><div className="text-xs text-dim">findings to look at</div></div>
                  </div>
                )}
              </div>
            )}
            <div className="flex gap-3">
              <Button variant={project ? "primary" : "default"} disabled={busy} onClick={() => setStep(2)}>{project ? "Continue" : "Skip for now"}</Button>
            </div>
          </section>
        )}

        {step === 2 && (
          <section className="space-y-5">
            <div>
              <h1 className="text-lg font-semibold">Connect a model (optional)</h1>
              <p className="mt-2 text-sm leading-relaxed text-dim">
                A model is needed only for questions about the code, AI-planned tasks and agents. A local runtime keeps everything on
                this computer and needs no key. You can add Anthropic, OpenAI or any OpenAI-compatible server later in Settings.
              </p>
            </div>
            <div className="rounded-lg border border-line bg-panel p-4 text-sm">
              {!local && <div className="text-dim">Looking for Ollama and LM Studio on this computer…</div>}
              {local && ollama?.running && (
                <div className="space-y-3">
                  <div className="flex items-center gap-2"><Cpu size={14} className="text-accent" /> Ollama is running at <span className="font-mono text-xs">{ollama.base_url}</span> with {ollama.models.length} model{ollama.models.length === 1 ? "" : "s"}.</div>
                  {!test && <Button variant="primary" busy={busy} onClick={() => void connectOllama(ollama)}>Connect and test</Button>}
                  {test && (
                    <div className={test.ok ? "text-ok" : "text-danger"} role="status">
                      {test.ok ? `Connected${test.latency_ms !== null ? ` (${test.latency_ms} ms)` : ""}. ${ollama.models.length === 0 ? "Download a model in Model Lab → Library to use it." : "Ready to use."}` : test.detail}
                    </div>
                  )}
                </div>
              )}
              {local && !ollama?.running && (
                <div className="space-y-3 text-dim">
                  <div>No local model server is running.</div>
                  <div className="flex flex-wrap gap-2">
                    <Button busy={busy} onClick={() => void startOllama()}>Start Ollama if it is installed</Button>
                    <Button variant="ghost" onClick={() => finish(() => ws.go("settings"))}>Set up a connection in Settings</Button>
                  </div>
                  <div className="text-xs text-faint">NEXUS does not install runtimes or download models for you.</div>
                </div>
              )}
              {phase && <div className="mt-2 text-xs text-dim" role="status">{phase}</div>}
            </div>
            <Button variant={hasModel ? "primary" : "default"} disabled={busy} onClick={() => setStep(3)}>{hasModel ? "Continue" : "Skip — no AI for now"}</Button>
          </section>
        )}

        {step === 3 && (
          <section className="space-y-4">
            <h1 className="text-lg font-semibold">You're set{project ? `: ${project.name} is ready` : ""}</h1>
            <p className="text-sm leading-relaxed text-dim">A few good first things to try:</p>
            <div className="grid gap-2">
              <Next icon={<Network size={16} />} title="See the architecture map" hint="Modules, imports, cycles and hotspots — no AI needed." onClick={() => finish(() => ws.go("intel"))} />
              {project && <Next icon={<FileText size={16} />} title="Share a codebase report" hint="One HTML file explaining the project's structure to someone new. It contains no source code." onClick={() => finish(() => ws.go("dashboard"))} />}
              <Next icon={<GitBranch size={16} />} title="Review your uncommitted changes" hint="Diffs, staging and committing what you staged." onClick={() => finish(() => ws.go("git"))} />
              <Next icon={<TerminalSquare size={16} />} title="Open a terminal in the project" hint="Your own shell, in tabs and splits that survive a restart." onClick={() => finish(() => ws.go("terminal"))} />
              {hasModel && <Next icon={<Workflow size={16} />} title="Give an agent a small task" hint="It plans, works in a copy of your project, and asks before anything reaches your files." onClick={() => finish(() => ws.go("agents"))} />}
            </div>
            <div className="pt-2 text-xs text-faint">Documentation and feedback are under “About & feedback” in the sidebar. You can show this setup again from Settings → General.</div>
          </section>
        )}

        <div className="mt-4"><ErrorBanner message={error} /></div>
      </div>
    </div>
  );
}
