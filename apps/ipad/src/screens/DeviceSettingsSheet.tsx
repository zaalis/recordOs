import { Button, Group, Row, Segmented, Sheet, Switch, useToast } from '@vision-pro/design';
import { useState } from 'react';
import { removeComputer, updatePrefs, useApp } from '../lib/store.ts';
import { resetEverything } from '../lib/storage.ts';

export function DeviceSettingsSheet({ open, onClose }: { open: boolean; onClose: () => void }) {
  const prefs = useApp((s) => s.prefs);
  const computers = useApp((s) => s.computers);
  const [confirmReset, setConfirmReset] = useState(false);
  const toast = useToast();

  const reset = async () => {
    if (!confirmReset) {
      setConfirmReset(true);
      return;
    }
    try {
      await resetEverything(() => toast('Close other Vision-Pro tabs to finish resetting this iPad.', { icon: 'alert', tone: 'orange' }));
      location.reload();
    } catch {
      toast('The reset could not be completed. Please try again.', { icon: 'alert', tone: 'orange' });
    }
  };

  return (
    <Sheet open={open} title="Settings" onClose={onClose} width={560}>
      <Group title="This iPad" footer="The name is shown on Windows when this iPad requests access.">
        <label className="field-stack">
          <span>Device name</span>
          <input
            className="vp-input"
            value={prefs.deviceName}
            maxLength={64}
            autoComplete="off"
            onChange={(event) => updatePrefs({ deviceName: event.currentTarget.value })}
          />
        </label>
        <Row
          icon="eye"
          title="Appearance"
          accessory={
            <Segmented
              ariaLabel="Appearance"
              value={document.documentElement.dataset.theme === 'light' ? 'light' : 'dark'}
              options={[{ value: 'dark', label: 'Dark' }, { value: 'light', label: 'Light' }]}
              onChange={(theme) => {
                document.documentElement.dataset.theme = theme;
                localStorage.setItem('vision-pro-theme', theme);
              }}
            />
          }
        />
      </Group>

      <Group title="Experience">
        <Row icon="sparkle" title="Automatic creative profiles" subtitle="Match controls to the active Windows app" accessory={<Switch label="Automatic creative profiles" checked={prefs.autoProfiles} onChange={(autoProfiles) => updatePrefs({ autoProfiles })} />} />
        <Row icon="stats" title="Performance statistics" accessory={<Switch label="Performance statistics" checked={prefs.showStats} onChange={(showStats) => updatePrefs({ showStats })} />} />
        <Row icon="pointer" title="Show remote cursor" accessory={<Switch label="Show remote cursor" checked={prefs.showLocalCursor} onChange={(showLocalCursor) => updatePrefs({ showLocalCursor })} />} />
      </Group>

      <Group title="Paired computers" footer="Retirer un ordinateur efface son autorisation locale sur cet iPad. Supprimez l’iPad depuis Windows pour couper son accès et effacer sa fiche sur le PC.">
        {computers.length === 0 ? (
          <Row icon="computer" title="No paired computers" />
        ) : (
          computers.map((computer) => (
            <Row
              key={computer.agentId}
              icon="computer"
              title={computer.name}
              subtitle={computer.hostname}
              accessory={
                <Button
                  size="sm"
                  variant="danger"
                  onClick={() => void removeComputer(computer.agentId).then(() => toast('Computer removed from this iPad.', { icon: 'check' }))}
                >
                  Forget
                </Button>
              }
            />
          ))
        )}
      </Group>

      <Group title="Privacy & security" footer="Resetting creates a new device identity. Every PC will require explicit pairing again.">
        <Row icon="lock" title="Private key" subtitle="Non-extractable and stored only on this iPad" value="Protected" />
        <div className="field-stack">
          <Button variant="danger" block onClick={() => void reset()}>
            {confirmReset ? 'Confirm reset' : 'Reset this iPad'}
          </Button>
          {confirmReset && <Button variant="ghost" block onClick={() => setConfirmReset(false)}>Cancel</Button>}
        </div>
      </Group>
    </Sheet>
  );
}
