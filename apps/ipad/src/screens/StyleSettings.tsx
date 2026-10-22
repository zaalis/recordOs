import { Button, Group, Row, Switch } from "@vision-pro/design";
import { DEFAULT_SURFACE_STYLE, type SurfaceStyle } from "../lib/storage.ts";
import { updatePrefs, useApp } from "../lib/store.ts";

export function StyleSettings() {
  const style = useApp((s) => s.prefs.surfaceStyle);
  const patch = (value: Partial<SurfaceStyle>) =>
    updatePrefs({ surfaceStyle: { ...style, ...value } });
  return (
    <Group
      title="Style"
      footer="Ces réglages personnalisent la surface de cette tablette. Ils ne modifient pas votre scène Blender."
    >
      <label className="field-stack">
        <span>Fond de la surface</span>
        <select
          aria-label="Fond de la surface"
          className="vp-input"
          value={style.background}
          onChange={(e) =>
            patch({ background: e.target.value as SurfaceStyle["background"] })
          }
        >
          <option value="graphite">Graphite</option>
          <option value="midnight">Bleu nuit</option>
          <option value="paper">Papier</option>
          <option value="custom">Couleur personnalisée</option>
        </select>
      </label>
      {style.background === "custom" && (
        <label className="style-control">
          Couleur du fond
          <input
            type="color"
            value={style.backgroundColor}
            onChange={(e) => patch({ backgroundColor: e.target.value })}
          />
        </label>
      )}
      <label className="field-stack">
        <span>Repères</span>
        <select
          aria-label="Repères"
          className="vp-input"
          value={style.pattern}
          onChange={(e) =>
            patch({ pattern: e.target.value as SurfaceStyle["pattern"] })
          }
        >
          <option value="dots">Pointillés</option>
          <option value="grid">Quadrillage</option>
          <option value="none">Aucun</option>
        </select>
      </label>
      {style.pattern !== "none" && (
        <>
          <label className="style-control">
            Couleur des repères
            <input
              type="color"
              value={style.patternColor}
              onChange={(e) => patch({ patternColor: e.target.value })}
            />
          </label>
          <label className="field-stack">
            Opacité des repères · {Math.round(style.patternOpacity * 100)} %
            <input
              type="range"
              min="0"
              max="1"
              step="0.01"
              value={style.patternOpacity}
              onChange={(e) =>
                patch({ patternOpacity: Number(e.target.value) })
              }
            />
          </label>
          <label className="field-stack">
            Espacement · {style.patternSpacing} px
            <input
              type="range"
              min="8"
              max="48"
              value={style.patternSpacing}
              onChange={(e) =>
                patch({ patternSpacing: Number(e.target.value) })
              }
            />
          </label>
        </>
      )}
      <Row
        icon="sparkle"
        title="Traînée brumeuse"
        subtitle="Une trace légère qui s’efface après votre passage"
        accessory={
          <Switch
            label="Traînée brumeuse"
            checked={style.trail}
            onChange={(trail) => patch({ trail })}
          />
        }
      />
      {style.trail && (
        <>
          <label className="style-control">
            Couleur de la traînée
            <input
              type="color"
              value={style.trailColor}
              onChange={(e) => patch({ trailColor: e.target.value })}
            />
          </label>
          <label className="field-stack">
            Largeur · {style.trailSize} px
            <input
              type="range"
              min="2"
              max="40"
              value={style.trailSize}
              onChange={(e) => patch({ trailSize: Number(e.target.value) })}
            />
          </label>
          <label className="field-stack">
            Durée · {style.trailDuration} ms
            <input
              type="range"
              min="100"
              max="1500"
              step="50"
              value={style.trailDuration}
              onChange={(e) => patch({ trailDuration: Number(e.target.value) })}
            />
          </label>
        </>
      )}
      <Button
        variant="ghost"
        onClick={() =>
          updatePrefs({ surfaceStyle: { ...DEFAULT_SURFACE_STYLE } })
        }
      >
        Rétablir le style initial
      </Button>
    </Group>
  );
}
