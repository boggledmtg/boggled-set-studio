import { useEffect, useRef, useState, type ReactNode, type CSSProperties } from "react";
import {
  ApiError,
  Client,
  draftFromCard,
  draftFromSet,
  isSupported,
  readEvents,
  sameDraft,
  type CardDocument,
  type CardSummary,
  type Draft,
  type FieldRect,
  type NativeLayout,
  type SetDraft,
} from "./api";
import { TerminalPane } from "./terminal";
import { CARD_TYPES, SUBTYPES, SUPERTYPES } from "./type-words";

type MenuName = "file" | "edit" | "cards" | "help";
type DialogName = "new" | "open" | "import" | "export" | "art" | "help";
type Tone = "info" | "error";

const EMPTY_DRAFT: Draft = {
  name: "",
  manaCost: "",
  type: "",
  text: "",
  flavorText: "",
  power: "",
  toughness: "",
  rarity: "common",
  number: "",
  artist: "",
  designNotes: "",
};

const EMPTY_SET: SetDraft = { name: "", code: "", designBrief: "" };

function messageOf(err: unknown): string {
  if (err instanceof Error) return err.message;
  return "The request failed";
}

async function windowControl(action: "minimize" | "toggleMaximize" | "close") {
  if (!("__TAURI_INTERNALS__" in window)) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  const current = getCurrentWindow();
  if (action === "minimize") await current.minimize();
  else if (action === "toggleMaximize") await current.toggleMaximize();
  else await current.close();
}

async function nativePick(command: "pick_folder" | "pick_file" | "save_file"): Promise<string | null> {
  if (!("__TAURI_INTERNALS__" in window)) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  const args = command === "save_file" ? { suggested: "set.json" } : {};
  return invoke<string | null>(command, args);
}

