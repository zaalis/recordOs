import { Button, Icon, IconButton } from "@vision-pro/design";
import { relayConnectUrl } from "@vision-pro/protocol";
import { useState } from "react";
import { connection } from "../lib/connection.ts";
import { store, useApp } from "../lib/store.ts";
import { AddComputerSheet } from "./AddComputerSheet.tsx";
import { DeviceSettingsSheet } from "./DeviceSettingsSheet.tsx";

export function Home() {
  const computers = useApp((s) => s.computers).filter((c) => c.relayUrl);
  const [sheet, setSheet] = useState<"add" | "settings" | null>(null);
  const connect = (agentId: string) => {
    const { identity, prefs } = store.get();
    const pc = computers.find((c) => c.agentId === agentId);
    const url = pc?.relayUrl && relayConnectUrl(pc.relayUrl, agentId);
    if (!identity || !pc || !url) return;
    void connection.connect(
      {
        agentId,
        name: pc.name,
        fingerprint: pc.fingerprint,
        mode: "auth",
        url,
      },
      identity,
      prefs.deviceName,
    );
  };
  return (
    <div className="page">
      <div className="ambient" />
      <div className="page-inner">
        <header className="topbar">
          <span className="wordmark">Record OS</span>
          <IconButton
            icon="gear"
            label="Réglages"
            onClick={() => setSheet("settings")}
          />
        </header>
        <section className="hero">
          <h1 className="vp-display hero-title vp-animate-in">
            Votre ordinateur.<span className="dim">Où que vous soyez.</span>
          </h1>
          <p className="hero-subtitle">
            Votre iPad et votre PC, connectés par Internet.
          </p>
        </section>
        {computers.length ? (
          <div className="computers">
            {computers.map((pc) => (
              <article className="computer-card" key={pc.agentId}>
                <div className="computer-card-head">
                  <div className="computer-glyph">
                    <Icon name="computer" size={26} />
                  </div>
                  <div>
                    <div className="computer-name">{pc.name}</div>
                    <div className="computer-meta">Appareil mémorisé</div>
                  </div>
                </div>
                <Button
                  variant="primary"
                  block
                  onClick={() => connect(pc.agentId)}
                >
                  Connecter
                </Button>
              </article>
            ))}
          </div>
        ) : (
          <div className="empty-state">
            <Icon name="computer" size={34} />
            <h3>Connectez votre premier ordinateur</h3>
            <p>
              Sur le PC, ouvrez Record OS et choisissez « Connecter un iPad ».
              Son code suffit pour commencer.
            </p>
          </div>
        )}
        <Button
          variant="primary"
          size="lg"
          block
          onClick={() => setSheet("add")}
        >
          Connecter un ordinateur
        </Button>
        <p className="vp-caption">
          Aucun compte nécessaire. Vous autorisez chaque nouvel iPad sur le PC.
        </p>
      </div>
      <AddComputerSheet open={sheet === "add"} onClose={() => setSheet(null)} />
      <DeviceSettingsSheet
        open={sheet === "settings"}
        onClose={() => setSheet(null)}
      />
    </div>
  );
}
