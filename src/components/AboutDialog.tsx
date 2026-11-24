import { Bug, Check, Copy, FileDown, Lightbulb, LifeBuoy, X } from "lucide-react";
import { useEffect, useState } from "react";
import { api, type FeedbackInfo, type FeedbackPage, type PlatformInfo } from "../lib/api";
import { errorText } from "../lib/util";
import { Button, ErrorBanner, Spinner } from "./ui";

/** Version, system details and feedback. The diagnostic report is shown in full and can be
 *  edited before it is copied or saved; NEXUS never sends it anywhere itself. */
export function AboutDialog({ onClose }: { onClose: () => void }) {
  const [info, setInfo] = useState<PlatformInfo>();
  const [feedback, setFeedback] = useState<FeedbackInfo>();
  const [report, setReport] = useState("");
  const [note, setNote] = useState<string>();
  const [error, setError] = useState<string>();

  useEffect(() => {
    Promise.all([api.platformInfo(), api.feedbackInfo()])
      .then(([p, f]) => {
        setInfo(p);
        setFeedback(f);
        setReport(f.diagnostics);
      })
      .catch((e) => setError(errorText(e)));
  }, []);

  const act = async (fn: () => Promise<string | null | void>, done: string) => {
    setError(undefined);
    setNote(undefined);
    try {
      const result = await fn();
      if (result !== null) setNote(done);
    } catch (e) {
      setError(errorText(e));
    }
  };
  const open = (kind: FeedbackPage, what: string) => act(() => api.openFeedback(kind), `${what} opened in your browser.`);

  return (
    <div className="absolute inset-0 z-40 grid place-items-center bg-black/60 p-6" onMouseDown={onClose}>
      <div
        role="dialog"
        aria-label="About NEXUS and feedback"
        data-testid="about-dialog"
        className="flex max-h-full w-full max-w-2xl flex-col rounded-lg border border-line bg-panel"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.stopPropagation()}
      >
        <div className="flex h-11 shrink-0 items-center gap-2 border-b border-line px-4">
          <LifeBuoy size={15} className="text-lav" />
          <span className="text-sm font-semibold">About NEXUS and feedback</span>
          <button className="ml-auto grid size-7 place-items-center rounded text-faint hover:bg-raised hover:text-ink" onClick={onClose} title="Close"><X size={14} /></button>
        </div>
        {!info || !feedback ? (error ? <div className="p-4"><ErrorBanner message={error} /></div> : <Spinner />) : (
          <div className="space-y-4 overflow-auto p-4">
            <dl className="selectable grid grid-cols-[9rem_1fr] gap-x-3 gap-y-1.5 text-sm">
              <dt className="text-dim">Version</dt><dd data-testid="about-version">{info.app_version} (public beta)</dd>
              <dt className="text-dim">System</dt><dd>{info.os} {info.os_version ?? ""} · {info.arch}</dd>
              <dt className="text-dim">Git</dt><dd>{info.git_version ? `${info.git_version}` : <span className="text-warn">not found</span>}</dd>
              <dt className="text-dim">Terminal shell</dt><dd className="font-mono text-xs">{info.shell}</dd>
              <dt className="text-dim">API keys kept in</dt><dd>{info.secure_storage}</dd>
              <dt className="text-dim">Your data</dt><dd className="font-mono text-xs break-all">{info.data_dir}</dd>
            </dl>

            <div>
              <div className="mb-1 flex items-center justify-between">
                <span className="text-xs font-medium text-dim">Diagnostic report</span>
                <span className="text-2xs text-faint">Read it, edit it if you like. Nothing is sent until you send it.</span>
              </div>
              <textarea
                aria-label="Diagnostic report"
                value={report}
                onChange={(e) => setReport(e.target.value)}
                rows={10}
                spellCheck={false}
                className="w-full resize-y rounded-md border border-line bg-bg p-2.5 font-mono text-xs leading-relaxed focus:border-lav/60 focus:outline-none"
              />
              <p className="mt-1 text-2xs leading-relaxed text-faint">
                Contains versions, your system, which tools were found and counts. It never contains source code, terminal output,
                file or project names, provider addresses, API keys or environment variables.
              </p>
            </div>

            <ErrorBanner message={error} />
            {note && <div className="flex items-center gap-1.5 text-xs text-ok"><Check size={12} /> {note}</div>}

            <div className="flex flex-wrap gap-2">
              <Button onClick={() => act(() => navigator.clipboard.writeText(report), "Report copied.")}><Copy size={13} /> Copy report</Button>
              <Button onClick={() => act(() => api.exportText("nexus-diagnostics.md", report), "Report saved.")}><FileDown size={13} /> Save to file</Button>
              <Button disabled={!feedback.links_available} onClick={() => open("bug", "Bug report form")}><Bug size={13} /> Report a bug</Button>
              <Button disabled={!feedback.links_available} onClick={() => open("feature", "Feature request form")}><Lightbulb size={13} /> Request a feature</Button>
              <Button variant="ghost" disabled={!feedback.links_available} onClick={() => open("docs", "Documentation")}>Documentation</Button>
            </div>
            <p className="text-2xs leading-relaxed text-faint">
              {feedback.links_available
                ? `The forms are on GitHub (${feedback.repository}) and need a GitHub account. Without one, copy or save the report and send it to the maintainers by any channel you already use. Paste the report into the form yourself; NEXUS does not attach it.`
                : "This build was made without a public repository address, so the forms cannot be opened from here. Copy or save the report and send it to whoever gave you this build."}
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
