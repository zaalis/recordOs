import { Sheet } from "@vision-pro/design";
import { relayConnectUrl } from "@vision-pro/protocol";
import { useState, useRef, useEffect } from "react";
import { connection } from "../lib/connection.ts";
import { store } from "../lib/store.ts";
import { CodeEntry } from "./CodeEntry.tsx";
export function AddComputerSheet({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const generation = useRef(0);
  useEffect(() => {
    generation.current++;
    setBusy(false);
    return () => {
      generation.current++;
    };
  }, [open]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const pair = async (code: string) => {
    if (busy) return;
    setBusy(true);
    setError("");
    const epoch = generation.current;
    try {
      const res = await fetch("/v1/pair", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({code}), cache: "no-store" });
      if (!res.ok)
        throw new Error(
          res.status === 429
            ? "Patientez une minute avant de réessayer."
            : "Code introuvable ou expiré. Affichez un nouveau code sur le PC.",
        );
      const data = await res.json();
      if (epoch !== generation.current) return;
      if (
        !/^[A-Za-z0-9_-]{22}$/.test(data.agentId) ||
        !/^[A-Za-z0-9_-]{43}$/.test(data.fingerprint)
      )
        throw new Error("Réponse invalide. Réessayez.");
      const { identity, prefs } = store.get();
      const url = relayConnectUrl(`wss://${location.host}`, data.agentId);
      if (!identity || !url)
        throw new Error("Initialisation en cours. Réessayez.");
      onClose();
      void connection.connect(
        {
          agentId: data.agentId,
          name: "Mon PC",
          fingerprint: data.fingerprint,
          url,
          mode: "pair",
          proof: { kind: "code", secret: code },
        },
        identity,
        prefs.deviceName,
      );
    } catch (e) {
      if (epoch !== generation.current) return;
      setError(
        e instanceof Error ? e.message : "Connexion indisponible. Réessayez.",
      );
    } finally {
      if (epoch === generation.current) setBusy(false);
    }
  };
  return (
    <Sheet
      open={open}
      title="Connecter un ordinateur"
      onClose={onClose}
      width={480}
    >
      <p className="vp-caption">
        Sur le PC, ouvrez Record OS et choisissez « Connecter un iPad ». Entrez
        son code, puis autorisez la connexion sur le PC.
      </p>
      <CodeEntry
        disabled={busy}
        onSubmit={(code) => void pair(code)}
        autoFocus
      />
      <p role="status" aria-live="polite">
        {busy ? "Recherche de votre ordinateur…" : error}
      </p>
      <p className="vp-caption">
        Connexion par Internet. Aucun compte nécessaire.
      </p>
    </Sheet>
  );
}
