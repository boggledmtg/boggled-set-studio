export class ApiError extends Error {
  status: number;

  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

export type FieldRect = [number, number, number, number];
export type NativeLayout = Record<"name" | "casting_cost" | "type" | "text" | "pt", FieldRect> & { flavorTop: number };

export type CardSummary = {
  folder: string;
  uuid: string;
  revision: string;
  name: string;
  manaCost: string;
  manaValue: number;
  type: string;
  power: string;
  toughness: string;
  rarity: string;
  number: string;
  colors: string[];
  layout: string;
  supported: boolean;
};

export type CardDocument = Record<string, unknown> & {
  uuid?: string;
  name?: string;
  layout?: string;
  manaCost?: string;
  manaValue?: number;
  type?: string;
  types?: string[];
  text?: string;
  flavorText?: string;
  power?: string;
  toughness?: string;
  rarity?: string;
  number?: string;
  artist?: string;
  colors?: string[];
  boggled?: { designNotes?: string; artFile?: string; renderFile?: string; stylesheet?: string };
};

export type SetDocument = Record<string, unknown> & {
  name?: string;
  code?: string;
  boggled?: { designBrief?: string };
};

export type Draft = {
  name: string;
  manaCost: string;
  type: string;
  text: string;
  flavorText: string;
  power: string;
  toughness: string;
  rarity: string;
  number: string;
  artist: string;
  designNotes: string;
};

export type SetDraft = {
  name: string;
  code: string;
  designBrief: string;
};

export function isSupported(card: CardDocument): boolean {
  const layout = card.layout ?? "normal";
  const style = card.boggled?.stylesheet ?? "magic-m15-altered.mse-style";
  return layout === "normal" && (card.types ?? []).includes("Creature")
    && (style === "magic-m15-altered.mse-style" || style === "m15-altered");
}

export function draftFromCard(card: CardDocument): Draft {
  return {
    name: card.name ?? "",
    manaCost: card.manaCost ?? "",
    type: card.type ?? "",
    text: card.text ?? "",
    flavorText: card.flavorText ?? "",
    power: card.power ?? "",
    toughness: card.toughness ?? "",
    rarity: card.rarity ?? "common",
    number: card.number ?? "",
    artist: card.artist ?? "",
    designNotes: card.boggled?.designNotes ?? "",
  };
}

export function draftFromSet(set: SetDocument): SetDraft {
  return {
    name: set.name ?? "",
    code: set.code ?? "",
    designBrief: set.boggled?.designBrief ?? "",
  };
}

export function sameDraft<T extends Record<string, string>>(left: T, right: T): boolean {
  return Object.keys(left).every((key) => left[key] === right[key]);
}

function joinUrl(base: string, path: string): string {
  if (!base) return path;
  return `${base.replace(/\/$/, "")}${path}`;
}

async function fail(response: Response): Promise<never> {
  const text = await response.text();
  let message = text || response.statusText;
  try {
    const body = JSON.parse(text) as { error?: string };
    if (body.error) message = body.error;
  } catch {
    /* the body was not JSON */
  }
  throw new ApiError(response.status, message);
}

export class Client {
  constructor(
    private token: string,
    private base: string,
  ) {}

  private headers(set?: string, revision?: string): Headers {
    const headers = new Headers();
    if (this.token) headers.set("Authorization", `Bearer ${this.token}`);
    if (set) headers.set("X-Boggled-Set", set);
    if (revision) headers.set("If-Match", revision);
    return headers;
  }

  private async send(path: string, init: RequestInit, set?: string, revision?: string): Promise<Response> {
    const headers = this.headers(set, revision);
    if (init.body) headers.set("Content-Type", "application/json");
    const response = await fetch(joinUrl(this.base, path), { ...init, headers });
    if (!response.ok) await fail(response);
    return response;
  }

  private async json<T>(path: string, init: RequestInit, set?: string, revision?: string): Promise<T> {
    const response = await this.send(path, init, set, revision);
    return (await response.json()) as T;
  }

  async open(path: string): Promise<string> {
    const body = await this.json<{ path: string }>("/api/sets/open", {
      method: "POST",
      body: JSON.stringify({ path }),
    });
    return body.path;
  }

  async init(path: string, name: string, code: string): Promise<string> {
    const body = await this.json<{ path: string }>("/api/sets/init", {
      method: "POST",
      body: JSON.stringify({ path, name, code }),
    });
    return body.path;
  }

  async close(path: string): Promise<void> {
    await this.json("/api/sets/close", { method: "POST", body: JSON.stringify({ path }) });
  }

  async importSet(source: string, dest: string): Promise<{ setPath: string; cards: number }> {
    return this.json("/api/import", {
      method: "POST",
      body: JSON.stringify({ source, dest }),
    });
  }

  async getSet(set: string): Promise<{ document: SetDocument; revision: string }> {
    const response = await this.send("/api/set", { method: "GET" }, set);
    return {
      document: (await response.json()) as SetDocument,
      revision: response.headers.get("x-revision") ?? "",
    };
  }

  async putSet(
    set: string,
    patch: { name: string; code: string; boggled: { designBrief: string } },
    revision: string,
  ): Promise<{ document: SetDocument; revision: string }> {
    const response = await this.send("/api/set", { method: "PUT", body: JSON.stringify(patch) }, set, revision);
    return {
      document: (await response.json()) as SetDocument,
      revision: response.headers.get("x-revision") ?? "",
    };
  }

  async listCards(set: string): Promise<CardSummary[]> {
    const body = await this.json<{ cards: CardSummary[] }>("/api/cards", { method: "GET" }, set);
    return body.cards;
  }

  async createCard(set: string): Promise<CardDocument> {
    const response = await this.send("/api/cards", { method: "POST", body: "{}" }, set);
    return (await response.json()) as CardDocument;
  }

  async getCard(set: string, uuid: string): Promise<{ card: CardDocument; revision: string }> {
    const response = await this.send(`/api/cards/${uuid}`, { method: "GET" }, set);
    return {
      card: (await response.json()) as CardDocument,
      revision: response.headers.get("x-revision") ?? "",
    };
  }

  async putCard(
    set: string,
    uuid: string,
    draft: Draft,
    revision: string,
  ): Promise<{ card: CardDocument; revision: string }> {
    const response = await this.send(
      `/api/cards/${uuid}`,
      {
        method: "PUT",
        body: JSON.stringify({
          name: draft.name,
          manaCost: draft.manaCost,
          type: draft.type,
          text: draft.text,
          flavorText: draft.flavorText,
          power: draft.power,
          toughness: draft.toughness,
          rarity: draft.rarity,
          number: draft.number,
          artist: draft.artist,
          boggled: { designNotes: draft.designNotes },
        }),
      },
      set,
      revision,
    );
    return {
      card: (await response.json()) as CardDocument,
      revision: response.headers.get("x-revision") ?? "",
    };
  }

  async deleteCard(set: string, uuid: string): Promise<void> {
    await this.json(`/api/cards/${uuid}`, { method: "DELETE" }, set);
  }

  async setArt(set: string, uuid: string, path: string, revision: string): Promise<{ card: CardDocument; revision: string }> {
    const response = await this.send(
      `/api/cards/${uuid}/art`,
      { method: "POST", body: JSON.stringify({ path }) },
      set,
      revision,
    );
    return {
      card: (await response.json()) as CardDocument,
      revision: response.headers.get("x-revision") ?? "",
    };
  }

  async picture(set: string, uuid: string) {
    const response = await this.send(`/api/cards/${uuid}/picture`, { method: "GET" }, set);
    const raw = response.headers.get("x-boggled-layout");
    return { image: await response.blob(), layout: raw ? JSON.parse(raw) as NativeLayout : null,
      revision: response.headers.get("x-revision") };
  }

  async exportSet(set: string, path: string): Promise<{ path: string; format: string }> {
    const body: { format: string; path?: string } = { format: "mtgjson" };
    if (path.trim()) body.path = path.trim();
    return this.json("/api/export", { method: "POST", body: JSON.stringify(body) }, set);
  }

  async undo(set: string): Promise<void> {
    await this.json("/api/history/undo", { method: "POST" }, set);
  }

  async redo(set: string): Promise<void> {
    await this.json("/api/history/redo", { method: "POST" }, set);
  }
}

export async function readEvents(
  clientBase: string,
  token: string,
  set: string,
  signal: AbortSignal,
  onEvent: () => void,
): Promise<void> {
  const headers = new Headers();
  if (token) headers.set("Authorization", `Bearer ${token}`);
  headers.set("X-Boggled-Set", set);
  const response = await fetch(joinUrl(clientBase, "/events"), { headers, signal });
  if (!response.ok || !response.body) return;
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  while (!signal.aborted) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    let split = buffer.indexOf("\n\n");
    while (split >= 0) {
      const raw = buffer.slice(0, split);
      buffer = buffer.slice(split + 2);
      let name = "";
      for (const line of raw.split("\n")) {
        if (line.startsWith("event:")) name = line.slice(6).trim();
      }
      if (name) onEvent();
      split = buffer.indexOf("\n\n");
    }
  }
}

export function socketUrl(base: string): string {
  if (!base) {
    const proto = location.protocol === "https:" ? "wss:" : "ws:";
    return `${proto}//${location.host}/terminal`;
  }
  return `${base.replace(/^http/, "ws").replace(/\/$/, "")}/terminal`;
}
