// Connections, connection kinds, Model Lab defaults and the model library, loaded once for a
// screen. Used by the Model Lab and by Settings, so both show the same data.
import { useCallback, useEffect, useState } from "react";
import { api, type Library } from "../../lib/api";
import { errorText, useAsync } from "../../lib/util";

export function useLabData() {
  const providers = useAsync(api.listProviders, []);
  const kinds = useAsync(api.labKinds, []);
  const settings = useAsync(api.labSettings, []);
  const [library, setLibrary] = useState<Library>();
  const [libraryBusy, setLibraryBusy] = useState(false);
  const [libraryError, setLibraryError] = useState<string>();

  const refreshLibrary = useCallback(async () => {
    setLibraryBusy(true);
    try {
      setLibrary(await api.labModelsRefresh());
      setLibraryError(undefined);
    } catch (e) {
      setLibraryError(errorText(e));
    } finally {
      setLibraryBusy(false);
    }
  }, []);

  // Show what is cached at once, then ask the runtimes what they have now.
  useEffect(() => {
    api.labModels().then((cached) => setLibrary((current) => current ?? cached)).catch(() => undefined);
    void refreshLibrary();
  }, [refreshLibrary]);

  return {
    providers: providers.data ?? [],
    providersLoaded: providers.data !== undefined,
    kinds: kinds.data ?? [],
    settings: settings.data,
    library,
    libraryBusy,
    libraryError,
    reloadProviders: providers.reload,
    reloadSettings: settings.reload,
    refreshLibrary,
    error: providers.error ?? kinds.error ?? settings.error,
  };
}
