// Choosing which model an AI-assisted feature uses, outside the Model Lab.
// It starts from the Model Lab default, always shows which connection and model will be used,
// and asks before anything is sent to a connection that is not on this machine.
import { useEffect, useRef, useState } from "react";
import { api, type Provider } from "../lib/api";
import { hostOf } from "../lib/lab";
import { useAsync } from "../lib/util";
import { Input, Select } from "./ui";

export interface ModelChoiceState {
  providerId: string;
  model: string;
  provider: Provider | undefined;
  /** The user ticked the box allowing data to be sent to a remote connection. */
  allowRemote: boolean;
  setProvider: (providerId: string) => void;
  setModel: (model: string) => void;
  setAllowRemote: (allow: boolean) => void;
  /** Why the choice cannot be used yet, if it cannot. */
  problem: string | undefined;
}

/**
 * `preselect`: "default" starts on the Model Lab default; "local" does so only when that
 * default stays on this machine; "none" starts with no AI selected.
 */
export function useModelChoice(providers: Provider[], preselect: "default" | "local" | "none"): ModelChoiceState {
  const enabled = providers.filter((p) => p.enabled);
  const [providerId, setProviderId] = useState("");
  const [model, setModel] = useState("");
  const [allowRemote, setAllowRemote] = useState(false);
  const settings = useAsync(api.labSettings, []);
  const applied = useRef(false);

  useEffect(() => {
    if (applied.current || !settings.data || enabled.length === 0) return;
    applied.current = true;
    if (preselect === "none") return;
    const p = enabled.find((x) => x.id === settings.data?.default_provider_id);
    if (!p || (preselect === "local" && !p.local)) return;
    setProviderId(p.id);
    setModel(settings.data.default_model ?? p.default_model ?? "");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settings.data, enabled.length]);

  const provider = enabled.find((p) => p.id === providerId);
  const problem = !providerId ? undefined
    : !provider ? "That connection is no longer available"
      : !model.trim() ? "Choose a model"
        : !provider.local && !allowRemote ? `Confirm sending to ${hostOf(provider.base_url)} first`
          : undefined;

  return {
    providerId,
    model,
    provider,
    allowRemote,
    setProvider: (id) => {
      const p = enabled.find((x) => x.id === id);
      setProviderId(id);
      setAllowRemote(false);
      setModel(p ? (p.id === settings.data?.default_provider_id ? settings.data?.default_model : null) ?? p.default_model ?? "" : "");
    },
    setModel,
    setAllowRemote,
    problem,
  };
}

/** The connection and model selectors. `noneLabel` adds a "no AI" option. */
export function ModelChoiceControls({ choice, providers, noneLabel, idPrefix, disabled }: {
  choice: ModelChoiceState; providers: Provider[]; noneLabel?: string; idPrefix: string; disabled?: boolean;
}) {
  const enabled = providers.filter((p) => p.enabled);
  const models = useAsync(() => (choice.providerId ? api.listModels(choice.providerId) : Promise.resolve([])), [choice.providerId]);
  return (
    <>
      <Select aria-label="AI connection" className="w-56!" value={choice.providerId} disabled={disabled} onChange={(e) => choice.setProvider(e.target.value)}>
        {noneLabel !== undefined && <option value="">{noneLabel}</option>}
        {noneLabel === undefined && !choice.provider && <option value="">Choose a connection…</option>}
        {enabled.map((p) => <option key={p.id} value={p.id}>{p.name}{p.local ? "" : " (remote)"}</option>)}
      </Select>
      {choice.providerId && (
        <>
          <Input
            aria-label="Model"
            className="w-56! font-mono"
            list={`${idPrefix}-models`}
            placeholder={models.loading ? "Loading models…" : "Model"}
            title={models.error ? `Could not list models: ${models.error}` : undefined}
            value={choice.model}
            disabled={disabled}
            onChange={(e) => choice.setModel(e.target.value)}
          />
          <datalist id={`${idPrefix}-models`}>{models.data?.map((m) => <option key={m.id} value={m.id} />)}</datalist>
        </>
      )}
    </>
  );
}

/** Says where the request goes. For a remote connection it must be ticked before anything is sent. */
export function ModelChoiceNotice({ choice, what }: { choice: ModelChoiceState; what: string }) {
  const p = choice.provider;
  if (!p) return null;
  if (p.local) return <div data-testid="model-local" className="text-2xs text-faint">Answered by <span className="font-mono">{choice.model || "a model"}</span> on this machine ({p.name}). Nothing leaves it.</div>;
  return (
    <label data-testid="remote-notice" className="flex items-start gap-2 rounded-md border border-warn/30 bg-warn/10 px-3 py-2 text-xs leading-relaxed text-warn">
      <input type="checkbox" className="mt-0.5 accent-warn" checked={choice.allowRemote} onChange={(e) => choice.setAllowRemote(e.target.checked)} />
      <span><span className="font-semibold">This leaves your machine.</span> {what} will be sent to <span className="font-mono">{hostOf(p.base_url)}</span> ({p.name}). Tick to allow it.</span>
    </label>
  );
}
