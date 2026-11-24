// Model defaults: the connection, model and generation settings the rest of NEXUS starts from.
// Shown in Settings.
import { useEffect, useState } from "react";
import { Button, Card, ErrorBanner, Select } from "../../components/ui";
import { api, type GenParams, type KindInfo } from "../../lib/api";
import { describeParams, emptyParams, normalizeParams, paramsFor } from "../../lib/lab";
import { errorText, useAsync } from "../../lib/util";
import { LocalBadge, ModelPicker, ParamsEditor, Section, usable, type LabCtx } from "./shared";

export function Defaults({ ctx }: { ctx: LabCtx }) {
  const providers = usable(ctx.providers);
  const [providerId, setProviderId] = useState("");
  const [model, setModel] = useState("");
  const [params, setParams] = useState<GenParams>(emptyParams());
  const [paramError, setParamError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [savedAt, setSavedAt] = useState<number>();
  const storage = useAsync(api.labStorage, []);

  const stored = ctx.settings;
  useEffect(() => {
    if (!stored) return;
    setProviderId(stored.default_provider_id ?? "");
    setModel(stored.default_model ?? "");
    setParams(normalizeParams(stored.params));
  }, [stored]);

  const provider = providers.find((p) => p.id === providerId);
  // With no default connection, offer every setting; each runtime only receives what it accepts.
  const kind: KindInfo | undefined = ctx.kinds.find((k) => k.kind === provider?.kind)
    ?? (ctx.kinds[0] ? { ...ctx.kinds[0], kind: "ollama", params: [...new Set(ctx.kinds.flatMap((k) => k.params))] } : undefined);
  const dirty = !!stored && (
    (stored.default_provider_id ?? "") !== providerId || (stored.default_model ?? "") !== model.trim()
    || JSON.stringify(normalizeParams(stored.params)) !== JSON.stringify(params)
  );

  const save = async () => {
    setBusy(true);
    setError(undefined);
    try {
      await api.labSettingsSet(providerId || null, providerId ? model.trim() || null : null, provider ? paramsFor(params, kind?.params ?? []) : params);
      await ctx.reloadSettings();
      setSavedAt(Date.now());
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card className="p-4">
      <form data-testid="lab-defaults" className="space-y-4" onSubmit={(e) => { e.preventDefault(); void save(); }}>
        <Section
          title="Defaults"
          hint="Used by the Playground and comparison runs, and preselected for project questions (Intelligence → Ask) and new agent tasks. An agent task keeps the connection and model it was created with; it never switches on its own."
        >
          <ErrorBanner message={error} />
          <div className="space-y-1">
            <div className="text-xs font-medium text-dim">Default connection and model</div>
            <div className="flex flex-wrap items-center gap-2">
              <Select aria-label="Default connection" className="w-48!" value={providerId} onChange={(e) => {
                const p = providers.find((x) => x.id === e.target.value);
                setProviderId(e.target.value);
                setModel(p?.default_model ?? "");
              }}>
                <option value="">No default</option>
                {providers.map((p) => <option key={p.id} value={p.id}>{p.name}{p.local ? "" : " (remote)"}</option>)}
              </Select>
              {provider && <ModelPicker ctx={ctx} idPrefix="defaults" providerId={providerId} model={model} onChange={(_, m) => setModel(m)} />}
              {provider && <LocalBadge provider={provider} />}
            </div>
            {provider && !provider.local && <div className="text-2xs leading-relaxed text-warn">A remote default does not send anything by itself. Each screen still asks before it sends data to it.</div>}
          </div>
          <div className="max-w-md space-y-1">
            <div className="text-xs font-medium text-dim">Default generation settings</div>
            <ParamsEditor kind={kind} value={params} onChange={setParams} onError={setParamError} />
            <div className="text-2xs text-faint">Currently: {describeParams(params)}. Empty fields leave the runtime's own default in place.</div>
          </div>
        </Section>
        <div className="flex items-center gap-3">
          <Button type="submit" variant="primary" busy={busy} disabled={!dirty || !!paramError || (!!providerId && !model.trim())}>Save defaults</Button>
          {savedAt && !dirty && <span data-testid="defaults-saved" className="text-xs text-accent">Saved</span>}
          {providerId && !model.trim() && <span className="text-xs text-warn">Choose a model for the default connection</span>}
        </div>
        <div className="border-t border-line/60 pt-3 text-xs leading-relaxed text-dim">
          <span className="text-faint">Model folder:</span> <span className="selectable font-mono">{storage.data?.dir ?? "…"}</span>{" "}
          <button type="button" className="text-accent hover:underline" onClick={() => ctx.go("library")}>Change it in Library → Storage</button>
        </div>
      </form>
    </Card>
  );
}