export function App() {
  const shell = new URLSearchParams(location.search).get("shell") === "1";
  const [token, setToken] = useState(() => window.__BOGGLED__?.token ?? "");
  const [base] = useState(() => window.__BOGGLED__?.url ?? "");
  const [tokenInput, setTokenInput] = useState("");
  const client = useRef(new Client(token, base));
  client.current = new Client(token, base);

  const [setPath, setSetPath] = useState<string | null>(null);
  const [setFields, setSetFields] = useState<SetDraft>(EMPTY_SET);
  const [setSavedFields, setSetSavedFields] = useState<SetDraft>(EMPTY_SET);
  const [cards, setCards] = useState<CardSummary[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [draft, setCardDraft] = useState<Draft>(EMPTY_DRAFT);
  const [saved, setCardSaved] = useState<Draft>(EMPTY_DRAFT);
  const [supported, setSupported] = useState(false);
  const [tab, setTab] = useState<"cards" | "set">("cards");
  const [search, setSearch] = useState("");
  const [menu, setMenu] = useState<MenuName | null>(null);
  const [dialog, setDialog] = useState<DialogName | null>(null);
  const [notice, setNotice] = useState("");
  const [tone, setTone] = useState<Tone>("info");
  const [picture, setPicture] = useState<{ url: string; layout: NativeLayout | null } | null>(null);
  const pictureRequest = useRef(0);
  useEffect(() => () => { if (picture) URL.revokeObjectURL(picture.url); }, [picture]);

  const [openPath, setOpenPath] = useState("");
  const [newPath, setNewPath] = useState("");
  const [newName, setNewName] = useState("Untitled Set");
  const [newCode, setNewCode] = useState("SET");
  const [importSource, setImportSource] = useState("");
  const [importDest, setImportDest] = useState("");
  const [exportPath, setExportPath] = useState("");
  const [artPath, setArtPath] = useState("");

  const setPathRef = useRef(setPath);
  const draftRef = useRef(draft);
  const savedRef = useRef(saved);
  const setFieldsRef = useRef(setFields);
  const setSavedFieldsRef = useRef(setSavedFields);
  const selectedRef = useRef(selected);
  const supportedRef = useRef(supported);
  const revisionRef = useRef("");
  const setRevisionRef = useRef("");
  const epoch = useRef(0);
  const loadId = useRef(0);
  const cardTimer = useRef(0);
  const setTimer = useRef(0);
  const cardChain = useRef(Promise.resolve());
  const setChain = useRef(Promise.resolve());
  const menuRef = useRef<HTMLElement>(null);
  setPathRef.current = setPath;
  draftRef.current = draft;
  savedRef.current = saved;
  setFieldsRef.current = setFields;
  setSavedFieldsRef.current = setSavedFields;
  selectedRef.current = selected;
  supportedRef.current = supported;

  function show(text: string, next: Tone = "info") {
    setNotice(text);
    setTone(next);
  }

  function applySet(next: SetDraft, revision: string) {
    setFieldsRef.current = next;
    setSavedFieldsRef.current = next;
    setRevisionRef.current = revision;
    setSetFields(next);
    setSetSavedFields(next);
  }

  async function reloadCard(uuid: string) {
    const path = setPathRef.current;
    if (!path) return;
    const id = ++loadId.current;
    const result = await client.current.getCard(path, uuid);
    if (id !== loadId.current || selectedRef.current !== uuid) return;
    const next = draftFromCard(result.card);
    const ok = isSupported(result.card);
    draftRef.current = next;
    savedRef.current = next;
    revisionRef.current = result.revision;
    supportedRef.current = ok;
    setCardDraft(next);
    setCardSaved(next);
    setSupported(ok);
    if (ok) await refreshPicture(uuid);
    else setPicture(null);
  }

  async function refreshPicture(uuid: string) {
    const path = setPathRef.current;
    if (!path) return;
    const id = ++pictureRequest.current;
    const current = () => id === pictureRequest.current && path === setPathRef.current && uuid === selectedRef.current;
    try {
      const result = await client.current.picture(path, uuid);
      if (!current() || (result.revision && result.revision !== revisionRef.current)) return;
      setPicture({ url: URL.createObjectURL(result.image), layout: result.layout });
    } catch (err) {
      if (!current()) return;
      setPicture(null);
      show(messageOf(err), "error");
    }
  }

  function patchSummary(uuid: string, card: CardDocument, revision: string) {
    setCards((list) =>
      list.map((item) =>
        item.uuid === uuid
          ? {
              ...item,
              revision,
              name: card.name ?? item.name,
              manaCost: card.manaCost ?? "",
              manaValue: card.manaValue ?? 0,
              type: card.type ?? "",
              power: card.power ?? "",
              toughness: card.toughness ?? "",
              rarity: card.rarity ?? "",
              number: card.number ?? "",
              colors: card.colors ?? [],
              supported: isSupported(card),
            }
          : item,
      ),
    );
  }

  async function commitCard() {
    const path = setPathRef.current;
    const uuid = selectedRef.current;
    if (!path || !uuid || !supportedRef.current) return;
    const sent = { ...draftRef.current };
    if (sameDraft(sent, savedRef.current)) return;
    const gen = epoch.current;
    try {
      const result = await client.current.putCard(path, uuid, sent, revisionRef.current);
      if (gen !== epoch.current || selectedRef.current !== uuid) return;
      revisionRef.current = result.revision;
      const canonical = draftFromCard(result.card);
      if (sameDraft(draftRef.current, sent)) {
        draftRef.current = canonical;
        savedRef.current = canonical;
        setCardDraft(canonical);
        setCardSaved(canonical);
      } else {
        savedRef.current = sent;
        setCardSaved(sent);
        scheduleCard();
      }
      patchSummary(uuid, result.card, result.revision);
      if (isSupported(result.card)) await refreshPicture(uuid);
    } catch (err) {
      if (gen !== epoch.current) return;
      if (err instanceof ApiError && err.status === 409) {
        await reloadCard(uuid);
        show("The card changed on disk. Reloaded.", "error");
      } else {
        show(messageOf(err), "error");
      }
    }
  }

  async function commitSet() {
    const path = setPathRef.current;
    if (!path) return;
    const sent = { ...setFieldsRef.current };
    if (sameDraft(sent, setSavedFieldsRef.current)) return;
    const gen = epoch.current;
    try {
      const result = await client.current.putSet(
        path,
        { name: sent.name, code: sent.code, boggled: { designBrief: sent.designBrief } },
        setRevisionRef.current,
      );
      if (gen !== epoch.current || setPathRef.current !== path) return;
      const canonical = draftFromSet(result.document);
      setRevisionRef.current = result.revision;
      if (sameDraft(setFieldsRef.current, sent)) {
        applySet(canonical, result.revision);
      } else {
        setSavedFieldsRef.current = sent;
        setSetSavedFields(sent);
        scheduleSet();
      }
    } catch (err) {
      if (gen !== epoch.current) return;
      show(messageOf(err), "error");
    }
  }

  const commitCardRef = useRef(commitCard);
  const commitSetRef = useRef(commitSet);
  commitCardRef.current = commitCard;
  commitSetRef.current = commitSet;

  function flushCard() {
    window.clearTimeout(cardTimer.current);
    const run = cardChain.current.then(() => commitCardRef.current());
    cardChain.current = run.then(
      () => undefined,
      () => undefined,
    );
    return run;
  }

  function flushSet() {
    window.clearTimeout(setTimer.current);
    const run = setChain.current.then(() => commitSetRef.current());
    setChain.current = run.then(
      () => undefined,
      () => undefined,
    );
    return run;
  }

  function scheduleCard() {
    window.clearTimeout(cardTimer.current);
    cardTimer.current = window.setTimeout(() => {
      void flushCard();
    }, 400);
  }

  function scheduleSet() {
    window.clearTimeout(setTimer.current);
    setTimer.current = window.setTimeout(() => {
      void flushSet();
    }, 400);
  }

  function updateDraft(key: keyof Draft, value: string, immediate = false) {
    const next = { ...draftRef.current, [key]: value };
    draftRef.current = next;
    setCardDraft(next);
    if (immediate) void flushCard();
    else scheduleCard();
  }

  function updateSet(key: keyof SetDraft, value: string) {
    const next = { ...setFieldsRef.current, [key]: value };
    setFieldsRef.current = next;
    setSetFields(next);
    scheduleSet();
  }

  async function loadWorkspace(path: string) {
    const [setResult, list] = await Promise.all([client.current.getSet(path), client.current.listCards(path)]);
    setPathRef.current = path;
    setSetPath(path);
    applySet(draftFromSet(setResult.document), setResult.revision);
    setCards(list);
    selectedRef.current = null;
    setSelected(null);
    setSupported(false);
    setPicture(null);
    setTab("cards");
    setSearch("");
    setDialog(null);
    setMenu(null);
    setNotice("");
  }

  async function refreshAfterHistory() {
    const path = setPathRef.current;
    if (!path) return;
    try {
      const [setResult, list] = await Promise.all([client.current.getSet(path), client.current.listCards(path)]);
      applySet(draftFromSet(setResult.document), setResult.revision);
      setCards(list);
      const uuid = selectedRef.current;
      if (uuid && list.some((card) => card.uuid === uuid)) await reloadCard(uuid);
      else {
        selectedRef.current = null;
        setSelected(null);
        setPicture(null);
      }
    } catch {
      setPathRef.current = null;
      setSetPath(null);
      setCards([]);
      selectedRef.current = null;
      setSelected(null);
      setPicture(null);
    }
  }

  async function history(kind: "undo" | "redo") {
    const path = setPathRef.current;
    if (!path) return;
    epoch.current += 1;
    window.clearTimeout(cardTimer.current);
    window.clearTimeout(setTimer.current);
    try {
      if (kind === "undo") await client.current.undo(path);
      else await client.current.redo(path);
      await refreshAfterHistory();
    } catch (err) {
      show(messageOf(err), "error");
    }
  }

  const historyRef = useRef(history);
  historyRef.current = history;

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const ctrl = event.ctrlKey || event.metaKey;
      if (!ctrl) return;
      const key = event.key.toLowerCase();
      const undo = key === "z" && !event.shiftKey;
      const redo = key === "y" || (key === "z" && event.shiftKey);
      if (!undo && !redo) return;
      const target = event.target;
      if (target instanceof Element && target.closest(".terminal")) return;
      if (
        target instanceof HTMLInputElement ||
        target instanceof HTMLTextAreaElement ||
        target instanceof HTMLSelectElement
      ) {
        if (target.dataset.saved !== undefined && target.value !== target.dataset.saved) return;
      }
      event.preventDefault();
      void historyRef.current(undo ? "undo" : "redo");
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    if (!menu) return;
    const onDown = (event: MouseEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setMenu(null);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [menu]);

  useEffect(() => {
    if (!setPath || !token) return;
    const ctrl = new AbortController();
    let timer = 0;
    const kick = () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        const path = setPathRef.current;
        if (!path) return;
        void client.current
          .listCards(path)
          .then((list) => {
            setCards(list);
            const uuid = selectedRef.current;
            if (uuid && sameDraft(draftRef.current, savedRef.current)) return reloadCard(uuid);
            return undefined;
          })
          .then(() => {
            if (!sameDraft(setFieldsRef.current, setSavedFieldsRef.current)) return undefined;
            return client.current.getSet(path).then((result) => {
              if (setPathRef.current !== path) return;
              if (!sameDraft(setFieldsRef.current, setSavedFieldsRef.current)) return;
              applySet(draftFromSet(result.document), result.revision);
            });
          })
          .catch(() => undefined);
      }, 80);
    };
    void readEvents(base, token, setPath, ctrl.signal, kick).catch(() => undefined);
    return () => {
      ctrl.abort();
      window.clearTimeout(timer);
      window.clearTimeout(cardTimer.current);
      window.clearTimeout(setTimer.current);
    };
  }, [setPath, token, base]);

  useEffect(() => {
    document.title = setSavedFields.name ? `${setSavedFields.name} — Boggled Set Studio` : "Boggled Set Studio";
  }, [setSavedFields.name]);

  async function chooseCard(uuid: string) {
    if (uuid === selectedRef.current) return;
    await flushCard();
    const summary = cards.find((card) => card.uuid === uuid);
    setPicture(null);
    selectedRef.current = uuid;
    setSelected(uuid);
    if (summary && !summary.supported) {
      supportedRef.current = false;
      setSupported(false);
    }
    try {
      await reloadCard(uuid);
    } catch (err) {
      show(messageOf(err), "error");
    }
  }

  async function createCard() {
    const path = setPathRef.current;
    if (!path) return;
    await flushCard();
    const card = await client.current.createCard(path);
    const list = await client.current.listCards(path);
    setCards(list);
    const uuid = String(card.uuid ?? "");
    selectedRef.current = uuid;
    setSelected(uuid);
    await reloadCard(uuid);
    setTab("cards");
  }

  async function deleteCard() {
    const path = setPathRef.current;
    const uuid = selectedRef.current;
    if (!path || !uuid) return;
    await client.current.deleteCard(path, uuid);
    selectedRef.current = null;
    setSelected(null);
    setPicture(null);
    setCards(await client.current.listCards(path));
  }

  async function useArt(imagePath: string) {
    const path = setPathRef.current;
    const uuid = selectedRef.current;
    if (!path || !uuid) return;
    await flushCard();
    const result = await client.current.setArt(path, uuid, imagePath, revisionRef.current);
    revisionRef.current = result.revision;
    patchSummary(uuid, result.card, result.revision);
    if (isSupported(result.card)) await refreshPicture(uuid);
    show("Illustration set");
    setDialog(null);
  }

  if (!token) {
    return (
      <form
        className="gate"
        onSubmit={(event) => {
          event.preventDefault();
          const next = tokenInput.trim();
          if (next) setToken(next);
        }}
      >
        <h1>Boggled Set Studio</h1>
        <label>
          Server token
          <input aria-label="Server token" value={tokenInput} onChange={(event) => setTokenInput(event.target.value)} />
        </label>
        <button type="submit">Connect</button>
      </form>
    );
  }

  const query = search.trim().toLowerCase();
  const shown = query
    ? cards.filter((card) =>
        [card.name, card.type, card.manaCost, card.number, card.rarity].join(" ").toLowerCase().includes(query),
      )
    : cards;
  const selectedCount = selected && cards.some((card) => card.uuid === selected) ? 1 : 0;

  function openMenuAction(name: DialogName, focusId?: string) {
    setMenu(null);
    if (!setPath && focusId) {
      document.getElementById(focusId)?.focus();
      return;
    }
    setDialog(name);
  }

  return (
    <div className="app">
      {shell && (
        <header className="titlebar" data-tauri-drag-region>
          <img src="/bogle-icon.png" alt="Bogle" />
          <strong>{setSavedFields.name || "Boggled Set Studio"}</strong>
          <span>Boggled Set Studio</span>
          <div className="window-controls">
            <button type="button" onClick={() => void windowControl("minimize")}>
              Minimize
            </button>
            <button type="button" onClick={() => void windowControl("toggleMaximize")}>
              Maximize
            </button>
            <button type="button" className="close" onClick={() => void windowControl("close")}>
              Close
            </button>
          </div>
        </header>
      )}
      <nav className="menubar" aria-label="Menu" ref={menuRef}>
        <Menu label="File" open={menu === "file"} onToggle={() => setMenu(menu === "file" ? null : "file")}>
          <MenuItem label="New set" onClick={() => openMenuAction("new", "new-set-folder")} />
          <MenuItem label="Open set" onClick={() => openMenuAction("open", "set-folder")} />
          <MenuItem label="Import MSE set" onClick={() => openMenuAction("import", "mse-package")} />
          <MenuItem
            label="Export MTGJSON"
            disabled={!setPath}
            onClick={() => {
              setMenu(null);
              setDialog("export");
            }}
          />
          <MenuItem
            label="Close set"
            disabled={!setPath}
            onClick={() => {
              setMenu(null);
              const path = setPathRef.current;
              if (!path) return;
              void client.current.close(path).then(() => {
                setPathRef.current = null;
                setSetPath(null);
                setCards([]);
                selectedRef.current = null;
                setSelected(null);
                setPicture(null);
              });
            }}
          />
        </Menu>
        <Menu label="Edit" open={menu === "edit"} onToggle={() => setMenu(menu === "edit" ? null : "edit")}>
          <MenuItem
            label="Undo"
            disabled={!setPath}
            onClick={() => {
              setMenu(null);
              void history("undo");
            }}
          />
          <MenuItem
            label="Redo"
            disabled={!setPath}
            onClick={() => {
              setMenu(null);
              void history("redo");
            }}
          />
        </Menu>
        <Menu label="Cards" open={menu === "cards"} onToggle={() => setMenu(menu === "cards" ? null : "cards")}>
          <MenuItem
            label="New card"
            disabled={!setPath}
            onClick={() => {
              setMenu(null);
              void createCard().catch((err) => show(messageOf(err), "error"));
            }}
          />
          <MenuItem
            label="Delete card"
            disabled={!selected}
            onClick={() => {
              setMenu(null);
              void deleteCard().catch((err) => show(messageOf(err), "error"));
            }}
          />
          <MenuItem
            label="Set illustration"
            disabled={!selected}
            onClick={() => {
              setMenu(null);
              setDialog("art");
            }}
          />
        </Menu>
        <Menu label="Help" open={menu === "help"} onToggle={() => setMenu(menu === "help" ? null : "help")}>
          <MenuItem
            label="About"
            onClick={() => {
              setMenu(null);
              setDialog("help");
            }}
          />
        </Menu>
      </nav>
      {setPath && (
        <div className="toolbar">
          <h1>{setSavedFields.name}</h1>
          <button type="button" onClick={() => void createCard().catch((err) => show(messageOf(err), "error"))}>
            New card
          </button>
          <button
            type="button"
            disabled={!selected}
            onClick={() => void deleteCard().catch((err) => show(messageOf(err), "error"))}
          >
            Delete card
          </button>
          <button type="button" disabled={!selected} onClick={() => setDialog("art")}>
            Set illustration
          </button>
          <button type="button" onClick={() => setDialog("export")}>
            Export
          </button>
          <label className="search">
            Search
            <input aria-label="Search" value={search} onChange={(event) => setSearch(event.target.value)} />
          </label>
          <span className="count">
            {selectedCount} selected
          </span>
          {notice && (
            <span className={tone === "error" ? "notice error" : "notice"} role={tone === "error" ? "alert" : "status"}>
              {notice}
            </span>
          )}
        </div>
      )}
      {setPath && (
        <div className="tabs" role="tablist" aria-label="Editor">
          <button type="button" role="tab" aria-selected={tab === "cards"} onClick={() => setTab("cards")}>
            Cards
          </button>
          <button type="button" role="tab" aria-selected={tab === "set"} onClick={() => setTab("set")}>
            Set info
          </button>
        </div>
      )}
      {!setPath ? (
        <main className="start">
          <h1>Boggled Set Studio</h1>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              const path = fieldValue(event.currentTarget, "path");
              setOpenPath(path);
              void client.current
                .open(path)
                .then(loadWorkspace)
                .catch((err) => show(messageOf(err), "error"));
            }}
          >
            <h2>Open</h2>
            <Field label="Set folder" name="path" value={openPath} onChange={setOpenPath} id="set-folder" pick="folder" />
            <button type="submit">Open</button>
          </form>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              const path = fieldValue(event.currentTarget, "path");
              const name = fieldValue(event.currentTarget, "name");
              const code = fieldValue(event.currentTarget, "code");
              setNewPath(path);
              setNewName(name);
              setNewCode(code);
              void client.current
                .init(path, name, code)
                .then(loadWorkspace)
                .catch((err) => show(messageOf(err), "error"));
            }}
          >
            <h2>New set</h2>
            <Field label="New set folder" name="path" value={newPath} onChange={setNewPath} id="new-set-folder" pick="folder" />
            <Field label="Set name" name="name" value={newName} onChange={setNewName} />
            <Field label="Set code" name="code" value={newCode} onChange={setNewCode} />
            <button type="submit">Create set</button>
          </form>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              const source = fieldValue(event.currentTarget, "source");
              const dest = fieldValue(event.currentTarget, "dest");
              setImportSource(source);
              setImportDest(dest);
              void client.current
                .importSet(source, dest)
                .then((result) => loadWorkspace(result.setPath))
                .catch((err) => show(messageOf(err), "error"));
            }}
          >
            <h2>Import</h2>
            <Field label="MSE package" name="source" value={importSource} onChange={setImportSource} id="mse-package" pick="file" />
            <Field label="Import destination" name="dest" value={importDest} onChange={setImportDest} pick="folder" />
            <button type="submit">Import</button>
          </form>
          {notice && (
            <p className={tone === "error" ? "notice error" : "notice"} role={tone === "error" ? "alert" : "status"}>
              {notice}
            </p>
          )}
        </main>
      ) : (
        <div className="workspace">
          {tab === "cards" ? (
            <>
              <section className="preview-col" aria-label="Card">
                <div className="card-stage">
                  {!selected && <p className="empty-note">No card selected</p>}
                  {selected && !supported && <p className="unsupported">This frame is unsupported</p>}
                  {selected && supported && (
                    <div className={picture?.layout ? "card-face native-layout" : "card-face"}>
                      {picture && <img alt={`Rendered ${saved.name}`} src={picture.url} />}
                      <CardBoxes
                        layout={picture?.layout}
                        draft={draft}
                        saved={saved}
                        onChange={(key, value) => updateDraft(key, value)}
                        onBlur={() => void flushCard()}
                      />
                    </div>
                  )}
                </div>
                {selected && !supported && (
                  <TextField
                    label="Name"
                    value={draft.name}
                    saved={saved.name}
                    disabled
                    wide
                    onChange={(value) => updateDraft("name", value)}
                    onBlur={() => void flushCard()}
                  />
                )}
                {selected && (
                  <div className="fields">
                    <label className="field">
                      <span>Rarity</span>
                      <select
                        aria-label="Rarity"
                        value={draft.rarity}
                        data-saved={saved.rarity}
                        disabled={!supported}
                        onChange={(event) => updateDraft("rarity", event.target.value, true)}
                      >
                        <option value="common">common</option>
                        <option value="uncommon">uncommon</option>
                        <option value="rare">rare</option>
                        <option value="mythic">mythic</option>
                      </select>
                    </label>
                    <TextField
                      label="Collector number"
                      value={draft.number}
                      saved={saved.number}
                      disabled={!supported}
                      onChange={(value) => updateDraft("number", value)}
                      onBlur={() => void flushCard()}
                    />
                    <TextField
                      label="Artist"
                      value={draft.artist}
                      saved={saved.artist}
                      disabled={!supported}
                      wide
                      onChange={(value) => updateDraft("artist", value)}
                      onBlur={() => void flushCard()}
                    />
                    <TextField
                      label="Notes"
                      value={draft.designNotes}
                      saved={saved.designNotes}
                      disabled={!supported}
                      wide
                      multiline
                      onChange={(value) => updateDraft("designNotes", value)}
                      onBlur={() => void flushCard()}
                    />
                  </div>
                )}
              </section>
              <section className="grid-col" aria-label="Cards">
                <table>
                  <thead>
                    <tr>
                      <th>Name</th>
                      <th>Cost</th>
                      <th>MV</th>
                      <th>Type</th>
                      <th>P/T</th>
                      <th>Rarity</th>
                      <th>Number</th>
                      <th>Color</th>
                    </tr>
                  </thead>
                  <tbody>
                    {shown.map((card) => (
                      <tr
                        key={card.uuid}
                        aria-selected={card.uuid === selected}
                        onClick={() => void chooseCard(card.uuid)}
                      >
                        <td>{card.name}</td>
                        <td>{card.manaCost}</td>
                        <td>{card.manaValue}</td>
                        <td>{card.type}</td>
                        <td>
                          {card.power || card.toughness ? `${card.power}/${card.toughness}` : ""}
                        </td>
                        <td>{card.rarity}</td>
                        <td>{card.number}</td>
                        <td>{card.colors.join("")}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                {shown.length === 0 && <p>No cards</p>}
              </section>
            </>
          ) : (
            <section className="preview-col set-pane" aria-label="Set info">
              <div className="fields">
                <TextField
                  label="Set name"
                  value={setFields.name}
                  saved={setSavedFields.name}
                  wide
                  onChange={(value) => updateSet("name", value)}
                  onBlur={() => void flushSet()}
                />
                <TextField
                  label="Set code"
                  value={setFields.code}
                  saved={setSavedFields.code}
                  onChange={(value) => updateSet("code", value)}
                  onBlur={() => void flushSet()}
                />
                <TextField
                  label="Design brief"
                  value={setFields.designBrief}
                  saved={setSavedFields.designBrief}
                  wide
                  multiline
                  onChange={(value) => updateSet("designBrief", value)}
                  onBlur={() => void flushSet()}
                />
              </div>
            </section>
          )}
          <section className="terminal" aria-label="Terminal">
            <TerminalPane setPath={setPath} token={token} base={base} />
          </section>
        </div>
      )}
      {dialog === "open" && (
        <Dialog
          title="Open set"
          onClose={() => setDialog(null)}
          onSubmit={(form) => {
            const path = fieldValue(form, "path");
            setOpenPath(path);
            void client.current
              .open(path)
              .then(loadWorkspace)
              .catch((err) => show(messageOf(err), "error"));
          }}
          submitLabel="Open"
        >
          <Field label="Set folder" name="path" value={openPath} onChange={setOpenPath} pick="folder" />
        </Dialog>
      )}
      {dialog === "new" && (
        <Dialog
          title="New set"
          onClose={() => setDialog(null)}
          onSubmit={(form) => {
            const path = fieldValue(form, "path");
            const name = fieldValue(form, "name");
            const code = fieldValue(form, "code");
            setNewPath(path);
            setNewName(name);
            setNewCode(code);
            void client.current
              .init(path, name, code)
              .then(loadWorkspace)
              .catch((err) => show(messageOf(err), "error"));
          }}
          submitLabel="Create set"
        >
          <Field label="New set folder" name="path" value={newPath} onChange={setNewPath} pick="folder" />
          <Field label="Set name" name="name" value={newName} onChange={setNewName} />
          <Field label="Set code" name="code" value={newCode} onChange={setNewCode} />
        </Dialog>
      )}
      {dialog === "import" && (
        <Dialog
          title="Import MSE set"
          onClose={() => setDialog(null)}
          onSubmit={(form) => {
            const source = fieldValue(form, "source");
            const dest = fieldValue(form, "dest");
            setImportSource(source);
            setImportDest(dest);
            void client.current
              .importSet(source, dest)
              .then((result) => loadWorkspace(result.setPath))
              .catch((err) => show(messageOf(err), "error"));
          }}
          submitLabel="Import"
        >
          <Field label="MSE package" name="source" value={importSource} onChange={setImportSource} pick="file" />
          <Field label="Import destination" name="dest" value={importDest} onChange={setImportDest} pick="folder" />
        </Dialog>
      )}
      {dialog === "export" && (
        <Dialog
          title="Export"
          onClose={() => setDialog(null)}
          onSubmit={(form) => {
            const path = fieldValue(form, "path");
            setExportPath(path);
            void client.current
              .exportSet(setPathRef.current ?? "", path)
              .then((result) => {
                show(`Exported ${result.path}`);
                setDialog(null);
              })
              .catch((err) => show(messageOf(err), "error"));
          }}
          submitLabel="Export set"
        >
          <Field label="Export path" name="path" value={exportPath} onChange={setExportPath} pick="save" />
        </Dialog>
      )}
      {dialog === "art" && (
        <Dialog
          title="Set illustration"
          onClose={() => setDialog(null)}
          onSubmit={(form) => {
            const path = fieldValue(form, "path");
            setArtPath(path);
            void useArt(path).catch((err) => show(messageOf(err), "error"));
          }}
          submitLabel="Use illustration"
        >
          <Field label="Illustration" name="path" value={artPath} onChange={setArtPath} pick="file" />
        </Dialog>
      )}
      {dialog === "help" && (
        <div className="scrim" onMouseDown={() => setDialog(null)}>
          <div className="dialog" role="dialog" aria-label="About" onMouseDown={(event) => event.stopPropagation()}>
            <h2>About</h2>
            <p>Boggled Set Studio edits a Magic set folder on this computer.</p>
            <button type="button" onClick={() => setDialog(null)}>
              Close
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function Menu({
  label,
  open,
  onToggle,
  children,
}: {
  label: string;
  open: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <div className="menu">
      <button type="button" aria-expanded={open} aria-haspopup="menu" onClick={onToggle}>
        {label}
      </button>
      {open && (
        <div role="menu" aria-label={label}>
          {children}
        </div>
      )}
    </div>
  );
}

function MenuItem({ label, onClick, disabled }: { label: string; onClick: () => void; disabled?: boolean }) {
  return (
    <button type="button" role="menuitem" disabled={disabled} onClick={onClick}>
      {label}
    </button>
  );
}

function Dialog({
  title,
  submitLabel,
  onClose,
  onSubmit,
  children,
}: {
  title: string;
  submitLabel: string;
  onClose: () => void;
  onSubmit: (form: HTMLFormElement) => void;
  children: ReactNode;
}) {
  return (
    <div className="scrim" onMouseDown={onClose}>
      <form
        className="dialog"
        role="dialog"
        aria-label={title}
        onMouseDown={(event) => event.stopPropagation()}
        onSubmit={(event) => {
          event.preventDefault();
          onSubmit(event.currentTarget);
        }}
      >
        <h2>{title}</h2>
        {children}
        <div className="dialog-actions">
          <button type="button" onClick={onClose}>
            Cancel
          </button>
          <button type="submit">{submitLabel}</button>
        </div>
      </form>
    </div>
  );
}

function fieldValue(form: HTMLFormElement, name: string): string {
  const element = form.elements.namedItem(name);
  if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement) return element.value;
  return "";
}

function Field({
  label,
  value,
  onChange,
  id,
  pick,
  name,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  id?: string;
  pick?: "folder" | "file" | "save";
  name?: string;
}) {
  const shell = new URLSearchParams(location.search).get("shell") === "1";
  const command = pick === "folder" ? "pick_folder" : pick === "file" ? "pick_file" : "save_file";
  return (
    <label className="field">
      <span>{label}</span>
      <span className="path-row">
        <input
          id={id}
          name={name}
          aria-label={label}
          value={value}
          autoComplete="off"
          onChange={(event) => onChange(event.target.value)}
        />
        {shell && pick && (
          <button
            type="button"
            onClick={() => {
              void nativePick(command).then((path) => {
                if (path) onChange(path);
              });
            }}
          >
            Browse
          </button>
        )}
      </span>
    </label>
  );
}

function CardBoxes({
  layout,
  draft,
  saved,
  onChange,
  onBlur,
}: {
  layout?: NativeLayout | null;
  draft: Draft;
  saved: Draft;
  onChange: (key: keyof Draft, value: string) => void;
  onBlur: () => void;
}) {
  // The style paints rules/flavor and power/toughness as two composite MSE fields.
  // Give each editable source value a target inside its native field.
  const rects: Record<string, CSSProperties> = {};
  if (layout) {
    const [x, y, w, h] = layout.text;
    const split = layout.flavorTop > 0 && layout.flavorTop < h ? layout.flavorTop : Math.max(0, h - 18);
    const [px, py, pw, ph] = layout.pt;
    const boxes: Record<string, FieldRect> = {
      name: layout.name, cost: layout.casting_cost, type: layout.type,
      rules: [x, y, w, split], flavor: [x, y + split, w, h - split],
      power: [px, py, pw / 2, ph], tough: [px + pw / 2, py, pw / 2, ph],
    };
    for (const [key, [left, top, width, height]] of Object.entries(boxes)) {
      rects[key] = { left: `${left / 375 * 100}%`, top: `${top / 523 * 100}%`,
        width: `${width / 375 * 100}%`, height: `${height / 523 * 100}%` };
    }
  }
  return (
    <>
      <CardInput style={rects.name} className="name" label="Name" value={draft.name} saved={saved.name} onChange={(value) => onChange("name", value)} onBlur={onBlur} />
      <CardInput style={rects.cost} className="cost" label="Mana cost" value={draft.manaCost} saved={saved.manaCost} onChange={(value) => onChange("manaCost", value)} onBlur={onBlur} />
      <TypeLine style={rects.type} value={draft.type} saved={saved.type} onChange={(value) => onChange("type", value)} onBlur={onBlur} />
      <CardInput style={rects.rules} className="rules" label="Rules" value={draft.text} saved={saved.text} multiline onChange={(value) => onChange("text", value)} onBlur={onBlur} />
      <CardInput style={rects.flavor} className="flavor" label="Flavor" value={draft.flavorText} saved={saved.flavorText} multiline onChange={(value) => onChange("flavorText", value)} onBlur={onBlur} />
      <CardInput style={rects.power} className="power" label="Power" value={draft.power} saved={saved.power} onChange={(value) => onChange("power", value)} onBlur={onBlur} />
      <span className="oncard slash" aria-hidden="true">/</span>
      <CardInput style={rects.tough} className="tough" label="Toughness" value={draft.toughness} saved={saved.toughness} onChange={(value) => onChange("toughness", value)} onBlur={onBlur} />
    </>
  );
}

function CardInput({
  style,
  className,
  label,
  value,
  saved,
  multiline,
  onChange,
  onBlur,
}: {
  className: string;
  style?: CSSProperties;
  label: string;
  value: string;
  saved: string;
  multiline?: boolean;
  onChange: (value: string) => void;
  onBlur: () => void;
}) {
  const props = {
    style,
    className: `oncard ${className}`,
    "aria-label": label,
    value,
    "data-saved": saved,
    autoComplete: "off" as const,
    spellCheck: false,
    onChange: (event: { target: { value: string } }) => onChange(event.target.value),
    onBlur,
  };
  return multiline ? <textarea {...props} /> : <input {...props} />;
}

function TypeLine({
  style,
  value,
  saved,
  onChange,
  onBlur,
}: {
  style?: CSSProperties;
  value: string;
  saved: string;
  onChange: (value: string) => void;
  onBlur: () => void;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [caret, setCaret] = useState(0);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const spot = wordAt(value, caret);
  const pool = spot.subtype ? SUBTYPES : [...SUPERTYPES, ...CARD_TYPES];
  const matches = spot.word.length === 0
    ? []
    : pool
        .filter((item) => item.toLowerCase().startsWith(spot.word.toLowerCase()) && item.toLowerCase() !== spot.word.toLowerCase())
        .slice(0, 8);
  const shown = open && matches.length > 0;

  function remember(element: HTMLInputElement) {
    setCaret(element.selectionStart ?? element.value.length);
  }

  function choose(choice: string) {
    const next = value.slice(0, spot.start) + choice + value.slice(spot.end);
    onChange(next);
    const pos = spot.start + choice.length;
    setCaret(pos);
    setOpen(false);
    requestAnimationFrame(() => {
      inputRef.current?.focus();
      inputRef.current?.setSelectionRange(pos, pos);
    });
  }

  return (
    <>
      <input
        ref={inputRef}
        style={style}
        className="oncard type"
        aria-label="Type"
        aria-autocomplete="list"
        aria-expanded={shown}
        value={value}
        data-saved={saved}
        autoComplete="off"
        spellCheck={false}
        onChange={(event) => {
          onChange(event.target.value);
          remember(event.currentTarget);
          setOpen(true);
          setActive(0);
        }}
        onSelect={(event) => remember(event.currentTarget)}
        onKeyUp={(event) => remember(event.currentTarget)}
        onClick={(event) => remember(event.currentTarget)}
        onFocus={(event) => {
          remember(event.currentTarget);
          setOpen(true);
        }}
        onBlur={() => {
          setOpen(false);
          onBlur();
        }}
        onKeyDown={(event) => {
          if (!shown) return;
          if (event.key === "ArrowDown") {
            event.preventDefault();
            setActive((index) => (index + 1) % matches.length);
          } else if (event.key === "ArrowUp") {
            event.preventDefault();
            setActive((index) => (index + matches.length - 1) % matches.length);
          } else if (event.key === "Enter") {
            event.preventDefault();
            choose(matches[Math.min(active, matches.length - 1)] ?? "");
          } else if (event.key === "Escape") {
            setOpen(false);
          }
        }}
      />
      {shown && (
        <ul className="type-suggest" style={style ? { left: style.left, top: `calc(${style.top} + ${style.height})` } : undefined} role="listbox" aria-label="Type suggestions">
          {matches.map((item, index) => (
            <li key={`${item}-${index}`}>
              <button
                type="button"
                role="option"
                aria-selected={index === active}
                onMouseDown={(event) => {
                  event.preventDefault();
                  choose(item);
                }}
              >
                {item}
              </button>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

function wordAt(value: string, caret: number) {
  const index = Math.max(0, Math.min(caret, value.length));
  let start = index;
  while (start > 0 && !isTypeBreak(value[start - 1])) start -= 1;
  let end = index;
  while (end < value.length && !isTypeBreak(value[end])) end += 1;
  const before = value.slice(0, start);
  const subtype = before.includes("\u2014") || before.includes(" - ") || /(?:^|\s)-\s*$/.test(before);
  return { start, end, word: value.slice(start, end), subtype };
}

function isTypeBreak(ch: string) {
  return ch === " " || ch === "\n" || ch === "\u2014" || ch === "-";
}

function TextField({
  label,
  value,
  saved,
  onChange,
  onBlur,
  disabled,
  wide,
  multiline,
}: {
  label: string;
  value: string;
  saved: string;
  onChange: (value: string) => void;
  onBlur: () => void;
  disabled?: boolean;
  wide?: boolean;
  multiline?: boolean;
}) {
  return (
    <label className={wide ? "field wide" : "field"}>
      <span>{label}</span>
      {multiline ? (
        <textarea
          aria-label={label}
          value={value}
          data-saved={saved}
          disabled={disabled}
          rows={3}
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => onChange(event.target.value)}
          onBlur={onBlur}
        />
      ) : (
        <input
          aria-label={label}
          value={value}
          data-saved={saved}
          disabled={disabled}
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => onChange(event.target.value)}
          onBlur={onBlur}
        />
      )}
    </label>
  );
}
