import { aiClients, integrationActionNotice, type AIClient, type AIIntegrationStatus, type IntegrationLocations } from "./ai-integration-presentation.ts";

type Paths = Required<IntegrationLocations>;
type Action = "configure" | "remove";
type ClientContext = {
  status: AIIntegrationStatus;
  manual: boolean;
  draft: Paths;
  checked: Paths | null;
  error: string;
  notice: string;
};
type Confirmation = { client: AIClient; action: Action; locations: Paths };
type SessionSnapshot = {
  client: AIClient;
  contexts: Record<AIClient, ClientContext>;
  pending: Confirmation | null;
  busy: boolean;
};
type Runtime = {
  desktop: boolean;
  readStatus: (client: AIClient, locations?: IntegrationLocations) => Promise<AIIntegrationStatus>;
  mutate: (action: Action, client: AIClient, locations: Paths) => Promise<AIIntegrationStatus>;
  onStatusChange: (status: AIIntegrationStatus) => void;
};

const pathsOf = (status: AIIntegrationStatus): Paths => ({ configPath: status.configPath, skillPath: status.skillPath });

export function createAIIntegrationSession(initial: AIIntegrationStatus[], runtime: Runtime) {
  let snapshot: SessionSnapshot = {
    client: "codex", busy: false, pending: null,
    contexts: Object.fromEntries(initial.map((status) => [status.client, {
      status, manual: false, draft: pathsOf(status), checked: null, error: "", notice: "",
    }])) as Record<AIClient, ClientContext>,
  };
  const listeners = new Set<() => void>();
  let initialization: Promise<void> | undefined;
  const publish = (patch: Partial<SessionSnapshot>) => {
    snapshot = { ...snapshot, ...patch };
    listeners.forEach((listener) => listener());
  };
  const update = (client: AIClient, patch: Partial<ClientContext>) => {
    publish({ contexts: { ...snapshot.contexts, [client]: { ...snapshot.contexts[client], ...patch } } });
  };
  const accept = (status: AIIntegrationStatus) => {
    update(status.client, { status, checked: pathsOf(status), draft: pathsOf(status), error: "" });
    runtime.onStatusChange(status);
  };
  const load = async (client: AIClient, locations?: IntegrationLocations) => {
    update(client, { checked: null, error: "", notice: "" });
    try { accept(await runtime.readStatus(client, locations)); }
    catch (reason) { update(client, { error: String(reason) }); }
  };
  const refresh = async () => {
    if (snapshot.busy) return;
    const client = snapshot.client;
    const context = snapshot.contexts[client];
    const locations = context.manual ? {
      configPath: context.draft.configPath.trim(), skillPath: context.draft.skillPath.trim(),
    } : undefined;
    publish({ busy: true, pending: null });
    if (locations && (!locations.configPath || !locations.skillPath)) {
      update(client, { checked: null, error: "请填写完整的配置文件与 Skill 目录路径。", notice: "" });
    } else { await load(client, locations); }
    publish({ busy: false });
  };
  const canAct = (action: Action) => {
    const context = snapshot.contexts[snapshot.client];
    const { status, checked, manual, draft } = context;
    return runtime.desktop && !snapshot.busy && !context.error && !!checked
      && (!manual || (draft.configPath === checked.configPath && draft.skillPath === checked.skillPath))
      && (action === "configure" ? !status.configured && status.canConfigure : status.canRemove);
  };

  return {
    getSnapshot: () => snapshot,
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    initialize() {
      if (!initialization) {
        publish({ busy: true, pending: null });
        initialization = Promise.all(aiClients.map((client) => load(client))).then(() => { publish({ busy: false }); });
      }
      return initialization;
    },
    select(client: AIClient) {
      if (!snapshot.busy) publish({ client, pending: null });
    },
    editDraft(field: keyof Paths, value: string) {
      if (snapshot.busy) return;
      const client = snapshot.client;
      publish({ pending: null });
      update(client, { draft: { ...snapshot.contexts[client].draft, [field]: value } });
    },
    async toggleManual() {
      if (snapshot.busy) return;
      const client = snapshot.client;
      const context = snapshot.contexts[client];
      publish({ pending: null });
      update(client, { manual: !context.manual, draft: pathsOf(context.status), checked: null, error: "", notice: "" });
      if (context.manual) await refresh();
    },
    refresh,
    canAct,
    confirm(action: Action) {
      if (canAct(action)) publish({ pending: {
        client: snapshot.client, action, locations: { ...snapshot.contexts[snapshot.client].checked! },
      } });
    },
    cancel() { if (!snapshot.busy) publish({ pending: null }); },
    async run() {
      const pending = snapshot.pending;
      if (!pending || pending.client !== snapshot.client || !canAct(pending.action)) return;
      publish({ busy: true });
      update(pending.client, { error: "", notice: "" });
      try {
        // Always send the displayed, confirmed paths; never resolve defaults again during a write.
        const next = await runtime.mutate(pending.action, pending.client, { ...pending.locations });
        accept(next);
        update(pending.client, { notice: integrationActionNotice(next) });
      } catch (reason) { update(pending.client, { checked: null, error: String(reason) }); }
      publish({ busy: false, pending: null });
    },
  };
}
