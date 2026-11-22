import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useEffect, useRef } from "react";
import { socketUrl } from "./api";

type Props = {
  setPath: string;
  token: string;
  base: string;
};

export function TerminalPane({ setPath, token, base }: Props) {
  const host = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const node = host.current;
    if (!node) return;
    const term = new Terminal({
      cursorBlink: true,
      fontFamily: "Consolas, ui-monospace, monospace",
      fontSize: 13,
      screenReaderMode: true,
      theme: {
        background: "#3E2718",
        foreground: "#FFF3DC",
        cursor: "#FBD56A",
        cursorAccent: "#3E2718",
      },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(node);
    fit.fit();
    const cols = Math.max(term.cols, 80);
    const rows = Math.max(term.rows, 24);
    const socket = new WebSocket(socketUrl(base));
    socket.binaryType = "arraybuffer";
    socket.addEventListener("open", () => {
      socket.send(JSON.stringify({ type: "auth", token, set: setPath, cols, rows }));
    });
    socket.addEventListener("message", (event) => {
      if (typeof event.data === "string") return;
      term.write(new Uint8Array(event.data as ArrayBuffer));
    });
    const disposeData = term.onData((data) => {
      if (socket.readyState === WebSocket.OPEN) socket.send(new TextEncoder().encode(data));
    });
    const resize = () => {
      fit.fit();
      if (socket.readyState !== WebSocket.OPEN) return;
      socket.send(JSON.stringify({ type: "resize", cols: term.cols, rows: term.rows }));
    };
    const observer = new ResizeObserver(resize);
    observer.observe(node);
    return () => {
      disposeData.dispose();
      observer.disconnect();
      if (socket.readyState === WebSocket.OPEN) {
        socket.send(JSON.stringify({ type: "close" }));
      }
      socket.close();
      term.dispose();
    };
  }, [setPath, token, base]);

  return <div className="terminal-host" ref={host} />;
}
